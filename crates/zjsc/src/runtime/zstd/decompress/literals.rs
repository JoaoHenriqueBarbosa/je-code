//! Seção de literais de um bloco comprimido: `ZSTD_decodeLiteralsBlock` (libzstd 1.5.7).

use super::huf::{self, HufTable};
use super::Error;

pub const BLOCK_SIZE_MAX: usize = 128 * 1024;

/// Literais decodificados e quantos bytes da fonte a seção ocupou.
pub struct Literals {
    pub data: Vec<u8>,
    pub consumed: usize,
}

/// Decodifica a seção de literais. `prev_table` é a tabela do último bloco que a trouxe (modo
/// `set_repeat`); `block_size_max` é o teto de `litSize`. Atualiza `prev_table` quando o bloco traz tabela.
pub fn decode(src: &[u8], block_size_max: usize, prev_table: &mut Option<HufTable>) -> Result<Literals, Error> {
    let b0 = *src.first().ok_or(Error::Corruption)?;
    let kind = b0 & 3;
    let size_format = (b0 >> 2) & 3;
    let need = |n: usize| if src.len() < n { Err(Error::Corruption) } else { Ok(()) };
    match kind {
        0 | 1 => {
            let (header, lit_size) = match size_format {
                0 | 2 => (1, (b0 >> 3) as usize),
                1 => {
                    need(2)?;
                    (2, (u16::from_le_bytes([src[0], src[1]]) >> 4) as usize)
                }
                _ => {
                    need(3)?;
                    (3, (u32::from_le_bytes([src[0], src[1], src[2], 0]) >> 4) as usize)
                }
            };
            if lit_size > block_size_max {
                return Err(Error::Corruption);
            }
            if kind == 0 {
                need(header + lit_size)?;
                Ok(Literals { data: src[header..header + lit_size].to_vec(), consumed: header + lit_size })
            } else {
                need(header + 1)?;
                Ok(Literals { data: vec![src[header]; lit_size], consumed: header + 1 })
            }
        }
        _ => {
            let single_stream = size_format == 0;
            let (header, lit_size, comp_size) = match size_format {
                0 | 1 => {
                    need(3)?;
                    let lhc = u32::from_le_bytes([src[0], src[1], src[2], 0]);
                    (3, ((lhc >> 4) & 0x3FF) as usize, ((lhc >> 14) & 0x3FF) as usize)
                }
                2 => {
                    need(4)?;
                    let lhc = u32::from_le_bytes([src[0], src[1], src[2], src[3]]);
                    (4, ((lhc >> 4) & 0x3FFF) as usize, (lhc >> 18) as usize)
                }
                _ => {
                    need(5)?;
                    let lhc = u32::from_le_bytes([src[0], src[1], src[2], src[3]]);
                    (5, ((lhc >> 4) & 0x3FFFF) as usize, (lhc >> 22) as usize + ((src[4] as usize) << 10))
                }
            };
            if lit_size > block_size_max || comp_size + header > src.len() {
                return Err(Error::Corruption);
            }
            let body = &src[header..header + comp_size];
            let stream = if kind == 2 {
                let (table, used) = huf::read_table(body)?;
                *prev_table = Some(table);
                &body[used..]
            } else {
                body
            };
            let table = prev_table.as_ref().ok_or(Error::DictionaryCorrupted)?;
            let mut data = Vec::with_capacity(lit_size);
            if single_stream {
                table.decode_1x(stream, lit_size, &mut data)?;
            } else {
                table.decode_4x(stream, lit_size, &mut data)?;
            }
            Ok(Literals { data, consumed: header + comp_size })
        }
    }
}
