//! Coherent filesystem loading for one immutable discovery generation.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Component, Path, PathBuf};

use crate::protocol::{ManagerKind, WorkerTarget};

use super::schema::validate_pair;
use super::{
    EndpointDescriptor, Manifest, ManifestError, ProducerIdentity, MAX_ENDPOINT_BYTES,
    MAX_MANIFEST_BYTES,
};

const SYSTEM_PARENT: &str = "/etc/graft";
const STORE_ROOT: &str = "/nix/store";

/// Accepted immutable-generation ownership model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationOwner {
    /// Multi-user Nix store ownership (`root:root`).
    MultiUser,
    /// Single-user Nix store ownership matching the worker account.
    SingleUser {
        /// Effective worker UID.
        uid: u32,
        /// Primary worker GID.
        gid: u32,
    },
}

impl GenerationOwner {
    const fn ids(self) -> (u32, u32) {
        match self {
            Self::MultiUser => (0, 0),
            Self::SingleUser { uid, gid } => (uid, gid),
        }
    }
}

/// One parsed manifest/endpoint pair retained with its opened generation.
#[derive(Debug)]
pub struct GenerationSnapshot {
    generation: File,
    manifest_file: File,
    endpoint_file: File,
    manifest: Manifest,
    endpoint: EndpointDescriptor,
    owner: GenerationOwner,
    bound_user_uid: Option<u32>,
}

impl GenerationSnapshot {
    /// Returns the validated manifest.
    #[must_use]
    pub const fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Returns the validated endpoint descriptor.
    #[must_use]
    pub const fn endpoint(&self) -> &EndpointDescriptor {
        &self.endpoint
    }

    /// Returns the accepted immutable-store ownership model.
    #[must_use]
    pub const fn owner(&self) -> GenerationOwner {
        self.owner
    }

    /// Returns the runtime-bound UID for a user manifest.
    ///
    /// System manifests return `None`; the UID never originates in manifest JSON.
    #[must_use]
    pub const fn bound_user_uid(&self) -> Option<u32> {
        self.bound_user_uid
    }

    /// Returns the retained generation directory descriptor.
    #[must_use]
    pub fn generation_file(&self) -> &File {
        &self.generation
    }

    /// Returns the retained manifest descriptor.
    #[must_use]
    pub fn manifest_file(&self) -> &File {
        &self.manifest_file
    }

    /// Returns the retained endpoint descriptor.
    #[must_use]
    pub fn endpoint_file(&self) -> &File {
        &self.endpoint_file
    }
}

/// Loader fixed to one Nix-installed worker context.
#[derive(Debug, Clone)]
pub struct ManifestLoader {
    parent: PathBuf,
    context: LoaderContext,
    installed_producer: ProducerIdentity,
}

#[derive(Debug, Clone, Copy)]
enum LoaderContext {
    System { graft_gid: u32 },
    User { uid: u32, gid: u32 },
}

impl ManifestLoader {
    /// Creates the fixed system-worker loader for `/etc/graft/current`.
    ///
    /// `graft_gid` is the Nix-resolved GID of the fixed `graft` readership group.
    #[must_use]
    pub fn system(graft_gid: u32, installed_producer: ProducerIdentity) -> Self {
        Self {
            parent: PathBuf::from(SYSTEM_PARENT),
            context: LoaderContext::System { graft_gid },
            installed_producer,
        }
    }

    /// Creates a user-worker loader from the Nix-expanded absolute config home.
    ///
    /// The caller must pass the fixed path installed in the worker service; this
    /// function never reads `XDG_CONFIG_HOME` or another environment variable.
    ///
    /// # Errors
    ///
    /// Returns an error for a relative or non-normalized config-home path.
    pub fn user(
        config_home: &Path,
        uid: u32,
        gid: u32,
        installed_producer: ProducerIdentity,
    ) -> Result<Self, ManifestError> {
        validate_absolute_normal_path(config_home)?;
        Ok(Self {
            parent: config_home.join("graft"),
            context: LoaderContext::User { uid, gid },
            installed_producer,
        })
    }

