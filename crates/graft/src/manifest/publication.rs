//! Atomic system activation publication for immutable discovery generations.

use std::{
    fs::File,
    io,
    os::unix::fs::MetadataExt as _,
    path::{Component, Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use rustix::{
    fs::{AtFlags, FileType, FlockOperation, Mode, OFlags},
    process::{getegid, geteuid, Gid, Uid},
};
use thiserror::Error;

use super::{
    filesystem::{
        open_directory_child, open_directory_path, open_nofollow, validate_absolute_normal_path,
        validate_base_directory_acl, validate_no_acl,
    },
    ManifestError, ManifestLoader, ProducerIdentity, UserDirectoryPolicy,
};

const SYSTEM_PARENT: &str = "/etc/graft";
const SYSTEM_RUNTIME_ROOT: &str = "/run/graft";
const SYSTEM_RUNTIME: &str = "/run/graft/system";
const STORE_ROOT: &str = "/nix/store";
const CURRENT: &str = "current";
const ACTIVATION_LOCK: &str = "activation.lock";
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_RETRY: Duration = Duration::from_millis(25);
const RELAXED_BASE_DIRECTORY_WARNING: &str =
    "warning: accepting unsafe user base-directory permissions or access ACL in relaxed base-directory mode";
const RELAXED_GRAFT_DIRECTORY_WARNING: &str =
    "warning: accepting Graft-owned user publication directory mode or ACL in relaxed base-directory mode";
const RELAXED_CREATED_DIRECTORY_WARNING: &str =
    "warning: could not fully normalize a newly created Graft-owned user directory in relaxed base-directory mode";

#[cfg(test)]
thread_local! {
    static RELAXED_WARNINGS: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn warn_relaxed(message: &'static str) {
    eprintln!("{message}");
    #[cfg(test)]
    RELAXED_WARNINGS.with(|warnings| warnings.borrow_mut().push(message));
}

#[cfg(test)]
fn take_relaxed_warnings() -> Vec<&'static str> {
    RELAXED_WARNINGS.with(|warnings| std::mem::take(&mut *warnings.borrow_mut()))
}

/// Publishes one immutable user generation through `$XDG_CONFIG_HOME/graft/current`.
///
/// The config and state homes are fixed paths supplied by Home Manager. The
/// effective UID and primary GID are obtained at activation time, never at
/// build time.
///
/// # Errors
///
/// Returns an error when the paths, lock, incoming generation, or existing
/// publication fail validation.
pub fn publish_user_generation(
    config_home: &Path,
    state_home: &Path,
    generation: &Path,
    installed_producer: ProducerIdentity,
    user_directory_policy: UserDirectoryPolicy,
) -> Result<(), PublicationError> {
    let uid = geteuid().as_raw();
    let gid = getegid().as_raw();
    Publisher::new_user(
        config_home,
        state_home,
        uid,
        gid,
        installed_producer,
        user_directory_policy,
    )?
    .publish(generation)
}

/// Publishes one validated immutable generation through `/etc/graft/current`.
///
/// The process must run as root. All mutable paths and their ownership policy
/// are fixed; only the Nix store generation and the Nix-resolved readership GID
/// are supplied by activation.
///
/// # Errors
///
/// Returns an error before replacement for an invalid incoming or current
/// generation, unsafe path, lock, ownership, mode, ACL, or lock timeout. Any
/// error after replacement attempts and validates restoration before the lock is
/// released.
pub fn publish_system_generation(
    generation: &Path,
    graft_gid: u32,
    installed_producer: ProducerIdentity,
) -> Result<(), PublicationError> {
    if !geteuid().is_root() || !getegid().is_root() {
        return Err(PublicationError::RootRequired);
    }
    if graft_gid == u32::MAX {
        return Err(PublicationError::InvalidGroup);
    }

    Publisher::new(
        PublicationPaths {
            parent: PathBuf::from(SYSTEM_PARENT),
            runtime_root: PathBuf::from(SYSTEM_RUNTIME_ROOT),
            runtime: PathBuf::from(SYSTEM_RUNTIME),
            store_root: PathBuf::from(STORE_ROOT),
        },
        0,
        0,
        graft_gid,
        installed_producer,
        LOCK_TIMEOUT,
    )?
    .publish(generation)
}

/// Fail-closed system-generation publication error.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PublicationError {
    /// System publication was not invoked as root.
    #[error("system manifest publication must run as root:root")]
    RootRequired,
    /// The supplied group identifier cannot represent a Unix group.
    #[error("system manifest publication received an invalid graft group ID")]
    InvalidGroup,
    /// A configured internal path is not absolute and normalized.
    #[error("system manifest publication path is not absolute and normalized")]
    UnsafePath,
    /// A publication directory, lock, or pointer violates fixed filesystem policy.
    #[error("system manifest publication filesystem object is unsafe")]
    UnsafeObject,
    /// The exclusive activation lock was not acquired within its fixed deadline.
    #[error("system manifest publication activation lock timed out")]
    LockTimeout,
    /// The immutable generation or current pointer failed existing manifest validation.
    #[error("system manifest publication generation is invalid")]
    Manifest(#[source] ManifestError),
    /// A filesystem operation failed.
    #[error("system manifest publication filesystem operation failed")]
    Filesystem(#[source] io::Error),
    /// Publication failed and the prior pointer could not be restored coherently.
    #[error("system manifest publication rollback failed: {0}")]
    RollbackFailed(String),
}

#[derive(Debug, Clone)]
struct PublicationPaths {
    parent: PathBuf,
    runtime_root: PathBuf,
    runtime: PathBuf,
    store_root: PathBuf,
}

#[derive(Debug, Clone)]
struct Publisher {
    paths: PublicationPaths,
    uid: u32,
    gid: u32,
    graft_gid: u32,
    loader: ManifestLoader,
    lock_timeout: Duration,
    user: bool,
    user_directory_policy: UserDirectoryPolicy,
}

impl Publisher {
    fn new(
        paths: PublicationPaths,
        uid: u32,
        gid: u32,
        graft_gid: u32,
        installed_producer: ProducerIdentity,
        lock_timeout: Duration,
    ) -> Result<Self, PublicationError> {
        for path in [
            &paths.parent,
            &paths.runtime_root,
            &paths.runtime,
            &paths.store_root,
        ] {
            validate_absolute_normal_path(path).map_err(|_| PublicationError::UnsafePath)?;
        }
        if paths.runtime.parent() != Some(paths.runtime_root.as_path())
            || paths.parent.file_name().is_none()
            || paths.runtime_root.file_name().is_none()
            || paths.runtime.file_name().is_none()
            || uid == u32::MAX
            || gid == u32::MAX
            || graft_gid == u32::MAX
        {
            return Err(PublicationError::UnsafePath);
        }
        let loader = ManifestLoader::system_at(
            paths.parent.clone(),
            uid,
            gid,
            graft_gid,
            installed_producer,
        );
        Ok(Self {
            paths,
            uid,
            gid,
            graft_gid,
            loader,
            lock_timeout,
            user: false,
            user_directory_policy: UserDirectoryPolicy::Strict,
        })
    }

    fn new_user(
        config_home: &Path,
        state_home: &Path,
        uid: u32,
        gid: u32,
        installed_producer: ProducerIdentity,
        user_directory_policy: UserDirectoryPolicy,
    ) -> Result<Self, PublicationError> {
        let parent = config_home.join("graft");
        let runtime = state_home.join("graft");
        let paths = PublicationPaths {
            parent,
            runtime_root: state_home.to_path_buf(),
            runtime,
            store_root: PathBuf::from(STORE_ROOT),
        };
        for path in [
            &paths.parent,
            &paths.runtime_root,
            &paths.runtime,
            &paths.store_root,
        ] {
            validate_absolute_normal_path(path).map_err(|_| PublicationError::UnsafePath)?;
        }
        if uid == u32::MAX || gid == u32::MAX {
            return Err(PublicationError::UnsafePath);
        }
        let loader = ManifestLoader::user_with_directory_policy(
            config_home,
            uid,
            gid,
            installed_producer,
            user_directory_policy,
        )
        .map_err(PublicationError::Manifest)?;
        Ok(Self {
            paths,
            uid,
            gid,
            graft_gid: gid,
            loader,
            lock_timeout: LOCK_TIMEOUT,
            user: true,
            user_directory_policy,
        })
    }

    fn publish(&self, generation: &Path) -> Result<(), PublicationError> {
        self.publish_with_hook(generation, || Ok(()))
    }

    fn publish_with_hook<F>(
        &self,
        generation: &Path,
        after_replacement: F,
    ) -> Result<(), PublicationError>
    where
        F: FnOnce() -> Result<(), PublicationError>,
    {
        validate_absolute_normal_path(generation).map_err(PublicationError::Manifest)?;
        let (parent, runtime) = if self.user {
            let config_home = self
                .paths
                .parent
                .parent()
                .ok_or(PublicationError::UnsafePath)?;
            let config_home = ensure_user_base_directory(
                config_home,
                self.uid,
                self.gid,
                self.user_directory_policy == UserDirectoryPolicy::RelaxedBaseDirectories,
            )?;
            let parent = ensure_graft_directory_child(
                &config_home,
                self.paths
                    .parent
                    .file_name()
                    .ok_or(PublicationError::UnsafePath)?,
                self.uid,
                self.gid,
                0o700,
                self.user_directory_policy,
            )?;
            let runtime_root = ensure_user_base_directory(
                &self.paths.runtime_root,
                self.uid,
                self.gid,
                self.user_directory_policy == UserDirectoryPolicy::RelaxedBaseDirectories,
            )?;
            let runtime = ensure_graft_directory_child(
                &runtime_root,
                self.paths
                    .runtime
                    .file_name()
                    .ok_or(PublicationError::UnsafePath)?,
                self.uid,
                self.gid,
                0o700,
                self.user_directory_policy,
            )?;
            (parent, runtime)
        } else {
            let parent =
                ensure_strict_owned_directory(&self.paths.parent, self.uid, self.graft_gid, 0o750)?;
            let runtime_root =
                ensure_strict_owned_directory(&self.paths.runtime_root, self.uid, self.gid, 0o755)?;
            let runtime = ensure_strict_owned_directory_child(
                &runtime_root,
                self.paths
                    .runtime
                    .file_name()
                    .ok_or(PublicationError::UnsafePath)?,
                self.uid,
                self.graft_gid,
                0o750,
            )?;
            (parent, runtime)
        };
        let lock = open_activation_lock(&runtime, self.uid, self.gid)?;
        acquire_exclusive(&lock, self.lock_timeout)?;
        validate_lock(&lock, self.uid, self.gid)?;
        validate_lock_binding(&runtime, &lock)?;

        let incoming = self
            .loader
            .load_generation_with_store_root(generation, &self.paths.store_root)
            .map_err(PublicationError::Manifest)?;
        // The current pointer can legitimately belong to the package generation
        // being upgraded or rolled back. Preserve it only after full pair and
        // context validation, without requiring the incoming producer identity.
        let previous = match self
            .loader
            .load_coherent_with_store_root(&self.paths.store_root)
        {
            Ok(snapshot) => Some(snapshot.generation_path().to_path_buf()),
            Err(ManifestError::MissingCurrent) => None,
            Err(error) => return Err(PublicationError::Manifest(error)),
        };
        if previous.as_deref() == Some(incoming.generation_path()) {
            return Ok(());
        }

        replace_pointer(&parent, incoming.generation_path(), self.uid, self.gid)?;
        let publication_result = (|| {
            rustix::fs::fsync(&parent).map_err(errno_to_publication)?;
            after_replacement()?;
            let published = self
                .loader
                .load_with_store_root(&self.paths.store_root)
                .map_err(PublicationError::Manifest)?;
            if published.generation_path() != incoming.generation_path() {
                return Err(PublicationError::UnsafeObject);
            }
            Ok(())
        })();

        if let Err(error) = publication_result {
            if let Err(rollback) = self.restore(&parent, previous.as_deref()) {
                return Err(PublicationError::RollbackFailed(rollback.to_string()));
            }
            return Err(error);
        }
        Ok(())
    }

    fn restore(&self, parent: &File, previous: Option<&Path>) -> Result<(), PublicationError> {
        match previous {
            Some(target) => replace_pointer(parent, target, self.uid, self.gid)?,
            None => rustix::fs::unlinkat(parent, CURRENT, AtFlags::empty())
                .map_err(errno_to_publication)?,
        }
        rustix::fs::fsync(parent).map_err(errno_to_publication)?;
        match (
            previous,
            self.loader
                .load_coherent_with_store_root(&self.paths.store_root),
        ) {
            (Some(expected), Ok(snapshot)) if snapshot.generation_path() == expected => Ok(()),
            (None, Err(ManifestError::MissingCurrent)) => Ok(()),
            (_, Err(error)) => Err(PublicationError::Manifest(error)),
            _ => Err(PublicationError::UnsafeObject),
        }
    }
}

fn open_or_create_directory_child(
    parent: &File,
    name: &std::ffi::OsStr,
    mode: u32,
) -> Result<(File, bool), PublicationError> {
    match rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(mode)) {
        Ok(()) => open_directory_child(parent, name)
            .map(|directory| (directory, true))
            .map_err(PublicationError::Manifest),
        Err(error) if error == rustix::io::Errno::EXIST => open_directory_child(parent, name)
            .map(|directory| (directory, false))
            .map_err(PublicationError::Manifest),
        Err(error) => Err(errno_to_publication(error)),
    }
}

fn initialize_created_directory(
    directory: &File,
    parent: &File,
    uid: u32,
    gid: u32,
    mode: u32,
) -> Result<(), PublicationError> {
    rustix::fs::fchown(
        directory,
        Some(Uid::from_raw(uid)),
        Some(Gid::from_raw(gid)),
    )
    .map_err(errno_to_publication)?;
    rustix::fs::fchmod(directory, Mode::from_raw_mode(mode)).map_err(errno_to_publication)?;
    rustix::fs::fsync(parent).map_err(errno_to_publication)
}

fn normalize_created_graft_directory(
    directory: &File,
    parent: &File,
    uid: u32,
    gid: u32,
    mode: u32,
    policy: UserDirectoryPolicy,
) -> Result<(), PublicationError> {
    let relaxed = policy == UserDirectoryPolicy::RelaxedBaseDirectories;
    for result in [
        rustix::fs::fchown(
            directory,
            Some(Uid::from_raw(uid)),
            Some(Gid::from_raw(gid)),
        ),
        rustix::fs::fremovexattr(directory, "system.posix_acl_access"),
        rustix::fs::fremovexattr(directory, "system.posix_acl_default"),
        rustix::fs::fchmod(directory, Mode::from_raw_mode(mode)),
    ] {
        match result {
            Ok(()) | Err(rustix::io::Errno::NODATA) => {}
            Err(_) if relaxed => warn_relaxed(RELAXED_CREATED_DIRECTORY_WARNING),
            Err(error) => return Err(errno_to_publication(error)),
        }
    }
    validate_directory(directory, uid, gid, mode, relaxed)?;
    rustix::fs::fsync(parent).map_err(errno_to_publication)
}

fn ensure_user_base_directory(
    path: &Path,
    uid: u32,
    gid: u32,
    relaxed: bool,
) -> Result<File, PublicationError> {
    validate_absolute_normal_path(path).map_err(|_| PublicationError::UnsafePath)?;
    let mut directory = open_nofollow(Path::new("/"), true).map_err(PublicationError::Manifest)?;
    let mut user_path = false;
    for component in path.components() {
        if let Component::Normal(name) = component {
            let (child, created) = open_or_create_directory_child(&directory, name, 0o700)?;
            if created {
                initialize_created_directory(&child, &directory, uid, gid, 0o700)?;
            }
            let metadata = child.metadata().map_err(PublicationError::Filesystem)?;
            user_path |= created || metadata.uid() == uid;
            if user_path {
                validate_base_directory(&child, uid, gid, relaxed)?;
            }
            directory = child;
        }
    }
    validate_base_directory(&directory, uid, gid, relaxed)?;
    Ok(directory)
}

fn ensure_strict_owned_directory(
    path: &Path,
    uid: u32,
    gid: u32,
    mode: u32,
) -> Result<File, PublicationError> {
    validate_absolute_normal_path(path).map_err(|_| PublicationError::UnsafePath)?;
    let parent = open_directory_path(path.parent().ok_or(PublicationError::UnsafePath)?)
        .map_err(PublicationError::Manifest)?;
    ensure_strict_owned_directory_child(
        &parent,
        path.file_name().ok_or(PublicationError::UnsafePath)?,
        uid,
        gid,
        mode,
    )
}

fn ensure_strict_owned_directory_child(
    parent: &File,
    name: &std::ffi::OsStr,
    uid: u32,
    gid: u32,
    mode: u32,
) -> Result<File, PublicationError> {
    let (directory, created) = open_or_create_directory_child(parent, name, mode)?;
    if created {
        initialize_created_directory(&directory, parent, uid, gid, mode)?;
    }
    validate_directory(&directory, uid, gid, mode, false)?;
    Ok(directory)
}

fn ensure_graft_directory_child(
    parent: &File,
    name: &std::ffi::OsStr,
    uid: u32,
    gid: u32,
    mode: u32,
    policy: UserDirectoryPolicy,
) -> Result<File, PublicationError> {
    let (directory, created) = open_or_create_directory_child(parent, name, mode)?;
    if created {
        normalize_created_graft_directory(&directory, parent, uid, gid, mode, policy)?;
    } else {
        validate_directory(
            &directory,
            uid,
            gid,
            mode,
            policy == UserDirectoryPolicy::RelaxedBaseDirectories,
        )?;
    }
    Ok(directory)
}

fn validate_base_directory(
    directory: &File,
    uid: u32,
    gid: u32,
    relaxed: bool,
) -> Result<(), PublicationError> {
    let metadata = directory.metadata().map_err(PublicationError::Filesystem)?;
    if !metadata.is_dir() || (metadata.uid() != uid && metadata.uid() != 0) {
        return Err(PublicationError::UnsafeObject);
    }
    let unsafe_mode =
        metadata.mode() & 0o0002 != 0 || (metadata.mode() & 0o0020 != 0 && metadata.gid() != gid);
    let unsafe_acl = validate_base_directory_acl(directory, uid).is_err();
    if unsafe_mode || unsafe_acl {
        if relaxed {
            warn_relaxed(RELAXED_BASE_DIRECTORY_WARNING);
        } else {
            return Err(PublicationError::UnsafeObject);
        }
    }
    Ok(())
}

fn validate_directory(
    directory: &File,
    uid: u32,
    gid: u32,
    mode: u32,
    relaxed: bool,
) -> Result<(), PublicationError> {
    let metadata = directory.metadata().map_err(PublicationError::Filesystem)?;
    if !metadata.is_dir() || (metadata.uid(), metadata.gid()) != (uid, gid) {
        return Err(PublicationError::UnsafeObject);
    }
    let strict_error = metadata.mode() & 0o7777 != mode || validate_no_acl(directory).is_err();
    if strict_error {
        if relaxed {
            warn_relaxed(RELAXED_GRAFT_DIRECTORY_WARNING);
        } else {
            return Err(PublicationError::UnsafeObject);
        }
    }
    Ok(())
}

fn open_activation_lock(runtime: &File, uid: u32, gid: u32) -> Result<File, PublicationError> {
    let create_flags =
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::CREATE | OFlags::EXCL;
    let (descriptor, created) = match rustix::fs::openat(
        runtime,
        ACTIVATION_LOCK,
        create_flags,
        Mode::from_raw_mode(0o600),
    ) {
        Ok(descriptor) => (descriptor, true),
        Err(error) if error == rustix::io::Errno::EXIST => (
            rustix::fs::openat(
                runtime,
                ACTIVATION_LOCK,
                OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(errno_to_publication)?,
            false,
        ),
        Err(error) => return Err(errno_to_publication(error)),
    };
    let lock = File::from(descriptor);
    if created {
        rustix::fs::fchown(&lock, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid)))
            .map_err(errno_to_publication)?;
        rustix::fs::fchmod(&lock, Mode::from_raw_mode(0o600)).map_err(errno_to_publication)?;
        rustix::fs::fsync(&lock).map_err(errno_to_publication)?;
        rustix::fs::fsync(runtime).map_err(errno_to_publication)?;
    }
    validate_lock(&lock, uid, gid)?;
    Ok(lock)
}

fn validate_lock(lock: &File, uid: u32, gid: u32) -> Result<(), PublicationError> {
    let metadata = lock.metadata().map_err(PublicationError::Filesystem)?;
    if !metadata.is_file()
        || (metadata.uid(), metadata.gid()) != (uid, gid)
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(PublicationError::UnsafeObject);
    }
    validate_no_acl(lock).map_err(PublicationError::Manifest)
}

