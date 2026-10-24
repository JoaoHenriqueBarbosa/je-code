//! Porte de `src/vs/editor/test/browser/viewModel/viewModelDecorations.test.ts` (suíte `ViewModelDecorations`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `testViewModel(text, opts, (viewModel, model) => ...)` é `test_view_model` de `tests/support/test_view_model.rs`.
//! - `model.changeDecorations((accessor) => ...)` é `change_decorations(callback, 0)` (o `ownerId` omitido vale `0`) e as
//!   opções `{ description, className, ... }` são `ModelDecorationOptions::register(&ModelDecorationOptionsInit { .. })`.
//! - `.map(dec => dec.options.className).filter(Boolean)` descarta o `className` ausente ou vazio, e
//!   `filter(x => Boolean(x.options.beforeContentClassName))` a mesma coisa para `beforeContentClassName`.
//! - `assert.deepStrictEqual(decorations, [])` sobre as decorações do `ViewModel` é conferir que a lista está vazia
//!   (`ViewModelDecoration` não implementa `Debug`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::inline_decorations::{InlineDecoration, InlineDecorationType};
use je_editor::model::ModelDecorationOptionsInit;
use je_editor::range::Range;
use je_editor::text_model::ModelDecorationOptions;
use je_editor::view_model_types::IViewModel;
use support::test_view_model::{no_options, test_view_model, TestViewModel};

/// A configuração `{ wordWrap: 'wordWrapColumn', wordWrapColumn: 13 }` dos dois primeiros casos.
fn word_wrap_column_13() -> Dynamic {
    Dynamic::object([("wordWrap", Dynamic::from("wordWrapColumn")), ("wordWrapColumn", Dynamic::from(13))])
}

/// As cinco linhas da visão do texto quebrado em 13 colunas.
fn assert_wrapped_lines(view_model: &TestViewModel) {
    assert_eq!(view_model.get_line_content(1), JsString::from("hello world, "));
    assert_eq!(view_model.get_line_content(2), JsString::from("this is a "));
    assert_eq!(view_model.get_line_content(3), JsString::from("buffer that "));
    assert_eq!(view_model.get_line_content(4), JsString::from("will be "));
    assert_eq!(view_model.get_line_content(5), JsString::from("wrapped"));
}

/// `createOpts(id)` do primeiro caso.
fn create_opts(id: &str) -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "test".to_owned(),
        class_name: Some(id.to_owned()),
        inline_class_name: Some(format!("i-{id}")),
        before_content_class_name: Some(format!("b-{id}")),
        after_content_class_name: Some(format!("a-{id}")),
        ..Default::default()
    })
}

