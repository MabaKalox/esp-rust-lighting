use crate::sub_modules::led_strip_animations::{AnimationConfig, Messages};
use animation_lang::program::Program;
use embedded_svc::http::server::Query;
use embedded_svc::http::Method;
use embedded_svc::io::adapters::ToStd;
use embedded_svc::io::Write;
use esp_idf_svc::http::server::EspHttpServer;
use std::io::Read;
use std::sync::mpsc::{Receiver, SyncSender};

use super::led_strip_animations::ReceivedAnimationConfig;

static WASM_BLOB: &[u8] = include_bytes!("../../webblob/wasm_blob");
static HTML_BLOB: &[u8] = include_bytes!("../../webblob/index.html");

trait QueryStr {
    fn query_str(&self) -> Option<&str>;
}

impl QueryStr for str {
    fn query_str(&self) -> Option<&str> {
        match self.split_once('?') {
            Some(pair) => Some(pair.1),
            None => None,
        }
    }
}

pub fn web_server(
    tx: SyncSender<Messages>,
    applied_config_rx: Receiver<AnimationConfig>,
) -> anyhow::Result<EspHttpServer> {
    let mut server = EspHttpServer::new(&Default::default())?;

    server.fn_handler("/", Method::Get, |req| {
        req.into_ok_response()?.write_all(HTML_BLOB)?;

        Ok(())
    })?;

    server.fn_handler("/wasm_blob", Method::Get, |req| {
        req.into_response(200, None, &[("Content-Type", "application/wasm")])?
            .write_all(WASM_BLOB)?;

        Ok(())
    })?;

    server.fn_handler("/set_conf", Method::Get, {
        let tx = tx.clone();
        move |req| {
            let query_str = req.uri().query_str().unwrap_or_default();
            let new_config: ReceivedAnimationConfig = match serde_urlencoded::from_str(query_str) {
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
        #[allow(clippy::redundant_clone)]
        let tx = tx.clone();
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

    Ok(server)
}
