#!/bin/bash
# Build all LightningPoS firmware variants
set -euo pipefail

echo "⚡ LightningPoS RS — Building all variants"
echo "===================================="

# Source ESP toolchain
if [ -f ~/export-esp.sh ]; then
    source ~/export-esp.sh
fi

variants=(
    "lightningpos-firmware:T-Display-S3:xtensa-esp32s3-none-elf"
    "firmware-touch35:Touch 3.5\":xtensa-esp32s3-none-elf"
    "firmware-headless:Headless:xtensa-esp32-none-elf"
    "firmware-c3:ESP32-C3:riscv32imc-unknown-none-elf"
)

for variant in "${variants[@]}"; do
    IFS=':' read -r package name target <<< "$variant"
    echo ""
    echo "📦 Building $name ($package)..."
    cargo build --release -p "$package" --target "$target" || {
        echo "❌ $name build failed!"
        exit 1
    }
    echo "✅ $name built successfully"
done

echo ""
echo "🎉 All variants built!"
