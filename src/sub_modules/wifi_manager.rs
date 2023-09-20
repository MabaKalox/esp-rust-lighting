use crate::T_CONFIG;
use anyhow::Result;
use embedded_svc::wifi::{AccessPointConfiguration, AccessPointInfo, Configuration};
use esp_idf_hal::peripheral;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use log::info;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;
use thiserror::Error;
use wifi_creds::WifiCredentials;
use wifi_states::{Scan, WifiBase, WifiState};

// pub mod net_utils {
//     use anyhow::{bail, Result};
//     use esp_idf_svc::ping;
//     use log::info;
//     use std::net::Ipv4Addr;
//
//     pub fn ping(ip: Ipv4Addr) -> Result<()> {
//         info!("About to do some pings for {:?}", ip);
//
//         let ping_summary = ping::EspPing::default().ping(ip, &Default::default())?;
//         if ping_summary.transmitted != ping_summary.received {
//             bail!("Pinging IP {} resulted in timeouts", ip);
//         }
//
//         info!("Pinging done");
//
//         Ok(())
//     }
// }

pub mod wifi_creds {
    use embedded_svc::storage::RawStorage;
    use esp_idf_svc::nvs::{EspNvs, EspNvsPartition, NvsDefault};
    use esp_idf_sys::EspError;
    use serde::Deserialize;

    pub const MAX_SSID: usize = 32;
    pub const MAX_PASS: usize = 64;
    const WIFI_CREDENTIALS_NAMESPACE: &str = "wifi_creds";
    const WIFI_SSID_NAMESPACE: &str = "wifi_ssid";
    const WIFI_PASS_NAMESPACE: &str = "wifi_pass";

    #[derive(Default, Debug, PartialEq, Deserialize)]
    #[serde(default)]
    #[serde(deny_unknown_fields)]
    pub struct WifiCredentials {
        pub ssid: heapless::String<MAX_SSID>,
        pub pass: heapless::String<MAX_PASS>,
        pub channel: Option<u8>,
    }

    impl WifiCredentials {
        pub fn load() -> anyhow::Result<Option<WifiCredentials>> {
            let nvs = Self::open_nvs()?;

            let mut ssid_buf = [0; MAX_SSID];
            let mut pass_buf = [0; MAX_PASS];

            if let (Some(ssid), Some(pass)) = (
                nvs.get_raw(WIFI_SSID_NAMESPACE, &mut ssid_buf)?,
                nvs.get_raw(WIFI_PASS_NAMESPACE, &mut pass_buf)?,
            ) {
                Ok(Some(WifiCredentials {
                    ssid: std::str::from_utf8(ssid)?.into(),
                    pass: std::str::from_utf8(pass)?.into(),
                    channel: None, // We dont store channel in nvs
                }))
            } else {
                Ok(None)
            }
        }

        pub fn store(self) -> Result<(), EspError> {
            let mut nvs = Self::open_nvs()?;

            nvs.set_raw(WIFI_SSID_NAMESPACE, self.ssid.as_bytes())?;
            nvs.set_raw(WIFI_PASS_NAMESPACE, self.pass.as_bytes())?;

            Ok(())
        }

        fn open_nvs() -> Result<EspNvs<NvsDefault>, EspError> {
            EspNvs::new(
                EspNvsPartition::<NvsDefault>::take()?,
                WIFI_CREDENTIALS_NAMESPACE,
                true,
            )
        }

        pub fn erase() -> Result<(), EspError> {
            let mut nvs = Self::open_nvs()?;

            nvs.remove(WIFI_SSID_NAMESPACE)?;
            nvs.remove(WIFI_PASS_NAMESPACE)?;

            Ok(())
        }
    }
}

pub mod wifi_states {
    // use super::net_utils::ping;
    use super::wifi_creds::WifiCredentials;
    use anyhow::Result;
    use embedded_svc::wifi::{AccessPointInfo, Configuration, Wifi};
    use enum_dispatch::enum_dispatch;
    use esp_idf_hal::peripheral;
    use esp_idf_svc::eventloop::EspSystemEventLoop;
    use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
    use log::info;

    pub struct WifiBase {
        wifi: BlockingWifi<EspWifi<'static>>,
    }
    // Wifi Manager states
    pub struct WifiMixed(WifiBase);
    pub struct WifiMixedStarted(WifiMixed);
    pub struct WifiMixedConnected(WifiMixedStarted);

    #[enum_dispatch(Scan)]
    pub enum WifiState {
        Started(WifiMixedStarted),
        Connected(WifiMixedConnected),
    }

    impl WifiBase {
        pub fn new(
            modem: impl peripheral::Peripheral<P = esp_idf_hal::modem::Modem> + 'static,
            sysloop: EspSystemEventLoop,
        ) -> Result<Self> {
            let wifi = BlockingWifi::wrap(EspWifi::new(modem, sysloop.clone(), None)?, sysloop)?;

            Ok(Self { wifi })
        }

