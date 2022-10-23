use crate::sub_modules::macros::enclose;
use crate::AnimationConfig;
use embedded_svc::errors::wrap::WrapError;
use embedded_svc::http::server::registry::Registry;
use embedded_svc::http::server::Response;
use embedded_svc::http::SendStatus;
use esp_idf_svc::http::server::EspHttpServer;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn web_server(led_strip_config: Arc<Mutex<AnimationConfig>>) -> anyhow::Result<EspHttpServer> {
    let mut server = EspHttpServer::new(&Default::default())?;

    server
        .handle_get("/", |_req, resp| {
            resp.send_str("Hello from Rust!")?;
            Ok(())
        })?
        .handle_get("/foo", |_req, _resp| {
            Err(WrapError("Boo, something happened!").into())
        })?
        .handle_get(
            "/speed_up",
            enclose! { (led_strip_config) move |_req, resp| {
                let mut config = led_strip_config.lock().unwrap();
                if config.animation_duration > Duration::from_secs(1) {
                    config.animation_duration -= Duration::from_secs(1);
                    resp.send_str(&format!(
                        "New animation duration is: {:?}",
                        config.animation_duration
                    ))?;
                    Ok(())
                } else {
                    Err(WrapError("Too fast").into())
                }
            }},
        )?
        .handle_get(
            "/speed_down",
            enclose! { (led_strip_config) move |_req, resp| {
                let mut config = led_strip_config.lock().unwrap();
                config.animation_duration += Duration::from_secs(1);
                resp.send_str(&format!(
                    "New animation duration is: {:?}",
                    config.animation_duration
                ))?;
                Ok(())
            }},
        )?
        .handle_get("/bar", |_req, resp| {
            resp.status(403)
                .status_message("No permissions")
                .send_str("You have no permissions to access this page")?;

            Ok(())
        })?
        .handle_get("/panic", |_req, _resp| panic!("User requested a panic!"))?;

    Ok(server)
}