    /// Opens and validates one coherent current generation.
    ///
    /// # Errors
    ///
    /// Returns an error for any path, ownership, mode, type, size, schema,
    /// compatibility, identity, ordering, uniqueness, or digest mismatch.
    pub fn load(&self) -> Result<GenerationSnapshot, ManifestError> {
        self.load_with_store_root(Path::new(STORE_ROOT))
    }

    fn load_with_store_root(&self, store_root: &Path) -> Result<GenerationSnapshot, ManifestError> {
        let parent_file = open_directory_path(&self.parent)?;
        self.validate_parent(&parent_file)?;
        validate_no_acl(&parent_file)?;
        let pointer = rustix::fs::statat(
            &parent_file,
            "current",
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(errno_to_manifest)?;
        if rustix::fs::FileType::from_raw_mode(pointer.st_mode) != rustix::fs::FileType::Symlink {
            return Err(ManifestError::FileType);
        }
        let expected_pointer_owner = match self.context {
            LoaderContext::System { .. } => (0, 0),
            LoaderContext::User { uid, gid } => (uid, gid),
        };
        if (pointer.st_uid, pointer.st_gid) != expected_pointer_owner {
            return Err(ManifestError::Ownership);
        }

        let target_bytes = rustix::fs::readlinkat(&parent_file, "current", Vec::new())
            .map_err(errno_to_manifest)?
            .into_bytes();
        let pointer_after = rustix::fs::statat(
            &parent_file,
            "current",
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(errno_to_manifest)?;
        if pointer.st_dev != pointer_after.st_dev || pointer.st_ino != pointer_after.st_ino {
            return Err(ManifestError::GenerationReference);
        }
        let target = PathBuf::from(std::ffi::OsString::from_vec(target_bytes));
        validate_store_target(&target, store_root)?;
        let store = open_directory_path(store_root)?;
        let generation_name = target
            .file_name()
            .ok_or(ManifestError::GenerationReference)?;
        let generation = open_directory_child(&store, generation_name)?;
        let opened_metadata = generation.metadata().map_err(ManifestError::Filesystem)?;
        validate_no_acl(&generation)?;
        let owner = self.validate_generation_owner(&opened_metadata)?;
        validate_mode(&opened_metadata, 0o555)?;
        validate_entries(&descriptor_path(&generation))?;

        let manifest_file = open_child(&generation, "manifest.json")?;
        let endpoint_file = open_child(&generation, "endpoint.json")?;
        let owner_ids = owner.ids();
        validate_document(&manifest_file, owner_ids, MAX_MANIFEST_BYTES)?;
        validate_document(&endpoint_file, owner_ids, MAX_ENDPOINT_BYTES)?;

        let manifest_bytes = read_bounded(&manifest_file, MAX_MANIFEST_BYTES)?;
        let endpoint_bytes = read_bounded(&endpoint_file, MAX_ENDPOINT_BYTES)?;
        let manifest = Manifest::from_json(&manifest_bytes)?;
        let endpoint = EndpointDescriptor::from_json(&endpoint_bytes)?;
        validate_pair(&manifest, &endpoint)?;
        validate_loaded_context(&manifest, self.context)?;
        if manifest.producer() != &self.installed_producer {
            return Err(ManifestError::InstalledProducerMismatch);
        }

        Ok(GenerationSnapshot {
            generation,
            manifest_file,
            endpoint_file,
            manifest,
            endpoint,
            owner,
            bound_user_uid: match self.context {
                LoaderContext::System { .. } => None,
                LoaderContext::User { uid, .. } => Some(uid),
            },
        })
    }

    fn validate_parent(&self, parent: &File) -> Result<(), ManifestError> {
        let metadata = parent.metadata().map_err(ManifestError::Filesystem)?;
        if !metadata.is_dir() {
            return Err(ManifestError::FileType);
        }
        validate_owner(&metadata, expected_parent_owner(self.context))?;
        let expected_mode = match self.context {
            LoaderContext::System { .. } => 0o750,
            LoaderContext::User { .. } => 0o700,
        };
        validate_mode(&metadata, expected_mode)
    }

    fn validate_generation_owner(
        &self,
        metadata: &Metadata,
    ) -> Result<GenerationOwner, ManifestError> {
        select_owner(self.context, (metadata.uid(), metadata.gid()))
    }
}

const fn expected_parent_owner(context: LoaderContext) -> (u32, u32) {
    match context {
        LoaderContext::System { graft_gid } => (0, graft_gid),
        LoaderContext::User { uid, gid } => (uid, gid),
    }
}

fn select_owner(
    context: LoaderContext,
    actual: (u32, u32),
) -> Result<GenerationOwner, ManifestError> {
    match context {
        LoaderContext::System { .. } | LoaderContext::User { .. } if actual == (0, 0) => {
            Ok(GenerationOwner::MultiUser)
        }
        LoaderContext::User { uid, gid } if actual == (uid, gid) => {
            Ok(GenerationOwner::SingleUser { uid, gid })
        }
        LoaderContext::System { .. } | LoaderContext::User { .. } => Err(ManifestError::Ownership),
    }
}

fn validate_loaded_context(
    manifest: &Manifest,
    context: LoaderContext,
) -> Result<(), ManifestError> {
    let expected = match context {
        LoaderContext::System { .. } => (WorkerTarget::System, ManagerKind::System),
        LoaderContext::User { .. } => (WorkerTarget::User, ManagerKind::User),
    };
    if (manifest.target(), manifest.manager()) != expected {
        return Err(ManifestError::ContextMismatch);
    }
    Ok(())
}

fn validate_absolute_normal_path(path: &Path) -> Result<(), ManifestError> {
    if !path.is_absolute() {
        return Err(ManifestError::GenerationReference);
    }
    let mut rebuilt = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => rebuilt.push(component.as_os_str()),
            Component::Prefix(_) | Component::CurDir | Component::ParentDir => {
                return Err(ManifestError::GenerationReference);
            }
        }
    }
    if rebuilt.as_os_str() != path.as_os_str() {
        return Err(ManifestError::GenerationReference);
    }
    Ok(())
}

