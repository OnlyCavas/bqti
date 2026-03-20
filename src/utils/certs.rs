use crate::{
    network::RootCA,
    utils::bqti::{certs_dir, ensure_dir},
};

use anyhow::{Context, Result};

pub async fn load_or_generate_root_ca() -> Result<RootCA> {
    let dir = certs_dir().context("cannot determine certs directory")?;

    ensure_dir(&dir).await?;

    let ca_path = dir.join("ca.der");
    let pk_path = dir.join("root_pk.der");

    if ca_path.exists() && pk_path.exists() {
        let ca = tokio::fs::read(&ca_path).await?;
        let pk = tokio::fs::read(&pk_path).await?;

        return Ok(RootCA::from_bytes(ca, pk)?);
    }

    let ca = RootCA::generate()?;
    let (cert_der, pk_der) = ca.cert.der();

    tokio::fs::write(&ca_path, &cert_der).await?;
    tokio::fs::write(&pk_path, pk_der.secret_der()).await?;

    Ok(ca)
}
