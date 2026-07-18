//! Bounded durable lifecycle activation interlocks.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::manifest::WorkloadLifecycle;
use crate::protocol::ConnectionIdentifier;

use super::discovery::BackendSelector;
use super::lifecycle::{LifecycleAction, LifecycleState, ManagerEpoch, OperationIdentifier};
use super::observation::{ObservationText, WorkloadSelector};

/// Maximum retained interlocks per worker.
pub const MAX_INTERLOCKS: usize = 256;
/// Maximum encoded bytes in one interlock.
pub const MAX_INTERLOCK_BYTES: usize = 4 * 1_024;
/// Maximum aggregate encoded interlock bytes.
pub const MAX_INTERLOCK_TOTAL_BYTES: usize = 1024 * 1_024;

/// Durable manager-work commitment phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterlockPhase {
    /// No backend call or attachment began.
    Prepared,
    /// A single-call submission may be in flight.
    CommittingSubmission,
    /// Verified start-job cancellation may be in flight.
    CommittingCancel,
    /// Cancellation committed and stop remains pending.
    CancelCommittedStopPending,
    /// Stop submission may be in flight.
    CommittingStop,
    /// Existing exact manager work is being observed.
    ObservingExisting,
    /// Manager acceptance was confirmed.
    CommittedSubmission,
}

/// Bounded non-secret activation interlock record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct InterlockRecord {
    /// Worker epoch that created the record.
    pub worker_epoch: ConnectionIdentifier,
    /// Authenticated principal UID.
    pub principal_uid: u32,
    /// Principal-scoped operation identity.
    pub operation_id: OperationIdentifier,
    /// Immutable workload identity.
    pub selector: WorkloadSelector,
    /// Declared lifecycle used for terminal reconciliation.
    pub lifecycle: WorkloadLifecycle,
    /// Requested action.
    pub action: LifecycleAction,
    /// Exact worker-derived backend identities required for reconciliation.
    pub backend_selector: BackendSelector,
    /// Server logical acceptance time.
    pub accepted_ms: u64,
    /// Absolute terminal-observation cutoff.
    pub observation_deadline_ms: u64,
    /// Manager submission time when this worker submitted work.
    pub submission_ms: Option<u64>,
    /// Manager state captured before commitment.
    pub initial_state: LifecycleState,
    /// Current durable phase.
    pub phase: InterlockPhase,
    /// Manager epoch when known.
    pub manager_epoch: Option<ManagerEpoch>,
    /// Correlated manager job ID when known.
    pub job_id: Option<u32>,
    /// Correlated invocation identity when known.
    pub invocation_id: Option<ObservationText>,
}

/// Secure fixed-context interlock store.
#[derive(Debug, Clone)]
pub struct InterlockStore {
    directory: PathBuf,
    activation_lock: PathBuf,
    uid: u32,
    gid: u32,
}

impl InterlockStore {
    /// Creates a store for Nix-installed absolute paths and fixed ownership.
    ///
    /// # Errors
    ///
    /// Returns an error for relative or non-normalized paths.
    pub fn new(
        directory: PathBuf,
        activation_lock: PathBuf,
        uid: u32,
        gid: u32,
    ) -> Result<Self, InterlockError> {
        if !is_normal_absolute(&directory) || !is_normal_absolute(&activation_lock) {
            return Err(InterlockError::UnsafePath);
        }
        Ok(Self {
            directory,
            activation_lock,
            uid,
            gid,
        })
    }

