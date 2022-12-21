#![feature(nonzero_min_max)]

use anyhow::bail;
use anyhow::Result;
use embedded_svc::wifi::Wifi;
use embedded_svc::{
    ipv4,
    wifi::{AccessPointConfiguration, ClientConfiguration},
};
use esp_idf_hal::gpio;
use esp_idf_hal::{peripheral, peripherals::Peripherals};
use esp_idf_svc::timer::EspTimerService;
use esp_idf_svc::wifi::WifiEvent;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    log::EspLogger,
    netif::{EspNetif, EspNetifWait},
    ping,
    sntp::SyncStatus,
    wifi::{EspWifi, WifiWait},
};
use esp_idf_sys::{self as _, esp}; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported
use log::info;
use std::ffi::CString;
use std::sync::mpsc;
use std::time::Duration;

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

    #[default("error")]
    log_level: &'static str,

    #[default("rust_led_strip")]
    mdns_hostname: &'static str,

    #[default("Led Strip Micro Controller with Rust firmware!")]
    mdns_instance_name: &'static str,
}

fn main() -> Result<()> {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    esp_idf_sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    EspLogger::initialize_default();
    EspLogger.set_target_level("*", T_CONFIG.log_level.parse_loglevel().unwrap());

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;

    let mut led1 = gpio::PinDriver::output(pins.gpio12)?;
    let mut led2 = gpio::PinDriver::output(pins.gpio13)?;

    led1.set_high()?;
    led2.set_high()?;

    let sysloop = EspSystemEventLoop::take()?;

    init_mdns()?;
    let mut wifi = wifi(peripherals.modem, sysloop.clone()).unwrap(); // Panic if wifi connection failed
    let _wifi_reconnect_subscription = sysloop.subscribe({
        let timer = EspTimerService::new()?.timer(move || wifi.connect().unwrap())?;
        move |event: &WifiEvent| match event {
            WifiEvent::StaBeaconTimeout => timer.every(Duration::from_secs(10)).unwrap(),
            WifiEvent::StaConnected => {
                timer.cancel().unwrap();
            }
            _ => (),
        }
    })?;

    let sntp = EspSntpWrapper::new_default()?;
    sntp.wait_status_with_timeout(Duration::from_secs(10), |status| {
        *status == SyncStatus::Completed
    })
    .unwrap();

    led1.set_low()?;
    led2.set_low()?;

    let (tx, rx) = mpsc::sync_channel(0);
    let (applied_config_tx, applied_config_rx) = mpsc::sync_channel(0);

    let _httpd = web_server(tx, applied_config_rx)?;

    let thr = std::thread::spawn(move || {
        LedStripAnimation::new(pins.gpio6, 0, Default::default())
            .unwrap()
            .led_strip_loop(rx, applied_config_tx)
            .unwrap()
    });

    thr.join().unwrap();
    Ok(())
}

fn ping(ip: ipv4::Ipv4Addr) -> Result<()> {
    info!("About to do some pings for {:?}", ip);

    let ping_summary = ping::EspPing::default().ping(ip, &Default::default())?;
    if ping_summary.transmitted != ping_summary.received {
        bail!("Pinging IP {} resulted in timeouts", ip);
    }

    info!("Pinging done");

    Ok(())
}

fn wifi(
    modem: impl peripheral::Peripheral<P = esp_idf_hal::modem::Modem> + 'static,
    sysloop: EspSystemEventLoop,
) -> Result<Box<EspWifi<'static>>> {
    use std::net::Ipv4Addr;

    let mut wifi = Box::new(EspWifi::new(modem, sysloop.clone(), None)?);

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

    wifi.set_configuration(&embedded_svc::wifi::Configuration::Mixed(
        ClientConfiguration {
            ssid: T_CONFIG.wifi_ssid.into(),
            password: T_CONFIG.wifi_pass.into(),
            channel,
            ..Default::default()
        },
        AccessPointConfiguration {
            ssid: "aptest".into(),
            channel: channel.unwrap_or(1),
            ..Default::default()
        },
    ))?;

    wifi.start()?;

    info!("Starting wifi...");

    if !WifiWait::new(&sysloop)?
        .wait_with_timeout(Duration::from_secs(20), || wifi.is_started().unwrap())
    {
        bail!("Wifi did not start");
    }

    info!("Connecting wifi...");

    wifi.connect()?;

    if !EspNetifWait::new::<EspNetif>(wifi.sta_netif(), &sysloop)?.wait_with_timeout(
        Duration::from_secs(20),
        || {
            wifi.is_connected().unwrap()
                && wifi.sta_netif().get_ip_info().unwrap().ip != Ipv4Addr::new(0, 0, 0, 0)
        },
    ) {
        bail!("Wifi did not connect or did not receive a DHCP lease");
    }

    let ip_info = wifi.sta_netif().get_ip_info()?;

    info!("Wifi DHCP info: {:?}", ip_info);

    ping(ip_info.subnet.gateway)?;

    Ok(wifi)
}

trait IntoLogLevel {
    fn parse_loglevel(&self) -> Result<log::LevelFilter>;
}

impl IntoLogLevel for str {
    fn parse_loglevel(&self) -> Result<log::LevelFilter> {
        Ok(match self {
            "off" => log::LevelFilter::Off,
            "error" => log::LevelFilter::Error,
            "warn" => log::LevelFilter::Warn,
            "info" => log::LevelFilter::Info,
            "trace" => log::LevelFilter::Trace,
            "debug" => log::LevelFilter::Debug,
            _ => bail!("Incorrect log level"),
        })
    }
}

fn init_mdns() -> anyhow::Result<()> {
    let cstr_mdns_hostname = CString::new(T_CONFIG.mdns_hostname)?;
    let cstr_mdns_instance_name = CString::new(T_CONFIG.mdns_instance_name)?;
    unsafe {
        esp!(esp_idf_sys::mdns_init())?;

        esp!(esp_idf_sys::mdns_hostname_set(cstr_mdns_hostname.as_ptr()))?;
        esp!(esp_idf_sys::mdns_instance_name_set(
            cstr_mdns_instance_name.as_ptr()
        ))?;

        esp!(esp_idf_sys::mdns_service_add(
            std::ptr::null(),
            CString::new("_http")?.as_ptr(),
            CString::new("_tcp")?.as_ptr(),
            80,
            std::ptr::null_mut() as *mut esp_idf_sys::mdns_txt_item_t,
            0
        ))?;
    }
    Ok(())
}
