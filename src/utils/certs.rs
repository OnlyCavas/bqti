use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
};

#[cfg(not(feature = "tee"))]
use crate::certs::SoftwareKeyIdentity;
use crate::{
    certs::ActiveKeyIdentity,
    types::Hash32Bytes,
    utils::bqti::{certs_dir, ensure_dir, swarm_dir},
};

use anyhow::{Context, Result};
use pgp::{
    composed::{Deserializable, DetachedSignature, SignedPublicKey},
    ser::Serialize,
};
use sha2::{Digest, Sha256};

pub struct CertOptions {
    pub dir: Option<PathBuf>,
}

pub const CERTIFICATE_ROOT_PATH: &str = "ca.der";
pub const SWARM_PGP_PUBKEY: &str = "swarm.asc";
pub const PRIVATE_KEY_PATH: &str = "root_pk.der";

impl CertOptions {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: Some(dir.to_path_buf()),
        }
    }

    pub fn resolve_dir(&self) -> Result<PathBuf> {
        match &self.dir {
            Some(dir) => Ok(dir.clone()),
            None => certs_dir().context("cannot determine certs directory"),
        }
    }
}

impl Default for CertOptions {
    fn default() -> Self {
        Self { dir: certs_dir() }
    }
}

pub async fn ensure_directories() -> Result<()> {
    let swarm_directory = swarm_dir().context("failed to fetch internal bqti data")?;
    ensure_dir(&swarm_directory).await?;

    Ok(())
}

pub async fn store_cert(cert: &ActiveKeyIdentity, options: &CertOptions) -> Result<()> {
    let dir = options.resolve_dir()?;
    ensure_dir(&dir).await?;

    let ca_path = dir.join(CERTIFICATE_ROOT_PATH);

    #[cfg(feature = "tee")]
    {
        use crate::certs::KeyIdentity;

        let cert_der = cert.cert_der().to_vec();
        tokio::fs::write(&ca_path, cert_der).await?;
    }

    #[cfg(not(feature = "tee"))]
    {
        let pk_path = dir.join(PRIVATE_KEY_PATH);
        let (cert_der, pk_der) = cert.to_bytes_der();

        tokio::fs::write(&ca_path, cert_der).await?;
        tokio::fs::write(&pk_path, pk_der).await?;
    }

    Ok(())
}

fn extract_swarm_id(public_key: &SignedPublicKey) -> Result<Hash32Bytes> {
    let pubkey_bytes = public_key.to_bytes()?;

    let swarm_id = Sha256::digest(pubkey_bytes).into();
    Ok(swarm_id)
}

pub fn load_pgp_keys(paths: Vec<PathBuf>) -> Result<HashMap<Hash32Bytes, SignedPublicKey>> {
    let mut keys = HashMap::new();

    for file_path in paths {
        if file_path.extension().and_then(|e| e.to_str()) != Some("asc") {
            continue;
        }

        let (public_key, _) = SignedPublicKey::from_armor_file(file_path)?;
        let swarm_id = extract_swarm_id(&public_key)?;

        keys.insert(swarm_id, public_key);
    }

    Ok(keys)
}

pub fn load_pgp_signagure() -> Result<(Hash32Bytes, DetachedSignature)> {
    let dir = certs_dir().context("failed to fetch internal bqti data")?;

    let swarm_pubkey = dir.join(SWARM_PGP_PUBKEY);
    let pgp_signature = dir.join(format!("{}.{}", CERTIFICATE_ROOT_PATH, "sig"));

    if !pgp_signature.exists() || !swarm_pubkey.exists() {
        return Err(anyhow::anyhow!(format!(
            "failed to load the PGP signature key, this node shouldn't be bootstrap: {}",
            pgp_signature.display()
        )));
    }

    let sig_bytes = std::fs::read(&pgp_signature)
        .with_context(|| format!("failed to read pgp signagure: {}", pgp_signature.display()))?;

    let (public_key, _) = SignedPublicKey::from_armor_file(&swarm_pubkey)
        .context("failed to parse PGP certificate")?;

    let swarm_id = extract_swarm_id(&public_key).context("failed to generate the swarm_id")?;
    let signature = DetachedSignature::from_bytes(Cursor::new(sig_bytes))
        .context("failed to parse pgp signature")?;

    Ok((swarm_id, signature))
}

#[allow(unused_variables)]
pub async fn read_cert(options: &CertOptions) -> Result<ActiveKeyIdentity> {
    #[cfg(not(feature = "tee"))]
    {
        use anyhow::ensure;

        use crate::certs::SoftwareKeyIdentity;

        let dir = options.resolve_dir()?;

        let ca_path = dir.join(CERTIFICATE_ROOT_PATH);
        ensure!(ca_path.exists(), "cert not found: {}", ca_path.display());

        let pk_path = dir.join(PRIVATE_KEY_PATH);
        ensure!(pk_path.exists(), "key not found: {}", pk_path.display());

        let pk = tokio::fs::read(&pk_path)
            .await
            .with_context(|| format!("failed to read key: {}", pk_path.display()))?;

        let ca = tokio::fs::read(&ca_path)
            .await
            .with_context(|| format!("failed to read cert: {}", ca_path.display()))?;

        return SoftwareKeyIdentity::from_bytes_der(&ca, &pk).map_err(Into::into);
    }

    #[cfg(feature = "tee")]
    {
        use crate::certs::TeeKeyIdentity;
        TeeKeyIdentity::new().map_err(Into::into)
    }
}

#[cfg(not(feature = "tee"))]
pub async fn make_ca_root() -> Result<SoftwareKeyIdentity> {
    let ca_root = read_cert(&CertOptions::default()).await;

    match ca_root {
        Ok(certificate) => Ok(certificate),
        Err(_) => {
            let ca_root = SoftwareKeyIdentity::root("BQTI - Certificate")?;
            store_cert(&ca_root, &CertOptions::default()).await?;

            Ok(ca_root)
        }
    }
}
