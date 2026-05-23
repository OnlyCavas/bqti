use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;
use thiserror::Error;

pub const MAIN_CA_PUBLIC_KEY: [u8; 32] = [
    0x38, 0x6f, 0x42, 0x54, 0x99, 0xc3, 0x10, 0xcc, 0xd6, 0x8f, 0xa8, 0x22, 0x97, 0x08, 0xef, 0x09,
    0x1d, 0x16, 0x65, 0x4d, 0x13, 0x81, 0x30, 0xea, 0xd6, 0x22, 0xb9, 0x6d, 0x14, 0x29, 0x7c, 0x69,
];

const MANIFEST_URL: &str = "https://onlycavas.github.io/bqti/manifest.json";
const MANIFEST_SIG_URL: &str = "https://onlycavas.github.io/bqti/manifest.sig";

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("invalid signature")]
    InvalidSignature,

    #[error("version mismatch: expected {expected}, got {got}")]
    VersionMismatch { expected: String, got: String },

    #[error("invalid hash")]
    InvalidHash,
}

pub type Result<T> = std::result::Result<T, ManifestError>;

#[derive(Deserialize)]
pub struct Manifest {
    version: String,
    enclave_hash: String,
}

impl Manifest {
    async fn get() -> Result<Self> {
        let web_manifest = reqwest::get(MANIFEST_URL).await?;
        let manifest = web_manifest.bytes().await?;

        let web_signature = reqwest::get(MANIFEST_SIG_URL).await?;
        let signature = web_signature.bytes().await?;

        UnparsedPublicKey::new(&ED25519, MAIN_CA_PUBLIC_KEY)
            .verify(&manifest, &signature)
            .map_err(|_| ManifestError::InvalidSignature)?;

        let manifest: Manifest =
            serde_json::from_slice(&manifest).map_err(|_| ManifestError::InvalidHash)?;

        Ok(manifest)
    }

    pub async fn get_enclave_hash(version: &str) -> Result<[u8; 64]> {
        let manifest = Self::get().await?;

        if manifest.version != version {
            return Err(ManifestError::VersionMismatch {
                expected: version.to_string(),
                got: manifest.version,
            });
        }

        let mut enclave_hash = [0u8; 64];
        hex::decode_to_slice(manifest.enclave_hash, &mut enclave_hash)
            .map_err(|_| ManifestError::InvalidHash)?;

        Ok(enclave_hash)
    }
}
