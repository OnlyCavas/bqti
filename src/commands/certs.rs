use clap::{Parser, ValueEnum};

use anyhow::{Context, Result};

use crate::utils::{
    bqti::certs_dir,
    certs::{make_ca_root, store_cert},
};

#[derive(Debug, Clone, ValueEnum)]
pub enum CertType {
    Root,
    Leaf,
}

#[derive(Parser, Debug)]
pub struct CertArgs {
    #[arg(short)]
    t: CertType,
}

pub async fn run(args: CertArgs) -> Result<()> {
    let ca_root = make_ca_root().await?;
    let dir = certs_dir().context("cannot determine certs directory")?;

    match args.t {
        CertType::Root => {
            info!("done, at {}", dir.display());

            Ok(())
        }
        CertType::Leaf => {
            let leaf_cert = ca_root.leaf("localhost", true)?;
            store_cert(&leaf_cert, "quic.der", "quic_pk.der").await?;
            info!("done, at {}", dir.display());

            Ok(())
        }
    }
}
