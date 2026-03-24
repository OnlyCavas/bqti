use sha1::{Digest, Sha1};
use sha2::Sha256;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sha1Hash([u8; 20]);

impl Sha1Hash {
    pub fn digest(data: &[u8]) -> Self {
        let mut hasher = Sha1::new();
        hasher.update(data);
        Self(hasher.finalize().into())
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    pub fn to_sha256_key(&self) -> Sha256Hash {
        Sha256Hash::digest(&self.0)
    }
}

impl From<[u8; 20]> for Sha1Hash {
    fn from(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for Sha1Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Display for Sha1Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sha256Hash([u8; 32]);

impl Sha256Hash {
    pub fn digest(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        Self(hasher.finalize().into())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for Sha256Hash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for Sha256Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Display for Sha256Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}
