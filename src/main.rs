#![no_std]
#![no_main]
#![allow(clippy::future_not_send)]
#![allow(clippy::large_futures)]

extern crate alloc;

mod allocator;
mod networking;
mod serial;

use alloc::string::String;
use embassy_executor::Spawner;
use embassy_rp::config::Config;
use embassy_time::Timer;

use reqwless::request::Method;
use serde::Deserialize;
use serial::{init_serial, read_line};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    allocator::init();

    let peripherals = embassy_rp::init(Config::default());

    spawner.spawn(init_serial(peripherals.USB)).unwrap();

    while !serial::enabled() {
        Timer::after_millis(10).await;
    }

    print!("Enter SSID: ");

    let mut ssid = String::new();
    read_line(&mut ssid).await.expect("Failed to read line");

    print!("Enter Password: ");

    let mut password: String = String::new();
    read_line(&mut password).await.expect("Failed to read line");

    println!("Attempting to connect to `{}`", ssid.trim());

    let client = networking::Client::new(
        &spawner,
        ssid.trim(),
        password.trim(),
        peripherals.PIN_23,
        peripherals.PIN_24,
        peripherals.PIN_25,
        peripherals.PIN_29,
        peripherals.PIO0,
        peripherals.DMA_CH0,
    )
    .await;

    println!("Connected to `{}`", ssid.trim());

    drop(ssid);
    drop(password);

    let body = client
        .request(
            "http://worldtimeapi.org/api/timezone/Europe/Berlin",
            Method::GET,
            None,
            None,
        )
        .await
        .unwrap();

    let response: ApiResponse = serde_json_core::from_str(&body).unwrap().0;

    println!("{}", response.datetime);
}

#[derive(Deserialize)]
struct ApiResponse<'a> {
    datetime: &'a str,
}
