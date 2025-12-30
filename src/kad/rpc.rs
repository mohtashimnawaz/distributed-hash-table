use crate::kad::{NodeId, Node};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum KadRequest {
    Ping { from: Node },
    FindNode { from: Node, target: NodeId },
    Store { from: Node, key: Vec<u8>, value: Vec<u8> },
    // FindValue can return nodes or value
    FindValue { from: Node, key: Vec<u8> },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum KadResponse {
    Pong { from: Node },
    Nodes { from: Node, nodes: Vec<Node> },
    Value { from: Node, value: Option<Vec<u8>> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kad::NodeId;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn serialize_ping() {
        let id = NodeId::random();
        let node = Node::new(id, std::net::SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4000));
        let req = KadRequest::Ping { from: node.clone() };
        let b = bincode::serialize(&req).unwrap();
        let out: KadRequest = bincode::deserialize(&b).unwrap();
        match out {
            KadRequest::Ping { from } => assert_eq!(from.addr.port(), 4000),
            _ => panic!("unexpected"),
        }
    }
}