fn validate_store_target(target: &Path, store_root: &Path) -> Result<(), ManifestError> {
    validate_absolute_normal_path(target)?;
    validate_absolute_normal_path(store_root)?;
    if target.parent() != Some(store_root) {
        return Err(ManifestError::GenerationReference);
    }
    let Some(name) = target.file_name().and_then(|name| name.to_str()) else {
        return Err(ManifestError::GenerationReference);
    };
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
    {
        return Err(ManifestError::GenerationReference);
    }
    Ok(())
}

fn validate_entries(target: &Path) -> Result<(), ManifestError> {
    let mut entries = BTreeNames::default();
    for entry in fs::read_dir(target).map_err(ManifestError::Filesystem)? {
        let entry = entry.map_err(ManifestError::Filesystem)?;
        entries.insert(&entry.file_name())?;
    }
    if entries.manifest && entries.endpoint && entries.count == 2 {
        Ok(())
    } else {
        Err(ManifestError::UnexpectedEntry)
    }
}

#[derive(Default)]
struct BTreeNames {
    manifest: bool,
    endpoint: bool,
    count: usize,
}

impl BTreeNames {
    fn insert(&mut self, name: &std::ffi::OsStr) -> Result<(), ManifestError> {
        self.count = self.count.saturating_add(1);
        match name.to_str() {
            Some("manifest.json") if !self.manifest => self.manifest = true,
            Some("endpoint.json") if !self.endpoint => self.endpoint = true,
            _ => return Err(ManifestError::UnexpectedEntry),
        }
        Ok(())
    }
}

fn errno_to_manifest(error: rustix::io::Errno) -> ManifestError {
    ManifestError::Filesystem(io::Error::from_raw_os_error(error.raw_os_error()))
}

fn open_directory_path(path: &Path) -> Result<File, ManifestError> {
    validate_absolute_normal_path(path)?;
    let mut directory = open_nofollow(Path::new("/"), true)?;
    for component in path.components() {
        if let Component::Normal(name) = component {
            directory = open_directory_child(&directory, name)?;
        }
    }
    Ok(directory)
}

