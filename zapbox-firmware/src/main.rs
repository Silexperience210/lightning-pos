//! LightningPoS Firmware — LilyGo T-Display-S3 (ESP32-S3)
//!
//! This is the main firmware binary for the T-Display-S3 variant.
//! It initializes all hardware, connects to WiFi/LNbits, and runs
//! the main event loop.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────┐
//! │                    Main Loop                       │
//! │  ┌──────────┐  ┌──────────┐  ┌────────────────┐  │
//! │  │ WiFi      │  │ WebSocket│  │ Event Queue    │  │
//! │  │ Manager   │  │ Client   │  │ (buttons, NFC  │  │
//! │  │           │  │          │  │  payments...)  │  │
//! │  └─────┬─────┘  └────┬─────┘  └───────┬────────┘  │
//! │        │             │                │           │
//! │        └─────────────┼────────────────┘           │
//! │                      │                            │
//! │               ┌──────┴──────┐                     │
//! │               │   State     │                     │
//! │               │   Machine   │                     │
//! │               └──────┬──────┘                     │
//! │                      │                            │
//! │        ┌─────────────┼─────────────┐              │
//! │   ┌────┴────┐   ┌────┴────┐   ┌────┴────┐        │
//! │   │ Display │   │ Relay   │   │ NFC     │        │
//! │   │ (ST7789)│   │ Control │   │ (PN532) │        │
//! │   └─────────┘   └─────────┘   └─────────┘        │
//! └───────────────────────────────────────────────────┘
//! ```

#![no_std]
#![no_main]

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::timer::timg::TimerGroup;
use log::{error, info, warn};

use lightningpos_core::events::{Event, EventQueue};
use lightningpos_core::state::{DeviceState, StateMachine, WifiState};
use lightningpos_core::config::DeviceConfig;
use lightningpos_hal::BoardVariant;
use lightningpos_net::wifi::WifiManager;
use lightningpos_net::websocket::WebSocketClient;
use lightningpos_net::api::HttpClient;

// ─── Global State (shared via embassy_sync) ────────────────────────

/// Static configuration loaded from NVS/FFat
static CONFIG: embassy_sync::blocking_mutex::Mutex<RefCell<Option<DeviceConfig>>> =
    embassy_sync::blocking_mutex::Mutex::new(RefCell::new(None));

/// Event queue capacity
const EVENT_QUEUE_CAPACITY: usize = 32;

// ─── Embassy Tasks ─────────────────────────────────────────────────

