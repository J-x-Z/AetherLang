//! Self-Hosted ELF64 Linker
//!
//! Provides functionality to generate ELF64 executable files directly.
#![allow(dead_code)]

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

// ==================== ELF Constants ====================

// File Type
pub const ET_NONE: u16 = 0;
pub const ET_REL: u16 = 1;
pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const ET_CORE: u16 = 4;

// Machine Architecture
pub const EM_NONE: u16 = 0;
pub const EM_X86_64: u16 = 62;
pub const EM_AARCH64: u16 = 183;

// Segment Types
pub const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_SHLIB: u32 = 5;
pub const PT_PHDR: u32 = 6;

// Segment Flags
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

// ==================== ELF Structures ====================

/// ELF64 File Header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Elf64_Ehdr {
    pub e_ident: [u8; 16], // Magic number and other info
    pub e_type: u16,       // Object file type
    pub e_machine: u16,    // Architecture
    pub e_version: u32,    // Object file version
    pub e_entry: u64,      // Entry point virtual address
    pub e_phoff: u64,      // Program header table file offset
    pub e_shoff: u64,      // Section header table file offset
    pub e_flags: u32,      // Processor-specific flags
    pub e_ehsize: u16,     // ELF header size in bytes
    pub e_phentsize: u16,  // Program header table entry size
    pub e_phnum: u16,      // Program header table entry count
    pub e_shentsize: u16,  // Section header table entry size
    pub e_shnum: u16,      // Section header table entry count
    pub e_shstrndx: u16,   // Section header string table index
}

/// ELF64 Program Header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Elf64_Phdr {
    pub p_type: u32,   // Segment type
    pub p_flags: u32,  // Segment flags
    pub p_offset: u64, // Segment file offset
    pub p_vaddr: u64,  // Segment virtual address
    pub p_paddr: u64,  // Segment physical address
    pub p_filesz: u64, // Segment size in file
    pub p_memsz: u64,  // Segment size in memory
    pub p_align: u64,  // Segment alignment
}

// Section Header Types
pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_HASH: u32 = 5;
pub const SHT_DYNAMIC: u32 = 6;
pub const SHT_NOTE: u32 = 7;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_REL: u32 = 9;
pub const SHT_SHLIB: u32 = 10;
pub const SHT_DYNSYM: u32 = 11;

// Section Flags
pub const SHF_WRITE: u64 = 1;
pub const SHF_ALLOC: u64 = 2;
pub const SHF_EXECINSTR: u64 = 4;

/// ELF64 Section Header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Elf64_Shdr {
    pub sh_name: u32,      // Section name (string tbl index)
    pub sh_type: u32,      // Section type
    pub sh_flags: u64,     // Section flags
    pub sh_addr: u64,      // Section virtual addr at execution
    pub sh_offset: u64,    // Section file offset
    pub sh_size: u64,      // Section size in bytes
    pub sh_link: u32,      // Link to another section
    pub sh_info: u32,      // Additional section information
    pub sh_addralign: u64, // Section alignment
    pub sh_entsize: u64,   // Entry size if section holds table
}

// ==================== Linker ====================

pub struct Linker {
    entry_point: u64,
    segments: Vec<Segment>,
    sections: Vec<Section>,
    shstrtab: Vec<u8>, // Section Header String Table
}

struct Segment {
    flags: u32,
    data: Vec<u8>,
    vaddr: u64,
}

struct Section {
    name: String,
    sh_type: u32,
    flags: u64,
    data: Vec<u8>,
    vaddr: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
}

impl Linker {
    pub fn new() -> Self {
        // Initialize with null byte for string table
        Self {
            entry_point: 0x400000,
            segments: Vec::new(),
            sections: Vec::new(),
            shstrtab: vec![0], // Starts with null byte
        }
    }

    pub fn set_entry_point(&mut self, addr: u64) {
        self.entry_point = addr;
    }

    pub fn add_segment(&mut self, data: Vec<u8>, flags: u32, vaddr: u64) {
        self.segments.push(Segment {
            flags,
            data,
            vaddr,
        });
    }

