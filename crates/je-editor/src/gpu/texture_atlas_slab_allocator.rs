//! Porte de `src/vs/editor/browser/gpu/atlas/textureAtlasSlabAllocator.ts`.
//!
//! Divergências:
//! - `OffscreenCanvas` e o contexto 2D viram a [`SharedPixelPage`] (ver `gpu_utils`), e
//!   `allocate` devolve `Result<Option<_>, Error>` (o `BugIndicatingError` lançado vira `Err`).
//! - `getActiveWindow().devicePixelRatio` vira o campo `device_pixel_ratio` das opções (padrão 1.0), já
//!   que não há janela do DOM. O tamanho padrão da laje é `64 << max(floor(dpr) - 1, 0)`; o original usa
//!   o `max` só no construtor, e no preview e nas estatísticas usa `64 << (floor(dpr) - 1)`, que para
//!   `dpr < 1` desloca por -1 e dá 0 (laje de tamanho zero, com divisão por zero). Aqui os três lugares
//!   usam o mesmo tamanho com `max`, idêntico para `dpr >= 1`.
//! - `_activeSlabsByDims: NKeyMap<ITextureAtlasSlab, [number, number]>` guarda o índice da laje em
//!   `slabs` (no JS é a referência ao objeto, que `slab.count++` muda por ali): `NKeyMap<usize>`.
//! - `sizeCandidate /= 2` do original é divisão em ponto flutuante (5 vira 2.5). Aqui a divisão é inteira
//!   e a condição `glyphWidth < sizeCandidate / 2` vira `glyphWidth * 2 < sizeCandidate`; as duas coincidem
//!   para páginas com dimensão potência de dois (o que o atlas sempre usa).
//! - Dois defeitos do original são mantidos de propósito: na busca por região livre, o ramo da altura
//!   confere `r.h === 0` depois de diminuir `r.w` (nunca é verdade), e a remoção de região esgotada que não
//!   é a última faz `_unusedRects.splice(i, 1)` (a lista errada, que ninguém lê) em vez de tirar de
//!   `openRegions`. O efeito visível é o mesmo do original.
//! - As partes mortas de `getUsagePreview` (os contadores `slabEntryPixels`, `usedPixels`, `slabEdgePixels` e
//!   `restrictedPixels`, que ele calcula e nunca usa) não são portadas; só o que pinta a página.
//! - `getUsagePreview(): Promise<Blob>` devolve o [`PixelPage`] pintado (sem PNG). `toFixed(2)` é
//!   [`to_fixed`], com o arredondamento do JavaScript.
//! - `Map<number, ...>` das regiões abertas vira `BTreeMap` (a ordem entre chaves não muda nenhuma soma nem
//!   pinta regiões sobrepostas) e o conjunto de glifos (`Set`) vira `Vec`.
//! - A busca nas regiões abertas é uma função só ([`find_in_open_regions`]), com o corte da região livre
//!   como parâmetro, porque os dois ramos do original só diferem nele.

use std::collections::BTreeMap;

use je_base::errors::Error;
use je_base::map::{NKey, NKeyMap};

use super::atlas::{ITextureAtlasAllocator, ITextureAtlasPageGlyph, UsagePreviewColors};
use super::gpu_utils::{PixelPage, PixelRect, SharedPixelPage};
use super::js_number_format::to_fixed;
use super::raster::IRasterizedGlyph;

/// `TextureAtlasSlabAllocatorOptions`, mais o `device_pixel_ratio` que o original lê da janela.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextureAtlasSlabAllocatorOptions {
    pub slab_w: Option<u32>,
    pub slab_h: Option<u32>,
    pub device_pixel_ratio: Option<f64>,
}

