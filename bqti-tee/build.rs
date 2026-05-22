use std::io::Write;
use std::{fs, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap() != "riscv64" {
        let mut f = fs::File::create(out_dir.join("enclave_assets.rs")).unwrap();
        writeln!(f, "pub static ENCLAVE_EAPP:   &[u8] = &[];").unwrap();
        writeln!(f, "pub static ENCLAVE_RT:     &[u8] = &[];").unwrap();
        writeln!(f, "pub static ENCLAVE_LOADER: &[u8] = &[];").unwrap();
        return;
    }

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let enclave_dir = manifest.parent().unwrap().join("bqti-enclave");
    let enclave_dir = enclave_dir.to_str().unwrap().to_string();

    let enclave_build =
        std::env::var("BQTI_ENCLAVE_BUILD").unwrap_or_else(|_| format!("{}/build", enclave_dir));

    let sdk = std::env::var("KEYSTONE_SDK_DIR")
        .unwrap_or_else(|_| format!("{}/vendor/keystone/sdk", enclave_dir));

    let keystone_dir = std::env::var("KEYSTONE_DIR").unwrap_or_else(|_| {
        format!(
            "{}/development/thesis-testing/keystone",
            std::env::var("HOME").unwrap()
        )
    });

    let riscv_gcc =
        std::env::var("RISCV_GCC").unwrap_or_else(|_| "riscv64-linux-gnu-gcc".to_string());

    let sysroot = std::env::var("RISCV_SYSROOT").ok();

    std::fs::create_dir_all(&enclave_build).unwrap();

    let mut cmake_args = vec![
        "-S".to_string(),
        enclave_dir.clone(),
        "-B".to_string(),
        enclave_build.clone(),
        format!(
            "-DCMAKE_TOOLCHAIN_FILE={}/vendor/keystone/toolchainfile.cmake",
            enclave_dir
        ),
        format!("-DKEYSTONE_SDK_DIR={}", sdk),
        format!("-DCMAKE_C_COMPILER={}", riscv_gcc),
        format!("-DCMAKE_CXX_COMPILER={}", riscv_gcc.replace("gcc", "g++")),
        "-DCMAKE_SYSTEM_NAME=Linux".to_string(),
        "-DCMAKE_SYSTEM_PROCESSOR=riscv64".to_string(),
        format!("-DKEYSTONE_RUNTIME={}/runtime", keystone_dir),
    ];

    if let Some(ref s) = sysroot {
        cmake_args.push(format!("-DCMAKE_SYSROOT={}", s));
        cmake_args.push(format!("-DCMAKE_FIND_ROOT_PATH={}", s));
    }

    let status = std::process::Command::new("cmake")
        .args(&cmake_args)
        .status()
        .expect("cmake configure failed");
    assert!(status.success(), "cmake configure failed");

    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .to_string();

    let status = std::process::Command::new("cmake")
        .args([
            "--build",
            &enclave_build,
            "--target",
            "examples",
            "-j",
            &jobs,
        ])
        .status()
        .expect("cmake build failed");

    assert!(status.success(), "cmake build failed");

    println!("cargo:rustc-link-search={}", enclave_build);
    println!("cargo:rustc-link-lib=static=enclave-ffi");
    println!("cargo:rustc-link-search={}/lib", sdk);
    println!("cargo:rustc-link-lib=static=keystone-host");
    println!("cargo:rustc-link-lib=static=keystone-edge");
    println!("cargo:rustc-link-lib=static=keystone-verifier");
    println!("cargo:rustc-link-lib=stdc++");

    println!("cargo:rerun-if-changed=../bqti-enclave/src/host/enclave_ffi.cpp");
    println!("cargo:rerun-if-changed=../bqti-enclave/include/enclave_ffi.h");
    println!("cargo:rerun-if-env-changed=KEYSTONE_DIR");
    println!("cargo:rerun-if-env-changed=RISCV_GCC");
    println!("cargo:rerun-if-env-changed=RISCV_SYSROOT");

    for asset in &["bqti", "eyrie-rt", "loader.bin"] {
        fs::copy(format!("{}/{}", enclave_build, asset), out_dir.join(asset))
            .unwrap_or_else(|_| panic!("missing enclave asset: {}", asset));
    }

    let mut f = fs::File::create(out_dir.join("enclave_assets.rs")).unwrap();
    let out = out_dir.display();
    writeln!(
        f,
        "pub static ENCLAVE_EAPP:   &[u8] = include_bytes!(\"{out}/bqti\");"
    )
    .unwrap();
    writeln!(
        f,
        "pub static ENCLAVE_RT:     &[u8] = include_bytes!(\"{out}/eyrie-rt\");"
    )
    .unwrap();
    writeln!(
        f,
        "pub static ENCLAVE_LOADER: &[u8] = include_bytes!(\"{out}/loader.bin\");"
    )
    .unwrap();
}
