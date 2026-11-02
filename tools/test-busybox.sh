#!/usr/bin/env bash
# test-busybox.sh — lance le noyau XILI avec busybox depuis l'initrd.
#
# Évite de modifier la CI : échange temporairement main.rs ↔ main-busybox.rs,
# compile, boote QEMU, vérifie les marqueurs busybox, puis restaure main.rs.
#
# Usage : ./tools/test-busybox.sh [uefi|esp]
#   défaut = BIOS
#   uefi   = ISO UEFI
#   esp    = disque ESP

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
KERNEL_DIR="$PROJECT_ROOT/kernel/src"
MAIN="$KERNEL_DIR/main.rs"
MAIN_BB="$PROJECT_ROOT/tools/main-busybox.rs"
INITRD="$PROJECT_ROOT/assets/initrd.cpio"
LOG="/tmp/xili-busybox.log"
UEFI_LOG="/tmp/xili-busybox-uefi.log"

MODE="${1:-bios}"
if [[ "$MODE" != "bios" && "$MODE" != "uefi" && "$MODE" != "esp" ]]; then
    echo "Usage: $0 [bios|uefi|esp]"
    exit 1
fi

if [[ ! -f "$MAIN_BB" ]]; then
    echo "ERREUR: $MAIN_BB introuvable"
    exit 1
fi

if [[ ! -f "$INITRD" ]]; then
    echo "ERREUR: $INITRD introuvable — lancer d'abord mk-initrd.py / make initrd"
    exit 1
fi

echo "=== Test busybox (mode: $MODE) ==="

# 1) Échange main.rs ↔ main-busybox.rs
echo "[1/4] Échange du point d'entrée..."
cp "$MAIN" "${MAIN}.bak"
cp "$MAIN_BB" "$MAIN"

# 2) Compile
echo "[2/4] Compilation..."
cd "$PROJECT_ROOT"
make iso

# 3) Boot QEMU + capture log
echo "[3/4] Boot QEMU + capture log..."
if [[ "$MODE" == "bios" ]]; then
    timeout 20 qemu-system-x86_64 -cdrom "$PROJECT_ROOT/xili.iso" \
        -m 512M -nographic -no-reboot > "$LOG" 2>&1 || true
elif [[ "$MODE" == "uefi" ]]; then
    timeout 30 qemu-system-x86_64 -machine q35 \
        -drive if=pflash,format=raw,unit=0,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
        -drive if=pflash,format=raw,unit=1,file="$PROJECT_ROOT/ovmf-vars.fd" \
        -cdrom "$PROJECT_ROOT/xili-uefi.iso" \
        -m 512M -nographic -no-reboot > "$LOG" 2>&1 || true
else
    timeout 30 qemu-system-x86_64 -machine q35 \
        -drive if=pflash,format=raw,unit=0,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
        -drive if=pflash,format=raw,unit=1,file="$PROJECT_ROOT/ovmf-vars.fd" \
        -drive format=raw,file="$PROJECT_ROOT/xili-esp.img" \
        -m 512M -nographic -no-reboot > "$LOG" 2>&1 || true
fi

# 4) Vérifie les marqueurs busybox
echo "[4/4] Vérification des marqueurs..."
FAIL=0

check_marker() {
    if grep -q "$1" "$LOG"; then
        echo "  ✓ $2"
    else
        echo "  ✗ $2"
        FAIL=$((FAIL + 1))
    fi
}

check_marker "J3.5: cpio initrd openat/read" "initrd J3.5"
check_marker "Hello from cpio initrd!" "fichier hello.txt"

# Marqueurs J1..J3.2 toujours attendus
check_marker "J1: noyau charge" "J1 console"
check_marker "J2.3: tables de pages" "J2.3 paging"
check_marker "higher-half : OK" "J2.3b higher-half"
check_marker "J2.4: allocateur physique bitmap - OK" "J2.4 bitmap"
check_marker "J2.5c: allocateur noyau slab" "J2.5c slab"
check_marker "J2.6: PIC remappe" "J2.6 PIC"
check_marker "J2.7: planificateur priorite" "J2.7 scheduler"
check_marker "J2.8: scheduler priorites + GPR" "J2.8 GPR"
check_marker "J3.2: syscall/sysret" "J3.2 syscall"
check_marker "J3.3: ELF charge OK" "J3.3 ELF"
check_marker "J3.4: multi-syscall ELF" "J3.4 multi-syscall"

# 5) Affiche le log pour inspection
echo ""
echo "=== LOG SERIE (dernières lignes) ==="
tail -n 40 "$LOG"

# 6) Restaure main.rs
echo ""
echo "=== Restauration de main.rs ==="
cp "${MAIN}.bak" "$MAIN"
rm -f "${MAIN}.bak"

if [[ $FAIL -eq 0 ]]; then
    echo ""
    echo "TEST BUSYBOX OK — $MODE"
    exit 0
else
    echo ""
    echo "TEST BUSYBOX ECHEC — $FAIL marqueur(s) manquant(s)"
    exit 1
fi
