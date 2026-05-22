use std::sync::Arc;

use rcgen::SignatureAlgorithm;
use rustls::sign::CertifiedKey;
use rustls_pki_types::CertificateDer;
use thiserror::Error;

mod config;
mod soft;

#[cfg(feature = "tee")]
mod tee;

#[cfg(feature = "tee")]
pub type ActiveKeyIdentity = tee::TeeKeyIdentity;

#[cfg(not(feature = "tee"))]
pub type ActiveKeyIdentity = soft::SoftwareKeyIdentity;

const DEFAULT_SIGN_ALGORITM: &SignatureAlgorithm = &rcgen::PKCS_ED25519;

pub type Signature = Vec<u8>;

pub use config::{NoVerifier, SingleCertResolver};

#[derive(Debug, Error)]
pub enum CertError {
    #[error("failed to create certificate")]
    Failed(),

    #[error(transparent)]
    Rcgen(#[from] rcgen::Error),

    #[cfg(feature = "tee")]
    #[error(transparent)]
    Tee(#[from] bqti_tee::TeeError),
}

pub trait PublicKey {
    fn pub_key(&self) -> &[u8];
}

pub trait Signer: PublicKey + Send + Sync {
    fn sign(&self, data: &[u8]) -> Result<Signature, CertError>;
}

pub trait Verifier {
    fn verify(pub_key: &[u8], data: &[u8], signature: &[u8]) -> bool;
}

pub trait KeyIdentity: Signer + Verifier {
    fn leaf(&self, common_name: &str, as_ca: bool) -> Result<ActiveKeyIdentity, CertError>;
    fn cert_der(&self) -> CertificateDer<'static>;
    fn certified_key(&self) -> Arc<CertifiedKey>;
}

pub use soft::SoftwareKeyIdentity;

#[cfg(feature = "tee")]
pub use tee::TeeKeyIdentity;
