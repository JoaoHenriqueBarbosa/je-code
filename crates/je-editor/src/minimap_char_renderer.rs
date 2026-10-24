//! Porte de `src/vs/editor/browser/viewParts/minimap/minimapCharRenderer.ts`: desenha um caractere do
//! minimap (ou o bloco que o substitui) num `ImageData`.
//!
//! Divergências:
//! - `ImageData` é a estrutura [`ImageData`] (largura, altura e os bytes RGBA), sem o `colorSpace`.
//! - Os `Uint8ClampedArray` são `Vec<u8>`. Toda escrita que o original faz com um número qualquer num
//!   `Uint8ClampedArray` passa por [`to_uint8_clamped`] (limita a [0, 255], arredonda para o par e dá 0 para
//!   `NaN`), que é a conversão da linguagem.
//! - `dx`, `dy` e `scale` são inteiros sem sinal; o aviso `console.warn('bad render request outside image
//!   data')` vira `eprintln!`.
//! - Ler além do fim de `charData` (o índice `CHAR_COUNT` que `getCharIndex` pode devolver) dá `undefined`
//!   em JavaScript, que vira `NaN` na conta e 0 na escrita; aqui a leitura fora da folha dá `NaN`.

use je_base::uint::to_uint8;

use crate::minimap_char_sheet::{constants, get_char_index};
use crate::rgba::Rgba8;

/// O que o minimap usa de `ImageData`: largura, altura e os bytes RGBA (`width * height * 4`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageData {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// A conversão de um número para um elemento de `Uint8ClampedArray`.
pub fn to_uint8_clamped(value: f64) -> u8 {
    if value.is_nan() {
        return 0;
    }
    value.clamp(0.0, 255.0).round_ties_even() as u8
}

/// `Math.max` do JavaScript: `NaN` contamina o resultado.
pub fn math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    a.max(b)
}

/// `MinimapCharRenderer`.
pub struct MinimapCharRenderer {
    char_data_normal: Vec<u8>,
    char_data_light: Vec<u8>,
    pub scale: u32,
}

impl MinimapCharRenderer {
    pub fn new(char_data: &[u8], scale: u32) -> Self {
        Self {
            char_data_normal: Self::soften(char_data, 12.0 / 15.0),
            char_data_light: Self::soften(char_data, 50.0 / 60.0),
            scale,
        }
    }

    fn soften(input: &[u8], ratio: f64) -> Vec<u8> {
        input.iter().map(|&value| to_uint8(f64::from(value) * ratio) as u8).collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_char(
        &self,
        target: &mut ImageData,
        dx: usize,
        dy: usize,
        ch_code: u32,
        color: Rgba8,
        foreground_alpha: f64,
        background_color: Rgba8,
        background_alpha: f64,
        font_scale: f64,
        use_lighter_font: bool,
        force_1px_height: bool,
    ) {
        let char_width = constants::BASE_CHAR_WIDTH * self.scale as usize;
        let char_height = constants::BASE_CHAR_HEIGHT * self.scale as usize;
        let render_height = if force_1px_height { 1 } else { char_height };
        if dx + char_width > target.width || dy + render_height > target.height {
            eprintln!("bad render request outside image data");
            return;
        }

        let char_data = if use_lighter_font { &self.char_data_light } else { &self.char_data_normal };
        let char_index = get_char_index(ch_code, font_scale);

        let dest_width = target.width * constants::RGBA_CHANNELS_CNT;

        let background_r = f64::from(background_color.r);
        let background_g = f64::from(background_color.g);
        let background_b = f64::from(background_color.b);

        let delta_r = f64::from(color.r) - background_r;
        let delta_g = f64::from(color.g) - background_g;
        let delta_b = f64::from(color.b) - background_b;

        let dest_alpha = math_max(foreground_alpha, background_alpha);

        let dest = &mut target.data;
        let mut source_offset = char_index * char_width * char_height;

        let mut row = dy * dest_width + dx * constants::RGBA_CHANNELS_CNT;
        for _ in 0..render_height {
            let mut column = row;
            for _ in 0..char_width {
                let sample = char_data.get(source_offset).map_or(f64::NAN, |&value| f64::from(value));
                source_offset += 1;
                let c = (sample / 255.0) * (foreground_alpha / 255.0);
                dest[column] = to_uint8_clamped(background_r + delta_r * c);
                dest[column + 1] = to_uint8_clamped(background_g + delta_g * c);
                dest[column + 2] = to_uint8_clamped(background_b + delta_b * c);
                dest[column + 3] = to_uint8_clamped(dest_alpha);
                column += 4;
            }

            row += dest_width;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn block_render_char(
        &self,
        target: &mut ImageData,
        dx: usize,
        dy: usize,
        color: Rgba8,
        foreground_alpha: f64,
        background_color: Rgba8,
        background_alpha: f64,
        force_1px_height: bool,
    ) {
        let char_width = constants::BASE_CHAR_WIDTH * self.scale as usize;
        let char_height = constants::BASE_CHAR_HEIGHT * self.scale as usize;
        let render_height = if force_1px_height { 1 } else { char_height };
        if dx + char_width > target.width || dy + render_height > target.height {
            eprintln!("bad render request outside image data");
            return;
        }

        let dest_width = target.width * constants::RGBA_CHANNELS_CNT;

        let c = 0.5 * (foreground_alpha / 255.0);

        let background_r = f64::from(background_color.r);
        let background_g = f64::from(background_color.g);
        let background_b = f64::from(background_color.b);

        let delta_r = f64::from(color.r) - background_r;
        let delta_g = f64::from(color.g) - background_g;
        let delta_b = f64::from(color.b) - background_b;

        let color_r = to_uint8_clamped(background_r + delta_r * c);
        let color_g = to_uint8_clamped(background_g + delta_g * c);
        let color_b = to_uint8_clamped(background_b + delta_b * c);

        let dest_alpha = to_uint8_clamped(math_max(foreground_alpha, background_alpha));

        let dest = &mut target.data;

        let mut row = dy * dest_width + dx * constants::RGBA_CHANNELS_CNT;
        for _ in 0..render_height {
            let mut column = row;
            for _ in 0..char_width {
                dest[column] = color_r;
                dest[column + 1] = color_g;
                dest[column + 2] = color_b;
                dest[column + 3] = dest_alpha;
                column += 4;
            }

            row += dest_width;
        }
    }
}
