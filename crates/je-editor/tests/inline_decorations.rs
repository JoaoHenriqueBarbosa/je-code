//! Porte de `src/vs/editor/test/common/viewModel/inlineDecorations.test.ts` (suítes
//! `InlineModelDecorationsComputer` e `InjectedTextInlineDecorationsComputer`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createTextModel(text)` é `create_text_model` de `tests/support/test_text_model.rs`.
//! - `createModelDecoration(id, range, options)` recebe as opções já no formato final
//!   (`ModelDecorationOptions`), como o objeto cru do original, sem passar pela normalização do `register`.
//! - Os contextos de função do original viram structs que implementam as traits de contexto.
//! - `assert.strictEqual`/`notStrictEqual` entre decorações da visão vira `Rc::ptr_eq`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::coordinates_converter::IdentityCoordinatesConverter;
use je_editor::inline_decorations::{
    IInjectedTextInlineDecorationsComputerContext, IInlineDecorationsComputer, IInlineModelDecorationsComputerContext,
    InjectedTextInlineDecorationsComputer, InlineDecoration, InlineDecorationType, InlineModelDecorationsComputer,
    ViewDecorationsCollection,
};
use je_editor::model::{InjectedTextOptions, ModelDecoration};
use je_editor::range::Range;
use je_editor::text_model::ModelDecorationOptions;
use support::test_text_model::create_text_model;

fn create_model_decoration(id: &str, range: Range, options: ModelDecorationOptions) -> ModelDecoration {
    ModelDecoration {
        id: id.to_owned(),
        owner_id: 0,
        range,
        options: Rc::new(options),
    }
}

/// O objeto cru `{ description: 'test', ... }` do original: só o que o caso preenche fica ligado.
fn options(configure: impl FnOnce(&mut ModelDecorationOptions)) -> ModelDecorationOptions {
    let mut options = (*ModelDecorationOptions::empty()).clone();
    options.description = "test".to_owned();
    configure(&mut options);
    options
}

fn inline(range: Range, class_name: &str, r#type: InlineDecorationType) -> InlineDecoration {
    InlineDecoration::new(range, class_name, r#type)
}

struct FixedModelDecorations(Vec<ModelDecoration>);

impl IInlineModelDecorationsComputerContext for FixedModelDecorations {
    fn get_model_decorations(&self, _view_range: &Range, _only_minimap: bool, _only_margin: bool) -> Vec<ModelDecoration> {
        self.0.clone()
    }
}

struct InjectedContext {
    injection_options: Option<Vec<InjectedTextOptions>>,
    injection_offsets: Option<Vec<i32>>,
    break_offsets: Vec<i32>,
    wrapped_text_indent_length: i32,
    base_view_line_number: i32,
}

impl InjectedContext {
    fn new(
        injection_options: Vec<InjectedTextOptions>,
        injection_offsets: Vec<i32>,
        break_offsets: Vec<i32>,
        wrapped_text_indent_length: i32,
        base_view_line_number: i32,
    ) -> Rc<Self> {
        Rc::new(Self {
            injection_options: Some(injection_options),
            injection_offsets: Some(injection_offsets),
            break_offsets,
            wrapped_text_indent_length,
            base_view_line_number,
        })
    }
}

impl IInjectedTextInlineDecorationsComputerContext for InjectedContext {
    fn get_injection_options(&self, _model_line_number: i32) -> Option<Vec<InjectedTextOptions>> {
        self.injection_options.clone()
    }

    fn get_injection_offsets(&self, _model_line_number: i32) -> Option<Vec<i32>> {
        self.injection_offsets.clone()
    }

    fn get_break_offsets(&self, _model_line_number: i32) -> Vec<i32> {
        self.break_offsets.clone()
    }

    fn get_wrapped_text_indent_length(&self, _model_line_number: i32) -> i32 {
        self.wrapped_text_indent_length
    }

    fn get_base_view_line_number(&self, _model_line_number: i32) -> i32 {
        self.base_view_line_number
    }
}

fn injected(content: &str, inline_class_name: Option<&str>) -> InjectedTextOptions {
    InjectedTextOptions {
        content: content.to_owned(),
        inline_class_name: inline_class_name.map(str::to_owned),
        ..Default::default()
    }
}

fn spacer(inline_class_name: Option<&str>) -> InjectedTextOptions {
    InjectedTextOptions {
        content: String::new(),
        inline_class_name: inline_class_name.map(str::to_owned),
        width_in_em: Some(1.0),
        ..Default::default()
    }
}

fn model_computer(text: &str, decorations: Vec<ModelDecoration>) -> (Rc<je_editor::text_model::TextModel>, InlineModelDecorationsComputer) {
    let model = create_text_model(text);
    let coordinates_converter = Rc::new(IdentityCoordinatesConverter::new(model.clone()));
    let context = Rc::new(FixedModelDecorations(decorations));
    let computer = InlineModelDecorationsComputer::new(context, model.clone(), coordinates_converter);
    (model, computer)
}

// Suíte InlineModelDecorationsComputer

#[test]
fn no_decorations() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer("hello world", vec![]);
    let result = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert_eq!(
        result,
        ViewDecorationsCollection {
            decorations: vec![],
            inline_decorations: vec![vec![]],
            has_variable_fonts: vec![false],
        }
    );
    model.dispose();
}