/// `new InlineDecoration(range, className, type)` com o `Range` em quatro números.
fn inline(
    range: (i32, i32, i32, i32),
    class_name: &str,
    r#type: InlineDecorationType,
) -> InlineDecoration {
    InlineDecoration::new(Range::new(range.0, range.1, range.2, range.3), class_name, r#type)
}

#[test]
fn get_decorations_viewport_data() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = ["hello world, this is a buffer that will be wrapped"];
    test_view_model(&text, &word_wrap_column_13(), |view_model, model, _| {
        assert_wrapped_lines(view_model);

        model.change_decorations(
            |accessor| {
                // VIEWPORT will be (1,14) -> (1,36)

                // completely before viewport
                accessor.add_decoration(&Range::new(1, 2, 1, 3), create_opts("dec1"));
                // starts before viewport, ends at viewport start
                accessor.add_decoration(&Range::new(1, 2, 1, 14), create_opts("dec2"));
                // starts before viewport, ends inside viewport
                accessor.add_decoration(&Range::new(1, 2, 1, 15), create_opts("dec3"));
                // starts before viewport, ends at viewport end
                accessor.add_decoration(&Range::new(1, 2, 1, 36), create_opts("dec4"));
                // starts before viewport, ends after viewport
                accessor.add_decoration(&Range::new(1, 2, 1, 51), create_opts("dec5"));

                // starts at viewport start, ends at viewport start (will not be visible on view line 2)
                accessor.add_decoration(&Range::new(1, 14, 1, 14), create_opts("dec6"));
                // starts at viewport start, ends inside viewport
                accessor.add_decoration(&Range::new(1, 14, 1, 16), create_opts("dec7"));
                // starts at viewport start, ends at viewport end
                accessor.add_decoration(&Range::new(1, 14, 1, 36), create_opts("dec8"));
                // starts at viewport start, ends after viewport
                accessor.add_decoration(&Range::new(1, 14, 1, 51), create_opts("dec9"));

                // starts inside viewport, ends inside viewport
                accessor.add_decoration(&Range::new(1, 16, 1, 18), create_opts("dec10"));
                // starts inside viewport, ends at viewport end
                accessor.add_decoration(&Range::new(1, 16, 1, 36), create_opts("dec11"));
                // starts inside viewport, ends after viewport
                accessor.add_decoration(&Range::new(1, 16, 1, 51), create_opts("dec12"));

                // starts at viewport end, ends at viewport end
                accessor.add_decoration(&Range::new(1, 36, 1, 36), create_opts("dec13"));
                // starts at viewport end, ends after viewport
                accessor.add_decoration(&Range::new(1, 36, 1, 51), create_opts("dec14"));

                // starts after viewport, ends after viewport
                accessor.add_decoration(&Range::new(1, 40, 1, 51), create_opts("dec15"));
            },
            0,
        );

        let actual_decorations: Vec<String> = view_model
            .get_decorations_in_viewport(Range::new(
                2,
                view_model.get_line_min_column(2),
                3,
                view_model.get_line_max_column(3),
            ))
            .iter()
            .filter_map(|dec| dec.options.class_name.clone())
            .filter(|class_name| !class_name.is_empty())
            .collect();

        let expected_decorations: Vec<String> = [
            "dec1", "dec2", "dec3", "dec4", "dec5", "dec6", "dec7", "dec8", "dec9", "dec10", "dec11", "dec12", "dec13",
            "dec14",
        ]
        .iter()
        .map(|class_name| (*class_name).to_owned())
        .collect();
        assert_eq!(actual_decorations, expected_decorations);

        let inline_decorations1 = view_model
            .get_viewport_view_line_rendering_data(
                Range::new(1, view_model.get_line_min_column(1), 2, view_model.get_line_max_column(2)),
                1,
            )
            .inline_decorations;

        // view line 1: (1,1 -> 1,14)
        assert_eq!(
            inline_decorations1,
            vec![
                inline((1, 2, 1, 3), "i-dec1", InlineDecorationType::Regular),
                inline((1, 2, 1, 2), "b-dec1", InlineDecorationType::Before),
                inline((1, 3, 1, 3), "a-dec1", InlineDecorationType::After),
                inline((1, 2, 1, 14), "i-dec2", InlineDecorationType::Regular),
                inline((1, 2, 1, 2), "b-dec2", InlineDecorationType::Before),
                inline((1, 14, 1, 14), "a-dec2", InlineDecorationType::After),
                inline((1, 2, 2, 2), "i-dec3", InlineDecorationType::Regular),
                inline((1, 2, 1, 2), "b-dec3", InlineDecorationType::Before),
                inline((1, 2, 3, 13), "i-dec4", InlineDecorationType::Regular),
                inline((1, 2, 1, 2), "b-dec4", InlineDecorationType::Before),
                inline((1, 2, 5, 8), "i-dec5", InlineDecorationType::Regular),
                inline((1, 2, 1, 2), "b-dec5", InlineDecorationType::Before),
            ]
        );

        let inline_decorations2 = view_model
            .get_viewport_view_line_rendering_data(
                Range::new(2, view_model.get_line_min_column(2), 3, view_model.get_line_max_column(3)),
                2,
            )
            .inline_decorations;

        // view line 2: (1,14 -> 1,24)
        assert_eq!(
            inline_decorations2,
            vec![
                inline((1, 2, 2, 2), "i-dec3", InlineDecorationType::Regular),
                inline((2, 2, 2, 2), "a-dec3", InlineDecorationType::After),
                inline((1, 2, 3, 13), "i-dec4", InlineDecorationType::Regular),
                inline((1, 2, 5, 8), "i-dec5", InlineDecorationType::Regular),
                inline((2, 1, 2, 1), "i-dec6", InlineDecorationType::Regular),
                inline((2, 1, 2, 1), "b-dec6", InlineDecorationType::Before),
                inline((2, 1, 2, 1), "a-dec6", InlineDecorationType::After),
                inline((2, 1, 2, 3), "i-dec7", InlineDecorationType::Regular),
                inline((2, 1, 2, 1), "b-dec7", InlineDecorationType::Before),
                inline((2, 3, 2, 3), "a-dec7", InlineDecorationType::After),
                inline((2, 1, 3, 13), "i-dec8", InlineDecorationType::Regular),
                inline((2, 1, 2, 1), "b-dec8", InlineDecorationType::Before),
                inline((2, 1, 5, 8), "i-dec9", InlineDecorationType::Regular),
                inline((2, 1, 2, 1), "b-dec9", InlineDecorationType::Before),
                inline((2, 3, 2, 5), "i-dec10", InlineDecorationType::Regular),
                inline((2, 3, 2, 3), "b-dec10", InlineDecorationType::Before),
                inline((2, 5, 2, 5), "a-dec10", InlineDecorationType::After),
                inline((2, 3, 3, 13), "i-dec11", InlineDecorationType::Regular),
                inline((2, 3, 2, 3), "b-dec11", InlineDecorationType::Before),
                inline((2, 3, 5, 8), "i-dec12", InlineDecorationType::Regular),
                inline((2, 3, 2, 3), "b-dec12", InlineDecorationType::Before),
            ]
        );

        let inline_decorations3 = view_model
            .get_viewport_view_line_rendering_data(
                Range::new(2, view_model.get_line_min_column(2), 3, view_model.get_line_max_column(3)),
                3,
            )
            .inline_decorations;

        // view line 3 (24 -> 36)
        assert_eq!(
            inline_decorations3,
            vec![
                inline((1, 2, 3, 13), "i-dec4", InlineDecorationType::Regular),
                inline((3, 13, 3, 13), "a-dec4", InlineDecorationType::After),
                inline((1, 2, 5, 8), "i-dec5", InlineDecorationType::Regular),
                inline((2, 1, 3, 13), "i-dec8", InlineDecorationType::Regular),
                inline((3, 13, 3, 13), "a-dec8", InlineDecorationType::After),
                inline((2, 1, 5, 8), "i-dec9", InlineDecorationType::Regular),
                inline((2, 3, 3, 13), "i-dec11", InlineDecorationType::Regular),
                inline((3, 13, 3, 13), "a-dec11", InlineDecorationType::After),
                inline((2, 3, 5, 8), "i-dec12", InlineDecorationType::Regular),
            ]
        );
    });
}

