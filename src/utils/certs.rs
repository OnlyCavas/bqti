use std::path::{Path, PathBuf};

use crate::{
    bit_torrent::certs::KeyIdentity,
    utils::bqti::{certs_dir, ensure_dir},
};

use anyhow::{Context, Result, ensure};

pub struct CertOptions {
    pub dir: Option<PathBuf>,
}

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

pub async fn store_cert(
    cert: &KeyIdentity,
    cert_name: &str,
    priv_key: &str,
    options: &CertOptions,
) -> Result<()> {
    let dir = options.resolve_dir()?;
    ensure_dir(&dir).await?;

    let ca_path = dir.join(cert_name);
    let pk_path = dir.join(priv_key);

    let (cert_der, pk_der) = cert.to_bytes_der();

    tokio::fs::write(&ca_path, cert_der).await?;
    tokio::fs::write(&pk_path, pk_der).await?;

    Ok(())
}

pub async fn read_cert(
    cert_name: &str,
    priv_key: &str,
    options: &CertOptions,
) -> Result<KeyIdentity> {
    let dir = options.resolve_dir()?;
    let ca_path = dir.join(cert_name);
    let pk_path = dir.join(priv_key);

    ensure!(ca_path.exists(), "cert not found: {}", ca_path.display());
    ensure!(pk_path.exists(), "key not found: {}", pk_path.display());

    let ca = tokio::fs::read(&ca_path)
        .await
        .with_context(|| format!("failed to read cert: {}", ca_path.display()))?;
    let pk = tokio::fs::read(&pk_path)
        .await
        .with_context(|| format!("failed to read key: {}", pk_path.display()))?;

    KeyIdentity::from_bytes_der(&ca, &pk).map_err(Into::into)
}

pub async fn make_ca_root() -> Result<KeyIdentity> {
    let ca_path = "ca.der";
    let pk_path = "root_pk.der";

    let ca_root = read_cert(ca_path, pk_path, &CertOptions::default()).await;

    match ca_root {
        Ok(certificate) => Ok(certificate),
        Err(_) => {
            let ca_root = KeyIdentity::root("BQTI - Certificate")?;
            store_cert(&ca_root, ca_path, pk_path, &CertOptions::default()).await?;

            Ok(ca_root)
        }
    }
}
