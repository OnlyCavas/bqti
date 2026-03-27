use std::net::IpAddr;

use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    bit_torrent::certs::{KeyIdentity, Signature, Signer, Verifier},
    dht::Key,
    types::{Hash32Bytes, UnixDate},
    utils::bqti::fetch_current_timestamp,
};

pub const TOKEN_EXP_SECONDS: UnixDate = 30 * 60;
pub const DIFFICULTY: u32 = 16;

pub trait Challange {
    fn generate(pub_key: &[u8], challange: u32, difficulty: u32) -> Self;
}

pub trait Evidence {
    fn sign(&mut self, signer: &impl Signer) -> Result<(), AuthError>;
}

pub trait Authorizable: Evidence {
    fn verify(&self, pub_key: &[u8]) -> bool;
    fn is_expired(&self) -> bool;
}

pub trait ChallangeProof {
    fn verify(&self, pub_key: &[u8]) -> bool;
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("failed to prove prof of work")]
    UnAuthorized(),

    #[error("authentication failed, token not trusted")]
    InvalidToken(),

    #[error("public key from responder doesn't match the signature, handshake failed")]
    RoguePeer(),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    peer: Vec<u8>,
    pow: Hash32Bytes,
    issued_at: UnixDate,
    exp_at: UnixDate,
    pub issuer: Vec<u8>,
    signature: Option<Signature>,
}

impl Token {
    pub fn new(peer: &[u8], pow: Hash32Bytes) -> Self {
        let peer = peer.to_vec();
        let pre_allocated_capacity = peer.capacity();

        Self {
            peer,
            pow,
            issued_at: 0,
            exp_at: 0,
            issuer: Vec::with_capacity(pre_allocated_capacity),
            signature: None,
        }
    }

    pub fn sender(&self) -> Key {
        Key::new(&self.peer)
    }

    fn calculate_hash(&self, issuer: &[u8]) -> Hash32Bytes {
        let mut hasher = Sha256::new();

        hasher.update(&self.peer);
        hasher.update(&self.pow);
        hasher.update(&self.exp_at.to_be_bytes());
        hasher.update(&self.issued_at.to_be_bytes());
        hasher.update(&issuer);

        hasher.finalize().into()
    }
}

impl Evidence for Token {
    fn sign(&mut self, signer: &impl Signer) -> Result<(), AuthError> {
        let current_time = fetch_current_timestamp();
        self.issued_at = current_time;
        self.exp_at = self.issued_at + TOKEN_EXP_SECONDS;

        let secret = self.calculate_hash(signer.pub_key());
        let signature = signer
            .sign(&secret)
            .map_err(|_| AuthError::UnAuthorized())?;

        self.issuer = signer.pub_key().to_vec();
        self.signature = Some(signature);

        Ok(())
    }
}

impl Authorizable for Token {
    fn verify(&self, pub_key: &[u8]) -> bool {
        if &self.peer != pub_key {
            return false;
        }

        if self.is_expired() {
            return false;
        }

        let Some(signature) = self.signature.clone() else {
            return false;
        };

        let secret = self.calculate_hash(&self.issuer);
        KeyIdentity::verify(&self.issuer, &secret, &signature)
    }

    fn is_expired(&self) -> bool {
        if self.exp_at == 0 {
            return true;
        }

        fetch_current_timestamp() > self.exp_at
    }
}

#[derive(Clone, Copy)]
pub struct SecretSalt(Hash32Bytes);

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

impl Challange for PoW {
    fn generate(pub_key: &[u8], challange: u32, difficulty: u32) -> PoW {
        let mut prof_of_work: [u8; 32];
        let mut nonce: u32 = 0;

        loop {
            prof_of_work = Self::calculate(pub_key, challange, nonce);

            if Self::validate(&prof_of_work, difficulty) {
                return Self {
                    value: prof_of_work,
                    challange,
                    nonce,
                    signature: None,
                    difficulty,
                };
            }

            nonce = nonce.wrapping_add(1);
        }
    }
}

impl Evidence for PoW {
    fn sign(&mut self, signer: &impl Signer) -> Result<(), AuthError> {
        let signature = signer
            .sign(&self.value)
            .map_err(|_| AuthError::UnAuthorized())?;

        self.signature = Some(signature);
        Ok(())
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

        KeyIdentity::verify(pub_key, &self.value, &signature)
    }
}
