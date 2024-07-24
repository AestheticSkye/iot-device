pub mod print;

use alloc::collections::vec_deque::VecDeque;
use alloc::string::{FromUtf8Error, String};
use alloc::vec::Vec;
use defmt::info;
use embassy_futures::join::join;
use embassy_futures::select::{select, Either};
use embassy_futures::yield_now;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, Instance, InterruptHandler};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::{Builder, Config};
use portable_atomic::AtomicBool;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

static STD_IN: Mutex<CriticalSectionRawMutex, VecDeque<u8>> = Mutex::new(VecDeque::new());
static STD_OUT: Mutex<CriticalSectionRawMutex, Vec<u8>> = Mutex::new(Vec::new());

static SERIAL_CONNECTED: AtomicBool = AtomicBool::new(false);

/// Whether or not serial has been enabled and connected.
/// This should be run after [`init_serial`] in a loop until true is returned.
pub fn enabled() -> bool {
    SERIAL_CONNECTED.fetch_and(true, portable_atomic::Ordering::Acquire)
}

const USB_CONFIG: Config = {
    // Create embassy-usb Config
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Embassy");
    config.product = Some("USB-serial example");
    config.serial_number = Some("12345678");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;
    config
};

/// Initialise serial communication through the USB bus.
/// This **must** be run before any usage of [`print!`] or [`println!`]
#[embassy_executor::task]
pub async fn init_serial(usb: USB) {
    // Create the driver, from the HAL.
    let driver = Driver::new(usb, Irqs);

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0; 64];

    let mut state = State::new();

    let mut builder = Builder::new(
        driver,
        USB_CONFIG,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [], // no msos descriptors
        &mut control_buf,
    );

    // Create classes on the builder.
    let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    // Do stuff with the class!
    let scan_fut = async {
        loop {
            class.wait_connection().await;
            info!("Serial Connected");
            SERIAL_CONNECTED.store(true, portable_atomic::Ordering::SeqCst);
            let _ = scan_serial(&mut class).await;
            info!("Serial Disconnected");
        }
    };

    join(usb_fut, scan_fut).await;
}

/// Cycles through reading data from the serial and placing it in [`STD_IN`] and flushing any data from [`STD_OUT`] to serial.
async fn scan_serial<'d, T: Instance + 'd>(
    class: &mut CdcAcmClass<'d, Driver<'d, T>>,
) -> Result<(), Disconnected> {
    let mut buf = [0; 64];
    loop {
        let read_fut = class.read_packet(&mut buf);
        let yield_fut = yield_now();

        match select(read_fut, yield_fut).await {
            Either::First(read_count) => {
                let data = &buf[..read_count?];
                class.write_packet(data).await?;

                let mut std_in = STD_IN.lock().await;

                for byte in data {
                    std_in.push_back(*byte);
                }
            }
            Either::Second(()) => {
                let mut std_out = STD_OUT.lock().await;

                if std_out.is_empty() {
                    continue;
                }

                let packets = std_out.chunks(64);

                for packet in packets {
                    class.write_packet(packet).await?;
                }

                *std_out = Vec::new();
            }
        }
    }
}

/// Reads text from the [`STD_IN`] until a line feed or carraige return is read and appends it to the provided `buffer`.
///
/// The first line feed or carraige return character is placed into the `buffer`, but any after is dropped.
#[allow(clippy::significant_drop_tightening)]
pub async fn read_line(buffer: &mut String) -> Result<usize, FromUtf8Error> {
    loop {
        Timer::after_millis(1).await;

        let mut std_in = STD_IN.lock().await;

        if !std_in.iter().any(|byte| *byte == b'\r' || *byte == b'\n') {
            continue;
        }

        let mut count = 0;

        // Push each char onto the string.
        while let Some(byte) = std_in.pop_front() {
            buffer.push(byte as char);
            count += 1;

            if byte == b'\r' || byte == b'\n' {
                break;
            }
        }

        // Remove any remaining cr or lf.
        if std_in
            .front()
            .is_some_and(|byte| *byte == b'\r' || *byte == b'\n')
        {
            std_in.pop_front();
        }

        return Ok(count);
    }
}

pub struct Disconnected {}

#[allow(clippy::fallible_impl_from)]
impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Self {},
        }
    }
}
