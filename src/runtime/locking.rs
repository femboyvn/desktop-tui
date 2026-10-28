use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::io::Read;
use zeroize::Zeroizing;

pub const MAX_PASSWORD_BYTES: usize = 1024;
#[derive(Clone, PartialEq, Eq)]
pub struct LockCredential {
    hash: Zeroizing<String>,
}
impl LockCredential {
    pub fn new(password: &str, confirmation: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(!password.is_empty(), "Enter a password before locking.");
        anyhow::ensure!(
            password.len() <= MAX_PASSWORD_BYTES,
            "Password is too long."
        );
        anyhow::ensure!(
            password == confirmation,
            "Passwords do not match. Enter both again."
        );
        let mut salt = [0u8; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut salt)?;
        let salt = SaltString::encode_b64(&salt)
            .map_err(|error| anyhow::anyhow!("Could not prepare a lock password: {error}"))?;
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| anyhow::anyhow!("Could not prepare a lock password: {error}"))?
            .to_string();
        Ok(Self {
            hash: Zeroizing::new(hash),
        })
    }
    pub fn verify(&self, password: &str) -> bool {
        if password.len() > MAX_PASSWORD_BYTES {
            return false;
        }
        PasswordHash::new(&self.hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    }
}
impl std::fmt::Debug for LockCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LockCredential([redacted])")
    }
}
impl TryFrom<String> for LockCredential {
    type Error = anyhow::Error;
    fn try_from(encoded: String) -> anyhow::Result<Self> {
        let encoded = Zeroizing::new(encoded);
        anyhow::ensure!(encoded.len() <= 256, "Invalid saved lock credential.");
        let hash = PasswordHash::new(&encoded)
            .map_err(|_| anyhow::anyhow!("Invalid saved lock credential."))?;
        // PHC parameters control the verifier's actual work. Accept only our bounded format.
        let mut salt_bytes = [0u8; 64];
        let salt_length = hash
            .salt
            .and_then(|salt| salt.decode_b64(&mut salt_bytes).ok())
            .map(|salt| salt.len());
        anyhow::ensure!(
            hash.algorithm.as_str() == "argon2id"
                && hash.version == Some(19)
                && hash.params.as_str() == "m=19456,t=2,p=1"
                && salt_length == Some(16)
                && hash.hash.is_some_and(|output| output.len() == 32),
            "Unsupported saved lock credential. Expected Argon2id v19 with the application's standard parameters."
        );
        Ok(Self { hash: encoded })
    }
}
impl Serialize for LockCredential {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hash)
    }
}
impl<'de> Deserialize<'de> for LockCredential {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_verifiers_reject_unknown_algorithms_and_unbounded_work() {
        let credential = LockCredential::new("saved", "saved").unwrap();
        let encoded = credential.hash.to_string();
        for bad in [
            encoded.replace("argon2id", "argon2i"),
            encoded.replace("v=19", "v=16"),
            encoded.replace("m=19456", "m=4294967295"),
            encoded.replace("t=2", "t=4294967295"),
            encoded.replace("p=1", "p=8"),
            "plaintext".to_owned(),
        ] {
            assert!(LockCredential::try_from(bad).is_err());
        }
        assert!(LockCredential::try_from(encoded).unwrap().verify("saved"));
    }

    #[test]
    fn credential_requires_confirmation_and_verifies_only_the_same_password() {
        assert!(LockCredential::new("", "").is_err());
        assert!(LockCredential::new("one", "two").is_err());
        assert!(
            LockCredential::new(
                &"x".repeat(MAX_PASSWORD_BYTES + 1),
                &"x".repeat(MAX_PASSWORD_BYTES + 1)
            )
            .is_err()
        );
        let credential = LockCredential::new("local 🔒 pass", "local 🔒 pass").unwrap();
        assert!(credential.verify("local 🔒 pass"));
        assert!(!credential.verify("wrong"));
        assert!(!credential.verify(""));
        let another = LockCredential::new("local 🔒 pass", "local 🔒 pass").unwrap();
        assert_ne!(*credential.hash, *another.hash);
        assert!(credential.hash.starts_with("$argon2id$"));
    }
}
