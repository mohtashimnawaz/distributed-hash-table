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
            server.start().await?;
        }
        "lookup" => {
            let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4000);
            let id = NodeId::random();
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            let node = Node::new(id, addr);
            let server = Arc::new(KadNode::bind(node, addr).await?);
            // For demo purposes we won't start server loop (so it won't accept inbound requests),
            // but we can use send_request to query a peer. Not ideal for real network.
            println!("Created ephemeral node at {}\nYou can run `serve <port>` in another shell and then use lookup to query it.", addr);
        }
        _ => {
            println!("Unknown command");
        }
    }

    Ok(())
}
