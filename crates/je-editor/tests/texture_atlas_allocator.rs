//! Porte de `src/vs/editor/test/browser/gpu/atlas/textureAtlasAllocator.test.ts` (suíte
//! `TextureAtlasAllocator`), caso a caso e na mesma ordem, com os auxiliares de
//! `src/vs/editor/test/browser/gpu/atlas/testUtil.ts` que ele usa (`assertIsValidGlyph`).
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `OffscreenCanvas` e `ImageData` viram [`PixelPage`] (página de pixels RGBA com largura e altura):
//!   `createRasterizedGlyph` monta a página com `PixelPage::from_rgba`, que faz o papel do
//!   `imageData.data.set(data)` seguido do `putImageData` na origem.
//! - As constantes `pixel1x1`, `pixel2x1` e `pixel1x2` viram funções, porque o glifo guarda um `Rc`.
//! - `assertIsValidGlyph(glyph, atlasOrSource)` recebe só a página (o ramo do `TextureAtlas` fica de
//!   fora: `textureAtlas.ts` não foi portado). O original calcula `pageH` com `atlasOrSource.width`
//!   também (provável descuido dele); o porte repete, e as páginas dos casos que o usam são quadradas.
//!   As conferências `isNumber(...)` e `>= 0` sobre `x`, `y` e as origens não existem: os tipos (`u32`,
//!   `i32`) já garantem número e não negatividade.
//! - O `ok(edge.some(color => (color & 0xFF) !== 0))` vira `any(|byte| byte != 0)`, sobre os bytes RGBA.
//! - `throws(fn, new BugIndicatingError(...))` vira `errors::catch` sobre `allocate_and_assert`, que relança
//!   o `Err` como `allocate` lançaria, e a comparação com o `Error` esperado.
//! - O `suite('shared tests')` com o laço sobre `allocatorDefinitions` vira os quatro casos em sequência
//!   (shelf e slab, nessa ordem), com o corpo comum em `check_single_allocation` e
//!   `check_glyph_too_large_for_canvas`.
//! - O `test.skip` do original (falha na web só ao conferir a mensagem) vira `#[ignore]`, mantendo o corpo:
//!   com `cargo test -- --ignored` ele roda e passa, porque aqui não há a falha da web.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors::{self, Error};
use je_editor::gpu::atlas::{ITextureAtlasAllocator, ITextureAtlasPageGlyph};
use je_editor::gpu::gpu_utils::{PixelPage, SharedPixelPage};
use je_editor::gpu::raster::{IBoundingBox, IOriginOffset, IRasterizedGlyph};
use je_editor::gpu::texture_atlas_shelf_allocator::TextureAtlasShelfAllocator;
use je_editor::gpu::texture_atlas_slab_allocator::{TextureAtlasSlabAllocator, TextureAtlasSlabAllocatorOptions};

const BLACK_ARR: [u8; 4] = [0x00, 0x00, 0x00, 0xFF];

fn pixel_1x1() -> IRasterizedGlyph {
    create_rasterized_glyph(1, 1, &BLACK_ARR)
}

fn pixel_2x1() -> IRasterizedGlyph {
    create_rasterized_glyph(2, 1, &[BLACK_ARR, BLACK_ARR].concat())
}

fn pixel_1x2() -> IRasterizedGlyph {
    create_rasterized_glyph(1, 2, &[BLACK_ARR, BLACK_ARR].concat())
}

fn create_rasterized_glyph(w: u32, h: u32, data: &[u8]) -> IRasterizedGlyph {
    assert_eq!(w as usize * h as usize * 4, data.len());
    let source = PixelPage::from_rgba(w, h, data.to_vec()).unwrap();
    IRasterizedGlyph {
        source: Rc::new(source),
        bounding_box: IBoundingBox { top: 0, left: 0, bottom: h - 1, right: w - 1 },
        origin_offset: IOriginOffset { x: 0, y: 0 },
        font_bounding_box_ascent: 0.0,
        font_bounding_box_descent: 0.0,
    }
}

/// `{ x, y, w, h }`.
type Placement = (u32, u32, u32, u32);

