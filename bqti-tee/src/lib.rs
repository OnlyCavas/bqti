#[cfg(feature = "tee")]
mod keystone;

#[cfg(feature = "tee")]
pub use keystone::*;

pub fn tee_available() -> bool {
    #[cfg(feature = "tee")]
    {
        std::path::Path::new("/dev/keystone_enclave").exists()
    }
    #[cfg(not(feature = "tee"))]
    {
        false
    }
}
