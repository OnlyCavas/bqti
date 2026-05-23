#[cfg(feature = "tee")]
use bqti_tee::TeeError;

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod manager;
mod pow;
mod token;

pub use manager::AuthManager;

use crate::{bit_torrent::certs::Signer, certs::CertError, types::UnixDate};

pub const TOKEN_EXP_SECONDS: UnixDate = 30 * 60;
pub const DIFFICULTY: u32 = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(u8)]
pub enum TrustLevel {
    Attested = 0,   // verified by TEE
    Unattested = 1, // doesn't support TEE
    Rejected = 2,   // it's invalid
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
}

pub use pow::*;
pub use token::*;
