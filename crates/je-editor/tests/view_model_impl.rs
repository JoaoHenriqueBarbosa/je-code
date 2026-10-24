//! Porte de `src/vs/editor/test/browser/viewModel/viewModelImpl.test.ts` (suíte `ViewModel`, com a suíte aninhada
//! `hidden areas must always leave at least one visible line`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `testViewModel(text, opts, (viewModel, model) => ...)` é `test_view_model` de `tests/support/test_view_model.rs`;
//!   `{}` é `no_options()`.
//! - `model.pushEditOperations([], [{ range, text }], () => ([]))` é [`push_edit`]: estado de cursor anterior vazio e o
//!   computador de estado que devolve `[]`.
//! - `model.applyEdits([{ range, text }])` usa [`edit`]; o `text: null` de uma remoção é `None`.
//! - O `new class extends ViewEventHandler { handleEvents(...) }` é o [`LineCountRecorder`], que sobrescreve
//!   `handle_events`. O `eventHandler.dispose()` não tem equivalente: o `ViewEventHandler` do porte só guarda o
//!   estado de renderização, sem nada a descartar.
//! - `viewModel.setHiddenAreas(ranges, 'sourceA')`: a fonte (`unknown`) é a identidade do dono, o `Some(id)` do
//!   `HiddenAreasSource`, e `forceUpdate` omitido é `false`.
//! - `assert.ok(viewModel.getVisibleRanges() !== null)` não tem o que conferir num `Vec` (nunca é nulo): o caso chama
//!   `get_visible_ranges` e termina sem pânico, que é o que o original de fato exercita.
//! - `createCodeEditorServices(disposables)` e `instantiateTestCodeEditor(instantiationService, model)` são as funções
//!   de mesmo nome de `tests/support/test_code_editor.rs`; os descartáveis entram no `DisposableStore` na ordem do
//!   original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, to_disposable};
use je_base::lifecycle::IDisposable as _;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::model::{
    EndOfLineSequence, InjectedTextOptions, ModelDecorationOptionsInit, ModelDeltaDecoration, PositionAffinity,
    TextModelUpdateOptions, ValidEditOperation,
};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::{ModelDecorationOptions, TextModel};
use je_editor::view_events::{ShouldRenderState, ViewEvent, ViewEventHandler};
use je_editor::view_model_types::{IViewModel, PlainTextSource};
use support::test_code_editor::{
    TestCodeEditorInstantiationOptions, create_code_editor_services, instantiate_test_code_editor,
};
use support::test_text_model::create_text_model;
use support::test_view_model::{TestViewModel, no_options, test_view_model};

/// `{ range, text }` de `applyEdits`, com `text: null` quando `value` é `None`.
fn edit(range: Range, value: Option<&str>) -> SingleEditOperation {
    SingleEditOperation::new(range, value.map(JsString::from), false)
}

/// `model.pushEditOperations([], [{ range, text }], () => ([]))`.
fn push_edit(model: &TextModel, range: Range, value: &str) {
    let mut cursor_state_computer = |_: &[ValidEditOperation]| -> Option<Vec<Selection>> { Some(Vec::new()) };
    model.push_edit_operations(Some(Vec::new()), &[edit(range, Some(value))], Some(&mut cursor_state_computer), None, None);
}

/// `viewModel.getLineCount() >= 1`, com a mensagem do original.
fn assert_has_visible_line(view_model: &TestViewModel) {
    let line_count = view_model.get_line_count();
    assert!(line_count >= 1, "expected at least 1 view line but got {line_count}");
}

/// O handler do caso `#44805`: a cada lote de eventos, anota o número de linhas da visão.
struct LineCountRecorder {
    should_render: ShouldRenderState,
    view_model: Rc<TestViewModel>,
    view_line_count: Rc<RefCell<Vec<i32>>>,
}

impl ViewEventHandler for LineCountRecorder {
    fn should_render_state(&self) -> &ShouldRenderState {
        &self.should_render
    }

    fn handle_events(&self, _events: &[ViewEvent]) {
        // Access the view model
        self.view_line_count.borrow_mut().push(self.view_model.get_line_count());
    }
}