#[test]
fn inline_class_name_decoration_on_a_single_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "hello world",
        vec![create_model_decoration(
            "dec1",
            Range::new(1, 1, 1, 6),
            options(|o| o.inline_class_name = Some("test-class".to_owned())),
        )],
    );
    let result = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert_eq!(result.decorations.len(), 1);
    assert_eq!(
        result.inline_decorations,
        vec![vec![inline(Range::new(1, 1, 1, 6), "test-class", InlineDecorationType::Regular)]]
    );
    assert_eq!(result.has_variable_fonts, vec![false]);
    model.dispose();
}

#[test]
fn inline_class_name_with_affects_letter_spacing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "hello world",
        vec![create_model_decoration(
            "dec1",
            Range::new(1, 1, 1, 6),
            options(|o| {
                o.inline_class_name = Some("test-class".to_owned());
                o.inline_class_name_affects_letter_spacing = true;
            }),
        )],
    );
    let result = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert_eq!(
        result.inline_decorations,
        vec![vec![inline(Range::new(1, 1, 1, 6), "test-class", InlineDecorationType::RegularAffectingLetterSpacing)]]
    );
    model.dispose();
}

#[test]
fn before_content_class_name_decoration() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "hello world",
        vec![create_model_decoration(
            "dec1",
            Range::new(1, 3, 1, 8),
            options(|o| o.before_content_class_name = Some("before-class".to_owned())),
        )],
    );
    let result = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert_eq!(
        result.inline_decorations,
        vec![vec![inline(Range::new(1, 3, 1, 3), "before-class", InlineDecorationType::Before)]]
    );
    model.dispose();
}

#[test]
fn after_content_class_name_decoration() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "hello world",
        vec![create_model_decoration(
            "dec1",
            Range::new(1, 3, 1, 8),
            options(|o| o.after_content_class_name = Some("after-class".to_owned())),
        )],
    );
    let result = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert_eq!(
        result.inline_decorations,
        vec![vec![inline(Range::new(1, 8, 1, 8), "after-class", InlineDecorationType::After)]]
    );
    model.dispose();
}

#[test]
fn all_decoration_types_combined() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "hello world",
        vec![create_model_decoration(
            "dec1",
            Range::new(1, 2, 1, 6),
            options(|o| {
                o.inline_class_name = Some("inline-class".to_owned());
                o.before_content_class_name = Some("before-class".to_owned());
                o.after_content_class_name = Some("after-class".to_owned());
            }),
        )],
    );
    let result = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert_eq!(
        result.inline_decorations,
        vec![vec![
            inline(Range::new(1, 2, 1, 6), "inline-class", InlineDecorationType::Regular),
            inline(Range::new(1, 2, 1, 2), "before-class", InlineDecorationType::Before),
            inline(Range::new(1, 6, 1, 6), "after-class", InlineDecorationType::After),
        ]]
    );
    model.dispose();
}

