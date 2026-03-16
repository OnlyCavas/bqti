use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::UnixDate;

pub fn version() -> String {
    format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

pub fn fetch_current_timestamp() -> UnixDate {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
