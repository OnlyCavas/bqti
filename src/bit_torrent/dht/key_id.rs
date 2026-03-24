use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{hasher::Sha256Hash, types::Hash32Bytes};

pub const KEY_ID_LENGTH: usize = 32;

pub trait OrdDistance {
    fn distance(&self, other: &Self) -> Self;
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Key(pub Hash32Bytes);

impl Key {
    pub fn new(pub_key: &[u8]) -> Self {
        Key(*Sha256Hash::digest(pub_key).as_bytes())
    }

    pub fn hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl OrdDistance for Key {
    fn distance(&self, other: &Self) -> Self {
        let mut distance = [0; KEY_ID_LENGTH];

        for i in 0..KEY_ID_LENGTH {
            distance[i] = self.0[i] ^ other.0[i];
        }

        Key(distance)
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct KeyVisitor;

        impl<'de> serde::de::Visitor<'de> for KeyVisitor {
            type Value = Key;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "32 bytes")
            }

            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Key, E> {
                let arr: [u8; 32] = v
                    .try_into()
                    .map_err(|_| E::invalid_length(v.len(), &self))?;
                Ok(Key(arr))
            }

            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Key, E> {
                self.visit_bytes(&v)
            }
        }

        d.deserialize_bytes(KeyVisitor)
    }
}
