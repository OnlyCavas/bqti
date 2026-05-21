fn main() {
    #[cfg(target_arch = "riscv64")]
    {
        use bqti_tee::TeeExecute;
        let tee = bqti_tee::Tee::new();

        match tee.pow(42, 10) {
            Ok(r) => {
                println!("nonce:   {}", r.nonce);
                println!("hash:    {}", hex::encode(&r.hash));
                println!("pub_key: {}", hex::encode(&r.pub_key));
                println!("sig:     {}", hex::encode(&r.sig));
            }
            Err(e) => eprintln!("error: {}", e),
        }
    }

    #[cfg(not(target_arch = "riscv64"))]
    println!("TEE not supported on this arch");
}