#[test]
fn issue_21073_split_lines_collection_attempt_to_access_a_newer_model() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [""];
    let opts = Dynamic::object([("lineNumbersMinChars", Dynamic::from(1))]);
    test_view_model(&text, &opts, |view_model, model, _| {
        assert_eq!(view_model.get_line_count(), 1);

        view_model.set_viewport(1, 1, 1);

        model.apply_edits(&[edit(
            Range::new(1, 1, 1, 1),
            Some(
                &[
                    "line01", "line02", "line03", "line04", "line05", "line06", "line07", "line08", "line09", "line10",
                ]
                .join("\n"),
            ),
        )]);

        assert_eq!(view_model.get_line_count(), 10);
    });
}

#[test]
fn issue_44805_split_lines_collection_attempt_to_access_a_newer_model() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [""];
    test_view_model(&text, &no_options(), |view_model, model, _| {
        assert_eq!(view_model.get_line_count(), 1);

        push_edit(model, Range::new(1, 1, 1, 1), "\ninsert1");
        push_edit(model, Range::new(1, 1, 1, 1), "\ninsert2");
        push_edit(model, Range::new(1, 1, 1, 1), "\ninsert3");

        let view_line_count: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));

        view_line_count.borrow_mut().push(view_model.get_line_count());
        let event_handler: Rc<dyn ViewEventHandler> = Rc::new(LineCountRecorder {
            should_render: ShouldRenderState::default(),
            view_model: view_model.clone(),
            view_line_count: view_line_count.clone(),
        });
        view_model.add_view_event_handler(event_handler.clone());
        model.undo();
        view_line_count.borrow_mut().push(view_model.get_line_count());

        assert_eq!(*view_line_count.borrow(), vec![4, 1, 1, 1, 1]);

        view_model.remove_view_event_handler(&event_handler);
    });
}

#[test]
fn view_models_react_first_to_model_changes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let initial_text = ["Hello", "world"];
    let disposables = DisposableStore::new();

    let model = create_text_model(&initial_text.join("\n"));
    let model_to_dispose = model.clone();
    disposables.add(to_disposable(move || model_to_dispose.dispose()));
    let instantiation_service = create_code_editor_services(&disposables, None);
    let options = TestCodeEditorInstantiationOptions::default().options;
    let ed1 = disposables.add_rc(instantiate_test_code_editor(&instantiation_service, Some(model.clone()), &options));
    disposables.add_rc(instantiate_test_code_editor(&instantiation_service, Some(model.clone()), &options));

    // Add a nasty listener which modifies the model during the model change event
    let is_first = Cell::new(true);
    let listener_model = model.clone();
    disposables.add(ed1.on_did_change_model_content().listen(move |_e| {
        if is_first.get() {
            is_first.set(false);
            // delete the \n
            listener_model.apply_edits(&[edit(Range::new(1, 6, 2, 1), Some(""))]);
        }
    }));

    model.apply_edits(&[edit(Range::new(2, 6, 2, 6), Some("!"))]);

    disposables.dispose();
}

#[test]
fn issue_44805_no_visible_lines_via_api_call() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = ["line1", "line2", "line3"];
    test_view_model(&text, &no_options(), |view_model, _model, _| {
        assert_eq!(view_model.get_line_count(), 3);
        view_model.set_hidden_areas(vec![Range::new(1, 1, 3, 1)], None, false);
        let _visible_ranges = view_model.get_visible_ranges();
    });
}

#[test]
fn issue_44805_no_visible_lines_via_undoing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [""];
    test_view_model(&text, &no_options(), |view_model, model, _| {
        assert_eq!(view_model.get_line_count(), 1);

        push_edit(model, Range::new(1, 1, 1, 1), "line1\nline2\nline3");

        view_model.set_hidden_areas(vec![Range::new(1, 1, 1, 1)], None, false);
        assert_eq!(view_model.get_line_count(), 2);

        model.undo();
        let _visible_ranges = view_model.get_visible_ranges();
    });
}

const USUAL_TEXT: [&str; 5] = ["", "line2", "line3", "line4", ""];

