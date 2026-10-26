TARGET  := x86_64-unknown-linux-gnu
KERNEL  := target/$(TARGET)/release/kernel
ISO     := xili.iso
LOG     := /tmp/xili-serial.log

# Le noyau est un binaire freestanding : pas de crt, pas de libc,
# pas de PIE, et un script de link dédié.
RUSTFLAGS := -C no-redzone=yes \
             -C relocation-model=static \
             -C link-arg=-nostdlib \
             -C link-arg=-static \
             -C link-arg=-no-pie \
             -C link-arg=-Wl,--build-id=none \
             -C link-arg=-T$(CURDIR)/kernel/linker.ld

.PHONY: all build iso run run-headless test clean

all: iso

build:
	RUSTFLAGS="$(RUSTFLAGS)" cargo build --release

iso: build
	rm -rf iso
	mkdir -p iso/boot/grub
	cp $(KERNEL) iso/boot/kernel
	cp boot/grub.cfg iso/boot/grub/grub.cfg
	grub-mkrescue -o $(ISO) iso

run: iso
	qemu-system-x86_64 -cdrom $(ISO) -m 512M -serial stdio

run-headless: iso
	qemu-system-x86_64 -cdrom $(ISO) -m 512M -nographic -no-reboot

# Test de non-régression : le noyau doit booter et atteindre le marqueur J1.
test: iso
	@timeout 10 qemu-system-x86_64 -cdrom $(ISO) -m 512M -nographic -no-reboot > $(LOG) 2>&1 || true
	@grep -q "J1: noyau charge" $(LOG) && echo "TEST OK — le noyau boote et atteint J1" || (echo "TEST ECHEC"; tail -40 $(LOG); exit 1)

clean:
	cargo clean
	rm -rf iso $(ISO)
