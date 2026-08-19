//! LightningPoS POS — firmware Touch 3.5" (JC3248W535C, ESP32-S3)
//!
//! Milestone 1 : boot + allocateur + clignotement du rétroéclairage (GPIO 1).
//! Valide la chaîne complète : toolchain → build → flash → série → GPIO.
//!
//! Build:
//!   cargo +esp build --release -p firmware-touch35 --target xtensa-esp32s3-none-elf
//! Flash:
//!   espflash flash --port /dev/ttyACM0 target/xtensa-esp32s3-none-elf/release/firmware-touch35

#![no_std]
#![no_main]

extern crate alloc;

use core::mem::MaybeUninit;

use esp_alloc as _;
use esp_hal::gpio::{Level, Output};
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;

/// ESP-IDF App Descriptor — lu par le bootloader à l'offset 0x20 de l'image.
/// Le placement au DÉBUT du segment DROM est assuré par build.rs (INSERT BEFORE .rodata).
#[repr(C)]
struct EspAppDesc {
    magic_word: u32,
    secure_version: u32,
    reserv1: [u32; 2],
    version: [u8; 32],
    project_name: [u8; 32],
    time: [u8; 16],
    date: [u8; 16],
    idf_ver: [u8; 32],
    app_elf_sha256: [u8; 32],
    min_efuse_blk_rev_full: u16,
    max_efuse_blk_rev_full: u16,
    mmu_page_size: u8,
    reserv3: [u8; 3],
    reserv2: [u32; 18],
}

#[allow(non_upper_case_globals)]
#[no_mangle]
#[used]
#[link_section = ".flash.appdesc"]
static esp_app_desc: EspAppDesc = EspAppDesc {
    magic_word: 0xABCD5432,
    secure_version: 0,
    reserv1: [0; 2],
    version: [0; 32],
    project_name: [0; 32],
    time: [0; 16],
    date: [0; 16],
    idf_ver: [0; 32],
    app_elf_sha256: [0; 32],
    min_efuse_blk_rev_full: 0,
    max_efuse_blk_rev_full: 0,
    mmu_page_size: 16, // log2(64 KiB) — évite l'inférence d'alignement d'espflash
    reserv3: [0; 3],
    reserv2: [0; 18],
};

/// Initialise le tas (requis pour String/Vec/format! en no_std).
fn init_heap() {
    const HEAP_SIZE: usize = 64 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            HEAP.as_mut_ptr() as *mut u8,
            HEAP_SIZE,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }
}

/// Ajoute la PSRAM (8 Mo) au tas — le framebuffer 320×480 (300 Ko) y vivra.
fn init_psram_heap(peripherals: &esp_hal::peripherals::Peripherals) {
    let (addr, size) = esp_hal::psram::psram_raw_parts(&peripherals.PSRAM);

    // TEST accès brut : round-trip lecture/écriture au début de la PSRAM.
    unsafe {
        let p = addr as *mut u32;
        println!("[PSRAM] vaddr={:08x}", addr as usize);
        let before = p.read_volatile();
        p.write_volatile(0x1234_5678);
        let after = p.read_volatile();
        println!("[PSRAM] test brut: avant={:08x} écrit=12345678 relu={:08x}", before, after);
    }

    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            addr,
            size,
            esp_alloc::MemoryCapability::External.into(),
        ));
    }
    println!("[HEAP] PSRAM: {} Ko", size / 1024);

    // TEST allocation : petit bloc puis le grand framebuffer.
    let mut probe: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(1024);
    probe.resize(1024, 0xA5);
    println!("[HEAP] allocation 1 Ko OK (ptr={:08x})", probe.as_ptr() as usize);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {:?}", info);
    loop {}
}

/// Couleur de test → nom (RGB565)
const TEST_COLORS: [(u16, &str); 6] = [
    (0xF800, "rouge"),
    (0x07E0, "vert"),
    (0x001F, "bleu"),
    (0xFFFF, "blanc"),
    (0xFFE0, "jaune"),
    (0x0000, "noir"),
];

