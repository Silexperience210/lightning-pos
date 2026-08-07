#!/bin/bash
# Flash a LightningPoS firmware variant
set -euo pipefail

VARIANT="${1:-tdisplay-s3}"
PORT="${2:-/dev/ttyUSB0}"

case "$VARIANT" in
    tdisplay-s3)
        BINARY="target/xtensa-esp32s3-none-elf/release/lightningpos-firmware"
        ;;
    touch35)
        BINARY="target/xtensa-esp32s3-none-elf/release/firmware-touch35"
        ;;
    headless)
        BINARY="target/xtensa-esp32-none-elf/release/firmware-headless"
        ;;
    c3)
        BINARY="target/riscv32imc-unknown-none-elf/release/firmware-c3"
        ;;
    *)
        echo "Unknown variant: $VARIANT"
        echo "Usage: $0 [tdisplay-s3|touch35|headless|c3] [port]"
        exit 1
        ;;
esac

echo "⚡ Flashing $VARIANT to $PORT..."
espflash flash --port "$PORT" "$BINARY"
