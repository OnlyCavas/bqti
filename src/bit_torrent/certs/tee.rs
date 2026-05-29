use std::sync::Arc;

use crate::certs::{
    ActiveKeyIdentity, CertError, KeyIdentity, PublicKey, Signature, Signer, Verifier,
};
use bqti_tee::TeeExecute;
use rcgen::{BasicConstraints, Certificate, CertificateParams, IsCa, PublicKeyData, SigningKey};
use ring::signature::UnparsedPublicKey;
use rustls::sign::CertifiedKey;
use rustls::sign::Signer as RustlsSigner;
use rustls::sign::SigningKey as RustlsSigningKey;

#[derive(Clone, Debug)]
struct TeeSigner {
    tee: Arc<bqti_tee::Tee>,
    pub_key: [u8; 32],
}

impl TeeSigner {
    fn new(tee: Arc<bqti_tee::Tee>, pub_key: &[u8; 32]) -> Self {
        Self {
            tee,
            pub_key: *pub_key,
        }
    }
}

impl RustlsSigningKey for TeeSigner {
    fn choose_scheme(&self, offered: &[rustls::SignatureScheme]) -> Option<Box<dyn RustlsSigner>> {
        if offered.contains(&rustls::SignatureScheme::ED25519) {
            Some(Box::new(self.clone()))
        } else {
            None
        }
    }

    fn algorithm(&self) -> rustls::SignatureAlgorithm {
        rustls::SignatureAlgorithm::ED25519
    }
}

impl RustlsSigner for TeeSigner {
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        self.tee
            .sign(msg)
            .map_err(|_| rustls::Error::General("TEE signing failed".into()))
            .map(|sign| sign.to_vec())
    }

    fn scheme(&self) -> rustls::SignatureScheme {
        rustls::SignatureScheme::ED25519
    }
}

struct RcgenSigner(TeeSigner);

impl PublicKeyData for RcgenSigner {
    fn der_bytes(&self) -> &[u8] {
        &self.0.pub_key
    }
    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &rcgen::PKCS_ED25519
    }
}

impl SigningKey for RcgenSigner {
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
        self.0
            .tee
            .sign(msg)
            .map_err(|_| rcgen::Error::RemoteKeyError)
            .map(|sig| sig.to_vec())
    }
}

pub struct TeeKeyIdentity {
    cert: Certificate,
    signer: TeeSigner,
}

impl TeeKeyIdentity {
    pub fn new() -> Result<Self, CertError> {
        let tee = Arc::new(bqti_tee::Tee::new());
        let pub_key = tee.get_pubkey()?;
        let signer = TeeSigner::new(Arc::clone(&tee), &pub_key);

        let mut params = CertificateParams::new(vec!["bqti".to_string()])?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

        let binding = RcgenSigner(signer.clone());
        let cert = params.self_signed(&binding)?;

        Ok(Self { cert, signer })
    }
}

impl KeyIdentity for TeeKeyIdentity {
    fn leaf(&self, common_name: &str, as_ca: bool) -> Result<ActiveKeyIdentity, CertError> {
        let signer = self.signer.clone();

        let mut params = CertificateParams::new(vec![common_name.to_string()])?;

        params.is_ca = if as_ca {
            IsCa::Ca(BasicConstraints::Unconstrained)
        } else {
            IsCa::NoCa
        };

        let binding = RcgenSigner(signer.clone());
        let cert = params.self_signed(&binding)?;

        Ok(Self { cert, signer })
    }

    fn cert_der(&self) -> rustls_pki_types::CertificateDer<'static> {
        rustls_pki_types::CertificateDer::from(self.cert.der().to_vec())
    }

    fn certified_key(&self) -> Arc<CertifiedKey> {
        let signer: Arc<dyn RustlsSigningKey> = Arc::new(self.signer.clone());
        Arc::new(CertifiedKey::new(vec![self.cert_der()], signer))
    }
}
impl Signer for TeeKeyIdentity {
    fn sign(&self, data: &[u8]) -> Result<Signature, CertError> {
        self.signer
            .sign(data)
            .map_err(|_| CertError::Tee(bqti_tee::TeeError::SignatureFailed(-1)))
    }
}

impl Verifier for TeeKeyIdentity {
    fn verify(pub_key: &[u8], data: &[u8], signature: &[u8]) -> bool {
        let key = UnparsedPublicKey::new(&ring::signature::ED25519, pub_key);
        key.verify(data, signature).is_ok()
    }
}

impl PublicKey for TeeKeyIdentity {
    fn pub_key(&self) -> &[u8] {
        &self.signer.pub_key
    }
}
