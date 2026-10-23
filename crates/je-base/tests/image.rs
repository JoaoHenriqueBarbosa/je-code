//! Porte de `src/vs/base/test/common/image.test.ts` (suíte `readImageDimensions`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: `fixtures` é uma lista de pares nome e entrada, e `deepStrictEqual` de dois objetos
//! montados com `Object.fromEntries` é a comparação das listas de resultados, com o nome na mensagem.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::VSBuffer;
use je_base::image::{IImageDimensions, read_image_dimensions};

fn buf(bytes: &[u8]) -> VSBuffer {
    VSBuffer::wrap(bytes.to_vec())
}

fn corrupt_byte(input: &VSBuffer, offset: usize, value: u8) -> VSBuffer {
    let mut copy = input.to_vec();
    copy[offset] = value;
    VSBuffer::wrap(copy)
}

/// Um JPEG mínimo: SOI, um APP0, um SOFn com as dimensões e EOI. Não decodifica, mas serve ao cabeçalho.
fn make_jpeg(width: u32, height: u32) -> VSBuffer {
    buf(&[
        0xFF, 0xD8, // SOI
        // APP0 (comprimento 4: o campo de comprimento e 2 bytes) para exercitar o salto de segmento
        0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, //
        // SOF0: marcador FF C0, comprimento 0x0011, precisão 8, altura (BE), largura (BE), 3 componentes
        0xFF, 0xC0, 0x00, 0x11, 0x08, ((height >> 8) & 0xFF) as u8, (height & 0xFF) as u8, ((width >> 8) & 0xFF) as u8, (width & 0xFF) as u8, //
        0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, //
        0xFF, 0xD9, // EOI
    ])
}

fn make_png(width: u32, height: u32) -> VSBuffer {
    let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52];
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[0x08, 0x06, 0x00, 0x00, 0x00]);
    buf(&bytes)
}

fn make_gif(width: u32, height: u32) -> VSBuffer {
    buf(&[0x47, 0x49, 0x46, 0x38, 0x39, 0x61, (width & 0xFF) as u8, ((width >> 8) & 0xFF) as u8, (height & 0xFF) as u8, ((height >> 8) & 0xFF) as u8, 0, 0, 0])
}

/// Um WebP/VP8 (com perda) mínimo, com dimensões de 14 bits.
fn make_web_p_vp8(width: u32, height: u32) -> VSBuffer {
    buf(&[
        0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, // "RIFF" <size> "WEBP"
        0x56, 0x50, 0x38, 0x20, 10, 0, 0, 0, // "VP8 " <chunkSize=10>
        0x00, 0x00, 0x00, // frame tag
        0x9D, 0x01, 0x2A, // start code
        (width & 0xFF) as u8, ((width >> 8) & 0x3F) as u8, (height & 0xFF) as u8, ((height >> 8) & 0x3F) as u8,
    ])
}

/// Um WebP/VP8L (sem perda) mínimo, com largura e altura de 14 bits e base 1.
fn make_web_p_vp8l(width: u32, height: u32) -> VSBuffer {
    let w = width - 1;
    let h = height - 1;
    let b21 = (w & 0xFF) as u8;
    let b22 = (((w >> 8) & 0x3F) | ((h & 0x03) << 6)) as u8;
    let b23 = ((h >> 2) & 0xFF) as u8;
    let b24 = ((h >> 10) & 0x0F) as u8;
    buf(&[
        0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, //
        0x56, 0x50, 0x38, 0x4C, 10, 0, 0, 0, // "VP8L" <chunkSize=10>
        0x2F, // assinatura do VP8L
        b21, b22, b23, b24, //
        0, 0, 0, 0, 0, // enchimento até o mínimo de 30 bytes
    ])
}

/// Um WebP/VP8X (estendido) mínimo.
fn make_web_p_vp8x(width: u32, height: u32) -> VSBuffer {
    let w = width - 1;
    let h = height - 1;
    buf(&[
        0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, //
        0x56, 0x50, 0x38, 0x58, 10, 0, 0, 0, // "VP8X" <chunkSize=10>
        0x00, 0x00, 0x00, 0x00, // flags e reservado
        (w & 0xFF) as u8, ((w >> 8) & 0xFF) as u8, ((w >> 16) & 0xFF) as u8, (h & 0xFF) as u8, ((h >> 8) & 0xFF) as u8, ((h >> 16) & 0xFF) as u8,
    ])
}

fn dims(width: u32, height: u32) -> Option<IImageDimensions> {
    Some(IImageDimensions { width, height })
}

#[test]
fn parses_dimensions_across_supported_formats_and_rejects_unrecognized_buffers() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixtures: Vec<(&str, VSBuffer, Option<IImageDimensions>)> = vec![
        ("jpeg", make_jpeg(640, 480), dims(640, 480)),
        ("png", make_png(1920, 1080), dims(1920, 1080)),
        ("gif", make_gif(50, 25), dims(50, 25)),
        ("webpVp8", make_web_p_vp8(300, 200), dims(300, 200)),
        ("webpVp8l", make_web_p_vp8l(1024, 768), dims(1024, 768)),
        ("webpVp8x", make_web_p_vp8x(8000, 6000), dims(8000, 6000)),
        ("empty", VSBuffer::alloc(0), None),
        ("tooShort", buf(&[0xFF, 0xD8, 0xFF, 0xE0]), None),
        ("unknown", buf(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B]), None),
        ("webpVp8MissingStartCode", corrupt_byte(&make_web_p_vp8(300, 200), 23, 0x00), None),
        ("webpVp8lMissingSignature", corrupt_byte(&make_web_p_vp8l(1024, 768), 20, 0x00), None),
        ("webpVp8ChunkTooSmall", corrupt_byte(&make_web_p_vp8(300, 200), 16, 4), None),
        ("webpVp8xChunkTooSmall", corrupt_byte(&make_web_p_vp8x(8000, 6000), 16, 4), None),
        (
            "jpegZeroSegmentLength",
            buf(&[
                0xFF, 0xD8, // SOI
                0xFF, 0xE0, 0x00, 0x00, // APP0 com comprimento inválido 0
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // enchimento para passar do `< 12` externo
            ]),
            None,
        ),
    ];

    let actual: Vec<(&str, Option<IImageDimensions>)> = fixtures.iter().map(|(k, input, _)| (*k, read_image_dimensions(input))).collect();
    let expected: Vec<(&str, Option<IImageDimensions>)> = fixtures.iter().map(|(k, _, expected)| (*k, *expected)).collect();

    assert_eq!(actual, expected);
}