/// `PlainTextSource` de uma `string`.
fn source_text(value: &str) -> PlainTextSource {
    PlainTextSource::Text(JsString::from(value))
}

/// `PlainTextSource` de um `string[]`.
fn source_lines(values: &[&str]) -> PlainTextSource {
    PlainTextSource::Lines(values.iter().map(|value| JsString::from(*value)).collect())
}

fn assert_get_plain_text_to_copy(text: &[&str], ranges: &[Range], empty_selection_clipboard: bool, expected: PlainTextSource) {
    test_view_model(text, &no_options(), |view_model, _model, _| {
        let actual = view_model.get_plain_text_to_copy(ranges, empty_selection_clipboard, false);
        assert_eq!(actual.source_text, expected);
    });
}

#[test]
fn get_plain_text_to_copy_0_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(&USUAL_TEXT, &[Range::new(2, 2, 2, 2)], false, source_text(""));
}

#[test]
fn get_plain_text_to_copy_0_1_empty_selection_clipboard() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(&USUAL_TEXT, &[Range::new(2, 2, 2, 2)], true, source_text("line2\n"));
}

#[test]
fn get_plain_text_to_copy_1_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(&USUAL_TEXT, &[Range::new(2, 2, 2, 6)], false, source_text("ine2"));
}

#[test]
fn get_plain_text_to_copy_1_1_empty_selection_clipboard() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(&USUAL_TEXT, &[Range::new(2, 2, 2, 6)], true, source_text("ine2"));
}

#[test]
fn get_plain_text_to_copy_0_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(
        &USUAL_TEXT,
        &[Range::new(2, 2, 2, 2), Range::new(3, 2, 3, 2)],
        false,
        source_text(""),
    );
}

#[test]
fn get_plain_text_to_copy_0_2_empty_selection_clipboard() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(
        &USUAL_TEXT,
        &[Range::new(2, 2, 2, 2), Range::new(3, 2, 3, 2)],
        true,
        source_lines(&["line2\n", "line3\n"]),
    );
}

#[test]
fn issue_256039_get_plain_text_to_copy_with_multiple_cursors_and_empty_selections_should_return_array() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Bug: When copying with multiple cursors (empty selections) with emptySelectionClipboard enabled,
    // the result should be an array so that pasting with "editor.multiCursorPaste": "full"
    // correctly distributes each line to the corresponding cursor.
    // Without the fix, this returns 'line2\nline3\n' (a single string).
    // With the fix, this returns ['line2\n', 'line3\n'] (an array).
    assert_get_plain_text_to_copy(
        &USUAL_TEXT,
        &[Range::new(2, 1, 2, 1), Range::new(3, 1, 3, 1)],
        true,
        source_lines(&["line2\n", "line3\n"]),
    );
}

#[test]
fn get_plain_text_to_copy_1_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(
        &USUAL_TEXT,
        &[Range::new(2, 2, 2, 6), Range::new(3, 2, 3, 2)],
        false,
        source_text("ine2"),
    );
}

#[test]
fn get_plain_text_to_copy_1_2_empty_selection_clipboard() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(
        &USUAL_TEXT,
        &[Range::new(2, 2, 2, 6), Range::new(3, 2, 3, 2)],
        true,
        source_lines(&["ine2", "line3\n"]),
    );
}

#[test]
fn get_plain_text_to_copy_2_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(
        &USUAL_TEXT,
        &[Range::new(2, 2, 2, 6), Range::new(3, 2, 3, 6)],
        false,
        source_lines(&["ine2", "ine3"]),
    );
}

#[test]
fn get_plain_text_to_copy_2_2_reversed() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(
        &USUAL_TEXT,
        &[Range::new(3, 2, 3, 6), Range::new(2, 2, 2, 6)],
        false,
        source_lines(&["ine2", "ine3"]),
    );
}

#[test]
fn get_plain_text_to_copy_0_3_empty_selection_clipboard() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_get_plain_text_to_copy(
        &USUAL_TEXT,
        &[Range::new(2, 2, 2, 2), Range::new(2, 3, 2, 3), Range::new(3, 2, 3, 2)],
        true,
        source_lines(&["line2\n", "line3\n"]),
    );
}

