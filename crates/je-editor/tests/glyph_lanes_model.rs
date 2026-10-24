//! Porte de `src/vs/editor/test/common/viewModel/glyphLanesModel.test.ts` (suíte `GlyphLanesModel`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de `je-base/tests/common`, criado no começo de cada
//!   caso (o modelo não cria disposables, o guarda só confere que nada vazou).
//! - O `setup` (`model = new GlyphMarginLanesModel(10)`) é [`setup`], chamado por cada caso.
//! - `lineRange(a, b)` é `Range::new(a, 1, b, 1)`; `push(lane, range)` sem `persist` é `push(lane, range, false)`.
//! - `assert.deepStrictEqual` sobre arrays de faixas é `assert_eq!` sobre `Vec<GlyphMarginLane>`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_editor::glyph_lanes_model::{GlyphMarginLanesModel, IGlyphMarginLanesModel};
use je_editor::model::GlyphMarginLane;
use je_editor::range::Range;

/// O `setup` da suíte, mais o `ensureNoDisposablesAreLeakedInTestSuite()` que o antecede.
fn setup() -> (LeakGuard, GlyphMarginLanesModel) {
    (ensure_no_disposables_are_leaked_in_test_suite(), GlyphMarginLanesModel::new(10))
}

fn line_range(start_line_number: i32, end_line_number: i32) -> Range {
    Range::new(start_line_number, 1, end_line_number, 1)
}

fn assert_lines(model: &GlyphMarginLanesModel, from_line: i32, n: i32, expected: Vec<Vec<GlyphMarginLane>>) {
    let result: Vec<Vec<GlyphMarginLane>> = (0..n).map(|i| model.get_lanes_at_line(from_line + i)).collect();
    assert_eq!(result, expected, "fromLine: {from_line}, n: {n}");
}

#[test]
fn handles_empty() {
    let (_guard, model) = setup();
    assert_eq!(model.required_lanes(), 1);
    assert_lines(&model, 1, 1, vec![vec![GlyphMarginLane::Center]]);
}

#[test]
fn works_with_a_single_line_range() {
    let (_guard, model) = setup();
    model.push(GlyphMarginLane::Left, line_range(2, 3), false);
    assert_eq!(model.required_lanes(), 1);
    assert_lines(
        &model,
        1,
        5,
        vec![
            vec![GlyphMarginLane::Center], // 1
            vec![GlyphMarginLane::Left],   // 2
            vec![GlyphMarginLane::Left],   // 3
            vec![GlyphMarginLane::Center], // 4
            vec![GlyphMarginLane::Center], // 5
        ],
    );
}

#[test]
fn persists_ranges() {
    let (_guard, model) = setup();
    model.push(GlyphMarginLane::Left, line_range(2, 3), true);
    assert_eq!(model.required_lanes(), 1);
    assert_lines(
        &model,
        1,
        5,
        vec![
            vec![GlyphMarginLane::Left], // 1
            vec![GlyphMarginLane::Left], // 2
            vec![GlyphMarginLane::Left], // 3
            vec![GlyphMarginLane::Left], // 4
            vec![GlyphMarginLane::Left], // 5
        ],
    );
}

#[test]
fn handles_overlaps() {
    let (_guard, model) = setup();
    model.push(GlyphMarginLane::Left, line_range(6, 9), false);
    model.push(GlyphMarginLane::Right, line_range(5, 7), false);
    model.push(GlyphMarginLane::Center, line_range(7, 8), false);
    assert_eq!(model.required_lanes(), 3);
    assert_lines(
        &model,
        5,
        6,
        vec![
            vec![GlyphMarginLane::Right],                                                   // 5
            vec![GlyphMarginLane::Left, GlyphMarginLane::Right],                            // 6
            vec![GlyphMarginLane::Left, GlyphMarginLane::Center, GlyphMarginLane::Right],   // 7
            vec![GlyphMarginLane::Left, GlyphMarginLane::Center],                           // 8
            vec![GlyphMarginLane::Left],                                                    // 9
            vec![GlyphMarginLane::Center],                                                  // 10
        ],
    );
}