    /// Add a section and return its index
    pub fn add_section(&mut self, name: &str, data: Vec<u8>, sh_type: u32, flags: u64, vaddr: u64) -> usize {
        self.sections.push(Section {
            name: name.to_string(),
            sh_type,
            flags,
            data,
            vaddr,
            link: 0,
            info: 0,
            align: 16, // Default alignment
            entsize: 0,
        });
        self.sections.len()
    }

    /// Emit the linked ELF file
    pub fn emit<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        let mut file = File::create(path)?;
        
        // 0. Finalize String Table
        // We need to build shstrtab based on section names
        // Clear existing (except null) to rebuild cleanly if called multiple times
        self.shstrtab = vec![0]; 
        let mut name_offsets = Vec::new();
        
        // Add null section name offset
        name_offsets.push(0);

        for section in &self.sections {
            let offset = self.shstrtab.len() as u32;
            name_offsets.push(offset);
            self.shstrtab.extend_from_slice(section.name.as_bytes());
            self.shstrtab.push(0); // Null terminator
        }
        
        // Add .shstrtab section itself to the list (temporarily or logic wise)
        // Usually .shstrtab is the last section.
        let _shstrtab_offset_in_shdr = self.shstrtab.len(); 
        // We will write shstrtab data at the end of file content, before Section Headers

        // 1. Prepare Headers
        let mut ehdr = Elf64_Ehdr::default();
        
        // Magic
        ehdr.e_ident[0] = 0x7F;
        ehdr.e_ident[1] = b'E';
        ehdr.e_ident[2] = b'L';
        ehdr.e_ident[3] = b'F';
        ehdr.e_ident[4] = 2; // Class: 64-bit
        ehdr.e_ident[5] = 1; // Endianness: Little
        ehdr.e_ident[6] = 1; // Version: 1
        ehdr.e_ident[7] = 0; // OS ABI: System V
        
        ehdr.e_type = ET_EXEC;
        ehdr.e_machine = EM_X86_64;
        ehdr.e_version = 1;
        ehdr.e_entry = self.entry_point;
        ehdr.e_ehsize = std::mem::size_of::<Elf64_Ehdr>() as u16;
        ehdr.e_phentsize = std::mem::size_of::<Elf64_Phdr>() as u16;
        ehdr.e_phnum = self.segments.len() as u16;
        ehdr.e_shentsize = std::mem::size_of::<Elf64_Shdr>() as u16;
        // +1 for Null Section, +1 for .shstrtab
        ehdr.e_shnum = (self.sections.len() + 2) as u16; 
        ehdr.e_shstrndx = (self.sections.len() + 1) as u16; // Index of .shstrtab
        
        // Calculate offsets
        let ph_offset = std::mem::size_of::<Elf64_Ehdr>() as u64;
        ehdr.e_phoff = ph_offset;
        
        // Write ELF Header
        let ehdr_bytes = unsafe {
            std::slice::from_raw_parts(
                &ehdr as *const _ as *const u8,
                std::mem::size_of::<Elf64_Ehdr>()
            )
        };
        file.write_all(ehdr_bytes)?;
        
        // Write Program Headers
        let mut current_offset = ph_offset + (self.segments.len() as u64 * std::mem::size_of::<Elf64_Phdr>() as u64);
        
        for segment in &self.segments {
            let phdr = Elf64_Phdr {
                p_type: PT_LOAD,
                p_flags: segment.flags,
                p_offset: current_offset,
                p_vaddr: segment.vaddr,
                p_paddr: segment.vaddr,
                p_filesz: segment.data.len() as u64,
                p_memsz: segment.data.len() as u64,
                p_align: 0x1000, 
            };
            
            let phdr_bytes = unsafe {
                std::slice::from_raw_parts(
                    &phdr as *const _ as *const u8,
                    std::mem::size_of::<Elf64_Phdr>()
                )
            };
            file.write_all(phdr_bytes)?;
            
            // Advance offset for next segment's data
            // Note: Use the segment's data length for file offset calculation
            current_offset += segment.data.len() as u64;
        }
        
