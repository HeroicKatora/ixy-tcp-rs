//! Bounce packets received via udp.
use std::process;
use std::net::{SocketAddr, SocketAddrV4};

use smoltcp::Error;
use smoltcp::phy::{Tracer, KillSwitch};
use smoltcp::iface::{EthernetInterfaceBuilder, NeighborCache, Routes};
use smoltcp::socket::{UdpPacketMetadata, UdpSocket, SocketSet};
use smoltcp::storage::PacketBuffer;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, EthernetFrame, IpAddress, IpEndpoint, IpCidr, PrettyPrinter};

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
    #[structopt(short="r", parse(from_str="parse_addr"))]
    remote_a: IpEndpoint,
    #[structopt(short="o")]
    out_dev: String,
    #[structopt(short="O", parse(from_str="parse_addr"))]
    out_addr: IpEndpoint,
    #[structopt(short="s", parse(from_str="parse_addr"))]
    remote_b: IpEndpoint,
}

struct Forward {
    from: IpEndpoint,
    to: IpEndpoint,
}

fn main() {
    env_logger::init();

    let options = Options::from_args();

    let in_phy = init_device(&options.in_dev);
    let out_phy = init_device(&options.out_dev);
    let in_phy = Tracer::new(in_phy, |_time, pp: PrettyPrinter<EthernetFrame<&[u8]>>| {
        eprintln!("{}", pp);
    });
    let out_phy = Tracer::new(out_phy, |_time, pp: PrettyPrinter<EthernetFrame<&[u8]>>| {
        eprintln!("{}", pp);
    });
    let in_phy = KillSwitch::new(in_phy);
    let out_phy = KillSwitch::new(out_phy);
    let in_switch = in_phy.switch();
    let out_switch = out_phy.switch();

    let mut neighbor_cache = [None; 8];
    let mut neighbor_cache = NeighborCache::new(&mut neighbor_cache[..]);
    neighbor_cache.fill(
        options.remote_a.addr,
        EthernetAddress::from_bytes(&[0, 1, 2, 3, 4, 5]),
        Instant::now());
    let mut iroutes = [None; 1];
    let routes = {
        let gateway = match options.remote_a.addr { 
            IpAddress::Ipv4(addr) => addr,
            _ => unreachable!("Only ipv4 addresses assigned to outgoing interface"),
        };
        let mut routes = Routes::new(&mut iroutes[..]);
        routes.add_default_ipv4_route(gateway).unwrap();
        routes
    };
    let mut iface = EthernetInterfaceBuilder::new(in_phy)
        .ethernet_addr(EthernetAddress::from_bytes(&[00,0x1b,0x21,0x94,0xde,0xb4]))
        .ip_addrs([IpCidr::new(options.in_addr.addr, 24)])
        .neighbor_cache(neighbor_cache)
        .routes(routes)
        .finalize();

    let mut neighbor_cache = [None; 8];
    let mut neighbor_cache = NeighborCache::new(&mut neighbor_cache[..]);
    neighbor_cache.fill(
        options.remote_b.addr,
        EthernetAddress::from_bytes(&[0, 1, 2, 3, 4, 5]),
        Instant::now());
    let mut oroutes = [None; 1];
    let routes = {
        let gateway = match options.remote_b.addr { 
            IpAddress::Ipv4(addr) => addr,
            _ => unreachable!("Only ipv4 addresses assigned to outgoing interface"),
        };
        let mut routes = Routes::new(&mut oroutes[..]);
        routes.add_default_ipv4_route(gateway).unwrap();
        routes
    };
    let mut oface = EthernetInterfaceBuilder::new(out_phy)
        .ethernet_addr(EthernetAddress::from_bytes(&[00,0x1b,0x21,0x94,0xde,0xb5]))
        .ip_addrs([IpCidr::new(options.out_addr.addr, 24)])
        .neighbor_cache(neighbor_cache)
        .routes(routes)
        .finalize();

    let in_udp = socket_endpoint(options.in_addr);
    let mut in_socket = SocketSet::new(Vec::with_capacity(1));
    let out_udp = socket_endpoint(options.out_addr);
    let mut out_socket = SocketSet::new(Vec::with_capacity(1));

    // Add the sockets and turn it into a handle.
    let in_udp = in_socket.add(in_udp);
    let out_udp = out_socket.add(out_udp);

    let mut rx_disabled = false;
    loop {
        let now = Instant::now();
        iface.poll(&mut in_socket, now).unwrap_or_else(|err| {
            eprintln!("Error polling first socket, this may be normal: {:?}", err);
            false
        });
        oface.poll(&mut out_socket, now).unwrap_or_else(|err| {
            eprintln!("Error polling second socket, this may be normal: {:?}", err);
            false
        });

        let mut in_sock = in_socket.get::<UdpSocket>(in_udp);
        let mut out_sock = out_socket.get::<UdpSocket>(out_udp);

        let in_count = forward(&mut in_sock, &mut out_sock, options.forward_a());
        let out_count = forward(&mut out_sock, &mut in_sock, options.forward_b());
        let count = in_count + out_count;

        rx_disabled = !rx_disabled;
        in_switch.kill_rx(rx_disabled);
        out_switch.kill_rx(rx_disabled);

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

fn forward(in_sock: &mut UdpSocket, out_sock: &mut UdpSocket, config: Forward) -> usize {
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

        match out_sock.send_slice(data, config.to) {
            Ok(_) => count += 1,
            Err(Error::Exhausted) => {
                // Do nothing, we just drop that packet.
            },
            Err(err) => {
                eprintln!("Send error: {}", err);
            },
        }

        // Consume the peeked packet.
        let _= in_sock.recv();
    }
    count
}

impl Options {
    fn forward_a(&self) -> Forward {
        Forward {
            from: self.remote_a,
            to: self.remote_b,
        }
    }

    fn forward_b(&self) -> Forward {
        Forward {
            from: self.remote_b,
            to: self.remote_a,
        }
    }
}
