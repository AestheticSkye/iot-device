#![no_std]
#![no_main]
#![allow(clippy::future_not_send)]
#![allow(clippy::large_futures)]

// extern crate alloc;

mod allocator;
mod networking;
mod serial;

use defmt::unwrap;
use embassy_executor::Spawner;
use embassy_rp::config::Config;

use networking::{network_config::NetworkConfig, Client};
use reqwless::request::Method;
use serde::Deserialize;
use serial::init_serial;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    allocator::init();

    let peripherals = embassy_rp::init(Config::default());

    unwrap!(spawner.spawn(init_serial(peripherals.USB)));

    serial::wait_serial_up().await;

    let mut disconnected_client = Client::new(
        &spawner,
        peripherals.PIN_23,
        peripherals.PIN_24,
        peripherals.PIN_25,
        peripherals.PIN_29,
        peripherals.PIO0,
        peripherals.DMA_CH0,
    )
    .await;

    let client = loop {
        let network_config = NetworkConfig::generate().await;

        println!("Attempting to connect to `{}`", network_config.ssid.trim());

        match disconnected_client
            .connect(
                network_config.ssid.trim(),
                network_config.password.as_ref().map(|s| s.trim()),
                10,
                network_config.ip_config,
            )
            .await
        {
            Ok(client) => {
                println!("Connected to `{}`", network_config.ssid.trim());
                break client;
            }
            Err((error, client)) => {
                disconnected_client = client;
                println!("Failed to connect to network: `{error}`");
            }
        };
    };

    client.print_config().await;

    let mut buffer = Client::BLANK_REQUEST_BUFFER;
    let (_, data) = match client
        .request_with_data::<ApiResponse>(
            "http://worldtimeapi.org/api/timezone/Europe/Berlin",
            Method::GET,
            None,
            None,
            &mut buffer,
        )
        .await
    {
        Ok(body) => body,
        Err(err) => {
            println!("{err}");
            return;
        }
    };

    println!("{}", data.datetime);
}

#[derive(Deserialize, Default)]
struct ApiResponse<'a> {
    datetime: &'a str,
}
