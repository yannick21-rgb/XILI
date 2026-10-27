TARGET   := x86_64-unknown-linux-gnu
KERNEL   := target/$(TARGET)/release/kernel
ISO      := xili.iso
UEFI_ISO := xili-uefi.iso
ESP_IMG  := xili-esp.img
LOG      := /tmp/xili-serial.log
UEFI_LOG := /tmp/xili-uefi-serial.log

OVMF_CODE := /usr/share/OVMF/OVMF_CODE_4M.fd
OVMF_VARS := /usr/share/OVMF/OVMF_VARS_4M.fd

# Le noyau est un binaire freestanding : pas de crt, pas de libc,
# pas de PIE, et un script de link dédié.
# code-model=kernel : le noyau est lié en higher-half (> 4 GiB). Le modèle
# `static` n'émet que des R_X86_64_32 qui débordent au-dessus de 4 GiB (pour
# les casts `as u64` et les références format_args! → #PF). Le code-model
# `kernel` émet des R_X86_64_64 / RIP-relatifs qui tiennent sur 64 bits.
RUSTFLAGS := -C no-redzone=yes \
             -C relocation-model=static \
             -C code-model=kernel \
             -C link-arg=-nostdlib \
             -C link-arg=-static \
             -C link-arg=-no-pie \
             -C link-arg=-Wl,--build-id=none \
             -C link-arg=-T$(CURDIR)/kernel/linker.ld

# QEMU avec le firmware UEFI (OVMF) : le BIOS est remplacé par OVMF.
# ovmf-vars.fd est une copie modifiable du VARS fourni en lecture seule.
QEMU_UEFI := qemu-system-x86_64 -machine q35 \
             -drive if=pflash,format=raw,unit=0,readonly=on,file=$(OVMF_CODE) \
             -drive if=pflash,format=raw,unit=1,file=$(CURDIR)/ovmf-vars.fd

.PHONY: all build iso iso-uefi esp run run-headless run-uefi run-esp test test-uefi clean

all: iso

build:
	RUSTFLAGS="$(RUSTFLAGS)" cargo build --release

# BIOS : GRUB chargé depuis le MBR de l'ISO.
iso: build
	rm -rf iso
	mkdir -p iso/boot/grub
	cp $(KERNEL) iso/boot/kernel
	cp boot/grub.cfg iso/boot/grub/grub.cfg
	grub-mkrescue -o $(ISO) iso

# UEFI : même noyau, GRUB lancé par le firmware. L'entrée El Torito EFI de
# l'ISO pointe vers une petite image FAT que grub-mkrescue sait construire.
iso-uefi: build
	rm -rf iso-uefi
	mkdir -p iso-uefi/boot/grub
	cp $(KERNEL) iso-uefi/boot/kernel
	cp boot/grub-uefi.cfg iso-uefi/boot/grub/grub.cfg
	grub-mkrescue -o $(UEFI_ISO) iso-uefi

# UEFI sur disque : une vraie ESP (partition FAT32 type 0xEF).
esp: build
	tools/mk-esp.sh $(ESP_IMG)

run: iso
	qemu-system-x86_64 -cdrom $(ISO) -m 512M -serial stdio

run-headless: iso
	qemu-system-x86_64 -cdrom $(ISO) -m 512M -nographic -no-reboot

run-uefi: iso-uefi
	@cp $(OVMF_VARS) ovmf-vars.fd
	$(QEMU_UEFI) -cdrom $(UEFI_ISO) -m 512M -nographic -no-reboot

run-esp: esp
	@cp $(OVMF_VARS) ovmf-vars.fd
	$(QEMU_UEFI) -drive format=raw,file=$(ESP_IMG) -m 512M -nographic -no-reboot

# Test de non-régression : le noyau doit booter et atteindre les marqueurs
# J1 (console), J2.3 (tables de pages 4 niveaux), J2.3b (higher-half)
# et J2.4 (allocateur physique bitmap).
#
# J2.3 est le critère du paging : un descripteur fautif fait triple-fauter
# le noyau au chargement de CR3, avant qu'il n'imprime le marqueur.
# J2.3b exige en plus que le noyau s'exécute en higher-half : l'adresse de
# lien d'un symbole (LINK_GUARD) est > HIGHER_HALF, et la lecture de sa
# valeur par l'adresse virtuelle réussit (sinon #PF avant l'impression).
test: iso
	@timeout 20 qemu-system-x86_64 -cdrom $(ISO) -m 512M -nographic -no-reboot > $(LOG) 2>&1 || true
	@grep -q "J1: noyau charge" $(LOG) && grep -q "J2.3: tables de pages" $(LOG) \
	  && grep -q "higher-half : OK" $(LOG) \
	  && grep -q "J2.4: allocateur physique bitmap - OK" $(LOG) \
	  && echo "TEST OK — BIOS (J1 + J2.3 + J2.3b + J2.4)" \
	  || (echo "TEST ECHEC"; tail -40 $(LOG); exit 1)

# Critère d'acceptation UEFI de J2 : le noyau boote sous OVMF, sur les deux
# chemins (ISO hybride et ESP disque).
#
# OVMF persiste son BootOrder dans le VARS : il faut donc des variables
# fraîches avant CHAQUE boot, sinon le second démarre sur le périphérique
# choisi par le premier et finit en PXE.
test-uefi: iso-uefi esp
	@cp $(OVMF_VARS) ovmf-vars.fd
	@timeout 60 $(QEMU_UEFI) -cdrom $(UEFI_ISO) -m 512M -nographic -no-reboot > $(UEFI_LOG) 2>&1 || true
	@grep -q "J1: noyau charge" $(UEFI_LOG) && grep -q "J2.3: tables de pages" $(UEFI_LOG) \
	  && grep -q "higher-half : OK" $(UEFI_LOG) \
	  && grep -q "J2.4: allocateur physique bitmap - OK" $(UEFI_LOG) \
	  && echo "TEST OK — UEFI ISO (J1 + J2.3 + J2.3b + J2.4)" \
	  || (echo "TEST UEFI ECHEC (ISO)"; tail -40 $(UEFI_LOG); exit 1)
	@cp $(OVMF_VARS) ovmf-vars.fd
	@timeout 60 $(QEMU_UEFI) -drive format=raw,file=$(ESP_IMG) -m 512M -nographic -no-reboot > $(UEFI_LOG) 2>&1 || true
	@grep -q "J1: noyau charge" $(UEFI_LOG) && grep -q "J2.3: tables de pages" $(UEFI_LOG) \
	  && grep -q "higher-half : OK" $(UEFI_LOG) \
	  && grep -q "J2.4: allocateur physique bitmap - OK" $(UEFI_LOG) \
	  && echo "TEST OK — UEFI ESP (J1 + J2.3 + J2.3b + J2.4)" \
	  || (echo "TEST UEFI ECHEC (ESP)"; tail -40 $(UEFI_LOG); exit 1)

clean:
	cargo clean
	rm -rf iso iso-uefi $(ISO) $(UEFI_ISO) $(ESP_IMG) ovmf-vars.fd
