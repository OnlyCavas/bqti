use std::{collections::HashMap, io::Cursor};

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    certs::{ActiveKeyIdentity, KeyIdentity, Signature, Signer, Verifier},
    dht::{
        Key,
        auth::{AuthError, Authorizable, Evidence, TOKEN_EXP_SECONDS, TrustLevel},
    },
    types::{Hash32Bytes, UnixDate},
    utils::bqti::fetch_current_timestamp,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    swarm_id: Hash32Bytes,
    peer: Vec<u8>,
    pow: Hash32Bytes,
    issued_at: UnixDate,
    exp_at: UnixDate,
    pub issuer: Vec<u8>,
    issuer_ca_cert: Vec<u8>,
    issuer_ca_cert_sig: Vec<u8>,
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
            swarm_id: [0u8; 32],
            issuer_ca_cert: Vec::new(),
            issuer_ca_cert_sig: Vec::new(),
        }
    }

    pub fn hash(&self) -> Hash32Bytes {
        self.calculate_hash(&self.issuer)
    }

    pub fn sender(&self) -> Key {
        Key::new(&self.peer)
    }

    pub fn trust_level(&self) -> &TrustLevel {
        &self.trust_level
    }

    pub(crate) fn bind_swarm(
        &mut self,
        swarm_id: &Hash32Bytes,
        ca_cert: &ActiveKeyIdentity,
        ca_cert_sig: &[u8],
    ) {
        self.issuer_ca_cert = ca_cert.cert_der().to_vec();
        self.issuer_ca_cert_sig = ca_cert_sig.to_vec();
        self.swarm_id = *swarm_id;
    }

    fn calculate_hash(&self, issuer: &[u8]) -> Hash32Bytes {
        let mut hasher = Sha256::new();

        hasher.update(&self.peer);
        hasher.update(&self.pow);
        hasher.update(&self.exp_at.to_be_bytes());
        hasher.update(&self.issued_at.to_be_bytes());
        hasher.update(&[self.trust_level as u8]);
        hasher.update(&self.swarm_id);
        hasher.update(&self.issuer_ca_cert);
        hasher.update(&self.issuer_ca_cert_sig);
        hasher.update(issuer);

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

    fn verify_pgp(&self, pgp_keys: &HashMap<Hash32Bytes, SignedPublicKey>) -> bool {
        if self.issuer_ca_cert_sig.is_empty() {
            return true;
        }

        let Some(pgp_key) = pgp_keys.get(&self.swarm_id) else {
            return false;
        };

        let Ok(sig) = DetachedSignature::from_bytes(Cursor::new(&self.issuer_ca_cert_sig)) else {
            return false;
        };

        sig.verify(pgp_key, &self.issuer_ca_cert).is_ok()
    }
}
