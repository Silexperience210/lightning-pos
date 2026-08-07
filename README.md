# ⚡ LightningPoS RS — Bitcoin Lightning Switch (Rust + Embassy)

[![CI](https://github.com/Silexperience210/lightningpos-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Silexperience210/lightningpos-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**A Bitcoin Lightning-controlled switching unit, rewritten in Rust.**

Pay a Lightning invoice — a relay switches. That's the core idea. Built for vending machines, tip boxes, kiosk terminals, coffee machines, LED signs, and servo dispensers.

> Forked and rewritten from [AxelHamburch/LightningPoS](https://github.com/AxelHamburch/LightningPoS) (C++/Arduino) with Rust's safety guarantees.

---

## ✨ Why Rust?

| Aspect | Original (C++/Arduino) | LightningPoS RS (Rust) |
|--------|------------------------|-------------------|
| **Memory Safety** | Manual `new`/`delete`, buffer overflows | Ownership system, no UB |
| **Concurrency** | FreeRTOS mutexes, volatile globals | `Send`/`Sync` traits, async/await |
| **Error Handling** | Return codes, `Serial.println()` | `Result<T, E>`, `thiserror` |
| **Testing** | No unit tests | `#[test]` modules throughout |
| **Dependencies** | Manual `lib/` folder, PlatformIO | Cargo, `Cargo.lock`, semver |
| **Build System** | PlatformIO (Python) | Cargo, `cfg` features for variants |
| **Async Runtime** | FreeRTOS tasks, manual scheduling | Embassy (async/await) |
| **CI/CD** | Manual builds per variant | GitHub Actions, all 4 variants |

---

## 🏗️ Architecture

```text
lightningpos-rs/
├── lightningpos-core/          # Platform-agnostic business logic
│   ├── state.rs          # State machine (13 states, validated transitions)
│   ├── config.rs         # Typed configuration (serde)
│   ├── payment.rs        # LNURL/bech32 encoding
│   ├── protocol.rs       # LNbits WebSocket/API types
│   ├── events.rs         # Event bus (typed, queue)
│   └── error.rs          # Error types (thiserror)
│
├── lightningpos-hal/           # Hardware Abstraction Layer
│   ├── pins.rs           # GPIO assignments (4 variants)
│   ├── variant.rs        # BoardVariant enum
│   └── relay.rs          # Relay control with queue
│
├── lightningpos-net/           # Network stack (async)
│   ├── wifi.rs           # WiFi manager (reconnect, backoff)
│   ├── websocket.rs      # LNbits WS client (ping/pong, payments)
│   └── api.rs            # HTTP client (labels, BTC price)
│
├── lightningpos-display/       # Display rendering
├── lightningpos-nfc/           # NFC (Bolt Card, NT3H2111)
│
├── lightningpos-firmware/      # T-Display-S3 binary
├── firmware-touch35/     # Touch 3.5" binary
├── firmware-headless/    # Headless binary
└── firmware-c3/          # ESP32-C3 binary
```

---

## 🚀 Quick Start

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install ESP32 toolchain
cargo install espup
espup install

# Add target
rustup target add xtensa-esp32s3-none-elf
```

### Build

```bash
# T-Display-S3 (default)
cargo build --release -p lightningpos-firmware

# Touch 3.5"
cargo build --release -p firmware-touch35

# Headless ESP32
cargo build --release -p firmware-headless

# ESP32-C3
cargo build --release -p firmware-c3
```

### Flash

```bash
# T-Display-S3
espflash flash target/xtensa-esp32s3-none-elf/release/lightningpos-firmware

# Touch 3.5"
espflash flash target/xtensa-esp32s3-none-elf/release/firmware-touch35
```

### Test

```bash
cargo test -p lightningpos-core
cargo test -p lightningpos-net
cargo test -p lightningpos-hal
```

---

## 🎯 Hardware Variants

| Variant | Chip | Display | Channels | Flash | Firmware Suffix |
|---------|------|---------|----------|-------|----------------|
| T-Display-S3 | ESP32-S3 | ST7789 170×320 | 4 | 16MB | (none) |
| Touch 3.5" | ESP32-S3 | QSPI 480×320 | 6 | 16MB | `t` |
| Headless ESP32 | ESP32 | None | 12 | 4MB | `h` |
| ESP32-C3 | ESP32-C3 R5 | None | 1 (+2 fx) | 4MB | `c` |

---

## 🔄 How It Works

1. **QR code** — the display shows a QR code containing the LNURL
2. **Lightning payment** — customer scans and pays (or taps NFC Bolt Card)
3. **WebSocket trigger** — LNbits pushes payment confirmation to ESP32
4. **Relay switches** — output activated for configured duration
5. **Confirmation** — display shows payment received

The LightningPoS never handles funds itself. It only listens for confirmation from **your** LNbits server.

---

## 🧪 Running Tests

```bash
# All platform-agnostic tests
cargo test --workspace --exclude lightningpos-firmware --exclude firmware-touch35 --exclude firmware-headless --exclude firmware-c3

# Specific module
cargo test -p lightningpos-core -- state
cargo test -p lightningpos-net -- websocket
cargo test -p lightningpos-hal -- relay
```

---

## 📦 Dependencies

- **esp-hal** — ESP32 hardware abstraction
- **embassy** — Async runtime for embedded
- **esp-wifi** — WiFi + TCP/IP stack
- **serde / serde_json** — Configuration & protocol parsing
- **bech32** — LNURL encoding
- **qrcodegen** — QR code generation
- **heapless** — Stack-allocated collections (no `std`)

---

## 🔒 Security

- All network communication uses **WSS** (WebSocket over TLS)
- Device ID + API key never exposed in logs
- Config stored in encrypted NVS partition
- No hardcoded credentials — all config via serial/web installer
- Memory-safe Rust: no buffer overflows, no use-after-free

---

## 📄 License

MIT — see [LICENSE](LICENSE)

Built on the shoulders of giants: [SATOFFEE](https://github.com/danielcharrua/satoffee), [bitcoinswitch](https://github.com/lnbits/bitcoinswitch), and the LNbits team.

---

## 🙏 Acknowledgements

- **Axel Hamburch** — Original LightningPoS firmware (C++/PlatformIO)
- **Ben Arc** + LNbits team — Lightning infrastructure
- **Daniel Charrua** — SATOFFEE (the project LightningPoS was forked from)
- **esp-rs** community — Rust on ESP32 tooling
