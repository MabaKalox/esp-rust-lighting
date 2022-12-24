# Esp Rust Lighting

Firmware for esp32-c3 microcontroller written in Rust, which shows animations on sk6812 led strip.
Compiled animations writen in [animation-lang](https://github.com/MabaKalox/animation-lang) can be uploaded using build
in web server.

## Description

Main idea of this project is to allow users write their own animations
in [animation-lang](https://github.com/MabaKalox/animation-lang)
using web-fronend and send them to the microcontroller for demonstrating on
sk6812 addressable led strip. For compiling and running [animation-lang](https://github.com/MabaKalox/animation-lang)
is used.

* Compiler is running in client browser though webassembly, and convert text into compiled program
* Virtual-machine is running on microcontroller as runtime for compiled programs

For details about animation language and examples of programs, please
check my [animation-lang](https://github.com/MabaKalox/animation-lang) repository.

## Getting Started

### Compiling Dependencies

* [clang](https://clang.llvm.org/)
* [pip](https://pypi.org/project/pip/)
* [ldproxy](https://crates.io/crates/ldproxy/0.3.2)
* [rust](https://www.rust-lang.org/learn/get-started)

### Flashing Tool

* [espflash](https://crates.io/crates/espflash)

### Getting sources

1) Clone project repository
   ```sh
   git clone https://github.com/MabaKalox/std-esp-rust-lighting.git
   ```
2) Navigate into it
   ```sh
   cd std-esp-rust-lighting
   ```

### Configuring

For operation, Wi-Fi connection is required, and wifi-manager is not implemented yet.
So you need to supply SSID and password of your Wi-Fi beforehand. Also you can specify
number of leds on your led strip.

1) create `cfg.toml` file in project directory with following content:

```toml
[std-esp-rust-lighting]
wifi_ssid = "your wifi SSID"
wifi_pass = "your wifi password"
led_quantity = 150
```

### Building

1) Install dependencies listed in `Compiling Dependencies`
2) Add rust nightly toolchain
   ```sh 
   rustup install nightly
   ```
3) Add rust-src for nightly toolchain
   ```sh
   rustup component add rust-src --toolchain nightly
   ```
4) Build, _you should be in project directory_
   ```sh
   cargo build --release
   ```

### Flashing

1) Install espflash
   ```sh
    cargo install espflash
   ```
2) Flash, replace `/dev/ttyACM0` by path to `tty` device of connected esp32-c3 over usb.
   ```
   espflash --speed 921600 --partition-table partitions.csv /dev/ttyACM0 /target/riscv32imc-esp-espidf/release/std-esp-rust-lighting
   ```

## Usage

1) Attach data pin of sk6812 led strip to GPIO6 of esp32-c3
2) Power up esp32-c3
3) Wait till both leds on microcontroller turn off

### Frontend

Navigate to [http://rust_led_strip.local](http://rust_led_strip.local/), be sure that your device support `mdns`.
Alternatively you can find ip of microcontroller in your router settings and use ip instead: `http://[IP]`

* Try to send some program:
    1) Open `Programming` tab (top left corner).
    2) Try to write some program in left textarea, e.g:
        ```
        // Blank everything
        for(n=get_length) {
          set_pixel(n-1, 0, 0, 0);
        };
        loop {
          // set random pixel to random color
          lucky = random(get_length);
          for(n=get_length) {
            if(n == lucky) {
              r = random(255);
              g = random(255);
              b = random(255);
              set_pixel(n-1, r, g, b);
            }
          };
          // Display on led strip
          blit;
          // Black one random pixel
          luckyb = random(get_length);
          for(n=get_length) {
            if(n == luckyb) {
              set_pixel(n-1, 0, 0, 0);
            }
          };
          blit;
        }   
        ```
    3) Check frame color
        * Green - compiled successfully, disassembly can be found in right window
        * Red - compiling error, detailes can be found in right window
    4) Press `Send` green button on web page in top right corner
    5) You should see how random pixels turn on and off on led strip
* Try presets (saved programs in your web browser)
    1) Write some program in `Programming` tab
    2) Click `Save` button in middle part of screen
    3) Input desired `name` for preset
    4) Preset should appear under `Save` button:

    * You can apply it, by clicking on `name`
    * You can delete it, by clicking `X` button near `name`
* Try to change configuration:
    1) Open `Configuring tab` tab (top right corner)
    2) Here you can change
        * `FPS` - changes delay between presenting new frames
        * `Led Quantity` - self-explanatory, worth to note, it is not reboot persistent
        * `White brightness` - sk6812 has dedicated white led in pixels, and you control their brightness by this
          setting
    3) Apply by `Submit` button
    4) Check applied config in window below

### REST API

The REST API to the std-esp-rust-lightning

---
#### Send compiled program in base64

Request

`POST /send_prog_base64`

Body

`base64 encoded string with compiled program`

Example

```
curl -X POST -d "[base64 encoded compiled program]" http://rust_led_strip.local/send_prog_base64
```

---
#### Set configuration

Request

`POST /set_conf`

Query params

`fps` - frames per second [0, 255]

`white_brightness` - brightness of white subpixel in _sk6812_ [0, 255]

`led_quantity` - how many leds in led strip to control [0, 2^32-1]

Example

```
curl -X POST -G -d 'white_brightness=20' -d 'led_quantity=150' -d 'fps=60' http://http://rust_led_strip.local/set_conf
```

## License

This project is licensed under the MIT License - see the LICENSE.md file for details

## Acknowledgments

* [esp-rs](https://github.com/esp-rs)
