//! Native ELF64 section & symbol inspector for Asterinas kernel objects.
//!
//! Verifies that an unstripped Asterinas kernel ELF (`vmlinux` / `asterinas-osdk-bin`)
//! retains `.symtab` and `.debug_info` sections and defines `__sanitizer_cov_trace_pc`.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_DYNSYM: u32 = 11;
const SHN_UNDEF: u16 = 0;

#[derive(Debug, Clone)]
pub struct ElfInspection {
    pub sections: HashSet<String>,
    pub defined_symbols: HashSet<String>,
    pub undefined_symbols: HashSet<String>,
    pub stext_addr: Option<u64>,
    pub __stext_addr: Option<u64>,
    pub etext_addr: Option<u64>,
}

impl ElfInspection {
    pub fn has_symtab(&self) -> bool {
        self.sections.contains(".symtab")
    }

    pub fn has_debug_info(&self) -> bool {
        self.sections.contains(".debug_info")
    }

    pub fn defines_symbol(&self, name: &str) -> bool {
        self.defined_symbols.contains(name)
    }

    pub fn references_undefined_symbol(&self, name: &str) -> bool {
        self.undefined_symbols.contains(name)
    }

    /// Resolves the Linux PC base address, preferring `_stext` and falling back to `__stext`.
    pub fn find_linux_pc_base(&self) -> Option<u64> {
        self.stext_addr.or(self.__stext_addr)
    }
}

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    Some(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn read_cstr(strtab: &[u8], offset: usize) -> Option<&str> {
    if offset >= strtab.len() {
        return None;
    }
    let tail = &strtab[offset..];
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    std::str::from_utf8(&tail[..end]).ok()
}

/// Inspects a 64-bit little-endian ELF binary natively in Rust.
pub fn inspect_elf(path: &Path) -> Result<ElfInspection, String> {
    let data = fs::read(path).map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    inspect_elf_bytes(&data)
}

pub fn inspect_elf_bytes(data: &[u8]) -> Result<ElfInspection, String> {
    if data.len() < 64 || data[0..4] != ELF_MAGIC {
        return Err("not a valid ELF file".to_string());
    }
    if data[4] != ELFCLASS64 || data[5] != ELFDATA2LSB {
        return Err("unsupported ELF class or endianness (expected 64-bit LSB)".to_string());
    }

    let e_shoff = read_u64_le(data, 40).ok_or("truncated ELF header")? as usize;
    let e_shentsize = read_u16_le(data, 58).ok_or("truncated ELF header")? as usize;
    let e_shnum = read_u16_le(data, 60).ok_or("truncated ELF header")? as usize;
    let e_shstrndx = read_u16_le(data, 62).ok_or("truncated ELF header")? as usize;

    if e_shoff == 0 || e_shnum == 0 || e_shentsize < 64 {
        return Err("ELF has no section headers".to_string());
    }
    let sh_end = e_shoff
        .checked_add(e_shnum.checked_mul(e_shentsize).ok_or("invalid shnum")?)
        .ok_or("invalid shoff")?;
    if sh_end > data.len() {
        return Err("truncated ELF section header table".to_string());
    }

    // Resolve section header string table index (handle SHN_XINDEX = 0xffff)
    let shstrndx = if e_shstrndx == 0xffff {
        read_u32_le(data, e_shoff + 40).ok_or("invalid sh_link in section 0")? as usize
    } else {
        e_shstrndx
    };
    if shstrndx >= e_shnum {
        return Err("invalid e_shstrndx".to_string());
    }
    let shstr_hdr = e_shoff + shstrndx * e_shentsize;
    let shstr_off = read_u64_le(data, shstr_hdr + 24).ok_or("bad shstrtab offset")? as usize;
    let shstr_size = read_u64_le(data, shstr_hdr + 32).ok_or("bad shstrtab size")? as usize;
    let shstrtab = data
        .get(shstr_off..shstr_off.saturating_add(shstr_size))
        .ok_or("truncated .shstrtab")?;

    let mut sections = HashSet::new();
    let mut defined_symbols = HashSet::new();
    let mut undefined_symbols = HashSet::new();
    let mut stext_addr = None;
    let mut __stext_addr = None;
    let mut etext_addr = None;

    for i in 0..e_shnum {
        let sh = e_shoff + i * e_shentsize;
        let sh_name = read_u32_le(data, sh).ok_or("bad sh_name")? as usize;
        let sh_type = read_u32_le(data, sh + 4).ok_or("bad sh_type")?;
        let sh_offset = read_u64_le(data, sh + 24).ok_or("bad sh_offset")? as usize;
        let sh_size = read_u64_le(data, sh + 32).ok_or("bad sh_size")? as usize;
        let sh_link = read_u32_le(data, sh + 40).ok_or("bad sh_link")? as usize;
        let sh_entsize = read_u64_le(data, sh + 56).ok_or("bad sh_entsize")? as usize;

        if let Some(name) = read_cstr(shstrtab, sh_name) {
            if !name.is_empty() {
                sections.insert(name.to_string());
            }
        }

        if (sh_type == SHT_SYMTAB || sh_type == SHT_DYNSYM) && sh_entsize >= 24 && sh_link < e_shnum
        {
            let str_hdr = e_shoff + sh_link * e_shentsize;
            let str_off = read_u64_le(data, str_hdr + 24).ok_or("bad strtab offset")? as usize;
            let str_size = read_u64_le(data, str_hdr + 32).ok_or("bad strtab size")? as usize;
            let Some(strtab) = data.get(str_off..str_off.saturating_add(str_size)) else {
                continue;
            };
            let Some(symtab) = data.get(sh_offset..sh_offset.saturating_add(sh_size)) else {
                continue;
            };
            let count = symtab.len() / sh_entsize;
            for s in 0..count {
                let sym = s * sh_entsize;
                let st_name = read_u32_le(symtab, sym).unwrap_or(0) as usize;
                let st_shndx = read_u16_le(symtab, sym + 6).unwrap_or(SHN_UNDEF);
                let st_value = read_u64_le(symtab, sym + 8).unwrap_or(0);
                if let Some(sym_name) = read_cstr(strtab, st_name) {
                    if !sym_name.is_empty() {
                        if st_shndx == SHN_UNDEF {
                            undefined_symbols.insert(sym_name.to_string());
                        } else {
                            defined_symbols.insert(sym_name.to_string());
                            match sym_name {
                                "_stext" => stext_addr = Some(st_value),
                                "__stext" if __stext_addr.is_none() => {
                                    __stext_addr = Some(st_value)
                                }
                                "_etext" => etext_addr = Some(st_value),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(ElfInspection {
        sections,
        defined_symbols,
        undefined_symbols,
        stext_addr,
        __stext_addr,
        etext_addr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pc_base_preference() {
        let mut info = ElfInspection {
            sections: HashSet::new(),
            defined_symbols: HashSet::new(),
            undefined_symbols: HashSet::new(),
            stext_addr: Some(0xffffffff81000000),
            __stext_addr: Some(0xffffffff8804b000),
            etext_addr: None,
        };
        assert_eq!(info.find_linux_pc_base(), Some(0xffffffff81000000));
        info.stext_addr = None;
        assert_eq!(info.find_linux_pc_base(), Some(0xffffffff8804b000));
        info.__stext_addr = None;
        assert_eq!(info.find_linux_pc_base(), None);
    }
}
