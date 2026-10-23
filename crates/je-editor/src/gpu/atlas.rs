//! Porte de `src/vs/editor/browser/gpu/atlas/atlas.ts`: os tipos comuns do atlas de texturas.
//!
//! Divergências:
//! - `OffscreenCanvas` vira [`PixelPage`] (ver `gpu_utils`), e `getUsagePreview(): Promise<Blob>` vira uma
//!   chamada síncrona que devolve o preview como [`PixelPage`] (a codificação em PNG não existe aqui).
//! - `allocate` devolve `Result`, porque o original lança `BugIndicatingError` quando o glifo é maior que a
//!   página, e recebe `&mut self`, porque o alocador muda de estado.
//! - `Readonly<ITextureAtlasPageGlyph>` é o próprio struct (`Copy`, sem método que mude algo).
//! - `glyphs: IterableIterator<...>` vira um `Box<dyn Iterator>`.
//! - `const enum UsagePreviewColors` (strings CSS) vira um `enum` com [`UsagePreviewColors::css`], o texto
//!   original, e [`UsagePreviewColors::rgba`], a cor que o [`PixelPage`] pinta.
//! - `GlyphMap<T>` é o `NKeyMap` do je-base: as chaves são, nesta ordem, `chars` (`NKey::Str`),
//!   `tokenMetadata` (`NKey::Num`), `decorationStyleSetId` (`NKey::Num`) e `rasterizerCacheKey`
//!   (`NKey::Str`).

use je_base::errors::Error;
use je_base::map::NKeyMap;

use super::gpu_utils::{PixelPage, Rgba, SharedPixelPage};
use super::raster::{IBoundingBox, IRasterizedGlyph};

/// Informações sobre um [`IRasterizedGlyph`] que foi desenhado numa página do atlas de texturas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ITextureAtlasPageGlyph {
    /// O índice da página do atlas onde o glifo foi desenhado.
    pub page_index: usize,
    /// O índice do glifo na página do atlas.
    pub glyph_index: usize,
    /// A coordenada x do glifo na página do atlas.
    pub x: u32,
    /// A coordenada y do glifo na página do atlas.
    pub y: u32,
    /// A largura do glifo em pixels.
    pub w: u32,
    /// A altura do glifo em pixels.
    pub h: u32,
    /// O deslocamento em x, a partir de `x`, da origem do glifo.
    pub origin_offset_x: i32,
    /// O deslocamento em y, a partir de `y`, da origem do glifo.
    pub origin_offset_y: i32,
    /// A distância da linha de base do glifo até o topo do mais alto retângulo delimitador de todas as
    /// fontes usadas para renderizar o texto.
    pub font_bounding_box_ascent: f64,
    /// A distância da linha de base do glifo até a base do retângulo delimitador de todas as fontes usadas
    /// para renderizar o texto.
    pub font_bounding_box_descent: f64,
}

/// Um alocador de atlas de texturas recebe glifos rasterizados, desenha numa página do atlas e devolve as
/// informações do glifo no atlas.
pub trait ITextureAtlasAllocator {
    /// Aloca um glifo rasterizado na página, desenhando-o e devolvendo a posição dele. Devolve `Ok(None)` se o
    /// glifo não cabe na página, e `Err` (`BugIndicatingError`) se ele é maior que a página inteira.
    fn allocate(&mut self, rasterized_glyph: &IRasterizedGlyph) -> Result<Option<ITextureAtlasPageGlyph>, Error>;

    /// Uma prévia do uso do atlas, para depuração.
    fn get_usage_preview(&self) -> PixelPage;

    /// Estatísticas do estado atual do alocador, para depuração.
    fn get_stats(&self) -> String;
}

/// Uma página do atlas de texturas que pode ser lida, mas não modificada.
pub trait IReadableTextureAtlasPage {
    /// Um identificador do estado atual da página: um número que cresce a cada glifo desenhado nela.
    fn version(&self) -> u32;

    /// A área da página atualmente em uso.
    fn used_area(&self) -> IBoundingBox;

    /// Todos os glifos desenhados na página, na ordem em que foram desenhados.
    fn glyphs(&self) -> Box<dyn Iterator<Item = ITextureAtlasPageGlyph> + '_>;

    /// A página de origem.
    fn source(&self) -> SharedPixelPage;

    /// Uma prévia do uso do atlas, para depuração.
    fn get_usage_preview(&self) -> PixelPage;

    /// Estatísticas do estado atual do alocador, para depuração.
    fn get_stats(&self) -> String;
}

/// `UsagePreviewColors`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsagePreviewColors {
    Unused,
    Used,
    Wasted,
    Restricted,
}

impl UsagePreviewColors {
    /// O valor CSS do original.
    pub fn css(self) -> &'static str {
        match self {
            Self::Unused => "#808080",
            Self::Used => "#4040FF",
            Self::Wasted => "#FF0000",
            Self::Restricted => "#FF000088",
        }
    }

    /// A mesma cor em RGBA de 8 bits.
    pub fn rgba(self) -> Rgba {
        match self {
            Self::Unused => [0x80, 0x80, 0x80, 0xFF],
            Self::Used => [0x40, 0x40, 0xFF, 0xFF],
            Self::Wasted => [0xFF, 0x00, 0x00, 0xFF],
            Self::Restricted => [0xFF, 0x00, 0x00, 0x88],
        }
    }
}

/// `GlyphMap<T>`: ver o cabeçalho do módulo para as chaves.
pub type GlyphMap<T> = NKeyMap<T>;
