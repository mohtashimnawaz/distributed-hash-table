use crate::kad::{KadRequest, KadResponse, Node, NodeId, RoutingTable};
use bincode;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

pub struct KadNode {
    pub me: Node,
    rt: Arc<Mutex<RoutingTable>>,
    socket: Arc<UdpSocket>,
}

impl KadNode {
    pub async fn bind(me: Node, bind_addr: SocketAddr) -> anyhow::Result<Self> {
        let socket = UdpSocket::bind(bind_addr).await?;
        let rt = RoutingTable::new(me.id);
        Ok(Self { me, rt: Arc::new(Mutex::new(rt)), socket: Arc::new(socket) })
    }

    pub async fn start(self: Arc<Self>) -> anyhow::Result<()> {
        let mut buf = [0u8; 2048];
        loop {
            let (len, src) = self.socket.recv_from(&mut buf).await?;
            let data = &buf[..len];
            let req: Result<KadRequest, _> = bincode::deserialize(data);
            match req {
                Ok(r) => {
                    let s = self.clone();
                    tokio::spawn(async move { s.handle_request(r, src).await.unwrap(); });
                }
                Err(e) => {
                    debug!("Failed to deserialize request: {}", e);
                }
            }
        }
    }

    async fn handle_request(self: Arc<Self>, req: KadRequest, src: SocketAddr) -> anyhow::Result<()> {
        match req {
            KadRequest::Ping { from } => {
                info!("Received Ping from {}", from.addr);
                // update routing table
                let mut rt = self.rt.lock().await;
                rt.add_node(from.clone());
                drop(rt);
                // respond Pong
                let resp = KadResponse::Pong { from: self.me.clone() };
                let b = bincode::serialize(&resp)?;
                let _ = self.socket.send_to(&b, src).await?;
            }
            KadRequest::FindNode { from, target } => {
                let mut rt = self.rt.lock().await;
                let closest = rt.find_closest(&target, 8);
                rt.add_node(from.clone());
                drop(rt);
                let resp = KadResponse::Nodes { from: self.me.clone(), nodes: closest };
                let b = bincode::serialize(&resp)?;
                let _ = self.socket.send_to(&b, src).await?;
            }
            KadRequest::FindValue { from, key: _ } => {
                // Not implemented value storage yet
                let mut rt = self.rt.lock().await;
                rt.add_node(from.clone());
                drop(rt);
                let resp = KadResponse::Value { from: self.me.clone(), value: None };
                let b = bincode::serialize(&resp)?;
                let _ = self.socket.send_to(&b, src).await?;
            }
            KadRequest::Store { from, .. } => {
                let mut rt = self.rt.lock().await;
                rt.add_node(from.clone());
                drop(rt);
                // Ack w/ Pong
                let resp = KadResponse::Pong { from: self.me.clone() };
                let b = bincode::serialize(&resp)?;
                let _ = self.socket.send_to(&b, src).await?;
            }
        }
        Ok(())
    }

    pub async fn send_request(addr: SocketAddr, req: KadRequest) -> anyhow::Result<KadResponse> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let b = bincode::serialize(&req)?;
        socket.send_to(&b, addr).await?;
        let mut buf = [0u8; 2048];
        let (len, _) = socket.recv_from(&mut buf).await?;
        let resp: KadResponse = bincode::deserialize(&buf[..len])?;
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kad::NodeId;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn ping_between_nodes() -> anyhow::Result<()> {
        let id1 = NodeId::random();
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14000);
        let n1 = Node::new(id1, addr1);
        let server1 = Arc::new(KadNode::bind(n1, addr1).await?);
        let s1 = server1.clone();
        tokio::spawn(async move { let _ = s1.start().await; });

        let id2 = NodeId::random();
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14001);
        let n2 = Node::new(id2, addr2);
        let server2 = Arc::new(KadNode::bind(n2, addr2).await?);
        let s2 = server2.clone();
        tokio::spawn(async move { let _ = s2.start().await; });

        // Send ping from 2 -> 1
        let ping = KadRequest::Ping { from: Node::new(id2, addr2) };
        let resp = KadNode::send_request(addr1, ping).await?;
        match resp {
            KadResponse::Pong { from } => assert_eq!(from.addr.port(), 14000),
            _ => panic!("unexpected"),
        }

        Ok(())
    }
}