#[test]
fn decoration_spanning_multiple_lines() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "line one\nline two\nline three",
        vec![create_model_decoration(
            "dec1",
            Range::new(1, 3, 3, 5),
            options(|o| o.inline_class_name = Some("multi-line".to_owned())),
        )],
    );
    let result = computer.get_decorations(Range::new(1, 1, 3, 11), false, false);
    let expected_inline_decoration = inline(Range::new(1, 3, 3, 5), "multi-line", InlineDecorationType::Regular);
    assert_eq!(
        result.inline_decorations,
        vec![
            vec![expected_inline_decoration.clone()],
            vec![expected_inline_decoration.clone()],
            vec![expected_inline_decoration],
        ]
    );
    model.dispose();
}

#[test]
fn decoration_with_affects_font_sets_has_variable_fonts() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "hello world",
        vec![create_model_decoration(
            "dec1",
            Range::new(1, 1, 1, 6),
            options(|o| {
                o.inline_class_name = Some("font-class".to_owned());
                o.affects_font = Some(true);
            }),
        )],
    );
    let result = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert_eq!(result.has_variable_fonts, vec![true]);
    model.dispose();
}

#[test]
fn multiple_decorations_on_different_lines() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "line one\nline two",
        vec![
            create_model_decoration("dec1", Range::new(1, 1, 1, 5), options(|o| o.inline_class_name = Some("class-a".to_owned()))),
            create_model_decoration("dec2", Range::new(2, 1, 2, 5), options(|o| o.inline_class_name = Some("class-b".to_owned()))),
        ],
    );
    let result = computer.get_decorations(Range::new(1, 1, 2, 9), false, false);
    assert_eq!(
        result.inline_decorations,
        vec![
            vec![inline(Range::new(1, 1, 1, 5), "class-a", InlineDecorationType::Regular)],
            vec![inline(Range::new(2, 1, 2, 5), "class-b", InlineDecorationType::Regular)],
        ]
    );
    model.dispose();
}

#[test]
fn decoration_cache_is_used_for_same_decoration_id() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let dec = create_model_decoration(
        "dec1",
        Range::new(1, 1, 1, 6),
        options(|o| o.inline_class_name = Some("test-class".to_owned())),
    );
    let (model, computer) = model_computer("hello world", vec![dec]);
    let result1 = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    let result2 = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert!(Rc::ptr_eq(&result1.decorations[0], &result2.decorations[0]));
    model.dispose();
}

#[test]
fn reset_clears_decoration_cache() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let dec = create_model_decoration(
        "dec1",
        Range::new(1, 1, 1, 6),
        options(|o| o.inline_class_name = Some("test-class".to_owned())),
    );
    let (model, computer) = model_computer("hello world", vec![dec]);
    let result1 = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    computer.reset();
    let result2 = computer.get_decorations(Range::new(1, 1, 1, 12), false, false);
    assert!(!Rc::ptr_eq(&result1.decorations[0], &result2.decorations[0]));
    model.dispose();
}

#[test]
fn get_inline_decorations_returns_inline_decorations_for_a_model_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (model, computer) = model_computer(
        "hello world",
        vec![create_model_decoration(
            "dec1",
            Range::new(1, 1, 1, 6),
            options(|o| o.inline_class_name = Some("test-class".to_owned())),
        )],
    );
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, vec![vec![inline(Range::new(1, 1, 1, 6), "test-class", InlineDecorationType::Regular)]]);
    model.dispose();
}

// Suíte InjectedTextInlineDecorationsComputer

#[test]
fn no_injections_returns_empty() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = Rc::new(InjectedContext {
        injection_options: None,
        injection_offsets: None,
        break_offsets: vec![10],
        wrapped_text_indent_length: 0,
        base_view_line_number: 1,
    });
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, Vec::<Vec<InlineDecoration>>::new());
}

#[test]
fn single_injection_with_inline_class_name_on_a_single_output_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // 10 (original) + 8 (injected)
    let context = InjectedContext::new(vec![injected("injected", Some("injected-class"))], vec![5], vec![18], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, vec![vec![inline(Range::new(1, 6, 1, 14), "injected-class", InlineDecorationType::Regular)]]);
}

