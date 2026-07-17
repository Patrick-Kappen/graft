//! Validated primitive and enum types used by the local worker protocol.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use uuid::{Uuid, Variant};

/// Protocol major version implemented by this package.
pub const PROTOCOL_MAJOR: u16 = 1;
/// Lowest contiguous protocol minor version implemented by this package.
pub const PROTOCOL_MIN_MINOR: u16 = 0;
/// Highest contiguous protocol minor version implemented by this package.
pub const PROTOCOL_MAX_MINOR: u16 = 0;
/// Largest integer that is interoperable in a JSON number.
pub const MAX_JSON_INTEGER: u64 = 9_007_199_254_740_991;
/// Maximum diagnostic software-version length in bytes.
pub const MAX_SOFTWARE_VERSION_BYTES: usize = 64;
/// Maximum safe protocol-error summary length in bytes.
pub const MAX_SAFE_SUMMARY_BYTES: usize = 256;

/// Error returned when a protocol primitive violates its wire contract.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A bounded string is empty.
    #[error("{field} must not be empty")]
    EmptyString {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// A bounded string exceeds its byte limit.
    #[error("{field} exceeds {maximum} bytes")]
    StringTooLong {
        /// Name of the invalid field.
        field: &'static str,
        /// Maximum accepted UTF-8 byte count.
        maximum: usize,
    },
    /// A bounded string contains a control character.
    #[error("{field} contains a control character")]
    ControlCharacter {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// A UUID is not canonical version 7.
    #[error("{field} must be a canonical lowercase UUIDv7")]
    InvalidUuidV7 {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// A protocol version range is reversed or spans another major.
    #[error("protocol minor range is invalid")]
    InvalidVersionRange,
    /// A requested limit is zero or above its protocol maximum.
    #[error("{field} must be between 1 and {maximum}")]
    InvalidLimit {
        /// Name of the invalid field.
        field: &'static str,
        /// Protocol maximum for this field.
        maximum: u64,
    },
    /// A JSON integer exceeds the interoperable range.
    #[error("{field} exceeds the interoperable JSON integer range")]
    JsonIntegerTooLarge {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// A request identifier is zero.
    #[error("request identifier must be non-zero")]
    ZeroRequestIdentifier,
    /// A capability appears more than once.
    #[error("requested capability is duplicated: {capability}")]
    DuplicateCapability {
        /// Duplicate capability.
        capability: Capability,
    },
    /// A manifest generation is not lowercase hexadecimal SHA-256.
    #[error("manifest generation must be 64 lowercase hexadecimal characters")]
    InvalidManifestGeneration,
    /// Worker target and manager kind do not form an approved context.
    #[error("worker target and manager kind do not match")]
    InvalidWorkerContext,
}

fn validate_bounded_text(
    value: &str,
    field: &'static str,
    maximum: usize,
) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::EmptyString { field });
    }
    if value.len() > maximum {
        return Err(ValidationError::StringTooLong { field, maximum });
    }
    if value.chars().any(char::is_control) {
        return Err(ValidationError::ControlCharacter { field });
    }
    Ok(())
}

/// Non-zero client-selected request identifier in JSON's interoperable range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RequestIdentifier(u64);

impl RequestIdentifier {
    /// Creates a validated request identifier.
    ///
    /// # Errors
    ///
    /// Returns an error for zero or a value above JSON's interoperable range.
    pub const fn new(value: u64) -> Result<Self, ValidationError> {
        if value == 0 {
            return Err(ValidationError::ZeroRequestIdentifier);
        }
        if value > MAX_JSON_INTEGER {
            return Err(ValidationError::JsonIntegerTooLarge {
                field: "request identifier",
            });
        }
        Ok(Self(value))
    }

    /// Returns the request identifier value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for RequestIdentifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Logical worker-epoch time encoded within JSON's interoperable integer range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ServerTimeMilliseconds(u64);

impl ServerTimeMilliseconds {
    /// Creates a validated logical server time.
    ///
    /// # Errors
    ///
    /// Returns an error when the value exceeds JSON's interoperable range.
    pub const fn new(value: u64) -> Result<Self, ValidationError> {
        if value > MAX_JSON_INTEGER {
            return Err(ValidationError::JsonIntegerTooLarge {
                field: "server_time_ms",
            });
        }
        Ok(Self(value))
    }

    /// Returns Unix milliseconds on the worker epoch's logical time scale.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ServerTimeMilliseconds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Diagnostic software version with bounded, control-free content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SoftwareVersion(String);

impl SoftwareVersion {
    /// Parses a diagnostic software version.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, oversized, or control-bearing values.
    pub fn parse(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_bounded_text(&value, "software version", MAX_SOFTWARE_VERSION_BYTES)?;
        Ok(Self(value))
    }

    /// Returns the diagnostic version text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SoftwareVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Safe bounded summary suitable for a pre-handshake protocol error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SafeSummary(String);

impl SafeSummary {
    /// Parses a safe protocol summary.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, oversized, or control-bearing text.
    pub fn parse(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_bounded_text(&value, "safe summary", MAX_SAFE_SUMMARY_BYTES)?;
        Ok(Self(value))
    }

