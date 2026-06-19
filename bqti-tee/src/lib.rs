use ed25519_dalek::{Signature, VerifyingKey, hazmat::raw_verify};
use serde_big_array::BigArray;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnclaveReport {
    #[serde(with = "BigArray")]
    pub hash: [u8; 64],

    pub data_len: u64,

    #[serde(with = "BigArray")]
    pub data: [u8; 1024],

    #[serde(with = "BigArray")]
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmReport {
    #[serde(with = "BigArray")]
    pub hash: [u8; 64],

    pub public_key: [u8; 32],

    #[serde(with = "BigArray")]
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeystoneAttestReport {
    pub enclave: EnclaveReport,
    pub sm: SmReport,
    pub dev_public_key: [u8; 32],
}

pub const SANCTUM_DEV_PUBLIC_KEY: [u8; 32] = [
    0x0f, 0xaa, 0xd4, 0xff, 0x01, 0x17, 0x85, 0x83, 0xba, 0xa5, 0x88, 0x96, 0x6f, 0x7c, 0x1f, 0xf3,
    0x25, 0x64, 0xdd, 0x17, 0xd7, 0xdc, 0x2b, 0x46, 0xcb, 0x50, 0xa8, 0x4a, 0x69, 0x27, 0x0b, 0x4c,
];

fn verify_keccak_ed25519(public_key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(pub_key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };

    let sig = Signature::from_bytes(signature);
    raw_verify::<Sha3_512>(&pub_key, message, &sig).is_ok()
}

impl KeystoneAttestReport {
    pub fn verify(&self, nonce: &[u8], expected_hash: &[u8], trusted_dev_key: &[u8; 32]) -> bool {
        if trusted_dev_key != &self.dev_public_key {
            return false;
        }

        let sm_msg: Vec<u8> = [self.sm.hash.as_ref(), self.sm.public_key.as_ref()].concat();
        if !verify_keccak_ed25519(trusted_dev_key, &sm_msg, &self.sm.signature) {
            return false;
        }

        let data_len = self.enclave.data_len as usize;
        let enclave_msg: Vec<u8> = [
            self.enclave.hash.as_ref(),
            self.enclave.data_len.to_le_bytes().as_ref(),
            &self.enclave.data[..data_len],
        ]
        .concat();

        if !verify_keccak_ed25519(&self.sm.public_key, &enclave_msg, &self.enclave.signature) {
            return false;
        }

        if self.enclave.hash.as_ref() != expected_hash {
            return false;
        }

        let nonce_len = nonce.len().min(data_len);
        nonce_len > 0 && self.enclave.data[..nonce_len] == nonce[..nonce_len]
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "tee", content = "report")]
pub enum AttestReport {
    Keystone(KeystoneAttestReport),
}

impl AttestReport {
    pub fn verify(&self, nonce: &[u8], expected_hash: &[u8]) -> bool {
        match self {
            AttestReport::Keystone(r) => r.verify(nonce, expected_hash, &SANCTUM_DEV_PUBLIC_KEY),
        }
    }
}

#[cfg(feature = "tee")]
mod keystone;

#[cfg(feature = "tee")]
pub use keystone::*;
use serde::{Deserialize, Serialize};
use sha3::Sha3_512;

pub fn tee_available() -> bool {
    #[cfg(feature = "tee")]
    {
        std::path::Path::new("/dev/keystone_enclave").exists()
    }
    #[cfg(not(feature = "tee"))]
    {
        false
    }
}