#[esp_hal_embassy::main]
async fn main(_spawner: embassy_executor::Spawner) {
    println!("[BOOT] avant esp_hal::init");
    // PSRAM 8 Mo explicite (N16R8), CPU 240 MHz, PSRAM 80 MHz.
    let mut config = esp_hal::Config::default();
    config.cpu_clock = esp_hal::clock::CpuClock::_240MHz;
    config.psram.size = esp_hal::psram::PsramSize::Size(8 * 1024 * 1024);
    config.psram.ram_frequency = esp_hal::psram::SpiRamFreq::Freq80m;
    let peripherals = esp_hal::init(config);
    println!("[BOOT] esp_hal::init OK");
    init_heap();
    init_psram_heap(&peripherals);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    println!("=== JC3248W535C POS — boot OK ===");

    // Rétroéclairage (GPIO 1)
    let mut backlight = Output::new(peripherals.GPIO1, Level::High);

    // DMA + écran AXS15231B : SCK=47, D0=21, D1=48, D2=40, D3=39, CS=45, SPI2
    let dma_channel = unsafe { esp_hal::dma::DmaChannel0::steal() };
    let mut display = lightningpos_display::axs15231b::Axs15231b::new(
        peripherals.GPIO47, // sck
        peripherals.GPIO21, // sio0 = D0
        peripherals.GPIO48, // sio1 = D1
        peripherals.GPIO40, // sio2 = D2
        peripherals.GPIO39, // sio3 = D3
        peripherals.GPIO45, // cs
        dma_channel,
        peripherals.SPI2,
    );

    println!("[DISPLAY] init…");
    display.init();
    println!("[DISPLAY] init OK");

    // DIAGNOSTIC : lire l'ID du panneau (RDID1=0xDA → 0x40 attendu pour AXS15231B).
    let mut id = [0u8; 1];
    display.read_cmd(0xDA, &mut id);
    println!("[DISPLAY] RDID1(0xDA) = 0x{:02X} (attendu 0x40)", id[0]);
    // Lire le registre 0xA0 (17 octets) pour vérifier que l'écriture a pris.
    let mut a0 = [0u8; 17];
    display.read_cmd(0xA0, &mut a0);
    println!("[DISPLAY] relu 0xA0 = {:02X?}", a0);

    // Framebuffer PSRAM 320×480. FLUSH PLEINE FRAME, CS MAINTENU BAS pendant
    // tout le stream (comme le bus Arduino du BlockClock) — les transactions
    // séparées font redémarrer le pointeur d'écriture du panneau.
    let mut framebuffer: alloc::vec::Vec<u16> = alloc::vec::Vec::with_capacity(320 * 480);
    framebuffer.resize(320 * 480, 0u16);
    println!("[DISPLAY] framebuffer PSRAM: {} mots", framebuffer.len());

    // TEST DIAGNOSTIC : 8 barres de couleurs primaires RGB565 (ordre naturel).
    const COLORS: [u16; 8] = [
        0x0000, // 0 noir
        0x001F, // 1 bleu
        0x07E0, // 2 vert
        0x07FF, // 3 cyan
        0xF800, // 4 rouge
        0xF81F, // 5 magenta
        0xFFE0, // 6 jaune
        0xFFFF, // 7 blanc
    ];
    println!("[DISPLAY] diagnostic: 8 barres, puis aplats rouge/vert/bleu");
    loop {
        // --- Frame 1 : 8 barres de 60 lignes ---------------------------------
        // Ce qu'il faut OBSERVER et rapporter :
        //  * 8 bandes distinctes sur les 480 lignes → le panneau consomme bien
        //    2 octets/pixel ; s'il reste gris, c'est cr_qspis_datamode (0xA0).
        //  * 4 bandes seulement, ou des bandes 2× trop hautes → le panneau
        //    consomme 1 octet/pixel = mode gris 8 bpp confirmé.
        for y in 0..480usize {
            let color = COLORS[y / 60];
            for x in 0..320usize {
                // swap_bytes : équivalent exact de MSB_32_16_16_SET côté
                // Arduino (octet fort d'abord sur le fil).
                framebuffer[y * 320 + x] = color.swap_bytes();
            }
        }
        display.flush_full(&framebuffer);
        println!("[DISPLAY] barres OK — compter les bandes");
        embassy_time::Timer::after(embassy_time::Duration::from_millis(3000)).await;

        // --- Frames 2..4 : aplats plein écran --------------------------------
        // Test insensible à la géométrie : si rouge, vert et bleu s'affichent
        // comme trois GRIS différents, le panneau décode bien le flux mais le
        // mappe en luminance → format pixel, pas transport.
        for &(color, name) in &[(0xF800u16, "rouge"), (0x07E0, "vert"), (0x001F, "bleu")] {
            framebuffer.fill(color.swap_bytes());
            display.flush_full(&framebuffer);
            println!("[DISPLAY] aplat {} (0x{:04X})", name, color);
            embassy_time::Timer::after(embassy_time::Duration::from_millis(2000)).await;
        }
    }
}
