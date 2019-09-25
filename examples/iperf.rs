//! A tcp client example
//!
//! Connects to a given remote tcp host and sends a single provided message. Any incoming data is
//! silently discarded without having been copied into a buffer (but no FIN sent).
//!
//! Prepend the ixy/ethox configuration to the usual iperf options. Call example:
//!
//! * `iperf3 '0000:01:00.0' 10.0.0.1/24 ab:ff:ff:ff:ff:ff 10.0.0.2/24 -c 10.0.0.2 5001 -n 10000 -l 1470 --udp`

use ethox::managed::{List, Slice};
use ethox::layer::{eth, ip};

use ethox_iperf::{config, iperf2};
use ixy_net::Phy;
use ixy::ixy_init;

fn main() {
    let config = config::Config::from_args();

    let ixy = ixy_init(&config.tap, 1, 1)
        .expect("Couldn't initialize ixy device");
    let pool = ixy.recv_pool(0).unwrap().clone();
    let mut interface = Phy::new(ixy, pool);

    let mut eth = eth::Endpoint::new(config.hostmac);

    let mut neighbors = [eth::Neighbor::default(); 1];
    let mut routes = [ip::Route::new_ipv4_gateway(config.gateway.address()); 1];
    let mut ip = ip::Endpoint::new(
        Slice::One(config.host.into()),
        ip::Routes::import(List::new_full(routes.as_mut().into())),
        eth::NeighborCache::new(&mut neighbors[..]));

    println!("[+] Configured layers, communicating");

    let result = match &config.iperf3 {
        config::Iperf3Config::Client(
            config::IperfClient { kind: config::ClientKind::Udp, client
        }) => {
            ethox_iperf::client(
                &mut interface,
                10,
                &mut eth,
                &mut ip,
                iperf2::Iperf::new(client),
            )
        },
        config::Iperf3Config::Client(
            config::IperfClient { kind: config::ClientKind::Tcp, client
        }) => {
            ethox_iperf::client(
                &mut interface,
                10,
                &mut eth,
                &mut ip,
                iperf2::IperfTcp::new(client),
            )
        },
    };

    println!("[+] Done\n");
    println!("{}", result);
}
