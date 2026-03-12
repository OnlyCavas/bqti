use serde_bytes::ByteBuf;

pub type UnixDate = u64;

pub type ByteSize = i64;
pub type PieceByte = ByteBuf;

pub type Hash2OBytes = [u8; 20];
pub type Hash32Bytes = [u8; 32];
pub type MerkleRoot = Hash32Bytes;

pub type EncodedBytes = Vec<u8>;
