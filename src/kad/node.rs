use crate::kad::NodeId;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub addr: SocketAddr,
}

impl Node {
    pub fn new(id: NodeId, addr: SocketAddr) -> Self {
        Self { id, addr }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn node_new() {
        let id = NodeId::random();
        let addr = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 4000);
        let n = Node::new(id, addr);
        assert_eq!(n.id, id);
    }
}
