use serde::{Deserialize, Serialize};
use sha1::Digest;
use sha2::Sha256;

use crate::{
    certs::{ActiveKeyIdentity, Signature, Signer, Verifier},
    dht::{
        Key,
        auth::{AuthError, Authorizable, Evidence, TOKEN_EXP_SECONDS, TrustLevel},
    },
    types::{Hash32Bytes, UnixDate},
    utils::bqti::fetch_current_timestamp,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    peer: Vec<u8>,
    pow: Hash32Bytes,
    issued_at: UnixDate,
    exp_at: UnixDate,
    pub issuer: Vec<u8>,
    trust_level: TrustLevel,
    signature: Option<Signature>,
}

impl Token {
    pub(crate) fn new(peer: &[u8], pow: Hash32Bytes, level: TrustLevel) -> Self {
        let peer = peer.to_vec();
        let pre_allocated_capacity = peer.capacity();

        Self {
            peer,
            pow,
            issued_at: 0,
            exp_at: 0,
            issuer: Vec::with_capacity(pre_allocated_capacity),
            signature: None,
            trust_level: level,
        }
    }

    pub fn sender(&self) -> Key {
        Key::new(&self.peer)
    }

    pub fn trust_level(&self) -> &TrustLevel {
        &self.trust_level
    }

    fn calculate_hash(&self, issuer: &[u8]) -> Hash32Bytes {
        let mut hasher = Sha256::new();

        hasher.update(&self.peer);
        hasher.update(&self.pow);
        hasher.update(&self.exp_at.to_be_bytes());
        hasher.update(&self.issued_at.to_be_bytes());

        let trust_byte: u8 = match &self.trust_level {
            TrustLevel::Attested(report) => {
                let report_bytes = serde_bencode::to_bytes(report).unwrap();
                hasher.update(&report_bytes);
                0
            }
            TrustLevel::Unattested => 1,
            TrustLevel::Rejected => 2,
        };

        hasher.update(&[trust_byte]);
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
    fn verify_for(&self, pub_key: &[u8]) -> bool {
        if &self.peer != pub_key {
            return false;
        }

        self.verify()
    }

    fn verify(&self) -> bool {
        if self.is_expired() {
            return false;
        }

        let Some(signature) = self.signature.clone() else {
            return false;
        };

        let secret = self.calculate_hash(&self.issuer);
        ActiveKeyIdentity::verify(&self.issuer, &secret, &signature)
    }

    fn is_expired(&self) -> bool {
        if self.exp_at == 0 {
            return true;
        }

        fetch_current_timestamp() > self.exp_at
    }
}
