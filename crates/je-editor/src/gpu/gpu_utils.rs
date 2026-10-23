//! Porte de `src/vs/editor/browser/gpu/gpuUtils.ts`, no que o pipeline de dados puros usa, mais o
//! [`PixelPage`], a página de pixels que substitui o `OffscreenCanvas` do original.
//!
//! Divergências:
//! - `quadVertices` (um `Float32Array`) vira a constante [`QUAD_VERTICES`].
//! - `ensureNonNullable(value: T | null)` vira [`ensure_non_nullable`] sobre `Option<T>`. A mensagem usa
//!   `null` porque `None` é o único valor "nulo" (o `${value}` do original só imprime `null`, ou o valor
//!   falso, `0` ou vazio, que num `Option` não existe).
//! - `observeDevicePixelDimensions` depende de `ResizeObserver` e de `HTMLElement`, que não existem no
//!   je-code (a janela é nativa e o tamanho em pixels vem direto do sistema de janelas). Fica de fora.
//! - O `OffscreenCanvas` com o contexto 2D (`drawImage`, `fillRect`, `getImageData`, `convertToBlob`) vira
//!   o [`PixelPage`], uma página de pixels RGBA de 8 bits por canal (alfa direto, não pré-multiplicado, que é
//!   o que o `getImageData` devolve), com só as operações que os alocadores de atlas e os testes usam:
//!   composição "source-over" em `fill_rect` e `draw_image` (com `globalAlpha`), e `get_image_data`. A
//!   codificação em PNG do `convertToBlob` fica de fora: o preview de uso é devolvido como [`PixelPage`].
//!   Como no canvas, desenhar fora dos limites é cortado e `get_image_data` fora dos limites devolve preto
//!   transparente.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};

/// `quadVertices`.
pub const QUAD_VERTICES: [f32; 12] = [1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0];

/// `ensureNonNullable`.
pub fn ensure_non_nullable<T>(value: Option<T>) -> T {
    match value {
        Some(value) => value,
        None => errors::throw(Error::new("Value \"null\" cannot be null")),
    }
}

/// Uma cor RGBA de 8 bits por canal, com o alfa direto (não pré-multiplicado).
pub type Rgba = [u8; 4];

/// Um retângulo de pixels (`x`, `y`, largura, altura).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Uma página de pixels RGBA, o equivalente de um `OffscreenCanvas` com contexto 2D.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelPage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// A página compartilhada entre quem desenha nela (o alocador) e quem a lê (o teste, o atlas): o
/// `OffscreenCanvas` do original é um objeto por referência.
pub type SharedPixelPage = Rc<RefCell<PixelPage>>;

impl PixelPage {
    /// `new OffscreenCanvas(width, height)`: tudo preto transparente.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, data: vec![0; width as usize * height as usize * 4] }
    }

    /// `new ImageData(width, height)` com `data.set(data)` desenhado na origem: o `data` precisa ter
    /// `width * height * 4` bytes (o `RangeError` do `TypedArray.set` quando não cabe).
    pub fn from_rgba(width: u32, height: u32, data: Vec<u8>) -> Result<Self, Error> {
        if data.len() > width as usize * height as usize * 4 {
            return Err(Error::named("RangeError", "offset is out of bounds"));
        }
        let mut page = Self::new(width, height);
        page.data[..data.len()].copy_from_slice(&data);
        Ok(page)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Os pixels, linha a linha, quatro bytes (R, G, B, A) cada.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    fn index_of(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then(|| (y as usize * self.width as usize + x as usize) * 4)
    }

    /// `ctx.getImageData(x, y, w, h).data`.
    pub fn get_image_data(&self, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        for row in y..y.saturating_add(h) {
            for column in x..x.saturating_add(w) {
                match self.index_of(column, row) {
                    Some(index) => out.extend_from_slice(&self.data[index..index + 4]),
                    None => out.extend_from_slice(&[0; 4]),
                }
            }
        }
        out
    }

    /// Compõe `source` (alfa multiplicado por `global_alpha`) sobre o pixel, "source-over".
    fn composite(&mut self, x: u32, y: u32, source: Rgba, global_alpha: f64) {
        let Some(index) = self.index_of(x, y) else { return };
        let source_alpha = f64::from(source[3]) / 255.0 * global_alpha;
        if source_alpha == 0.0 {
            return;
        }
        let destination = &mut self.data[index..index + 4];
        let destination_alpha = f64::from(destination[3]) / 255.0;
        let result_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
        for channel in 0..3 {
            let value = (f64::from(source[channel]) * source_alpha
                + f64::from(destination[channel]) * destination_alpha * (1.0 - source_alpha))
                / result_alpha;
            destination[channel] = value.round() as u8;
        }
        destination[3] = (result_alpha * 255.0).round() as u8;
    }

    /// `ctx.fillStyle = color; ctx.fillRect(x, y, w, h)`.
    pub fn fill_rect(&mut self, rect: PixelRect, color: Rgba) {
        for row in rect.y..rect.y.saturating_add(rect.h) {
            for column in rect.x..rect.x.saturating_add(rect.w) {
                self.composite(column, row, color, 1.0);
            }
        }
    }

    /// `ctx.globalAlpha = global_alpha; ctx.drawImage(source, sx, sy, sw, sh, dx, dy, sw, sh)`: copia
    /// `source_rect` de `source` para `(dx, dy)` sem mudar de escala, cortando o que sair de qualquer
    /// uma das páginas.
    pub fn draw_image(&mut self, source: &PixelPage, source_rect: PixelRect, dx: u32, dy: u32, global_alpha: f64) {
        for row in 0..source_rect.h {
            for column in 0..source_rect.w {
                let Some(index) = source.index_of(source_rect.x.saturating_add(column), source_rect.y.saturating_add(row))
                else {
                    continue;
                };
                let pixel: Rgba = [source.data[index], source.data[index + 1], source.data[index + 2], source.data[index + 3]];
                self.composite(dx.saturating_add(column), dy.saturating_add(row), pixel, global_alpha);
            }
        }
    }
}
