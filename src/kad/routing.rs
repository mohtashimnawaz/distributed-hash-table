use crate::kad::{Node, NodeId};
use std::collections::VecDeque;
use std::net::SocketAddr;

const K: usize = 20; // bucket size
const ID_BITS: usize = 256;

/// A single k-bucket storing up to K nodes in least-recently-seen order.
#[derive(Debug, Default)]
pub struct KBucket {
    nodes: VecDeque<Node>,
}

impl KBucket {
    pub fn new() -> Self {
        Self { nodes: VecDeque::new() }
    }

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
            // naive: drop oldest
            self.nodes.pop_front();
            self.nodes.push_back(node);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter()
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
}
