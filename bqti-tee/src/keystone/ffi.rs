use std::ffi::{c_char, c_int, c_uint};

#[repr(C)]
pub struct PowResult {
    pub pow: [u8; 32],
    pub nonce: u32,
    pub sig: [u8; 64],
    pub pub_key: [u8; 32],
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

    pub unsafe fn enclave_destroy();
}
