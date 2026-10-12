use std::env;
use std::error::Error;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::{fs, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const ASSETS: &[(&str, &str)] = &[
    ("ENCLAVE_EAPP", "bqti"),
    ("ENCLAVE_RT", "eyrie-rt"),
    ("ENCLAVE_LOADER", "loader.bin"),
];

struct Config {
    out_dir: PathBuf,
    enclave_dir: PathBuf,
    build_dir: PathBuf,
    sdk_dir: PathBuf,
}

fn require_env(name: &str) -> Result<String> {
    env::var(name).map_err(|e| format!("environment variable {name}: {e}").into())
}

impl Config {
    fn from_env(out_dir: PathBuf) -> Result<Self> {
        let manifest = PathBuf::from(require_env("CARGO_MANIFEST_DIR")?);
        let enclave_dir = manifest
            .parent()
            .ok_or("manifest has no parent directory")?
            .join("bqti-enclave");

        let build_dir = enclave_dir.join("build");
        let sdk_dir = enclave_dir.join("vendor/keystone/sdk");

        Ok(Self {
            enclave_dir,
            build_dir,
            out_dir,
            sdk_dir,
        })
    }
}

fn write_assets(out_dir: &Path, embed_from: Option<&Path>) -> Result<()> {
    let mut f = fs::File::create(out_dir.join("enclave_assets.rs"))?;

    for (name, file) in ASSETS {
        match embed_from {
            Some(build_dir) => {
                let src = build_dir.join(file);
                let dst = out_dir.join(file);

                fs::copy(&src, &dst)
                    .map_err(|e| format!("missing enclave asset {}: {e}", src.display()))?;

                writeln!(f, "pub static {name}: &[u8] = include_bytes!({:?});", dst)?;
            }
            None => writeln!(f, "pub static {name}: &[u8] = &[];")?,
        }
    }

    Ok(())
}

fn build_enclave(cfg: &Config) -> Result<()> {
    let mut make = Command::new("make");
    make.arg("-C").arg(&cfg.enclave_dir);

    let status = make
        .status()
        .map_err(|e| format!("failed to spawn {make:?}: {e}"))?;

    if !status.success() {
        return Err(format!("{make:?} exited with {status}").into());
    }

    Ok(())
}

fn emit_link_directives(cfg: &Config) {
    println!("cargo:rustc-link-search={}", cfg.build_dir.display());
    println!("cargo:rustc-link-search={}/lib", cfg.sdk_dir.display());

    for lib in [
        "enclave-ffi",
        "keystone-host",
        "keystone-edge",
        "keystone-verifier",
    ] {
        println!("cargo:rustc-link-lib=static={lib}");
    }

    println!("cargo:rustc-link-lib=stdc++");
}

fn emit_rerun_triggers(enclave_dir: &Path) {
    for p in ["src", "include", "CMakeLists.txt", "Makefile"] {
        println!("cargo:rerun-if-changed={}", enclave_dir.join(p).display());
    }
}

fn main() -> Result<()> {
    let out_dir = PathBuf::from(require_env("OUT_DIR")?);

    if env::var("CARGO_CFG_TARGET_ARCH")? != "riscv64" {
        return write_assets(&out_dir, None);
    }

    let cfg = Config::from_env(out_dir)?;
    emit_link_directives(&cfg);
    build_enclave(&cfg)?;
    emit_rerun_triggers(&cfg.enclave_dir);
    write_assets(&cfg.out_dir, Some(&cfg.build_dir))
}
