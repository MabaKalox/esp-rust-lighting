#![feature(nonzero_min_max)]

use animation_lang::program::Program;
use embedded_hal::digital::v2::OutputPin;
use embedded_svc::{
    http::{
        client::{
            Client as HttpClient, Request as HttpRequest, RequestWrite as HttpRequestWrite,
            Response as HttpResponse,
        },
        Status as HttpStatus,
    },
    io::Read,
    ipv4,
    ping::Ping,
    wifi::{self, Wifi},
};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::{
    http::client::{EspHttpClient, EspHttpClientConfiguration},
    log::EspLogger,
    netif::EspNetifStack,
    nvs::EspDefaultNvs,
    ping,
    sntp::SyncStatus,
    sysloop::EspSysLoopStack,
    wifi::EspWifi,
};
use esp_idf_sys::{self as _}; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported
use log::info;
use smart_leds_trait::RGBA;
use std::{
    io::{self, Read as StdRead},
    net::TcpListener,
    sync::mpsc,
};
use std::{sync::Arc, time::Duration};

mod sub_modules;
use crate::sub_modules::esp_sntp_wrapper::EspSntpWrapper;
use sub_modules::led_strip_animations::LedStripAnimation;
use sub_modules::web_server::web_server;

#[toml_cfg::toml_config]
struct TConfig {
    #[default("some_ssid")]
    wifi_ssid: &'static str,

    #[default("some_pass")]
    wifi_pass: &'static str,
}

