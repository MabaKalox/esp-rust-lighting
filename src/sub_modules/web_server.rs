use crate::AnimationConfig;
use embedded_svc::errors::wrap::WrapError;
use embedded_svc::http::server::registry::Registry;
use embedded_svc::http::server::{Request, Response};
use embedded_svc::http::SendStatus;
use esp_idf_svc::http::server::EspHttpServer;
use std::num::{NonZeroU64, NonZeroU8};
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
        .handle_get("/set_animation_duration_s", {
            let led_strip_config = led_strip_config.clone();
            move |req, resp| {
                let query_params = url::form_urlencoded::parse(req.query_string().as_bytes());
                let animation_duration_s = query_params
                    .filter(|p| p.0 == "animation_duration_s")
                    .map(|p| str::parse::<NonZeroU64>(&p.1))
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No query parm animation_duration_s"))??;

                let mut config = led_strip_config.lock().unwrap();
                config.animation_duration = Duration::from_secs(animation_duration_s.get());
                resp.send_str(&format!(
                    "New animation duration is: {:?}",
                    config.animation_duration
                ))?;
                Ok(())
            }
        })?
        .handle_get("/set_brightness", {
            let led_strip_config = led_strip_config.clone();
            move |req, resp| {
                let query_params = url::form_urlencoded::parse(req.query_string().as_bytes());
                let brightness = query_params
                    .filter(|p| p.0 == "brightness")
                    .map(|p| str::parse::<NonZeroU8>(&p.1))
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No query param brightness"))??;

                let mut config = led_strip_config.lock().unwrap();
                config.brighness = brightness;
                resp.send_str(&format!("New brightness is: {}", config.brighness))?;
                Ok(())
            }
        })?
        .handle_get("/bar", |_req, resp| {
            resp.status(403)
                .status_message("No permissions")
                .send_str("You have no permissions to access this page")?;

            Ok(())
        })?
        .handle_get("/panic", |_req, _resp| panic!("User requested a panic!"))?;

    Ok(server)
}
