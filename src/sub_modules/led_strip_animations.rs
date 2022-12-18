use super::esp_random::EspRand;
use animation_lang::program::Program;
use animation_lang::vm::{VMState, VMStateConfig, VM};
use anyhow::{anyhow, bail, Result};
use esp_idf_hal::gpio::OutputPin;
use serde::Deserialize;
use smart_leds_trait::{SmartLedsWrite, White};
use std::sync::mpsc::Receiver;
use std::sync::mpsc::{SyncSender, TryRecvError};
use std::time::Duration;
use ws2812_esp32_rmt_driver::driver::color::LedPixelColorGrbw32;
use ws2812_esp32_rmt_driver::{LedPixelEsp32Rmt, RGBW8};

type Ws2812I = LedPixelEsp32Rmt<RGBW8, LedPixelColorGrbw32>;

#[derive(Debug, Clone, Copy)]
pub struct AnimationConfig {
    pub led_quantity: usize,
    pub target_fps: usize,
}

#[derive(Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReceivedAnimationConfig {
    pub led_quantity: Option<usize>,
    pub target_fps: Option<usize>,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            led_quantity: 150,
            target_fps: 60,
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
        if let Some(new_val) = new_config.led_quantity {
            self.led_quantity = new_val;
        }
    }
}

pub struct LedStripAnimation {
    ws2812: Ws2812I,
    config: AnimationConfig,
}

enum VmStatus {
    Running(VMState),
    Stoped((VM, VMStateConfig)),
}

impl LedStripAnimation {
    pub fn new<P: OutputPin>(led_pin: P, rmt_channel: u8, config: AnimationConfig) -> Result<Self> {
        let ws2812 =
            LedPixelEsp32Rmt::<RGBW8, LedPixelColorGrbw32>::new(rmt_channel, led_pin.pin() as u32)
                .map_err(|e| anyhow!("{:?}", e))?;

        Ok(Self { ws2812, config })
    }

    pub fn led_strip_loop(
        &mut self,
        rx: Receiver<Messages>,
        applied_config_tx: SyncSender<AnimationConfig>,
    ) -> Result<()> {
        use std::time::Instant;

        let calc_delay = |target_fps| Duration::from_millis(1000 / target_fps as u64);

        let mut white_brightness = u8::MIN;
        let mut target_delay = calc_delay(self.config.target_fps);
        let mut last_update = Instant::now();
        // let mut last_stack_check = Instant::now();

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
                        target_delay = calc_delay(self.config.target_fps);

                        match vm_status {
                            VmStatus::Running(vm_state) => {
                                println!("Restarting vm");
                                let (mut vm, cfg, prog) = vm_state.stop();
                                vm.set_stip_length(self.config.led_quantity);
                                vm_status = VmStatus::Running(vm.start(prog, cfg));
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
                Err(TryRecvError::Disconnected) => panic!(),
                Err(TryRecvError::Empty) => (),
            }

            // if last_stack_check.elapsed() > Duration::from_secs(1) {
            //     let high_water_mark =
            //         unsafe { esp_idf_sys::uxTaskGetStackHighWaterMark(std::ptr::null_mut()) };

            //     println!(
            //         "Animation thread stack high water mark: {}",
            //         high_water_mark
            //     );

            //     last_stack_check = Instant::now();
            // }

            if last_update.elapsed() >= target_delay {
                last_update = Instant::now();
                if let VmStatus::Running(mut vm_state) = vm_status {
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
                            self.ws2812.write(v.map(|c| {
                                RGBW8::new_alpha(c.r, c.g, c.b, White(white_brightness))
                            }))?;
                            VmStatus::Running(vm_state)
                        }
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
