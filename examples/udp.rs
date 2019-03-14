//! Bounce packets received via udp.
use std::{env, process};
use std::collections::BTreeMap;
use std::net::{SocketAddr, SocketAddrV4};

use smoltcp::Error;
use smoltcp::iface::{EthernetInterfaceBuilder, NeighborCache};
use smoltcp::phy::EthernetTracer;
use smoltcp::socket::{UdpSocket, SocketSet};
use smoltcp::storage::PacketBuffer;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpEndpoint, IpCidr};
use ixy::{self, IxyDevice, memory::Mempool};
use ixy_net::Phy;

fn main() {
    env_logger::init();

    // Start up the network interface itself.
    let addr = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: cargo run --example udp <pci_bus_id> <udp_addr>");
        process::exit(1)
    });

    let udp_addr = parse_addr(env::args().nth(2).unwrap_or_else(|| {
        eprintln!("Usage: cargo run --example udp <pci_bus_id> <udp_addr>");
        process::exit(1)
    }));

    let phy = init_device(&addr);
    // let phy = EthernetTracer::new(phy, |time, printer| {
        // eprintln!("Trace: {}", printer);
    // });
    let mut iface = EthernetInterfaceBuilder::new(phy)
        .ethernet_addr(EthernetAddress::from_bytes(&[00,0x1b,0x21,0x94,0xde,0xb4]))
	.ip_addrs([IpCidr::new(udp_addr.addr, 24)])
        .neighbor_cache(NeighborCache::new(BTreeMap::new()))
        .finalize();

    let udp = socket_endpoint(udp_addr);
    let mut sockets = SocketSet::new(Vec::new());
    // Add the socket and turn it into a handle.
    let udp = sockets.add(udp);

    let mut buffer = Vec::new();

    loop {
        iface.poll(&mut sockets, Instant::now()).unwrap_or_else(|err| {
            eprintln!("Error during receive, this may be normal: {:?}", err);
            false
        });
        let mut socket = sockets.get::<UdpSocket>(udp);

        // Bounce back every packet.
	let mut count = 0;
        loop {
            let endpoint = match socket.recv() {
                Ok((slice, endpoint)) => {
                    buffer.clear();
                    buffer.extend_from_slice(slice);
                    endpoint
                },
                Err(Error::Exhausted) => break,
                Err(err) => {
                    eprintln!("Receive error: {}", err);
                    break
                },
            };

            socket.send_slice(&buffer, endpoint).unwrap_or_else(|err|
                eprintln!("Send error: {}", err));
            count += 1;
        }
	if count != 0 {
		eprintln!("Packets bounced: {}", count);
	}
    }
}

fn init_device(pci_addr: &str) -> Phy<Box<IxyDevice>> {
    // number of packets in the send mempool
    const NUM_PACKETS: usize = 2048;

    let device = ixy::ixy_init(pci_addr, 1, 1)
        .unwrap_or_else(|err| panic!("Couldn't initialize ixy device at {}: {:?}", pci_addr, err));
    let pool = Mempool::allocate(NUM_PACKETS, 0, &*device).unwrap();

    Phy::new(device, pool)
}

fn socket_endpoint(addr: IpEndpoint) -> UdpSocket<'static, 'static> {
    let mut udp = UdpSocket::new(
        PacketBuffer::new(Vec::new(), Vec::new()),
        PacketBuffer::new(Vec::new(), Vec::new()));
    udp.bind(addr).unwrap();
    udp
}

fn parse_addr(arg: String) -> IpEndpoint {
    let sock_addr: SocketAddr = arg.parse::<SocketAddrV4>().unwrap_or_else(|err| {
        eprintln!("Second argument not a valid `ip:port` tuple: {}", err);
        process::exit(1)
    }).into();

    sock_addr.into()
}
