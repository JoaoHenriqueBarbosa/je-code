//! Porte de `src/vs/editor/contrib/find/test/browser/findModel.test.ts` (suíte `FindModel`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup` e o `teardown` da suíte são [`find_test`] (o guarda, o
//!   `DisposableStore` `disposables` e o descarte) e [`run_plain_test`] (o caso `issue #288515`, que não passa por
//!   `findTest`).
//! - `findTest(name, callback)` roda o `callback` duas vezes, uma com as linhas do texto e outra com o buffer montado em
//!   três pedaços pelo `PieceTreeTextBufferBuilder` (`substr(0, 94)`, `substr(94, 101)`, `substr(195, 59)`): é
//!   [`find_test`], com `with_test_code_editor` e o `TestEditorInput::Factory`. O `disposables` é descartado ao fim de cada
//!   rodada, antes do editor (no original ele vive até o `teardown`, depois do editor; as `dispose()` explícitas dos casos já
//!   rodaram, e a segunda é inócua).
//! - `editor.trigger('mouse', CoreNavigationCommands.MoveTo.id, { position })` é `move_to` com o `MoveCommandOptions`
//!   (`source` `mouse`), pelo `run_core_editor_command` do comando registrado em `core_navigation_commands::instance()`,
//!   como os casos de `cursor.rs`.
//! - `model.getAllDecorations()` é `get_all_decorations(0, false, false)` e `dec.options.className` o `class_name`.
//! - `new Range(...)` no `searchScope` é `Some(Some(vec![...]))`; `searchScope: null` é `Some(None)`.
//! - `assert.deepStrictEqual(editor.getSelections().map(s => s.toString()), ...)` compara as `Selection` (a igualdade inclui
//!   o sentido, o que o `toString` do original também carrega).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_editor::code_editor_widget::{EditorViewModel, ICodeEditor};
use je_editor::core_commands::{MoveCommandOptions, core_navigation_commands};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::find::find_model::FindModelBoundToEditorModel;
use je_editor::find::find_state::{FindReplaceState, INewFindReplaceState};
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferBuilder;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};

/// `[startLineNumber, startColumn, endLineNumber, endColumn]` (`fromRange`).
type R = [i32; 4];

/// As ocorrências de `hello` sem palavra inteira.
const HELLO_ALL: [R; 5] = [[6, 14, 6, 19], [6, 27, 6, 32], [7, 14, 7, 19], [8, 14, 8, 19], [9, 14, 9, 19]];
/// As ocorrências de `hello` com palavra inteira.
const HELLO_WHOLE_WORD: [R; 4] = [[6, 14, 6, 19], [6, 27, 6, 32], [7, 14, 7, 19], [8, 14, 8, 19]];
/// As linhas do texto de `findTest`.
const TEXT_ARR: [&str; 12] = [
    "// my cool header",
    "#include \"cool.h\"",
    "#include <iostream>",
    "",
    "int main() {",
    "    cout << \"hello world, Hello!\" << endl;",
    "    cout << \"hello world again\" << endl;",
    "    cout << \"Hello world again\" << endl;",
    "    cout << \"helloworld again\" << endl;",
    "}",
    "// blablablaciao",
    "",
];

fn from_range(rng: &Range) -> R {
    [rng.start_line_number, rng.start_column, rng.end_line_number, rng.end_column]
}

/// `findTest(testName, callback)`.
fn find_test(callback: impl Fn(&Rc<TestCodeEditor>, &Rc<EditorViewModel>, &DisposableStore)) {
    core_navigation_commands::instance();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let run = |editor: &Rc<TestCodeEditor>, view_model: &Rc<EditorViewModel>| {
        let disposables = DisposableStore::new();
        callback(editor, view_model, &disposables);
        disposables.dispose();
    };
    with_test_code_editor(&TEXT_ARR[..], TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| run(editor, view_model));

    let text = TEXT_ARR.join("\n");
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut pt_builder = PieceTreeTextBufferBuilder::new();
    pt_builder.accept_chunk(&units[..94]);
    pt_builder.accept_chunk(&units[94..195]);
    pt_builder.accept_chunk(&units[195..]);
    let factory = pt_builder.finish(true);
    with_test_code_editor(factory, TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| run(editor, view_model));
}

/// `_getFindState(editor)`.
fn get_find_state(editor: &TestCodeEditor) -> (Vec<R>, Vec<R>) {
    let model = editor.get_model().unwrap();
    let mut current_find_matches: Vec<Range> = Vec::new();
    let mut all_find_matches: Vec<Range> = Vec::new();

    for dec in model.get_all_decorations(0, false, false) {
        if dec.options.class_name.as_deref() == Some("currentFindMatch") {
            current_find_matches.push(dec.range);
            all_find_matches.push(dec.range);
        } else if dec.options.class_name.as_deref() == Some("findMatch") {
            all_find_matches.push(dec.range);
        }
    }

    let by_starts = |a: &Range, b: &Range| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0);
    current_find_matches.sort_by(by_starts);
    all_find_matches.sort_by(by_starts);

    (current_find_matches.iter().map(from_range).collect(), all_find_matches.iter().map(from_range).collect())
}

