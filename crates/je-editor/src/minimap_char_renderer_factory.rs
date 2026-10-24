//! Porte de `src/vs/editor/browser/viewParts/minimap/minimapCharRendererFactory.ts`: cria os renderizadores
//! de caractere do minimap, reduzindo a amostra de fonte para a escala pedida.
//!
//! Divergências:
//! - O original desenha a amostra de fonte num `canvas` (`createSampleData`), o que não existe na pintura
//!   nativa. [`MinimapCharRendererFactory::create`] recebe quem produz os bytes RGBA da amostra
//!   (`create_sample_data(fontFamily)`) e só o chama quando a escala não tem dados pré-calculados, como o
//!   original; `createSampleData` em si fica com o chamador (o rasterizador de texto).
//! - `lastCreated` e `lastFontFamily` (estáticos da classe) são um `thread_local`, e o renderizador
//!   compartilhado é um `Rc`.
//! - `createFromSampleData` lança `Error` com o tamanho errado; aqui entra em pânico com a mesma mensagem.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::uint::to_uint8;

use crate::minimap_char_renderer::{MinimapCharRenderer, math_max, to_uint8_clamped};
use crate::minimap_char_sheet::constants;
use crate::minimap_pre_baked::prebaked_mini_map;

thread_local! {
    /// `lastCreated` e `lastFontFamily`.
    static LAST_CREATED: RefCell<Option<(Rc<MinimapCharRenderer>, String)>> = const { RefCell::new(None) };
}

/// Cria renderizadores de caractere. Recebe uma escala que diz o tamanho dos caracteres desenhados; com ela
/// reduz a amostra de cada caractere para o tamanho da tela atual, o que é mais barato que desenhar a
/// fonte inteira (minúscula) ou reduzir em tempo real.
pub struct MinimapCharRendererFactory;

impl MinimapCharRendererFactory {
    /// `create(scale, fontFamily)`.
    pub fn create(scale: u32, font_family: &str, create_sample_data: impl FnOnce(&str) -> Vec<u8>) -> Rc<MinimapCharRenderer> {
        // os renderizadores são imutáveis. Por padrão 'criamos' um novo sempre que trocamos de editor, sem
        // trabalho extra.
        let cached = LAST_CREATED.with(|last| {
            last.borrow()
                .as_ref()
                .filter(|(renderer, last_font_family)| scale == renderer.scale && font_family == last_font_family)
                .map(|(renderer, _)| renderer.clone())
        });
        if let Some(renderer) = cached {
            return renderer;
        }

        let factory = Rc::new(match prebaked_mini_map(scale) {
            Some(data) => MinimapCharRenderer::new(data, scale),
            None => Self::create_from_sample_data(&create_sample_data(font_family), scale),
        });

        LAST_CREATED.with(|last| *last.borrow_mut() = Some((factory.clone(), font_family.to_string())));
        factory
    }

    /// `createFromSampleData(source, scale)`: cria um renderizador a partir dos dados de amostra do canvas.
    pub fn create_from_sample_data(source: &[u8], scale: u32) -> MinimapCharRenderer {
        let expected_length =
            constants::SAMPLED_CHAR_HEIGHT * constants::SAMPLED_CHAR_WIDTH * constants::RGBA_CHANNELS_CNT * constants::CHAR_COUNT;
        if source.len() != expected_length {
            panic!("Unexpected source in MinimapCharRenderer");
        }

        let char_data = Self::downsample(source, scale);
        MinimapCharRenderer::new(&char_data, scale)
    }

    fn downsample_char(source: &[u8], source_offset: usize, dest: &mut [u8], dest_offset: usize, scale: u32) -> f64 {
        let width = constants::BASE_CHAR_WIDTH * scale as usize;
        let height = constants::BASE_CHAR_HEIGHT * scale as usize;

        let mut target_index = dest_offset;
        let mut brightest = 0.0_f64;

        // Este é essencialmente um algoritmo de reescala ad-hoc. Abordagens padrão como a interpolação
        // bicúbica são ótimas para mudar entre tamanhos de imagem, mas não funcionam bem ao reduzir para
        // valores de pixel muito pequenos: saem formas borradas e indistintas.
        //
        // A abordagem aqui é mapear cada pixel de origem para os pixels de destino, tomar os valores
        // ponderados de todos os pixels de cada um e tirar a média. No fim vem um reforço de intensidade
        // em `downsample`, porque nos menores tamanhos de pixel há mais espaço preto, o que deixa os
        // caracteres bem menos distintos.
        for y in 0..height {
            // 1. Para este pixel de destino, pega os pixels de origem de onde amostrar, de (x1, y1) até o
            // pixel seguinte (x2, y2)
            let source_y1 = (y as f64 / height as f64) * constants::SAMPLED_CHAR_HEIGHT as f64;
            let source_y2 = ((y + 1) as f64 / height as f64) * constants::SAMPLED_CHAR_HEIGHT as f64;

            for x in 0..width {
                let source_x1 = (x as f64 / width as f64) * constants::SAMPLED_CHAR_WIDTH as f64;
                let source_x2 = ((x + 1) as f64 / width as f64) * constants::SAMPLED_CHAR_WIDTH as f64;

                // 2. Amostra todos, somando e ponderando. Parecido com a interpolação bilinear.
                let mut value = 0.0_f64;
                let mut samples = 0.0_f64;
                let mut sy = source_y1;
                while sy < source_y2 {
                    let source_row = source_offset + sy.floor() as usize * constants::RGBA_SAMPLED_ROW_WIDTH;
                    let y_balance = 1.0 - (sy - sy.floor());
                    let mut sx = source_x1;
                    while sx < source_x2 {
                        let x_balance = 1.0 - (sx - sx.floor());
                        let source_index = source_row + sx.floor() as usize * constants::RGBA_CHANNELS_CNT;

                        let weight = x_balance * y_balance;
                        samples += weight;
                        value += ((f64::from(source[source_index]) * f64::from(source[source_index + 3])) / 255.0) * weight;
                        sx += 1.0;
                    }
                    sy += 1.0;
                }

                let final_value = value / samples;
                brightest = math_max(brightest, final_value);
                dest[target_index] = to_uint8(final_value) as u8;
                target_index += 1;
            }
        }

        brightest
    }

    fn downsample(data: &[u8], scale: u32) -> Vec<u8> {
        let pixels_per_character = constants::BASE_CHAR_HEIGHT * scale as usize * constants::BASE_CHAR_WIDTH * scale as usize;
        let result_len = pixels_per_character * constants::CHAR_COUNT;
        let mut result = vec![0_u8; result_len];

        let mut result_offset = 0;
        let mut source_offset = 0;
        let mut brightest = 0.0_f64;
        for _ in 0..constants::CHAR_COUNT {
            brightest = math_max(brightest, Self::downsample_char(data, source_offset, &mut result, result_offset, scale));
            result_offset += pixels_per_character;
            source_offset += constants::SAMPLED_CHAR_WIDTH * constants::RGBA_CHANNELS_CNT;
        }

        if brightest > 0.0 {
            let adjust = 255.0 / brightest;
            for value in &mut result {
                *value = to_uint8_clamped(f64::from(*value) * adjust);
            }
        }

        result
    }
}
