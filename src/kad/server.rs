use crate::kad::{KadRequest, KadResponse, Node, NodeId, RoutingTable, Store};
use bincode;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{Mutex, watch};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct KadNode {
    pub me: Node,
    rt: Arc<Mutex<RoutingTable>>,
    socket: Arc<UdpSocket>,
    store: Store,
}

impl KadNode {
    pub async fn bind(mut me: Node, bind_addr: SocketAddr) -> anyhow::Result<Self> {
        let socket = UdpSocket::bind(bind_addr).await?;
        // update the provided node with the actual local address (useful when binding to port 0)
        if me.addr.port() == 0 {
            let local = socket.local_addr()?;
            let port = local.port();
            // prefer explicit localhost for reachability in tests
            me.addr = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), port);
        }
        let rt = RoutingTable::new(me.id);
        let store = Store::new();
        Ok(Self { me, rt: Arc::new(Mutex::new(rt)), socket: Arc::new(socket), store })
    }

    pub async fn start(self: Arc<Self>) -> anyhow::Result<()> {
        // convenience method: start without shutdown control (never shuts down)
        let (_tx, rx) = watch::channel(false);
        self.start_with_shutdown(rx).await
    }

    /// Start UDP listener that can be canceled by sending `true` on the watch receiver
    pub async fn start_with_shutdown(self: Arc<Self>, mut shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
        let mut buf = [0u8; 2048];
        loop {
            tokio::select! {
                res = self.socket.recv_from(&mut buf) => {
                    match res {
                        Ok((len, src)) => {
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
                        Err(e) => {
                            warn!("udp recv error: {}", e);
                        }
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("UDP shutdown requested");
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    async fn process_request(&self, req: KadRequest, src: SocketAddr) -> anyhow::Result<KadResponse> {
        match req {
            KadRequest::Ping { from } => {
                info!("Received Ping from {}", from.addr);
                // update routing table
                let mut rt = self.rt.lock().await;
                rt.add_node(from.clone());
                drop(rt);
                Ok(KadResponse::Pong { from: self.me.clone() })
            }
            KadRequest::FindNode { from, target } => {
                let mut rt = self.rt.lock().await;
                let closest = rt.find_closest(&target, 8);
                rt.add_node(from.clone());
                drop(rt);
                Ok(KadResponse::Nodes { from: self.me.clone(), nodes: closest })
            }
            KadRequest::FindValue { from, key } => {
                // Check local store first
                let val = self.store.get(&key).await;
                let mut rt = self.rt.lock().await;
                rt.add_node(from.clone());
                drop(rt);
                if let Some(v) = val {
                    Ok(KadResponse::Value { from: self.me.clone(), value: Some(v) })
                } else {
                    // hash key -> NodeId and return closest nodes
                    let mut hasher = Sha256::new();
                    hasher.update(&key);
                    let hash = hasher.finalize();
                    let id: [u8; 32] = hash.as_slice().try_into().unwrap();
                    let target = NodeId::from_bytes(id);
                    let rt = self.rt.lock().await;
                    let closest = rt.find_closest(&target, 8);
                    drop(rt);
                    Ok(KadResponse::Nodes { from: self.me.clone(), nodes: closest })
                }
            }
            KadRequest::Store { from, key, value } => {
                // store locally
                self.store.put(key.clone(), value.clone()).await;
                let mut rt = self.rt.lock().await;
                rt.add_node(from.clone());
                drop(rt);
                // replicate to k closest based on key hash
                let mut hasher = Sha256::new();
                hasher.update(&key);
                let hash = hasher.finalize();
                let id: [u8; 32] = hash.as_slice().try_into().unwrap();
                let target = NodeId::from_bytes(id);
                let rt2 = self.rt.lock().await;
                let closest = rt2.find_closest(&target, 8);
                drop(rt2);
                for n in closest.iter() {
                    // best-effort replicate, log failure
                    if let Err(e) = KadNode::send_request(n.addr, KadRequest::Store { from: self.me.clone(), key: key.clone(), value: value.clone() }).await {
                        warn!("replication to {} failed: {}", n.addr, e);
                    }
                }

                Ok(KadResponse::Pong { from: self.me.clone() })
            }
        }
    }

    async fn handle_request(self: Arc<Self>, req: KadRequest, src: SocketAddr) -> anyhow::Result<()> {
        let resp = self.process_request(req, src).await?;
        let b = bincode::serialize(&resp)?;
        let _ = self.socket.send_to(&b, src).await?;
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

    /// Send a KadRequest over TCP (length-prefixed bincode frame)
    pub async fn send_request_tcp(addr: SocketAddr, req: KadRequest) -> anyhow::Result<KadResponse> {
        let mut stream = TcpStream::connect(addr).await?;
        let payload = bincode::serialize(&req)?;
        let len = (payload.len() as u32).to_be_bytes();
        stream.write_all(&len).await?;
        stream.write_all(&payload).await?;
        // read response length
        let mut lenb = [0u8; 4];
        stream.read_exact(&mut lenb).await?;
        let rlen = u32::from_be_bytes(lenb) as usize;
        let mut respb = vec![0u8; rlen];
        stream.read_exact(&mut respb).await?;
        let resp: KadResponse = bincode::deserialize(&respb)?;
        Ok(resp)
    }

    /// Start a TCP listener that accepts single-request connections and responds.
    pub async fn start_tcp(self: Arc<Self>, bind_addr: SocketAddr) -> anyhow::Result<()> {
        // convenience: never shuts down (forever)
        let (_tx, rx) = watch::channel(false);
        self.start_tcp_with_shutdown(bind_addr, rx).await
    }

    /// Start TCP listener that can be canceled by sending `true` on the watch receiver
    pub async fn start_tcp_with_shutdown(self: Arc<Self>, bind_addr: SocketAddr, mut shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
        let listener = TcpListener::bind(bind_addr).await?;
        loop {
            tokio::select! {
                acc = listener.accept() => {
                    match acc {
                        Ok((mut socket, peer)) => {
                            let s = self.clone();
                            tokio::spawn(async move {
                                if let Err(e) = s.handle_tcp_conn(&mut socket, peer).await {
                                    warn!("tcp handler error: {}", e);
                                }
                            });
                        }
                        Err(e) => warn!("tcp accept error: {}", e),
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("TCP shutdown requested");
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_tcp_conn(self: Arc<Self>, stream: &mut TcpStream, peer: SocketAddr) -> anyhow::Result<()> {
        // read len
        let mut lenb = [0u8; 4];
        stream.read_exact(&mut lenb).await?;
        let rlen = u32::from_be_bytes(lenb) as usize;
        let mut payload = vec![0u8; rlen];
        stream.read_exact(&mut payload).await?;
        let req: KadRequest = bincode::deserialize(&payload)?;
        let resp = self.process_request(req, peer).await?;
        let respb = bincode::serialize(&resp)?;
        let lenr = (respb.len() as u32).to_be_bytes();
        stream.write_all(&lenr).await?;
        stream.write_all(&respb).await?;
        Ok(())
    }

    /// Iterative FIND_NODE (simple variant)
    pub async fn find_node(&self, target: NodeId) -> anyhow::Result<Vec<Node>> {
        // add self to routing table and heartbeat
        let mut rt = self.rt.lock().await;
        rt.add_node(self.me.clone());
        drop(rt);
        use futures::future::join_all;
        use std::collections::{HashMap, HashSet};

        const ALPHA: usize = 3;
        const K_RETURN: usize = 20;

        let mut seen: HashMap<Vec<u8>, Node> = HashMap::new();
        let mut queried: HashSet<Vec<u8>> = HashSet::new();

        // seed shortlist
        let shortlist = { self.rt.lock().await.find_closest(&target, K_RETURN) };
        for n in shortlist.iter() {
            seen.insert(n.id.0.to_vec(), n.clone());
        }

        loop {
            // pick up to ALPHA closest unqueried nodes
            let mut candidates: Vec<Node> = seen.values()
                .filter(|n| !queried.contains(&n.id.0.to_vec()))
                .cloned()
                .collect();
            candidates.sort_by_key(|n| n.id.xor(&target));
            if candidates.is_empty() {
                break;
            }
            let round: Vec<Node> = candidates.into_iter().take(ALPHA).collect();

            // send FindNode to all in round
            let futures = round.iter().map(|n| {
                let addr = n.addr;
                let req = KadRequest::FindNode { from: self.me.clone(), target };
                async move { KadNode::send_request(addr, req).await }
            });
            let results = join_all(futures).await;

            let mut any_new = false;
            for (i, res) in results.into_iter().enumerate() {
                let node = &round[i];
                queried.insert(node.id.0.to_vec());
                if let Ok(resp) = res {
                    match resp {
                        KadResponse::Nodes { nodes, .. } => {
                            for n in nodes {
                                if !seen.contains_key(&n.id.0.to_vec()) {
                                    seen.insert(n.id.0.to_vec(), n.clone());
                                    any_new = true;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            if !any_new {
                break;
            }
        }

        // return up to K_RETURN closest
        let mut out: Vec<Node> = seen.values().cloned().collect();
        out.sort_by_key(|n| n.id.xor(&target));
        out.truncate(K_RETURN);
        Ok(out)
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
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let n1 = Node::new(id1, addr1);
        let server1 = Arc::new(KadNode::bind(n1, addr1).await?);
        let s1 = server1.clone();
        tokio::spawn(async move { let _ = s1.start().await; });

        let id2 = NodeId::random();
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let n2 = Node::new(id2, addr2);
        let server2 = Arc::new(KadNode::bind(n2, addr2).await?);
        let s2 = server2.clone();
        tokio::spawn(async move { let _ = s2.start().await; });

        // give time to bind
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Send ping from 2 -> 1
        let ping = KadRequest::Ping { from: server2.me.clone() };
        let resp = KadNode::send_request(server1.me.addr, ping).await?;
        match resp {
            KadResponse::Pong { from } => assert_eq!(from.addr.port(), server1.me.addr.port()),
            _ => panic!("unexpected"),
        }

        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn store_and_find_integration() -> anyhow::Result<()> {
        // start a small cluster of nodes (both UDP/TCP)
        let mut servers = Vec::new();
        async fn try_spawn_tcp() -> anyhow::Result<Arc<KadNode>> {
            // Try unspecified and localhost with more retries
            let mut attempts = 0;
            loop {
                attempts += 1;
                let candidates = [Ipv4Addr::UNSPECIFIED, Ipv4Addr::LOCALHOST];
                for cand in candidates.iter() {
                    let addr = SocketAddr::new(IpAddr::V4(*cand), 0);
                    let id = NodeId::random();
                    let n = Node::new(id, addr);
                    match KadNode::bind(n, addr).await {
                        Ok(s) => return Ok(Arc::new(s)),
                        Err(e) => println!("bind attempt failed for {}: {}", addr, e),
                    }
                }
                if attempts >= 12 {
                    return Err(anyhow::anyhow!("failed to bind after retries"));
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        let mut shutdowns: Vec<watch::Sender<bool>> = Vec::new();
        // create servers using UDP only (avoid TCP binds in tests)
        for _ in 0..2u16 {
            let server = try_spawn_tcp().await?;
            let s2 = server.clone();
            let (tx, rx) = watch::channel(false);
            let h1 = tokio::spawn(async move { let _ = s2.start_with_shutdown(rx).await; });
            println!("Started server {} at {}", servers.len(), server.me.addr);
            servers.push(server);
            handles.push(h1);
            shutdowns.push(tx);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        println!("Cluster started with {} nodes", servers.len());
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // ping nodes[0] from each other to populate routing tables (UDP)
        for i in 1..servers.len() {
            let src = &servers[i];
            let req = KadRequest::Ping { from: src.me.clone() };
            let r = tokio::time::timeout(std::time::Duration::from_secs(2), KadNode::send_request(servers[0].me.addr, req)).await;
            match r {
                Ok(Ok(_)) => println!("Pinged node0 from node {}", i),
                Ok(Err(e)) => println!("Ping error from node {}: {}", i, e),
                Err(_) => println!("Ping timed out from node {}", i),
            }
        }

        // store a key on node 0 (UDP)
        let key = b"mykey".to_vec();
        let val = b"the-value".to_vec();
        let req = KadRequest::Store { from: servers[0].me.clone(), key: key.clone(), value: val.clone() };
        let store_res = tokio::time::timeout(std::time::Duration::from_secs(2), KadNode::send_request(servers[0].me.addr, req)).await;
        println!("Store response: {:?}", store_res);

        // give some time for replication
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // query other nodes for the value (with timeouts)
        let mut found = false;
        for i in 0..servers.len() {
            println!("Querying node {}", i);
            let req = KadRequest::FindValue { from: servers[i].me.clone(), key: key.clone() };
            let resp_r = tokio::time::timeout(std::time::Duration::from_secs(2), KadNode::send_request(servers[i].me.addr, req)).await;
            match resp_r {
                Ok(Ok(resp)) => {
                    println!("Node {} response: {:?}", i, resp);
                    if let KadResponse::Value { value: Some(v), .. } = resp {
                        if v == val {
                            found = true;
                            break;
                        }
                    }
                }
                Ok(Err(e)) => println!("Error querying node {}: {}", i, e),
                Err(_) => println!("Query timed out for node {}", i),
            }
        }

        // shut down background tasks to free sockets
        for tx in shutdowns.into_iter() {
            let _ = tx.send(true);
        }
        // Give listeners a moment to clean up
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        for h in handles.into_iter() {
            h.abort();
        }

        assert!(found, "Did not find replicated value on cluster");
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn find_node_iterative() -> anyhow::Result<()> {
        // spawn a small network on ephemeral ports
        let mut servers = Vec::new();
        async fn try_spawn() -> anyhow::Result<Arc<KadNode>> {
            let mut attempts = 0;
            loop {
                attempts += 1;
                // try both unspecified and localhost
                let candidates = [Ipv4Addr::UNSPECIFIED, Ipv4Addr::LOCALHOST];
                for cand in candidates.iter() {
                    let addr = SocketAddr::new(IpAddr::V4(*cand), 0);
                    let id = NodeId::random();
                    let n = Node::new(id, addr);
                    match KadNode::bind(n, addr).await {
                        Ok(s) => return Ok(Arc::new(s)),
                        Err(e) => println!("bind attempt failed for {}: {}", addr, e),
                    }
                }
                if attempts >= 12 {
                    return Err(anyhow::anyhow!("failed to bind after retries"));
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        let mut shutdowns: Vec<watch::Sender<bool>> = Vec::new();
        for i in 0..3u16 {
            let server = try_spawn().await?;
            println!("Started server {} at {}", i, server.me.addr);
            let s = server.clone();
            let (tx, rx) = watch::channel(false);
            let h = tokio::spawn(async move { let _ = s.start_with_shutdown(rx).await; });
            servers.push(server);
            handles.push(h);
            shutdowns.push(tx);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Ping nodes to populate routing tables
        for i in 1..servers.len() {
            let src = &servers[i];
            let dest_addr = servers[0].me.addr;
            let ping = KadRequest::Ping { from: src.me.clone() };
            let _ = KadNode::send_request(dest_addr, ping).await?;
        }

        // Now ask server 0 to find some random target
        let target = NodeId::random();
        let found = servers[0].find_node(target).await?;

        // shut down background tasks
        for tx in shutdowns.into_iter() {
            let _ = tx.send(true);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        for h in handles.into_iter() {
            h.abort();
        }

        // Should return something (at least the nodes we pinged)
        assert!(!found.is_empty());
        Ok(())
    }
}