fn allocate_and_assert(allocator: &mut dyn ITextureAtlasAllocator, rasterized_glyph: &IRasterizedGlyph, expected: Option<Placement>) {
    let actual = allocator.allocate(rasterized_glyph).unwrap_or_else(|error| errors::throw(error));
    let Some(actual) = actual else {
        assert_eq!(None, expected);
        return;
    };
    assert_eq!(Some((actual.x, actual.y, actual.w, actual.h)), expected);
}

fn init_shelf_allocator(w: u32, h: u32) -> (SharedPixelPage, Box<dyn ITextureAtlasAllocator>) {
    let canvas: SharedPixelPage = Rc::new(RefCell::new(PixelPage::new(w, h)));
    let allocator = TextureAtlasShelfAllocator::new(canvas.clone(), 0);
    (canvas, Box::new(allocator))
}

fn init_slab_allocator(
    w: u32,
    h: u32,
    options: Option<TextureAtlasSlabAllocatorOptions>,
) -> (SharedPixelPage, Box<dyn ITextureAtlasAllocator>) {
    let canvas: SharedPixelPage = Rc::new(RefCell::new(PixelPage::new(w, h)));
    let allocator = TextureAtlasSlabAllocator::new(canvas.clone(), 0, options);
    (canvas, Box::new(allocator))
}

/// `{ slabW, slabH }`.
fn slab_size(slab_w: u32, slab_h: u32) -> Option<TextureAtlasSlabAllocatorOptions> {
    Some(TextureAtlasSlabAllocatorOptions { slab_w: Some(slab_w), slab_h: Some(slab_h), device_pixel_ratio: None })
}

/// `assertIsValidGlyph` de `testUtil.ts`.
fn assert_is_valid_glyph(glyph: Option<ITextureAtlasPageGlyph>, source: &SharedPixelPage) {
    let Some(glyph) = glyph else {
        panic!("glyph is undefined");
    };
    let source = source.borrow();
    let page_w = source.width();
    let page_h = source.width();

    // (x,y) are valid coordinates
    assert!(glyph.x < page_w);
    assert!(glyph.y < page_h);

    // (w,h) are valid dimensions
    assert!(glyph.w > 0);
    assert!(glyph.w <= page_w);
    assert!(glyph.h > 0);
    assert!(glyph.h <= page_h);

    // (x,y) + (w,h) are within the bounds of the atlas
    assert!(glyph.x + glyph.w <= page_w);
    assert!(glyph.y + glyph.h <= page_h);

    // Each of the glyph's outer pixel edges contain at least 1 non-transparent pixel
    let edges = [
        source.get_image_data(glyph.x, glyph.y, glyph.w, 1),
        source.get_image_data(glyph.x, glyph.y + glyph.h - 1, glyph.w, 1),
        source.get_image_data(glyph.x, glyph.y, 1, glyph.h),
        source.get_image_data(glyph.x + glyph.w - 1, glyph.y, 1, glyph.h),
    ];
    for edge in edges {
        assert!(edge.iter().any(|color| *color != 0));
    }
}

fn check_single_allocation(canvas: &SharedPixelPage, allocator: &mut dyn ITextureAtlasAllocator) {
    assert_is_valid_glyph(allocator.allocate(&pixel_1x1()).unwrap(), canvas);
}

fn check_glyph_too_large_for_canvas(allocator: &mut dyn ITextureAtlasAllocator) {
    let thrown = errors::catch(|| allocate_and_assert(allocator, &pixel_2x1(), None));
    assert_eq!(thrown.unwrap_err(), Error::bug_indicating(Some("Glyph is too large for the atlas page")));
}

// suite('shared tests')

#[test]
fn shared_shelf_single_allocation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (canvas, mut allocator) = init_shelf_allocator(2, 2);
    check_single_allocation(&canvas, allocator.as_mut());
}

// Skipping because it fails unexpectedly on web only when asserting the error message
#[test]
#[ignore = "test.skip no original: falha na web só ao conferir a mensagem do erro"]
fn shared_shelf_glyph_too_large_for_canvas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_shelf_allocator(1, 1);
    check_glyph_too_large_for_canvas(allocator.as_mut());
}

