//! Bounce packets received via udp.
use std::process;
use std::net::{SocketAddr, SocketAddrV4};

use smoltcp::Error;
use smoltcp::iface::{EthernetInterfaceBuilder, NeighborCache, Routes};
use smoltcp::socket::{UdpPacketMetadata, UdpSocket, SocketSet};
use smoltcp::storage::PacketBuffer;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpEndpoint, IpCidr};

use structopt::StructOpt;

use ixy::{self, IxyDevice, memory::Mempool};
use ixy_net::Phy;

#[derive(StructOpt)]
#[structopt(name="udp_forwarder", about="Udp forwarding interfacing with MoonGen")]
struct Options {
    #[structopt(short="i")]
    in_dev: String,
    #[structopt(short="I", parse(from_str="parse_addr"))]
    in_addr: IpEndpoint,
    #[structopt(short="o")]
    out_dev: String,
    #[structopt(short="O", parse(from_str="parse_addr"))]
    out_addr: IpEndpoint,
}

fn main() {
    env_logger::init();

    let Options { in_dev, in_addr, out_dev, out_addr } = Options::from_args();

    let in_phy = init_device(&in_dev);
    let out_phy = init_device(&out_dev);
    let mut neighbor_cache = [None; 8];
    let mut iface = EthernetInterfaceBuilder::new(in_phy)
        .ethernet_addr(EthernetAddress::from_bytes(&[00,0x1b,0x21,0x94,0xde,0xb4]))
        .ip_addrs([IpCidr::new(in_addr.addr, 24)])
        .neighbor_cache(NeighborCache::new(&mut neighbor_cache[..]))
        .finalize();

    let mut neighbor_cache = [None; 8];
    let mut oroutes = [None; 1];
    let routes = {
        let gateway = match out_addr.addr { 
            IpAddress::Ipv4(addr) => addr,
            _ => unreachable!("Only ipv4 addresses assigned to outgoing interface"),
        };
        let mut routes = Routes::new(&mut oroutes[..]);
        routes.add_default_ipv4_route(gateway).unwrap();
        routes
    };
    let mut oface = EthernetInterfaceBuilder::new(out_phy)
        .ethernet_addr(EthernetAddress::from_bytes(&[00,0x1b,0x21,0x94,0xde,0xb4]))
        .ip_addrs([IpCidr::new(out_addr.addr, 24)])
        .neighbor_cache(NeighborCache::new(&mut neighbor_cache[..]))
        .routes(routes)
        .finalize();

    let in_udp = socket_endpoint(in_addr);
    let mut in_socket = SocketSet::new(Vec::with_capacity(1));
    let out_udp = socket_endpoint(out_addr);
    let mut out_socket = SocketSet::new(Vec::with_capacity(1));

    // Add the sockets and turn it into a handle.
    let in_udp = in_socket.add(in_udp);
    let out_udp = out_socket.add(out_udp);

    loop {
        let now = Instant::now();
        oface.poll(&mut out_socket, now).unwrap_or_else(|err| {
            eprintln!("Error during receive, this may be normal: {:?}", err);
            false
        });
        iface.poll(&mut in_socket, now).unwrap_or_else(|err| {
            eprintln!("Error during receive, this may be normal: {:?}", err);
            false
        });

        let mut in_sock = in_socket.get::<UdpSocket>(in_udp);
        let mut out_sock = out_socket.get::<UdpSocket>(out_udp);

        // Bounce back every packet.
        let mut count = 0;
        loop {
            let (data, endpoint) = match in_sock.peek() {
                Ok((slice, endpoint)) => {
                    (slice, *endpoint)
                },
                Err(Error::Exhausted) => break,
                Err(err) => {
                    eprintln!("Receive error: {}", err);
                    break
                },
            };

            match out_sock.send_slice(data, endpoint) {
                Ok(_) => (),
                Err(Error::Exhausted) => {
                    break
                },
                Err(err) => {
                    eprintln!("Send error: {}", err);
                },
            }

            // Consume the peeked packet.
            let _= in_sock.recv();
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
        PacketBuffer::new(vec![UdpPacketMetadata::EMPTY; 128], vec![0; 4096]),
        PacketBuffer::new(vec![UdpPacketMetadata::EMPTY; 128], vec![0; 4096]));
    udp.bind(addr).unwrap();
    udp
}

fn parse_addr(arg: &str) -> IpEndpoint {
    let sock_addr: SocketAddr = arg.parse::<SocketAddrV4>().unwrap_or_else(|err| {
        eprintln!("Second argument not a valid `ip:port` tuple: {}", err);
        process::exit(1)
    }).into();

    sock_addr.into()
}
