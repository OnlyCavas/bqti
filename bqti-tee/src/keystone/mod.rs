use std::{
    ffi::{CString, c_void},
    fs,
};

use tempfile::TempDir;
use thiserror::Error;

use crate::{
    KeystoneAttestReport,
    keystone::ffi::{ATTEST_REPORT_SIZE, AttestReport},
};

mod ffi;

include!(concat!(env!("OUT_DIR"), "/enclave_assets.rs"));

#[derive(Debug, Error)]
pub enum TeeError {
    #[error("enclave init failed: {0}")]
    InitFailed(i32),

    #[error("pow failed: {0}")]
    PowFailed(i32),

    #[error("signature failed: {0}")]
    SignatureFailed(i32),

    #[error("fetch failed: {0}")]
    PublicKeyFailed(i32),

    #[error("attestation failed: {0}")]
    AttestFailed(i32),
}

pub struct PowResult {
    pub hash: [u8; 32],
    pub nonce: u32,
    pub sig: [u8; 64],
    pub pub_key: [u8; 32],
}

pub type Result<T> = std::result::Result<T, TeeError>;

pub trait TeeExecute {
    fn pow(&self, challenge: u32, difficulty: u32) -> Result<PowResult>;
    fn sign(&self, data: &[u8]) -> Result<[u8; 64]>;
    fn get_pubkey(&self) -> Result<[u8; 32]>;
    fn attest(&self, nonce: &[u8]) -> Result<crate::AttestReport>;
}

fn extract_enclave() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bqti"), ENCLAVE_EAPP).unwrap();
    fs::write(dir.path().join("eyrie-rt"), ENCLAVE_RT).unwrap();
    fs::write(dir.path().join("loader.bin"), ENCLAVE_LOADER).unwrap();
    dir
}

#[derive(Debug)]
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

    fn enclave_init(&self) -> Result<()> {
        let inite_result = unsafe {
            ffi::enclave_init(self.eapp.as_ptr(), self.rt.as_ptr(), self.loader.as_ptr())
        };

        match inite_result {
            0 => Ok(()),
            _ => Err(TeeError::InitFailed(inite_result)),
        }
    }

    fn enclave_drop(&self) {
        unsafe { ffi::enclave_destroy() };
    }
}

impl TeeExecute for Tee {
    fn pow(&self, challenge: u32, difficulty: u32) -> Result<PowResult> {
        self.enclave_init()?;

        let mut out = ffi::PowResult {
            pow: [0u8; 32],
            nonce: 0,
            sig: [0u8; 64],
            pub_key: [0u8; 32],
        };

        let pow_result = unsafe { ffi::enclave_run_pow(challenge, difficulty, &mut out) };
        self.enclave_drop();

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

    fn sign(&self, data: &[u8]) -> Result<[u8; 64]> {
        let mut signature = [0u8; 64];

        self.enclave_init()?;

        let result = unsafe {
            ffi::enclave_sign(data.as_ptr() as *const c_void, data.len(), &mut signature)
        };

        if result != 0 {
            return Err(TeeError::SignatureFailed(result));
        }

        self.enclave_drop();
        return Ok(signature);
    }

    fn get_pubkey(&self) -> Result<[u8; 32]> {
        let mut pub_key = [0u8; 32];
        self.enclave_init()?;

        let result = unsafe { ffi::enclave_get_pubkey(&mut pub_key) };

        if result != 0 {
            return Err(TeeError::PublicKeyFailed(result));
        }

        self.enclave_drop();
        return Ok(pub_key);
    }

    fn attest(&self, nonce: &[u8]) -> Result<crate::AttestReport> {
        let mut bytes = [0u8; ATTEST_REPORT_SIZE];
        self.enclave_init()?;

        let result = unsafe {
            ffi::enclave_attest(
                nonce.as_ptr() as *const c_void,
                nonce.len(),
                bytes.as_mut_ptr(),
            )
        };

        if result != 0 {
            return Err(TeeError::AttestFailed(result));
        }

        self.enclave_drop();

        let tee_report = parse_attest_report(&bytes);

        let report = KeystoneAttestReport {
            enclave: crate::EnclaveReport {
                hash: tee_report.enclave.hash,
                data_len: tee_report.enclave.data_len,
                data: tee_report.enclave.data,
                signature: tee_report.enclave.signature,
            },
            sm: crate::SmReport {
                hash: tee_report.sm.hash,
                public_key: tee_report.sm.pub_key,
                signature: tee_report.sm.signature,
            },
            dev_public_key: tee_report.dev_pub_key,
        };

        return Ok(crate::AttestReport::Keystone(report));
    }
}

fn parse_attest_report(bytes: &[u8; 1352]) -> &AttestReport {
    unsafe { &*(bytes.as_ptr() as *const AttestReport) }
}
