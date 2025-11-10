use anyhow::{Context, Result};
use byteorder::{ByteOrder, LittleEndian};
use object::{
    build::{self, elf::SectionData, ByteString},
    elf, Object, ObjectSection,
};
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

    fn add_embedded_config_data_section(&mut self, config_data: &[u8], use_xz: bool) -> Result<()> {
        let final_data = if use_xz {
            self.compress_xz(config_data)?
        } else {
            config_data.to_vec()
        };

        let data_cloned = self.data.clone();
        let mut binary = object::build::elf::Builder::read(data_cloned.as_slice())?;
        let new_section = binary.sections.add();
        new_section.data = SectionData::Data(final_data.into());
        new_section.sh_flags = elf::SHF_ALLOC as u64 | elf::SHF_EXECINSTR as u64;
        new_section.sh_type = elf::SHT_PROGBITS;
        new_section.name = ByteString::from(".fripack_config");
        new_section.sh_offset = self.data.len() as u64 + 1;

        self.data = vec![];
        binary.write(&mut self.data)?;
        Ok(())
    }

    pub fn add_embedded_config_data(&mut self, config_data: &[u8], use_xz: bool) -> Result<()> {
        self.add_embedded_config_data_section(config_data, use_xz)?;
        let file = object::File::parse(self.data.as_slice())?;
        // search for the embedded config and update it
        let offset = self
            .find_embedded_config()
            .context("Cannot found embedded config")?;
        
        let embedded_config = EmbeddedConfig::new(
            config_data.len() as i32,
            file.section_by_name(".fripack_config")
                .context("Cannot find .fripack_config section")?
                .file_range()
                .context("Cannot get file range of .fripack_config section")?
                .0 as i32 - offset as i32,
            use_xz,
        );

        let embedded_config_bytes = embedded_config.as_bytes();
        self.data[offset..offset + embedded_config_bytes.len()]
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
