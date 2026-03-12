use serde_bytes::ByteBuf;

pub mod bencode;
pub mod torrent;

type ByteSize = i64;
type PieceByte = ByteBuf;

type Hash2OBytes = [u8; 20];
type Hash32Bytes = [u8; 32];
type MerkleRoot = Hash32Bytes;

type EncodedBytes = Vec<u8>;