/// O alocador de lajes é um alocador mais complexo que põe os glifos em lajes quadradas de tamanho fixo.
/// Cada laje é definida por uma faixa pequena de tamanhos de glifo que ela abriga, o que junta glifos de
/// tamanho parecido na mesma laje e reduz o desperdício.
///
/// As lajes também podem ter regiões "sem uso" à esquerda e embaixo, conforme o tamanho dos glifos que
/// guardam. Esse espaço serve para glifos muito finos ou baixos, que desperdiçariam muito numa laje própria.
pub struct TextureAtlasSlabAllocator {
    canvas: SharedPixelPage,
    texture_index: usize,
    slabs: Vec<ITextureAtlasSlab>,
    active_slabs_by_dims: NKeyMap<usize>,
    unused_rects: Vec<ITextureAtlasSlabUnusedRect>,
    open_regions_by_height: BTreeMap<u32, Vec<ITextureAtlasSlabUnusedRect>>,
    open_regions_by_width: BTreeMap<u32, Vec<ITextureAtlasSlabUnusedRect>>,
    /// Todos os glifos alocados, guardados só para as funções de depuração.
    allocated_glyphs: Vec<ITextureAtlasPageGlyph>,
    device_pixel_ratio: f64,
    slab_w: u32,
    slab_h: u32,
    slabs_per_row: u32,
    slabs_per_column: u32,
    next_index: usize,
}

#[derive(Clone, Copy)]
struct ITextureAtlasSlab {
    x: u32,
    y: u32,
    entry_h: u32,
    entry_w: u32,
    count: u32,
}

#[derive(Clone, Copy)]
struct ITextureAtlasSlabUnusedRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// `64 << max(floor(dpr) - 1, 0)`.
fn default_slab_size(device_pixel_ratio: f64) -> u32 {
    let shift = (device_pixel_ratio.floor() as i32 - 1).max(0);
    (64.0 * 2.0_f64.powi(shift)) as u32
}

/// `Math.floor(total / part)`; zero quando `part` é zero (o `Infinity` do original nunca é usado).
fn count_in(total: u32, part: u32) -> u32 {
    total.checked_div(part).unwrap_or(0)
}

/// Corta, do começo da região, o espaço de um glifo de largura `glyph_width` (região indexada pela largura):
/// o que sobra na largura vira uma região nova e o começo desce `glyph_height`.
fn carve_along_width(
    region: &mut ITextureAtlasSlabUnusedRect,
    glyph_width: u32,
    glyph_height: u32,
    unused_rects: &mut Vec<ITextureAtlasSlabUnusedRect>,
) {
    if glyph_width < region.w {
        unused_rects.push(ITextureAtlasSlabUnusedRect {
            x: region.x + glyph_width,
            y: region.y,
            w: region.w - glyph_width,
            h: glyph_height,
        });
    }
    region.y += glyph_height;
    region.h -= glyph_height;
}

/// O mesmo corte para a região indexada pela altura: o que sobra na altura vira uma região nova e o começo
/// avança `glyph_width`.
fn carve_along_height(
    region: &mut ITextureAtlasSlabUnusedRect,
    glyph_width: u32,
    glyph_height: u32,
    unused_rects: &mut Vec<ITextureAtlasSlabUnusedRect>,
) {
    if glyph_height < region.h {
        unused_rects.push(ITextureAtlasSlabUnusedRect {
            x: region.x,
            y: region.y + glyph_height,
            w: glyph_width,
            h: region.h - glyph_height,
        });
    }
    region.x += glyph_width;
    region.w -= glyph_width;
}

type Carve = fn(&mut ITextureAtlasSlabUnusedRect, u32, u32, &mut Vec<ITextureAtlasSlabUnusedRect>);

/// Procura, do fim para o começo (para tirar da pilha), uma região aberta onde o glifo caiba, corta o
/// espaço dele e devolve a posição.
fn find_in_open_regions(
    open_regions: &mut Vec<ITextureAtlasSlabUnusedRect>,
    unused_rects: &mut Vec<ITextureAtlasSlabUnusedRect>,
    glyph_width: u32,
    glyph_height: u32,
    carve: Carve,
) -> Option<(u32, u32)> {
    for i in (0..open_regions.len()).rev() {
        let region = &mut open_regions[i];
        if region.w >= glyph_width && region.h >= glyph_height {
            let position = (region.x, region.y);
            carve(region, glyph_width, glyph_height, unused_rects);
            if region.h == 0 {
                if i == open_regions.len() - 1 {
                    open_regions.pop();
                } else if i < unused_rects.len() {
                    unused_rects.remove(i);
                }
            }
            return Some(position);
        }
    }
    None
}

