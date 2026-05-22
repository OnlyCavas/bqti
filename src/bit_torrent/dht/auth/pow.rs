use std::{net::IpAddr, sync::Arc};

#[cfg(feature = "tee")]
use bqti_tee::{Tee, TeeExecute};

use rand::Rng;
use sha1::Digest;
use sha2::Sha256;

use crate::{
    certs::{Signature, Signer, SoftwareKeyIdentity, Verifier},
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
                });
            }

            nonce = nonce.wrapping_add(1);
        }
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

        let pow = PoW {
            value: tee_pow.hash,
            challange: challenge,
            nonce: tee_pow.nonce,
            signature: Some(tee_pow.sig.to_vec()),
            difficulty,
        };

        return Ok(pow);
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
    pub difficulty: u32,
}

impl PoW {
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

// impl Challenge for PoW {
//     fn generate(pub_key: &[u8], challange: u32, difficulty: u32) -> PoW {
//         let mut prof_of_work: [u8; 32];
//         let mut nonce: u32 = 0;
//
//         loop {
//             prof_of_work = Self::calculate(pub_key, challange, nonce);
//
//             if Self::validate(&prof_of_work, difficulty) {
//                 return Self {
//                     value: prof_of_work,
//                     challange,
//                     nonce,
//                     signature: None,
//                     difficulty,
//                 };
//             }
//
//             nonce = nonce.wrapping_add(1);
//         }
//     }
// }

// impl Evidence for PoW {
//     fn sign(&mut self, signer: &impl Signer) -> Result<(), AuthError> {
//         let signature = signer
//             .sign(&self.value)
//             .map_err(|_| AuthError::UnAuthorized())?;
//
//         self.signature = Some(signature);
//         Ok(())
//     }
// }

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

        SoftwareKeyIdentity::verify(pub_key, &self.value, &signature)
    }
}