/// `assertFindState(editor, cursor, highlighted, findDecorations)`.
fn assert_find_state(editor: &TestCodeEditor, cursor: R, highlighted: Option<R>, find_decorations: &[R]) {
    assert_eq!(from_range(&editor.get_selection().unwrap()), cursor, "cursor");

    let expected_highlighted: Vec<R> = highlighted.into_iter().collect();
    assert_eq!(get_find_state(editor), (expected_highlighted, find_decorations.to_vec()), "state");
}

/// `editor.trigger('mouse', CoreNavigationCommands.MoveTo.id, { position: new Position(line, column) })`.
fn move_to(view_model: &EditorViewModel, line_number: i32, column: i32) {
    let options = MoveCommandOptions {
        source: Some("mouse".to_owned()),
        position: Some(Position::new(line_number, column)),
        ..MoveCommandOptions::default()
    };
    core_navigation_commands::instance().move_to.run_core_editor_command(view_model, &options);
}

fn text(value: &str) -> Option<JsString> {
    Some(JsString::from(value))
}

/// `new FindReplaceState()` registrado nos `disposables`.
fn new_find_state(disposables: &DisposableStore) -> Rc<FindReplaceState> {
    let find_state: Rc<FindReplaceState> = FindReplaceState::new();
    disposables.add_rc(find_state.clone());
    find_state
}

/// `new FindModelBoundToEditorModel(editor, findState)` registrado nos `disposables`.
fn new_find_model(editor: &TestCodeEditor, find_state: &Rc<FindReplaceState>, disposables: &DisposableStore) -> Rc<FindModelBoundToEditorModel> {
    let find_model = FindModelBoundToEditorModel::new(editor.widget(), find_state.clone());
    disposables.add_rc(find_model.clone());
    find_model
}

fn assert_line(editor: &TestCodeEditor, line_number: i32, expected: &str) {
    assert_eq!(editor.get_model().unwrap().get_line_content(line_number), expected);
}

#[test]
fn incremental_find_from_beginning_of_file() {
    find_test(|editor, _, disposables| {
        editor.set_position(Position::new(1, 1), None);
        let find_state = new_find_state(disposables);
        let find_model = new_find_model(editor, &find_state, disposables);

        // simulate typing the search string
        find_state.change(INewFindReplaceState { search_string: text("H"), ..Default::default() }, true, None);
        assert_find_state(
            editor,
            [1, 12, 1, 13],
            Some([1, 12, 1, 13]),
            &[[1, 12, 1, 13], [2, 16, 2, 17], [6, 14, 6, 15], [6, 27, 6, 28], [7, 14, 7, 15], [8, 14, 8, 15], [9, 14, 9, 15]],
        );

        // simulate typing the search string
        find_state.change(INewFindReplaceState { search_string: text("He"), ..Default::default() }, true, None);
        assert_find_state(
            editor,
            [1, 12, 1, 14],
            Some([1, 12, 1, 14]),
            &[[1, 12, 1, 14], [6, 14, 6, 16], [6, 27, 6, 29], [7, 14, 7, 16], [8, 14, 8, 16], [9, 14, 9, 16]],
        );

        // simulate typing the search string
        find_state.change(INewFindReplaceState { search_string: text("Hello"), ..Default::default() }, true, None);
        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &HELLO_ALL);

        // simulate toggling on `matchCase`
        find_state.change(INewFindReplaceState { match_case: Some(true), ..Default::default() }, true, None);
        assert_find_state(editor, [6, 27, 6, 32], Some([6, 27, 6, 32]), &[[6, 27, 6, 32], [8, 14, 8, 19]]);

        // simulate typing the search string
        find_state.change(INewFindReplaceState { search_string: text("hello"), ..Default::default() }, true, None);
        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &[[6, 14, 6, 19], [7, 14, 7, 19], [9, 14, 9, 19]]);

        // simulate toggling on `wholeWord`
        find_state.change(INewFindReplaceState { whole_word: Some(true), ..Default::default() }, true, None);
        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &[[6, 14, 6, 19], [7, 14, 7, 19]]);

        // simulate toggling off `matchCase`
        find_state.change(INewFindReplaceState { match_case: Some(false), ..Default::default() }, true, None);
        assert_find_state(
            editor,
            [6, 14, 6, 19],
            Some([6, 14, 6, 19]),
            &[[6, 14, 6, 19], [6, 27, 6, 32], [7, 14, 7, 19], [8, 14, 8, 19]],
        );

        // simulate toggling off `wholeWord`
        find_state.change(INewFindReplaceState { whole_word: Some(false), ..Default::default() }, true, None);
        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &HELLO_ALL);

        // simulate adding a search scope
        find_state.change(
            INewFindReplaceState { search_scope: Some(Some(vec![Range::new(8, 1, 10, 1)])), ..Default::default() },
            true,
            None,
        );
        assert_find_state(editor, [8, 14, 8, 19], Some([8, 14, 8, 19]), &[[8, 14, 8, 19], [9, 14, 9, 19]]);

        // simulate removing the search scope
        find_state.change(INewFindReplaceState { search_scope: Some(None), ..Default::default() }, true, None);
        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &HELLO_ALL);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn find_model_removes_its_decorations() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_eq!(find_state.matches_count(), 5);
        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_ALL);

        find_model.dispose();
        find_state.dispose();

        assert_find_state(editor, [1, 1, 1, 1], None, &[]);
    });
}