#[test]
fn injection_without_inline_class_name_produces_no_inline_decorations() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![injected("injected", None)], vec![5], vec![18], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    // empty - no inlineClassName
    assert_eq!(result, vec![Vec::<InlineDecoration>::new()]);
}

#[test]
fn fixed_width_injection_uses_its_regular_inline_decoration() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let options = InjectedTextOptions {
        content: "\u{a0}".to_owned(),
        inline_class_name: Some("fixed-width".to_owned()),
        width_in_em: Some(3.0),
        ..Default::default()
    };
    let context = InjectedContext::new(vec![options], vec![5], vec![11], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, vec![vec![inline(Range::new(1, 6, 1, 7), "fixed-width", InlineDecorationType::Regular)]]);
}

#[test]
fn injection_with_inline_class_name_affects_letter_spacing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let options = InjectedTextOptions {
        content: "abc".to_owned(),
        inline_class_name: Some("ls-class".to_owned()),
        inline_class_name_affects_letter_spacing: true,
        ..Default::default()
    };
    // 10 + 3
    let context = InjectedContext::new(vec![options], vec![0], vec![13], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(
        result,
        vec![vec![inline(Range::new(1, 1, 1, 4), "ls-class", InlineDecorationType::RegularAffectingLetterSpacing)]]
    );
}

#[test]
fn multiple_injections_on_a_single_output_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // 10 + 2 + 3
    let context = InjectedContext::new(
        vec![injected("AA", Some("class-a")), injected("BBB", Some("class-b"))],
        vec![2, 5],
        vec![15],
        0,
        1,
    );
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(
        result,
        vec![vec![
            inline(Range::new(1, 3, 1, 5), "class-a", InlineDecorationType::Regular),
            inline(Range::new(1, 8, 1, 11), "class-b", InlineDecorationType::Regular),
        ]]
    );
}

#[test]
fn injection_spanning_across_wrapped_lines() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Original text is 20 chars, injection of 10 chars at offset 8
    // Break offsets split at 15 and 30 (two wrapped lines)
    let context = InjectedContext::new(vec![injected("1234567890", Some("injected"))], vec![8], vec![15, 30], 0, 5);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    // Injected text starts at offset 8 in the input with injections
    // Line 0: [0, 15), injected text occupies [8, 18) -> clipped to [8, 15)
    // Line 1: [15, 30), injected text occupies [8, 18) -> clipped to [15, 18) -> relative: [0, 3)
    assert_eq!(
        result,
        vec![
            vec![inline(Range::new(5, 9, 5, 16), "injected", InlineDecorationType::Regular)],
            vec![inline(Range::new(6, 1, 6, 4), "injected", InlineDecorationType::Regular)],
        ]
    );
}

#[test]
fn injection_with_wrapped_text_indent_length_on_wrapped_lines() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![injected("12345678901234567890", Some("injected"))], vec![0], vec![15, 30], 4, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    // Line 0 (outputLineIndex 0): no offset, start=0, end=15 -> columns 1 to 16
    // Line 1 (outputLineIndex 1): wrappedTextIndentLength=4, start=4+0=4, end=4+5=9 -> columns 5 to 10
    assert_eq!(
        result,
        vec![
            vec![inline(Range::new(1, 1, 1, 16), "injected", InlineDecorationType::Regular)],
            vec![inline(Range::new(2, 5, 2, 10), "injected", InlineDecorationType::Regular)],
        ]
    );
}

#[test]
fn injection_starting_in_later_wrapped_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Injection at offset 20 which is past the first line break; 30 + 2
    let context = InjectedContext::new(vec![injected("ab", Some("late-class"))], vec![20], vec![15, 32], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    // Line 0: [0, 15) -> injection at offset 20 is past this line -> empty
    // Line 1: [15, 32) -> injection at offset 20 -> start=20-15=5, end=22-15=7 -> columns 6 to 8
    assert_eq!(
        result,
        vec![
            vec![],
            vec![inline(Range::new(2, 6, 2, 8), "late-class", InlineDecorationType::Regular)],
        ]
    );
}

#[test]
fn base_view_line_number_offsets_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![injected("test", Some("test-class"))], vec![0], vec![14], 0, 10);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, vec![vec![inline(Range::new(10, 1, 10, 5), "test-class", InlineDecorationType::Regular)]]);
}