    /// Takes the shared activation lock used around worker submission.
    ///
    /// # Errors
    ///
    /// Returns an error when the installed lock is unsafe or unavailable.
    pub fn lock_submission(&self) -> Result<ActivationGuard, InterlockError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.activation_lock)
            .map_err(InterlockError::Io)?;
        validate_file(&file, self.uid, self.gid, 0o600)?;
        // SAFETY: `file` owns a live descriptor for the fixed activation-lock
        // regular file. `flock` does not outlive that descriptor.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) } != 0 {
            return Err(InterlockError::Io(std::io::Error::last_os_error()));
        }
        Ok(ActivationGuard { file })
    }

    /// Loads and validates every retained record within fixed limits.
    ///
    /// # Errors
    ///
    /// Returns an error for excess, malformed, unsafe, duplicate, or oversized records.
    pub fn load(&self) -> Result<Vec<InterlockRecord>, InterlockError> {
        let _transaction = lock_directory(&self.directory, false)?;
        self.load_unlocked()
    }

    fn load_unlocked(&self) -> Result<Vec<InterlockRecord>, InterlockError> {
        validate_directory(&self.directory, self.uid, self.gid)?;
        let mut entries = fs::read_dir(&self.directory).map_err(InterlockError::Io)?;
        let mut records = Vec::new();
        let mut total = 0_usize;
        for index in 0..=MAX_INTERLOCKS {
            let Some(entry) = entries.next() else { break };
            let entry = entry.map_err(InterlockError::Io)?;
            if index == MAX_INTERLOCKS {
                return Err(InterlockError::Capacity);
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or(InterlockError::InvalidName)?;
            let expected = name
                .strip_suffix(".json")
                .ok_or(InterlockError::InvalidName)?;
            OperationIdentifier::parse(expected).map_err(|_| InterlockError::InvalidName)?;
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(entry.path())
                .map_err(InterlockError::Io)?;
            validate_file(&file, self.uid, self.gid, 0o600)?;
            let length = usize::try_from(file.metadata().map_err(InterlockError::Io)?.len())
                .map_err(|_| InterlockError::RecordTooLarge)?;
            if length == 0 || length > MAX_INTERLOCK_BYTES {
                return Err(InterlockError::RecordTooLarge);
            }
            total = total.checked_add(length).ok_or(InterlockError::Capacity)?;
            if total > MAX_INTERLOCK_TOTAL_BYTES {
                return Err(InterlockError::Capacity);
            }
            let mut bytes = Vec::with_capacity(length);
            let read_limit =
                u64::try_from(MAX_INTERLOCK_BYTES + 1).map_err(|_| InterlockError::Capacity)?;
            file.take(read_limit)
                .read_to_end(&mut bytes)
                .map_err(InterlockError::Io)?;
            if bytes.len() != length {
                return Err(InterlockError::RecordTooLarge);
            }
            let record: InterlockRecord =
                serde_json::from_slice(&bytes).map_err(InterlockError::Json)?;
            if record.operation_id.to_canonical_string() != expected
                || records
                    .iter()
                    .any(|existing: &InterlockRecord| existing.operation_id == record.operation_id)
            {
                return Err(InterlockError::Duplicate);
            }
            records.push(record);
        }
        records.sort_by_key(|record| record.operation_id);
        Ok(records)
    }

    /// Atomically creates one synchronized record without replacement.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity exists or validation, capacity,
    /// encoding, or durable I/O fails.
    pub fn persist_new(&self, record: &InterlockRecord) -> Result<(), InterlockError> {
        self.persist_internal(record, false)
    }

    /// Atomically creates or replaces one synchronized record.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, capacity, encoding, or durable I/O fails.
    pub fn persist(&self, record: &InterlockRecord) -> Result<(), InterlockError> {
        self.persist_internal(record, true)
    }

    fn persist_internal(
        &self,
        record: &InterlockRecord,
        allow_replace: bool,
    ) -> Result<(), InterlockError> {
        validate_directory(&self.directory, self.uid, self.gid)?;
        let _transaction = lock_directory(&self.directory, true)?;
        let bytes = serde_json::to_vec(record).map_err(InterlockError::Json)?;
        if bytes.is_empty() || bytes.len() > MAX_INTERLOCK_BYTES {
            return Err(InterlockError::RecordTooLarge);
        }
        let existing = self.load_unlocked()?;
        let replacing = existing
            .iter()
            .any(|value| value.operation_id == record.operation_id);
        if replacing && !allow_replace {
            return Err(InterlockError::AlreadyExists);
        }
        if !replacing && existing.len() >= MAX_INTERLOCKS {
            return Err(InterlockError::Capacity);
        }
        let (existing_total, replaced_length) =
            existing
                .iter()
                .try_fold((0_usize, 0_usize), |(total, replaced_length), value| {
                    let length = serde_json::to_vec(value)
                        .map_err(InterlockError::Json)?
                        .len();
                    let total = total.checked_add(length).ok_or(InterlockError::Capacity)?;
                    let replaced_length = if value.operation_id == record.operation_id {
                        length
                    } else {
                        replaced_length
                    };
                    Ok::<_, InterlockError>((total, replaced_length))
                })?;
        if existing_total
            .saturating_sub(replaced_length)
            .saturating_add(bytes.len())
            > MAX_INTERLOCK_TOTAL_BYTES
        {
            return Err(InterlockError::Capacity);
        }
        let name = format!("{}.json", record.operation_id.to_canonical_string());
        let temporary_name = format!(".{name}.{}.tmp", uuid::Uuid::now_v7());
        let temporary_path = self.directory.join(&temporary_name);
        let final_path = self.directory.join(name);
        let result = (|| {
            let mut temporary = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary_path)
                .map_err(InterlockError::Io)?;
            temporary.write_all(&bytes).map_err(InterlockError::Io)?;
            temporary.sync_all().map_err(InterlockError::Io)?;
            if allow_replace {
                fs::rename(&temporary_path, &final_path).map_err(InterlockError::Io)?;
            } else {
                fs::hard_link(&temporary_path, &final_path).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        InterlockError::AlreadyExists
                    } else {
                        InterlockError::Io(error)
                    }
                })?;
                fs::remove_file(&temporary_path).map_err(InterlockError::Io)?;
            }
            sync_directory(&self.directory)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }

    /// Removes a known synchronized record.
    ///
    /// # Errors
    ///
    /// Returns an error when the record or directory cannot be synchronized.
    pub fn remove(&self, operation_id: OperationIdentifier) -> Result<(), InterlockError> {
        validate_directory(&self.directory, self.uid, self.gid)?;
        let _transaction = lock_directory(&self.directory, true)?;
        let path = self
            .directory
            .join(format!("{}.json", operation_id.to_canonical_string()));
        fs::remove_file(path).map_err(InterlockError::Io)?;
        sync_directory(&self.directory)
    }
}

