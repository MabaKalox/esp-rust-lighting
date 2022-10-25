use crate::RGBA;
use anyhow::{anyhow, Result};
use esp_idf_hal::gpio::OutputPin;
use smart_leds_trait::{SmartLedsWrite, White, RGB, RGBW};
use std::num::NonZeroU8;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use ws2812_esp32_rmt_driver::driver::color::LedPixelColorGrbw32;
use ws2812_esp32_rmt_driver::{LedPixelEsp32Rmt, RGBW8};

type Ws2812I = LedPixelEsp32Rmt<RGBW8, LedPixelColorGrbw32>;

pub struct AnimationConfig {
    pub led_quantity: usize,
    pub animation_duration: Duration,
    pub target_fps: usize,
    pub brighness: NonZeroU8,
}

pub struct LedStripAnimation {
    ws2812: Arc<Mutex<Ws2812I>>,
    pub config: Arc<Mutex<AnimationConfig>>,
}

impl LedStripAnimation {
    pub fn new<P: OutputPin>(led_pin: P, rmt_channel: u8, config: AnimationConfig) -> Result<Self> {
        let ws2812 = LedPixelEsp32Rmt::<RGBW8, LedPixelColorGrbw32>::new(
            rmt_channel,
            led_pin.pin().try_into().unwrap(),
        )
        .map_err(|e| anyhow!("{:?}", e))?;

        Ok(Self {
            ws2812: Arc::new(Mutex::new(ws2812)),
            config: Arc::new(Mutex::new(config)),
        })
    }

    pub fn set_color(
        ws2812: &mut Ws2812I,
        led_quantity: usize,
        color: RGBA<u8, White<u8>>,
    ) -> Result<()> {
        let pixels = std::iter::repeat(color).take(led_quantity);
        ws2812.write(pixels)?;
        Ok(())
    }

    pub fn gradient(&mut self, gradient: colorous::Gradient) -> JoinHandle<()> {
        let ws2812 = self.ws2812.clone();
        let config = self.config.clone();

        std::thread::spawn(move || {
            let mut frames_counter = 0;
            let mut now = std::time::Instant::now();

            loop {
                let (gradient_discreteness, target_delay, led_quantity, brighness) = {
                    let config = config.lock().unwrap();
                    let gradient_discreteness =
                        config.target_fps * config.animation_duration.as_secs() as usize;
                    let target_delay = Duration::from_millis(1000 / config.target_fps as u64);

                    (
                        gradient_discreteness,
                        target_delay,
                        config.led_quantity,
                        config.brighness,
                    )
                };

                for i in 0..gradient_discreteness {
                    let write_start = std::time::Instant::now();

                    let color = gradient.eval_rational(i, gradient_discreteness);
                    let mut rgbw = RGBW::from((color.r, color.g, color.b, White(0)));

                    rgbw.r /= u8::MAX / brighness;
                    rgbw.g /= u8::MAX / brighness;
                    rgbw.b /= u8::MAX / brighness;

                    Self::set_color(&mut ws2812.lock().unwrap(), led_quantity, rgbw).unwrap();

                    // Smart delay
                    if write_start.elapsed() < target_delay {
                        std::thread::sleep(target_delay - write_start.elapsed());
                    }

                    // Frames counter
                    frames_counter += 1;
                    if now.elapsed() >= Duration::from_secs(1) {
                        // println!("Frames counted: {}", frames_counter);
                        frames_counter = 0;
                        now = std::time::Instant::now();
                    }
                }
            }
        })
    }
}
