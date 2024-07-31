pub mod network_config;

use core::fmt::{Debug, Write};

use cyw43::Control;
use cyw43_pio::PioSpi;
use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    Config, DhcpConfig, StackResources, StaticConfigV4,
};
use embassy_rp::{
    bind_interrupts,
    clocks::RoscRng,
    gpio::{Level, Output},
    peripherals::{DMA_CH0, PIN_23, PIN_24, PIN_25, PIN_29, PIO0},
    pio::{InterruptHandler, Pio},
};
use embassy_time::{Instant, Timer};
use heapless::{String, Vec};
use rand::RngCore;
use reqwless::{
    client::{HttpClient, TlsConfig, TlsVerify},
    request::{Method, RequestBuilder},
    response::StatusCode,
};
use serde::Deserialize;
use serde_json_core::de;
use static_cell::StaticCell;
use thiserror_no_std::Error;

use crate::println;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

type Stack = embassy_net::Stack<cyw43::NetDriver<'static>>;

#[embassy_executor::task]
async fn wifi_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack) -> ! {
    stack.run().await
}

const RX_BUFFER_SIZE: usize = 8192;

pub struct Disconnected {
    control: Control<'static>,
}

pub struct Connected {
    config: StaticConfigV4,
}

pub struct Client<T> {
    stack: &'static Stack,
    seed: u64,
    state: T,
}

#[derive(Clone, Copy, Error)]
pub enum ConnectionError {
    #[error("SSID not found")]
    SsidNotFound,
    #[error("DHCP configuration can not be resolved from server, consider using a static config")]
    DhcpTimeout,
    #[error("A timeout occurred")]
    OtherTimeout,
    #[error("An unknown error occurred with code `{0}`")]
    UnknownError(u32),
}

#[derive(Error, Debug)]
pub enum RequestError {
    #[error("An error occured with the request: `{0}`")]
    NetworkError(String<64>),
    #[error("Request returned with status code `{0}`")]
    HttpCode(u16),
    #[error("Failed to decode response: `{0}`")]
    JsonDecodingError(#[from] de::Error),
    #[error("Failed to decode uf8: `{0}`")]
    UtfDecodingError(#[from] core::str::Utf8Error),
}

impl From<reqwless::Error> for RequestError {
    fn from(value: reqwless::Error) -> Self {
        let mut buf = String::new();
        write!(buf, "{value:?}").expect("Failed to write to error buffer");
        Self::NetworkError(buf)
    }
}

impl<'a> Client<Disconnected> {
    #[allow(clippy::items_after_statements)]
    pub async fn new(
        spawner: &'a Spawner,
        pin_23: PIN_23,
        pin_24: PIN_24,
        pin_25: PIN_25,
        pin_29: PIN_29,
        pio0: PIO0,
        dma_ch0: DMA_CH0,
    ) -> Self {
        let mut rng = RoscRng;

        // let fw = include_bytes!("../firmware/43439A0.bin");
        let clm = include_bytes!("../firmware/43439A0_clm.bin");

        let fw = unsafe { core::slice::from_raw_parts(0x1010_0000 as *const u8, 2_303_211) };
        // let clm = unsafe { core::slice::from_raw_parts(0x10140000 as *const u8, 4752) };

        let pwr = Output::new(pin_23, Level::Low);
        let cs = Output::new(pin_25, Level::High);
        let mut pio = Pio::new(pio0, Irqs);
        let spi = PioSpi::new(
            &mut pio.common,
            pio.sm0,
            pio.irq0,
            cs,
            pin_24,
            pin_29,
            dma_ch0,
        );

        static STATE: StaticCell<cyw43::State> = StaticCell::new();
        let state = STATE.init(cyw43::State::new());
        let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
        unwrap!(spawner.spawn(wifi_task(runner)));

        control.init(clm).await;
        control
            .set_power_management(cyw43::PowerManagementMode::PowerSave)
            .await;

        let config = Config::dhcpv4(DhcpConfig::default());

        // Generate random seed
        let seed = rng.next_u64();

        // Init network stack
        static STACK: StaticCell<Stack> = StaticCell::new();
        static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
        let stack = &*STACK.init(Stack::new(
            net_device,
            config,
            RESOURCES.init(StackResources::<5>::new()),
            seed,
        ));

        unwrap!(spawner.spawn(net_task(stack)));

        info!("Network stack initialised");

        Self {
            state: Disconnected { control },
            stack,
            seed,
        }
    }

    pub async fn connect(
        mut self,
        ssid: &str,
        password: Option<&str>,
        timeout: u64,
        static_config: Option<StaticConfigV4>,
    ) -> Result<Client<Connected>, (ConnectionError, Self)> {
        if let Some(static_config) = static_config {
            self.stack
                .set_config_v4(embassy_net::ConfigV4::Static(static_config));
        }

        if let Err(error) = {
            if let Some(password) = password {
                self.state.control.join_wpa2(ssid, password).await
            } else {
                self.state.control.join_open(ssid).await
            }
        } {
            info!("join failed with status={}", error.status);
            let error = match error.status {
                2 => ConnectionError::OtherTimeout,
                3 => ConnectionError::SsidNotFound,
                _ => ConnectionError::UnknownError(error.status),
            };
            return Err((error, self));
        }

        println!("waiting for DHCP...");
        let start = Instant::now();
        while !self.stack.is_config_up() {
            Timer::after_millis(100).await;
            let now = Instant::now();
            if (now - start).as_secs() > timeout {
                return Err((ConnectionError::DhcpTimeout, self));
            }
        }
        println!("DHCP is now up!");

        println!("waiting for link up...");
        let start = Instant::now();
        while !self.stack.is_link_up() {
            Timer::after_millis(500).await;
            let now = Instant::now();
            if (now - start).as_secs() > timeout {
                return Err((ConnectionError::OtherTimeout, self));
            }
        }
        println!("Link is up!");

        println!("waiting for stack to be up...");
        self.stack.wait_config_up().await;
        println!("Stack is up!");

        self.state.control.gpio_set(0, true).await;

        Ok(Client {
            state: Connected {
                config: self.stack.config_v4().unwrap(),
            },
            stack: self.stack,
            seed: self.seed,
        })
    }
}

impl Client<Connected> {
    pub const BLANK_REQUEST_BUFFER: String<RX_BUFFER_SIZE> = String::new();

