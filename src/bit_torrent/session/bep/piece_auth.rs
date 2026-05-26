use sha1::Digest;
use sha2::Sha256;

use crate::{
    certs::{ActiveKeyIdentity, Verifier},
    dht::{AuthError, ProveChallenge},
    types::Hash32Bytes,
};

const SIGNATURE_LENGTH: usize = 64;

#[cfg(not(feature = "ml_piece_sub"))]
fn piece_hash(index: u32, data: &[u8]) -> Hash32Bytes {
    let mut hasher = Sha256::new();

    hasher.update(&index.to_le_bytes());
    hasher.update(data);

    hasher.finalize().into()
}

#[cfg(feature = "ml_piece_sub")]
fn piece_hash(_index: u32, data: &[u8]) -> Hash32Bytes {
    let fake_index = rand::random::<u32>();
    let mut hasher = Sha256::new();

    hasher.update(&fake_index.to_le_bytes());
    hasher.update(data);

    hasher.finalize().into()
}

pub fn make_payload(
    index: u32,
    data: Vec<u8>,
    prover: &dyn ProveChallenge,
) -> Result<Vec<u8>, AuthError> {
    let hash = piece_hash(index, &data);
    let sig = prover.sign(&hash)?;

    let mut payload = data;
    payload.extend_from_slice(&sig);

    Ok(payload)
}

pub fn split_payload(mut payload: Vec<u8>) -> Option<(Vec<u8>, [u8; SIGNATURE_LENGTH])> {
    if payload.len() <= SIGNATURE_LENGTH {
        return None;
    }

    let sig_bytes = payload.split_off(payload.len() - SIGNATURE_LENGTH);
    let sig: [u8; SIGNATURE_LENGTH] = sig_bytes.try_into().ok()?;

    Some((payload, sig))
}

pub fn verify_piece(index: u32, data: &[u8], sig: &[u8; 64], pub_key: &[u8]) -> bool {
    ActiveKeyIdentity::verify(pub_key, &piece_hash(index, data), sig)
}
