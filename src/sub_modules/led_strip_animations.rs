use super::esp_random::EspRand;
use crate::RGBA;
use animation_lang::program::Program;
use animation_lang::vm::{VMState, VMStateConfig, VM};
use anyhow::{anyhow, bail, Result};
use enum_assoc::Assoc;
use esp_idf_hal::gpio::OutputPin;
use serde::Deserialize;
use smart_leds_trait::{SmartLedsWrite, White, RGBW};
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
    pub rgb_brightness: u8,
    pub gradient: Gradient,
}

#[derive(Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReceivedAnimationConfig {
    pub led_quantity: Option<usize>,
    #[serde(with = "humantime_serde")]
    pub animation_duration: Option<Duration>,
    pub target_fps: Option<usize>,
    pub rgb_brightness: Option<u8>,
    pub gradient: Option<Gradient>,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            led_quantity: 150,
            animation_duration: Duration::from_secs(20),
            target_fps: 60,
            rgb_brightness: u8::MAX,
            gradient: Default::default(),
        }
    }
}

pub enum Messages {
    NewConfig(ReceivedAnimationConfig),
    NewProg(Program),
    SetWhite(u8), // GetConfig,
}

impl AnimationConfig {
    pub fn update(&mut self, new_config: ReceivedAnimationConfig) {
        if let Some(new_val) = new_config.target_fps {
            self.target_fps = new_val;
        }
        if let Some(new_val) = new_config.rgb_brightness {
            self.rgb_brightness = new_val;
        }
        if let Some(new_val) = new_config.led_quantity {
            self.led_quantity = new_val;
        }
        if let Some(new_val) = new_config.animation_duration {
            self.animation_duration = new_val;
        }
        if let Some(new_val) = new_config.gradient {
            self.gradient = new_val;
        }
    }
}

pub struct LedStripAnimation {
    ws2812: Ws2812I,
    config: AnimationConfig,
}

#[derive(Assoc, Deserialize, Default, PartialEq, Debug, Clone, Copy)]
#[func(pub const fn val(&self) -> colorous::Gradient)]
pub enum Gradient {
    #[default]
    #[assoc(val = colorous::SINEBOW)]
    Sinebow,
    #[assoc(val = colorous::RAINBOW)]
    Rainbow,
    #[assoc(val = colorous::CUBEHELIX)]
    Cubehelix,
    #[assoc(val = colorous::COOL)]
    Cool,
}

enum VmStatus {
    Running(VMState),
    Stoped((VM, VMStateConfig)),
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
        rx: Receiver<Messages>,
        applied_config_tx: SyncSender<AnimationConfig>,
    ) -> Result<()> {
        use std::time::Instant;

        let calc_delay = |target_fps| Duration::from_millis(1000 / target_fps as u64);
        let calc_discreteness = |target_fps, animation_duration: Duration| -> usize {
            target_fps * animation_duration.as_secs() as usize
        };

        let mut white_brightness = u8::MIN;
        let mut target_delay = calc_delay(self.config.target_fps);
        let mut i = WrappingIndex {
            val: 0,
            wrap_at: calc_discreteness(self.config.target_fps, self.config.animation_duration),
        };
        let mut last_update = Instant::now();
        let mut last_check = Instant::now();

        let mut vm_status = VmStatus::Stoped((
            VM::new(self.config.led_quantity, Default::default()),
            VMStateConfig {
                local_instruction_limit: Some(1_000_000),
                rng: Box::new(EspRand {}),
                ..Default::default()
            },
        ));

        loop {
            match rx.try_recv() {
                Ok(message) => match message {
                    Messages::NewConfig(conf) => {
                        self.config.update(conf);
                        applied_config_tx.send(self.config)?;
                        i.wrap_at = calc_discreteness(
                            self.config.target_fps,
                            self.config.animation_duration,
                        );
                        target_delay = calc_delay(self.config.target_fps);

                        match vm_status {
                            VmStatus::Running(vm_state) => {
                                let (vm, cfg, _) = vm_state.stop();
                                vm_status = VmStatus::Stoped((vm, cfg));
                            }
                            VmStatus::Stoped(_) => (),
                        }
                    }
                    Messages::SetWhite(value) => white_brightness = value,
                    Messages::NewProg(prog) => {
                        vm_status = VmStatus::Running(match vm_status {
                            VmStatus::Running(vm_state) => {
                                let (vm, cfg, _) = vm_state.stop();
                                vm.start(prog, cfg)
                            }
                            VmStatus::Stoped((vm, cfg)) => vm.start(prog, cfg),
                        });
                    }
                },
                Err(TryRecvError::Disconnected) => bail!("Unexpected channel closing"),
                Err(TryRecvError::Empty) => (),
            }

            if last_check.elapsed() > Duration::from_secs(1) {
                let high_water_mark =
                    unsafe { esp_idf_sys::uxTaskGetStackHighWaterMark(std::ptr::null_mut()) };

                println!(
                    "Animation thread stack high water mark: {}",
                    high_water_mark
                );

                last_check = Instant::now();
            }

            if last_update.elapsed() >= target_delay {
                match vm_status {
                    VmStatus::Running(mut vm_state) => {
                        vm_status = match vm_state.next() {
                            None => {
                                println!("Program ended");
                                println!("Halting VM and Waiting for new prog...");
                                let (vm, cfg, _) = vm_state.stop();
                                VmStatus::Stoped((vm, cfg))
                            }
                            Some(Err(e)) => {
                                eprintln!("{:?}", e);
                                println!("Halting VM and Waiting for new prog...");
                                let (vm, cfg, _) = vm_state.stop();
                                VmStatus::Stoped((vm, cfg))
                            }
                            Some(Ok(v)) => {
                                self.ws2812.write(v.map(|mut c| {
                                    c.set_alpha(white_brightness);
                                    c
                                }))?;
                                VmStatus::Running(vm_state)
                            }
                        }
                    }
                    VmStatus::Stoped(_) => {
                        let color = self
                            .config
                            .gradient
                            .val()
                            .eval_rational(
                                (i.val as isize - i.wrap_at as isize / 2).unsigned_abs(),
                                i.wrap_at / 2,
                            )
                            .apply_brightness(self.config.rgb_brightness);
                        self.set_color(RGBW::from((
                            color.r,
                            color.g,
                            color.b,
                            White(white_brightness),
                        )))?;
                        i.increment();

                        last_update = Instant::now();
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

trait SetBrightness {
    fn apply_brightness(self, brightness: u8) -> Self;
}

impl SetBrightness for colorous::Color {
    fn apply_brightness(mut self, brightness: u8) -> Self {
        let apply_alpha = |v: u8, a: u8| (v as f32 * (a as f32 / u8::MAX as f32)) as u8;

        self.r = apply_alpha(self.r, brightness);
        self.g = apply_alpha(self.g, brightness);
        self.b = apply_alpha(self.b, brightness);
        self
    }
}

struct WrappingIndex {
    val: usize,
    wrap_at: usize,
}

impl WrappingIndex {
    fn increment(&mut self) {
        if self.val < self.wrap_at {
            self.val += 1;
        } else {
            self.val = 0;
        }
    }
}
