use std::io::Write;
use std::{fs, path::PathBuf};

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap() != "riscv64" {
        return;
    }

    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let enclave_dir = format!("{}/../bqti-enclave", manifest);
    let enclave_build =
        std::env::var("BQTI_ENCLAVE_BUILD").unwrap_or_else(|_| format!("{}/build", enclave_dir));
    let sdk = std::env::var("KEYSTONE_SDK_DIR")
        .unwrap_or_else(|_| format!("{}/vendor/keystone/sdk", enclave_dir));

    if !std::path::Path::new(&format!("{}/bqti", enclave_build)).exists() {
        let keystone_dir = std::env::var("KEYSTONE_DIR").unwrap_or_else(|_| {
            format!(
                "{}/development/thesis-testing/keystone",
                std::env::var("HOME").unwrap()
            )
        });
        let riscv_gcc = format!(
            "{}/build-generic64/buildroot.build/per-package/keystone-examples/host/bin/riscv64-buildroot-linux-gnu-gcc",
            keystone_dir
        );
        let sysroot = format!(
            "{}/build-generic64/buildroot.build/per-package/keystone-examples/host/riscv64-buildroot-linux-gnu/sysroot",
            keystone_dir
        );

        std::fs::create_dir_all(&enclave_build).unwrap();

        let status = std::process::Command::new("cmake")
            .args([
                "-S",
                &enclave_dir,
                "-B",
                &enclave_build,
                &format!(
                    "-DCMAKE_TOOLCHAIN_FILE={}/vendor/keystone/toolchainfile.cmake",
                    enclave_dir
                ),
                &format!("-DKEYSTONE_SDK_DIR={}", sdk),
                &format!("-DCMAKE_C_COMPILER={}", riscv_gcc),
                &format!("-DCMAKE_CXX_COMPILER={}", riscv_gcc.replace("gcc", "g++")),
                "-DCMAKE_SYSTEM_NAME=Linux",
                "-DCMAKE_SYSTEM_PROCESSOR=riscv64",
                &format!("-DCMAKE_SYSROOT={}", sysroot),
                &format!("-DKEYSTONE_RUNTIME={}/runtime", keystone_dir),
                &format!("-DCMAKE_FIND_ROOT_PATH={}", sysroot),
            ])
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
    }

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

    println!("cargo:rerun-if-changed=../bqti-enclave/build/bqti");
    println!("cargo:rerun-if-changed=../bqti-enclave/build/eyrie-rt");
    println!("cargo:rerun-if-changed=../bqti-enclave/build/loader.bin");
}