        pub fn configure(mut self, config: &Configuration) -> Result<WifiMixed> {
            self.wifi.set_configuration(config)?;

            Ok(WifiMixed(self))
        }
    }

    #[enum_dispatch]
    pub trait Scan {
        fn scan(&mut self) -> Result<Vec<AccessPointInfo>>;
    }

    impl Scan for WifiMixedStarted {
        fn scan(&mut self) -> Result<Vec<AccessPointInfo>> {
            Ok(self.0 .0.wifi.scan()?)
        }
    }

    impl Scan for WifiMixedConnected {
        fn scan(&mut self) -> Result<Vec<AccessPointInfo>> {
            Ok(self.0 .0 .0.wifi.scan()?)
        }
    }

    impl WifiMixed {
        pub fn start(mut self) -> Result<WifiMixedStarted> {
            self.0.wifi.start()?;

            Ok(WifiMixedStarted(self))
        }

        pub fn connect(mut self, creds: WifiCredentials) -> Result<WifiState> {
            let mut config = self.0.wifi.get_configuration()?;

            match &mut config {
                Configuration::Mixed(sta, _) => {
                    sta.ssid = creds.ssid;
                    sta.password = creds.pass;
                    sta.channel = creds.channel;
                }
                _ => unreachable!(),
            }

            self.0.wifi.set_configuration(&config)?;
            let mut started_wifi = self.start()?;

            started_wifi.0 .0.wifi.connect()?;

            match started_wifi.0 .0.wifi.wait_netif_up() {
                Ok(_) => {
                    let ip_info = started_wifi.0 .0.wifi.wifi().sta_netif().get_ip_info()?;

                    info!("Wifi DHCP info: {:?}", ip_info);

                    Ok(WifiState::Connected(WifiMixedConnected(started_wifi)))
                }
                Err(e) => {
                    info!("Connection failed: {:?}", e);
                    Ok(WifiState::Started(started_wifi))
                }
            }
        }
    }

    impl WifiMixedStarted {
        pub fn stop(mut self) -> Result<WifiMixed> {
            self.0 .0.wifi.stop()?;

            Ok(self.0)
        }
    }

    impl WifiMixedConnected {
        pub fn disconnect(mut self) -> Result<WifiMixedStarted> {
            self.0 .0 .0.wifi.disconnect()?;

            Ok(self.0)
        }

