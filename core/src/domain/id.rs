//! Core identity types for HyprDrive
//!
//! Each ID is a 32-byte value that uniquely identifies a domain entity.
//! `ObjectId` is content-addressed (BLAKE3 hash), all others use UUID v4.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// A 32-byte identity wrapper used for all HyprDrive IDs.
/// Content-addressed for Objects, UUID-based for all others.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObjectId([u8; 32]);

impl ObjectId {
    /// Create a new ObjectId from a raw 32-byte array.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create a content-addressed ObjectId by BLAKE3-hashing data.
    /// Deterministic: same input always produces the same ObjectId.
    pub fn from_blake3(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Self(*hash.as_bytes())
    }

    /// Return the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({})", self)
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl FromStr for ObjectId {
    type Err = IdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(IdParseError::InvalidLength {
                expected: 64,
                got: s.len(),
            });
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| IdParseError::InvalidHex)?;
        }
        Ok(Self(bytes))
    }
}

/// Error parsing an ID from a hex string.
#[derive(Debug, thiserror::Error)]
pub enum IdParseError {
    /// Hex string has wrong length.
    #[error("expected {expected} hex chars, got {got}")]
    InvalidLength {
        /// Expected number of hex characters.
        expected: usize,
        /// Actual number of hex characters.
        got: usize,
    },
    /// Hex string contains invalid characters.
    #[error("invalid hex character")]
    InvalidHex,
}

/// Generate a UUID-based 32-byte ID (16 bytes UUID + 16 bytes zero padding).
fn uuid_to_32_bytes() -> [u8; 32] {
    let uuid = uuid::Uuid::new_v4();
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid.as_bytes());
    bytes
}

// Macro to generate UUID-based ID types without code duplication.
macro_rules! define_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Create a new random ID.
            pub fn new() -> Self {
                Self(uuid_to_32_bytes())
            }

            /// Create from raw bytes.
            pub fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            /// Return the raw bytes.
            pub fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                for byte in &self.0 {
                    write!(f, "{:02x}", byte)?;
                }
                Ok(())
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.len() != 64 {
                    return Err(IdParseError::InvalidLength {
                        expected: 64,
                        got: s.len(),
                    });
                }
                let mut bytes = [0u8; 32];
                for i in 0..32 {
                    bytes[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                        .map_err(|_| IdParseError::InvalidHex)?;
                }
                Ok(Self(bytes))
            }
        }
    };
}

define_id!(LocationId, "Identifies a file/folder location within a volume.");
define_id!(VolumeId, "Identifies a mounted volume (drive, partition, cloud bucket).");
define_id!(LibraryId, "Identifies a user library (collection of indexed locations).");
define_id!(DeviceId, "Identifies a device in the HyprDrive network.");
define_id!(TagId, "Identifies a semantic tag.");

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn object_id_from_bytes_display_roundtrip() {
        let bytes = [42u8; 32];
        let id = ObjectId::from_bytes(bytes);
        let hex = id.to_string();
        let parsed: ObjectId = hex.parse().ok().unwrap(); // unwrap intentionally in test-only
        assert_eq!(id, parsed);
    }

    #[test]
    fn object_id_from_blake3_is_deterministic() {
        let data = b"hello world";
        let id1 = ObjectId::from_blake3(data);
        let id2 = ObjectId::from_blake3(data);
        assert_eq!(id1, id2);
    }

    #[test]
    fn object_id_from_blake3_differs_for_different_data() {
        let id1 = ObjectId::from_blake3(b"hello");
        let id2 = ObjectId::from_blake3(b"world");
        assert_ne!(id1, id2);
    }

    #[test]
    fn all_id_types_serde_roundtrip() {
        let obj = ObjectId::from_blake3(b"test");
        let json = serde_json::to_string(&obj).ok().unwrap(); // unwrap intentionally in test-only
        let back: ObjectId = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(obj, back);

        let loc = LocationId::new();
        let json = serde_json::to_string(&loc).ok().unwrap();
        let back: LocationId = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(loc, back);

        let vol = VolumeId::new();
        let json = serde_json::to_string(&vol).ok().unwrap();
        let back: VolumeId = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(vol, back);

        let lib = LibraryId::new();
        let json = serde_json::to_string(&lib).ok().unwrap();
        let back: LibraryId = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(lib, back);

        let dev = DeviceId::new();
        let json = serde_json::to_string(&dev).ok().unwrap();
        let back: DeviceId = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(dev, back);

        let tag = TagId::new();
        let json = serde_json::to_string(&tag).ok().unwrap();
        let back: TagId = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(tag, back);
    }

    #[test]
    fn object_id_works_as_hash_key() {
        let id = ObjectId::from_blake3(b"test");
        let mut map = HashMap::new();
        map.insert(id, "value");
        assert_eq!(map.get(&id), Some(&"value"));
    }

    #[test]
    fn uuid_based_ids_are_unique() {
        let a = LocationId::new();
        let b = LocationId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn from_str_rejects_invalid_length() {
        let result: Result<ObjectId, _> = "abcd".parse();
        assert!(result.is_err());
    }

    #[test]
    fn from_str_rejects_invalid_hex() {
        let bad = "zz".repeat(32);
        let result: Result<ObjectId, _> = bad.parse();
        assert!(result.is_err());
    }
}
