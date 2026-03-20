use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, IsCa, Issuer, KeyPair,
    SignatureAlgorithm,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::fs;

const DEFAULT_SIGN_ALGORITM: &SignatureAlgorithm = &rcgen::PKCS_ECDSA_P256_SHA256;

pub struct Cert {
    cert: CertificateDer<'static>,
    pair: KeyPair,
}

impl Cert {
    pub fn to_bytes_der(&self) -> (Vec<u8>, Vec<u8>) {
        let cert_bytes = self.cert.to_vec();
        let key_bytes = self.pair.serialize_der();

        (cert_bytes, key_bytes)
    }

    pub fn from_bytes_der(cert_bytes: &[u8], priv_key_bytes: &[u8]) -> Result<Self, rcgen::Error> {
        let priv_key_der = PrivateKeyDer::Pkcs8(priv_key_bytes.into());
        let pair = KeyPair::from_der_and_sign_algo(&priv_key_der, DEFAULT_SIGN_ALGORITM)?;
        let cert = CertificateDer::from(cert_bytes.to_vec());

        Ok(Self { cert, pair })
    }

    pub fn issuer(&self) -> Result<Issuer<'_, &KeyPair>, rcgen::Error> {
        Issuer::from_ca_cert_der(&self.cert, &self.pair)
    }

    pub fn der(&self) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        let cert_der = CertificateDer::from(self.cert.to_vec());
        let priv_key = PrivateKeyDer::Pkcs8(self.pair.serialize_der().into());

        (cert_der, priv_key)
    }
}

pub struct RootCA {
    pub cert: Cert,
}

impl RootCA {
    pub async fn load_or_generate(
        ca_root_path: &str,
        pk_root_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if fs::try_exists(ca_root_path).await? && fs::try_exists(pk_root_path).await? {
            let ca_bytes = fs::read(ca_root_path).await?;
            let pk_bytes = fs::read(pk_root_path).await?;

            let cert = Cert::from_bytes_der(&ca_bytes, &pk_bytes)?;

            info!("loading ca root");
            return Ok(RootCA { cert });
        }

        Self::generate()
    }

    pub fn generate() -> Result<Self, Box<dyn std::error::Error>> {
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

        let mut dn = DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, "MyThesisRootCA");

        params.distinguished_name = dn;

        let key_pair = KeyPair::generate_for(&DEFAULT_SIGN_ALGORITM)?;
        let cert = CertificateDer::from(params.self_signed(&key_pair)?.der().to_vec());

        Ok(Self {
            cert: Cert {
                cert,
                pair: key_pair,
            },
        })
    }
}

pub struct LeafCert {
    pub cert: Cert,
}

impl LeafCert {
    pub fn generate(
        alt_names: impl Into<Vec<String>>,
        issuer: &RootCA,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let alt_names = alt_names.into();

        let mut params = CertificateParams::new(alt_names.clone())?;
        let mut dn = DistinguishedName::new();

        if let Some(alt_name) = alt_names.first() {
            dn.push(rcgen::DnType::CommonName, alt_name);
        }

        params.distinguished_name = dn;

        let key_pair = KeyPair::generate_for(&DEFAULT_SIGN_ALGORITM)?;

        let cert = CertificateDer::from(
            params
                .signed_by(&key_pair, &issuer.cert.issuer()?)?
                .der()
                .to_vec(),
        );

        Ok(Self {
            cert: Cert {
                cert,
                pair: key_pair,
            },
        })
    }
}