#[test]
fn find_model_updates_state_matches_count() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_eq!(find_state.matches_count(), 5);
        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_ALL);

        find_state.change(INewFindReplaceState { search_string: text("helloo"), ..Default::default() }, false, None);
        assert_eq!(find_state.matches_count(), 0);
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn find_model_reacts_to_position_change() {
    find_test(|editor, view_model, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_ALL);

        move_to(view_model, 6, 20);

        assert_find_state(editor, [6, 20, 6, 20], None, &HELLO_ALL);

        find_state.change(INewFindReplaceState { search_string: text("Hello"), ..Default::default() }, true, None);
        assert_find_state(editor, [6, 27, 6, 32], Some([6, 27, 6, 32]), &HELLO_ALL);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn find_model_next() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), whole_word: Some(true), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        for expected in [[6, 14, 6, 19], [6, 27, 6, 32], [7, 14, 7, 19], [8, 14, 8, 19], [6, 14, 6, 19]] {
            find_model.move_to_next_match();
            assert_find_state(editor, expected, Some(expected), &HELLO_WHOLE_WORD);
        }

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn find_model_next_stays_in_scope() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState {
                search_string: text("hello"),
                whole_word: Some(true),
                search_scope: Some(Some(vec![Range::new(7, 1, 9, 1)])),
                ..Default::default()
            },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[7, 14, 7, 19], [8, 14, 8, 19]]);

        for expected in [[7, 14, 7, 19], [8, 14, 8, 19], [7, 14, 7, 19]] {
            find_model.move_to_next_match();
            assert_find_state(editor, expected, Some(expected), &[[7, 14, 7, 19], [8, 14, 8, 19]]);
        }

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn multi_selection_find_model_next_stays_in_scope_overlap() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState {
                search_string: text("hello"),
                whole_word: Some(true),
                search_scope: Some(Some(vec![Range::new(7, 1, 8, 2), Range::new(8, 1, 9, 1)])),
                ..Default::default()
            },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[7, 14, 7, 19], [8, 14, 8, 19]]);

        for expected in [[7, 14, 7, 19], [8, 14, 8, 19], [7, 14, 7, 19]] {
            find_model.move_to_next_match();
            assert_find_state(editor, expected, Some(expected), &[[7, 14, 7, 19], [8, 14, 8, 19]]);
        }

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn multi_selection_find_model_next_stays_in_scope() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState {
                search_string: text("hello"),
                match_case: Some(true),
                whole_word: Some(false),
                search_scope: Some(Some(vec![Range::new(6, 1, 7, 38), Range::new(9, 3, 9, 38)])),
                ..Default::default()
            },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        // `matchCase: false` would find the match [6, 27, 6, 32] as well, and `wholeWord: true` would exclude [9, 14, 9, 19]
        let all: [R; 3] = [[6, 14, 6, 19], [7, 14, 7, 19], [9, 14, 9, 19]];
        assert_find_state(editor, [1, 1, 1, 1], None, &all);

        for expected in [[6, 14, 6, 19], [7, 14, 7, 19], [9, 14, 9, 19], [6, 14, 6, 19]] {
            find_model.move_to_next_match();
            assert_find_state(editor, expected, Some(expected), &all);
        }

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn find_model_prev() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), whole_word: Some(true), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        for expected in [[8, 14, 8, 19], [7, 14, 7, 19], [6, 27, 6, 32], [6, 14, 6, 19], [8, 14, 8, 19]] {
            find_model.move_to_prev_match();
            assert_find_state(editor, expected, Some(expected), &HELLO_WHOLE_WORD);
        }

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn find_model_prev_stays_in_scope() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState {
                search_string: text("hello"),
                whole_word: Some(true),
                search_scope: Some(Some(vec![Range::new(7, 1, 9, 1)])),
                ..Default::default()
            },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[7, 14, 7, 19], [8, 14, 8, 19]]);

        for expected in [[8, 14, 8, 19], [7, 14, 7, 19], [8, 14, 8, 19]] {
            find_model.move_to_prev_match();
            assert_find_state(editor, expected, Some(expected), &[[7, 14, 7, 19], [8, 14, 8, 19]]);
        }

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn find_model_next_prev_with_no_matches() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("helloo"), whole_word: Some(true), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.move_to_next_match();
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.move_to_prev_match();
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn find_model_next_prev_respects_cursor_position() {
    find_test(|editor, view_model, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), whole_word: Some(true), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        move_to(view_model, 6, 20);
        assert_find_state(editor, [6, 20, 6, 20], None, &HELLO_WHOLE_WORD);

        find_model.move_to_next_match();
        assert_find_state(editor, [6, 27, 6, 32], Some([6, 27, 6, 32]), &HELLO_WHOLE_WORD);

        find_model.dispose();
        find_state.dispose();
    });
}

