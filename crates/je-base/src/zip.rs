//! Porte de `src/vs/base/common/zip.ts`: um ZIP sem compressão para arquivos em memória, sem ZIP64.

use std::sync::OnceLock;

use jiff::Timestamp;

use crate::buffer::VSBuffer;
use crate::date::local_time_zone;
use crate::errors::Error;
use crate::runtime;

fn crc_table() -> &'static [u32; 256] {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (index, slot) in table.iter_mut().enumerate() {
            let mut value = index as u32;
            for _ in 0..8 {
                value = if value & 1 != 0 { 0xedb8_8320 ^ (value >> 1) } else { value >> 1 };
            }
            *slot = value;
        }
        table
    })
}

fn crc32(contents: &[u8]) -> u32 {
    let table = crc_table();
    let mut crc = 0xffff_ffffu32;
    for &byte in contents {
        crc = table[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

/// Um arquivo para o [`create_zip`].
pub struct ZipFile {
    pub path: String,
    pub contents: VSBuffer,
}

fn set_u16(buffer: &mut [u8], value: u32, offset: usize) {
    buffer[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
}

fn set_u32(buffer: &mut [u8], value: u32, offset: usize) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// O caminho tem `\`, `:` ou NUL, ou um segmento vazio, `.` ou `..`?
fn is_unsafe_path(path: &str) -> bool {
    path.contains(['\\', ':', '\0']) || path.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

/// `createZip`: um ZIP não comprimido (método 0) para arquivos em memória, sem ZIP64. As mensagens de erro
/// são as do original.
pub fn create_zip(files: &[ZipFile]) -> Result<VSBuffer, Error> {
    if files.len() >= 0xffff {
        return Err(Error::new("Too many files for a ZIP archive without ZIP64 support."));
    }

    let mut paths = std::collections::HashSet::new();
    let mut total_length: u64 = 22;
    let mut entries = Vec::with_capacity(files.len());
    for file in files {
        if is_unsafe_path(&file.path) {
            return Err(Error::new("ZIP entries must have relative file paths."));
        }
        if !paths.insert(file.path.as_str()) {
            return Err(Error::new("ZIP entries must have unique paths."));
        }
        let name = file.path.as_bytes();
        if name.len() > 0xffff {
            return Err(Error::new("ZIP entry path is too long."));
        }
        total_length += 30 + 46 + 2 * name.len() as u64 + file.contents.byte_length() as u64;
        if total_length >= 0xffff_ffff {
            return Err(Error::new("ZIP archive is too large without ZIP64 support."));
        }
        entries.push((name, file.contents.to_vec()));
    }

    let now = Timestamp::from_millisecond(runtime::date_now() as i64).expect("Date.now() cabe no intervalo do jiff").to_zoned(local_time_zone());
    let year = i32::from(now.year()).clamp(1980, 2107) as u32;
    let modified_date = ((year - 1980) << 9) | ((now.month() as u32) << 5) | now.day() as u32;
    let modified_time = ((now.hour() as u32) << 11) | ((now.minute() as u32) << 5) | (now.second() as u32 >> 1);

    let mut local_files: Vec<u8> = Vec::new();
    let mut central_directory: Vec<u8> = Vec::new();
    let mut offset: u32 = 0;
    for (name, contents) in &entries {
        let mut header = [0u8; 30];
        set_u32(&mut header, 0x0403_4b50, 0);
        set_u16(&mut header, 20, 4);
        set_u16(&mut header, 0x0800, 6); // UTF-8 filenames.
        set_u16(&mut header, modified_time, 10);
        set_u16(&mut header, modified_date, 12);
        set_u32(&mut header, crc32(contents), 14);
        set_u32(&mut header, contents.len() as u32, 18);
        set_u32(&mut header, contents.len() as u32, 22);
        set_u16(&mut header, name.len() as u32, 26);
        local_files.extend_from_slice(&header);
        local_files.extend_from_slice(name);
        local_files.extend_from_slice(contents);

        let mut central_header = [0u8; 46];
        set_u32(&mut central_header, 0x0201_4b50, 0);
        set_u16(&mut central_header, 20, 4);
        central_header[6..30].copy_from_slice(&header[4..28]);
        set_u32(&mut central_header, offset, 42);
        central_directory.extend_from_slice(&central_header);
        central_directory.extend_from_slice(name);
        offset += (header.len() + name.len() + contents.len()) as u32;
    }

    let mut end = [0u8; 22];
    set_u32(&mut end, 0x0605_4b50, 0);
    set_u16(&mut end, files.len() as u32, 8);
    set_u16(&mut end, files.len() as u32, 10);
    set_u32(&mut end, central_directory.len() as u32, 12);
    set_u32(&mut end, offset, 16);

    let mut archive = local_files;
    archive.extend_from_slice(&central_directory);
    archive.extend_from_slice(&end);
    Ok(VSBuffer::wrap(archive))
}
