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

static CORS_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Headers", "*"),
    ("Access-Control-Allow-Methods", "PUT,POST,GET,OPTIONS"),
    ("Access-Control-Max-Age", "600"),
];

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

fn add_cors(server: &mut EspHttpServer, endpoint: &str) -> anyhow::Result<()> {
    server.fn_handler(endpoint, Method::Options, |req| {
        req.into_response(200, None, CORS_HEADERS)?;

        Ok(())
    })?;

    Ok(())
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
        req.into_ok_response()?.write_all(WASM_BLOB)?;

        Ok(())
    })?;

    server.fn_handler("/set_white", Method::Get, {
        let tx = tx.clone();
        move |req| {
            let query_str = req.uri().query_str().unwrap_or_default();
            let white_brightness =
                match form_urlencoded::parse(query_str.as_bytes()).find(|pair| pair.0 == "val") {
                    Some(pair) => match str::parse::<u8>(&pair.1) {
                        Ok(v) => v,
                        Err(e) => {
                            req.into_response(400, Some(&e.to_string()), &[])?;
                            return Ok(());
                        }
                    },
                    None => {
                        req.into_response(400, Some("missing query param: val"), &[])?;
                        return Ok(());
                    }
                };

            tx.send(Messages::SetWhite(white_brightness))?;
            Ok(())
        }
    })?;

    server.fn_handler("/set_conf", Method::Get, {
        let tx = tx.clone();
        move |req| {
            let query_str = req.uri().query_str().unwrap_or_default();
            let new_config: ReceivedAnimationConfig = match serde_urlencoded::from_str(query_str) {
                Ok(cfg) => cfg,
                Err(e) => {
                    req.into_response(400, Some(&e.to_string()), &[])?;
                    return Ok(());
                }
            };

            tx.send(Messages::NewConfig(new_config))?;
            req.into_ok_response()?
                .write_all(format!("Applied config: {:?}", applied_config_rx.recv()?).as_bytes())?;
            Ok(())
        }
    })?;

    add_cors(&mut server, "/send_prog_base64")?;
    server.fn_handler("/send_prog_base64", Method::Post, {
        #[allow(clippy::redundant_clone)]
        let tx = tx.clone();
        move |mut req| {
            let mut body = Vec::new();
            ToStd::new(&mut req).read_to_end(&mut body)?;
            let bin_prog = match base64::decode(body) {
                Ok(bin_prog) => bin_prog,
                Err(e) => {
                    req.into_response(400, Some(&e.to_string()), CORS_HEADERS)?;
                    return Ok(());
                }
            };

            tx.send(Messages::NewProg(Program::from_binary(bin_prog)))?;

            req.into_response(200, None, CORS_HEADERS)?;
            Ok(())
        }
    })?;

    Ok(server)
}