#[test]
fn issue_17208_problem_scrolling_in_1_8_0() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = ["hello world, this is a buffer that will be wrapped"];
    test_view_model(&text, &word_wrap_column_13(), |view_model, model, _| {
        assert_wrapped_lines(view_model);

        model.change_decorations(
            |accessor| {
                accessor.add_decoration(
                    &Range::new(1, 50, 1, 51),
                    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "test".to_owned(),
                        before_content_class_name: Some("dec1".to_owned()),
                        ..Default::default()
                    }),
                );
            },
            0,
        );

        let decorations: Vec<_> = view_model
            .get_decorations_in_viewport(Range::new(
                2,
                view_model.get_line_min_column(2),
                3,
                view_model.get_line_max_column(3),
            ))
            .into_iter()
            .filter(|x| x.options.before_content_class_name.as_deref().is_some_and(|name| !name.is_empty()))
            .collect();
        assert!(decorations.is_empty());

        let inline_decorations1 = view_model
            .get_viewport_view_line_rendering_data(
                Range::new(2, view_model.get_line_min_column(2), 3, view_model.get_line_max_column(3)),
                2,
            )
            .inline_decorations;
        assert_eq!(inline_decorations1, Vec::<InlineDecoration>::new());

        let inline_decorations2 = view_model
            .get_viewport_view_line_rendering_data(
                Range::new(2, view_model.get_line_min_column(2), 3, view_model.get_line_max_column(3)),
                3,
            )
            .inline_decorations;
        assert_eq!(inline_decorations2, Vec::<InlineDecoration>::new());
    });
}

#[test]
fn issue_37401_allow_both_before_and_after_decorations_on_empty_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [""];
    test_view_model(&text, &no_options(), |view_model, model, _| {
        model.change_decorations(
            |accessor| {
                accessor.add_decoration(
                    &Range::new(1, 1, 1, 1),
                    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "test".to_owned(),
                        before_content_class_name: Some("before1".to_owned()),
                        after_content_class_name: Some("after1".to_owned()),
                        ..Default::default()
                    }),
                );
            },
            0,
        );

        let inline_decorations = view_model
            .get_viewport_view_line_rendering_data(Range::new(1, 1, 1, 1), 1)
            .inline_decorations;
        assert_eq!(
            inline_decorations,
            vec![
                inline((1, 1, 1, 1), "before1", InlineDecorationType::Before),
                inline((1, 1, 1, 1), "after1", InlineDecorationType::After),
            ]
        );
    });
}
