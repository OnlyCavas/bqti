use crate::dht::{KEY_ID_LENGTH, Key, Node, OrdDistance, RpcHandler, k_bucket::KBucket};

const KBUCKET_MAX: usize = 20;

pub struct RouteTable {
    pub host: Node,
    kbuckets: Vec<KBucket>,
}

impl RouteTable {
    pub fn new(host: Node) -> Self {
        let kbuckets = Self::gen_kbuckets();

        Self {
            host: host,
            kbuckets,
        }
    }

    pub fn host(&self) -> &Node {
        &self.host
    }

    fn gen_kbuckets() -> Vec<KBucket> {
        (0..KEY_ID_LENGTH * 8)
            .map(|depth| KBucket::new(KBUCKET_MAX, depth))
            .collect()
    }

    fn get_bucket_index(&self, key: &Key) -> usize {
        let distance = self.host.id.distance(key);

        for i in 0..KEY_ID_LENGTH {
            for j in (0..8).rev() {
                if (distance.0[i] >> j) & 1 != 0 {
                    return i * 8 + (7 - j);
                }
            }
        }

        KEY_ID_LENGTH * 8 - 1
    }

    pub async fn insert_node(&mut self, node: &Node) {
        let index = self.get_bucket_index(&node.id);

        let Some(kbucket) = self.kbuckets.get_mut(index) else {
            return;
        };

        if !kbucket.is_full() {
            kbucket.insert(node.clone());
            return;
        }

        let Some(_oldest_node) = kbucket.get_oldest_node() else {
            return;
        };

        // TODO ping the oldest node
        // let Err(_) = DHTNode::ping(&self.host, &oldest_node).await else {
        //     return;
        // };

        kbucket.envict_and_insert(node.clone());
    }

    pub fn remove(&mut self, node: &Node) {
        let kbucket_index = self.get_bucket_index(&node.id);

        let Some(kbucket) = self.kbuckets.get_mut(kbucket_index) else {
            return;
        };

        kbucket.remove(&node.id);
    }

    pub fn get_closest_nodes(&self, key: &Key, count: usize) -> Vec<&Node> {
        let mut distances: Vec<_> = self
            .kbuckets
            .iter()
            .flat_map(|bucket| bucket.get_nodes())
            .filter(|node| node.id != self.host.id)
            .map(|node| (key.distance(&node.id), node))
            .collect();

        distances.sort_by(|(a, _), (b, _)| a.cmp(b));

        distances
            .into_iter()
            .take(count)
            .map(|(_, node)| node)
            .collect()
    }
}
