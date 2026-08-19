#!/bin/bash
# Ré-applique le patch du linker esp-hal 0.23.1 (placement de .flash.appdesc).
#
# POURQUOI : le bootloader ESP-IDF 5.x lit l'app descriptor à un OFFSET FIXE
# (0x20) depuis le début de l'image app. esp-hal 0.23 ne fournit PAS le
# placement de la section .flash.appdesc ; sans patch, elle atterrit en fin
# de .rodata → bootloader lit du mauvais contenu → "Image requires efuse blk
# rev >= v136.66".
#
# Le patch ajoute .flash.appdesc dans le bloc INSERT de rodata_dummy.x, ce qui
# la place à 0x3C010020 (début du segment DROM = offset 0x20 de l'image).
#
# Le patch modifie le REGISTRY cargo (~/.cargo/registry/.../esp-hal-0.23.1/ld/).
# À ré-appliquer après `cargo update`, suppression du registry, ou nouvelle
# machine. Une copie propre (non hack) serait un [patch.crates-io] local.
#
# Après application : supprimer les OUT_DIR d'esp-hal pour forcer la re-copie :
#   rm -rf target/xtensa-esp32s3-none-elf/release/build/esp-hal-*
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REG="$(ls -d ~/.cargo/registry/src/index.crates.io-*/esp-hal-0.23.1 2>/dev/null | head -1)"

if [ -z "$REG" ]; then
  echo "esp-hal-0.23.1 introuvable dans le registry cargo" >&2
  exit 1
fi

cp "$SCRIPT_DIR/sections/rodata.x" "$REG/ld/sections/rodata.x"
cp "$SCRIPT_DIR/sections/fixups/rodata_dummy.x" "$REG/ld/sections/fixups/rodata_dummy.x"
echo "Patch appliqué sur: $REG"
echo "N'oubliez pas: rm -rf target/xtensa-esp32s3-none-elf/release/build/esp-hal-*"