fn open_directory_child(directory: &File, name: &std::ffi::OsStr) -> Result<File, ManifestError> {
    let descriptor = rustix::fs::openat(
        directory,
        name,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::DIRECTORY,
        rustix::fs::Mode::empty(),
    )
    .map_err(errno_to_manifest)?;
    Ok(File::from(descriptor))
}

fn open_nofollow(path: &Path, directory: bool) -> Result<File, ManifestError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    if directory {
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY);
    }
    options.open(path).map_err(ManifestError::Filesystem)
}

fn descriptor_path(directory: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}

fn open_child(directory: &File, name: &str) -> Result<File, ManifestError> {
    let descriptor = rustix::fs::openat(
        directory,
        name,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(errno_to_manifest)?;
    Ok(File::from(descriptor))
}

fn validate_document(file: &File, owner: (u32, u32), maximum: u64) -> Result<(), ManifestError> {
    let metadata = file.metadata().map_err(ManifestError::Filesystem)?;
    if !metadata.is_file() {
        return Err(ManifestError::FileType);
    }
    validate_owner(&metadata, owner)?;
    validate_mode(&metadata, 0o444)?;
    validate_no_acl(file)?;
    if metadata.len() == 0 || metadata.len() > maximum {
        return Err(ManifestError::DocumentTooLarge);
    }
    Ok(())
}

fn validate_owner(metadata: &Metadata, expected: (u32, u32)) -> Result<(), ManifestError> {
    if (metadata.uid(), metadata.gid()) == expected {
        Ok(())
    } else {
        Err(ManifestError::Ownership)
    }
}

fn validate_mode(metadata: &Metadata, expected: u32) -> Result<(), ManifestError> {
    if metadata.mode() & 0o7777 == expected {
        Ok(())
    } else {
        Err(ManifestError::Permissions)
    }
}

fn validate_no_acl(file: &File) -> Result<(), ManifestError> {
    let mut buffer = vec![0_u8; 64 * 1_024];
    let length = rustix::fs::flistxattr(file, &mut buffer).map_err(|error| {
        ManifestError::Filesystem(io::Error::from_raw_os_error(error.raw_os_error()))
    })?;
    buffer.truncate(length);
    if contains_posix_acl(&buffer) {
        return Err(ManifestError::Permissions);
    }
    Ok(())
}

fn contains_posix_acl(attributes: &[u8]) -> bool {
    attributes.split(|byte| *byte == 0).any(|name| {
        matches!(
            name,
            b"system.posix_acl_access" | b"system.posix_acl_default"
        )
    })
}

fn bounded_capacity(current_length: u64, maximum: u64) -> Result<usize, ManifestError> {
    usize::try_from(current_length.min(maximum)).map_err(|_| ManifestError::DocumentTooLarge)
}

fn read_bounded(file: &File, maximum: u64) -> Result<Vec<u8>, ManifestError> {
    let current_length = file.metadata().map_err(ManifestError::Filesystem)?.len();
    let capacity = bounded_capacity(current_length, maximum)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.try_clone()
        .map_err(ManifestError::Filesystem)?
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(ManifestError::Filesystem)?;
    if u64::try_from(bytes.len()).map_or(true, |length| length > maximum) {
        return Err(ManifestError::DocumentTooLarge);
    }
    Ok(bytes)
}

impl From<io::Error> for ManifestError {
    fn from(error: io::Error) -> Self {
        Self::Filesystem(error)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    use serde_json::{json, Value};
    use sha2::{Digest as _, Sha256};
    use tempfile::TempDir;

    use super::*;

    struct Fixture {
        temporary: TempDir,
        config_home: PathBuf,
        store_root: PathBuf,
        generation: PathBuf,
        loader: ManifestLoader,
    }

    impl Fixture {
        fn new() -> Self {
            let temporary = TempDir::new().unwrap();
            let config_home = temporary.path().join("config");
            let parent = config_home.join("graft");
            let store_root = temporary.path().join("store");
            let generation = store_root.join("generation-one");
            fs::create_dir_all(&parent).unwrap();
            fs::create_dir_all(&generation).unwrap();
            chmod(&parent, 0o700);

            let metadata = parent.metadata().unwrap();
            let loader = ManifestLoader::user(
                &config_home,
                metadata.uid(),
                metadata.gid(),
                installed_producer(),
            )
            .unwrap();
            let (manifest, endpoint) = documents();
            fs::write(
                generation.join("manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            fs::write(
                generation.join("endpoint.json"),
                serde_json::to_vec(&endpoint).unwrap(),
            )
            .unwrap();
            chmod(&generation.join("manifest.json"), 0o444);
            chmod(&generation.join("endpoint.json"), 0o444);
            chmod(&generation, 0o555);
            symlink(&generation, parent.join("current")).unwrap();

            Self {
                temporary,
                config_home,
                store_root,
                generation,
                loader,
            }
        }
    }

    #[test]
    fn coherent_user_generation_loads_and_retains_open_files() {
        let fixture = Fixture::new();

        let snapshot = fixture
            .loader
            .load_with_store_root(&fixture.store_root)
            .unwrap();

        assert_eq!(snapshot.manifest().target(), WorkerTarget::User);
        assert_eq!(
            snapshot.owner(),
            GenerationOwner::SingleUser {
                uid: fixture.generation.metadata().unwrap().uid(),
                gid: fixture.generation.metadata().unwrap().gid(),
            }
        );
        assert_eq!(
            snapshot.bound_user_uid(),
            Some(fixture.generation.metadata().unwrap().uid())
        );
        assert!(snapshot.generation_file().metadata().unwrap().is_dir());
        assert!(snapshot.manifest_file().metadata().unwrap().is_file());
        assert!(snapshot.endpoint_file().metadata().unwrap().is_file());

        fs::remove_file(fixture.config_home.join("graft/current")).unwrap();
        assert_eq!(snapshot.manifest().workload_count(), 0);
        assert!(snapshot.manifest_file().metadata().unwrap().is_file());
    }

    #[test]
    fn loader_rejects_symlinked_document_wrong_mode_and_unexpected_entry() {
        let symlink_fixture = Fixture::new();
        fs::set_permissions(
            &symlink_fixture.generation,
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::remove_file(symlink_fixture.generation.join("manifest.json")).unwrap();
        symlink(
            symlink_fixture.generation.join("endpoint.json"),
            symlink_fixture.generation.join("manifest.json"),
        )
        .unwrap();
        chmod(&symlink_fixture.generation, 0o555);
        assert!(symlink_fixture
            .loader
            .load_with_store_root(&symlink_fixture.store_root)
            .is_err());

        let fifo_fixture = Fixture::new();
        chmod(&fifo_fixture.generation, 0o755);
        fs::remove_file(fifo_fixture.generation.join("manifest.json")).unwrap();
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            fifo_fixture.generation.join("manifest.json"),
            rustix::fs::Mode::RUSR | rustix::fs::Mode::RGRP | rustix::fs::Mode::ROTH,
        )
        .unwrap();
        chmod(&fifo_fixture.generation, 0o555);
        assert!(matches!(
            fifo_fixture
                .loader
                .load_with_store_root(&fifo_fixture.store_root),
            Err(ManifestError::FileType)
        ));

        let mode_fixture = Fixture::new();
        chmod(&mode_fixture.generation.join("manifest.json"), 0o644);
        assert!(matches!(
            mode_fixture
                .loader
                .load_with_store_root(&mode_fixture.store_root),
            Err(ManifestError::Permissions)
        ));

        let extra_fixture = Fixture::new();
        fs::set_permissions(&extra_fixture.generation, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(extra_fixture.generation.join("extra"), b"unexpected").unwrap();
        chmod(&extra_fixture.generation, 0o555);
        assert!(matches!(
            extra_fixture
                .loader
                .load_with_store_root(&extra_fixture.store_root),
            Err(ManifestError::UnexpectedEntry)
        ));
    }

    #[test]
    fn user_loader_rejects_non_normal_paths_and_symlinked_ancestors() {
        assert!(ManifestLoader::user(
            Path::new("/home/user//.config"),
            1000,
            100,
            installed_producer()
        )
        .is_err());
        assert!(ManifestLoader::user(
            Path::new("/home/user/./.config"),
            1000,
            100,
            installed_producer()
        )
        .is_err());

        let fixture = Fixture::new();
        let alias = fixture.temporary.path().join("config-alias");
        symlink(&fixture.config_home, &alias).unwrap();
        let metadata = fixture.config_home.join("graft").metadata().unwrap();
        let loader =
            ManifestLoader::user(&alias, metadata.uid(), metadata.gid(), installed_producer())
                .unwrap();

        assert!(loader.load_with_store_root(&fixture.store_root).is_err());
    }

    #[test]
    fn loader_rejects_pointer_chain_and_mismatched_descriptor() {
        let chain_fixture = Fixture::new();
        let current = chain_fixture.config_home.join("graft/current");
        fs::remove_file(&current).unwrap();
        let intermediate = chain_fixture.store_root.join("intermediate");
        symlink(&chain_fixture.generation, &intermediate).unwrap();
        symlink(&intermediate, &current).unwrap();
        assert!(chain_fixture
            .loader
            .load_with_store_root(&chain_fixture.store_root)
            .is_err());

        let mismatch_fixture = Fixture::new();
        fs::set_permissions(
            &mismatch_fixture.generation,
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        chmod(&mismatch_fixture.generation.join("endpoint.json"), 0o644);
        let (_, mut endpoint) = documents();
        endpoint["hostId"] = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21".into();
        endpoint.as_object_mut().unwrap().remove("endpointDigest");
        endpoint["endpointDigest"] = digest(&endpoint).into();
        fs::write(
            mismatch_fixture.generation.join("endpoint.json"),
            serde_json::to_vec(&endpoint).unwrap(),
        )
        .unwrap();
        chmod(&mismatch_fixture.generation.join("endpoint.json"), 0o444);
        chmod(&mismatch_fixture.generation, 0o555);
        assert!(matches!(
            mismatch_fixture
                .loader
                .load_with_store_root(&mismatch_fixture.store_root),
            Err(ManifestError::DescriptorMismatch)
        ));
    }

    #[test]
    fn loader_rejects_generation_from_another_installed_producer() {
        let fixture = Fixture::new();
        chmod(&fixture.generation, 0o755);
        chmod(&fixture.generation.join("manifest.json"), 0o644);
        chmod(&fixture.generation.join("endpoint.json"), 0o644);
        let (mut manifest, mut endpoint) = documents();
        manifest["producer"]["buildId"] = "other-build".into();
        manifest.as_object_mut().unwrap().remove("generationId");
        manifest.as_object_mut().unwrap().remove("manifestDigest");
        let manifest_digest = digest(&manifest);
        manifest["generationId"] = manifest_digest.clone().into();
        manifest["manifestDigest"] = manifest_digest.clone().into();
        endpoint["producer"]["buildId"] = "other-build".into();
        endpoint["generationId"] = manifest_digest.clone().into();
        endpoint["manifestDigest"] = manifest_digest.into();
        endpoint.as_object_mut().unwrap().remove("endpointDigest");
        endpoint["endpointDigest"] = digest(&endpoint).into();
        fs::write(
            fixture.generation.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            fixture.generation.join("endpoint.json"),
            serde_json::to_vec(&endpoint).unwrap(),
        )
        .unwrap();
        chmod(&fixture.generation.join("manifest.json"), 0o444);
        chmod(&fixture.generation.join("endpoint.json"), 0o444);
        chmod(&fixture.generation, 0o555);

        assert!(matches!(
            fixture.loader.load_with_store_root(&fixture.store_root),
            Err(ManifestError::InstalledProducerMismatch)
        ));
    }

    #[test]
    fn loader_rejects_empty_and_oversized_documents_before_parsing() {
        assert_eq!(
            bounded_capacity(u64::MAX, MAX_MANIFEST_BYTES).unwrap(),
            1_048_576
        );
        let empty_fixture = Fixture::new();
        fs::set_permissions(&empty_fixture.generation, fs::Permissions::from_mode(0o755)).unwrap();
        chmod(&empty_fixture.generation.join("manifest.json"), 0o644);
        fs::write(empty_fixture.generation.join("manifest.json"), []).unwrap();
        chmod(&empty_fixture.generation.join("manifest.json"), 0o444);
        chmod(&empty_fixture.generation, 0o555);
        assert!(matches!(
            empty_fixture
                .loader
                .load_with_store_root(&empty_fixture.store_root),
            Err(ManifestError::DocumentTooLarge)
        ));

        let large_fixture = Fixture::new();
        fs::set_permissions(&large_fixture.generation, fs::Permissions::from_mode(0o755)).unwrap();
        chmod(&large_fixture.generation.join("endpoint.json"), 0o644);
        fs::write(
            large_fixture.generation.join("endpoint.json"),
            vec![b'x'; usize::try_from(MAX_ENDPOINT_BYTES).unwrap() + 1],
        )
        .unwrap();
        chmod(&large_fixture.generation.join("endpoint.json"), 0o444);
        chmod(&large_fixture.generation, 0o555);
        assert!(matches!(
            large_fixture
                .loader
                .load_with_store_root(&large_fixture.store_root),
            Err(ManifestError::DocumentTooLarge)
        ));
    }

    #[test]
    fn acl_attribute_detection_rejects_access_and_default_acls() {
        assert!(!contains_posix_acl(b"user.example\0security.selinux\0"));
        assert!(contains_posix_acl(b"system.posix_acl_access\0"));
        assert!(contains_posix_acl(b"system.posix_acl_default\0"));
    }

    #[test]
    fn owner_policy_distinguishes_multi_user_single_user_and_foreign_store() {
        assert_eq!(
            expected_parent_owner(LoaderContext::System { graft_gid: 42 }),
            (0, 42)
        );
        assert_eq!(
            select_owner(LoaderContext::System { graft_gid: 42 }, (0, 0)).unwrap(),
            GenerationOwner::MultiUser
        );
        assert_eq!(
            select_owner(
                LoaderContext::User {
                    uid: 1000,
                    gid: 100
                },
                (0, 0)
            )
            .unwrap(),
            GenerationOwner::MultiUser
        );
        assert_eq!(
            select_owner(
                LoaderContext::User {
                    uid: 1000,
                    gid: 100
                },
                (1000, 100)
            )
            .unwrap(),
            GenerationOwner::SingleUser {
                uid: 1000,
                gid: 100
            }
        );
        assert!(select_owner(
            LoaderContext::User {
                uid: 1000,
                gid: 100
            },
            (1001, 100)
        )
        .is_err());
        assert!(select_owner(LoaderContext::System { graft_gid: 42 }, (1000, 100)).is_err());
    }

    fn installed_producer() -> ProducerIdentity {
        ProducerIdentity::new("graft", "0.3.0-alpha.1", "test-build").unwrap()
    }

    fn documents() -> (Value, Value) {
        let producer = json!({
            "name":"graft",
            "version":"0.3.0-alpha.1",
            "buildId":"test-build"
        });
        let api = json!({"major":1,"min_minor":0,"max_minor":0});
        let mut manifest = json!({
            "schemaVersion":{"major":1,"minor":0},
            "workerApiRange":api,
            "producer":producer,
            "hostId":"018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20",
            "target":"user",
            "manager":"user",
            "workloadCount":0,
            "workloads":[]
        });
        let manifest_digest = digest(&manifest);
        manifest["generationId"] = manifest_digest.clone().into();
        manifest["manifestDigest"] = manifest_digest.clone().into();
        let mut endpoint = json!({
            "schemaVersion":{"major":1,"minor":0},
            "workerApiRange":api,
            "producer":producer,
            "hostId":"018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20",
            "target":"user",
            "manager":"user",
            "generationId":manifest_digest,
            "manifestDigest":manifest_digest,
            "socketAddress":{
                "kind":"linux_user_runtime_relative",
                "value":"graft/user/worker.sock"
            }
        });
        endpoint["endpointDigest"] = digest(&endpoint).into();
        (manifest, endpoint)
    }

    fn digest(value: &Value) -> String {
        format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
    }

    fn chmod(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
}