    /// Send a http/s request and serialize the returning data.
    ///
    /// To ignore the response body use [`request`]
    pub async fn request_with_data<'a, T: Deserialize<'a>>(
        &self,
        url: &str,
        method: Method,
        headers: Option<&[(&str, &str)]>,
        body: Option<&'a str>,
        serialization_buffer: &'a mut String<RX_BUFFER_SIZE>,
    ) -> Result<(StatusCode, T), RequestError> {
        let result = self.inner_request(url, method, headers, body).await?;
        *serialization_buffer = result.1;
        Ok((result.0, serde_json_core::from_str(serialization_buffer)?.0))
    }

    /// Send a http/s request but ignore the returned response body.
    ///
    /// To include the response body, use [`request_with_data`]
    pub async fn request<'a>(
        &self,
        url: &str,
        method: Method,
        headers: Option<&[(&str, &str)]>,
        body: Option<&'a str>,
    ) -> Result<StatusCode, RequestError> {
        let response = self.inner_request(url, method, headers, body).await?;
        Ok(response.0)
    }

    async fn inner_request<'a>(
        &self,
        url: &str,
        method: Method,
        headers: Option<&[(&str, &str)]>,
        body: Option<&'a str>,
    ) -> Result<(StatusCode, String<RX_BUFFER_SIZE>), RequestError> {
        let mut rx_buffer = [0; RX_BUFFER_SIZE];
        let mut tls_read_buffer = [0; 16640];
        let mut tls_write_buffer = [0; 16640];

        let client_state = TcpClientState::<1, 1024, 1024>::new();
        let tcp_client = TcpClient::new(self.stack, &client_state);
        let dns_client = DnsSocket::new(self.stack);
        let tls_config = TlsConfig::new(
            self.seed,
            &mut tls_read_buffer,
            &mut tls_write_buffer,
            TlsVerify::None,
        );

        let mut http_client = if url.starts_with("https://") {
            HttpClient::new_with_tls(&tcp_client, &dns_client, tls_config)
        } else {
            HttpClient::new(&tcp_client, &dns_client)
        };

        // Create request with headers and body.
        let mut request = {
            let mut request = http_client.request(method, url).await?;
            if let Some(headers) = headers {
                request = request.headers(headers);
            }
            request.body(body.map(str::as_bytes))
        };

        info!("connecting to {}", &url);

        // Send request.
        let response = request.send(&mut rx_buffer).await?;
        let status = response.status;
        if !status.is_successful() {
            return Err(RequestError::HttpCode(status.0));
        }

        // Serialize data as string.
        let body = response.body().read_to_end().await?;
        let buffer = String::from_utf8(Vec::from_slice(body).unwrap())?;

        info!("Response body: {:?}", buffer);
        Ok((status, buffer))
    }

    pub async fn print_config(&self) {
        println!("~~~Config~~~");

        println!("Address: {}", self.state.config.address);

        if let Some(gateway) = self.state.config.gateway {
            println!("Gateway: {}", gateway);
        } else {
            println!("Gateway: N/A");
        }

        for index in 0..=2 {
            if let Some(address) = self.state.config.dns_servers.get(index) {
                println!("DNS {}: {address}", index + 1);
            } else {
                println!("DNS {}: N/A", index + 1);
            }
        }

        println!("~~~~~~~~~~~~");
    }
}
