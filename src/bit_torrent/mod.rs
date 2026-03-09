use serde_bytes::ByteBuf;

pub mod bencode;
pub mod torrent;

type ByteSize = i64;
type PieceByte = ByteBuf;
type Hash2OBytes = [u8; 20];
type EncodedBytes = Vec<u8>;