#[test]
fn issue_22688_always_use_crlf_for_clipboard_on_windows() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_view_model(&USUAL_TEXT, &no_options(), |view_model, model, _| {
        model.set_eol(EndOfLineSequence::Lf);
        let actual = view_model.get_plain_text_to_copy(&[Range::new(2, 1, 5, 1)], true, true);
        assert_eq!(actual.source_text, source_text("line2\r\nline3\r\nline4\r\n"));
    });
}

#[test]
fn issue_40926_incorrect_spacing_when_inserting_new_line_after_multiple_folded_blocks_of_code() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_view_model(
        &[
            "foo = {",
            "    foobar: function() {",
            "        this.foobar();",
            "    },",
            "    foobar: function() {",
            "        this.foobar();",
            "    },",
            "    foobar: function() {",
            "        this.foobar();",
            "    },",
            "}",
        ],
        &no_options(),
        |view_model, model, _| {
            view_model.set_hidden_areas(
                vec![Range::new(3, 1, 3, 1), Range::new(6, 1, 6, 1), Range::new(9, 1, 9, 1)],
                None,
                false,
            );

            model.apply_edits(&[
                edit(Range::new(4, 7, 4, 7), Some("\n    ")),
                edit(Range::new(7, 7, 7, 7), Some("\n    ")),
                edit(Range::new(10, 7, 10, 7), Some("\n    ")),
            ]);

            assert_eq!(view_model.get_line_count(), 11);
        },
    );
}

/// O `{ range, options: { description: 'test', before: { content }, showIfCollapsed: true } }` do caso
/// `normalizePosition with multiple touching injected text`.
fn injected_before(content: &str) -> ModelDeltaDecoration {
    ModelDeltaDecoration {
        range: Range::new(1, 8, 1, 8),
        options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "test".to_owned(),
            before: Some(InjectedTextOptions { content: content.to_owned(), ..InjectedTextOptions::default() }),
            show_if_collapsed: true,
            ..Default::default()
        }),
    }
}

#[test]
fn normalize_position_with_multiple_touching_injected_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_view_model(&["just some text"], &no_options(), |view_model, model, _| {
        model.delta_decorations(&[], &[injected_before("bar"), injected_before("bz")], 0);

        // just sobarbzme text

        let normalize = |line_number: i32, column: i32, affinity: PositionAffinity| {
            view_model.normalize_position(Position::new(line_number, column), affinity)
        };

        assert_eq!(normalize(1, 8, PositionAffinity::None), Position::new(1, 8));
        assert_eq!(normalize(1, 9, PositionAffinity::None), Position::new(1, 8));
        assert_eq!(normalize(1, 11, PositionAffinity::None), Position::new(1, 11));
        assert_eq!(normalize(1, 12, PositionAffinity::None), Position::new(1, 11));
        assert_eq!(normalize(1, 13, PositionAffinity::None), Position::new(1, 13));

        assert_eq!(normalize(1, 8, PositionAffinity::Left), Position::new(1, 8));
        assert_eq!(normalize(1, 9, PositionAffinity::Left), Position::new(1, 8));
        assert_eq!(normalize(1, 11, PositionAffinity::Left), Position::new(1, 8));
        assert_eq!(normalize(1, 12, PositionAffinity::Left), Position::new(1, 8));
        assert_eq!(normalize(1, 13, PositionAffinity::Left), Position::new(1, 8));

        assert_eq!(normalize(1, 8, PositionAffinity::Right), Position::new(1, 13));
        assert_eq!(normalize(1, 9, PositionAffinity::Right), Position::new(1, 13));
        assert_eq!(normalize(1, 11, PositionAffinity::Right), Position::new(1, 13));
        assert_eq!(normalize(1, 12, PositionAffinity::Right), Position::new(1, 13));
        assert_eq!(normalize(1, 13, PositionAffinity::Right), Position::new(1, 13));
    });
}

