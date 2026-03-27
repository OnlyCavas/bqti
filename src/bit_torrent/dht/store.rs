use std::time::Duration;

use indexmap::IndexMap;

use crate::{
    dht::{KademliaData, Key},
    types::UnixDate,
    utils::bqti::fetch_current_timestamp,
};

pub const PRUNE_CHECK_DURATION: Duration = Duration::from_secs(300);
const MAX_STORE_ENTRIES: usize = 10_000;
const DEFAULT_TTL: UnixDate = 24 * 3600;

// TODO implement a rate limiter

#[derive(Debug, Clone)]
struct StoreValue {
    data: KademliaData,
    expires_at: UnixDate,
}

pub struct DHTStore {
    store: IndexMap<Key, StoreValue>,
    max_entries: usize,
}

impl DHTStore {
    pub fn new() -> Self {
        Self {
            store: IndexMap::new(),
            max_entries: MAX_STORE_ENTRIES,
        }
    }

    pub fn insert(&mut self, key: Key, value: KademliaData) {
        let expiry = fetch_current_timestamp() + DEFAULT_TTL;

        if let Some(entry) = self.store.get_mut(&key) {
            entry.data.merge(value);
            entry.expires_at = expiry;

            if let Some(entry_owned) = self.store.shift_remove(&key) {
                self.store.insert(key, entry_owned);
            }

            return;
        }

        if self.store.len() >= self.max_entries {
            self.store.shift_remove_index(0);
        }

        self.store.insert(
            key,
            StoreValue {
                data: value,
                expires_at: expiry,
            },
        );
    }

    pub fn prune(&mut self) {
        let now = fetch_current_timestamp();
        self.store.retain(|_, entry| entry.expires_at > now);
    }

    pub fn get(&self, key: &Key) -> Option<&KademliaData> {
        let now = fetch_current_timestamp();

        self.store.get(key).and_then(|entry| {
            if now > entry.expires_at {
                None
            } else {
                Some(&entry.data)
            }
        })
    }
}