fn add_entry_to_map_array(
    map: &mut BTreeMap<u32, Vec<ITextureAtlasSlabUnusedRect>>,
    key: u32,
    entry: ITextureAtlasSlabUnusedRect,
) {
    map.entry(key).or_default().push(entry);
}

impl TextureAtlasSlabAllocator {
    pub fn new(canvas: SharedPixelPage, texture_index: usize, options: Option<TextureAtlasSlabAllocatorOptions>) -> Self {
        let options = options.unwrap_or_default();
        let device_pixel_ratio = options.device_pixel_ratio.unwrap_or(1.0);
        let (canvas_width, canvas_height) = {
            let canvas = canvas.borrow();
            (canvas.width(), canvas.height())
        };
        let slab_w = options.slab_w.unwrap_or_else(|| default_slab_size(device_pixel_ratio)).min(canvas_width);
        let slab_h = options.slab_h.unwrap_or(slab_w).min(canvas_height);
        Self {
            canvas,
            texture_index,
            slabs: Vec::new(),
            active_slabs_by_dims: NKeyMap::new(),
            unused_rects: Vec::new(),
            open_regions_by_height: BTreeMap::new(),
            open_regions_by_width: BTreeMap::new(),
            allocated_glyphs: Vec::new(),
            device_pixel_ratio,
            slab_w,
            slab_h,
            slabs_per_row: count_in(canvas_width, slab_w),
            slabs_per_column: count_in(canvas_height, slab_h),
            next_index: 0,
        }
    }

    fn canvas_size(&self) -> (u32, u32) {
        let canvas = self.canvas.borrow();
        (canvas.width(), canvas.height())
    }

    /// As regiões livres das bordas das lajes, primeiro as indexadas pela largura e depois pela altura.
    fn unused_regions(&self) -> Vec<ITextureAtlasSlabUnusedRect> {
        self.open_regions_by_width.values().chain(self.open_regions_by_height.values()).flatten().copied().collect()
    }
}

