//! A análise do corpo de `Blob.prototype.formData()`, portada de `bun_core::form_data` (`Encoding::get`,
//! `get_boundary`) e de `FormData.rs` (`for_each_multipart_entry`) do bun. Medido no bun 1.4.2
//! (`scripts/gen-blob-golden.js`):
//!
//! - corpo vazio vira `FormData` vazio com qualquer `type`; senão o `type` (já em minúsculas, pelo `Blob`) tem de
//!   CONTER `application/x-www-form-urlencoded` (em qualquer posição) ou `multipart/form-data` com parâmetro `boundary`;
//!   o resto é `Invalid encoding`;
//! - urlencoded: BOM UTF-8 removido, o `?` inicial NÃO é removido (diferente de `URLSearchParams`);
//! - multipart: o último `--boundary--` tem de existir (`missing final boundary`), `--boundary` tem no máximo 72 bytes
//!   (`boundary is too long`); parte sem `name` é descartada; parte com `filename` vira arquivo.
//!
//! O arquivo (parte com `filename`) vira `Entry::File`; o tipo, sem `Content-Type` antes do `Content-Disposition`
//! (depois dele o cabeçalho nunca é lido), vem da extensão (`mime_for`, tabela do bun) ou do conteúdo (`sniff`: bmp, jpeg, tiff, gif89a, png).

use crate::runtime::url_search_params::parse_query;
use crate::runtime::web_iterable::Units;

/// Uma entrada do corpo.
pub(crate) enum Entry {
    Text(Units, Units),
    File { name: Units, filename: Units, content_type: Vec<u8>, bytes: Vec<u8> },
}

enum Encoding {
    UrlEncoded,
    Multipart(Vec<u8>),
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn contains_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window.eq_ignore_ascii_case(needle))
}

fn trim<'a>(mut bytes: &'a [u8], set: &[u8]) -> &'a [u8] {
    while let [first, rest @ ..] = bytes {
        if !set.contains(first) {
            break;
        }
        bytes = rest;
    }
    while let [rest @ .., last] = bytes {
        if !set.contains(last) {
            break;
        }
        bytes = rest;
    }
    bytes
}

/// O próximo `;` fora de aspas (a `\` escapa o byte seguinte dentro delas).
fn unquoted_semicolon(bytes: &[u8]) -> Option<usize> {
    let mut quoted = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => quoted = !quoted,
            b'\\' if quoted => index += 1,
            b';' if !quoted => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

fn boundary_of(content_type: &[u8]) -> Option<Vec<u8>> {
    let mut rest = content_type;
    loop {
        rest = &rest[unquoted_semicolon(rest)? + 1..];
        let param = trim_start(rest, b" \t");
        let Some(equals) = param.iter().position(|&byte| byte == b'=') else { continue };
        if !param[..equals].eq_ignore_ascii_case(b"boundary") {
            continue;
        }
        let begin = &param[equals + 1..];
        if begin.is_empty() {
            return None;
        }
        let end = begin.iter().position(|&byte| byte == b';').unwrap_or(begin.len());
        if begin[0] == b'"' {
            return (end > 1 && begin[end - 1] == b'"').then(|| begin[1..end - 1].to_vec());
        }
        return Some(begin[..end].to_vec());
    }
}

fn trim_start<'a>(bytes: &'a [u8], set: &[u8]) -> &'a [u8] {
    let skipped = bytes.iter().take_while(|byte| set.contains(byte)).count();
    &bytes[skipped..]
}

fn encoding_of(content_type: &[u8]) -> Option<Encoding> {
    if contains_ci(content_type, b"application/x-www-form-urlencoded") {
        return Some(Encoding::UrlEncoded);
    }
    if !contains_ci(content_type, b"multipart/form-data") {
        return None;
    }
    boundary_of(content_type).map(Encoding::Multipart)
}

fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

fn utf16(bytes: &[u8]) -> Units {
    String::from_utf8_lossy(bytes).encode_utf16().collect()
}

