use crate::sub_modules::led_strip_animations::{AnimationConfig, Messages};
use crate::sub_modules::wifi_manager::wifi_creds::WifiCredentials;
use crate::sub_modules::wifi_manager::{TryConnectArgs, WifiManagerCommunication};
use crate::T_CONFIG;
use animation_lang::program::Program;
use embedded_svc::http::Method;
use embedded_svc::io::adapters::ToStd;
use embedded_svc::io::Write;
use esp_idf_svc::http::server::EspHttpServer;
use std::io::Read;
use std::sync::mpsc::{Receiver, SyncSender};

use super::led_strip_animations::ReceivedAnimationConfig;

static WASM_BLOB: &[u8] = include_bytes!(env!("WASM_BLOB_PATH"));
static JS_BLOB: &[u8] = include_bytes!(env!("JS_BLOB_PATH"));
static HTML_BLOB: &[u8] = include_bytes!("../../frontend/index.html");

pub fn web_server(
    tx: SyncSender<Messages>,
    applied_config_rx: Receiver<AnimationConfig>,
    wifi_manager_communication: WifiManagerCommunication,
) -> anyhow::Result<EspHttpServer> {
    let mut server = EspHttpServer::new(&Default::default())?;

    // Frontend
    server.fn_handler("/", Method::Get, |req| {
        req.into_ok_response()?.write_all(HTML_BLOB)?;

        Ok(())
    })?;

    server.fn_handler("/get_wasm_blob", Method::Get, |req| {
        req.into_response(200, None, &[("Content-Type", "application/wasm")])?
            .write_all(WASM_BLOB)?;

        Ok(())
    })?;

    server.fn_handler("/get_js_blob", Method::Get, |req| {
        req.into_response(200, None, &[("Content-Type", "text/javascript")])?
            .write_all(JS_BLOB)?;

        Ok(())
    })?;

    server.fn_handler("/get_max_led_quantity", Method::Get, |req| {
        req.into_response(200, None, &[("Content-Type", "text/plain")])?
            .write_all(T_CONFIG.led_quantity.to_string().as_bytes())?;

        Ok(())
    })?;

    // Led related
    server.fn_handler("/set_conf", Method::Post, {
        let tx = tx.clone();
        move |mut req| {
            let new_config: ReceivedAnimationConfig =
                match serde_json::from_reader(ToStd::new(&mut req)) {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        let message = e.to_string();
                        req.into_response(400, Some(&message), &[])?
                            .write_all(message.as_bytes())?;
                        return Ok(());
                    }
                };

            tx.send(Messages::NewConfig(new_config))?;
            req.into_ok_response()?
                .write_all(format!("Applied config: {:?}", applied_config_rx.recv()?).as_bytes())?;
            Ok(())
        }
    })?;

    server.fn_handler("/send_prog_base64", Method::Post, {
        move |mut req| {
            let mut body = Vec::new();
            ToStd::new(&mut req).read_to_end(&mut body)?;
            let bin_prog = match base64::decode(body) {
                Ok(bin_prog) => bin_prog,
                Err(e) => {
                    let message = e.to_string();
                    req.into_response(400, Some(&message), &[])?
                        .write_all(message.as_bytes())?;
                    return Ok(());
                }
            };

            tx.send(Messages::NewProg(Program::from_binary(bin_prog)))?;

            req.into_response(200, None, &[])?;
            Ok(())
        }
    })?;

    // Wifi Related
    server.fn_handler("/wifi/store_credentials", Method::Post, move |req| {
        match wifi_manager_communication.store_credentials_api.store()? {
            Ok(_) => {}
            Err(e) => {
                let message = e.to_string();
                req.into_response(400, Some(&message), &[])?
                    .write_all(message.as_bytes())?;
                return Ok(());
            }
        };

        Ok(())
    })?;

    server.fn_handler("/wifi/erase_credentials", Method::Post, move |_| {
        WifiCredentials::erase()?;

        Ok(())
    })?;

    server.fn_handler("/wifi/scan", Method::Get, move |req| {
        let access_points = wifi_manager_communication.scan_api.scan()?;
        let serialized_access_points = serde_json::to_vec(&access_points)?;

        req.into_ok_response()?
            .write_all(&serialized_access_points)?;

        Ok(())
    })?;

    server.fn_handler("/wifi/connect", Method::Post, move |mut req| {
        let body_reader = ToStd::new(&mut req);
        let connect_args: TryConnectArgs = match serde_json::from_reader(body_reader) {
            Ok(cfg) => cfg,
            Err(e) => {
                let message = e.to_string();
                req.into_response(400, Some(&message), &[])?
                    .write_all(message.as_bytes())?;
                return Ok(());
            }
        };

        wifi_manager_communication
            .connect_api
            .try_connect(connect_args)?;

        Ok(())
    })?;

    server.fn_handler("/wifi/disconnect", Method::Post, move |req| {
        match wifi_manager_communication.disconnect_api.disconnect()? {
            Ok(_) => {}
            Err(e) => {
                let message = e.to_string();
                req.into_response(400, Some(&message), &[])?
                    .write_all(message.as_bytes())?;
                return Ok(());
            }
        };
        Ok(())
    })?;

    server.fn_handler("/wifi/status", Method::Get, move |req| {
        let status = wifi_manager_communication.status_api.get_status()?;

        serde_json::to_writer(ToStd::new(req.into_ok_response()?), &status)?;

        Ok(())
    })?;

    Ok(server)
}
