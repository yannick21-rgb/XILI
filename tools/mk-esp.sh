#!/usr/bin/env bash
# Construit une image disque contenant une ESP (partition FAT32 type 0xEF)
# bootable en UEFI : XILI démarre sous OVMF, sans BIOS.
#
# Trois pièges, tous rencontrés et documentés ici :
#
#  1. Ne jamais lancer `mformat` sur le disque entier. mtools écrit le boot
#     sector FAT au secteur 0 et écrase la table de partitions au passage :
#     l'entrée 0xEF devient 0x0C avec un LBA de départ à 0, et OVMF répond
#     « Not Found » puis part en PXE. Il faut formater l'image de la partition
#     seule, puis la recopier à l'offset de la partition.
#
#  2. `grub-mkstandalone` doit recevoir `--modules=part_msdos`. Sans lui, GRUB
#     ne parse pas la table de partitions : `ls` n'affiche que
#     (memdisk) (hd0) (cd0), `hd0` est vu comme « No known filesystem detected »
#     et `hd0,msdos1` est inconnu.
#
#  3. La configuration doit être passée en chemin RELATIF (`boot/grub/grub.cfg`).
#     En chemin absolu, grub-mkstandalone l'embarque au mauvais endroit du
#     memdisk : GRUB ne trouve pas sa config et retombe sur son shell.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$ROOT/xili-esp.img}"
SIZE_MB="${ESP_SIZE_MB:-64}"
PART_START=2048
PART_SECTORS=$(( SIZE_MB * 1024 * 1024 / 512 - PART_START ))

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "ESP UEFI — disque ${SIZE_MB} Mio, partition au secteur ${PART_START}"

# 1. Table de partitions MBR : une seule partition FAT, type 0xEF (ESP).
dd if=/dev/zero of="$WORK/disk.img" bs=1M count="$SIZE_MB" status=none
sfdisk "$WORK/disk.img" >/dev/null 2>&1 <<EOF
label: dos
unit: sectors
start=${PART_START}, size=${PART_SECTORS}, type=EF
EOF

# 2. La partition, formatée seule.
dd if=/dev/zero of="$WORK/part.img" bs=512 count="$PART_SECTORS" status=none
mformat -i "$WORK/part.img" -F -v XILI :: >/dev/null

# 3. GRUB standalone UEFI, avec le module de partitionnement et un chemin relatif.
mkdir -p "$WORK/esp/EFI/BOOT" "$WORK/esp/boot/grub"
cp "$ROOT/boot/grub-uefi.cfg" "$WORK/esp/boot/grub/grub.cfg"
cp "$ROOT/target/x86_64-unknown-linux-gnu/release/kernel" "$WORK/esp/boot/kernel"
( cd "$WORK/esp" && grub-mkstandalone -O x86_64-efi --modules=part_msdos \
    -o "$WORK/BOOTX64.EFI" boot/grub/grub.cfg ) >/dev/null 2>&1

# 4. Remplir la partition.
mmd -i "$WORK/part.img" ::EFI ::EFI/BOOT ::boot ::boot/grub
mcopy -i "$WORK/part.img" "$WORK/BOOTX64.EFI" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$WORK/part.img" "$WORK/esp/boot/grub/grub.cfg" ::/boot/grub/grub.cfg
mcopy -i "$WORK/part.img" "$WORK/esp/boot/kernel" ::/boot/kernel

# 5. Injecter la partition dans le disque, à son offset.
dd if="$WORK/part.img" of="$WORK/disk.img" bs=512 seek="$PART_START" \
    conv=notrunc status=none

cp "$WORK/disk.img" "$OUT"
echo "ESP UEFI prete : $OUT ($(du -h "$OUT" | cut -f1))"