impl ITextureAtlasAllocator for TextureAtlasSlabAllocator {
    fn allocate(&mut self, rasterized_glyph: &IRasterizedGlyph) -> Result<Option<ITextureAtlasPageGlyph>, Error> {
        // Acha a laje ideal, criando uma se nenhuma serve.
        let (canvas_width, canvas_height) = self.canvas_size();
        let glyph_width = rasterized_glyph.bounding_box.width();
        let glyph_height = rasterized_glyph.bounding_box.height();

        // O glifo não cabe na página; na prática glifos nunca são tão grandes.
        if glyph_width > canvas_width || glyph_height > canvas_height {
            return Err(Error::bug_indicating(Some("Glyph is too large for the atlas page")));
        }

        // O glifo não cabe numa laje.
        if glyph_width > self.slab_w || glyph_height > self.slab_h {
            // Só se este for o primeiro glifo do alocador, ajusta o tamanho da laje para caber nele.
            if !self.allocated_glyphs.is_empty() {
                return Ok(None);
            }
            // Acha o maior divisor potência de dois em que o glifo cabe, para não sobrar espaço fora das
            // lajes alocadas.
            let mut size_candidate = canvas_width;
            while glyph_width.saturating_mul(2) < size_candidate && glyph_height.saturating_mul(2) < size_candidate {
                size_candidate /= 2;
            }
            self.slab_w = size_candidate;
            self.slab_h = size_candidate;
            self.slabs_per_row = count_in(canvas_width, self.slab_w);
            self.slabs_per_column = count_in(canvas_height, self.slab_h);
        }

        // Tamanho exato apenas (o original testa e descarta outros arredondamentos).
        let dims = [NKey::Num(f64::from(glyph_width)), NKey::Num(f64::from(glyph_height))];

        // Pega a laje existente, se houver.
        let mut slab = self.active_slabs_by_dims.get(&dims).copied();

        // Confere se a laje está cheia.
        if let Some(index) = slab {
            let existing = &self.slabs[index];
            let glyphs_per_slab = count_in(self.slab_w, existing.entry_w) * count_in(self.slab_h, existing.entry_h);
            if existing.count >= glyphs_per_slab {
                slab = None;
            }
        }

        // Procura espaço nas regiões sem uso, olhando só o menor lado.
        let mut placement = None;
        if slab.is_none() {
            placement = if glyph_width < glyph_height {
                self.open_regions_by_width.get_mut(&glyph_width).and_then(|regions| {
                    find_in_open_regions(regions, &mut self.unused_rects, glyph_width, glyph_height, carve_along_width)
                })
            } else {
                self.open_regions_by_height.get_mut(&glyph_height).and_then(|regions| {
                    find_in_open_regions(regions, &mut self.unused_rects, glyph_width, glyph_height, carve_along_height)
                })
            };
        }

        // Cria uma laje nova, se não coube em região livre.
        let (dx, dy) = match placement {
            Some(position) => position,
            None => {
                let slab_index = match slab {
                    Some(index) => index,
                    None => {
                        if self.slabs.len() >= (self.slabs_per_row * self.slabs_per_column) as usize {
                            return Ok(None);
                        }

                        let index = self.slabs.len();
                        let new_slab = ITextureAtlasSlab {
                            x: (index as u32 % self.slabs_per_row) * self.slab_w,
                            y: (index as u32 / self.slabs_per_row) * self.slab_h,
                            entry_w: glyph_width,
                            entry_h: glyph_height,
                            count: 0,
                        };
                        // Registra as regiões sem uso para os glifos pequenos:
                        // +-------------+----+
                        // |             |    |
                        // |             |    | <- região sem uso de largura
                        // |             |    |
                        // |-------------+----+
                        // |                  | <- região sem uso de altura
                        // +------------------+
                        let unused_w = self.slab_w.checked_rem(new_slab.entry_w).unwrap_or(0);
                        let unused_h = self.slab_h.checked_rem(new_slab.entry_h).unwrap_or(0);
                        if unused_w != 0 {
                            add_entry_to_map_array(
                                &mut self.open_regions_by_width,
                                unused_w,
                                ITextureAtlasSlabUnusedRect {
                                    x: new_slab.x + self.slab_w - unused_w,
                                    w: unused_w,
                                    y: new_slab.y,
                                    h: self.slab_h - unused_h,
                                },
                            );
                        }
                        if unused_h != 0 {
                            add_entry_to_map_array(
                                &mut self.open_regions_by_height,
                                unused_h,
                                ITextureAtlasSlabUnusedRect {
                                    x: new_slab.x,
                                    w: self.slab_w,
                                    y: new_slab.y + self.slab_h - unused_h,
                                    h: unused_h,
                                },
                            );
                        }
                        self.slabs.push(new_slab);
                        self.active_slabs_by_dims.set(index, &dims);
                        index
                    }
                };

                let target = &mut self.slabs[slab_index];
                let glyphs_per_row = count_in(self.slab_w, target.entry_w);
                let dx = target.x + target.count.checked_rem(glyphs_per_row).unwrap_or(0) * target.entry_w;
                let dy = target.y + target.count.checked_div(glyphs_per_row).unwrap_or(0) * target.entry_h;

                // Avança a laje.
                target.count += 1;
                (dx, dy)
            }
        };

        // Desenha o glifo.
        self.canvas.borrow_mut().draw_image(&rasterized_glyph.source, rasterized_glyph.bounding_box.rect(), dx, dy, 1.0);

        // Cria o objeto do glifo.
        let glyph = ITextureAtlasPageGlyph {
            page_index: self.texture_index,
            glyph_index: self.next_index,
            x: dx,
            y: dy,
            w: glyph_width,
            h: glyph_height,
            origin_offset_x: rasterized_glyph.origin_offset.x,
            origin_offset_y: rasterized_glyph.origin_offset.y,
            font_bounding_box_ascent: rasterized_glyph.font_bounding_box_ascent,
            font_bounding_box_descent: rasterized_glyph.font_bounding_box_descent,
        };
        self.next_index += 1;

        // Guarda o glifo.
        self.allocated_glyphs.push(glyph);

        Ok(Some(glyph))
    }

