use anyhow::bail;
use anyhow::Result;
use esp_idf_hal::gpio;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::sntp::SyncStatus;
use esp_idf_svc::wifi::WifiEvent;
use esp_idf_svc::{eventloop::EspSystemEventLoop, log::EspLogger};
use esp_idf_sys::{self as _, esp}; // Dont remove it, required for binstart
use std::ffi::CString;
use std::sync::mpsc;
use std::time::Duration;

mod sub_modules;

use crate::sub_modules::esp_sntp_wrapper::EspSntpWrapper;
use crate::sub_modules::led_strip_animations::AnimationConfig;
use crate::sub_modules::wifi_manager::wifi_states::WifiState;
use sub_modules::led_strip_animations::LedStripAnimation;
use sub_modules::web_server::web_server;
use sub_modules::wifi_manager::WifiManager;

#[toml_cfg::toml_config]
struct TConfig {
    #[default("some_ap_ssid")]
    wifi_ap_ssid: &'static str,

    #[default("some_ap_pass")]
    wifi_ap_pass: &'static str,

    #[default(150)]
    led_quantity: usize,

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

    // Subscribe for wifi connection/disconnection to display status on onboard led
    let _wifi_status_subscription = sysloop.subscribe({
        move |event: &WifiEvent| match event {
            WifiEvent::StaConnected => led1.set_low().unwrap(),
            WifiEvent::StaDisconnected => led1.set_high().unwrap(),
            _ => {}
        }
    })?;
    // Create wifi manager instance, it will start AP and if credentials stored - connect to STA
    let wifi_manager = WifiManager::new(peripherals.modem, sysloop)?;

    // Start up sntp to sync time
    let sntp = EspSntpWrapper::new_default()?;
    // Wait for sntp to sync, if we have internet connection
    if matches!(wifi_manager.state, WifiState::Connected(_)) {
        sntp.wait_status_with_timeout(Duration::from_secs(20), |status| {
            matches!(status, SyncStatus::Completed)
        })?;
    }

    // Daemonize wifi manager, so it run in background
    let (wifi_manager_thread, wifi_manager_api) = wifi_manager.daemon(5 * 1024)?;

    let (tx, rx) = mpsc::sync_channel(0);
    let (applied_config_tx, applied_config_rx) = mpsc::sync_channel(0);

    let _httpd = web_server(tx, applied_config_rx, wifi_manager_api)?;

    led2.set_low()?;

    let thr = std::thread::spawn(move || {
        LedStripAnimation::new(
            pins.gpio6,
            0,
            AnimationConfig {
                led_quantity: T_CONFIG.led_quantity,
                ..Default::default()
            },
        )
        .unwrap()
        .led_strip_loop(rx, applied_config_tx)
        .unwrap()
    });

    wifi_manager_thread.join().unwrap();
    thr.join().unwrap();

    Ok(())
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

fn init_mdns() -> Result<()> {
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
            std::ptr::null_mut(),
            0
        ))?;
    }
    Ok(())
}
