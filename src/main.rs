use std::{sync::Arc, time::Duration};

use anyhow::Ok;
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
use esp_idf_hal::{gpio::Pin, peripherals::Peripherals};
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
use log::{debug, info};
use smart_leds_trait::{SmartLedsWrite, White, RGBA, RGBW};
use ws2812_esp32_rmt_driver::{
    driver::color::{LedPixelColorGrbw32, LedPixelColorImpl},
    LedPixelEsp32Rmt, RGBW8,
};
mod sub_modules;
use sub_modules::esp_sntp_wrapper::EspSntpWrapper;

#[toml_cfg::toml_config]
struct TConfig {
    #[default("some_ssid")]
    wifi_ssid: &'static str,

    #[default("some_pass")]
    wifi_pass: &'static str,
}

const LED_QUANTITY: usize = 150;
const TARGET_FPS: u64 = 60;
const GRADIENT_DISCRETENESS: usize = 200;

fn main() -> anyhow::Result<()> {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    esp_idf_sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    EspLogger::initialize_default();
    EspLogger.set_target_level("*", log::LevelFilter::Error);

    let perephirals = Peripherals::take().unwrap();
    let pins = perephirals.pins;

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

    let mut ws2812 =
        LedPixelEsp32Rmt::<RGBW8, LedPixelColorGrbw32>::new(0, pins.gpio6.pin().try_into()?)
            .unwrap();

    let mut frames_counter = 0;
    let mut now = std::time::Instant::now();

    loop {
        for i in 0..GRADIENT_DISCRETENESS {
            let color = colorous::SINEBOW.eval_rational(i, GRADIENT_DISCRETENESS);

            let write_start = std::time::Instant::now();
            let target_delay = Duration::from_millis(1000 / TARGET_FPS);

            ws2812.set_color(RGBW::from((color.r, color.g, color.b, White(0))))?;

            // Smart delay
            if write_start.elapsed() < target_delay {
                std::thread::sleep(target_delay - write_start.elapsed());
            }

            // Frames counter
            frames_counter += 1;
            if now.elapsed() >= Duration::from_secs(1) {
                println!("Frames counted: {}", frames_counter);
                frames_counter = 0;
                now = std::time::Instant::now();
            }
        }
    }
    // loop {
    //     ws2812.set_color(RGBW::from((5, 0, 0, White(0))))?;
    //     std::thread::sleep(Duration::from_secs(1));
    //
    //     ws2812.set_color(RGBW::from((0, 5, 0, White(0))))?;
    //     std::thread::sleep(Duration::from_secs(1));
    //
    //     ws2812.set_color(RGBW::from((0, 0, 5, White(0))))?;
    //     std::thread::sleep(Duration::from_secs(1));
    //
    //     ws2812.set_color(RGBW::from((0, 0, 0, White(5))))?;
    //     std::thread::sleep(Duration::from_secs(1));
    // }

    // Ok(())
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

trait SetColor {
    fn set_color(&mut self, color: RGBA<u8, White<u8>>) -> anyhow::Result<()>;
}

impl SetColor for LedPixelEsp32Rmt<RGBA<u8, White<u8>>, LedPixelColorImpl<4, 1, 0, 2, 3>> {
    fn set_color(&mut self, color: RGBA<u8, White<u8>>) -> anyhow::Result<()> {
        let pixels = std::iter::repeat(color).take(LED_QUANTITY);
        self.write(pixels)?;
        Ok(())
    }
}
