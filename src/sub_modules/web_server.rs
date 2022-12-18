use crate::sub_modules::led_strip_animations::{AnimationConfig, Messages};
use animation_lang::program::Program;
use anyhow::Result;
use embedded_svc::http::server::Query;
use embedded_svc::http::Method;
use embedded_svc::io::adapters::ToStd;
use embedded_svc::io::Write;
use esp_idf_svc::http::server::EspHttpServer;
use std::io::Read;
use std::sync::mpsc::{Receiver, SyncSender};

use super::led_strip_animations::ReceivedAnimationConfig;

pub fn web_server(
    tx: SyncSender<Messages>,
    applied_config_rx: Receiver<AnimationConfig>,
) -> anyhow::Result<EspHttpServer> {
    let mut server = EspHttpServer::new(&Default::default())?;

    server.fn_handler("/", Method::Get, |req| {
        req.into_ok_response()?
            .write_all("Hello from Rust!".as_bytes())?;

        Ok(())
    })?;

    server.fn_handler("/set_white", Method::Get, {
        let tx = tx.clone();
        move |req| {
            let white_brightness = match req.uri().split_once('?') {
                Some(url_split) => match url::form_urlencoded::parse(url_split.1.as_bytes())
                    .find(|pair| pair.0 == "val")
                {
                    Some(pair) => match str::parse::<u8>(&pair.1) {
                        Ok(val) => val,
                        Err(e) => {
                            req.into_response(400, Some(&e.to_string()), &[])?;
                            return Ok(());
                        }
                    },
                    None => {
                        req.into_response(400, Some("missing query param: val"), &[])?;
                        return Ok(());
                    }
                },
                None => {
                    req.into_response(400, Some("missing query string"), &[])?;
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
            let new_config: ReceivedAnimationConfig = match req.uri().split_once('?') {
                Some(url_split) => match serde_qs::from_str(url_split.1) {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        req.into_response(400, Some(&e.to_string()), &[])?;
                        return Ok(());
                    }
                },
                None => {
                    req.into_response(400, Some("missing query string"), &[])?;
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
        let tx = tx.clone();
        move |mut req| {
            let mut body = Vec::new();
            ToStd::new(&mut req).read_to_end(&mut body)?;
            let bin_prog = match base64::decode(body) {
                Ok(bin_prog) => bin_prog,
                Err(e) => {
                    req.into_response(400, Some(&e.to_string()), &[])?;
                    return Ok(());
                }
            };

            tx.send(Messages::NewProg(Program::from_binary(bin_prog)))?;
            Ok(())
        }
    })?;

    Ok(server)
}
