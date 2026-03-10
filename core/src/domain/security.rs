//! Security types — capability tokens and revocation lists.

use crate::domain::id::DeviceId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A capability token granting specific permissions to a device.
///
/// Tokens are time-limited and carry a unique nonce for revocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Unique nonce to identify this token (for revocation).
    pub nonce: Uuid,
    /// The device this token was issued to.
    pub device_id: DeviceId,
    /// Human-readable list of permissions.
    pub permissions: Vec<String>,
    /// When this token was issued.
    pub issued_at: DateTime<Utc>,
    /// When this token expires.
    pub expires_at: DateTime<Utc>,
    /// Cryptographic signature over the token data.
    pub signature: Vec<u8>,
}

impl CapabilityToken {
    /// Check if this token has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if this token has a specific permission.
    pub fn has_permission(&self, perm: &str) -> bool {
        self.permissions.iter().any(|p| p == perm)
    }
}

/// A list of revoked tokens and devices.
///
/// Used to invalidate previously granted capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RevocationList {
    /// Nonces of revoked tokens.
    pub revoked_nonces: Vec<Uuid>,
    /// Device IDs that are fully revoked.
    pub revoked_devices: Vec<DeviceId>,
}

impl RevocationList {
    /// Create an empty revocation list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Revoke a specific token by nonce.
    pub fn revoke_nonce(&mut self, nonce: Uuid) {
        if !self.revoked_nonces.contains(&nonce) {
            self.revoked_nonces.push(nonce);
        }
    }

    /// Revoke all tokens for a device.
    pub fn revoke_device(&mut self, device_id: DeviceId) {
        if !self.revoked_devices.contains(&device_id) {
            self.revoked_devices.push(device_id);
        }
    }

    /// Check if a token nonce has been revoked.
    pub fn is_nonce_revoked(&self, nonce: &Uuid) -> bool {
        self.revoked_nonces.contains(nonce)
    }

    /// Check if a device has been fully revoked.
    pub fn is_device_revoked(&self, device_id: &DeviceId) -> bool {
        self.revoked_devices.contains(device_id)
    }

    /// Check if a token is revoked (by nonce or device).
    pub fn is_token_revoked(&self, token: &CapabilityToken) -> bool {
        self.is_nonce_revoked(&token.nonce) || self.is_device_revoked(&token.device_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(expires_in_secs: i64) -> CapabilityToken {
        CapabilityToken {
            nonce: Uuid::new_v4(),
            device_id: DeviceId::new(),
            permissions: vec!["read".into(), "write".into()],
            issued_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::seconds(expires_in_secs),
            signature: vec![0u8; 64],
        }
    }

    #[test]
    fn valid_token_not_expired() {
        let token = make_token(3600); // 1 hour from now
        assert!(!token.is_expired());
    }

    #[test]
    fn expired_token_detected() {
        let token = make_token(-1); // 1 second ago
        assert!(token.is_expired());
    }

    #[test]
    fn token_permission_check() {
        let token = make_token(3600);
        assert!(token.has_permission("read"));
        assert!(token.has_permission("write"));
        assert!(!token.has_permission("admin"));
    }

    #[test]
    fn nonce_uniqueness() {
        let t1 = make_token(3600);
        let t2 = make_token(3600);
        assert_ne!(t1.nonce, t2.nonce);
    }

    #[test]
    fn revocation_list_nonce() {
        let mut rl = RevocationList::new();
        let token = make_token(3600);
        assert!(!rl.is_nonce_revoked(&token.nonce));

        rl.revoke_nonce(token.nonce);
        assert!(rl.is_nonce_revoked(&token.nonce));
    }

    #[test]
    fn revocation_list_device() {
        let mut rl = RevocationList::new();
        let device = DeviceId::new();
        assert!(!rl.is_device_revoked(&device));

        rl.revoke_device(device);
        assert!(rl.is_device_revoked(&device));
    }

    #[test]
    fn revocation_list_token_check() {
        let mut rl = RevocationList::new();
        let token = make_token(3600);
        assert!(!rl.is_token_revoked(&token));

        rl.revoke_nonce(token.nonce);
        assert!(rl.is_token_revoked(&token));
    }
}