/// A `findTest` de uma busca por regex em todo o texto, que só anda para a próxima (`moveToNextMatch`) ou para a
/// anterior (`moveToPrevMatch`) e confere o cursor, a ocorrência atual e todas as ocorrências.
fn run_regex_navigation(search_string: &str, all: &[R], steps: &[(bool, R)]) {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text(search_string), is_regex: Some(true), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, all);

        for (next, expected) in steps {
            if *next {
                find_model.move_to_next_match();
            } else {
                find_model.move_to_prev_match();
            }
            assert_find_state(editor, *expected, Some(*expected), all);
        }

        find_model.dispose();
        find_state.dispose();
    });
}

const CARET_ALL: [R; 12] = [
    [1, 1, 1, 1],
    [2, 1, 2, 1],
    [3, 1, 3, 1],
    [4, 1, 4, 1],
    [5, 1, 5, 1],
    [6, 1, 6, 1],
    [7, 1, 7, 1],
    [8, 1, 8, 1],
    [9, 1, 9, 1],
    [10, 1, 10, 1],
    [11, 1, 11, 1],
    [12, 1, 12, 1],
];

const DOLLAR_ALL: [R; 12] = [
    [1, 18, 1, 18],
    [2, 18, 2, 18],
    [3, 20, 3, 20],
    [4, 1, 4, 1],
    [5, 13, 5, 13],
    [6, 43, 6, 43],
    [7, 41, 7, 41],
    [8, 41, 8, 41],
    [9, 40, 9, 40],
    [10, 2, 10, 2],
    [11, 17, 11, 17],
    [12, 1, 12, 1],
];

const DOT_STAR_ALL: [R; 12] = [
    [1, 1, 1, 18],
    [2, 1, 2, 18],
    [3, 1, 3, 20],
    [4, 1, 4, 1],
    [5, 1, 5, 13],
    [6, 1, 6, 43],
    [7, 1, 7, 41],
    [8, 1, 8, 41],
    [9, 1, 9, 40],
    [10, 1, 10, 2],
    [11, 1, 11, 17],
    [12, 1, 12, 1],
];

#[test]
fn find_caret() {
    run_regex_navigation("^", &CARET_ALL, &[(true, [2, 1, 2, 1]), (true, [3, 1, 3, 1])]);
}

#[test]
fn find_dollar() {
    run_regex_navigation("$", &DOLLAR_ALL, &[(true, [1, 18, 1, 18]), (true, [2, 18, 2, 18]), (true, [3, 20, 3, 20])]);
}

#[test]
fn find_next_caret_dollar() {
    run_regex_navigation("^$", &[[4, 1, 4, 1], [12, 1, 12, 1]], &[(true, [4, 1, 4, 1]), (true, [12, 1, 12, 1]), (true, [4, 1, 4, 1])]);
}

#[test]
fn find_dot_star() {
    run_regex_navigation(".*", &DOT_STAR_ALL, &[]);
}

#[test]
fn find_next_caret_dot_star_dollar() {
    run_regex_navigation("^.*$", &DOT_STAR_ALL, &[(true, [1, 1, 1, 18]), (true, [2, 1, 2, 18])]);
}

#[test]
fn find_prev_caret_dot_star_dollar() {
    run_regex_navigation("^.*$", &DOT_STAR_ALL, &[(false, [12, 1, 12, 1]), (false, [11, 1, 11, 17])]);
}

#[test]
fn find_prev_caret_dollar() {
    run_regex_navigation(
        "^$",
        &[[4, 1, 4, 1], [12, 1, 12, 1]],
        &[(false, [12, 1, 12, 1]), (false, [4, 1, 4, 1]), (false, [12, 1, 12, 1])],
    );
}

