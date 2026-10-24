//! WOFF 1.0 para sfnt (TrueType/OpenType), o que o Chromium faz ao carregar um `@font-face` `format('woff')`.
//! O tema de ícones Seti do VS Code (`extensions/theme-seti/icons/seti.woff`) chega assim, e o `fontique` só lê
//! sfnt. Formato em <https://www.w3.org/TR/WOFF/>: cabeçalho de 44 bytes, um diretório de 20 bytes por tabela
//! e cada tabela comprimida com zlib quando `compLength < origLength`.

const HEADER: usize = 44;
const DIRECTORY_ENTRY: usize = 20;
const SFNT_RECORD: usize = 16;

fn read_u32(data: &[u8], at: usize) -> Result<u32, String> {
    data.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| "WOFF truncado".to_owned())
}

fn read_u16(data: &[u8], at: usize) -> Result<u16, String> {
    data.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).ok_or_else(|| "WOFF truncado".to_owned())
}

/// Converte um WOFF 1.0 no sfnt equivalente.
pub fn woff_to_sfnt(data: &[u8]) -> Result<Vec<u8>, String> {
    if read_u32(data, 0)? != u32::from_be_bytes(*b"wOFF") {
        return Err("não é WOFF 1.0".to_owned());
    }
    let flavor = read_u32(data, 4)?;
    let num_tables = read_u16(data, 12)?;
    let count = usize::from(num_tables);
    // O cabeçalho do sfnt: searchRange é a maior potência de 2 que cabe em numTables, vezes 16.
    let entry_selector = if count == 0 { 0 } else { count.ilog2() as u16 };
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = num_tables * 16 - search_range;
    let mut out = Vec::new();
    out.extend_from_slice(&flavor.to_be_bytes());
    for v in [num_tables, search_range, entry_selector, range_shift] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut tables = Vec::with_capacity(count);
    for i in 0..count {
        let at = HEADER + i * DIRECTORY_ENTRY;
        let (tag, offset, comp_length, orig_length, checksum) = (read_u32(data, at)?, read_u32(data, at + 4)? as usize, read_u32(data, at + 8)? as usize, read_u32(data, at + 12)? as usize, read_u32(data, at + 16)?);
        let raw = data.get(offset..offset + comp_length).ok_or_else(|| "tabela WOFF fora do arquivo".to_owned())?;
        let table = if comp_length < orig_length {
            miniz_oxide::inflate::decompress_to_vec_zlib(raw).map_err(|e| format!("tabela WOFF corrompida: {e:?}"))?
        } else {
            raw.to_vec()
        };
        if table.len() != orig_length {
            return Err("tabela WOFF com tamanho errado".to_owned());
        }
        tables.push((tag, checksum, table));
    }
    let mut offset = 12 + count * SFNT_RECORD;
    for (tag, checksum, table) in &tables {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        offset += table.len().next_multiple_of(4);
    }
    for (_, _, table) in &tables {
        out.extend_from_slice(table);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Ok(out)
}
