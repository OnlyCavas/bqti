use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::types::UnixDate;

pub fn bqti_data_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "bqti").map(|proj| proj.data_dir().to_path_buf())
}

pub fn certs_dir() -> Option<PathBuf> {
    let data = bqti_data_dir()?;
    Some(data.join("certs"))
}

pub async fn ensure_dir(path: &PathBuf) -> Result<()> {
    tokio::fs::create_dir_all(path)
        .await
        .with_context(|| format!("Failed to create directory {}", path.display()))
}

pub fn version() -> String {
    format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

pub fn fetch_current_timestamp() -> UnixDate {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