    fn get_usage_preview(&self) -> PixelPage {
        let (w, h) = self.canvas_size();
        let mut preview = PixelPage::new(w, h);
        preview.fill_rect(PixelRect { x: 0, y: 0, w, h }, UsagePreviewColors::Unused.rgba());

        let slab_w = default_slab_size(self.device_pixel_ratio);

        // Pinta primeiro o desperdício por baixo dos glifos.
        for slab in &self.slabs {
            let mut x = 0;
            let mut y = 0;
            for _ in 0..slab.count {
                if x + slab.entry_w > slab_w {
                    x = 0;
                    y += slab.entry_h;
                }
                preview.fill_rect(
                    PixelRect { x: slab.x + x, y: slab.y + y, w: slab.entry_w, h: slab.entry_h },
                    UsagePreviewColors::Wasted.rgba(),
                );
                x += slab.entry_w;
            }
        }

        // Pinta os glifos.
        for g in &self.allocated_glyphs {
            preview.fill_rect(PixelRect { x: g.x, y: g.y, w: g.w, h: g.h }, UsagePreviewColors::Used.rgba());
        }

        // Pinta o espaço sem uso nas bordas.
        for r in self.unused_regions() {
            preview.fill_rect(PixelRect { x: r.x, y: r.y, w: r.w, h: r.h }, UsagePreviewColors::Restricted.rgba());
        }

        // Sobrepõe os glifos reais por cima.
        preview.draw_image(&self.canvas.borrow(), PixelRect { x: 0, y: 0, w, h }, 0, 0, 0.5);
        preview
    }

    fn get_stats(&self) -> String {
        let (w, h) = self.canvas_size();

        let mut slab_entry_pixels: i64 = 0;
        let mut used_pixels: i64 = 0;
        let mut slab_edge_pixels: i64 = 0;
        let mut restricted_pixels: i64 = 0;
        let total_pixels = i64::from(w) * i64::from(h);
        let slab_w = default_slab_size(self.device_pixel_ratio);
        let slab_h = slab_w;

        for slab in &self.slabs {
            let entry_pixels = i64::from(slab.entry_w) * i64::from(slab.entry_h);
            slab_entry_pixels += i64::from(slab.count) * entry_pixels;
            let entries_per_row = i64::from(count_in(slab_w, slab.entry_w));
            let entries_per_col = i64::from(count_in(slab_h, slab.entry_h));
            let this_slab_pixels = entry_pixels * entries_per_row * entries_per_col;
            slab_edge_pixels += i64::from(slab_w) * i64::from(slab_h) - this_slab_pixels;
        }

        for g in &self.allocated_glyphs {
            used_pixels += i64::from(g.w) * i64::from(g.h);
        }

        for r in self.unused_regions() {
            restricted_pixels += i64::from(r.w) * i64::from(r.h);
        }

        let edge_used_pixels = slab_edge_pixels - restricted_pixels;
        let wasted_pixels = slab_entry_pixels - (used_pixels - edge_used_pixels);

        let efficiency = used_pixels as f64 / (used_pixels + wasted_pixels + restricted_pixels) as f64;
        let percent_of_total = |pixels: i64| to_fixed(pixels as f64 / total_pixels as f64 * 100.0, 2);

        [
            format!("page[{}]:", self.texture_index),
            format!("     Total: {total_pixels}px ({w}x{h})"),
            format!("      Used: {used_pixels}px ({}%)", percent_of_total(used_pixels)),
            format!("    Wasted: {wasted_pixels}px ({}%)", percent_of_total(wasted_pixels)),
            format!(
                "Restricted: {restricted_pixels}px ({}%) (hard to allocate)",
                percent_of_total(restricted_pixels)
            ),
            format!(
                "Efficiency: {}%",
                if efficiency == 1.0 { "100".to_owned() } else { to_fixed(efficiency * 100.0, 2) }
            ),
            format!(
                "     Slabs: {} of {}",
                self.slabs.len(),
                u64::from(count_in(w, slab_w)) * u64::from(count_in(h, slab_h))
            ),
        ]
        .join("\n")
    }
}