#[test]
fn issue_193262_incorrect_implementation_of_modify_position() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let opts = Dynamic::object([
        ("wordWrap", Dynamic::from("wordWrapColumn")),
        ("wordWrapColumn", Dynamic::from(5)),
    ]);
    test_view_model(&["just some text"], &opts, |view_model, _model, _| {
        assert_eq!(Position::new(3, 1), view_model.modify_position(Position::new(3, 2), -1));
    });
}

mod hidden_areas_must_always_leave_at_least_one_visible_line {
    use super::*;

    #[test]
    fn replacing_the_only_visible_line_content_does_not_make_it_hidden() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["line1", "line2", "line3"];
        test_view_model(&text, &no_options(), |view_model, model, _| {
            // Hide lines 1 and 3, leaving only line 2 visible
            view_model.set_hidden_areas(vec![Range::new(1, 1, 1, 1), Range::new(3, 1, 3, 1)], None, false);
            assert_eq!(view_model.get_line_count(), 1);

            // Replace line 2 content entirely
            model.apply_edits(&[edit(Range::new(2, 1, 2, 6), Some("new content"))]);

            assert_has_visible_line(view_model);
        });
    }

    #[test]
    fn deleting_the_only_visible_line_when_it_is_the_last_line() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["line1", "line2", "line3"];
        test_view_model(&text, &no_options(), |view_model, model, _| {
            // Hide lines 1-2, leaving only line 3 visible
            view_model.set_hidden_areas(vec![Range::new(1, 1, 2, 1)], None, false);
            assert_eq!(view_model.get_line_count(), 1);

            // Delete line 3 by merging it into line 2
            model.apply_edits(&[edit(Range::new(2, 6, 3, 6), None)]);

            assert_has_visible_line(view_model);
        });
    }

    #[test]
    fn deleting_the_only_visible_line_when_it_is_in_the_middle() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["line1", "line2", "line3", "line4", "line5"];
        test_view_model(&text, &no_options(), |view_model, model, _| {
            // Hide lines 1-2 and 4-5, leaving only line 3 visible
            view_model.set_hidden_areas(vec![Range::new(1, 1, 2, 1), Range::new(4, 1, 5, 1)], None, false);
            assert_eq!(view_model.get_line_count(), 1);

            // Delete line 3 by merging adjacent lines
            model.apply_edits(&[edit(Range::new(2, 6, 4, 1), None)]);

            assert_has_visible_line(view_model);
        });
    }

    #[test]
    fn undo_that_removes_the_only_visible_line() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["line1"];
        test_view_model(&text, &no_options(), |view_model, model, _| {
            assert_eq!(view_model.get_line_count(), 1);

            // Insert lines to create content
            push_edit(model, Range::new(1, 6, 1, 6), "\nline2\nline3\nline4\nline5");

            assert_eq!(view_model.get_line_count(), 5);

            // Hide lines 1-2 and 4-5, leaving only line 3 visible
            view_model.set_hidden_areas(vec![Range::new(1, 1, 2, 1), Range::new(4, 1, 5, 1)], None, false);
            assert_eq!(view_model.get_line_count(), 1);

            // Undo collapses back to 1 line, but hidden area decorations may grow
            model.undo();

            assert_has_visible_line(view_model);
        });
    }

    #[test]
    fn deleting_the_only_visible_line_between_two_hidden_areas_leaves_all_lines_hidden() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["line1", "line2", "line3", "line4", "line5", "line6", "line7", "line8"];
        test_view_model(&text, &no_options(), |view_model, model, _| {
            assert_eq!(view_model.get_line_count(), 8);

            // Hide lines 1-5 and 7-8, leaving only line 6 visible
            view_model.set_hidden_areas(vec![Range::new(1, 1, 5, 1), Range::new(7, 1, 8, 1)], None, false);
            assert_eq!(view_model.get_line_count(), 1);

            // Delete lines 6, 7, 8, the only visible line plus some hidden ones
            model.apply_edits(&[edit(Range::new(6, 1, 8, 5), None)]);

            // The view model must still have at least one visible line
            assert_has_visible_line(view_model);
        });
    }

    #[test]
    fn multiple_visible_lines_deleted_leaving_only_hidden_lines() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["hidden1", "hidden2", "visible1", "visible2", "hidden3", "hidden4"];
        test_view_model(&text, &no_options(), |view_model, model, _| {
            view_model.set_hidden_areas(vec![Range::new(1, 1, 2, 1), Range::new(5, 1, 6, 1)], None, false);
            assert_eq!(view_model.get_line_count(), 2);

            // Delete visible lines 3 and 4
            model.apply_edits(&[edit(Range::new(2, 8, 5, 1), None)]);

            assert_has_visible_line(view_model);
        });
    }

    #[test]
    fn hidden_areas_from_multiple_sources_that_overlap_produce_valid_merged_result() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text: Vec<String> = (1..=10).map(|i| format!("line{i}")).collect();
        let text: Vec<&str> = text.iter().map(String::as_str).collect();
        test_view_model(&text, &no_options(), |view_model, _model, _| {
            // Source A hides a large range [1-8].
            // Source B hides small ranges [2-3] and [5-6] that are subsumed by A.
            // mergeLineRangeArray has a bug where it advances both pointers after
            // merging [1-8]+[2-3]=[1-8], leaving [5-6] and [8,9] as separate entries
            // that overlap with or are subsumed by [1-8].
            // normalizeLineRanges in setHiddenAreas cleans this up, so the result
            // should still be correct: lines 1-8 hidden, lines 9-10 visible.
            view_model.set_hidden_areas(vec![Range::new(1, 1, 8, 1)], Some(1), false);
            view_model.set_hidden_areas(
                vec![Range::new(2, 1, 3, 1), Range::new(5, 1, 6, 1), Range::new(8, 1, 9, 1)],
                Some(2),
                false,
            );

            // Lines 1-9 should be hidden (merged from [1-8] and [8-9]), line 10 visible
            assert_eq!(view_model.get_line_count(), 1, "only line 10 should be visible");

            // The hidden areas returned should be non-overlapping and sorted
            let hidden_areas = view_model.get_hidden_areas();
            for i in 1..hidden_areas.len() {
                assert!(
                    hidden_areas[i].start_line_number > hidden_areas[i - 1].end_line_number,
                    "hidden areas should not overlap: [{}-{}] and [{}-{}]",
                    hidden_areas[i - 1].start_line_number,
                    hidden_areas[i - 1].end_line_number,
                    hidden_areas[i].start_line_number,
                    hidden_areas[i].end_line_number
                );
            }
        });
    }

    #[test]
    fn tab_size_change_with_drifted_hidden_area_decorations_must_not_leave_0_visible_lines() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["line1", "line2", "line3"];
        test_view_model(&text, &no_options(), |view_model, model, _| {
            // Hide lines 1-2, leaving only line 3 visible.
            view_model.set_hidden_areas(vec![Range::new(1, 1, 2, 1)], None, false);
            assert_eq!(view_model.get_line_count(), 1);

            // Insert at (2,1), the end edge of the hidden area decoration.
            // AlwaysGrowsWhenTypingAtEdges causes the decoration to grow from
            // [1,1 -> 2,1] to [1,1 -> 3,1], covering what was the visible line 3.
            // After this insert, the file has 4 lines, decoration covers [1-3], line 4 visible.
            model.apply_edits(&[edit(Range::new(2, 1, 2, 1), Some("x\n"))]);
            // Insert again to push decoration further
            model.apply_edits(&[edit(Range::new(3, 1, 3, 1), Some("y\n"))]);
            // Now file has 5 lines, decoration covers [1-4], line 5 visible.

            // Delete lines 4-5 to collapse back, making decoration cover everything
            model.apply_edits(&[edit(Range::new(4, 1, 5, 6), Some(""))]);
            // Now file has 4 lines. acceptVersionId ensures viewLines >= 1.

            // Tab size change: triggers _constructLines(resetHiddenAreas=false)
            // which re-reads the decoration ranges (which may cover all lines).
            model.update_options(TextModelUpdateOptions { tab_size: Some(8), ..Default::default() });

            assert_has_visible_line(view_model);
        });
    }
}
