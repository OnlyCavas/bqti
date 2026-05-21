use std::{ffi::CString, fs};

use tempfile::TempDir;
use thiserror::Error;

mod ffi;

include!(concat!(env!("OUT_DIR"), "/enclave_assets.rs"));

#[derive(Debug, Error)]
pub enum TeeError {
    #[error("enclave init failed: {0}")]
    InitFailed(i32),

    #[error("pow failed: {0}")]
    PowFailed(i32),
}

pub struct PowResult {
    pub hash: [u8; 32],
    pub nonce: u32,
    pub sig: [u8; 64],
    pub pub_key: [u8; 32],
}

type Result<T> = std::result::Result<T, TeeError>;

pub trait TeeExecute {
    fn pow(&self, challenge: u32, difficulty: u32) -> Result<PowResult>;
}

fn extract_enclave() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bqti"), ENCLAVE_EAPP).unwrap();
    fs::write(dir.path().join("eyrie-rt"), ENCLAVE_RT).unwrap();
    fs::write(dir.path().join("loader.bin"), ENCLAVE_LOADER).unwrap();
    dir
}

pub struct Tee {
    _dir: TempDir,
    eapp: CString,
    rt: CString,
    loader: CString,
}

impl Tee {
    pub fn new() -> Self {
        let dir = extract_enclave();
        let eapp = CString::new(dir.path().join("bqti").to_str().unwrap()).unwrap();
        let rt = CString::new(dir.path().join("eyrie-rt").to_str().unwrap()).unwrap();
        let loader = CString::new(dir.path().join("loader.bin").to_str().unwrap()).unwrap();

        Self {
            _dir: dir,
            eapp,
            rt,
            loader,
        }
    }
}

impl TeeExecute for Tee {
    fn pow(&self, challenge: u32, difficulty: u32) -> Result<PowResult> {
        let inite_result = unsafe {
            ffi::enclave_init(self.eapp.as_ptr(), self.rt.as_ptr(), self.loader.as_ptr())
        };

        if inite_result != 0 {
            return Err(TeeError::InitFailed(inite_result));
        }

        let mut out = ffi::PowResult {
            pow: [0u8; 32],
            nonce: 0,
            sig: [0u8; 64],
            pub_key: [0u8; 32],
        };

        let pow_result = unsafe { ffi::enclave_run_pow(challenge, difficulty, &mut out) };
        unsafe { ffi::enclave_destroy() };

        if pow_result != 0 {
            return Err(TeeError::PowFailed(pow_result));
        }

        Ok(PowResult {
            hash: out.pow,
            nonce: out.nonce,
            sig: out.sig,
            pub_key: out.pub_key,
        })
    }
}
