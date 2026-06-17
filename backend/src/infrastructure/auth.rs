//! Auth — argon2 password hashing + JWT (stateless bearer token)
//!
//! JWT secret comes from env `JWT_SECRET`; if absent, a random one is generated at boot
//! (token is only valid for the duration of the process — suitable for dev/single-machine use)

use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};

use crate::domain::models::Claims;
use crate::domain::ports::{DomainError, DomainResult};

const TOKEN_TTL_DAYS: i64 = 30;

#[derive(Clone)]
pub struct Auth {
    enc: EncodingKey,
    dec: DecodingKey,
}

impl Auth {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            enc: EncodingKey::from_secret(secret),
            dec: DecodingKey::from_secret(secret),
        }
    }

    /// Generate a random hex string (for JWT secret / initial password)
    pub fn random_hex(bytes: usize) -> String {
        let mut buf = vec![0u8; bytes];
        OsRng.fill_bytes(&mut buf);
        buf.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Hash a password with argon2id (for storage in DB)
    pub fn hash_password(password: &str) -> DomainResult<String> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| DomainError::Auth(format!("hash failed: {e}")))
    }

    /// Verify a password against its stored hash
    pub fn verify_password(password: &str, hash: &str) -> bool {
        match PasswordHash::new(hash) {
            Ok(parsed) => Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok(),
            Err(_) => false,
        }
    }

    /// Issue a JWT for a user
    pub fn issue(&self, user_id: i64, email: &str) -> DomainResult<String> {
        let now = Utc::now();
        let claims = Claims {
            sub: user_id,
            email: email.to_string(),
            iat: now.timestamp(),
            exp: (now + Duration::days(TOKEN_TTL_DAYS)).timestamp(),
        };
        jsonwebtoken::encode(&Header::default(), &claims, &self.enc)
            .map_err(|e| DomainError::Auth(format!("token issuance failed: {e}")))
    }

    /// Verify a JWT and return its Claims
    pub fn verify(&self, token: &str) -> DomainResult<Claims> {
        jsonwebtoken::decode::<Claims>(token, &self.dec, &Validation::default())
            .map(|d| d.claims)
            .map_err(|e| DomainError::Auth(format!("invalid token: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_roundtrip() {
        let h = Auth::hash_password("s3cret!").unwrap();
        assert!(Auth::verify_password("s3cret!", &h));
        assert!(!Auth::verify_password("wrong", &h));
    }

    #[test]
    fn jwt_roundtrip() {
        let a = Auth::new(b"test-secret-key");
        let tok = a.issue(42, "x@y.z").unwrap();
        let claims = a.verify(&tok).unwrap();
        assert_eq!(claims.sub, 42);
        assert_eq!(claims.email, "x@y.z");
    }

    #[test]
    fn jwt_rejects_tampered() {
        let a = Auth::new(b"secret-a");
        let b = Auth::new(b"secret-b");
        let tok = a.issue(1, "a@b.c").unwrap();
        assert!(b.verify(&tok).is_err());
    }
}
