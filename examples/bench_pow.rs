use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use bqti::{
    certs::{ActiveKeyIdentity, PublicKey},
    dht::{ProveChallenge, make_prover},
};
use tracing::error;

const DIFFICULTY: u32 = 24;
const ITERATIONS: u32 = 10_000;

fn main() -> anyhow::Result<()> {
    let identity = ActiveKeyIdentity::new().context("failed to create a testing keypair")?;
    let identity_sh_ptr = Arc::new(identity);

    let prover = make_prover(identity_sh_ptr.clone());
    let pub_key = identity_sh_ptr.pub_key();

    let mut times = Vec::with_capacity(ITERATIONS as usize);

    println!(
        "Running {} PoW iterations at difficulty={}...",
        ITERATIONS, DIFFICULTY
    );

    for i in 0..ITERATIONS {
        let challenge = i.wrapping_mul(0x9e3779b9);
        let start = Instant::now();

        match prover.prove(pub_key, challenge, DIFFICULTY) {
            Ok(_) => (),
            Err(_) => error!("failed to compute pow on interation {}", i + 1),
        }

        times.push(start.elapsed());
    }

    let total: Duration = times.iter().sum();
    let mean = total / ITERATIONS;
    let mean_ms = mean.as_secs_f64() * 1000.0;

    times.sort();
    let min_ms = times[0].as_secs_f64() * 1000.0;
    let max_ms = times[times.len() - 1].as_secs_f64() * 1000.0;
    let p50_ms = times[ITERATIONS as usize / 2].as_secs_f64() * 1000.0;
    let p95_ms = times[(ITERATIONS as f64 * 0.95) as usize].as_secs_f64() * 1000.0;

    println!(
        "Results ({} iterations, difficulty={}):",
        ITERATIONS, DIFFICULTY
    );

    println!("  Mean:   {:.2} ms", mean_ms);
    println!("  Min:    {:.2} ms", min_ms);
    println!("  Max:    {:.2} ms", max_ms);
    println!("  P50:    {:.2} ms", p50_ms);
    println!("  P95:    {:.2} ms", p95_ms);
    println!();

    let sybil_1000 = mean_ms * 1000.0 / 1000.0;
    let sybil_10000 = mean_ms * 10000.0 / 1000.0;

    println!("Sybil cost (attacker best case = min {:.2} ms):", min_ms);

    println!(
        "  1,000 identities:  {:.1} seconds ({:.1} minutes)",
        min_ms * 1000.0 / 1000.0,
        min_ms * 1000.0 / 60000.0
    );
    println!(
        "  10,000 identities: {:.1} seconds ({:.1} minutes)",
        min_ms * 10000.0 / 1000.0,
        min_ms * 10000.0 / 60000.0
    );

    println!();
    println!("Sybil cost (mean {:.2} ms):", mean_ms);

    println!(
        "  1,000 identities:  {:.1} seconds ({:.1} minutes)",
        sybil_1000,
        sybil_1000 / 60.0
    );

    println!(
        "  10,000 identities: {:.1} seconds ({:.1} minutes)",
        sybil_10000,
        sybil_10000 / 60.0
    );

    Ok(())
}