#[test]
fn replace_hello() {
    find_test(|editor, view_model, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("hello"), replace_string: text("hi"), whole_word: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        move_to(view_model, 6, 20);
        assert_find_state(editor, [6, 20, 6, 20], None, &HELLO_WHOLE_WORD);
        assert_line(editor, 6, "    cout << \"hello world, Hello!\" << endl;");

        find_model.replace();
        assert_find_state(editor, [6, 27, 6, 32], Some([6, 27, 6, 32]), &HELLO_WHOLE_WORD);
        assert_line(editor, 6, "    cout << \"hello world, Hello!\" << endl;");

        find_model.replace();
        assert_find_state(editor, [7, 14, 7, 19], Some([7, 14, 7, 19]), &[[6, 14, 6, 19], [7, 14, 7, 19], [8, 14, 8, 19]]);
        assert_line(editor, 6, "    cout << \"hello world, hi!\" << endl;");

        find_model.replace();
        assert_find_state(editor, [8, 14, 8, 19], Some([8, 14, 8, 19]), &[[6, 14, 6, 19], [8, 14, 8, 19]]);
        assert_line(editor, 7, "    cout << \"hi world again\" << endl;");

        find_model.replace();
        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &[[6, 14, 6, 19]]);
        assert_line(editor, 8, "    cout << \"hi world again\" << endl;");

        find_model.replace();
        assert_find_state(editor, [6, 16, 6, 16], None, &[]);
        assert_line(editor, 6, "    cout << \"hi world, hi!\" << endl;");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_bla() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("bla"), replace_string: text("ciao"), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[11, 4, 11, 7], [11, 7, 11, 10], [11, 10, 11, 13]]);

        find_model.replace();
        assert_find_state(editor, [11, 4, 11, 7], Some([11, 4, 11, 7]), &[[11, 4, 11, 7], [11, 7, 11, 10], [11, 10, 11, 13]]);
        assert_line(editor, 11, "// blablablaciao");

        find_model.replace();
        assert_find_state(editor, [11, 8, 11, 11], Some([11, 8, 11, 11]), &[[11, 8, 11, 11], [11, 11, 11, 14]]);
        assert_line(editor, 11, "// ciaoblablaciao");

        find_model.replace();
        assert_find_state(editor, [11, 12, 11, 15], Some([11, 12, 11, 15]), &[[11, 12, 11, 15]]);
        assert_line(editor, 11, "// ciaociaoblaciao");

        find_model.replace();
        assert_find_state(editor, [11, 16, 11, 16], None, &[]);
        assert_line(editor, 11, "// ciaociaociaociao");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_all_hello() {
    find_test(|editor, view_model, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("hello"), replace_string: text("hi"), whole_word: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        move_to(view_model, 6, 20);
        assert_find_state(editor, [6, 20, 6, 20], None, &HELLO_WHOLE_WORD);
        assert_line(editor, 6, "    cout << \"hello world, Hello!\" << endl;");

        find_model.replace_all();
        assert_find_state(editor, [6, 17, 6, 17], None, &[]);
        assert_line(editor, 6, "    cout << \"hi world, hi!\" << endl;");
        assert_line(editor, 7, "    cout << \"hi world again\" << endl;");
        assert_line(editor, 8, "    cout << \"hi world again\" << endl;");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_all_two_spaces_with_one_space() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("  "), replace_string: text(" "), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(
            editor,
            [1, 1, 1, 1],
            None,
            &[[6, 1, 6, 3], [6, 3, 6, 5], [7, 1, 7, 3], [7, 3, 7, 5], [8, 1, 8, 3], [8, 3, 8, 5], [9, 1, 9, 3], [9, 3, 9, 5]],
        );

        find_model.replace_all();
        assert_find_state(editor, [1, 1, 1, 1], None, &[[6, 1, 6, 3], [7, 1, 7, 3], [8, 1, 8, 3], [9, 1, 9, 3]]);
        assert_line(editor, 6, "  cout << \"hello world, Hello!\" << endl;");
        assert_line(editor, 7, "  cout << \"hello world again\" << endl;");
        assert_line(editor, 8, "  cout << \"Hello world again\" << endl;");
        assert_line(editor, 9, "  cout << \"helloworld again\" << endl;");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_all_bla() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("bla"), replace_string: text("ciao"), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[11, 4, 11, 7], [11, 7, 11, 10], [11, 10, 11, 13]]);

        find_model.replace_all();
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);
        assert_line(editor, 11, "// ciaociaociaociao");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_all_bla_with_tab_and_newline() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("bla"), replace_string: text("<\\n\\t>"), is_regex: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[11, 4, 11, 7], [11, 7, 11, 10], [11, 10, 11, 13]]);

        find_model.replace_all();
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);
        assert_line(editor, 11, "// <");
        assert_line(editor, 12, "\t><");
        assert_line(editor, 13, "\t><");
        assert_line(editor, 14, "\t>ciao");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn issue_3516_replace_all_moves_page_cursor_focus_scroll_to_the_place_of_the_last_replacement() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("include"), replace_string: text("bar"), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[2, 2, 2, 9], [3, 2, 3, 9]]);

        find_model.replace_all();
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        assert_line(editor, 2, "#bar \"cool.h\"");
        assert_line(editor, 3, "#bar <iostream>");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn listens_to_model_content_changes() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("hello"), replace_string: text("hi"), whole_word: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        editor.get_model().unwrap().set_value("hello\nhi".into());
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn select_all_matches() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("hello"), replace_string: text("hi"), whole_word: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        find_model.select_all_matches();

        assert_eq!(
            editor.get_selections().unwrap(),
            vec![Selection::new(6, 14, 6, 19), Selection::new(6, 27, 6, 32), Selection::new(7, 14, 7, 19), Selection::new(8, 14, 8, 19)]
        );

        assert_find_state(editor, [6, 14, 6, 19], None, &HELLO_WHOLE_WORD);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn issue_14143_select_all_matches_should_maintain_primary_cursor_if_feasible() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("hello"), replace_string: text("hi"), whole_word: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        editor.set_selection(Range::new(7, 14, 7, 19).into(), None);

        find_model.select_all_matches();

        assert_eq!(
            editor.get_selections().unwrap(),
            vec![Selection::new(7, 14, 7, 19), Selection::new(6, 14, 6, 19), Selection::new(6, 27, 6, 32), Selection::new(8, 14, 8, 19)]
        );

        assert_eq!(editor.get_selection().unwrap(), Selection::new(7, 14, 7, 19));

        assert_find_state(editor, [7, 14, 7, 19], None, &HELLO_WHOLE_WORD);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn issue_1914_npe_when_there_is_only_one_find_match() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("cool.h"), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[2, 11, 2, 17]]);

        find_model.move_to_next_match();
        assert_find_state(editor, [2, 11, 2, 17], Some([2, 11, 2, 17]), &[[2, 11, 2, 17]]);

        find_model.move_to_next_match();
        assert_find_state(editor, [2, 11, 2, 17], Some([2, 11, 2, 17]), &[[2, 11, 2, 17]]);

        find_model.dispose();
        find_state.dispose();
    });
}