/// Main application entry point
#[embassy_executor::task]
async fn main_task(spawner: Spawner) {
    info!("╔══════════════════════════════════════════╗");
    info!("║       ⚡ LightningPoS Firmware v0.1.0         ║");
    info!("║  T-Display-S3 — Lightning Switch       ║");
    info!("╚══════════════════════════════════════════╝");

    // Initialize hardware
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // Initialize timers for embassy
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    info!("Hardware initialized — T-Display-S3 (4 channels, ST7789)");

    // Load configuration from NVS
    let config = load_configuration().unwrap_or_default();
    info!("Configuration loaded: WiFi SSID='{}'", config.wifi.ssid);

    // Store config globally
    {
        let mut guard = CONFIG.lock().await;
        *guard.borrow_mut() = Some(config.clone());
    }

    let variant = BoardVariant::TDisplayS3;

    // Create shared state
    let state_machine = embassy_sync::blocking_mutex::Mutex::new(RefCell::new(StateMachine::new()));
    let event_queue = embassy_sync::blocking_mutex::Mutex::new(RefCell::new(
        EventQueue::<EVENT_QUEUE_CAPACITY>::new(),
    ));
    let wifi_manager = embassy_sync::blocking_mutex::Mutex::new(RefCell::new(WifiManager::new()));
    let ws_client = embassy_sync::blocking_mutex::Mutex::new(RefCell::new(WebSocketClient::new()));
    let http_client = embassy_sync::blocking_mutex::Mutex::new(RefCell::new(HttpClient::new()));

    // Configure wifi
    {
        let mut guard = wifi_manager.lock().await;
        guard.borrow_mut().configure(
            config.wifi.ssid.as_str(),
            config.wifi.password.as_str(),
        );
    }

    // Configure HTTP client
    {
        let mut guard = http_client.lock().await;
        guard.borrow_mut().configure(
            config.wifi.lnbits_server.as_str(),
            config.extension_path.as_str(),
            config.wifi.device_id.as_str(),
        );
    }

    // Spawn subsystem tasks
    // In real firmware, each of these would be an embassy task:
    // - wifi_task: handles WiFi connect/disconnect/reconnect
    // - display_task: renders UI based on current state
    // - websocket_task: manages WS connection and parses messages
    // - nfc_task: polls PN532 for Bolt Card taps
    // - button_task: monitors button presses (BOOT, HELP)
    // - sensor_task: monitors vending machine sensors

    // ─── Main Event Loop ────────────────────────────────────────────

    let mut now = 0u64;
    let mut tick_interval = Duration::from_millis(50);

    loop {
        Timer::after(tick_interval).await;
        now = now.saturating_add(50);

        // Process events from the queue
        let events: heapless::Vec<Event, 16> = {
            let mut guard = event_queue.lock().await;
            let mut q = guard.borrow_mut();
            let mut v = heapless::Vec::new();
            while let Some(e) = q.pop() {
                let _ = v.push(e);
                if v.is_full() {
                    break;
                }
            }
            v
        };

        for event in events {
            process_event(
                &event,
                &state_machine,
                &wifi_manager,
                &ws_client,
                now,
            )
            .await;

            log_event(&event);
        }

        // Check WiFi status periodically
        if now % 5000 == 0 {
            let wifi_ok = {
                let guard = wifi_manager.lock().await;
                guard.borrow().is_connected()
            };

            if !wifi_ok {
                let mut guard = state_machine.lock().await;
                let _ = guard.borrow_mut().update_wifi(WifiState::Disconnected, now);
            }
        }

        // Check WebSocket health (ping/pong)
        if now % 10000 == 0 {
            let mut guard = ws_client.lock().await;
            let ws = guard.borrow_mut();

            if ws.is_connected() && ws.should_ping(now) {
                ws.ping_sent(now);
                info!("WebSocket ping sent");
            }

            if ws.is_connected() && ws.is_half_open(now) {
                if ws.on_ping_timeout() {
                    warn!("WebSocket half-open — reconnecting");
                    ws.on_disconnected();
                }
            }
        }

        // Check state transitions
        {
            let guard = state_machine.lock().await;
            let sm = guard.borrow();
            info!(
                "State: {} (WiFi: {:?}, uptime: {}s)",
                sm.current().name(),
                sm.wifi_state(),
                now / 1000
            );
        }

        // Heartbeat LED pattern
        // Toggle ready LED based on state
    }
}

// ─── Event Processing ──────────────────────────────────────────────

