use bqti_tee::AttestReport;
#[cfg(feature = "tee")]
use bqti_tee::TeeError;

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod manager;
mod pow;
mod token;

pub use manager::AuthManager;

use crate::{bit_torrent::certs::Signer, certs::CertError, dht::ManifestError, types::UnixDate};

pub const TOKEN_EXP_SECONDS: UnixDate = 30 * 60;
pub const DIFFICULTY: u32 = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrustLevel {
    Attested(AttestReport), // tee chain valid, unless the dev_pubkey that can't be validated
    // without an manufactor PKI
    Unattested, // no TEE, software only
    Rejected,   // TEE present but verification failed
}

pub trait Evidence {
    fn sign(&mut self, signer: &impl Signer) -> Result<(), AuthError>;
}

pub trait Authorizable: Evidence {
    fn verify_for(&self, pub_key: &[u8]) -> bool;
    fn verify(&self) -> bool;
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

    #[error("rate limit exceeded")]
    RateLimited(),

    #[error("failed to sign proof of work")]
    PowSignFailed(#[from] CertError),

    #[cfg(feature = "tee")]
    #[error(transparent)]
    TeeError(#[from] TeeError),

    #[error("failed to fetch version manifest")]
    ManifestError(#[from] ManifestError),
}

pub use pow::*;
pub use token::*;
