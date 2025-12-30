use distributed_hash_table::kad::{NodeId, Node, RoutingTable};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

fn main() {
    env_logger::init();
    println!("Kademlia-style DHT demo");

    let me = NodeId::random();
    let mut rt = RoutingTable::new(me);

    for port in 10000..10010u16 {
        let id = NodeId::random();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        rt.add_node(Node::new(id, addr));
    }

    let target = NodeId::random();
    let closest = rt.find_closest(&target, 5);
    println!("Found {} closest nodes", closest.len());
    for n in closest {
        println!("- {}:{}", n.addr.ip(), n.addr.port());
    }
}