fn lock_directory(path: &Path, exclusive: bool) -> Result<DirectoryGuard, InterlockError> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map_err(InterlockError::Io)?;
    let operation = if exclusive {
        libc::LOCK_EX
    } else {
        libc::LOCK_SH
    };
    // SAFETY: `directory` owns a live descriptor and the flock does not outlive it.
    if unsafe { libc::flock(directory.as_raw_fd(), operation) } != 0 {
        return Err(InterlockError::Io(std::io::Error::last_os_error()));
    }
    Ok(DirectoryGuard { directory })
}

#[derive(Debug)]
struct DirectoryGuard {
    directory: File,
}

impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        // SAFETY: the guard owns the locked descriptor until this drop call.
        let _ = unsafe { libc::flock(self.directory.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// Held shared activation lock.
#[derive(Debug)]
pub struct ActivationGuard {
    file: File,
}

impl Drop for ActivationGuard {
    fn drop(&mut self) {
        // SAFETY: the guard owns the locked descriptor until this drop call.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// Fail-closed interlock error.
#[derive(Debug, Error)]
pub enum InterlockError {
    /// Installed path is not absolute and normalized.
    #[error("unsafe interlock path")]
    UnsafePath,
    /// Ownership, mode, or file type is unsafe.
    #[error("unsafe interlock filesystem object")]
    UnsafeObject,
    /// Entry name is not a canonical operation record.
    #[error("invalid interlock entry name")]
    InvalidName,
    /// Record exceeds its encoded bound.
    #[error("interlock record exceeds encoded bound")]
    RecordTooLarge,
    /// Count or aggregate byte capacity is exhausted.
    #[error("interlock capacity exhausted")]
    Capacity,
    /// Duplicate operation identity was found.
    #[error("duplicate interlock operation identity")]
    Duplicate,
    /// Durable operation identity already exists.
    #[error("interlock operation identity already exists")]
    AlreadyExists,
    /// Filesystem operation failed.
    #[error("interlock filesystem operation failed: {0}")]
    Io(#[source] std::io::Error),
    /// Typed record decoding failed.
    #[error("interlock record decoding failed: {0}")]
    Json(#[source] serde_json::Error),
}

fn sync_directory(path: &Path) -> Result<(), InterlockError> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(InterlockError::Io)
}

fn validate_directory(path: &Path, uid: u32, gid: u32) -> Result<(), InterlockError> {
    let metadata = fs::symlink_metadata(path).map_err(InterlockError::Io)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.permissions().mode() & 0o7777 != 0o700
    {
        return Err(InterlockError::UnsafeObject);
    }
    Ok(())
}

fn validate_file(file: &File, uid: u32, gid: u32, mode: u32) -> Result<(), InterlockError> {
    let metadata = file.metadata().map_err(InterlockError::Io)?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.permissions().mode() & 0o7777 != mode
    {
        return Err(InterlockError::UnsafeObject);
    }
    Ok(())
}

fn is_normal_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use tempfile::TempDir;

    use crate::protocol::{ManifestGeneration, WorkerTarget};

    use super::*;

    fn fixture() -> (TempDir, InterlockStore) {
        let temporary = TempDir::new().unwrap();
        let directory = temporary.path().join("interlocks");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let lock = temporary.path().join("activation.lock");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&lock)
            .unwrap();
        let metadata = directory.metadata().unwrap();
        let store = InterlockStore::new(directory, lock, metadata.uid(), metadata.gid()).unwrap();
        (temporary, store)
    }

    fn operation_id() -> OperationIdentifier {
        OperationIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20").unwrap()
    }

    fn record() -> InterlockRecord {
        InterlockRecord {
            worker_epoch: ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21")
                .unwrap(),
            principal_uid: rustix::process::geteuid().as_raw(),
            operation_id: operation_id(),
            selector: WorkloadSelector {
                target: WorkerTarget::User,
                name: ObservationText::parse("alpha").unwrap(),
                generation: ManifestGeneration::parse("a".repeat(64)).unwrap(),
                workload_id: ManifestGeneration::parse("b".repeat(64)).unwrap(),
            },
            lifecycle: WorkloadLifecycle::LongRunning,
            action: LifecycleAction::Up,
            backend_selector: BackendSelector {
                workload_name: ObservationText::parse("alpha").unwrap(),
                generated_service: ObservationText::parse("alpha.service").unwrap(),
                container_name: ObservationText::parse("alpha").unwrap(),
            },
            accepted_ms: operation_id().timestamp_ms(),
            observation_deadline_ms: operation_id().timestamp_ms().saturating_add(600_000),
            submission_ms: None,
            initial_state: LifecycleState::Inactive,
            phase: InterlockPhase::Prepared,
            manager_epoch: None,
            job_id: None,
            invocation_id: None,
        }
    }

    #[test]
    fn record_is_atomically_persisted_advanced_loaded_and_removed() {
        let (_temporary, store) = fixture();
        let _guard = store.lock_submission().unwrap();
        let mut value = record();

        store.persist_new(&value).unwrap();
        assert_eq!(store.load().unwrap(), vec![value.clone()]);
        assert!(matches!(
            store.persist_new(&value),
            Err(InterlockError::AlreadyExists)
        ));
        assert_eq!(store.load().unwrap(), vec![value.clone()]);
        value.phase = InterlockPhase::CommittingSubmission;
        store.persist(&value).unwrap();
        assert_eq!(store.load().unwrap(), vec![value.clone()]);
        store.remove(value.operation_id).unwrap();
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn concurrent_transactions_hide_temporary_files_and_preserve_records() {
        let (_temporary, store) = fixture();
        let first = record();
        let mut second = first.clone();
        second.operation_id =
            OperationIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c22").unwrap();

        std::thread::scope(|scope| {
            let first_write = scope.spawn(|| store.persist_new(&first));
            let second_write = scope.spawn(|| store.persist_new(&second));
            first_write.join().unwrap().unwrap();
            second_write.join().unwrap().unwrap();
        });
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded
            .iter()
            .any(|value| value.operation_id == first.operation_id));
        assert!(loaded
            .iter()
            .any(|value| value.operation_id == second.operation_id));
    }

    #[test]
    fn unsafe_modes_malformed_records_and_oversized_records_fail_closed() {
        let (_temporary, store) = fixture();
        let value = record();
        store.persist(&value).unwrap();
        let path = store
            .directory
            .join(format!("{}.json", value.operation_id.to_canonical_string()));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(store.load(), Err(InterlockError::UnsafeObject)));

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let hardlink = store.directory.parent().unwrap().join("record-link");
        fs::hard_link(&path, &hardlink).unwrap();
        assert!(matches!(store.load(), Err(InterlockError::UnsafeObject)));
        fs::remove_file(hardlink).unwrap();

        fs::write(&path, vec![b'x'; MAX_INTERLOCK_BYTES + 1]).unwrap();
        assert!(matches!(store.load(), Err(InterlockError::RecordTooLarge)));

        fs::write(&path, b"{}").unwrap();
        assert!(matches!(store.load(), Err(InterlockError::Json(_))));
    }

    #[test]
    fn non_normal_paths_and_excess_entries_are_rejected() {
        let temporary = TempDir::new().unwrap();
        assert!(matches!(
            InterlockStore::new(
                temporary.path().join("a/../b"),
                temporary.path().join("lock"),
                0,
                0,
            ),
            Err(InterlockError::UnsafePath)
        ));

        let (_temporary, store) = fixture();
        for _ in 0..=MAX_INTERLOCKS {
            let id =
                OperationIdentifier::parse(&uuid::Uuid::now_v7().hyphenated().to_string()).unwrap();
            let path = store
                .directory
                .join(format!("{}.json", id.to_canonical_string()));
            let mut value = record();
            value.operation_id = id;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            file.write_all(&serde_json::to_vec(&value).unwrap())
                .unwrap();
        }
        assert!(matches!(store.load(), Err(InterlockError::Capacity)));
    }
}
