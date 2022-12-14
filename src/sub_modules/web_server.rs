use crate::sub_modules::led_strip_animations::{
    AnimationConfig, Messages, ReceivedAnimationConfig,
};
use embedded_svc::errors::wrap::WrapError;
use embedded_svc::http::server::registry::Registry;
use embedded_svc::http::server::{Request, Response};
use embedded_svc::http::SendStatus;
use esp_idf_svc::http::server::EspHttpServer;
use serde_qs;
use std::sync::mpsc::{Receiver, SyncSender};

pub fn web_server(
    tx: SyncSender<Messages>,
    applied_config_rx: Receiver<AnimationConfig>,
) -> anyhow::Result<EspHttpServer> {
    let mut server = EspHttpServer::new(&Default::default())?;

    server
        .handle_get("/", |_req, resp| {
            resp.send_str("Hello from Rust!")?;
            Ok(())
        })?
        .handle_get("/foo", |_req, _resp| {
            Err(WrapError("Boo, something happened!").into())
        })?
        .handle_get("/set_conf", {
            let tx = tx.clone();
            move |req, resp| {
                let new_config: ReceivedAnimationConfig = serde_qs::from_str(req.query_string())?;
                tx.send(Messages::NewConfig(new_config))?;

                resp.send_str(&format!("{:?}", applied_config_rx.recv()))?;

                Ok(())
            }
        })?
        .handle_get("/set_white", {
            let tx = tx.clone();
            move |req, _resp| {
                let white_brightness = url::form_urlencoded::parse(req.query_string().as_bytes())
                    .filter(|p| p.0 == "value")
                    .map(|p| str::parse::<u8>(&p.1))
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No parameter value"))??;

                tx.send(Messages::SetWhite(white_brightness))?;

                Ok(())
            }
        })?
        // .handle_post("/set_prog", {
        //     let tx = tx.clone();
        //     move |req, _resp| {
        //         let smth = req.reader();
        //         Ok(())
        //     }
        // })?
        .handle_get("/bar", |_req, resp| {
            resp.status(403)
                .status_message("No permissions")
                .send_str("You have no permissions to access this page")?;

            Ok(())
        })?
        .handle_get("/panic", |_req, _resp| panic!("User requested a panic!"))?;

    Ok(server)
}
