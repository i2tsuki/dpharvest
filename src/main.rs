extern crate pnet;
extern crate chrono;
#[macro_use]
extern crate lazy_static;

use pnet::datalink::{self, NetworkInterface};
use pnet::datalink::Channel::Ethernet;
use pnet::packet::{Packet, PacketSize};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::Ipv4Packet;
// use pnet::packet::ipv6::Ipv6Packet;
use pnet::packet::tcp::TcpPacket;

use chrono::prelude::*;

use std::env;
use std::io::{self, Write};
use std::net::IpAddr;
use std::process;
use std::collections::HashMap;
use std::sync::Mutex;
use std::{thread, time};


lazy_static! {
    static ref BUCKET: Mutex<Bucket> = Mutex::new(Bucket::new());
    static ref PACKET: Mutex<i64> = Mutex::new(0);    
}

#[derive(Debug, Clone)]
struct Bucket {
    hashmap: HashMap<String, Grain>,
}

#[derive(Debug, Clone)]
struct Grain {
    timestamp: i64,
    duplicate: i64,
    flags: u16,
}

impl Bucket {
    fn new() -> Bucket {
        Bucket { hashmap: HashMap::new() }
    }

    fn collect(&mut self, ipv4: Ipv4Packet) {
        // let source = IpAddr::V4(ipv4.get_source());
        let destination = IpAddr::V4(ipv4.get_destination());
        match ipv4.get_next_level_protocol() {
            IpNextHeaderProtocols::Tcp => {
                let next = TcpPacket::new(ipv4.payload());
                match next {
                    Some(tcp) => {
                        let key =
                            format!(
                            "{},{},{},{},{}",
                            destination,
                            tcp.get_destination(),
                            tcp.get_sequence(),
                            tcp.get_acknowledgement(),    
                            tcp.packet_size(),
                        );
                        if self.hashmap.contains_key(&key) {
                            match self.hashmap.get_mut(&key) {
                                Some(grain) => grain.duplicate += 1,
                                None => (),
                            }
                        } else {
                            let grain = Grain {
                                timestamp: Local::now().timestamp(),
                                duplicate: 0,
                                flags: tcp.get_flags(),
                            };
                            self.hashmap.insert(key, grain);
                        }
                    }
                    None => (),
                }
            }
            _ => (),
        }
    }

    fn refresh(&mut self) {
        let threshold: i64 = 30;
        println!("------------------------------------------------------------------------");
        for (key, grain) in &self.clone().hashmap {
            if grain.duplicate >= 2 {
                println!("{}: {:?}", key, grain);
            }
            let now = Local::now().timestamp();
            if now - grain.timestamp > threshold {
                &self.hashmap.remove(key);
            }
        }
    }

    // fn handle_transport_protocol(
    //     interface_name: &str,
    //     source: IpAddr,
    //     destination: IpAddr,
    //     protocol: IpNextHeaderProtocol,
    //     packet: &[u8],
    // ) {
    //     match protocol {
    //         IpNextHeaderProtocols::Tcp => {
    //             handle_tcp_packet(interface_name, source, destination, packet);
    //         }
    //         _ => (),
    //     }
}

fn handle_ipv4_packet(ethernet: &EthernetPacket) {
    let header = Ipv4Packet::new(ethernet.payload());
    BUCKET.lock().unwrap().collect(header.unwrap());
}

// fn handle_ipv6_packet(interface_name: &str, ethernet: &EthernetPacket) {
//     let header = Ipv6Packet::new(ethernet.payload());
//     if let Some(header) = header {
//         handle_transport_protocol(
//             interface_name,
//             IpAddr::V6(header.get_source()),
//             IpAddr::V6(header.get_destination()),
//             header.get_next_header(),
//             header.payload(),
//         );
//     }
// }

fn handle_packet(ethernet: &EthernetPacket) {
    match ethernet.get_ethertype() {
        EtherTypes::Ipv4 => handle_ipv4_packet(ethernet),
        // EtherTypes::Ipv6 => handle_ipv6_packet(interface_name, ethernet),
        _ => (),
    }
}

fn main() {
    let iface_name = match env::args().nth(1) {
        Some(n) => n,
        None => {
            writeln!(io::stderr(), "USAGE: dupharvest <NETWORK INTERFACE>").unwrap();
            process::exit(1);
        }
    };
    let interface_names_match = |iface: &NetworkInterface| iface.name == iface_name;

    // Find the network interface with the provided name
    let interfaces = datalink::interfaces();
    let interface = interfaces
        .into_iter()
        .filter(interface_names_match)
        .next()
        .unwrap();

    // Create a channel to receive on
    let (_, mut rx) = match datalink::channel(&interface, Default::default()) {
        Ok(Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("packetdump: unhandled channel type: {}"),
        Err(e) => panic!("packetdump: unable to create channel: {}", e),
    };

    let mut rx_iter = rx.iter();

    thread::spawn(move || loop {
        {
            BUCKET.lock().unwrap().refresh();
            thread::sleep(time::Duration::from_secs(10));
        }
    });

    loop {
        match rx_iter.next() {
            Ok(packet) => {
                if packet.get_source() == interface.mac.unwrap() {
                    handle_packet(&packet);
                }
            }
            Err(e) => panic!("packetdump: unable to receive packet: {}", e),
        }
    }
}