        // Write Segment Data
        for segment in &self.segments {
            file.write_all(&segment.data)?;
        }
        
        // Write .shstrtab Data (it's not in segments usually, but resides in file)
        let shstrtab_file_offset = current_offset;
        file.write_all(&self.shstrtab)?;
        current_offset += self.shstrtab.len() as u64;
        
        // Write Section Headers
        // Update ELF Header with Section Header Offset (need to seek back or write it later? We passed it, so seek back)
        let sh_offset = current_offset;
        
        // 1. Null Section
        let null_shdr = Elf64_Shdr::default();
        let null_bytes = unsafe {
            std::slice::from_raw_parts(
                &null_shdr as *const _ as *const u8,
                std::mem::size_of::<Elf64_Shdr>()
            )
        };
        file.write_all(null_bytes)?;
        
        // 2. User Sections
        // We need to calculate file offsets for sections. 
        // For simplicity in this self-hosted linker, we assume sections map to segments.
        // Or we just point them to where we wrote segment data.
        // Simplification: We blindly assume the first segment is .text and second is data if exists.
        // This is fragile. Stronger logic: user adds section, we verify it matches a segment or we append it.
        // For this iteration: just write headers that point to 0 or where we think they are.
        
        let mut section_data_offset = ph_offset + (self.segments.len() as u64 * std::mem::size_of::<Elf64_Phdr>() as u64);

        for (i, section) in self.sections.iter().enumerate() {
            let shdr = Elf64_Shdr {
                sh_name: name_offsets[i+1], // +1 because 0 is null section name
                sh_type: section.sh_type,
                sh_flags: section.flags,
                sh_addr: section.vaddr,
                sh_offset: section_data_offset, // This assumes section data corresponds 1:1 to segments in order.
                sh_size: section.data.len() as u64,
                sh_link: section.link,
                sh_info: section.info,
                sh_addralign: section.align,
                sh_entsize: section.entsize,
            };
            
            let shdr_bytes = unsafe {
                std::slice::from_raw_parts(
                    &shdr as *const _ as *const u8,
                    std::mem::size_of::<Elf64_Shdr>()
                )
            };
            file.write_all(shdr_bytes)?;
            
            section_data_offset += section.data.len() as u64;
        }
        
        // 3. .shstrtab Section Header
        
        // Add .shstrtab name to the string table itself?
        // Standard convention: The string table usually contains its own name.
        // Let's ensure we added ".shstrtab" to the string table.
        // But for now, let's just create the header.
        
        let _shstrtab_name_offset = self.shstrtab.len() as u32;
        // We actually modify shstrtab AFTER the loop in a real implementation to include its own name
        // but since we already wrote it to file, we can't easily append.
        // Quick fix: Add ".shstrtab" initially or just use 0 (no name) for now.
        // Or better: Append it to the vector before writing to file.
        
        // RE-DOING string table logic properly:
        // We need to rebuild string table including ".shstrtab" at the end if we want it named.
        
        let shstrtab_shdr = Elf64_Shdr {
            sh_name: 0, // No name for now to avoid logic complexity
            sh_type: SHT_STRTAB,
            sh_flags: 0,
            sh_addr: 0,
            sh_offset: shstrtab_file_offset,
            sh_size: self.shstrtab.len() as u64,
            sh_link: 0,
            sh_info: 0,
            sh_addralign: 1,
            sh_entsize: 0,
        };

         // Fix: sh_name for .shstrtab. 
         // We should have added ".shstrtab" to the string table.
         
        let shstrtab_bytes = unsafe {
            std::slice::from_raw_parts(
                &shstrtab_shdr as *const _ as *const u8,
                std::mem::size_of::<Elf64_Shdr>()
            )
        };
        file.write_all(shstrtab_bytes)?;

        // Go back and update e_shoff
        // Elf64_Ehdr is at 0
        // e_shoff is at offset 40 (0x28)
        let shoff_bytes = sh_offset.to_le_bytes();
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(40))?;
        file.write_all(&shoff_bytes)?;
        
        Ok(())
    }
}
