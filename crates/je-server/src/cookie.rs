//! Porte do `parse` do pacote `cookie` do npm (0.7), que `serverConnectionToken.ts` usa para ler o cabeçalho `Cookie`.
//!
//! Divergências:
//! - O resultado é uma lista de pares na ordem em que as chaves aparecem pela primeira vez (o objeto do JavaScript, sem
//!   chaves numéricas aqui, as mantém nessa ordem); a primeira ocorrência de uma chave vence, como o `hasOwnProperty`.
//! - O texto é percorrido em unidades UTF-16, como o `charCodeAt` e o `slice` do original, e o `decodeURIComponent` é o
//!   de `je_base::uri`; um valor que não decodifica fica como está (`tryDecode`).
//! - As opções (`decode` próprio) não existem: ninguém no servidor as passa.

use je_base::uri::decode_uri_component;

const SPACE: u16 = 0x20;
const TAB: u16 = 0x09;
const QUOTE: u16 = 0x22;

fn index_of(units: &[u16], needle: u16, from: usize) -> Option<usize> {
    units.get(from..)?.iter().position(|&unit| unit == needle).map(|position| position + from)
}

fn last_index_of(units: &[u16], needle: u16, from: usize) -> Option<usize> {
    units.get(..=from.min(units.len().saturating_sub(1)))?.iter().rposition(|&unit| unit == needle)
}

/// `startIndex(str, index, max)`: o primeiro índice a partir de `index` que não é espaço nem tabulação (ou `max`).
fn start_index(units: &[u16], mut index: usize, max: usize) -> usize {
    loop {
        let code = units.get(index).copied();
        if code != Some(SPACE) && code != Some(TAB) {
            return index;
        }
        index += 1;
        if index >= max {
            return max;
        }
    }
}

/// `endIndex(str, index, min)`: um depois do último caractere que não é espaço nem tabulação, indo de `index` para trás
/// até `min`.
fn end_index(units: &[u16], mut index: usize, min: usize) -> usize {
    while index > min {
        index -= 1;
        let code = units[index];
        if code != SPACE && code != TAB {
            return index + 1;
        }
    }
    min
}

/// `decode(str)`: `decodeURIComponent` só quando há `%`.
fn decode(units: &[u16]) -> Result<Vec<u16>, je_base::errors::Error> {
    if units.contains(&u16::from(b'%')) { decode_uri_component(units) } else { Ok(units.to_vec()) }
}

/// `cookie.parse(str)`.
pub fn parse(header: &str) -> Vec<(String, String)> {
    let units: Vec<u16> = header.encode_utf16().collect();
    let len = units.len();
    let mut result: Vec<(String, String)> = Vec::new();
    if len < 2 {
        return result;
    }

    let mut index = 0;
    loop {
        let Some(eq_idx) = index_of(&units, u16::from(b'='), index) else {
            break;
        };

        let mut end_idx = match index_of(&units, u16::from(b';'), index) {
            Some(end_idx) => end_idx,
            None => len,
        };
        if end_idx != len && eq_idx > end_idx {
            // o `;` vem antes do `=`: este trecho não tem par, recomeça depois do último `;` antes do `=`
            index = last_index_of(&units, u16::from(b';'), eq_idx - 1).map_or(0, |last| last + 1);
            if index >= len {
                break;
            }
            continue;
        }
        if end_idx == usize::MAX {
            end_idx = len;
        }

        let key_start_idx = start_index(&units, index, eq_idx);
        let key_end_idx = end_index(&units, eq_idx, key_start_idx);
        let key = String::from_utf16_lossy(&units[key_start_idx..key_end_idx]);

        if !result.iter().any(|(existing, _)| *existing == key) {
            let mut val_start_idx = start_index(&units, eq_idx + 1, end_idx);
            let mut val_end_idx = end_index(&units, end_idx, val_start_idx);

            if units.get(val_start_idx) == Some(&QUOTE) && val_end_idx > 0 && units.get(val_end_idx - 1) == Some(&QUOTE) {
                val_start_idx += 1;
                val_end_idx = val_end_idx.saturating_sub(1);
            }

            let value = &units[val_start_idx.min(val_end_idx)..val_end_idx];
            // `tryDecode`: um valor que não decodifica fica como está
            let decoded = decode(value).unwrap_or_else(|_| value.to_vec());
            result.push((key, String::from_utf16_lossy(&decoded)));
        }

        index = end_idx + 1;
        if index >= len {
            break;
        }
    }

    result
}

/// `cookies[name]`: o valor da primeira ocorrência do nome.
pub fn get<'a>(cookies: &'a [(String, String)], name: &str) -> Option<&'a str> {
    cookies.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}
