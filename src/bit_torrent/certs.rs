use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, SignatureAlgorithm};
use ring::signature::{self, Ed25519KeyPair};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use thiserror::Error;

const DEFAULT_SIGN_ALGORITM: &SignatureAlgorithm = &rcgen::PKCS_ED25519;

#[derive(Debug, Error)]
pub enum CertError {
    #[error("failed to create certificate")]
    Failed(),

    #[error(transparent)]
    Rcgen(#[from] rcgen::Error),
}

trait Signer {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>, CertError>;
}

trait Verifier {
    fn verify(&self, pub_key: &[u8], data: &[u8], signature: &[u8]) -> bool;
}

pub struct KeyIdentity {
    key_pair: KeyPair,
    cert_der: CertificateDer<'static>,
}

impl KeyIdentity {
    pub fn from_bytes_der(cert_bytes: &[u8], priv_key_bytes: &[u8]) -> Result<Self, CertError> {
        let priv_key_der = PrivateKeyDer::Pkcs8(priv_key_bytes.to_vec().into());
        let key_pair = KeyPair::from_der_and_sign_algo(&priv_key_der, DEFAULT_SIGN_ALGORITM)?;
        let cert_der = CertificateDer::from(cert_bytes.to_vec());

        Ok(Self { key_pair, cert_der })
    }

    pub fn cert_der(&self) -> CertificateDer<'static> {
        return self.cert_der.clone();
    }

    pub fn key_der(&self) -> PrivateKeyDer<'static> {
        return PrivateKeyDer::Pkcs8(self.key_pair.serialize_der().into());
    }

    pub fn to_bytes_der(&self) -> (Vec<u8>, Vec<u8>) {
        let cert_bytes = self.cert_der.to_vec();
        let key_bytes = self.key_pair.serialize_der();

        (cert_bytes, key_bytes)
    }

    pub fn root(common_name: &str) -> Result<Self, CertError> {
        let mut params = CertificateParams::new(vec![common_name.to_string()])?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

        let key_pair = KeyPair::generate_for(DEFAULT_SIGN_ALGORITM)?;
        let cert = params.self_signed(&key_pair)?;

        Ok(Self {
            key_pair: key_pair,
            // params,
            cert_der: cert.der().clone(),
        })
    }

    pub fn leaf(&self, common_name: &str, as_ca: bool) -> Result<Self, CertError> {
        let mut params = CertificateParams::new(vec![common_name.to_string()])?;
        params.is_ca = if as_ca {
            IsCa::Ca(BasicConstraints::Unconstrained)
        } else {
            IsCa::NoCa
        };

        let issuer = Issuer::from_ca_cert_der(&self.cert_der, &self.key_pair)?;

        let leaf_key = KeyPair::generate_for(DEFAULT_SIGN_ALGORITM)?;
        let leaf_cert = params.signed_by(&leaf_key, &issuer)?;

        Ok(Self {
            key_pair: leaf_key,
            cert_der: leaf_cert.der().clone(),
        })
    }
}

impl Signer for KeyIdentity {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>, CertError> {
        let pkcs8_key = self.key_pair.serialize_der();

        let signing_key =
            Ed25519KeyPair::from_pkcs8(&pkcs8_key).map_err(|_| CertError::Failed())?;

        let signature = signing_key.sign(data);

        Ok(signature.as_ref().to_vec())
    }
}

impl Verifier for KeyIdentity {
    fn verify(&self, pub_key: &[u8], data: &[u8], signature: &[u8]) -> bool {
        let pub_key = signature::UnparsedPublicKey::new(&signature::ED25519, pub_key);
        pub_key.verify(data, signature).is_ok()
    }
}