/// A `findTest` das buscas `hello(?=\sworld)` com `replace` e com `isRegex`.
fn look_ahead_state() -> INewFindReplaceState {
    INewFindReplaceState { search_string: text("hello(?=\\sworld)"), replace_string: text("hi"), is_regex: Some(true), ..Default::default() }
}

#[test]
fn replace_when_search_string_has_look_ahed_regex() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(look_ahead_state(), false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[6, 14, 6, 19], [7, 14, 7, 19], [8, 14, 8, 19]]);

        find_model.replace();

        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &[[6, 14, 6, 19], [7, 14, 7, 19], [8, 14, 8, 19]]);
        assert_line(editor, 6, "    cout << \"hello world, Hello!\" << endl;");

        find_model.replace();
        assert_find_state(editor, [7, 14, 7, 19], Some([7, 14, 7, 19]), &[[7, 14, 7, 19], [8, 14, 8, 19]]);
        assert_line(editor, 6, "    cout << \"hi world, Hello!\" << endl;");

        find_model.replace();
        assert_find_state(editor, [8, 14, 8, 19], Some([8, 14, 8, 19]), &[[8, 14, 8, 19]]);
        assert_line(editor, 7, "    cout << \"hi world again\" << endl;");

        find_model.replace();
        assert_find_state(editor, [8, 16, 8, 16], None, &[]);
        assert_line(editor, 8, "    cout << \"hi world again\" << endl;");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_when_search_string_has_look_ahed_regex_and_cursor_is_at_the_last_find_match() {
    find_test(|editor, view_model, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(look_ahead_state(), false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        move_to(view_model, 8, 14);

        assert_find_state(editor, [8, 14, 8, 14], None, &[[6, 14, 6, 19], [7, 14, 7, 19], [8, 14, 8, 19]]);

        find_model.replace();

        assert_find_state(editor, [8, 14, 8, 19], Some([8, 14, 8, 19]), &[[6, 14, 6, 19], [7, 14, 7, 19], [8, 14, 8, 19]]);

        assert_line(editor, 8, "    cout << \"Hello world again\" << endl;");

        find_model.replace();
        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &[[6, 14, 6, 19], [7, 14, 7, 19]]);
        assert_line(editor, 8, "    cout << \"hi world again\" << endl;");

        find_model.replace();
        assert_find_state(editor, [7, 14, 7, 19], Some([7, 14, 7, 19]), &[[7, 14, 7, 19]]);
        assert_line(editor, 6, "    cout << \"hi world, Hello!\" << endl;");

        find_model.replace();
        assert_find_state(editor, [7, 16, 7, 16], None, &[]);
        assert_line(editor, 7, "    cout << \"hi world again\" << endl;");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_all_when_search_string_has_look_ahed_regex() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(look_ahead_state(), false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[6, 14, 6, 19], [7, 14, 7, 19], [8, 14, 8, 19]]);

        find_model.replace_all();

        assert_line(editor, 6, "    cout << \"hi world, Hello!\" << endl;");
        assert_line(editor, 7, "    cout << \"hi world again\" << endl;");
        assert_line(editor, 8, "    cout << \"hi world again\" << endl;");

        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_when_search_string_has_look_ahed_regex_and_replace_string_has_capturing_groups() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("hel(lo)(?=\\sworld)"), replace_string: text("hi$1"), is_regex: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[6, 14, 6, 19], [7, 14, 7, 19], [8, 14, 8, 19]]);

        find_model.replace();

        assert_find_state(editor, [6, 14, 6, 19], Some([6, 14, 6, 19]), &[[6, 14, 6, 19], [7, 14, 7, 19], [8, 14, 8, 19]]);
        assert_line(editor, 6, "    cout << \"hello world, Hello!\" << endl;");

        find_model.replace();
        assert_find_state(editor, [7, 14, 7, 19], Some([7, 14, 7, 19]), &[[7, 14, 7, 19], [8, 14, 8, 19]]);
        assert_line(editor, 6, "    cout << \"hilo world, Hello!\" << endl;");

        find_model.replace();
        assert_find_state(editor, [8, 14, 8, 19], Some([8, 14, 8, 19]), &[[8, 14, 8, 19]]);
        assert_line(editor, 7, "    cout << \"hilo world again\" << endl;");

        find_model.replace();
        assert_find_state(editor, [8, 18, 8, 18], None, &[]);
        assert_line(editor, 8, "    cout << \"hilo world again\" << endl;");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_all_when_search_string_has_look_ahed_regex_and_replace_string_has_capturing_groups() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("wo(rl)d(?=.*;$)"), replace_string: text("gi$1"), is_regex: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[6, 20, 6, 25], [7, 20, 7, 25], [8, 20, 8, 25], [9, 19, 9, 24]]);

        find_model.replace_all();

        assert_line(editor, 6, "    cout << \"hello girl, Hello!\" << endl;");
        assert_line(editor, 7, "    cout << \"hello girl again\" << endl;");
        assert_line(editor, 8, "    cout << \"Hello girl again\" << endl;");
        assert_line(editor, 9, "    cout << \"hellogirl again\" << endl;");

        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_all_when_search_string_is_multiline_and_has_look_ahed_regex_and_replace_string_has_capturing_groups() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState {
                search_string: text("wo(rl)d(.*;\\n)(?=.*hello)"),
                replace_string: text("gi$1$2"),
                is_regex: Some(true),
                match_case: Some(true),
                ..Default::default()
            },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[6, 20, 7, 1], [8, 20, 9, 1]]);

        find_model.replace_all();

        assert_line(editor, 6, "    cout << \"hello girl, Hello!\" << endl;");
        assert_line(editor, 8, "    cout << \"Hello girl again\" << endl;");

        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn replace_all_preserving_case() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState {
                search_string: text("hello"),
                replace_string: text("goodbye"),
                is_regex: Some(false),
                match_case: Some(false),
                preserve_case: Some(true),
                ..Default::default()
            },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_ALL);

        find_model.replace_all();

        assert_line(editor, 6, "    cout << \"goodbye world, Goodbye!\" << endl;");
        assert_line(editor, 7, "    cout << \"goodbye world again\" << endl;");
        assert_line(editor, 8, "    cout << \"Goodbye world again\" << endl;");
        assert_line(editor, 9, "    cout << \"goodbyeworld again\" << endl;");

        assert_find_state(editor, [1, 1, 1, 1], None, &[]);

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn issue_18711_replace_all_with_empty_string() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState { search_string: text("hello"), replace_string: text(""), whole_word: Some(true), ..Default::default() },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &HELLO_WHOLE_WORD);

        find_model.replace_all();
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);
        assert_line(editor, 6, "    cout << \" world, !\" << endl;");
        assert_line(editor, 7, "    cout << \" world again\" << endl;");
        assert_line(editor, 8, "    cout << \" world again\" << endl;");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn issue_32522_replace_all_with_caret_on_more_than_1000_matches() {
    find_test(|editor, _, disposables| {
        let mut initial_text = String::new();
        for i in 0..1100 {
            initial_text.push_str(&format!("line{i}\n"));
        }
        editor.get_model().unwrap().set_value(initial_text.into());
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("^"), replace_string: text("a "), is_regex: Some(true), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        find_model.replace_all();

        let mut expected_text = String::new();
        for i in 0..1100 {
            expected_text.push_str(&format!("a line{i}\n"));
        }
        expected_text.push_str("a ");
        assert_eq!(editor.get_model().unwrap().get_value(), expected_text.as_str());

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn issue_19740_find_and_replace_capture_group_backreference_inserts_undefined_instead_of_empty_string() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState {
                search_string: text("hello(z)?"),
                replace_string: text("hi$1"),
                is_regex: Some(true),
                match_case: Some(true),
                ..Default::default()
            },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[6, 14, 6, 19], [7, 14, 7, 19], [9, 14, 9, 19]]);

        find_model.replace_all();
        assert_find_state(editor, [1, 1, 1, 1], None, &[]);
        assert_line(editor, 6, "    cout << \"hi world, Hello!\" << endl;");
        assert_line(editor, 7, "    cout << \"hi world again\" << endl;");
        assert_line(editor, 9, "    cout << \"hiworld again\" << endl;");

        find_model.dispose();
        find_state.dispose();
    });
}

