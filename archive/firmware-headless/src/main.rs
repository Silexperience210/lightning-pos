//! LightningPoS firmware — Headless ESP32 variant
//!
//! 12 relay channels, no display, LED diagnostics, 2 vending sensors.
//! The workhorse variant for embedded/hidden installations.
//!
//! Build: `cargo build --release -p firmware-headless --target xtensa-esp32-none-elf`

#![no_std]
#![no_main]

use lightningpos_hal::BoardVariant;

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let _variant = BoardVariant::Headless;
    // 12 relay channels, LED diagnostics via blinks, 2 vending sensors.
    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(1)).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("PANIC: {}", info);
    loop {}
}
