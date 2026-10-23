//! Porte de `src/vs/editor/browser/gpu/atlas/textureAtlasShelfAllocator.ts`.
//!
//! Divergências:
//! - `OffscreenCanvas` e o contexto 2D (`willReadFrequently`) viram a [`SharedPixelPage`]; o `drawImage` é
//!   `PixelPage::draw_image`, e o `ensureNonNullable(getContext(...))` some porque a página já é o contexto.
//! - `allocate` devolve `Result<Option<_>, Error>`: o `BugIndicatingError` lançado vira `Err`.
//! - `getUsagePreview(): Promise<Blob>` devolve o [`PixelPage`] pintado (sem PNG), e as listas `Map<y, ...>`
//!   viram `BTreeMap` (a ordem de iteração não muda o resultado: as regiões pintadas não se sobrepõem).
//! - `toPrecision(2)` é [`to_precision`], que repete o arredondamento do JavaScript.
//! - O conjunto de glifos (`Set`) vira `Vec`, que preserva a ordem de inserção e não tem duplicata
//!   (cada glifo tem um `glyphIndex` próprio).
//! - As contas de linha repetidas em `getUsagePreview` e `getStats` ganham o nome `row_extents` (DRY).

use std::collections::BTreeMap;

use je_base::errors::Error;

use super::atlas::{ITextureAtlasAllocator, ITextureAtlasPageGlyph, UsagePreviewColors};
use super::gpu_utils::{PixelPage, PixelRect, SharedPixelPage};
use super::js_number_format::to_precision;
use super::raster::IRasterizedGlyph;

/// O alocador de prateleiras é um alocador simples que põe os glifos em linhas, começando uma linha nova
/// quando a atual enche. Por ser simples ele pode desperdiçar espaço, mas é muito rápido.
pub struct TextureAtlasShelfAllocator {
    canvas: SharedPixelPage,
    texture_index: usize,
    current_row: ITextureAtlasShelf,
    /// Todos os glifos alocados, guardados só para as funções de depuração.
    allocated_glyphs: Vec<ITextureAtlasPageGlyph>,
    next_index: usize,
}

#[derive(Clone, Copy)]
struct ITextureAtlasShelf {
    x: u32,
    y: u32,
    h: u32,
}

impl TextureAtlasShelfAllocator {
    pub fn new(canvas: SharedPixelPage, texture_index: usize) -> Self {
        Self {
            canvas,
            texture_index,
            current_row: ITextureAtlasShelf { x: 0, y: 0, h: 0 },
            allocated_glyphs: Vec::new(),
            next_index: 0,
        }
    }

    fn canvas_size(&self) -> (u32, u32) {
        let canvas = self.canvas.borrow();
        (canvas.width(), canvas.height())
    }
}

/// A altura e a largura usadas de cada linha (a chave é o `y` da linha), a partir dos glifos alocados.
fn row_extents(glyphs: &[ITextureAtlasPageGlyph]) -> (BTreeMap<u32, u32>, BTreeMap<u32, u32>) {
    let mut row_height: BTreeMap<u32, u32> = BTreeMap::new();
    let mut row_width: BTreeMap<u32, u32> = BTreeMap::new();
    for glyph in glyphs {
        let height = row_height.entry(glyph.y).or_insert(0);
        *height = (*height).max(glyph.h);
        let width = row_width.entry(glyph.y).or_insert(0);
        *width = (*width).max(glyph.x + glyph.w);
    }
    (row_height, row_width)
}