#[test]
fn issue_27083_search_scope_works_even_if_it_is_a_single_line() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(
            INewFindReplaceState {
                search_string: text("hello"),
                whole_word: Some(true),
                search_scope: Some(Some(vec![Range::new(7, 1, 8, 1)])),
                ..Default::default()
            },
            false,
            None,
        );
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_find_state(editor, [1, 1, 1, 1], None, &[[7, 14, 7, 19]]);

        find_model.dispose();
        find_state.dispose();
    });
}

/// `assert.strictEqual(findState.matchesPosition, ...)` e as duas `canNavigate*`.
fn assert_navigation(find_state: &FindReplaceState, matches_position: usize, can_navigate_forward: bool, can_navigate_back: bool) {
    assert_eq!(find_state.matches_position(), matches_position);
    assert_eq!(find_state.can_navigate_forward(), can_navigate_forward);
    assert_eq!(find_state.can_navigate_back(), can_navigate_back);
}

#[test]
fn issue_3516_control_behavior_of_next_operations_not_looping_back_to_beginning() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), r#loop: Some(false), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_eq!(find_state.matches_count(), 5);

        // Test next operations
        assert_navigation(&find_state, 0, true, true);

        find_model.move_to_next_match();
        assert_navigation(&find_state, 1, true, false);

        find_model.move_to_next_match();
        assert_navigation(&find_state, 2, true, true);

        find_model.move_to_next_match();
        assert_navigation(&find_state, 3, true, true);

        find_model.move_to_next_match();
        assert_navigation(&find_state, 4, true, true);

        find_model.move_to_next_match();
        assert_navigation(&find_state, 5, false, true);

        find_model.move_to_next_match();
        assert_navigation(&find_state, 5, false, true);

        find_model.move_to_next_match();
        assert_navigation(&find_state, 5, false, true);

        // Test previous operations
        find_model.move_to_prev_match();
        assert_navigation(&find_state, 4, true, true);

        find_model.move_to_prev_match();
        assert_navigation(&find_state, 3, true, true);

        find_model.move_to_prev_match();
        assert_navigation(&find_state, 2, true, true);

        find_model.move_to_prev_match();
        assert_navigation(&find_state, 1, true, false);

        find_model.move_to_prev_match();
        assert_navigation(&find_state, 1, true, false);

        find_model.move_to_prev_match();
        assert_navigation(&find_state, 1, true, false);
    });
}