/// O valor de um parâmetro de `Content-Disposition`: devolve `(valor, resto)`.
fn parameter_value(value: &[u8]) -> (&[u8], &[u8]) {
    let value = value.strip_prefix(b"\"").unwrap_or(value);
    let mut end = 0;
    while end < value.len() {
        match value[end] {
            b'"' => break,
            b'\\' => end += usize::from(value.get(end + 1) == Some(&b'"')),
            _ => {}
        }
        end += 1;
    }
    (&value[..end.min(value.len())], &value[(end + 1).min(value.len())..])
}

/// Os campos de `Content-Disposition` (`name` e `filename`) do valor do cabeçalho.
fn read_disposition(value: &[u8], name: &mut Vec<u8>, filename: &mut Option<Vec<u8>>) {
    let mut value = trim(value, b" \t");
    if value.len() >= 10 && value[..10].eq_ignore_ascii_case(b"form-data;") {
        value = trim(&value[10..], b" \t");
    }
    while let Some(equals) = value.iter().position(|&byte| byte == b'=') {
        let key = trim(&value[..equals], b" \t;");
        let (field, rest) = parameter_value(&value[equals + 1..]);
        value = rest;
        if key.eq_ignore_ascii_case(b"name") {
            *name = field.to_vec();
        } else if key.eq_ignore_ascii_case(b"filename") {
            *filename = Some(field.to_vec());
        }
        if !name.is_empty() && filename.is_some() {
            break;
        }
        match value.iter().position(|&byte| byte == b';') {
            Some(semicolon) => value = &value[semicolon + 1..],
            None => break,
        }
    }
}

fn multipart_entries(input: &[u8], boundary: &[u8]) -> Result<Vec<Entry>, String> {
    if boundary.len() + 4 > 76 {
        return Err("boundary is too long".into());
    }
    let final_boundary = [b"--", boundary, b"--"].concat();
    let end = input.windows(final_boundary.len()).rposition(|window| window == final_boundary).ok_or("missing final boundary")?;
    let separator = [b"--", boundary, b"\r\n"].concat();
    let mut entries = Vec::new();
    let mut chunks = split(&input[..end], &separator).into_iter();
    chunks.next();
    for chunk in chunks {
        let header_end = find(chunk, b"\r\n\r\n").ok_or("is missing header end")?;
        let mut header = &chunk[..header_end + 2];
        let remain = &chunk[header_end + 4..];
        let mut name = Vec::new();
        let mut filename: Option<Vec<u8>> = None;
        let mut content_type: Vec<u8> = Vec::new();
        while !header.is_empty() && (filename.is_none() || name.is_empty()) {
            let line_end = find(header, b"\r\n").ok_or("is missing header line end")?;
            let line = &header[..line_end];
            header = &header[line_end + 2..];
            let colon = line.iter().position(|&byte| byte == b':').ok_or("is missing header colon separator")?;
            let (key, value) = (&line[..colon], &line[colon + 1..]);
            if key.eq_ignore_ascii_case(b"content-disposition") {
                read_disposition(value, &mut name, &mut filename);
            } else if !value.is_empty() && content_type.is_empty() && key.eq_ignore_ascii_case(b"content-type") {
                let trimmed = trim(value, b"; \t");
                if trimmed.iter().all(|&byte| byte == b'\t' || (0x20..=0x7e).contains(&byte)) {
                    content_type = trimmed.to_vec();
                }
            }
        }
        if name.is_empty() {
            continue;
        }
        let body = remain.strip_suffix(b"\r\n").unwrap_or(remain);
        entries.push(match filename {
            Some(filename) => {
                let content_type = if content_type.is_empty() { mime_for(&filename, body) } else { content_type };
                Entry::File { name: utf16(&name), filename: utf16(&filename), content_type, bytes: body.to_vec() }
            }
            None => Entry::Text(utf16(&name), utf16(without_bom(body))),
        });
    }
    Ok(entries)
}

/// O tipo de um arquivo sem `Content-Type` antes do `Content-Disposition`: pela extensão do nome (tabela inteira do bun,
/// `mime_table`), senão pelo conteúdo (`sniff`).
fn mime_for(filename: &[u8], body: &[u8]) -> Vec<u8> {
    let extension = path_extension(filename);
    let extension = extension.strip_prefix(b".").unwrap_or(extension);
    match super::mime_table::by_extension(extension) {
        Some(known) => known.to_vec(),
        None => sniff(body).map(<[u8]>::to_vec).unwrap_or_default(),
    }
}