impl ITextureAtlasAllocator for TextureAtlasShelfAllocator {
    fn allocate(&mut self, rasterized_glyph: &IRasterizedGlyph) -> Result<Option<ITextureAtlasPageGlyph>, Error> {
        let (canvas_width, canvas_height) = self.canvas_size();

        // O glifo não cabe na página.
        let glyph_width = rasterized_glyph.bounding_box.width();
        let glyph_height = rasterized_glyph.bounding_box.height();
        if glyph_width > canvas_width || glyph_height > canvas_height {
            return Err(Error::bug_indicating(Some("Glyph is too large for the atlas page")));
        }

        // Fecha a linha e começa outra se o glifo não cabe na horizontal.
        if glyph_width > canvas_width - self.current_row.x {
            self.current_row.x = 0;
            self.current_row.y += self.current_row.h;
            self.current_row.h = 1;
        }

        // Devolve `None` se não sobrou espaço.
        if self.current_row.y + glyph_height > canvas_height {
            return Ok(None);
        }

        // Desenha o glifo.
        self.canvas.borrow_mut().draw_image(
            &rasterized_glyph.source,
            rasterized_glyph.bounding_box.rect(),
            self.current_row.x,
            self.current_row.y,
            1.0,
        );

        // Cria o objeto do glifo.
        let glyph = ITextureAtlasPageGlyph {
            page_index: self.texture_index,
            glyph_index: self.next_index,
            x: self.current_row.x,
            y: self.current_row.y,
            w: glyph_width,
            h: glyph_height,
            origin_offset_x: rasterized_glyph.origin_offset.x,
            origin_offset_y: rasterized_glyph.origin_offset.y,
            font_bounding_box_ascent: rasterized_glyph.font_bounding_box_ascent,
            font_bounding_box_descent: rasterized_glyph.font_bounding_box_descent,
        };
        self.next_index += 1;

        // Avança a linha atual.
        self.current_row.x += glyph_width;
        self.current_row.h = self.current_row.h.max(glyph_height);

        // Guarda o glifo.
        self.allocated_glyphs.push(glyph);

        Ok(Some(glyph))
    }

    fn get_usage_preview(&self) -> PixelPage {
        let (w, h) = self.canvas_size();
        let mut canvas = PixelPage::new(w, h);
        canvas.fill_rect(PixelRect { x: 0, y: 0, w, h }, UsagePreviewColors::Unused.rgba());

        let (row_height, row_width) = row_extents(&self.allocated_glyphs);
        for g in &self.allocated_glyphs {
            canvas.fill_rect(PixelRect { x: g.x, y: g.y, w: g.w, h: g.h }, UsagePreviewColors::Used.rgba());
            canvas.fill_rect(
                PixelRect { x: g.x, y: g.y + g.h, w: g.w, h: row_height[&g.y] - g.h },
                UsagePreviewColors::Wasted.rgba(),
            );
        }
        for (&row_y, &row_w) in &row_width {
            if row_y != self.current_row.y {
                canvas.fill_rect(
                    PixelRect { x: row_w, y: row_y, w: w - row_w, h: row_height[&row_y] },
                    UsagePreviewColors::Wasted.rgba(),
                );
            }
        }
        canvas
    }

    fn get_stats(&self) -> String {
        let (w, h) = self.canvas_size();

        let mut used_pixels: u64 = 0;
        let mut wasted_pixels: u64 = 0;
        let total_pixels = u64::from(w) * u64::from(h);

        let (row_height, row_width) = row_extents(&self.allocated_glyphs);
        for g in &self.allocated_glyphs {
            used_pixels += u64::from(g.w) * u64::from(g.h);
            wasted_pixels += u64::from(g.w) * u64::from(row_height[&g.y] - g.h);
        }
        for (&row_y, &row_w) in &row_width {
            if row_y != self.current_row.y {
                wasted_pixels += u64::from(w - row_w) * u64::from(row_height[&row_y]);
            }
        }
        let percent_of_total = |pixels: u64| to_precision(pixels as f64 / total_pixels as f64 * 100.0, 2);
        [
            format!("page{}:", self.texture_index),
            format!("     Total: {total_pixels} ({w}x{h})"),
            format!("      Used: {used_pixels} ({}%)", percent_of_total(used_pixels)),
            format!("    Wasted: {wasted_pixels} ({}%)", percent_of_total(wasted_pixels)),
            format!(
                "Efficiency: {}%",
                to_precision(used_pixels as f64 / (used_pixels + wasted_pixels) as f64 * 100.0, 2)
            ),
        ]
        .join("\n")
    }
}
