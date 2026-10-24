//! Cabeçalho de quadro: `ZSTD_getFrameHeader_advanced` (libzstd 1.5.7).

use super::Error;

pub const MAGIC: u32 = 0xFD2F_B528;
pub const SKIPPABLE_MASK: u32 = 0xFFFF_FFF0;
pub const SKIPPABLE_START: u32 = 0x184D_2A50;
/// `ZSTD_WINDOWLOG_LIMIT_DEFAULT`: teto de janela do `ZSTD_decompressStream` sem ajuste do chamador.
pub const WINDOW_LOG_LIMIT_DEFAULT: u32 = 27;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub header_size: usize,
    pub window_size: u64,
    pub content_size: Option<u64>,
    pub checksum: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// Faltam bytes: o total necessário para decidir.
    Need(usize),
    Skippable { header_size: usize, skip: u32 },
    Frame(FrameHeader),
}

/// Se `head` (1 a 4 bytes) é prefixo do número mágico de um quadro zstd ou skippable (o teste do bun para o que
/// vem depois de um quadro completo: o que não é prefixo é lixo no fim).
pub fn is_frame_prefix(head: &[u8]) -> bool {
    const SKIPPABLE_TAIL: [u8; 3] = [0x2A, 0x4D, 0x18];
    head.iter().zip(MAGIC.to_le_bytes()).all(|(a, b)| *a == b)
        || (head.first().is_some_and(|first| first & 0xF0 == 0x50) && head[1..].iter().zip(SKIPPABLE_TAIL).all(|(a, b)| *a == b))
}

pub fn parse(src: &[u8]) -> Result<Parsed, Error> {
    if src.len() < 4 {
        return Ok(Parsed::Need(4));
    }
    let magic = u32::from_le_bytes([src[0], src[1], src[2], src[3]]);
    if magic & SKIPPABLE_MASK == SKIPPABLE_START {
        if src.len() < 8 {
            return Ok(Parsed::Need(8));
        }
        return Ok(Parsed::Skippable { header_size: 8, skip: u32::from_le_bytes([src[4], src[5], src[6], src[7]]) });
    }
    if magic != MAGIC {
        return Err(Error::PrefixUnknown);
    }
    let fhd = match src.get(4) {
        Some(&b) => b,
        None => return Ok(Parsed::Need(5)),
    };
    let dict_id_size = [0usize, 1, 2, 4][(fhd & 3) as usize];
    let checksum = (fhd >> 2) & 1 == 1;
    if (fhd >> 3) & 1 != 0 {
        return Err(Error::FrameParameterUnsupported);
    }
    let single_segment = (fhd >> 5) & 1 == 1;
    let fcs_id = fhd >> 6;
    let window_desc_size = usize::from(!single_segment);
    let fcs_size = match fcs_id {
        0 => usize::from(single_segment),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let header_size = 5 + window_desc_size + dict_id_size + fcs_size;
    if src.len() < header_size {
        return Ok(Parsed::Need(header_size));
    }
    let mut at = 5;
    let mut window_size = 0u64;
    if !single_segment {
        let wd = src[at];
        at += 1;
        let window_log = (wd >> 3) as u32 + 10;
        if window_log > WINDOW_LOG_LIMIT_DEFAULT {
            return Err(Error::FrameParameterWindowTooLarge);
        }
        let base = 1u64 << window_log;
        window_size = base + (base >> 3) * (wd & 7) as u64;
    }
    at += dict_id_size;
    let mut content_size = None;
    if fcs_size > 0 {
        let mut v = 0u64;
        for i in 0..fcs_size {
            v |= (src[at + i] as u64) << (8 * i);
        }
        if fcs_size == 2 {
            v += 256;
        }
        content_size = Some(v);
    }
    if single_segment {
        window_size = content_size.unwrap_or(0);
        if window_size > 1u64 << WINDOW_LOG_LIMIT_DEFAULT {
            return Err(Error::FrameParameterWindowTooLarge);
        }
    }
    Ok(Parsed::Frame(FrameHeader { header_size, window_size, content_size, checksum }))
}