/// Port de `bun_core::strings::basename_posix` (barras finais saem, `\` não é separador; vazio ou só `/` dá vazio).
fn basename_posix(path: &[u8]) -> &[u8] {
    let end = path.iter().rposition(|&byte| byte != b'/').map_or(0, |last| last + 1);
    let start = path[..end].iter().rposition(|&byte| byte == b'/').map_or(0, |slash| slash + 1);
    &path[start..end]
}

/// Port de `bun_paths::extension`: a extensão do basename com o ponto; sem ponto, ou só com o ponto no índice 0 (dotfile),
/// é vazia.
fn path_extension(path: &[u8]) -> &[u8] {
    let name = basename_posix(path);
    match name.iter().rposition(|&byte| byte == b'.') {
        Some(dot) if dot > 0 => &name[dot..],
        _ => &[],
    }
}

/// Port de `MimeType::sniff` e `IMAGES_HEADERS` (`src/http_types/MimeType.rs` do bun): menos de 2 bytes não é farejado; o
/// primeiro cabeçalho que for prefixo vence. O de GIF é `GIF89a` inteiro (`GIF87a` não casa).
fn sniff(bytes: &[u8]) -> Option<&'static [u8]> {
    const IMAGES_HEADERS: &[(&[u8], &[u8])] = &[
        (&[0x42, 0x4d], b"image/bmp"),
        (&[0xff, 0xd8, 0xff], b"image/jpeg"),
        (&[0x49, 0x49, 0x2a, 0x00], b"image/tiff"),
        (&[0x4d, 0x4d, 0x00, 0x2a], b"image/tiff"),
        (&[0x47, 0x49, 0x46, 0x38, 0x39, 0x61], b"image/gif"),
        (&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], b"image/png"),
    ];
    if bytes.len() < 2 {
        return None;
    }
    IMAGES_HEADERS.iter().find(|(header, _)| bytes.starts_with(header)).map(|&(_, mime)| mime)
}

fn split<'a>(mut bytes: &'a [u8], separator: &[u8]) -> Vec<&'a [u8]> {
    let mut chunks = Vec::new();
    while let Some(at) = find(bytes, separator) {
        chunks.push(&bytes[..at]);
        bytes = &bytes[at + separator.len()..];
    }
    chunks.push(bytes);
    chunks
}

/// O `Content-Type` serve a `formData()`: `urlencoded`, ou `multipart` com `boundary`.
pub(crate) fn has_form_encoding(content_type: &[u8]) -> bool {
    encoding_of(content_type).is_some()
}

/// As entradas do corpo de um Blob com este `type`; `Err(mensagem)` já é o texto do erro do bun (o multipart com o
/// prefixo `FormData encoding failed: `).
pub(crate) fn parse_body(bytes: &[u8], content_type: &[u8]) -> Result<Vec<Entry>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    parse_encoded(bytes, encoding_of(content_type).ok_or("Invalid encoding")?).map_err(|message| format!("FormData encoding failed: {message}"))
}

/// As entradas de `bytes` com a fronteira dada (`FormData.from`): sem fronteira, o corpo é `urlencoded`. O erro é o
/// motivo cru (medido no bun 1.4.2: `missing final boundary while parsing FormData`, sem o prefixo do `Blob`).
pub(crate) fn parse_with_boundary(bytes: &[u8], boundary: Option<Vec<u8>>) -> Result<Vec<Entry>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    parse_encoded(bytes, boundary.map_or(Encoding::UrlEncoded, Encoding::Multipart))
}

/// O urlencoded nunca falha; o erro é o motivo do multipart, sem prefixo.
fn parse_encoded(bytes: &[u8], encoding: Encoding) -> Result<Vec<Entry>, String> {
    match encoding {
        Encoding::UrlEncoded => Ok(parse_query(&utf16(without_bom(bytes))).into_iter().map(|(name, value)| Entry::Text(name, value)).collect()),
        Encoding::Multipart(boundary) => multipart_entries(bytes, &boundary),
    }
}
