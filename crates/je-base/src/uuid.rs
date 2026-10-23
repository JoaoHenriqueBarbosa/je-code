//! Porte de `src/vs/base/common/uuid.ts`.
//!
//! `generateUuid` usa `crypto.randomUUID` quando existe e, senão, 16 bytes de `getRandomValues` com
//! os bits de versão 4 e variante RFC 4122; os dois produzem o mesmo formato, e aqui só o segundo
//! caminho existe (os bytes vêm de [`crate::random::fill_bytes`]).

use crate::random::fill_bytes;

/// `/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i`.
pub fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { *b == b'-' } else { b.is_ascii_hexdigit() })
}

/// Um UUID v4 aleatório, em minúsculas e com hifens.
pub fn generate_uuid() -> String {
    let mut data = [0u8; 16];
    fill_bytes(&mut data);
    // bits de versão e de variante
    data[6] = (data[6] & 0x0f) | 0x40;
    data[8] = (data[8] & 0x3f) | 0x80;
    let mut result = String::with_capacity(36);
    for (i, byte) in data.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            result.push('-');
        }
        result.push_str(&format!("{byte:02x}"));
    }
    result
}

/// `namespace` deve ter 3 letras, por exemplo `abc-<uuid>`.
pub fn prefixed_uuid(namespace: &str) -> String {
    format!("{namespace}-{}", generate_uuid())
}
