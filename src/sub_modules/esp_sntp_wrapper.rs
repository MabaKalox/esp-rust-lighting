use anyhow::{bail, Result};
use esp_idf_hal::mutex::{Condvar, Mutex};
use esp_idf_svc::sntp::{EspSntp, SntpConf, SyncStatus};
use log::{debug, info};
use std::sync::Arc;
use std::time::Duration;

static SYNC_CALLBACK: Mutex<Option<Box<dyn FnMut() + Send>>> = Mutex::new(None);

pub struct EspSntpWrapper {
    pub esp_sntp: EspSntp,
    condvar_pair: Arc<(Mutex<SyncStatus>, Condvar)>,
}

impl EspSntpWrapper {
    pub fn new(sntp_conf: SntpConf) -> Result<Self> {
        let esp_sntp = EspSntp::new(&sntp_conf)?;
        let condvar_pair = Arc::new((Mutex::new(SyncStatus::Reset), Condvar::new()));

        let callback_pair = condvar_pair.clone();
        *SYNC_CALLBACK.lock() = Some(Box::new(move || {
            let sync_status = SyncStatus::from(unsafe { esp_idf_sys::sntp_get_sync_status() });
            debug!("callback called, status: {:?}", sync_status);
            *callback_pair.0.lock() = sync_status;
            callback_pair.1.notify_one();
        }));

        unsafe {
            extern "C" fn sync_cb_wrapper(_: *mut esp_idf_sys::timeval) {
                SYNC_CALLBACK.lock().as_mut().unwrap()();
            }
            esp_idf_sys::sntp_set_time_sync_notification_cb(Some(sync_cb_wrapper));
        }

        Ok(Self {
            esp_sntp,
            condvar_pair,
        })
    }

    pub fn new_default() -> Result<Self> {
        Self::new(Default::default())
    }

    pub fn wait_status_with_timeout(
        &self,
        dur: Duration,
        matcher: impl Fn(&SyncStatus) -> bool,
    ) -> Result<()> {
        info!("About to wait {:?} for status", dur);

        let mut status = self.condvar_pair.0.lock();
        let mut dur_left = dur;

        loop {
            let now = std::time::Instant::now();

            debug!("status is: {:?}", *status);
            if matcher(&status) {
                *status = SyncStatus::Reset;
                return Ok(());
            }

            let (new_state, timeout) = self.condvar_pair.1.wait_timeout(status, dur_left);

            status = new_state;

            if timeout {
                bail!("Timeout, status: {:?}", *status);
            } else if let Some(new_dur) = dur_left.checked_sub(now.elapsed()) {
                dur_left = new_dur
            }
        }
    }
}
