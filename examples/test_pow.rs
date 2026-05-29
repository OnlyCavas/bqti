fn main() {
    #[cfg(target_arch = "riscv64")]
    {
        use bqti_tee::TeeExecute;
        let tee = bqti_tee::Tee::new();

        println!("testing pow calculation");

        match tee.pow(42, 12) {
            Ok(r) => {
                use bqti::certs::{ActiveKeyIdentity, Verifier};

                println!("nonce:   {}", r.nonce);
                println!("hash:    {}", hex::encode(&r.hash));
                println!("pub_key: {}", hex::encode(&r.pub_key));
                println!("sig:     {}", hex::encode(&r.sig));

                let result = ActiveKeyIdentity::verify(&r.pub_key, &r.hash, &r.sig);
                println!("Verify: {}", result);
            }
            Err(e) => eprintln!("error: {}", e),
        };

        println!("");

        let pub_key = match tee.get_pubkey() {
            Ok(k) => k,
            Err(e) => {
                eprintln!("error: {}", e);
                return;
            }
        };

        println!("{}", hex::encode(pub_key));

        println!("");

        let data = [1u8; 64];
        let signature = match tee.sign(&data) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("error: {}", e);
                return;
            }
        };

        println!("{}", hex::encode(signature));

        let attest_nonce = &[4u8];
        let report = match tee.attest(attest_nonce) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: {}", e);
                return;
            }
        };

        if report.verify(attest_nonce, None) {
            use bqti_tee::AttestReport;

            println!("report valid");
            println!("enclave hash:");

            match report {
                AttestReport::Keystone(k_report) => {
                    println!("{}", hex::encode(k_report.enclave.hash))
                }
            }
        } else {
            println!("report invalid");
        }
    }

    #[cfg(not(target_arch = "riscv64"))]
    println!("TEE not supported on this arch");
}
