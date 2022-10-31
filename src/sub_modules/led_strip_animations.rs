use crate::RGBA;
use anyhow::{anyhow, bail, Result};
use esp_idf_hal::gpio::OutputPin;
use serde::Deserialize;
use smart_leds_trait::{SmartLedsWrite, White, RGBW};
use std::num::NonZeroU8;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::{SyncSender, TryRecvError};
use std::time::Duration;
use ws2812_esp32_rmt_driver::driver::color::LedPixelColorGrbw32;
use ws2812_esp32_rmt_driver::{LedPixelEsp32Rmt, RGBW8};

type Ws2812I = LedPixelEsp32Rmt<RGBW8, LedPixelColorGrbw32>;

#[derive(Debug, Clone, Copy)]
pub struct AnimationConfig {
    pub led_quantity: usize,
    pub animation_duration: Duration,
    pub target_fps: usize,
    pub brightness: NonZeroU8,
    pub white_brightness: u8,
}

#[derive(Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReceivedAnimationConfig {
    pub led_quantity: Option<usize>,
    #[serde(with = "humantime_serde")]
    pub animation_duration: Option<Duration>,
    pub target_fps: Option<usize>,
    pub brightness: Option<NonZeroU8>,
    pub white_brightness: Option<u8>,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            led_quantity: 150,
            animation_duration: Duration::from_secs(20),
            target_fps: 60,
            brightness: NonZeroU8::MAX,
            white_brightness: u8::MIN,
        }
    }
}

pub enum Messages {
    NewConfig(ReceivedAnimationConfig),
    // GetConfig,
}

impl AnimationConfig {
    pub fn update(&mut self, new_config: ReceivedAnimationConfig) {
        if let Some(new_val) = new_config.target_fps {
            self.target_fps = new_val;
        }
        if let Some(new_val) = new_config.brightness {
            self.brightness = new_val;
        }
        if let Some(new_val) = new_config.led_quantity {
            self.led_quantity = new_val;
        }
        if let Some(new_val) = new_config.animation_duration {
            self.animation_duration = new_val;
        }
        if let Some(new_val) = new_config.white_brightness {
            self.white_brightness = new_val;
        }
    }
}

pub struct LedStripAnimation {
    ws2812: Ws2812I,
    config: AnimationConfig,
}

impl LedStripAnimation {
    pub fn new<P: OutputPin>(led_pin: P, rmt_channel: u8, config: AnimationConfig) -> Result<Self> {
        let ws2812 = LedPixelEsp32Rmt::<RGBW8, LedPixelColorGrbw32>::new(
            rmt_channel,
            led_pin.pin().try_into().unwrap(),
        )
        .map_err(|e| anyhow!("{:?}", e))?;

        Ok(Self { ws2812, config })
    }

    pub fn set_color(&mut self, color: RGBA<u8, White<u8>>) -> Result<()> {
        let pixels = std::iter::repeat(color).take(self.config.led_quantity);
        self.ws2812.write(pixels)?;
        Ok(())
    }

    pub fn led_strip_loop(
        &mut self,
        gradient: colorous::Gradient,
        rx: Receiver<Messages>,
        applied_config_tx: SyncSender<AnimationConfig>,
    ) -> Result<()> {
        let mut i = 0;
        let mut gradient_discreteness =
            self.config.target_fps * self.config.animation_duration.as_secs() as usize;
        let mut target_delay = Duration::from_millis(1000 / self.config.target_fps as u64);

        loop {
            match rx.try_recv() {
                Ok(message) => match message {
                    Messages::NewConfig(conf) => {
                        self.config.update(conf);
                        applied_config_tx.send(self.config)?;
                        gradient_discreteness = self.config.target_fps
                            * self.config.animation_duration.as_secs() as usize;
                        target_delay = Duration::from_millis(1000 / self.config.target_fps as u64);
                    } // Messages::GetConfig => applied_config_tx.send(self.config)?,
                },
                Err(TryRecvError::Disconnected) => bail!("Unexpected channel closing"),
                Err(TryRecvError::Empty) => (),
            }

            let write_start = std::time::Instant::now();

            let color = gradient.eval_rational(i, gradient_discreteness);
            let mut rgbw = RGBW::from((
                color.r,
                color.g,
                color.b,
                White(self.config.white_brightness),
            ));

            rgbw.r /= u8::MAX / self.config.brightness;
            rgbw.g /= u8::MAX / self.config.brightness;
            rgbw.b /= u8::MAX / self.config.brightness;

            self.set_color(rgbw)?;

            // Smart delay
            if write_start.elapsed() < target_delay {
                std::thread::sleep(target_delay - write_start.elapsed());
            }
            i += 1;
            if i >= gradient_discreteness {
                i = 0;
            }
        }
    }
}
