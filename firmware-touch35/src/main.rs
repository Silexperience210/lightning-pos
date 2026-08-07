//! LightningPoS firmware — Touch 3.5" variant (JC3248W535C)
//!
//! 6 flexible channels (relay/servo/ambient-light/sensor), QSPI 480×320 display,
//! capacitive touch, battery gauge, Mini-PoS mode, vending sensors.
//!
//! Build: `cargo build --release -p firmware-touch35 --target xtensa-esp32s3-none-elf`

#![no_std]
#![no_main]

use lightningpos_hal::BoardVariant;

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let _variant = BoardVariant::Touch35;
    // See lightningpos-firmware/src/main.rs for the full implementation pattern.
    // Touch 3.5" adds: QSPI display, AXS15231B touch, battery gauge,
    // Mini-PoS amount entry, 6 flex channels, 3 vending sensors.
    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(1)).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("PANIC: {}", info);
    loop {}
}
