use std::{net::IpAddr, sync::Arc};

use bqti_tee::AttestReport;
#[cfg(feature = "tee")]
use bqti_tee::{Tee, TeeExecute};

use rand::Rng;
use sha1::Digest;
use sha2::Sha256;

use crate::{
    certs::{ActiveKeyIdentity, Signature, Signer, Verifier},
    dht::auth::{AuthError, ChallangeProof},
    types::Hash32Bytes,
};

#[cfg(feature = "tee")]
pub type ActiveProver = TeeProver;

#[cfg(not(feature = "tee"))]
pub type ActiveProver = SoftwareProver;

#[allow(unused_variables)]
pub fn make_prover(signer: Arc<dyn Signer>) -> ActiveProver {
    #[cfg(feature = "tee")]
    {
        TeeProver::new(Arc::new(bqti_tee::Tee::new()))
    }
    #[cfg(not(feature = "tee"))]
    {
        SoftwareProver::new(signer)
    }
}

#[derive(Clone, Copy)]
pub struct SecretSalt(Hash32Bytes);

pub trait ProveChallenge {
    fn prove(&self, public_key: &[u8], challenge: u32, difficulty: u32) -> Result<PoW, AuthError>;
    fn sign(&self, data: &[u8]) -> Result<Signature, AuthError>;
}

#[cfg(not(feature = "tee"))]
type SoftwareSigner = Arc<dyn Signer + Send + Sync>;

#[cfg(not(feature = "tee"))]
pub struct SoftwareProver {
    signer: SoftwareSigner,
}

#[cfg(not(feature = "tee"))]
impl SoftwareProver {
    pub fn new(signer: SoftwareSigner) -> Self {
        Self { signer }
    }
}

#[cfg(not(feature = "tee"))]
impl ProveChallenge for SoftwareProver {
    fn prove(&self, public_key: &[u8], challenge: u32, difficulty: u32) -> Result<PoW, AuthError> {
        let mut nonce = 0;

        loop {
            let hash = PoW::calculate(public_key, challenge, nonce);

            if PoW::validate(&hash, difficulty) {
                let signature = self.signer.sign(&hash)?;

                return Ok(PoW {
                    value: hash,
                    challange: challenge,
                    nonce,
                    signature: Some(signature),
                    difficulty,
                    attestation: None,
                });
            }

            nonce = nonce.wrapping_add(1);
        }
    }

    fn sign(&self, data: &[u8]) -> Result<Signature, AuthError> {
        let sig = self.signer.sign(data)?;
        Ok(sig)
    }
}

#[cfg(feature = "tee")]
pub struct TeeProver {
    tee: Arc<Tee>,
}

#[cfg(feature = "tee")]
impl TeeProver {
    pub fn new(tee: Arc<Tee>) -> Self {
        Self { tee }
    }
}

#[cfg(feature = "tee")]
impl ProveChallenge for TeeProver {
    fn prove(&self, _public_key: &[u8], challenge: u32, difficulty: u32) -> Result<PoW, AuthError> {
        let tee_pow = self.tee.pow(challenge, difficulty)?;
        let report = self.tee.attest(&tee_pow.hash)?;

        let pow = PoW::with_attestation(
            tee_pow.hash,
            challenge,
            tee_pow.nonce,
            difficulty,
            tee_pow.sig.to_vec(),
            report,
        );

        return Ok(pow);
    }

    fn sign(&self, data: &[u8]) -> Result<Signature, AuthError> {
        let tee_sign = self.tee.sign(data)?;
        Ok(tee_sign.to_vec())
    }
}

impl SecretSalt {
    pub fn new() -> Self {
        let mut salt = [0u8; 32];
        rand::rng().fill_bytes(&mut salt);

        Self(salt)
    }

    pub fn calculate_challenge(
        pub_key: &[u8],
        sender_ip: &IpAddr,
        secret_salt: &SecretSalt,
    ) -> u32 {
        let mut hasher = Sha256::new();
        hasher.update(pub_key);

        match sender_ip {
            IpAddr::V4(v4) => hasher.update(&v4.octets()),
            IpAddr::V6(v6) => hasher.update(&v6.octets()),
        }

        hasher.update(secret_salt.0);
        let hash = hasher.finalize();

        u32::from_be_bytes(hash[0..4].try_into().expect("SHA256 is 32 bytes"))
    }
}

pub struct PoW {
    pub value: Hash32Bytes,
    pub challange: u32,
    pub nonce: u32,
    pub signature: Option<Signature>,
    pub attestation: Option<AttestReport>,
    pub difficulty: u32,
}

impl PoW {
    pub fn with_attestation(
        value: Hash32Bytes,
        challange: u32,
        nonce: u32,
        difficulty: u32,
        signature: Signature,
        attestation: AttestReport,
    ) -> Self {
        Self {
            value,
            challange,
            nonce,
            signature: Some(signature),
            attestation: Some(attestation),
            difficulty,
        }
    }

    pub fn new(
        value: Hash32Bytes,
        challange: u32,
        nonce: u32,
        difficulty: u32,
        signature: Signature,
    ) -> Self {
        Self {
            value,
            challange,
            nonce,
            signature: Some(signature),
            attestation: None,
            difficulty,
        }
    }

    fn calculate(pub_key: &[u8], challenge: u32, nonce: u32) -> [u8; 32] {
        let mut hasher = Sha256::new();

        hasher.update(pub_key);
        hasher.update(&challenge.to_be_bytes());
        hasher.update(&nonce.to_be_bytes());

        hasher.finalize().into()
    }

    fn validate(hash: &[u8; 32], difficulty: u32) -> bool {
        let target_zeros = difficulty as usize;
        let full_bytes = target_zeros / 8;
        let remaining_bits = target_zeros % 8;

        for i in 0..full_bytes {
            if hash[i] != 0 {
                return false;
            }
        }

        if remaining_bits > 0 {
            let mask = 0xFFu8 << (8 - remaining_bits);

            if (hash[full_bytes] & mask) != 0 {
                return false;
            }
        }

        true
    }
}

impl ChallangeProof for PoW {
    fn verify(&self, pub_key: &[u8]) -> bool {
        let Some(signature) = self.signature.clone() else {
            return false;
        };

        let expected_hash = Self::calculate(pub_key, self.challange, self.nonce);

        if expected_hash != self.value {
            return false;
        }

        if !Self::validate(&expected_hash, self.difficulty) {
            return false;
        }

        ActiveKeyIdentity::verify(pub_key, &self.value, &signature)
    }
}
