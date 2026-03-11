# android-auto

[![Crates.io](https://img.shields.io/crates/v/android-auto)](https://crates.io/crates/android-auto)
[![docs.rs](https://docs.rs/android-auto/badge.svg)](https://docs.rs/android-auto/latest/android_auto/)
[![License: LGPL-3.0-or-later](https://img.shields.io/badge/license-LGPL--3.0--or--later-blue)](LICENSE)

A Rust implementation of the Android Auto protocol — both a reusable library and a working head unit application. Connect an Android phone to a Linux host and get a full Android Auto experience, including wireless connection via Bluetooth.

---

## Table of Contents

- [What is this?](#what-is-this)
- [How it works](#how-it-works)
- [Features](#features)
- [Prerequisites](#prerequisites)
- [Building](#building)
- [Running the head unit](#running-the-head-unit)
- [Using the library](#using-the-library)
- [Architecture](#architecture)
- [Contributing](#contributing)
- [License](#license)

---

## What is this?

`android-auto` is two things in one repository:

1. **A library crate** (`android-auto` on crates.io) that implements the Android Auto wire protocol. You can embed it in your own Rust application to build a custom head unit.
2. **A head unit example application** (`examples/main/`) that demonstrates the library in action, allowing a Linux machine to act as an Android Auto head unit for a connected phone.

---

## How it works

Android Auto uses a binary framing protocol over a TLS-secured transport. The phone (acting as the server) and the head unit (acting as the client) exchange protobuf-encoded messages. At a high level:

1. **Transport layer** — wired USB (via the `usb` feature) or wireless (Wi-Fi + Bluetooth for handoff, via the `wireless` feature). Bluetooth is used to advertise the head unit and negotiate the wireless connection; actual media data flows over Wi-Fi.
2. **TLS handshake** — the head unit authenticates using an X.509 client certificate. This library handles the full TLS negotiation via [`rustls`](https://github.com/rustls/rustls).
3. **Frame layer** — messages may be split across multiple frames (`FrameHeaderType`). The library reassembles them transparently.
4. **Channel layer** — Android Auto multiplexes multiple logical channels over a single connection (video, audio, input, sensors, etc.). Each channel has its own message types, encoded with protobuf.

The library exposes this through a clean async trait interface so that integrators only need to handle the application-level events they care about.

---

## Features

- Wired USB Android Auto connections (enable with the `usb` feature)
- Wireless (Bluetooth-initiated) Android Auto connections (enable with the `wireless` feature)
- Full TLS client support via `rustls`
- Protobuf message encoding/decoding for all standard Android Auto channels
- Async-first API built on `tokio`
- Configurable head unit identity (`HeadUnitInfo`, `AndroidAutoConfiguration`)
- Video channel trait (`AndroidAutoVideoChannelTrait`) for custom video rendering backends
- Audio output and input channel traits (`AndroidAutoAudioOutputTrait`, `AndroidAutoAudioInputTrait`)
- Input channel trait (`AndroidAutoInputChannelTrait`) for touchscreen and keycode support
- Sensor channel trait (`AndroidAutoSensorTrait`) for reporting sensor data to the phone
- Navigation channel trait (`AndroidAutoNavigationTrait`) for receiving turn-by-turn updates

---

## Prerequisites

- **Rust** (stable, recent version recommended — see `rust-version` in `Cargo.toml`)
- **Tokio** async runtime (pulled in automatically as a dependency)
- For wired USB support: enable the `usb` feature (uses the `nusb` crate)
- For wireless support: enable the `wireless` feature (uses the `bluetooth-rust` crate) and ensure a system Bluetooth stack is accessible
- A phone running Android with the Android Auto app installed

---

## Building

Clone the repository and build with Cargo:

```bash
git clone https://github.com/uglyoldbob/android-auto.git
cd android-auto
cargo build --release
```

To enable wired USB support:

```bash
cargo build --release --features usb
```

To enable wireless (Bluetooth-initiated) Android Auto support:

```bash
cargo build --release --features wireless
```

To enable both:

```bash
cargo build --release --features usb,wireless
```

---

## Running the head unit

The head unit is provided as an example application. Run it with:

```bash
cargo run --example main --release
```

> **Note:** the example uses several dev-dependencies (e.g. `eframe`, `openh264`, `cpal`). Make sure all system libraries they require are installed.

**Wired connection:** plug your phone in via USB and build with the `usb` feature. The head unit will detect the connection and initiate the Android Auto handshake automatically.

**Wireless connection:** build with the `wireless` feature and ensure Bluetooth is enabled on both your host machine and phone. The library starts a Bluetooth server that advertises the head unit; the phone will discover it and negotiate a Wi-Fi session.

---

## Using the library

Add `android-auto` to your `Cargo.toml`:

```toml
[dependencies]
android-auto = "0.3.3"

# For wired (USB) support:
android-auto = { version = "0.3.3", features = ["usb"] }

# For wireless (Bluetooth-initiated) support:
android-auto = { version = "0.3.3", features = ["wireless"] }

# For both:
android-auto = { version = "0.3.3", features = ["usb", "wireless"] }
```

### Initialization

Call `android_auto::setup()` once at program startup before doing anything else. It installs the TLS crypto provider and returns an `AndroidAutoSetup` token that **must** be passed to `run()` (and related methods). Requiring this token at the call site is a compile-time guarantee that initialisation is never accidentally skipped:

```rust
fn main() {
    let setup = android_auto::setup();
    // pass `setup` to run() later …
}
```

### Minimal example

```rust
use android_auto::{
    AndroidAutoConfiguration, AndroidAutoMainTrait, AndroidAutoVideoChannelTrait,
    AndroidAutoAudioOutputTrait, AndroidAutoAudioInputTrait, AndroidAutoInputChannelTrait,
    AndroidAutoSensorTrait, HeadUnitInfo, VideoConfiguration, InputConfiguration,
    SensorInformation, AudioChannelType, SendableAndroidAutoMessage,
};

struct MyHeadUnit;

#[async_trait::async_trait]
impl AndroidAutoSensorTrait for MyHeadUnit {
    fn get_supported_sensors(&self) -> &SensorInformation { todo!() }
    async fn start_sensor(&self, _: android_auto::Wifi::sensor_type::Enum) -> Result<(), ()> { Ok(()) }
}

#[async_trait::async_trait]
impl AndroidAutoAudioOutputTrait for MyHeadUnit {
    async fn open_output_channel(&self, _: AudioChannelType) -> Result<(), ()> { Ok(()) }
    async fn close_output_channel(&self, _: AudioChannelType) -> Result<(), ()> { Ok(()) }
    async fn receive_output_audio(&self, _: AudioChannelType, _: Vec<u8>) {}
    async fn start_output_audio(&self, _: AudioChannelType) {}
    async fn stop_output_audio(&self, _: AudioChannelType) {}
}

#[async_trait::async_trait]
impl AndroidAutoAudioInputTrait for MyHeadUnit {
    async fn open_input_channel(&self) -> Result<(), ()> { Ok(()) }
    async fn close_input_channel(&self) -> Result<(), ()> { Ok(()) }
    async fn start_input_audio(&self) {}
    async fn stop_input_audio(&self) {}
    async fn audio_input_ack(&self, _: u8, _: android_auto::Wifi::AVMediaAckIndication) {}
}

#[async_trait::async_trait]
impl AndroidAutoInputChannelTrait for MyHeadUnit {
    async fn binding_request(&self, _: u32) -> Result<(), ()> { Ok(()) }
    fn retrieve_input_configuration(&self) -> &InputConfiguration { todo!() }
}

#[async_trait::async_trait]
impl AndroidAutoVideoChannelTrait for MyHeadUnit {
    async fn receive_video(&self, _data: Vec<u8>, _timestamp: Option<u64>) {}
    async fn setup_video(&self) -> Result<(), ()> { Ok(()) }
    async fn teardown_video(&self) {}
    async fn wait_for_focus(&self) {}
    async fn set_focus(&self, _focus: bool) {}
    fn retrieve_video_configuration(&self) -> &VideoConfiguration { todo!() }
}

#[async_trait::async_trait]
impl AndroidAutoMainTrait for MyHeadUnit {
    async fn connect(&self) {}
    async fn disconnect(&self) {}
    async fn get_receiver(&self) -> Option<tokio::sync::mpsc::Receiver<SendableAndroidAutoMessage>> {
        None
    }
}

#[tokio::main]
async fn main() {
    let setup = android_auto::setup();

    let config = AndroidAutoConfiguration {
        unit: HeadUnitInfo {
            name: "My Head Unit".to_string(),
            car_model: "My Car".to_string(),
            car_year: "2024".to_string(),
            car_serial: "000000".to_string(),
            left_hand: true,
            head_manufacturer: "ACME".to_string(),
            head_model: "HU-1".to_string(),
            sw_build: "1".to_string(),
            sw_version: "1.0".to_string(),
            native_media: false,
            hide_clock: None,
        },
        custom_certificate: None,
    };

    let mut js = tokio::task::JoinSet::new();
    let _ = Box::new(MyHeadUnit).run(config, &mut js, &setup).await;
}
```

See the [docs.rs documentation](https://docs.rs/android-auto/latest/android_auto/) and the `examples/main/` directory for a complete, working reference implementation.

---

## Architecture

```
android-auto/
├── src/
│   ├── lib.rs          # Library entry point and protocol logic
│   ├── control.rs      # Control channel handler
│   ├── video.rs        # Video channel handler
│   ├── mediaaudio.rs   # Media audio channel handler
│   ├── speechaudio.rs  # Speech audio channel handler
│   ├── sysaudio.rs     # System audio channel handler
│   ├── avinput.rs      # AV input channel handler
│   ├── input.rs        # Input channel handler
│   ├── sensor.rs       # Sensor channel handler
│   ├── navigation.rs   # Navigation channel handler
│   ├── mediastatus.rs  # Media status channel handler
│   ├── bluetooth.rs    # Bluetooth channel handler
│   ├── common.rs       # Shared utilities
│   ├── cert.rs         # Built-in TLS certificate
│   └── usb.rs          # USB transport (usb feature)
├── examples/
│   └── main/           # Full head unit example application
│       └── main.rs
├── protobuf/           # Protobuf definitions (Bluetooth.proto, Wifi.proto)
└── Cargo.toml
```

### Key types

| Type | Role |
|------|------|
| `AndroidAutoSetup` | Proof-of-initialisation token returned by `setup()`; must be passed to `run()` and related methods — ensures initialisation is never skipped |
| `AndroidAutoConfiguration` | Top-level configuration for the head unit (`unit: HeadUnitInfo`, optional custom certificate) |
| `HeadUnitInfo` | Static identity information sent to the phone during handshake |
| `BluetoothInformation` | Bluetooth adapter MAC address used for wireless negotiation |
| `NetworkInformation` | Wi-Fi network details relayed to the phone for the wireless session |
| `SensorInformation` | Set of sensor types the head unit reports to the phone |
| `VideoConfiguration` | Desired video resolution, FPS, and display DPI |
| `InputConfiguration` | Supported keycodes and optional touchscreen dimensions |
| `AudioChannelType` | Discriminates between `Media`, `System`, and `Speech` audio channels |
| `AndroidAutoMessage` | Enum of all message types that can be received over the link |
| `SendableAndroidAutoMessage` | Wire-ready message sent from the application back to the phone |
| `SendableChannelType` | Identifies which channel a `SendableAndroidAutoMessage` targets |
| `FrameHeaderType` | Whether a packet fits in a single frame or is fragmented (`Single`, `First`, `Middle`, `Last`) |

### Key traits

| Trait | Purpose |
|-------|---------|
| `AndroidAutoMainTrait` | Core trait — implement to handle connect/disconnect and provide the message sender; requires all channel traits below |
| `AndroidAutoVideoChannelTrait` | Receive and render H.264 video frames from the phone |
| `AndroidAutoAudioOutputTrait` | Receive and play audio for media, system, and speech channels |
| `AndroidAutoAudioInputTrait` | Capture and stream microphone audio to the phone |
| `AndroidAutoInputChannelTrait` | Handle touch and keycode input binding |
| `AndroidAutoSensorTrait` | Report sensor data (e.g. night mode, driving status) to the phone |
| `AndroidAutoNavigationTrait` | Receive turn-by-turn navigation events from the phone |
| `AndroidAutoWiredTrait` | Marker trait indicating the implementation supports USB connections (`usb` feature) |
| `AndroidAutoWirelessTrait` | Bluetooth + Wi-Fi negotiation for wireless connections (`wireless` feature) |
| `AndroidAutoBluetoothTrait` | Low-level Bluetooth adapter configuration |

### Dependencies

The library relies on:

- [`tokio`](https://tokio.rs) — async runtime
- [`rustls`](https://github.com/rustls/rustls) — TLS 1.2/1.3 for the secure channel
- [`aws-lc-rs`](https://crates.io/crates/aws-lc-rs) — cryptographic backend used by rustls
- [`protobuf`](https://github.com/stepancheg/rust-protobuf) — message encoding/decoding
- [`async-trait`](https://crates.io/crates/async-trait) — async trait support
- [`futures`](https://crates.io/crates/futures) — async combinators
- [`serde`](https://crates.io/crates/serde) — serialization for message types
- [`log`](https://crates.io/crates/log) — structured logging
- [`nusb`](https://crates.io/crates/nusb) *(optional, `usb` feature)* — USB device access
- [`bluetooth-rust`](https://crates.io/crates/bluetooth-rust) *(optional, `wireless` feature)* — Bluetooth RFCOMM for wireless handoff

---

## Contributing

Contributions are welcome! A few guidelines:

1. **Fork** the repository and create a feature branch.
2. **Run the tests** before submitting: `cargo test`
3. **Keep it async** — the library is built around Tokio; new I/O code should follow the same pattern.
4. **Protobuf changes** — if you modify `.proto` files under `protobuf/`, regenerate the Rust bindings with `protobuf-codegen` before committing.
5. Open a **pull request** with a clear description of what you changed and why.

If you find a bug or want to request a feature, please open an [issue](https://github.com/uglyoldbob/android-auto/issues).

---

## License

Licensed under the **GNU Lesser General Public License v3.0 or later**. See [LICENSE](LICENSE) for the full text.

This means you can use this library in your own applications — including proprietary or closed-source ones — without being required to release your application's source code. The library itself must remain LGPL.
