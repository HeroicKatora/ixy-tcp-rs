//! Bounce packets received via udp.
use std::process;
use std::net::{SocketAddr, SocketAddrV4};
use std::time::Instant as StdInstant;

use smoltcp::Error;
use smoltcp::phy::Tracer;
use smoltcp::iface::{EthernetInterfaceBuilder, NeighborCache, Routes};
use smoltcp::socket::{UdpPacketMetadata, UdpSocket, SocketSet};
use smoltcp::storage::PacketBuffer;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, EthernetFrame, IpAddress, Ipv4Address, IpEndpoint, IpCidr, PrettyPrinter};

use structopt::StructOpt;

use ixy::{self, DeviceStats, IxyDevice, memory::Mempool};
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

struct Measure {
    time: StdInstant,
    stats_a: DeviceStats,
    stats_b: DeviceStats,
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

    let mut count = 0;
    let mut measure = Measure::new(iface.phy().ixy(), oface.phy().ixy());
    loop {
        let now = Instant::now();

        if count & 0xfff == 0 {
            measure.print(iface.phy().ixy(), oface.phy().ixy());
        }

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

        let _in_count = forward(&mut in_sock, &mut out_sock, options.forward_a());
        let _out_count = forward(&mut out_sock, &mut in_sock, options.forward_b());

        count += 1;
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
    let sock_addr = arg.parse::<SocketAddrV4>().unwrap_or_else(|err| {
        eprintln!("Second argument not a valid `ip:port` tuple: {}", err);
        process::exit(1)
    });

    let [a, b, c, d] = sock_addr.ip().octets();
    (Ipv4Address::new(a, b, c, d), sock_addr.port()).into()
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

impl Measure {
    pub fn new(phy1: &dyn IxyDevice, phy2: &dyn IxyDevice) -> Self {
        let mut stats_a = DeviceStats::default();
        let mut stats_b = DeviceStats::default();
        phy1.reset_stats();
        phy2.reset_stats();
        phy1.read_stats(&mut stats_a);
        phy2.read_stats(&mut stats_b);

        Measure {
            time: StdInstant::now(),
            stats_a,
            stats_b,
        }
    }

    pub fn print(&mut self, phy1: &dyn IxyDevice, phy2: &dyn IxyDevice) {
        let mut stats_a_new = DeviceStats::default();
        let mut stats_b_new = DeviceStats::default();

        let now = StdInstant::now();
        let elapsed = now - self.time;
        let nanos = 1_000_000_000*elapsed.as_secs() as u32 + elapsed.subsec_nanos();
        phy1.read_stats(&mut stats_a_new);
        phy2.read_stats(&mut stats_b_new);

        stats_a_new.print_stats_diff(phy1, &self.stats_a, nanos);
        stats_b_new.print_stats_diff(phy2, &self.stats_b, nanos);

        self.stats_a = stats_a_new;
        self.stats_b = stats_b_new;
        self.time = now;
    }
}