    /// Returns the safe summary text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SafeSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Canonical lowercase `UUIDv7` used for diagnostic connection correlation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionIdentifier(Uuid);

impl ConnectionIdentifier {
    /// Parses a canonical lowercase `UUIDv7`.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is not canonical lowercase `UUIDv7`.
    pub fn parse(value: &str) -> Result<Self, ValidationError> {
        let parsed = Uuid::parse_str(value).map_err(|_| ValidationError::InvalidUuidV7 {
            field: "connection identifier",
        })?;
        if parsed.get_version_num() != 7
            || parsed.get_variant() != Variant::RFC4122
            || parsed.to_string() != value
        {
            return Err(ValidationError::InvalidUuidV7 {
                field: "connection identifier",
            });
        }
        Ok(Self(parsed))
    }

    /// Creates an identifier from an already generated `UUIDv7`.
    ///
    /// # Errors
    ///
    /// Returns an error when the UUID version is not 7.
    pub fn from_uuid(value: Uuid) -> Result<Self, ValidationError> {
        if value.get_version_num() != 7 || value.get_variant() != Variant::RFC4122 {
            return Err(ValidationError::InvalidUuidV7 {
                field: "connection identifier",
            });
        }
        Ok(Self(value))
    }

    /// Returns the UUID value.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for ConnectionIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for ConnectionIdentifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ConnectionIdentifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Inclusive contiguous protocol minor-version range for one major version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProtocolVersionRange {
    major: u16,
    min_minor: u16,
    max_minor: u16,
}

impl ProtocolVersionRange {
    /// Creates a validated contiguous range.
    ///
    /// # Errors
    ///
    /// Returns an error when the minor range is reversed.
    pub const fn new(major: u16, min_minor: u16, max_minor: u16) -> Result<Self, ValidationError> {
        if min_minor > max_minor {
            return Err(ValidationError::InvalidVersionRange);
        }
        Ok(Self {
            major,
            min_minor,
            max_minor,
        })
    }

    /// Returns the major version.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the lowest supported minor version.
    #[must_use]
    pub const fn min_minor(self) -> u16 {
        self.min_minor
    }

    /// Returns the highest supported minor version.
    #[must_use]
    pub const fn max_minor(self) -> u16 {
        self.max_minor
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtocolVersionRangeWire {
    major: u16,
    min_minor: u16,
    max_minor: u16,
}

impl<'de> Deserialize<'de> for ProtocolVersionRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ProtocolVersionRangeWire::deserialize(deserializer)?;
        Self::new(wire.major, wire.min_minor, wire.max_minor).map_err(serde::de::Error::custom)
    }
}

/// Selected protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    major: u16,
    minor: u16,
}

impl ProtocolVersion {
    /// Creates a selected protocol version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Returns the major version.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the minor version.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// Client component participating in the local protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientComponent {
    /// Command-line client.
    Cli,
    /// Terminal user interface.
    Tui,
    /// Future authenticated controller bridge.
    Controller,
}

/// Fixed worker target context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerTarget {
    /// System/rootful worker.
    System,
    /// Owning user worker.
    User,
}

/// Fixed systemd manager kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerKind {
    /// System manager.
    System,
    /// Owning user manager.
    User,
}

/// Typed operation capability negotiated during the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Discover workloads and summary status.
    Observe,
    /// Retrieve full status and inspect snapshots.
    Inspect,
    /// Query bounded historical logs.
    LogsQuery,
    /// Follow logs.
    LogsFollow,
    /// Retrieve fast metric snapshots.
    Metrics,
    /// Follow status/event streams.
    Events,
    /// Perform storage accounting.
    Storage,
    /// Perform lifecycle operations.
    Lifecycle,
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Observe => "observe",
            Self::Inspect => "inspect",
            Self::LogsQuery => "logs_query",
            Self::LogsFollow => "logs_follow",
            Self::Metrics => "metrics",
            Self::Events => "events",
            Self::Storage => "storage",
            Self::Lifecycle => "lifecycle",
        };
        formatter.write_str(value)
    }
}

/// Sorted duplicate-free capability set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    /// Builds a duplicate-free capability set.
    ///
    /// # Errors
    ///
    /// Returns an error when a capability appears more than once.
    pub fn new(values: impl IntoIterator<Item = Capability>) -> Result<Self, ValidationError> {
        let mut capabilities = BTreeSet::new();
        for capability in values {
            if !capabilities.insert(capability) {
                return Err(ValidationError::DuplicateCapability { capability });
            }
        }
        Ok(Self(capabilities))
    }

    /// Returns whether the set contains a capability.
    #[must_use]
    pub fn contains(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    /// Iterates in stable wire order.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    /// Returns whether the set has no capabilities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for CapabilitySet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.iter().collect::<Vec<_>>().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CapabilitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<Capability>::deserialize(deserializer)?;
        Self::new(values).map_err(serde::de::Error::custom)
    }
}

/// Lowercase hexadecimal SHA-256 manifest generation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ManifestGeneration(String);

impl ManifestGeneration {
    /// Parses a manifest generation.
    ///
    /// # Errors
    ///
    /// Returns an error unless the value is 64 lowercase hexadecimal characters.
    pub fn parse(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ValidationError::InvalidManifestGeneration);
        }
        Ok(Self(value))
    }

    /// Returns the generation text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ManifestGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}
