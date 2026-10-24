//! Cabeçalho do quadro (`ZSTD_writeFrameHeader`), cabeçalho de bloco e blocos raw e RLE.

pub const MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
/// `ZSTD_BLOCKSIZE_MAX`: 128 KiB.
pub const BLOCK_SIZE_MAX: usize = 1 << 17;

/// Tipos de bloco do formato (`blockType_e`).
#[derive(Clone, Copy)]
pub enum BlockType {
    Raw = 0,
    Rle = 1,
    Compressed = 2,
}

/// Escreve o cabeçalho do quadro sem checksum e sem dicionário.
/// `pledged` é o tamanho do conteúdo prometido (`None` quando desconhecido); `window_log` já ajustado.
/// Segmento único quando o tamanho é conhecido e cabe na janela, como no libzstd.
pub fn write_frame_header(out: &mut Vec<u8>, pledged: Option<u64>, window_log: u32) {
    let window_size = 1u64 << window_log;
    let single_segment = matches!(pledged, Some(n) if window_size >= n);
    let fcs_code: u8 = match pledged {
        None => 0,
        Some(n) => u8::from(n >= 256) + u8::from(n >= 65536 + 256) + u8::from(n >= 0xFFFF_FFFF),
    };
    out.extend_from_slice(&MAGIC);
    out.push((fcs_code << 6) | (u8::from(single_segment) << 5));
    if !single_segment {
        out.push(((window_log - 10) << 3) as u8);
    }
    if let Some(n) = pledged {
        match fcs_code {
            0 => {
                if single_segment {
                    out.push(n as u8);
                }
            }
            1 => out.extend_from_slice(&((n - 256) as u16).to_le_bytes()),
            2 => out.extend_from_slice(&(n as u32).to_le_bytes()),
            _ => out.extend_from_slice(&n.to_le_bytes()),
        }
    }
}

/// Cabeçalho de bloco de 3 bytes: último (bit 0), tipo (bits 1 e 2), tamanho (bits 3 a 23).
pub fn write_block_header(out: &mut Vec<u8>, last: bool, kind: BlockType, size: usize) {
    let header = u32::from(last) | ((kind as u32) << 1) | ((size as u32) << 3);
    out.extend_from_slice(&header.to_le_bytes()[..3]);
}

/// `ZSTD_noCompressBlock`.
pub fn write_raw_block(out: &mut Vec<u8>, block: &[u8], last: bool) {
    write_block_header(out, last, BlockType::Raw, block.len());
    out.extend_from_slice(block);
}

/// `ZSTD_rleCompressBlock`: o tamanho do cabeçalho é o do conteúdo, o corpo é um byte.
pub fn write_rle_block(out: &mut Vec<u8>, byte: u8, content_size: usize, last: bool) {
    write_block_header(out, last, BlockType::Rle, content_size);
    out.push(byte);
}

/// `ZSTD_isRLE`: o bloco inteiro é um só byte repetido (e tem ao menos um byte).
pub fn rle_byte(block: &[u8]) -> Option<u8> {
    let first = *block.first()?;
    block.iter().all(|&b| b == first).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(pledged: Option<u64>, wlog: u32) -> String {
        let mut v = Vec::new();
        write_frame_header(&mut v, pledged, wlog);
        v.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn matches_bun_vectors() {
        assert_eq!(header(None, 21), "28b52ffd0058");
        assert_eq!(header(Some(0), 10), "28b52ffd2000");
        let mut v = Vec::new();
        write_raw_block(&mut v, b"", true);
        assert_eq!(v, [1, 0, 0]);
        let mut v = Vec::new();
        write_raw_block(&mut v, b"a", true);
        assert_eq!(v, [9, 0, 0, 0x61]);
    }
}
