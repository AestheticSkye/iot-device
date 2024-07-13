#![no_std]
#![no_main]

extern crate alloc;

mod serial;
mod allocator;

use alloc::string::String;
use allocator::init_allocator;
use embassy_executor::Spawner;
use embassy_time::Timer;
use log::info;
use serial::{init_serial, read_line};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    init_allocator();

    let peripherals = embassy_rp::init(Default::default());

    spawner.spawn(init_serial(peripherals.USB)).unwrap();

    while !serial::serial_enabled() {
        Timer::after_millis(10).await;
    }

    loop {
        let mut buffer = String::new();
        if let Err(error) = read_line(&mut buffer).await {
            info!("{error}");
        }
        // info!("{}", buffer.trim())
        println!("You typed: {}", buffer.trim())
    }
}