async fn process_event(
    event: &Event,
    state_machine: &embassy_sync::blocking_mutex::Mutex<RefCell<StateMachine>>,
    wifi_manager: &embassy_sync::blocking_mutex::Mutex<RefCell<WifiManager>>,
    ws_client: &embassy_sync::blocking_mutex::Mutex<RefCell<WebSocketClient>>,
    now: u64,
) {
    match event {
        Event::WifiConnected => {
            let mut guard = wifi_manager.lock().await;
            guard.borrow_mut().on_connected(-45); // RSSI placeholder

            let mut sm_guard = state_machine.lock().await;
            let _ = sm_guard.borrow_mut().update_wifi(WifiState::Connected, now);
        }

        Event::WifiDisconnected => {
            let mut guard = wifi_manager.lock().await;
            guard.borrow_mut().on_disconnected();

            let mut sm_guard = state_machine.lock().await;
            let _ = sm_guard.borrow_mut().update_wifi(WifiState::Disconnected, now);
        }

        Event::WebSocketConnected => {
            let mut guard = ws_client.lock().await;
            guard.borrow_mut().on_connected(now);
        }

        Event::PaymentReceived {
            pin: _,
            amount_sats,
            payment_hash: _,
        } => {
            let amount = amount_sats.unwrap_or(0);
            info!("⚡ Payment received: {} sats — activating relay", amount);

            // Transition to READY (payment completed)
            {
                let mut guard = state_machine.lock().await;
                let _ = guard.borrow_mut().transition(DeviceState::Ready, now);
            }

            // Relay activation would be handled by the relay task:
            // - Set pin HIGH for configured duration
            // - Apply special mode (blink/pulse) if configured
        }

        Event::NfcBoltCardTapped { lnurlw } => {
            info!("NFC Bolt Card tapped: {}", &lnurlw.as_str()[..lnurlw.len().min(60)]);

            // Determine active pin based on current product selection
            let active_pin: u8 = 12; // Default CH01

            let ws_json = WebSocketClient::build_lnurlw_event(lnurlw.as_str(), active_pin);
            info!("Sending LNURLW event: {}", ws_json);
            // In real firmware: ws_client.send(ws_json)
        }

        Event::ButtonLongPress(btn) => {
            if *btn == 0 {
                info!("BOOT button long-press → Config Mode");
                {
                    let mut guard = state_machine.lock().await;
                    let _ = guard.borrow_mut().transition(DeviceState::ConfigMode, now);
                }
            }
        }

        Event::ScreensaverTimeout => {
            let mut guard = state_machine.lock().await;
            let sm = guard.borrow();
            if sm.current() == DeviceState::Ready || sm.current() == DeviceState::BtcTicker {
                drop(sm);
                let _ = guard.borrow_mut().transition(DeviceState::Screensaver, now);
            }
        }

        Event::Error { critical, message } => {
            if *critical {
                error!("CRITICAL: {}", message);
                let mut guard = state_machine.lock().await;
                let _ = guard.borrow_mut().transition(DeviceState::ErrorCritical, now);
            } else {
                warn!("Error: {}", message);
                let mut guard = state_machine.lock().await;
                let _ = guard.borrow_mut().transition(DeviceState::ErrorRecoverable, now);
            }
        }

        _ => {
            // Other events are handled by their respective subsystems
            debug_log_event(event);
        }
    }
}

// ─── Helpers ───────────────────────────────────────────────────────

fn log_event(event: &Event) {
    match event.severity() {
        lightningpos_core::events::Severity::Error => error!("Event: {}", event.name()),
        lightningpos_core::events::Severity::Warn => warn!("Event: {}", event.name()),
        _ => info!("Event: {}", event.name()),
    }
}

fn debug_log_event(event: &Event) {
    info!("Event (debug): {}", event.name());
}

/// Load configuration from NVS (Non-Volatile Storage)
fn load_configuration() -> Option<DeviceConfig> {
    // On real hardware, this would:
    // 1. Open NVS partition "lightningpos"
    // 2. Read stored JSON config
    // 3. Parse with serde_json
    // 4. Parse switch URL into server/device_id
    //
    // For now, return a default config for development
    let mut config = DeviceConfig::default();

    // Example: hardcoded dev config (replace with NVS read)
    let _ = config.wifi.ssid.push_str("MyWiFi");
    let _ = config.wifi.password.push_str("password123");
    let _ = config.wifi.switch_url.push_str(
        "wss://lnbits.example.com/bitcoinswitch/api/v1/ws/abc123def456ghi789jkl0"
    );

    // Parse the switch URL
    config.parse_urls();

    Some(config)
}

/// Panic handler — logs the panic info and restarts
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    error!("FIRMWARE PANIC: {}", info);

    // Try to log to serial before restarting
    // On real hardware: esp_hal::reset::software_reset()
    loop {
        core::hint::spin_loop();
    }
}