#[test]
fn range_uses_view_line_number_not_model_line_number() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Model line 3 maps to view line 7 (e.g. due to previous lines wrapping).
    // The range in the resulting InlineDecoration must use the view line number (7),
    // not the model line number (3) that is passed to getInlineDecorations().
    let model_line_number = 3;
    let base_view_line_number = 7;
    // 10 (original) + 5 (injected)
    let context = InjectedContext::new(vec![injected("ghost", Some("ghost-class"))], vec![0], vec![15], 0, base_view_line_number);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(model_line_number);
    // The range must reference view line 7, not model line 3
    assert_eq!(result, vec![vec![inline(Range::new(7, 1, 7, 6), "ghost-class", InlineDecorationType::Regular)]]);
}

#[test]
fn range_uses_view_line_number_on_wrapped_lines_not_model_line_number() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Model line 2 wraps into view lines 5 and 6.
    // Both output lines must use view line numbers, not model line 2.
    let model_line_number = 2;
    let base_view_line_number = 5;
    let context = InjectedContext::new(vec![injected("1234567890", Some("wrap-class"))], vec![0], vec![8, 20], 0, base_view_line_number);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(model_line_number);
    // First wrapped line uses view line 5, second uses view line 6
    assert_eq!(
        result,
        vec![
            vec![inline(Range::new(5, 1, 5, 9), "wrap-class", InlineDecorationType::Regular)],
            vec![inline(Range::new(6, 1, 6, 3), "wrap-class", InlineDecorationType::Regular)],
        ]
    );
}

#[test]
fn spacing_only_injection_in_the_middle_of_a_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![spacer(Some("spacer"))], vec![5], vec![10], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, vec![vec![inline(Range::new(1, 6, 1, 6), "spacer", InlineDecorationType::WidthOnly)]]);
}

#[test]
fn spacing_only_injection_at_the_beginning_of_a_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![spacer(Some("spacer"))], vec![0], vec![10], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, vec![vec![inline(Range::new(1, 1, 1, 1), "spacer", InlineDecorationType::WidthOnly)]]);
}

#[test]
fn spacing_only_injection_on_an_empty_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![spacer(Some("spacer"))], vec![0], vec![0], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, vec![vec![inline(Range::new(1, 1, 1, 1), "spacer", InlineDecorationType::WidthOnly)]]);
}

#[test]
fn spacing_only_injection_at_a_wrap_boundary_belongs_to_the_following_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![spacer(Some("spacer"))], vec![5], vec![5, 10], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(
        result,
        vec![vec![], vec![inline(Range::new(2, 1, 2, 1), "spacer", InlineDecorationType::WidthOnly)]]
    );
}

#[test]
fn spacing_only_injection_at_the_end_of_a_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![spacer(Some("spacer"))], vec![10], vec![10], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    // There is no following output line to move it to, so it stays at the end of this one.
    assert_eq!(result, vec![vec![inline(Range::new(1, 11, 1, 11), "spacer", InlineDecorationType::WidthOnly)]]);
}

#[test]
fn spacing_only_injection_next_to_an_injection_with_content() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // 10 (original) + 0 + 2
    let context = InjectedContext::new(
        vec![spacer(Some("spacer")), injected("AB", Some("text-class"))],
        vec![2, 5],
        vec![12],
        0,
        1,
    );
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    // The spacing-only injection must not shift the offsets of the injections that follow it.
    assert_eq!(
        result,
        vec![vec![
            inline(Range::new(1, 3, 1, 3), "spacer", InlineDecorationType::WidthOnly),
            inline(Range::new(1, 6, 1, 8), "text-class", InlineDecorationType::Regular),
        ]]
    );
}

#[test]
fn spacing_only_injection_without_inline_class_name_produces_no_inline_decorations() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context = InjectedContext::new(vec![spacer(None)], vec![5], vec![10], 0, 1);
    let computer = InjectedTextInlineDecorationsComputer::new(context);
    let result = computer.get_inline_decorations(1);
    assert_eq!(result, vec![Vec::<InlineDecoration>::new()]);
}
