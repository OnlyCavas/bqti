use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{hasher::Sha256Hash, types::Hash32Bytes};
use rand::RngExt;

pub const KEY_ID_LENGTH: usize = 32;

pub trait OrdDistance {
    fn distance(&self, other: &Self) -> Self;
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Key {
    id: Hash32Bytes,
    pub_key: Vec<u8>,
}

impl Key {
    pub fn new(pub_key: &[u8]) -> Self {
        Key {
            id: *Sha256Hash::digest(pub_key).as_bytes(),
            pub_key: pub_key.to_vec(),
        }
    }

    pub fn id(&self) -> [u8; 32] {
        self.id
    }

    pub fn pub_key(&self) -> &[u8] {
        &self.pub_key
    }

    pub fn randomize(&self, index: usize) -> Self {
        let mut result = self.id;

        let byte_idx = index / 8;
        let bit_idx = index % 8;

        result[byte_idx] ^= 1 << (7 - bit_idx);

        let mut rng = rand::rng();
        for i in (index + 1)..256 {
            let b_idx = i / 8;
            let bt_idx = i % 8;

            if rng.random_bool(0.5) {
                result[b_idx] |= 1 << (7 - bt_idx);
            } else {
                result[b_idx] &= !(1 << (7 - bt_idx));
            }
        }

        Key::new(&result)
    }

    pub fn hex(&self) -> String {
        hex::encode(self.id)
    }
}

impl OrdDistance for Key {
    fn distance(&self, other: &Self) -> Self {
        let mut distance = [0; KEY_ID_LENGTH];

        for i in 0..KEY_ID_LENGTH {
            distance[i] = self.id[i] ^ other.id[i];
        }

        Key::new(&distance)
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;

        let mut t = s.serialize_tuple(2)?;
        t.serialize_element(&self.id)?;
        t.serialize_element(serde_bytes::Bytes::new(&self.pub_key))?;
        t.end()
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct KeyVisitor;
        impl<'de> serde::de::Visitor<'de> for KeyVisitor {
            type Value = Key;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a tuple of (32-byte id, public key bytes)")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Key, A::Error> {
                let id: Hash32Bytes = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::missing_field("id"))?;

                let pub_key: serde_bytes::ByteBuf = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::missing_field("pub_key"))?;

                Ok(Key {
                    id,
                    pub_key: pub_key.into_vec(),
                })
            }
        }

        d.deserialize_tuple(2, KeyVisitor)
    }
}