fn acquire_exclusive(lock: &File, timeout: Duration) -> Result<(), PublicationError> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(PublicationError::LockTimeout)?;
    loop {
        match rustix::fs::flock(lock, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => return Ok(()),
            Err(error) if error == rustix::io::Errno::INTR => {
                if Instant::now() >= deadline {
                    return Err(PublicationError::LockTimeout);
                }
            }
            Err(error) if error == rustix::io::Errno::AGAIN => {
                if Instant::now() >= deadline {
                    return Err(PublicationError::LockTimeout);
                }
                thread::sleep(LOCK_RETRY.min(deadline.saturating_duration_since(Instant::now())));
            }
            Err(error) => return Err(errno_to_publication(error)),
        }
    }
}

fn validate_lock_binding(runtime: &File, lock: &File) -> Result<(), PublicationError> {
    let opened = lock.metadata().map_err(PublicationError::Filesystem)?;
    let installed = rustix::fs::statat(runtime, ACTIVATION_LOCK, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(errno_to_publication)?;
    if FileType::from_raw_mode(installed.st_mode) != FileType::RegularFile
        || (opened.dev(), opened.ino()) != (installed.st_dev, installed.st_ino)
    {
        return Err(PublicationError::UnsafeObject);
    }
    Ok(())
}

fn replace_pointer(
    parent: &File,
    target: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), PublicationError> {
    let temporary = format!(".current.{}.tmp", uuid::Uuid::now_v7());
    rustix::fs::symlinkat(target, parent, temporary.as_str()).map_err(errno_to_publication)?;
    let result = (|| {
        let pointer = rustix::fs::statat(parent, temporary.as_str(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(errno_to_publication)?;
        if FileType::from_raw_mode(pointer.st_mode) != FileType::Symlink
            || (pointer.st_uid, pointer.st_gid) != (uid, gid)
        {
            return Err(PublicationError::UnsafeObject);
        }
        rustix::fs::renameat(parent, temporary.as_str(), parent, CURRENT)
            .map_err(errno_to_publication)
    })();
    if result.is_err() {
        let _ = rustix::fs::unlinkat(parent, temporary.as_str(), AtFlags::empty());
    }
    result
}

fn errno_to_publication(error: rustix::io::Errno) -> PublicationError {
    PublicationError::Filesystem(io::Error::from_raw_os_error(error.raw_os_error()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        os::unix::fs::{symlink, PermissionsExt as _},
        sync::{Arc, Barrier},
    };

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::manifest::{render, RenderInput};

    const HOST_ONE: &str = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
    const HOST_TWO: &str = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21";

    struct Fixture {
        temporary: TempDir,
        publisher: Publisher,
        parent: PathBuf,
        runtime: PathBuf,
        first: PathBuf,
        second: PathBuf,
    }

    struct UserFixture {
        _temporary: TempDir,
        publisher: Publisher,
        config_home: PathBuf,
        state_home: PathBuf,
        first: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temporary = TempDir::new().unwrap();
            let root = temporary.path();
            let etc = root.join("etc");
            let run = root.join("run");
            let store = root.join("store");
            fs::create_dir(&etc).unwrap();
            fs::create_dir(&run).unwrap();
            fs::create_dir(&store).unwrap();
            let parent = etc.join("graft");
            let runtime_root = run.join("graft");
            let runtime = runtime_root.join("system");
            let first = write_generation(&store, "generation-one", HOST_ONE);
            let second = write_generation(&store, "generation-two", HOST_TWO);
            let uid = geteuid().as_raw();
            let gid = getegid().as_raw();
            let publisher = Publisher::new(
                PublicationPaths {
                    parent: parent.clone(),
                    runtime_root,
                    runtime: runtime.clone(),
                    store_root: store,
                },
                uid,
                gid,
                gid,
                producer(),
                Duration::from_millis(100),
            )
            .unwrap();
            Self {
                temporary,
                publisher,
                parent,
                runtime,
                first,
                second,
            }
        }

        fn current(&self) -> PathBuf {
            fs::read_link(self.parent.join(CURRENT)).unwrap()
        }

        fn assert_current_valid(&self, expected: &Path) {
            let snapshot = self
                .publisher
                .loader
                .load_with_store_root(&self.publisher.paths.store_root)
                .unwrap();
            assert_eq!(snapshot.generation_path(), expected);
            assert_eq!(self.current(), expected);
        }
    }

    impl UserFixture {
        fn new() -> Self {
            let temporary = TempDir::new().unwrap();
            let root = temporary.path();
            let config_home = root.join("config");
            let state_home = root.join("state");
            let store = root.join("store");
            fs::create_dir(&config_home).unwrap();
            fs::create_dir(&store).unwrap();
            let first = write_user_generation(&store, "generation-one", HOST_ONE);
            let uid = geteuid().as_raw();
            let gid = getegid().as_raw();
            let mut publisher = Publisher::new_user(
                &config_home,
                &state_home,
                uid,
                gid,
                producer(),
                UserDirectoryPolicy::Strict,
            )
            .unwrap();
            publisher.paths.store_root = store;
            Self {
                _temporary: temporary,
                publisher,
                config_home,
                state_home,
                first,
            }
        }
    }

    #[test]
    fn relaxed_publication_and_worker_policy_load_the_same_generation() {
        let mut fixture = UserFixture::new();
        fs::create_dir(fixture.config_home.join("graft")).unwrap();
        fs::set_permissions(
            fixture.config_home.join("graft"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fixture.publisher.user_directory_policy = UserDirectoryPolicy::RelaxedBaseDirectories;
        fixture.publisher.loader = ManifestLoader::user_with_directory_policy(
            &fixture.config_home,
            fixture.publisher.uid,
            fixture.publisher.gid,
            producer(),
            UserDirectoryPolicy::RelaxedBaseDirectories,
        )
        .unwrap();

        fixture.publisher.publish(&fixture.first).unwrap();

        let strict = ManifestLoader::user(
            &fixture.config_home,
            fixture.publisher.uid,
            fixture.publisher.gid,
            producer(),
        )
        .unwrap();
        assert!(matches!(
            strict.load_with_store_root(&fixture.publisher.paths.store_root),
            Err(ManifestError::Permissions)
        ));
        let loaded = fixture
            .publisher
            .loader
            .load_with_store_root(&fixture.publisher.paths.store_root)
            .unwrap();
        assert_eq!(loaded.generation_path(), fixture.first);
    }

    #[test]
    fn user_publisher_creates_missing_config_home_before_publication_directory() {
        let fixture = UserFixture::new();
        fs::remove_dir(&fixture.config_home).unwrap();

        fixture.publisher.publish(&fixture.first).unwrap();

        assert_eq!(
            fixture.config_home.metadata().unwrap().mode() & 0o7777,
            0o700
        );
        assert_eq!(
            fs::read_link(fixture.config_home.join("graft/current")).unwrap(),
            fixture.first
        );
    }

    #[test]
    fn user_publisher_creates_missing_state_home_before_runtime_directory() {
        let fixture = UserFixture::new();

        fixture.publisher.publish(&fixture.first).unwrap();

        assert_eq!(
            fixture.state_home.metadata().unwrap().mode() & 0o7777,
            0o700
        );
        assert_eq!(
            fixture.state_home.join("graft").metadata().unwrap().mode() & 0o7777,
            0o700
        );
        assert_eq!(
            fs::read_link(fixture.config_home.join("graft/current")).unwrap(),
            fixture.first
        );
    }

    #[test]
    fn user_publisher_accepts_existing_state_home_without_rewriting_it() {
        let fixture = UserFixture::new();
        fs::create_dir(&fixture.state_home).unwrap();
        fs::set_permissions(&fixture.state_home, fs::Permissions::from_mode(0o755)).unwrap();

        fixture.publisher.publish(&fixture.first).unwrap();

        assert_eq!(
            fixture.state_home.metadata().unwrap().mode() & 0o7777,
            0o755
        );
        assert_eq!(
            fixture.state_home.join("graft").metadata().unwrap().mode() & 0o7777,
            0o700
        );
    }

    #[test]
    fn inherited_default_acls_are_removed_only_from_new_graft_directories() {
        let fixture = UserFixture::new();
        fs::create_dir(&fixture.state_home).unwrap();
        let config_home = File::open(&fixture.config_home).unwrap();
        let state_home = File::open(&fixture.state_home).unwrap();
        if !set_default_acl(&config_home) || !set_default_acl(&state_home) {
            return;
        }
        let config_acl = get_acl(&config_home, "system.posix_acl_default");
        let state_acl = get_acl(&state_home, "system.posix_acl_default");

        fixture.publisher.publish(&fixture.first).unwrap();

        assert_eq!(
            get_acl(
                &File::open(&fixture.config_home).unwrap(),
                "system.posix_acl_default"
            ),
            config_acl
        );
        assert_eq!(
            get_acl(
                &File::open(&fixture.state_home).unwrap(),
                "system.posix_acl_default"
            ),
            state_acl
        );
        for directory in [
            fixture.config_home.join("graft"),
            fixture.state_home.join("graft"),
        ] {
            let directory = File::open(directory).unwrap();
            assert_acl_absent(&directory, "system.posix_acl_access");
            assert_acl_absent(&directory, "system.posix_acl_default");
        }
    }

    #[test]
    fn user_publisher_rejects_unsafe_access_acl_on_existing_state_home() {
        let fixture = UserFixture::new();
        fs::create_dir(&fixture.state_home).unwrap();
        if !set_access_acl(&File::open(&fixture.state_home).unwrap()) {
            return;
        }

        assert!(matches!(
            fixture.publisher.publish(&fixture.first),
            Err(PublicationError::UnsafeObject)
        ));
        assert!(!fixture.state_home.join("graft").exists());
    }

    #[test]
    fn user_publisher_rejects_foreign_owned_existing_state_home() {
        let fixture = UserFixture::new();
        fs::create_dir(&fixture.state_home).unwrap();
        let caller_uid = fixture.publisher.uid;
        let foreign_uid = if caller_uid == 0 {
            rustix::fs::chownat(
                rustix::fs::CWD,
                &fixture.state_home,
                Some(Uid::from_raw(65_534)),
                None,
                AtFlags::empty(),
            )
            .unwrap();
            caller_uid
        } else {
            0
        };

        assert!(matches!(
            ensure_user_base_directory(
                &fixture.state_home,
                foreign_uid,
                fixture.publisher.gid,
                false,
            ),
            Err(PublicationError::UnsafeObject)
        ));
        assert!(!fixture.state_home.join("graft").exists());
    }

    #[test]
    fn user_base_directory_allows_group_write_for_the_effective_group() {
        let fixture = UserFixture::new();
        fs::create_dir(&fixture.state_home).unwrap();
        fs::set_permissions(&fixture.state_home, fs::Permissions::from_mode(0o720)).unwrap();

        validate_base_directory(
            &File::open(&fixture.state_home).unwrap(),
            fixture.publisher.uid,
            fixture.publisher.gid,
            false,
        )
        .unwrap();
    }

    #[test]
    fn user_base_directory_rejects_group_write_for_a_foreign_group() {
        let fixture = UserFixture::new();
        fs::create_dir(&fixture.state_home).unwrap();
        fs::set_permissions(&fixture.state_home, fs::Permissions::from_mode(0o720)).unwrap();

        assert!(matches!(
            validate_base_directory(
                &File::open(&fixture.state_home).unwrap(),
                fixture.publisher.uid,
                fixture.publisher.gid.saturating_add(1),
                false,
            ),
            Err(PublicationError::UnsafeObject)
        ));
    }

    #[test]
    fn relaxed_user_base_directory_still_rejects_foreign_owner() {
        let fixture = UserFixture::new();
        fs::create_dir(&fixture.state_home).unwrap();

        let foreign_uid = u32::from(fixture.publisher.uid == 0);
        assert!(matches!(
            validate_base_directory(
                &File::open(&fixture.state_home).unwrap(),
                foreign_uid,
                fixture.publisher.gid,
                true,
            ),
            Err(PublicationError::UnsafeObject)
        ));
    }

    #[test]
    fn user_publisher_rejects_world_writable_existing_state_home() {
        let fixture = UserFixture::new();
        fs::create_dir(&fixture.state_home).unwrap();
        fs::set_permissions(&fixture.state_home, fs::Permissions::from_mode(0o702)).unwrap();

        assert!(matches!(
            fixture.publisher.publish(&fixture.first),
            Err(PublicationError::UnsafeObject)
        ));
        assert!(!fixture.state_home.join("graft").exists());
    }

    #[test]
    fn relaxed_user_publisher_warns_and_accepts_an_unsafe_state_home() {
        let mut fixture = UserFixture::new();
        let _ = take_relaxed_warnings();
        fixture.publisher.user_directory_policy = UserDirectoryPolicy::RelaxedBaseDirectories;
        fixture.publisher.loader = ManifestLoader::user_with_directory_policy(
            &fixture.config_home,
            fixture.publisher.uid,
            fixture.publisher.gid,
            producer(),
            UserDirectoryPolicy::RelaxedBaseDirectories,
        )
        .unwrap();
        fs::create_dir(&fixture.state_home).unwrap();
        fs::set_permissions(&fixture.state_home, fs::Permissions::from_mode(0o702)).unwrap();

        fixture.publisher.publish(&fixture.first).unwrap();

        assert!(fixture.state_home.join("graft").is_dir());
        assert!(take_relaxed_warnings().contains(&RELAXED_BASE_DIRECTORY_WARNING));
    }

    #[test]
    fn relaxed_user_publisher_warns_and_accepts_an_unsafe_graft_directory() {
        let mut fixture = UserFixture::new();
        fixture.publisher.publish(&fixture.first).unwrap();
        fs::set_permissions(
            fixture.config_home.join("graft"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let mut publisher = Publisher::new_user(
            &fixture.config_home,
            &fixture.state_home,
            fixture.publisher.uid,
            fixture.publisher.gid,
            producer(),
            UserDirectoryPolicy::RelaxedBaseDirectories,
        )
        .unwrap();
        publisher.paths.store_root = fixture.publisher.paths.store_root.clone();
        fixture.publisher = publisher;
        let _ = take_relaxed_warnings();

        fixture.publisher.publish(&fixture.first).unwrap();

        assert_eq!(
            fs::read_link(fixture.config_home.join("graft/current")).unwrap(),
            fixture.first
        );
        assert!(take_relaxed_warnings().contains(&RELAXED_GRAFT_DIRECTORY_WARNING));
    }

    #[test]
    fn user_publisher_creates_missing_state_home_parents() {
        let mut fixture = UserFixture::new();
        fixture.state_home = fixture
            .config_home
            .join("missing")
            .join("deeper")
            .join("state");
        fixture.publisher.paths.runtime_root = fixture.state_home.clone();
        fixture.publisher.paths.runtime = fixture.state_home.join("graft");

        fixture.publisher.publish(&fixture.first).unwrap();

        for directory in [
            fixture.config_home.join("missing"),
            fixture.config_home.join("missing/deeper"),
            fixture.state_home.clone(),
        ] {
            let metadata = directory.metadata().unwrap();
            assert_eq!(
                (metadata.uid(), metadata.gid()),
                (fixture.publisher.uid, fixture.publisher.gid)
            );
            assert_eq!(metadata.mode() & 0o7777, 0o700);
        }
        assert_eq!(
            fixture.state_home.join("graft").metadata().unwrap().mode() & 0o7777,
            0o700
        );
    }

    #[test]
    fn user_publisher_rejects_unsafe_existing_intermediate_state_directory() {
        let mut fixture = UserFixture::new();
        let intermediate = fixture.config_home.join("state-parent");
        fs::create_dir(&intermediate).unwrap();
        fs::set_permissions(&intermediate, fs::Permissions::from_mode(0o702)).unwrap();
        fixture.state_home = intermediate.join("state");
        fixture.publisher.paths.runtime_root = fixture.state_home.clone();
        fixture.publisher.paths.runtime = fixture.state_home.join("graft");

        assert!(matches!(
            fixture.publisher.publish(&fixture.first),
            Err(PublicationError::UnsafeObject)
        ));
        assert!(!fixture.state_home.exists());
    }

    #[test]
    fn system_publisher_rejects_missing_runtime_parent_without_creating_it() {
        let mut fixture = Fixture::new();
        let runtime_root = fixture.temporary.path().join("run/missing/graft");
        fixture.publisher.paths.runtime_root = runtime_root.clone();
        fixture.publisher.paths.runtime = runtime_root.join("system");

        let result = fixture.publisher.publish(&fixture.first);

        assert!(matches!(
            result,
            Err(PublicationError::Manifest(ManifestError::Filesystem(error)))
                if error.kind() == io::ErrorKind::NotFound
        ));
        assert!(!runtime_root.parent().unwrap().exists());
        assert!(!runtime_root.exists());
    }

    #[test]
    fn system_publisher_keeps_strict_runtime_directory_validation() {
        let fixture = Fixture::new();
        fs::create_dir_all(&fixture.publisher.paths.runtime_root).unwrap();
        fs::set_permissions(
            &fixture.publisher.paths.runtime_root,
            fs::Permissions::from_mode(0o777),
        )
        .unwrap();

        assert!(matches!(
            fixture.publisher.publish(&fixture.first),
            Err(PublicationError::UnsafeObject)
        ));
    }

    #[test]
    fn publishes_and_repeats_one_coherent_generation() {
        let fixture = Fixture::new();
        fixture.publisher.publish(&fixture.first).unwrap();
        fixture.publisher.publish(&fixture.first).unwrap();
        fixture.assert_current_valid(&fixture.first);
        assert_eq!(fs::read_dir(&fixture.parent).unwrap().count(), 1);
    }

    #[test]
    fn invalid_incoming_generation_leaves_prior_pointer() {
        let fixture = Fixture::new();
        fixture.publisher.publish(&fixture.first).unwrap();
        fs::set_permissions(
            fixture.second.join("endpoint.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        assert!(fixture.publisher.publish(&fixture.second).is_err());
        fixture.assert_current_valid(&fixture.first);
    }

    #[test]
    fn post_replacement_failure_restores_prior_pointer() {
        let fixture = Fixture::new();
        fixture.publisher.publish(&fixture.first).unwrap();
        let result = fixture.publisher.publish_with_hook(&fixture.second, || {
            Err(PublicationError::Filesystem(io::Error::other(
                "injected post-replacement failure",
            )))
        });

        assert!(result.is_err());
        fixture.assert_current_valid(&fixture.first);
    }

    #[test]
    fn failed_first_publication_removes_pointer() {
        let fixture = Fixture::new();
        let result = fixture.publisher.publish_with_hook(&fixture.first, || {
            Err(PublicationError::Filesystem(io::Error::other(
                "injected post-replacement failure",
            )))
        });

        assert!(result.is_err());
        assert!(!fixture.parent.join(CURRENT).exists());
    }

    #[test]
    fn package_upgrade_accepts_and_restores_a_prior_coherent_producer() {
        let fixture = Fixture::new();
        let old_version = "0.2.0";
        let old_build_id = "old-build";
        let old_generation = write_generation_for(
            &fixture.publisher.paths.store_root,
            "generation-old-producer",
            HOST_ONE,
            old_version,
            old_build_id,
        );
        let old_publisher = Publisher::new(
            fixture.publisher.paths.clone(),
            fixture.publisher.uid,
            fixture.publisher.gid,
            fixture.publisher.graft_gid,
            producer_for(old_version, old_build_id),
            fixture.publisher.lock_timeout,
        )
        .unwrap();
        old_publisher.publish(&old_generation).unwrap();

        let failed_upgrade = fixture.publisher.publish_with_hook(&fixture.second, || {
            Err(PublicationError::Filesystem(io::Error::other(
                "injected upgrade failure",
            )))
        });
        assert!(failed_upgrade.is_err());
        assert_eq!(fixture.current(), old_generation);
        assert_eq!(
            old_publisher
                .loader
                .load_with_store_root(&old_publisher.paths.store_root)
                .unwrap()
                .generation_path(),
            old_generation
        );

        fixture.publisher.publish(&fixture.second).unwrap();
        fixture.assert_current_valid(&fixture.second);
    }

    #[test]
    fn exclusive_lock_has_a_bounded_deadline() {
        let fixture = Fixture::new();
        fixture.publisher.publish(&fixture.first).unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(fixture.runtime.join(ACTIVATION_LOCK))
            .unwrap();
        rustix::fs::flock(&lock, FlockOperation::LockExclusive).unwrap();

        assert!(matches!(
            fixture.publisher.publish(&fixture.second),
            Err(PublicationError::LockTimeout)
        ));
        fixture.assert_current_valid(&fixture.first);
    }

    #[test]
    fn concurrent_publishers_never_create_a_mixed_pair() {
        let fixture = Arc::new(Fixture::new());
        fixture.publisher.publish(&fixture.first).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let handles = [&fixture.first, &fixture.second]
            .into_iter()
            .map(|generation| {
                let fixture = Arc::clone(&fixture);
                let generation = generation.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    fixture.publisher.publish(&generation)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let current = fixture.current();
        assert!(current == fixture.first || current == fixture.second);
        fixture.assert_current_valid(&current);
        assert_eq!(fs::read_dir(&fixture.parent).unwrap().count(), 1);
    }

    #[test]
    fn symlinked_and_hardlinked_locks_are_rejected() {
        let fixture = Fixture::new();
        fixture.publisher.publish(&fixture.first).unwrap();
        let lock = fixture.runtime.join(ACTIVATION_LOCK);
        fs::remove_file(&lock).unwrap();
        symlink(&fixture.first, &lock).unwrap();
        assert!(fixture.publisher.publish(&fixture.second).is_err());
        fixture.assert_current_valid(&fixture.first);

        fs::remove_file(&lock).unwrap();
        fs::write(&lock, []).unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
        fs::hard_link(&lock, fixture.runtime.join("activation-linked")).unwrap();
        assert!(matches!(
            fixture.publisher.publish(&fixture.second),
            Err(PublicationError::UnsafeObject)
        ));
        fixture.assert_current_valid(&fixture.first);
    }

    #[test]
    fn malformed_existing_pointer_is_not_overwritten() {
        let fixture = Fixture::new();
        ensure_strict_owned_directory(
            &fixture.parent,
            geteuid().as_raw(),
            getegid().as_raw(),
            0o750,
        )
        .unwrap();
        fs::write(fixture.parent.join(CURRENT), b"not a symlink").unwrap();

        assert!(fixture.publisher.publish(&fixture.first).is_err());
        assert_eq!(
            fs::read(fixture.parent.join(CURRENT)).unwrap(),
            b"not a symlink"
        );
    }

    #[test]
    fn publisher_rejects_relative_internal_paths_and_invalid_ids() {
        let result = Publisher::new(
            PublicationPaths {
                parent: PathBuf::from("relative"),
                runtime_root: PathBuf::from("/run/graft"),
                runtime: PathBuf::from("/run/graft/system"),
                store_root: PathBuf::from("/nix/store"),
            },
            0,
            0,
            0,
            producer(),
            Duration::ZERO,
        );
        assert!(matches!(result, Err(PublicationError::UnsafePath)));
    }

    fn write_generation(store: &Path, name: &str, host_id: &str) -> PathBuf {
        write_generation_for_context(
            store,
            name,
            host_id,
            env!("CARGO_PKG_VERSION"),
            "source",
            "system",
            "system",
        )
    }

    fn write_user_generation(store: &Path, name: &str, host_id: &str) -> PathBuf {
        write_generation_for_context(
            store,
            name,
            host_id,
            env!("CARGO_PKG_VERSION"),
            "source",
            "user",
            "user",
        )
    }

    fn write_generation_for(
        store: &Path,
        name: &str,
        host_id: &str,
        producer_version: &str,
        producer_build_id: &str,
    ) -> PathBuf {
        write_generation_for_context(
            store,
            name,
            host_id,
            producer_version,
            producer_build_id,
            "system",
            "system",
        )
    }

    fn write_generation_for_context(
        store: &Path,
        name: &str,
        host_id: &str,
        producer_version: &str,
        producer_build_id: &str,
        target: &str,
        manager: &str,
    ) -> PathBuf {
        let input: RenderInput = serde_json::from_value(json!({
            "producer": {
                "name": "graft",
                "version": producer_version,
                "buildId": producer_build_id
            },
            "hostId": host_id,
            "target": target,
            "manager": manager,
            "workerApiRange": { "major": 1, "min_minor": 0, "max_minor": 0 },
            "workloads": []
        }))
        .unwrap();
        let rendered = render(input).unwrap();
        let generation = store.join(name);
        fs::create_dir(&generation).unwrap();
        fs::write(generation.join("manifest.json"), rendered.manifest_json()).unwrap();
        fs::write(generation.join("endpoint.json"), rendered.endpoint_json()).unwrap();
        fs::set_permissions(&generation, fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(
            generation.join("manifest.json"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        fs::set_permissions(
            generation.join("endpoint.json"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        generation
    }

    fn default_acl() -> Vec<u8> {
        acl(&[
            (0x0001, 0o7, u32::MAX),
            (0x0002, 0o7, 65_534),
            (0x0004, 0o7, u32::MAX),
            (0x0010, 0o7, u32::MAX),
            (0x0020, 0, u32::MAX),
        ])
    }

    fn access_acl() -> Vec<u8> {
        default_acl()
    }

    fn acl(entries: &[(u16, u16, u32)]) -> Vec<u8> {
        let mut bytes = 0x0002_u32.to_le_bytes().to_vec();
        for (tag, permissions, identifier) in entries {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&permissions.to_le_bytes());
            bytes.extend_from_slice(&identifier.to_le_bytes());
        }
        bytes
    }

    fn set_default_acl(directory: &File) -> bool {
        set_acl(directory, "system.posix_acl_default", &default_acl())
    }

    fn set_access_acl(directory: &File) -> bool {
        set_acl(directory, "system.posix_acl_access", &access_acl())
    }

    fn set_acl(directory: &File, name: &str, acl: &[u8]) -> bool {
        match rustix::fs::fsetxattr(directory, name, acl, rustix::fs::XattrFlags::empty()) {
            Ok(()) => true,
            Err(rustix::io::Errno::OPNOTSUPP) if std::env::var_os("NIX_BUILD_TOP").is_some() => {
                eprintln!(
                    "skipping real POSIX ACL regression: Nix build sandbox does not support ACL xattrs"
                );
                false
            }
            Err(error) => panic!("cannot create POSIX ACL fixture: {error}"),
        }
    }

    fn get_acl(directory: &File, name: &str) -> Vec<u8> {
        let mut acl = vec![0_u8; 64 * 1_024];
        let length = rustix::fs::fgetxattr(directory, name, &mut acl).unwrap();
        acl.truncate(length);
        acl
    }

    fn assert_acl_absent(directory: &File, name: &str) {
        let mut acl = [0_u8; 1];
        assert_eq!(
            rustix::fs::fgetxattr(directory, name, &mut acl),
            Err(rustix::io::Errno::NODATA)
        );
    }

    fn producer() -> ProducerIdentity {
        producer_for(env!("CARGO_PKG_VERSION"), "source")
    }

    fn producer_for(version: &str, build_id: &str) -> ProducerIdentity {
        ProducerIdentity::new("graft", version, build_id).unwrap()
    }
}
