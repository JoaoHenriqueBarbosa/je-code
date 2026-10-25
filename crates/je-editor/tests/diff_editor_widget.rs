//! Porte de `src/vs/editor/test/browser/widget/diffEditorWidget.test.ts` (suíte `DiffEditorWidget2`), caso a caso e na mesma ordem,
//! menos os dois casos de abrir e fechar as regiões sem mudanças pelo teclado (`compact disclosures expand and collapse with a
//! persistent keyboard target`, um por valor de `renderSideBySide`): eles consultam o DOM (`container.querySelector('.editor.modified
//! .disclosure-toggle')`, `aria-expanded`, `document.activeElement`) e dependem do `HideUnchangedRegionsFeature`, que é a fatia 3
//! do editor de diff. Entram com ela.
//!
//! Notas de tradução:
//! - O `deepStrictEqual` sobre objetos vira a comparação de tuplas de campos, na ordem do objeto do original.
//! - `new TestAccessibilityService()` é o [`TestAccessibilityService`] daqui: sem leitor de tela, e nenhum evento dispara.
//! - `options.setWidth(width)` e `options.setWidth(width, smoothResizeStartWidth)` são `set_width(width, None)` e
//!   `set_width(width, Some(..))`; `options.resetWidthBasedLayout()` é `reset_width_based_layout(None)`.
//! - `serialize(regions)` usa o `Display` de `LineRange` (`[início,fim)`), que é o `toString()` do original.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::observable::Observable;
use je_base::promise::Promise;
use je_editor::diff::range_mapping::DetailedLineRangeMapping;
use je_editor::diff_editor_options::DiffEditorOptions;
use je_editor::diff_editor_view_model::UnchangedRegion;
use je_editor::line_range::LineRange;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};

/// `TestAccessibilityService`: sem leitor de tela, e nenhum evento dispara.
struct TestAccessibilityService;

impl IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }
    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
    fn is_screen_reader_optimized(&self) -> bool {
        false
    }
    fn is_motion_reduced(&self) -> bool {
        false
    }
    fn is_transparency_reduced(&self) -> bool {
        false
    }
    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }
    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

/// `new DiffEditorOptions({ renderSideBySide: true, renderSideBySideInlineBreakpoint: 900, useInlineViewWhenSpaceIsLimited: true },
/// new TestAccessibilityService())`.
fn width_based_options() -> DiffEditorOptions {
    let options = Dynamic::object([
        ("renderSideBySide", Dynamic::Bool(true)),
        ("renderSideBySideInlineBreakpoint", Dynamic::Number(900.0)),
        ("useInlineViewWhenSpaceIsLimited", Dynamic::Bool(true)),
    ]);
    DiffEditorOptions::new(&options, Rc::new(TestAccessibilityService))
}

// #region width based layout

#[test]
fn commits_temporary_inline_when_smoothly_enlarging_from_automatic_inline() {
    let options = width_based_options();

    options.set_width(1000.0, None);
    let initially_side_by_side = options.render_side_by_side.get();
    options.set_width(800.0, Some(1000.0));
    let inline_during_resize = options.render_side_by_side.get();
    let temporary_inline_after_shrinking = options.temporary_inline_mode.get();
    options.set_width(1000.0, Some(1000.0));
    let restored_during_resize = options.render_side_by_side.get();
    options.set_width(800.0, Some(1000.0));
    let temporary_inline_after_ending_narrow = options.temporary_inline_mode.get();
    options.set_width(1000.0, Some(800.0));
    let wide_after_inline_was_committed = options.render_side_by_side.get();
    let temporary_inline_mode = options.temporary_inline_mode.get();
    options.set_width(800.0, None);
    let temporary_inline_after_becoming_narrow = options.temporary_inline_mode.get();
    options.set_width(1000.0, Some(800.0));
    options.reset_width_based_layout(None);
    let wide_after_resetting_automatic = options.render_side_by_side.get();
    options.set_width(800.0, None);
    let automatic_inline_result = options.render_side_by_side_in_automatic_mode.get();
    options.set_width(1000.0, None);
    let automatic_side_by_side_result = options.render_side_by_side_in_automatic_mode.get();
    options.update_options(&Dynamic::object([("renderSideBySide", Dynamic::Bool(false))]));
    options.update_options(&Dynamic::object([("renderSideBySide", Dynamic::Bool(true))]));

    assert_eq!(
        (
            initially_side_by_side,
            inline_during_resize,
            temporary_inline_after_shrinking,
            restored_during_resize,
            temporary_inline_after_ending_narrow,
            wide_after_inline_was_committed,
            temporary_inline_mode,
            temporary_inline_after_becoming_narrow,
            wide_after_resetting_automatic,
            automatic_inline_result,
            automatic_side_by_side_result,
            options.render_side_by_side.get(),
        ),
        (true, false, false, true, false, false, true, false, true, false, true, true),
    );
}

#[test]
fn keeps_auto_layout_after_a_non_resize_layout_change() {
    let options = width_based_options();

    options.set_width(800.0, None);
    let narrow = options.render_side_by_side.get();
    options.set_width(1000.0, None);

    assert_eq!((narrow, options.render_side_by_side.get()), (false, true));
}

// #endregion

// #region UnchangedRegion

fn serialize(regions: &[Rc<UnchangedRegion>]) -> Vec<String> {
    regions.iter().map(|r| format!("{} - {}", r.original_unchanged_range(), r.modified_unchanged_range())).collect()
}

/// `new DetailedLineRangeMapping(new LineRange(a, b), new LineRange(c, d), [])`.
fn change(original: (i32, i32), modified: (i32, i32)) -> DetailedLineRangeMapping {
    DetailedLineRangeMapping::new(LineRange::new(original.0, original.1), LineRange::new(modified.0, modified.1), Some(Vec::new()))
}

#[test]
fn everything_changed() {
    assert_eq!(serialize(&UnchangedRegion::from_diffs(&[change((1, 10), (1, 10))], 10, 10, 3, 3)), Vec::<String>::new());
}

#[test]
fn nothing_changed() {
    assert_eq!(serialize(&UnchangedRegion::from_diffs(&[], 10, 10, 3, 3)), vec!["[1,11) - [1,11)"]);
}

#[test]
fn change_in_the_middle() {
    assert_eq!(
        serialize(&UnchangedRegion::from_diffs(&[change((50, 60), (50, 60))], 100, 100, 3, 3)),
        vec!["[1,47) - [1,47)", "[63,101) - [63,101)"],
    );
}

#[test]
fn change_at_the_end() {
    assert_eq!(serialize(&UnchangedRegion::from_diffs(&[change((99, 100), (100, 100))], 100, 100, 3, 3)), vec!["[1,96) - [1,96)"]);
}

// #endregion
