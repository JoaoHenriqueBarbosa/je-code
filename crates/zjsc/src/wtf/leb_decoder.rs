//! Tradução de `WTF/wtf/LEBDecoder.h`: decodificação de números LEB128 (usada pelo parser de
//! módulos WebAssembly). Como no C++, `offset` avança mesmo quando a decodificação falha.

/// `maxByteLength<T>()` para um inteiro de `bits` bits: `(numBits - 1) / 7 + 1`.
pub const fn max_byte_length(bits: u32) -> usize {
    ((bits - 1) / 7 + 1) as usize
}

/// `lastByteMask<T>()`: `~((1U << (numBits % 7)) - 1)`. Exige `bits % 7 != 0`, como o `static_assert`.
const fn last_byte_mask(bits: u32) -> u32 {
    !((1u32 << (bits % 7)) - 1)
}

/// `decodeUInt<T>` para `T` de `bits` bits (32 ou 64); o resultado cabe em `bits` bits.
fn decode_uint(bytes: &[u8], offset: &mut usize, bits: u32) -> Option<u64> {
    if bytes.len() <= *offset {
        return None;
    }
    let max_len = max_byte_length(bits);
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    let last = max_len.min(bytes.len() - *offset) - 1;
    let mut i = 0usize;
    loop {
        let byte = bytes[*offset];
        *offset += 1;
        result |= u64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if max_len - 1 == i && u32::from(byte) & last_byte_mask(bits) != 0 {
                return None;
            }
            return Some(result);
        }
        if i == last {
            return None;
        }
        i += 1;
    }
}

/// `decodeInt<T>` para `T` de `bits` bits (32 ou 64); devolve o padrão de bits já estendido de
/// sinal para 64 bits.
fn decode_int(bytes: &[u8], offset: &mut usize, bits: u32) -> Option<u64> {
    if bytes.len() <= *offset {
        return None;
    }
    let max_len = max_byte_length(bits);
    let value_mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    let last = max_len.min(bytes.len() - *offset) - 1;
    let mut byte;
    let mut i = 0usize;
    loop {
        byte = bytes[*offset];
        *offset += 1;
        result |= (u64::from(byte & 0x7f) << shift) & value_mask;
        shift += 7;
        if byte & 0x80 == 0 {
            if max_len - 1 == i {
                let half = last_byte_mask(bits) >> 1;
                if byte & 0x40 == 0 {
                    // Positivo sem extensão de sinal: os bits restantes do último byte são zero.
                    if u32::from(byte) & half != 0 {
                        return None;
                    }
                } else if u32::from(!(byte | 0x80)) & half != 0 {
                    // Negativo: os bits restantes do último byte (menos o de cima) são um.
                    return None;
                }
            }
            break;
        }
        if i == last {
            return None;
        }
        i += 1;
    }
    if shift < bits && byte & 0x40 != 0 {
        result |= (u64::MAX << shift) & value_mask;
    }
    if bits == 32 {
        // Estende o sinal de 32 para 64 bits.
        return Some(i64::from(result as u32 as i32) as u64);
    }
    Some(result)
}

/// `decodeUInt32`.
pub fn decode_uint32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    decode_uint(bytes, offset, 32).map(|value| value as u32)
}

/// `decodeUInt64`.
pub fn decode_uint64(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    decode_uint(bytes, offset, 64)
}

/// `decodeInt32`.
pub fn decode_int32(bytes: &[u8], offset: &mut usize) -> Option<i32> {
    decode_int(bytes, offset, 32).map(|value| value as i32)
}

/// `decodeInt64`.
pub fn decode_int64(bytes: &[u8], offset: &mut usize) -> Option<i64> {
    decode_int(bytes, offset, 64).map(|value| value as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_values() {
        let mut offset = 0;
        assert_eq!(decode_uint32(&[0xe5, 0x8e, 0x26], &mut offset), Some(624485));
        assert_eq!(offset, 3);
        let mut offset = 0;
        assert_eq!(decode_uint32(&[0xff, 0xff, 0xff, 0xff, 0x0f], &mut offset), Some(u32::MAX));
        let mut offset = 0;
        // Bit sobrando no quinto byte.
        assert_eq!(decode_uint32(&[0xff, 0xff, 0xff, 0xff, 0x1f], &mut offset), None);
        let mut offset = 0;
        // Seis bytes: passa do comprimento máximo.
        assert_eq!(decode_uint32(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00], &mut offset), None);
        let mut offset = 0;
        assert_eq!(decode_uint32(&[], &mut offset), None);
    }

    #[test]
    fn signed_values() {
        let mut offset = 0;
        assert_eq!(decode_int32(&[0x7f], &mut offset), Some(-1));
        let mut offset = 0;
        assert_eq!(decode_int32(&[0xc0, 0xbb, 0x78], &mut offset), Some(-123456));
        let mut offset = 0;
        assert_eq!(decode_int32(&[0x80, 0x80, 0x80, 0x80, 0x78], &mut offset), Some(i32::MIN));
        let mut offset = 0;
        assert_eq!(decode_int32(&[0xff, 0xff, 0xff, 0xff, 0x07], &mut offset), Some(i32::MAX));
        let mut offset = 0;
        assert_eq!(decode_int32(&[0xff, 0xff, 0xff, 0xff, 0x0f], &mut offset), None);
        let mut offset = 0;
        assert_eq!(
            decode_int64(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7f], &mut offset),
            Some(i64::MIN)
        );
        let mut offset = 0;
        assert_eq!(decode_uint64(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01], &mut offset), Some(u64::MAX));
    }
}