#[test]
fn shared_slab_single_allocation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (canvas, mut allocator) = init_slab_allocator(2, 2, None);
    check_single_allocation(&canvas, allocator.as_mut());
}

// Skipping because it fails unexpectedly on web only when asserting the error message
#[test]
#[ignore = "test.skip no original: falha na web só ao conferir a mensagem do erro"]
fn shared_slab_glyph_too_large_for_canvas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_slab_allocator(1, 1, None);
    check_glyph_too_large_for_canvas(allocator.as_mut());
}

// suite('TextureAtlasShelfAllocator')

#[test]
fn shelf_single_allocation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_shelf_allocator(2, 2);
    // 1o
    // oo
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 0, 1, 1)));
}

#[test]
fn shelf_wrapping() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_shelf_allocator(5, 4);

    // 1233o
    // o2ooo
    // ooooo
    // ooooo
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 0, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x2(), Some((1, 0, 1, 2)));
    allocate_and_assert(allocator.as_mut(), &pixel_2x1(), Some((2, 0, 2, 1)));

    // 1233x
    // x2xxx
    // 44556
    // ooooo
    allocate_and_assert(allocator.as_mut(), &pixel_2x1(), Some((0, 2, 2, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_2x1(), Some((2, 2, 2, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((4, 2, 1, 1)));

    // 1233x
    // x2xxx
    // 44556
    // 7oooo
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 3, 1, 1)));
}

#[test]
fn shelf_full() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_shelf_allocator(3, 2);
    // 122
    // 1oo
    allocate_and_assert(allocator.as_mut(), &pixel_1x2(), Some((0, 0, 1, 2)));
    allocate_and_assert(allocator.as_mut(), &pixel_2x1(), Some((1, 0, 2, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), None);
}

// suite('TextureAtlasSlabAllocator')

#[test]
fn slab_single_allocation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_slab_allocator(2, 2, None);
    // 1o
    // oo
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 0, 1, 1)));
}

#[test]
fn slab_single_slab_single_glyph_full() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_slab_allocator(1, 1, slab_size(1, 1));

    // 1
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 0, 1, 1)));

    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), None);
}

#[test]
fn slab_single_slab_multiple_glyph_full() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_slab_allocator(2, 2, slab_size(2, 2));

    // 1
    // 1
    allocate_and_assert(allocator.as_mut(), &pixel_1x2(), Some((0, 0, 1, 2)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x2(), Some((1, 0, 1, 2)));

    allocate_and_assert(allocator.as_mut(), &pixel_1x2(), None);
}

#[test]
fn slab_allocate_1x1_to_multiple_slabs_until_full() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_slab_allocator(4, 2, slab_size(2, 2));

    // 12│oo
    // 34│oo
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 0, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((1, 0, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 1, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((1, 1, 1, 1)));

    // 12│56
    // 34│78
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((2, 0, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((3, 0, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((2, 1, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((3, 1, 1, 1)));

    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), None);
}

#[test]
fn slab_glyph_too_large_for_slab_increase_slab_size_for_first_glyph() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_slab_allocator(2, 2, slab_size(1, 1));
    allocate_and_assert(allocator.as_mut(), &pixel_2x1(), Some((0, 0, 2, 1)));
}

#[test]
fn slab_glyph_too_large_for_slab_undefined_as_its_not_the_first_glyph() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_slab_allocator(2, 2, slab_size(1, 1));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 0, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_2x1(), None);
}

#[test]
fn slab_separate_slabs_for_different_sized_glyphs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_canvas, mut allocator) = init_slab_allocator(4, 2, slab_size(2, 2));

    // 10│2o
    // 00│2o
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((0, 0, 1, 1)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x2(), Some((2, 0, 1, 2)));

    // 14│23
    // 00│23
    allocate_and_assert(allocator.as_mut(), &pixel_1x2(), Some((3, 0, 1, 2)));
    allocate_and_assert(allocator.as_mut(), &pixel_1x1(), Some((1, 0, 1, 1)));
}
