use distributed_hash_table::kad::{KadNode, KadRequest, Node, NodeId};
use env_logger;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage:\n  serve <port>         Start a node binding to port\n  lookup <port>        Run a sample lookup from node at port");
        return Ok(());
    }

    match args[1].as_str() {
        "serve" => {
            let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4000);
            let id = NodeId::random();
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            let node = Node::new(id, addr);
            let server = Arc::new(KadNode::bind(node, addr).await?);
            println!("Serving on {}", addr);
            let s1 = server.clone();
            let s2 = server.clone();
            tokio::spawn(async move { let _ = s1.start().await; });
            tokio::spawn(async move { let _ = s2.start_tcp(addr).await; });
            // keep running
            futures::future::pending::<()>().await;
        }
        "store" => {
            // store <target_port> <key> <value>
            let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4000);
            let key = args.get(3).expect("key required").as_bytes().to_vec();
            let value = args.get(4).expect("value required").as_bytes().to_vec();

            // ephemeral node to act as requester
            let id = NodeId::random();
            let mut addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
            let node = Node::new(id, addr);
            let server = Arc::new(KadNode::bind(node, addr).await?);
            let local_addr = server.me.addr;
            let s = server.clone();
            tokio::spawn(async move { let _ = s.start().await; });
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            let req = KadRequest::Store { from: server.me.clone(), key: key.clone(), value: value.clone() };
            let resp = KadNode::send_request_tcp(target, req).await?;
            println!("Store response: {:?}", resp);
        }
        "find" => {
            // find <target_port> <key>
            let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4000);
            let key = args.get(3).expect("key required").as_bytes().to_vec();

            let id = NodeId::random();
            let mut addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
            let node = Node::new(id, addr);
            let server = Arc::new(KadNode::bind(node, addr).await?);
            let s = server.clone();
            tokio::spawn(async move { let _ = s.start().await; });
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            let req = KadRequest::FindValue { from: server.me.clone(), key: key.clone() };
            let resp = KadNode::send_request_tcp(target, req).await?;
            println!("Find response: {:?}", resp);
        }
        _ => {
            println!("Unknown command");
        }
    }

    Ok(())
}
