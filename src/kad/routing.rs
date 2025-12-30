use crate::kad::{Node, NodeId};
use std::collections::VecDeque;
use std::net::SocketAddr;

const K: usize = 20; // bucket size
const RCACHE: usize = 20; // replacement cache size
const ID_BITS: usize = 256;

/// A single k-bucket storing up to K nodes in least-recently-seen order
/// and a replacement cache for nodes that couldn't be admitted when the
/// bucket was full.
#[derive(Debug, Default)]
pub struct KBucket {
    nodes: VecDeque<Node>,
    repl: VecDeque<Node>,
}

impl KBucket {
    pub fn new() -> Self {
        Self { nodes: VecDeque::new(), repl: VecDeque::new() }
    }

    /// Touch a node: if present move to most-recent; if absent and there
    /// is capacity, insert; otherwise place into replacement cache.
    pub fn touch(&mut self, node: Node) {
        // If exists, move to back (most-recent)
        if let Some(pos) = self.nodes.iter().position(|n| n.id == node.id) {
            self.nodes.remove(pos);
            self.nodes.push_back(node);
            return;
        }
        if self.nodes.len() < K {
            self.nodes.push_back(node);
        } else {
            // bucket full: add to replacement cache if not present
            if !self.repl.iter().any(|n| n.id == node.id) {
                if self.repl.len() >= RCACHE {
                    self.repl.pop_front();
                }
                self.repl.push_back(node);
            }
        }
    }

    /// Promote a replacement into the bucket by evicting the oldest node.
    /// Returns the id of the evicted node (if any) and the node that was promoted.
    pub fn promote_replacement(&mut self) -> Option<(Node, Node)> {
        if let Some(repl_node) = self.repl.pop_front() {
            if let Some(evicted) = self.nodes.pop_front() {
                self.nodes.push_back(repl_node.clone());
                return Some((evicted, repl_node));
            } else {
                // no eviction necessary
                self.nodes.push_back(repl_node.clone());
                return Some((repl_node.clone(), repl_node));
            }
        }
        None
    }

    /// Remove a node by id (e.g., detected dead) and if there is replacement
    /// available promote it into the bucket.
    pub fn remove_and_promote(&mut self, id: &NodeId) -> Option<Node> {
        if let Some(pos) = self.nodes.iter().position(|n| &n.id == id) {
            self.nodes.remove(pos);
            if let Some(repl_node) = self.repl.pop_front() {
                self.nodes.push_back(repl_node.clone());
                return Some(repl_node);
            }
        }
        None
    }

    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn contains(&self, id: &NodeId) -> bool {
        self.nodes.iter().any(|n| &n.id == id) || self.repl.iter().any(|n| &n.id == id)
    }
}

/// Routing table of 256 buckets
#[derive(Debug)]
pub struct RoutingTable {
    pub me: NodeId,
    buckets: Vec<KBucket>,
}

impl RoutingTable {
    pub fn new(me: NodeId) -> Self {
        let mut buckets = Vec::with_capacity(ID_BITS);
        for _ in 0..ID_BITS {
            buckets.push(KBucket::new());
        }
        Self { me, buckets }
    }

    pub fn add_node(&mut self, node: Node) {
        if node.id == self.me {
            return;
        }
        let idx = match self.me.bucket_index(&node.id) {
            Some(i) => i,
            None => return,
        };
        self.buckets[idx].touch(node);
    }

    /// If a node is known to be dead/unresponsive, remove it from its bucket
    /// and promote a replacement if available (returns the promoted node if any).
    pub fn remove_dead(&mut self, dead: &NodeId) -> Option<Node> {
        let idx = match self.me.bucket_index(dead) {
            Some(i) => i,
            None => return None,
        };
        self.buckets[idx].remove_and_promote(dead)
    }

    /// Return up to `count` closest nodes to `target` ordered by XOR distance
    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<Node> {
        let mut all: Vec<Node> = Vec::new();
        for b in &self.buckets {
            for n in b.iter() {
                all.push(n.clone());
            }
        }
        all.sort_by_key(|n| {
            let xor = n.id.xor(target);
            xor
        });
        all.into_iter().take(count).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn routing_add_and_find() {
        let me = NodeId::random();
        let mut rt = RoutingTable::new(me);

        for port in 10000..10010u16 {
            let id = NodeId::random();
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            rt.add_node(Node::new(id, addr));
        }

        let target = NodeId::random();
        let closest = rt.find_closest(&target, 5);
        assert!(closest.len() <= 5);
    }

    #[test]
    fn replacement_cache_promotes_on_dead() {
        let me = NodeId::random();
        let mut rt = RoutingTable::new(me);

        // choose two ids that map to the same bucket
        let id_base = NodeId::random();
        // Force all added nodes to be close to id_base by replacing their ids' last byte
        let mut nodes = Vec::new();
        for i in 0..(K as u8 + 3) {
            let mut b = NodeId::random().0;
            b[31] = i;
            let id = NodeId::from_bytes(b);
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 20000 + i as u16);
            nodes.push(Node::new(id, addr));
        }

        // Insert first K nodes -> fill bucket
        for n in nodes.iter().take(K) {
            rt.add_node(n.clone());
        }
        // Adding extra nodes should go into replacement cache
        for n in nodes.iter().skip(K) {
            rt.add_node(n.clone());
        }

        // pick the oldest in that bucket and remove it
        let oldest_id = nodes.first().unwrap().id;
        let promoted = rt.remove_dead(&oldest_id).unwrap();
        // promoted should be one of the nodes we added in the replacement cache
        assert!(nodes.iter().any(|n| n.id == promoted.id));
    }
}
