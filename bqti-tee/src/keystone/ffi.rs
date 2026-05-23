use std::ffi::{c_char, c_int, c_uint, c_void};

use libc::size_t;

pub const ATTEST_MDSIZE: usize = 64;
pub const ATTEST_DATA_MAXLEN: usize = 1024;
pub const ATTEST_REPORT_SIZE: usize = 1352;

pub const SIGNATURE_LENGTH: usize = 64;
pub const HASH_LENGTH: usize = 32;
pub const PUBKEY_LENGTH: usize = 32;

const _: () = assert!(
    std::mem::size_of::<EnclaveReport>()
        == ATTEST_MDSIZE + 8 + ATTEST_DATA_MAXLEN + SIGNATURE_LENGTH,
    "EnclaveReport size mismatch"
);

const _: () = assert!(
    std::mem::size_of::<SmReport>() == ATTEST_MDSIZE + PUBKEY_LENGTH + SIGNATURE_LENGTH,
    "SmReport size mismatch"
);

const _: () = assert!(
    std::mem::size_of::<AttestReport>() == ATTEST_REPORT_SIZE,
    "AttestReport size mismatch — layout does not match Keystone SDK"
);

#[repr(C)]
pub struct PowResult {
    pub pow: [u8; HASH_LENGTH],
    pub nonce: u32,
    pub sig: [u8; SIGNATURE_LENGTH],
    pub pub_key: [u8; PUBKEY_LENGTH],
}

#[repr(C, packed)]
pub struct EnclaveReport {
    pub hash: [u8; ATTEST_MDSIZE],
    pub data_len: u64,
    pub data: [u8; ATTEST_DATA_MAXLEN],
    pub signature: [u8; SIGNATURE_LENGTH],
}

#[repr(C, packed)]
pub struct SmReport {
    pub hash: [u8; ATTEST_MDSIZE],
    pub pub_key: [u8; PUBKEY_LENGTH],
    pub signature: [u8; SIGNATURE_LENGTH],
}

#[repr(C, packed)]
pub struct AttestReport {
    pub enclave: EnclaveReport,
    pub sm: SmReport,
    pub dev_pub_key: [u8; PUBKEY_LENGTH],
}

unsafe extern "C" {
    pub unsafe fn enclave_init(
        eapp: *const c_char,
        rt: *const c_char,
        loader_path: *const c_char,
    ) -> c_int;

    pub unsafe fn enclave_run_pow(
        challenge: c_uint,
        difficulty: c_uint,
        out: *mut PowResult,
    ) -> c_int;

    pub unsafe fn enclave_get_pubkey(out: &mut [u8; 32]) -> c_int;

    pub unsafe fn enclave_sign(data: *const c_void, data_len: size_t, out: &mut [u8; 64]) -> c_int;

    pub unsafe fn enclave_attest(nonce: *const c_void, nonce_len: size_t, out: *mut u8) -> c_int;

    pub unsafe fn enclave_destroy();
}
