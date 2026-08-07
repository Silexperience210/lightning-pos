//! LightningPoS firmware — ESP32-C3 variant (RISC-V)
//!
//! Single-core RISC-V, smallest footprint, lowest power.
//! 1 relay + 2 flex channels, native USB CDC, no display.
//! Ideal for cost-sensitive volume deployments.
//!
//! Build: `cargo build --release -p firmware-c3 --target riscv32imc-unknown-none-elf`

#![no_std]
#![no_main]

use lightningpos_hal::BoardVariant;

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let _variant = BoardVariant::Esp32C3;
    // Single relay (GPIO4), 2 flex channels (GPIO6/7), native USB CDC serial.
    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(1)).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("PANIC: {}", info);
    loop {}
}
