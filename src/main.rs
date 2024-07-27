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

use networking::{Client, Connected, Disconnected};
use reqwless::request::Method;
use serde::Deserialize;
use serial::{init_serial, read_line};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    allocator::init();

    let peripherals = embassy_rp::init(Config::default());

    unwrap!(spawner.spawn(init_serial(peripherals.USB)));

    serial::wait_serial_up().await;

    let client: Client<Connected> = {
        let disconnected_client = Client::new(
            &spawner,
            peripherals.PIN_23,
            peripherals.PIN_24,
            peripherals.PIN_25,
            peripherals.PIN_29,
            peripherals.PIO0,
            peripherals.DMA_CH0,
        )
        .await;

        connect_to_network(disconnected_client).await
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

async fn connect_to_network(mut disconnected_client: Client<Disconnected>) -> Client<Connected> {
    loop {
        let ssid = loop {
            let mut ssid = heapless::String::<64>::new();
            print!("Enter SSID: ");
            _ = read_line(&mut ssid).await;
            if !ssid.trim().is_empty() {
                break ssid;
            }
            println!("SSID can not be blank");
        };

        // Retry if password is under 8 chars as the spec requires it to be 8 or over.
        let password = loop {
            let mut password = heapless::String::<64>::new();
            print!("Enter Password (leave blank for open network): ");
            _ = read_line(&mut password).await;
            match password.trim().len() {
                8.. => break Some(password),
                0 => break None,
                _ => println!("Password must have more than 8 characters"),
            }
        };

        println!("Attempting to connect to `{}`", ssid.trim());

        match disconnected_client
            .connect(ssid.trim(), password.as_ref().map(|s| s.trim()), 10)
            .await
        {
            Ok(client) => {
                println!("Connected to `{}`", ssid.trim());
                break client;
            }
            Err((error, client)) => {
                disconnected_client = client;
                println!("{error}");
            }
        };
    }
}
