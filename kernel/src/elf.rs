/* ============================================================
 * elf.rs — chargeur ELF64 pour exécutables userland (J3.3/J3.4)
 * ============================================================ */

use core::mem::size_of;
use crate::console;

/* ------------------------------------------------------------
 * Structures ELF64.
 * ------------------------------------------------------------ */

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Ehdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

/* ------------------------------------------------------------
 * Constantes ELF.
 * ------------------------------------------------------------ */

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EM_X86_64: u16 = 62;

const PT_LOAD: u32 = 1;
const PT_PHDR: u32 = 6;
const PT_INTERP: u32 = 3;

const PF_X: u32 = 1;

const PAGE_SIZE: usize = 0x1000;

// --- Vecteur auxiliaire (auxv) ---
const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_BASE: u64 = 7;
const AT_ENTRY: u64 = 9;
const AT_PAGESZ: u64 = 17;

/* ------------------------------------------------------------
 * Informations sur le binaire chargé.
 * ------------------------------------------------------------ */

pub struct ElfInfo {
    pub entry: u64,
    pub phdr_vaddr: u64,
    pub phnum: u16,
    /// Adresse virtuelle de l'interpréteur (ld-musl), 0 si absent.
    pub interp: u64,
    /// Adresse virtuelle du premier segment LOAD (base de l'image).
    pub load_base: u64,
    /// Nom de l'interpréteur (chemin NUL-terminé dans le fichier).
    pub interp_name: [u8; 128],
    pub interp_name_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AuxvEntry {
    a_type: u64,
    a_val: u64,
}

/* ------------------------------------------------------------
 * Helpers.
 * ------------------------------------------------------------ */

unsafe fn parse_phdr(data: &[u8], off: usize) -> Option<Elf64Phdr> {
    if off + size_of::<Elf64Phdr>() > data.len() {
        return None;
    }
    // Lecture via read_unaligned : le champ de header n'est pas garanti
    // aligne (ELF n'impose que l'alignement du segment, pas des headers).
    Some(core::ptr::read_unaligned(data.as_ptr().add(off) as *const Elf64Phdr))
}

/* ------------------------------------------------------------
 * Chargeur ELF64.
 * ------------------------------------------------------------ */

pub unsafe fn load_elf(data: &[u8]) -> Option<ElfInfo> {
    if data.len() < core::mem::size_of::<Elf64Ehdr>() {
        return None;
    }

    let ehdr = &*(data.as_ptr() as *const Elf64Ehdr);

    if ehdr.e_ident[0..4] != ELF_MAGIC {
        console::print(format_args!("load_elf: FAIL bad magic\n"));
        return None;
    }
    if ehdr.e_ident[4] != ELFCLASS64 {
        console::print(format_args!("load_elf: FAIL bad class\n"));
        return None;
    }
    if ehdr.e_ident[5] != ELFDATA2LSB {
        console::print(format_args!("load_elf: FAIL bad endian\n"));
        return None;
    }
    if ehdr.e_machine != EM_X86_64 {
        console::print(format_args!("load_elf: FAIL bad machine={}\n", ehdr.e_machine));
        return None;
    }
    if ehdr.e_phentsize as usize != size_of::<Elf64Phdr>() {
        return None;
    }

    let phoff = ehdr.e_phoff as usize;
    let phnum = ehdr.e_phnum;

    let mut phdr_vaddr: u64 = 0;
    let mut interp: u64 = 0;
    let mut load_base: u64 = 0;
    let mut interp_name: [u8; 128] = [0; 128];
    let mut interp_name_len: usize = 0;

    // Premier passage : repérer PT_INTERP et la base de chargement.
    for i in 0..phnum as usize {
        let off = phoff + i * size_of::<Elf64Phdr>();
        let phdr = parse_phdr(data, off)?;

        match phdr.p_type {
            PT_PHDR => {
                phdr_vaddr = phdr.p_vaddr;
            }
            PT_INTERP => {
                // Le nom est une chaîne NUL-terminée dans le fichier.
                let start = phdr.p_offset as usize;
                let end = core::cmp::min(start + phdr.p_filesz as usize, data.len());
                let mut n = 0usize;
                let mut i = start;
                while i < end && n < 127 && data[i] != 0 {
                    interp_name[n] = data[i];
                    n += 1;
                    i += 1;
                }
                interp_name_len = n;
            }
            PT_LOAD => {
                if load_base == 0 {
                    load_base = phdr.p_vaddr;
                }
            }
            _ => {}
        }
    }

    // Deuxième passage : charger les segments LOAD.
    for i in 0..phnum as usize {
        let off = phoff + i * size_of::<Elf64Phdr>();
        let phdr = parse_phdr(data, off)?;

        // W8 : congruence p_vaddr ≡ p_offset (mod page) — invariant ELF
        // pour les segments LOAD; un desync = header corrompu.
        if (phdr.p_vaddr as usize) % PAGE_SIZE != (phdr.p_offset as usize) % PAGE_SIZE {
            return None;
        }

        match phdr.p_type {
            PT_LOAD => {
                let mut page_flags = crate::mm::paging::PRESENT
                    | crate::mm::paging::WRITABLE
                    | crate::mm::paging::USER;
                if phdr.p_flags & PF_X == 0 {
                    page_flags |= crate::mm::paging::NOEXEC;
                }

                let seg_vaddr = phdr.p_vaddr;
                let seg_offset = phdr.p_offset as usize;
                let filesz = phdr.p_filesz as usize;
                let memsz = phdr.p_memsz as usize;
                let page_offset_in_seg = (seg_vaddr & 0xFFF) as usize;
                let total_size = memsz + page_offset_in_seg;
                let num_pages = (total_size + PAGE_SIZE - 1) / PAGE_SIZE;

                for page_idx in 0..num_pages {
                    let page_vaddr =
                        seg_vaddr + page_idx as u64 * PAGE_SIZE as u64;

                    let paddr = crate::mm::phys::alloc_page()
                        .expect("ELF: OOM") as u64;

                    crate::mm::paging::map_page(page_vaddr, paddr, page_flags);

                    let file_page_off =
                        seg_offset + page_idx * PAGE_SIZE;

                    if file_page_off < data.len()
                        && file_page_off < seg_offset + filesz
                    {
                        let copy_end = core::cmp::min(
                            file_page_off + PAGE_SIZE,
                            core::cmp::min(seg_offset + filesz, data.len()),
                        );
                        let copy_size = copy_end - file_page_off;
                        unsafe {
                            let src = data.as_ptr().add(file_page_off);
                            let dst = page_vaddr as *mut u8;
                            core::ptr::copy_nonoverlapping(src, dst, copy_size);
                        }
                    }

                    let bss_offset = filesz - page_idx * PAGE_SIZE;
                    if bss_offset < PAGE_SIZE && memsz > filesz {
                        let bss_start = (page_vaddr as usize)
                            .saturating_add(bss_offset.max(0));
                        let bss_len = PAGE_SIZE.saturating_sub(bss_offset.max(0));
                        if bss_len > 0 {
                            core::ptr::write_bytes(
                                bss_start as *mut u8,
                                0,
                                bss_len,
                            );
                        }
                    }
                }
            }
            _ => {
                // Segment ignoré (ex : PT_NOTE en phase J3.5).
            }
        }
    }

    Some(ElfInfo {
        entry: ehdr.e_entry,
        phdr_vaddr,
        phnum,
        interp,
        load_base,
        interp_name,
        interp_name_len,
    })
}

/* ------------------------------------------------------------
 * Construction de la pile initiale (argc/argv/envp/auxv).
 * ------------------------------------------------------------ */

pub unsafe fn build_stack(
    stack_base_vaddr: u64,
    _num_pages: usize,
    name: &str,
    elf: &ElfInfo,
    interp_base: u64,
) -> u64 {
    const ARG_MAX: usize = 32;

    let argc: usize = 1;
    let mut argv: [u64; ARG_MAX] = [0; ARG_MAX];
    let envp: [u64; ARG_MAX] = [0; ARG_MAX];
    let envc: usize = 0;
    let mut auxv: [AuxvEntry; 16] = [AuxvEntry { a_type: 0, a_val: 0 }; 16];
    let mut aux_idx: usize = 0;

    argv[0] = stack_base_vaddr - 8 * (name.len() + 1) as u64;

    auxv[aux_idx] = AuxvEntry { a_type: AT_PHDR, a_val: elf.phdr_vaddr };
    aux_idx += 1;
    auxv[aux_idx] = AuxvEntry { a_type: AT_PHENT, a_val: size_of::<Elf64Phdr>() as u64 };
    aux_idx += 1;
    auxv[aux_idx] = AuxvEntry { a_type: AT_PHNUM, a_val: elf.phnum as u64 };
    aux_idx += 1;
    // AT_BASE : adresse de l'interpréteur (0 si binaire statique).
    auxv[aux_idx] = AuxvEntry { a_type: AT_BASE, a_val: interp_base };
    aux_idx += 1;
    auxv[aux_idx] = AuxvEntry { a_type: AT_PAGESZ, a_val: PAGE_SIZE as u64 };
    aux_idx += 1;
    auxv[aux_idx] = AuxvEntry { a_type: AT_ENTRY, a_val: elf.entry };
    aux_idx += 1;
    auxv[aux_idx] = AuxvEntry { a_type: AT_NULL, a_val: 0 };
    aux_idx += 1;

    let name_bytes = name.as_bytes();
    let name_dst = argv[0] as *mut u8;
    core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), name_dst, name_bytes.len());
    *name_dst.add(name_bytes.len()) = 0;

    let mut total_size: usize = 8; // argc
    total_size += argc.saturating_mul(8); // argv : argc pointeurs (+ NULL final)
    for _i in 0..envc { total_size += 32; }
    for _i in 0..aux_idx { total_size += 16; }

    let stack_top = (stack_base_vaddr - total_size as u64) & !0xF;

    let sp = stack_top as *mut u64;
    *sp = argc as u64;
    let mut sp = sp.add(1);

    for i in 0..argc { *sp = argv[i]; sp = sp.add(1); }
    *sp = 0; sp = sp.add(1);
    for i in 0..envc { *sp = envp[i]; sp = sp.add(1); }
    *sp = 0; sp = sp.add(1);
    for i in 0..aux_idx { *sp = auxv[i].a_type; sp = sp.add(1); *sp = auxv[i].a_val; sp = sp.add(1); }

    stack_top
}
