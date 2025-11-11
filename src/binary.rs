use anyhow::{Context, Result};
use log::info;
use object::{
    build::ByteString,
    elf::{PF_R, PF_W, PT_NOTE, PT_PHDR},
};
use object_rewrite::Rewriter;
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedConfig {
    pub magic1: i32,
    pub magic2: i32,
    pub version: i32,
    pub data_size: i32,
    pub data_offset: i32,
    pub data_xz: bool,
}

impl Default for EmbeddedConfig {
    fn default() -> Self {
        Self {
            magic1: 0x0d000721,
            magic2: 0x1f8a4e2b,
            version: 1,
            data_size: 0,
            data_offset: 0,
            data_xz: false,
        }
    }
}

impl EmbeddedConfig {
    pub fn new(data_size: i32, data_offset: i32, data_xz: bool) -> Self {
        Self {
            magic1: 0x0d000721,
            magic2: 0x1f8a4e2b,
            version: 1,
            data_size,
            data_offset,
            data_xz,
        }
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0; std::mem::size_of::<EmbeddedConfig>()];
        unsafe {
            let ptr = self as *const EmbeddedConfig as *const u8;
            std::ptr::copy_nonoverlapping(
                ptr,
                bytes.as_mut_ptr(),
                std::mem::size_of::<EmbeddedConfig>(),
            );
        }
        bytes
    }
}

pub struct BinaryProcessor {
    data: Vec<u8>,
}

impl BinaryProcessor {
    pub fn new(data: Vec<u8>) -> Result<Self> {
        if data.len() < 16 || &data[0..4] != b"\x7fELF" {
            anyhow::bail!("Invalid ELF binary");
        }

        Ok(Self { data })
    }

    pub fn find_embedded_config(&self) -> Option<usize> {
        let magic1_bytes = (0x0d000721i32).to_le_bytes();
        let magic2_bytes = (0x1f8a4e2bi32).to_le_bytes();

        for i in 0..self
            .data
            .len()
            .saturating_sub(std::mem::size_of::<EmbeddedConfig>())
        {
            if self.data[i..i + 4] == magic1_bytes && self.data[i + 4..i + 8] == magic2_bytes {
                return Some(i);
            }
        }

        None
    }

    pub fn add_embedded_config_data(&mut self, config_data: &[u8], use_xz: bool) -> Result<()> {
        let data = if use_xz {
            self.compress_xz(config_data)?
        } else {
            config_data.to_vec()
        };
        let mut embedded_config = EmbeddedConfig::new(data.len() as i32, 0, use_xz);

        let data_cloned = self.data.clone();
        let elf = object::build::elf::Builder::read(data_cloned.as_slice())?;
        self.data = vec![];
        // re-write ELF to ensure segments are properly aligned
        elf.write(&mut self.data)?;
        let data_cloned = self.data.clone();
        let mut elf = object::build::elf::Builder::read(data_cloned.as_slice())?;
        let vaddr_spare_area = elf
            .segments
            .iter()
            .map(|seg| seg.p_vaddr + seg.p_memsz)
            .max()
            .unwrap_or(0);

        
        let vaddr_spare_area = (vaddr_spare_area + 0xfff) & !0xfff;
        info!("vaddr_spare_area: {:#x}", vaddr_spare_area);

        let mut offset_spare_area = self.data.len() as u64;

        let fripack_section_id = {
            let new_segment = elf.segments.add_load_segment(PF_R | PF_W, 4096);
            let new_section = elf.sections.add();

            offset_spare_area = (offset_spare_area + 0xfff) & !0xfff;

            new_section.sh_size = data.len() as u64;
            new_section.data = object::build::elf::SectionData::Data(data.into());
            new_section.sh_flags = (object::elf::SHF_ALLOC | object::elf::SHF_WRITE) as u64;
            new_section.sh_type = object::elf::SHT_PROGBITS;
            new_section.sh_addralign = 4096;
            new_section.sh_offset = offset_spare_area;
            new_section.sh_addr = vaddr_spare_area;
            new_segment.p_vaddr = vaddr_spare_area;
            new_segment.append_section(new_section);
            new_section.sh_addr = vaddr_spare_area;
            new_segment.p_vaddr = vaddr_spare_area;
            offset_spare_area += new_section.sh_size;
            offset_spare_area = (offset_spare_area + 0xfff) & !0xfff;

            new_section.id()
        };

        let header_size = elf.file_header_size() as u64 + elf.program_headers_size() as u64;
        // move sections overlapped with the header to the end of file
        for section in elf.sections.iter_mut() {
            if section.sh_offset < header_size {
                info!("Moving section {}", section.name);
                section.sh_offset = offset_spare_area;
                offset_spare_area = (section.sh_offset + section.sh_size + 0xfff) & !0xfff;
            }
        }

        let size = elf.program_headers_size() as u64;
        let (phdr_segment_offset, phdr_segment_size_diff) = {
            let phdr_segment = elf
                .segments
                .iter_mut()
                .find(|seg| seg.p_type == PT_PHDR)
                .context("Failed to find PT_PHDR segment")?;

            // update PT_PHDR segment size
            let size_diff = size - phdr_segment.p_filesz;
            phdr_segment.p_filesz = size;
            phdr_segment.p_memsz = size;
            (phdr_segment.p_offset, size_diff)
        };

        // resize the PT_LOAD segment that covered PT_PHDR segment to include the new size
        let seg = &mut elf.segments;
        let segment_that_covers_phdr = seg
            .iter_mut()
            .find(|seg| {
                seg.p_type == object::elf::PT_LOAD
                    && seg.p_offset <= phdr_segment_offset
                    && phdr_segment_offset < seg.p_offset + seg.p_filesz
            })
            .context("Failed to find PT_LOAD segment that covers PT_PHDR")?;

        segment_that_covers_phdr.p_filesz += phdr_segment_size_diff;
        segment_that_covers_phdr.p_memsz += phdr_segment_size_diff;

        self.data = vec![];
        elf.write(&mut self.data)?;
        let data_cloned = self.data.clone();
        let mut elf = object::build::elf::Builder::read(data_cloned.as_slice())?;
        // update embedded config offset
        let embedded_config_offset = self
            .find_embedded_config()
            .context("Failed to find embedded config after adding data")?;

        let data_section_segment = elf
            .segments
            .iter()
            .find(|seg| {
                seg.p_offset <= embedded_config_offset as u64
                    && (embedded_config_offset as u64) < seg.p_offset + seg.p_filesz
            })
            .context("Failed to find data segment")?;

        let fripack_section_segment = elf
            .segments
            .iter()
            .find(|seg| {
                seg.sections
                    .iter()
                    .any(|sec| *sec == fripack_section_id)
            })
            .context("Failed to find fripack_config segment")?;

        embedded_config.data_offset = ((fripack_section_segment.p_offset as i32
            - embedded_config_offset as i32)
            - (fripack_section_segment.p_offset as i32 - data_section_segment.p_offset as i32)
            + (fripack_section_segment.p_vaddr as i32 - data_section_segment.p_vaddr as i32))
            as i32;
        let embedded_config_bytes = embedded_config.as_bytes();
        self.data[embedded_config_offset..embedded_config_offset + embedded_config_bytes.len()]
            .copy_from_slice(&embedded_config_bytes);

        Ok(())
    }

    fn compress_xz(&self, data: &[u8]) -> Result<Vec<u8>> {
        use std::io::Write;
        use xz2::write::XzEncoder;

        let mut encoder = XzEncoder::new(Vec::new(), 6);
        encoder.write_all(data)?;
        Ok(encoder.finish()?)
    }

    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
}
