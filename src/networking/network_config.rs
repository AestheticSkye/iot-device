use embassy_net::{Ipv4Address, StaticConfigV4};
use heapless::{String, Vec};

use crate::{print, println, serial::read_line};

pub struct NetworkConfig {
    pub ssid: String<32>,
    pub password: Option<String<64>>,
    pub ip_config: Option<StaticConfigV4>,
}

impl NetworkConfig {
    pub async fn generate() -> Self {
        let ssid = loop {
            let mut ssid = String::<32>::new();
            print!("Enter SSID: ");
            read_line(&mut ssid).await.ok();
            if !ssid.trim().is_empty() {
                break ssid;
            }
            println!("SSID can not be blank");
        };

        // Retry if password is under 8 chars as the spec requires it to be 8 or over.
        let password = loop {
            let mut password = String::<64>::new();
            print!("Enter Password (leave blank for open network): ");
            read_line(&mut password).await.ok();
            match password.trim().len() {
                8.. => break Some(password),
                0 => break None,
                _ => println!("Password must have more than 8 characters"),
            }
        };

        let ip_config = loop {
            let mut choice = String::<2>::new();
            print!("Use DHCP? [Y/n] ");
            read_line(&mut choice).await.ok();
            match choice.trim().chars().next() {
                Some('y') | None => break None,
                Some('n') => break Some(Self::create_static_config().await),
                _ => {}
            }
        };

        Self {
            ssid,
            password,
            ip_config,
        }
    }

    async fn create_static_config() -> StaticConfigV4 {
        let address = loop {
            print!("Enter device address with subnet [eg: `192.128.1.1/24`]: ");
            let mut buf = String::<64>::new();
            read_line(&mut buf).await.ok();
            match buf.trim().parse() {
                Ok(address) => break address,
                Err(()) => println!("Incorrect IPv4 address inputted"),
            }
        };

        let gateway = loop {
            print!("Enter Gateway [eg: `192.128.1.0`] (leave blank for none): ");
            let mut buf = String::<64>::new();
            read_line(&mut buf).await.ok();
            if buf.is_empty() {
                break None;
            }
            match buf.trim().parse() {
                Ok(gateway) => break Some(gateway),
                Err(()) => println!("Incorrect IPv4 address inputted"),
            };
        };

        let mut dns_servers: Vec<Ipv4Address, 3> = Vec::new();

        loop {
            if dns_servers.is_full() {
                break;
            }
            print!(
                "Enter DNS server {}. [eg: `1.1.1.1`] (leave blank for none): ",
                dns_servers.len() + 1
            );
            let mut buf = String::<64>::new();
            read_line(&mut buf).await.ok();
            if buf.trim().is_empty() {
                break;
            }
            match buf.trim().parse() {
                Ok(dns_server) => _ = dns_servers.push(dns_server),
                Err(()) => println!("Incorrect IPv4 address inputted"),
            }
        }

        StaticConfigV4 {
            address,
            gateway,
            dns_servers,
        }
    }
}