        pub fn get_creds(&self) -> Result<WifiCredentials> {
            let cfg = match self.0 .0 .0.wifi.get_configuration()? {
                Configuration::Mixed(client_cfg, _) => client_cfg,
                _ => unreachable!(),
            };

            Ok(WifiCredentials {
                ssid: cfg.ssid,
                pass: cfg.password,
                channel: cfg.channel,
            })
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum WifiStatus {
    Connected {
        ssid: heapless::String<{ wifi_creds::MAX_SSID }>,
    },
    Started,
}

pub enum WifiManagerCmd {
    Scan,
    TryConnect(TryConnectArgs),
    Disconnect,
    GetStatus,
    SaveCredentialsNvs,
}

#[derive(Error, Debug)]
pub enum APIError {
    #[error("Wifi is not connected")]
    NotConnected,
}

pub struct ScanAPI(SyncSender<WifiManagerCmd>, Receiver<Vec<AccessPointInfo>>);

impl ScanAPI {
    pub fn scan(&self) -> Result<Vec<AccessPointInfo>> {
        self.0.send(WifiManagerCmd::Scan)?;
        Ok(self.1.recv()?)
    }
}

pub struct StatusAPI(SyncSender<WifiManagerCmd>, Receiver<WifiStatus>);

impl StatusAPI {
    pub fn get_status(&self) -> Result<WifiStatus> {
        self.0.send(WifiManagerCmd::GetStatus)?;
        Ok(self.1.recv()?)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TryConnectArgs {
    creds: WifiCredentials,
    store_on_connect: bool,
}

pub struct TryConnectAPI(SyncSender<WifiManagerCmd>);

impl TryConnectAPI {
    pub fn try_connect(&self, cfg: TryConnectArgs) -> Result<()> {
        self.0.send(WifiManagerCmd::TryConnect(cfg))?;
        Ok(())
    }
}

pub struct DisconnectAPI(
    SyncSender<WifiManagerCmd>,
    Receiver<std::result::Result<(), APIError>>,
);

impl DisconnectAPI {
    pub fn disconnect(&self) -> Result<std::result::Result<(), APIError>> {
        self.0.send(WifiManagerCmd::Disconnect)?;
        Ok(self.1.recv()?)
    }
}

pub struct StoreCredentials(
    SyncSender<WifiManagerCmd>,
    Receiver<std::result::Result<(), APIError>>,
);

impl StoreCredentials {
    pub fn store(&self) -> Result<std::result::Result<(), APIError>> {
        self.0.send(WifiManagerCmd::SaveCredentialsNvs)?;
        Ok(self.1.recv()?)
    }
}

pub struct WifiManagerCommunication {
    pub scan_api: ScanAPI,
    pub status_api: StatusAPI,
    pub connect_api: TryConnectAPI,
    pub disconnect_api: DisconnectAPI,
    pub store_credentials_api: StoreCredentials,
}

pub struct WifiManager {
    pub state: WifiState,
}

impl WifiManager {
    pub fn new(
        modem: impl peripheral::Peripheral<P = esp_idf_hal::modem::Modem> + 'static,
        sysloop: EspSystemEventLoop,
    ) -> Result<Self> {
        let wifi_creds = WifiCredentials::load()?;
        let wifi_initial_cfg = Configuration::Mixed(
            Default::default(),
            AccessPointConfiguration {
                ssid: T_CONFIG.wifi_ap_ssid.into(),
                password: T_CONFIG.wifi_ap_pass.into(),
                auth_method: Default::default(),
                ..Default::default()
            },
        );

        let wifi = WifiBase::new(modem, sysloop)?.configure(&wifi_initial_cfg)?;

        let wifi_state = match wifi_creds {
            Some(creds) => wifi.connect(creds)?,
            None => WifiState::Started(wifi.start()?),
        };

        Ok(Self { state: wifi_state })
    }

    pub fn daemon(self, stack_size: usize) -> Result<(JoinHandle<()>, WifiManagerCommunication)> {
        let (cmd_tx, cmd_rx) = sync_channel(1);
        let (scan_tx, scan_rx) = sync_channel(0);
        let (status_tx, status_rx) = sync_channel(0);
        let (disconnect_res_tx, disconnect_res_rx) = sync_channel(0);
        let (store_credentials_res_tx, store_credentials_res_rx) = sync_channel(0);

        let thread_handle =
            std::thread::Builder::new()
                .stack_size(stack_size)
                .spawn(move || {
                    let mut manager = self;
                    loop {
                        match cmd_rx.recv().unwrap() {
                            WifiManagerCmd::Scan => {
                                scan_tx.send(manager.state.scan().unwrap()).unwrap()
                            }
                            WifiManagerCmd::TryConnect(connect_args) => {
                                std::thread::sleep(Duration::from_millis(1000));
                                manager.state = match manager.state {
                                    WifiState::Started(m) => {
                                        m.stop().unwrap().connect(connect_args.creds).unwrap()
                                    }
                                    WifiState::Connected(m) => m
                                        .disconnect()
                                        .unwrap()
                                        .stop()
                                        .unwrap()
                                        .connect(connect_args.creds)
                                        .unwrap(),
                                };
                                // Check if we connected and store credentials if requested
                                match &manager.state {
                                    WifiState::Connected(m) if connect_args.store_on_connect => {
                                        m.get_creds().unwrap().store().unwrap();
                                    }
                                    _ => {}
                                }
                            }
                            WifiManagerCmd::Disconnect => match manager.state {
                                WifiState::Connected(m) => {
                                    disconnect_res_tx.send(Ok(())).unwrap();
                                    std::thread::sleep(Duration::from_millis(1000));
                                    manager.state = WifiState::Started(
                                        m.disconnect().unwrap().stop().unwrap().start().unwrap(),
                                    );
                                }
                                WifiState::Started(_) => {
                                    disconnect_res_tx.send(Err(APIError::NotConnected)).unwrap();
                                }
                            },
                            WifiManagerCmd::GetStatus => {
                                let status = match &manager.state {
                                    WifiState::Started(_) => WifiStatus::Started,
                                    WifiState::Connected(m) => WifiStatus::Connected {
                                        ssid: m.get_creds().unwrap().ssid,
                                    },
                                };
                                status_tx.send(status).unwrap()
                            }
                            WifiManagerCmd::SaveCredentialsNvs => match &manager.state {
                                WifiState::Started(_) => store_credentials_res_tx
                                    .send(Err(APIError::NotConnected))
                                    .unwrap(),
                                WifiState::Connected(m) => {
                                    m.get_creds().unwrap().store().unwrap();
                                    store_credentials_res_tx.send(Ok(())).unwrap();
                                }
                            },
                        }
                        info!("daemon stack high water mark: {}", unsafe {
                            esp_idf_sys::uxTaskGetStackHighWaterMark(std::ptr::null_mut())
                        });
                    }
                })?;

        let communication = WifiManagerCommunication {
            scan_api: ScanAPI(cmd_tx.clone(), scan_rx),
            status_api: StatusAPI(cmd_tx.clone(), status_rx),
            connect_api: TryConnectAPI(cmd_tx.clone()),
            disconnect_api: DisconnectAPI(cmd_tx.clone(), disconnect_res_rx),
            store_credentials_api: StoreCredentials(cmd_tx, store_credentials_res_rx),
        };

        Ok((thread_handle, communication))
    }
}
