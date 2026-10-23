//! Porte de `src/vs/base/common/image.ts`: as dimensões de uma imagem lidas dos bytes do cabeçalho.
//!
//! Reconhece JPEG, PNG, GIF e WebP (com perda, sem perda e estendido) pelo número mágico.

use crate::buffer::VSBuffer;

/// `IImageDimensions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IImageDimensions {
    pub width: u32,
    pub height: u32,
}

/// `readImageDimensions`: `None` se o buffer não começa com uma assinatura conhecida ou se as dimensões não
/// puderam ser lidas.
pub fn read_image_dimensions(buffer: &VSBuffer) -> Option<IImageDimensions> {
    buffer.with_bytes(read_image_dimensions_of_bytes)
}

/// [`read_image_dimensions`] sobre bytes soltos.
pub fn read_image_dimensions_of_bytes(bytes: &[u8]) -> Option<IImageDimensions> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes[0] == 0xFF && bytes[1] == 0xD8 {
        return read_jpeg_dimensions(bytes);
    }
    if bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return read_png_dimensions(bytes);
    }
    if bytes[..4] == [0x47, 0x49, 0x46, 0x38] && (bytes[4] == 0x37 || bytes[4] == 0x39) && bytes[5] == 0x61 {
        return read_gif_dimensions(bytes);
    }
    if bytes[..4] == [0x52, 0x49, 0x46, 0x46] && bytes[8..12] == [0x57, 0x45, 0x42, 0x50] {
        return read_webp_dimensions(bytes);
    }
    None
}

fn read_jpeg_dimensions(bytes: &[u8]) -> Option<IImageDimensions> {
    let mut i = 2;
    while i + 9 < bytes.len() {
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let marker = bytes[i];
        i += 1;
        if marker == 0xD8 || marker == 0xD9 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if i + 1 >= bytes.len() {
            return None;
        }
        let seg_length = (usize::from(bytes[i]) << 8) | usize::from(bytes[i + 1]);
        if seg_length < 2 {
            return None;
        }
        if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
            if i + 6 >= bytes.len() {
                return None;
            }
            let height = (u32::from(bytes[i + 3]) << 8) | u32::from(bytes[i + 4]);
            let width = (u32::from(bytes[i + 5]) << 8) | u32::from(bytes[i + 6]);
            return Some(IImageDimensions { width, height });
        }
        i += seg_length;
    }
    None
}

fn read_png_dimensions(bytes: &[u8]) -> Option<IImageDimensions> {
    if bytes.len() < 24 || bytes[12..16] != [0x49, 0x48, 0x44, 0x52] {
        return None;
    }
    let be = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    Some(IImageDimensions { width: be(16), height: be(20) })
}

fn read_gif_dimensions(bytes: &[u8]) -> Option<IImageDimensions> {
    if bytes.len() < 10 {
        return None;
    }
    let width = u32::from(bytes[6]) | (u32::from(bytes[7]) << 8);
    let height = u32::from(bytes[8]) | (u32::from(bytes[9]) << 8);
    Some(IImageDimensions { width, height })
}

fn read_webp_dimensions(bytes: &[u8]) -> Option<IImageDimensions> {
    if bytes.len() < 30 {
        return None;
    }
    let chunk_size = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
    if chunk_size > bytes.len() - 20 {
        return None;
    }
    let b = |i: usize| u32::from(bytes[i]);
    match &bytes[12..16] {
        // "VP8 ": lossy.
        [0x56, 0x50, 0x38, 0x20] => {
            if chunk_size < 10 || bytes[23] != 0x9D || bytes[24] != 0x01 || bytes[25] != 0x2A {
                return None;
            }
            Some(IImageDimensions { width: (b(26) | (b(27) << 8)) & 0x3FFF, height: (b(28) | (b(29) << 8)) & 0x3FFF })
        }
        // "VP8L": lossless.
        [0x56, 0x50, 0x38, 0x4C] => {
            if chunk_size < 5 || bytes[20] != 0x2F {
                return None;
            }
            let width = ((b(21) | (b(22) << 8)) & 0x3FFF) + 1;
            let height = (((b(22) >> 6) | (b(23) << 2) | ((b(24) & 0x0F) << 10)) & 0x3FFF) + 1;
            Some(IImageDimensions { width, height })
        }
        // "VP8X": extended.
        [0x56, 0x50, 0x38, 0x58] => {
            if chunk_size < 10 {
                return None;
            }
            let width = ((b(24) | (b(25) << 8) | (b(26) << 16)) & 0xFF_FFFF) + 1;
            let height = ((b(27) | (b(28) << 8) | (b(29) << 16)) & 0xFF_FFFF) + 1;
            Some(IImageDimensions { width, height })
        }
        _ => None,
    }
}