fn main() -> anyhow::Result<()> {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    esp_idf_sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    EspLogger::initialize_default();
    EspLogger.set_target_level("*", log::LevelFilter::Error);

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;

    let mut led1 = pins.gpio12.into_output()?;
    let mut led2 = pins.gpio13.into_output()?;

    led1.set_high()?;
    led2.set_high()?;

    let netif_stack = Arc::new(EspNetifStack::new()?);
    let sys_loop_stack = Arc::new(EspSysLoopStack::new()?);
    let default_nvs = Arc::new(EspDefaultNvs::new()?);

    let _wifi_interface = wifi(netif_stack, sys_loop_stack, default_nvs)?;

    let sntp = EspSntpWrapper::new_default()?;
    sntp.wait_status_with_timeout(Duration::from_secs(10), |status| {
        *status == SyncStatus::Completed
    })?;
    println!(
        "Current time is: {:?}",
        chrono::DateTime::from(std::time::SystemTime::now())
    );

    // get("https://google.com")?;

    led1.set_low()?;
    led2.set_low()?;

    let (tx, rx) = mpsc::sync_channel(0);
    let (applied_config_tx, applied_config_rx) = mpsc::sync_channel(0);

    let _httpd = web_server(tx.clone(), applied_config_rx)?;

    let led_strip_thead = std::thread::Builder::new()
        .stack_size(4 * 1024)
        .spawn(move || {
            let mut ws2812 = LedStripAnimation::new(pins.gpio6, 0, Default::default()).unwrap();

            ws2812.led_strip_loop(rx, applied_config_tx)
        })?;

    let mut listener = TcpListener::bind("0.0.0.0:8888").unwrap();
    listener.set_nonblocking(true).unwrap();

    loop {
        if let Some(new_prog) = check_tcp(&mut listener) {
            tx.send(sub_modules::led_strip_animations::Messages::NewProg(
                Program::from_binary(new_prog),
            ))?;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    println!("Starting to wait for thread");
    led_strip_thead.join().unwrap()?;
    Ok(())
}

fn ping(ip_settings: &ipv4::ClientSettings) -> anyhow::Result<()> {
    info!("About to do some pings for {:?}", ip_settings);

    let ping_summary =
        ping::EspPing::default().ping(ip_settings.subnet.gateway, &Default::default())?;
    if ping_summary.transmitted != ping_summary.received {
        anyhow::bail!(
            "Pinging gateway {} resulted in timeouts",
            ip_settings.subnet.gateway
        );
    }

    info!("Pinging done");

    Ok(())
}

fn wifi(
    netif_stack: Arc<EspNetifStack>,
    sys_loop_stack: Arc<EspSysLoopStack>,
    default_nvs: Arc<EspDefaultNvs>,
) -> anyhow::Result<Box<EspWifi>> {
    let mut wifi = Box::new(EspWifi::new(netif_stack, sys_loop_stack, default_nvs)?);

    info!("Wifi created, about to scan");

    let ap_infos = wifi.scan()?;

    let ours = ap_infos.into_iter().find(|a| a.ssid == T_CONFIG.wifi_ssid);

    let channel = if let Some(ours) = ours {
        info!(
            "Found configured access point {} on channel {}",
            T_CONFIG.wifi_ssid, ours.channel
        );
        Some(ours.channel)
    } else {
        info!(
            "Configured access point {} not found during scanning, will go with unknown channel",
            T_CONFIG.wifi_ssid
        );
        None
    };

    wifi.set_configuration(&wifi::Configuration::Mixed(
        wifi::ClientConfiguration {
            ssid: T_CONFIG.wifi_ssid.into(),
            password: T_CONFIG.wifi_pass.into(),
            channel,
            ..Default::default()
        },
        wifi::AccessPointConfiguration {
            ssid: "aptest".into(),
            channel: channel.unwrap_or(1),
            ..Default::default()
        },
    ))?;

    info!("Wifi configuration set, about to get status");

    wifi.wait_status_with_timeout(Duration::from_secs(20), |status| !status.is_transitional())
        .map_err(|e| anyhow::anyhow!("Unexpected Wifi status: {:?}", e))?;

    let status = wifi.get_status();

    if let wifi::Status(
        wifi::ClientStatus::Started(wifi::ClientConnectionStatus::Connected(
            wifi::ClientIpStatus::Done(ip_settings),
        )),
        wifi::ApStatus::Started(wifi::ApIpStatus::Done),
    ) = status
    {
        info!("Wifi connected");

        ping(&ip_settings)?;
    } else {
        anyhow::bail!("Unexpected Wifi status: {:?}", status);
    }

    Ok(wifi)
}

fn get(url: &str) -> anyhow::Result<()> {
    // 1. create a new EspHttpClient with SSL certificates enabled
    let mut client = EspHttpClient::new(&EspHttpClientConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_sys::esp_crt_bundle_attach),

        ..Default::default()
    })?;

    // 2. open a GET request to `url`
    let request = client.get(url.as_ref())?;

    // 3. requests *may* send data to the server. Turn the request into a writer, specifying 0 bytes as write length
    // (since we don't send anything - but have to do the writer step anyway)
    //
    // https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-reference/protocols/esp_http_client.html
    // if this were a POST request, you'd set a write length > 0 and then writer.do_write(&some_buf);
    let writer = request.into_writer(0)?;
    // 4. submit our write request and check the status code of the response.
    // Successful http status codes are in the 200..=299 range.
    let mut response = writer.submit()?;
    let status = response.status();
    println!("response code: {}\n", status);
    match status {
        200..=299 => {
            // 5. if the status is OK, read response data chunk by chunk into a buffer and print it until done
            let mut buf = [0u8; 256];
            let mut total_size = 0;
            let mut reader = response.reader();
            loop {
                let size = reader.read(&mut buf)?;
                if size == 0 {
                    break;
                }
                total_size += size;
                // strictly speaking, we should check the response's encoding...

                // 6. try converting the bytes into a Rust (UTF-8) string and print it
                let response_text = std::str::from_utf8(&buf)?;
                print!("{}", response_text);
            }

            println!("\n\nDone! read {} bytes:", total_size);
        }
        _ => anyhow::bail!("unexpected response code: {}", status),
    }

    Ok(())
}

fn check_tcp(listener: &mut TcpListener) -> Option<Vec<u8>> {
    match listener.accept() {
        Ok(mut stream) => {
            println!("Receiving new program");
            let mut new_prog = Vec::new();
            stream.0.read_to_end(&mut new_prog).unwrap();
            Some(new_prog)
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => None,
        Err(e) => Err(e).unwrap(),
    }
}