#[test]
fn issue_3516_control_behavior_of_next_operations_looping_back_to_beginning() {
    find_test(|editor, _, disposables| {
        let find_state = new_find_state(disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), ..Default::default() }, false, None);
        let find_model = new_find_model(editor, &find_state, disposables);

        assert_eq!(find_state.matches_count(), 5);

        // Test next operations
        assert_navigation(&find_state, 0, true, true);

        for expected_position in [1, 2, 3, 4, 5, 1, 2] {
            find_model.move_to_next_match();
            assert_navigation(&find_state, expected_position, true, true);
        }

        // Test previous operations
        for expected_position in [1, 5, 4, 3, 2, 1] {
            find_model.move_to_prev_match();
            assert_navigation(&find_state, expected_position, true, true);
        }
    });
}

#[test]
fn issue_288515_wrong_current_index_in_find_widget_if_matches_more_than_1000() {
    core_navigation_commands::instance();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Create 1001 lines of 'hello'
    let text_arr: Vec<String> = vec!["hello".to_owned(); 1001];
    with_test_code_editor(text_arr, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
        let disposables = DisposableStore::new();

        // Place cursor at line 900, selecting 'hello'
        editor.set_selection(Selection::new(900, 1, 900, 6).into(), None);

        let find_state = new_find_state(&disposables);
        find_state.change(INewFindReplaceState { search_string: text("hello"), ..Default::default() }, false, None);
        new_find_model(editor, &find_state, &disposables);

        assert_eq!(find_state.matches_count(), 1001);
        // With cursor selecting 'hello' at line 900, matchesPosition should be 900
        assert_eq!(find_state.matches_position(), 900);
        disposables.dispose();
    });
}
