//! Atomic system activation publication for immutable discovery generations.

use std::{
    fs::File,
    io,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
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
        open_directory_child, open_directory_path, validate_absolute_normal_path, validate_no_acl,
    },
    ManifestError, ManifestLoader, ProducerIdentity,
};

const SYSTEM_PARENT: &str = "/etc/graft";
const SYSTEM_RUNTIME_ROOT: &str = "/run/graft";
const SYSTEM_RUNTIME: &str = "/run/graft/system";
const STORE_ROOT: &str = "/nix/store";
const CURRENT: &str = "current";
const ACTIVATION_LOCK: &str = "activation.lock";
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_RETRY: Duration = Duration::from_millis(25);

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
        let parent = ensure_owned_directory(&self.paths.parent, self.uid, self.graft_gid, 0o750)?;
        ensure_owned_directory(&self.paths.runtime_root, self.uid, self.gid, 0o755)?;
        let runtime = ensure_owned_directory(&self.paths.runtime, self.uid, self.graft_gid, 0o750)?;
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

fn ensure_owned_directory(
    path: &Path,
    uid: u32,
    gid: u32,
    mode: u32,
) -> Result<File, PublicationError> {
    let parent_path = path.parent().ok_or(PublicationError::UnsafePath)?;
    let name = path.file_name().ok_or(PublicationError::UnsafePath)?;
    let parent = open_directory_path(parent_path).map_err(PublicationError::Manifest)?;
    let created = match rustix::fs::mkdirat(&parent, name, Mode::from_raw_mode(mode)) {
        Ok(()) => true,
        Err(error) if error == rustix::io::Errno::EXIST => false,
        Err(error) => return Err(errno_to_publication(error)),
    };
    let directory = open_directory_child(&parent, name).map_err(PublicationError::Manifest)?;
    if created {
        rustix::fs::fchown(
            &directory,
            Some(Uid::from_raw(uid)),
            Some(Gid::from_raw(gid)),
        )
        .map_err(errno_to_publication)?;
        rustix::fs::fchmod(&directory, Mode::from_raw_mode(mode)).map_err(errno_to_publication)?;
        rustix::fs::fsync(&parent).map_err(errno_to_publication)?;
    }
    validate_directory(&directory, uid, gid, mode)?;
    Ok(directory)
}

fn validate_directory(
    directory: &File,
    uid: u32,
    gid: u32,
    mode: u32,
) -> Result<(), PublicationError> {
    let metadata = directory.metadata().map_err(PublicationError::Filesystem)?;
    if !metadata.is_dir()
        || (metadata.uid(), metadata.gid()) != (uid, gid)
        || metadata.mode() & 0o7777 != mode
    {
        return Err(PublicationError::UnsafeObject);
    }
    validate_no_acl(directory).map_err(PublicationError::Manifest)
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
        _temporary: TempDir,
        publisher: Publisher,
        parent: PathBuf,
        runtime: PathBuf,
        first: PathBuf,
        second: PathBuf,
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
                _temporary: temporary,
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
        ensure_owned_directory(
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
        write_generation_for(store, name, host_id, env!("CARGO_PKG_VERSION"), "source")
    }

    fn write_generation_for(
        store: &Path,
        name: &str,
        host_id: &str,
        producer_version: &str,
        producer_build_id: &str,
    ) -> PathBuf {
        let input: RenderInput = serde_json::from_value(json!({
            "producer": {
                "name": "graft",
                "version": producer_version,
                "buildId": producer_build_id
            },
            "hostId": host_id,
            "target": "system",
            "manager": "system",
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

    fn producer() -> ProducerIdentity {
        producer_for(env!("CARGO_PKG_VERSION"), "source")
    }

    fn producer_for(version: &str, build_id: &str) -> ProducerIdentity {
        ProducerIdentity::new("graft", version, build_id).unwrap()
    }
}
