//! Porte de `src/vs/editor/contrib/snippet/test/browser/snippetSession.test.ts` (suíte `SnippetSession` e a
//! subsuíte `createEditsAndSnippetsFromEdits`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` e `ensureNoDisposablesAreLeakedInTestSuite()` são o `with_fixture`: cria o `LeakGuard`, o
//!   modelo, o editor de teste, os serviços e as seleções iniciais, roda o caso e descarta o modelo e o editor (nesta
//!   ordem, como o `teardown`).
//! - O `ServiceCollection` com `ILabelService`, `ILanguageConfigurationService` e `IWorkspaceContextService`
//!   é o `SnippetServices` (o `invokeWithinContext` do original). `mock<ILabelService>()` é o
//!   `ClosureLabelService::without_methods` e o `IWorkspaceContextService` que só define `getWorkspace()` é o
//!   `WorkspaceOnlyService` (em `tests/snippet_doubles`).
//! - `editor.trigger('test', 'type', { text })` é o `type_text`; `editor.trigger('test', 'cut', {})` é o `trigger`
//!   com o handler `cut` e sem carga.
//! - `assertSelections` compara a lista inteira das seleções do editor com a esperada (o original percorre as do editor
//!   consumindo as esperadas e confere que não sobrou nenhuma).
//! - `assert.doesNotThrow(() => ...)` é a chamada direta: um pânico reprova o caso, e um erro inesperado que o modelo
//!   captura em `changeDecorations` reprova no `LeakGuard`.
//! - Os `deepStrictEqual` de `Range` e `string` são `assert_eq!` (o texto de uma edição é `Option<JsString>`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod snippet_doubles;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::code_editor_widget::{Handler, ICodeEditor, TypePayload};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::model::{Eol, IndentSize, TextModelUpdateOptions};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::snippet::snippet_parser::SnippetParser;
use je_editor::snippet::snippet_session::{
    SnippetEdit, SnippetServices, SnippetSession, SnippetSessionInsertOptions,
};
use je_editor::text_model::TextModel;
use je_platform::workspace::Workspace;
use snippet_doubles::{ClosureLabelService, WorkspaceOnlyService};
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, create_test_code_editor};
use support::test_language_configuration_service::TestLanguageConfigurationService;
use support::test_text_model::create_text_model;

fn sel(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Selection {
    Selection::new(start_line_number, start_column, end_line_number, end_column)
}

fn text(value: &str) -> Option<JsString> {
    Some(JsString::from(value))
}

fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// O que o `setup` deixa em variáveis da suíte.
struct Fixture {
    model: Rc<TextModel>,
    editor: Rc<TestCodeEditor>,
    services: Rc<SnippetServices>,
}

impl Fixture {
    /// `new SnippetSession(editor, template, options, languageConfigurationService)`.
    fn session(&self, template: &str, options: Option<SnippetSessionInsertOptions>) -> SnippetSession {
        SnippetSession::new(self.editor.widget().clone(), template, options, self.services.clone())
    }

    /// `editor.trigger('test', 'type', { text })`.
    fn type_text(&self, value: &str) {
        self.editor.trigger(Some("test"), Handler::TYPE, Some(TypePayload { text: JsString::from(value) }.to_dynamic()));
    }

    /// `assertSelections(editor, ...s)`.
    fn assert_selections(&self, expected: &[Selection]) {
        let actual: Vec<String> =
            self.editor.get_selections().expect("o editor tem o modelo").iter().map(ToString::to_string).collect();
        let expected: Vec<String> = expected.iter().map(ToString::to_string).collect();
        assert_eq!(actual, expected);
    }

    fn assert_value(&self, expected: &str) {
        assert_eq!(self.model.get_value(), expected);
    }
}

fn with_fixture(test: impl FnOnce(&Fixture)) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = create_text_model("function foo() {\n    console.log(a);\n}");
    let language_configuration_service = TestLanguageConfigurationService::new();
    let workspace = Workspace::new("workspace-id", Vec::new(), false, None, Rc::new(|_: &je_base::uri::URI| false), None);
    let services = Rc::new(SnippetServices {
        language_configuration_service: language_configuration_service.clone(),
        label_service: ClosureLabelService::without_methods(),
        workspace_service: Some(WorkspaceOnlyService::new(workspace)),
    });
    let editor = create_test_code_editor(Some(model.clone()), TestCodeEditorInstantiationOptions::default());
    editor.set_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)], None, None);
    assert_eq!(model.get_eol(), Eol::Lf);

    let fixture = Fixture { model, editor, services };
    test(&fixture);

    // teardown
    fixture.model.dispose();
    fixture.editor.dispose();
    language_configuration_service.dispose();
}

#[test]
fn normalize_whitespace() {
    with_fixture(|f| {
        let assert_normalized = |position: Position, input: &str, expected: &str| {
            let snippet = SnippetParser::new().parse(&utf16(input), false, false);
            SnippetSession::adjust_whitespace(&f.model, position, true, &snippet, None);
            assert_eq!(snippet.to_textmate_string(), expected);
        };

        assert_normalized(Position::new(1, 1), "foo", "foo");
        assert_normalized(Position::new(1, 1), "foo\rbar", "foo\nbar");
        assert_normalized(Position::new(1, 1), "foo\rbar", "foo\nbar");
        assert_normalized(Position::new(2, 5), "foo\r\tbar", "foo\n        bar");
        assert_normalized(Position::new(2, 3), "foo\r\tbar", "foo\n    bar");
        assert_normalized(Position::new(2, 5), "foo\r\tbar\nfoo", "foo\n        bar\n    foo");

        //Indentation issue with choice elements that span multiple lines #46266
        assert_normalized(Position::new(2, 5), "a\nb${1|foo,\nbar|}", "a\n    b${1|foo,\nbar|}");
    });
}

#[test]
fn adjust_selection_overwrite_before_after() {
    with_fixture(|f| {
        let mut range = SnippetSession::adjust_selection(&f.model, &sel(1, 2, 1, 2), 1, 0);
        assert_eq!(*range, Range::new(1, 1, 1, 2));
        range = SnippetSession::adjust_selection(&f.model, &sel(1, 2, 1, 2), 1111, 0);
        assert_eq!(*range, Range::new(1, 1, 1, 2));
        range = SnippetSession::adjust_selection(&f.model, &sel(1, 2, 1, 2), 0, 10);
        assert_eq!(*range, Range::new(1, 2, 1, 12));
        range = SnippetSession::adjust_selection(&f.model, &sel(1, 2, 1, 2), 0, 10111);
        assert_eq!(*range, Range::new(1, 2, 1, 17));
    });
}

#[test]
fn text_edits_and_selection() {
    with_fixture(|f| {
        let session = f.session("foo${1:bar}foo$0", None);
        session.insert(None);
        f.assert_value("foobarfoofunction foo() {\n    foobarfooconsole.log(a);\n}");

        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);
        session.next();
        f.assert_selections(&[sel(1, 10, 1, 10), sel(2, 14, 2, 14)]);
    });
}

#[test]
fn text_edit_with_reversed_selection() {
    with_fixture(|f| {
        let session = f.session("${1:bar}$0", None);
        f.editor.set_selections(&[sel(2, 5, 2, 5), sel(1, 1, 1, 1)], None, None);

        session.insert(None);
        f.assert_value("barfunction foo() {\n    barconsole.log(a);\n}");
        f.assert_selections(&[sel(2, 5, 2, 8), sel(1, 1, 1, 4)]);
    });
}

#[test]
fn snippets_repeated_tabstops() {
    with_fixture(|f| {
        let session = f.session("${1:abc}foo${1:abc}$0", None);
        session.insert(None);
        f.assert_selections(&[sel(1, 1, 1, 4), sel(1, 7, 1, 10), sel(2, 5, 2, 8), sel(2, 11, 2, 14)]);
        session.next();
        f.assert_selections(&[sel(1, 10, 1, 10), sel(2, 14, 2, 14)]);
    });
}

#[test]
fn snippets_just_text() {
    with_fixture(|f| {
        let session = f.session("foobar", None);
        session.insert(None);
        f.assert_value("foobarfunction foo() {\n    foobarconsole.log(a);\n}");
        f.assert_selections(&[sel(1, 7, 1, 7), sel(2, 11, 2, 11)]);
    });
}

#[test]
fn snippets_selections_and_new_text_with_newlines() {
    with_fixture(|f| {
        let session = f.session("foo\n\t${1:bar}\n$0", None);
        session.insert(None);

        f.assert_value("foo\n    bar\nfunction foo() {\n    foo\n        bar\n    console.log(a);\n}");

        f.assert_selections(&[sel(2, 5, 2, 8), sel(5, 9, 5, 12)]);

        session.next();
        f.assert_selections(&[sel(3, 1, 3, 1), sel(6, 5, 6, 5)]);
    });
}

#[test]
fn snippets_newline_no_whitespace_adjust() {
    with_fixture(|f| {
        f.editor.set_selection(sel(2, 5, 2, 5).into(), None);
        let session = f.session(
            "abc\n    foo\n        bar\n$0",
            Some(SnippetSessionInsertOptions { adjust_whitespace: false, ..Default::default() }),
        );
        session.insert(None);
        f.assert_value("function foo() {\n    abc\n    foo\n        bar\nconsole.log(a);\n}");
    });
}

#[test]
fn snippets_selections_next_prev() {
    with_fixture(|f| {
        let session = f.session("f$1oo${2:bar}foo$0", None);
        session.insert(None);

        // @ $2
        f.assert_selections(&[sel(1, 2, 1, 2), sel(2, 6, 2, 6)]);
        // @ $1
        session.next();
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);
        // @ $2
        session.prev();
        f.assert_selections(&[sel(1, 2, 1, 2), sel(2, 6, 2, 6)]);
        // @ $1
        session.next();
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);
        // @ $0
        session.next();
        f.assert_selections(&[sel(1, 10, 1, 10), sel(2, 14, 2, 14)]);
    });
}

#[test]
fn snippets_selections_and_typing() {
    with_fixture(|f| {
        let session = f.session("f${1:oo}_$2_$0", None);
        session.insert(None);

        f.type_text("X");
        session.next();
        f.type_text("bar");

        // go back to ${2:oo} which is now just 'X'
        session.prev();
        f.assert_selections(&[sel(1, 2, 1, 3), sel(2, 6, 2, 7)]);

        // go forward to $1 which is now 'bar'
        session.next();
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);

        // go to final tabstop
        session.next();
        f.assert_value("fX_bar_function foo() {\n    fX_bar_console.log(a);\n}");
        f.assert_selections(&[sel(1, 8, 1, 8), sel(2, 12, 2, 12)]);
    });
}

#[test]
fn snippets_insert_shorter_snippet_into_non_empty_selection() {
    with_fixture(|f| {
        f.model.set_value("foo_bar_foo".into());
        f.editor.set_selections(&[sel(1, 1, 1, 4), sel(1, 9, 1, 12)], None, None);

        f.session("x$0", None).insert(None);
        f.assert_value("x_bar_x");
        f.assert_selections(&[sel(1, 2, 1, 2), sel(1, 8, 1, 8)]);
    });
}

#[test]
fn snippets_insert_longer_snippet_into_non_empty_selection() {
    with_fixture(|f| {
        f.model.set_value("foo_bar_foo".into());
        f.editor.set_selections(&[sel(1, 1, 1, 4), sel(1, 9, 1, 12)], None, None);

        f.session("LONGER$0", None).insert(None);
        f.assert_value("LONGER_bar_LONGER");
        f.assert_selections(&[sel(1, 7, 1, 7), sel(1, 18, 1, 18)]);
    });
}

#[test]
fn snippets_dont_grow_final_tabstop() {
    with_fixture(|f| {
        f.model.set_value("foo_zzz_foo".into());
        f.editor.set_selection(sel(1, 5, 1, 8).into(), None);
        let session = f.session("$1bar$0", None);
        session.insert(None);

        f.assert_selections(&[sel(1, 5, 1, 5)]);
        f.type_text("foo-");

        session.next();
        f.assert_value("foo_foo-bar_foo");
        f.assert_selections(&[sel(1, 12, 1, 12)]);

        f.type_text("XXX");
        f.assert_value("foo_foo-barXXX_foo");
        session.prev();
        f.assert_selections(&[sel(1, 5, 1, 9)]);
        session.next();
        f.assert_selections(&[sel(1, 15, 1, 15)]);
    });
}

#[test]
fn snippets_dont_merge_touching_tabstops_1_2() {
    with_fixture(|f| {
        let session = f.session("$1$2$3$0", None);
        session.insert(None);
        f.assert_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)]);

        session.next();
        f.assert_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)]);

        session.next();
        f.assert_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)]);

        session.next();
        f.assert_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)]);

        session.prev();
        session.prev();
        session.prev();
        f.assert_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)]);
        f.type_text("111");

        session.next();
        f.type_text("222");

        session.next();
        f.type_text("333");

        session.next();
        f.assert_value("111222333function foo() {\n    111222333console.log(a);\n}");
        f.assert_selections(&[sel(1, 10, 1, 10), sel(2, 14, 2, 14)]);

        session.prev();
        f.assert_selections(&[sel(1, 7, 1, 10), sel(2, 11, 2, 14)]);
        session.prev();
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);
        session.prev();
        f.assert_selections(&[sel(1, 1, 1, 4), sel(2, 5, 2, 8)]);
    });
}

#[test]
fn snippets_dont_merge_touching_tabstops_2_2() {
    with_fixture(|f| {
        let session = f.session("$1$2$3$0", None);
        session.insert(None);
        f.assert_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)]);

        f.type_text("111");

        session.next();
        f.assert_selections(&[sel(1, 4, 1, 4), sel(2, 8, 2, 8)]);
        f.type_text("222");

        session.next();
        f.assert_selections(&[sel(1, 7, 1, 7), sel(2, 11, 2, 11)]);
        f.type_text("333");

        session.next();
        assert!(session.is_at_last_placeholder());
    });
}

#[test]
fn snippets_gracefully_move_over_final_tabstop() {
    with_fixture(|f| {
        let session = f.session("${1}bar$0", None);
        session.insert(None);

        assert!(!session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)]);

        session.next();
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 4, 1, 4), sel(2, 8, 2, 8)]);

        session.next();
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 4, 1, 4), sel(2, 8, 2, 8)]);
    });
}

#[test]
fn snippets_overwriting_nested_placeholder() {
    with_fixture(|f| {
        let session = f.session("log(${1:\"$2\"});$0", None);
        session.insert(None);
        f.assert_selections(&[sel(1, 5, 1, 7), sel(2, 9, 2, 11)]);

        f.type_text("XXX");
        f.assert_value("log(XXX);function foo() {\n    log(XXX);console.log(a);\n}");

        session.next();
        assert!(!session.is_at_last_placeholder());
        // assertSelections(editor, new Selection(1, 7, 1, 7), new Selection(2, 11, 2, 11));

        session.next();
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 10, 1, 10), sel(2, 14, 2, 14)]);
    });
}

#[test]
fn snippets_selections_and_snippet_ranges() {
    with_fixture(|f| {
        let session = f.session("${1:foo}farboo${2:bar}$0", None);
        session.insert(None);
        f.assert_value("foofarboobarfunction foo() {\n    foofarboobarconsole.log(a);\n}");
        f.assert_selections(&[sel(1, 1, 1, 4), sel(2, 5, 2, 8)]);

        assert!(session.is_selection_within_placeholders());

        f.editor.set_selections(&[sel(1, 1, 1, 1)], None, None);
        assert!(!session.is_selection_within_placeholders());

        f.editor.set_selections(&[sel(1, 6, 1, 6), sel(2, 10, 2, 10)], None, None);
        assert!(!session.is_selection_within_placeholders()); // in snippet, outside placeholder

        f.editor.set_selections(&[sel(1, 6, 1, 6), sel(2, 10, 2, 10), sel(1, 1, 1, 1)], None, None);
        assert!(!session.is_selection_within_placeholders()); // in snippet, outside placeholder

        f.editor.set_selections(&[sel(1, 6, 1, 6), sel(2, 10, 2, 10), sel(2, 20, 2, 21)], None, None);
        assert!(!session.is_selection_within_placeholders());

        // reset selection to placeholder
        session.next();
        assert!(session.is_selection_within_placeholders());
        f.assert_selections(&[sel(1, 10, 1, 13), sel(2, 14, 2, 17)]);

        // reset selection to placeholder
        session.next();
        assert!(session.is_selection_within_placeholders());
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 13, 1, 13), sel(2, 17, 2, 17)]);
    });
}

#[test]
fn snippets_nested_sessions() {
    with_fixture(|f| {
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        let first = f.session("foo${2:bar}foo$0", None);
        first.insert(None);
        f.assert_value("foobarfoo");
        f.assert_selections(&[sel(1, 4, 1, 7)]);

        let second = f.session("ba${1:zzzz}$0", None);
        second.insert(None);
        f.assert_value("foobazzzzfoo");
        f.assert_selections(&[sel(1, 6, 1, 10)]);

        second.next();
        assert!(second.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 10, 1, 10)]);

        first.next();
        assert!(first.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 13, 1, 13)]);
    });
}

#[test]
fn snippets_typing_at_final_tabstop() {
    with_fixture(|f| {
        let session = f.session("farboo$0", None);
        session.insert(None);
        assert!(session.is_at_last_placeholder());
        assert!(!session.is_selection_within_placeholders());

        f.type_text("XXX");
        assert!(!session.is_selection_within_placeholders());
    });
}

#[test]
fn snippets_typing_at_beginning() {
    with_fixture(|f| {
        f.editor.set_selection(sel(1, 2, 1, 2).into(), None);
        let session = f.session("farboo$0", None);
        session.insert(None);

        f.editor.set_selection(sel(1, 2, 1, 2).into(), None);
        assert!(!session.is_selection_within_placeholders());
        assert!(session.is_at_last_placeholder());

        f.type_text("XXX");
        assert_eq!(f.model.get_line_content(1), "fXXXfarboounction foo() {");
        assert!(!session.is_selection_within_placeholders());

        session.next();
        f.assert_selections(&[sel(1, 11, 1, 11)]);
    });
}

#[test]
fn snippets_typing_with_nested_placeholder() {
    with_fixture(|f| {
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("This ${1:is ${2:nested}}.$0", None);
        session.insert(None);
        f.assert_selections(&[sel(1, 6, 1, 15)]);

        session.next();
        f.assert_selections(&[sel(1, 9, 1, 15)]);

        f.editor.trigger(Some("test"), Handler::CUT, None);
        f.assert_selections(&[sel(1, 9, 1, 9)]);

        f.type_text("XXX");
        session.prev();
        f.assert_selections(&[sel(1, 6, 1, 12)]);
    });
}

#[test]
fn snippets_snippet_with_variables() {
    with_fixture(|f| {
        let session = f.session("@line=$TM_LINE_NUMBER$0", None);
        session.insert(None);

        f.assert_value("@line=1function foo() {\n    @line=2console.log(a);\n}");
        f.assert_selections(&[sel(1, 8, 1, 8), sel(2, 12, 2, 12)]);
    });
}

#[test]
fn snippets_merge() {
    with_fixture(|f| {
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("This ${1:is ${2:nested}}.$0", None);
        session.insert(None);
        session.next();
        f.assert_selections(&[sel(1, 9, 1, 15)]);

        session.merge("really ${1:nested}$0", None);
        f.assert_selections(&[sel(1, 16, 1, 22)]);

        session.next();
        f.assert_selections(&[sel(1, 22, 1, 22)]);
        assert!(!session.is_at_last_placeholder());

        session.next();
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 23, 1, 23)]);

        session.prev();
        f.type_text("AAA");

        // back to `really ${1:nested}`
        session.prev();
        f.assert_selections(&[sel(1, 16, 1, 22)]);

        // back to `${1:is ...}` which now grew
        session.prev();
        f.assert_selections(&[sel(1, 6, 1, 25)]);
    });
}

#[test]
fn snippets_next_does_not_throw_when_placeholder_decorations_are_missing() {
    with_fixture(|f| {
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        let session = f.session("${1:foo}$0", None);
        session.insert(None);

        let decoration_ids: Vec<String> =
            f.model.get_all_decorations(0, false, false).into_iter().map(|decoration| decoration.id).collect();
        assert!(!decoration_ids.is_empty());
        f.model.change_decorations(
            |accessor| {
                for decoration_id in &decoration_ids {
                    accessor.remove_decoration(decoration_id);
                }
            },
            0,
        );

        session.next();
    });
}

#[test]
fn snippets_deep_merge_does_not_produce_phantom_cursors() {
    with_fixture(|f| {
        // Recursively expanding a snippet (e.g. \sin\left(${1:x}\right)) used to assign
        // fractional placeholder indicies. After ~16 nestings, double-precision rounding
        // collapsed distinct placeholders onto the same index, producing a phantom cursor.
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("\\sin\\left(${1:x}\\right) ", None);
        session.insert(None);
        for i in 0..25 {
            session.merge("\\sin\\left(${1:x}\\right) ", None);
            assert_eq!(f.editor.get_selections().expect("o editor tem o modelo").len(), 1, "selection count after {} merges", i + 1);
        }
    });
}

#[test]
fn snippets_merge_preserves_mirrors_in_nested_snippet() {
    with_fixture(|f| {
        // Nested snippets that themselves contain mirrors ($1...$1) must still
        // produce multi-cursor selections after renormalization of placeholder
        // indicies, since the renormalize step maps each unique old index to a
        // single new index.
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("(${1:x})", None);
        session.insert(None);
        session.merge("${1:y}-${1}", None);
        f.assert_value("(y-y)");
        assert_eq!(f.editor.get_selections().expect("o editor tem o modelo").len(), 2);
    });
}

#[test]
fn snippets_merge_does_not_throw_when_placeholder_occurrences_collapse_to_same_position() {
    with_fixture(|f| {
        // $1$1 places two zero-width occurrences of the same placeholder at the same position;
        // the editor's cursor normalization collapses these into one selection. Previously this
        // caused merge() to crash because there were more placeholder occurrences than nested snippets.
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("$1$1$0", None);
        session.insert(None);

        session.merge("${1:nested}$0", None);
        f.assert_value("nested");
    });
}

#[test]
fn snippets_transform() {
    with_fixture(|f| {
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("${1/foo/bar/}$0", None);
        session.insert(None);
        f.assert_selections(&[sel(1, 1, 1, 1)]);

        f.type_text("foo");
        session.next();

        f.assert_value("bar");
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 4, 1, 4)]);
    });
}

#[test]
fn snippets_multi_placeholder_same_index_one_transform() {
    with_fixture(|f| {
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("$1 baz ${1/foo/bar/}$0", None);
        session.insert(None);
        f.assert_selections(&[sel(1, 1, 1, 1), sel(1, 6, 1, 6)]);

        f.type_text("foo");
        session.next();

        f.assert_value("foo baz bar");
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 12, 1, 12)]);
    });
}

#[test]
fn snippets_transform_example() {
    with_fixture(|f| {
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("${1:name} : ${2:type}${3/\\s:=(.*)/${1:+ :=}${1}/};\n$0", None);
        session.insert(None);

        f.assert_selections(&[sel(1, 1, 1, 5)]);
        f.type_text("clk");
        session.next();

        f.assert_selections(&[sel(1, 7, 1, 11)]);
        f.type_text("std_logic");
        session.next();

        f.assert_selections(&[sel(1, 16, 1, 16)]);
        session.next();

        f.assert_value("clk : std_logic;\n");
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(2, 1, 2, 1)]);
    });
}

#[test]
fn snippets_transform_with_indent() {
    with_fixture(|f| {
        let snippet = [
            "private readonly ${1} = new Emitter<$2>();",
            "readonly ${1/^_(.*)/$1/}: Event<$2> = this.$1.event;",
            "$0",
        ]
        .join("\n");
        let expected = [
            "{",
            "\tprivate readonly _prop = new Emitter<string>();",
            "\treadonly prop: Event<string> = this._prop.event;",
            "\t",
            "}",
        ]
        .join("\n");
        let base = ["{", "\t", "}"].join("\n");

        f.model.set_value(base.into());
        f.model.update_options(TextModelUpdateOptions { insert_spaces: Some(false), ..Default::default() });
        f.editor.set_selection(sel(2, 2, 2, 2).into(), None);

        let session = f.session(&snippet, None);
        session.insert(None);

        f.assert_selections(&[sel(2, 19, 2, 19), sel(3, 11, 3, 11), sel(3, 28, 3, 28)]);
        f.type_text("_prop");
        session.next();

        f.assert_selections(&[sel(2, 39, 2, 39), sel(3, 23, 3, 23)]);
        f.type_text("string");
        session.next();

        f.assert_value(&expected);
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(4, 2, 4, 2)]);
    });
}

#[test]
fn snippets_transform_example_hit_if() {
    with_fixture(|f| {
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("${1:name} : ${2:type}${3/\\s:=(.*)/${1:+ :=}${1}/};\n$0", None);
        session.insert(None);

        f.assert_selections(&[sel(1, 1, 1, 5)]);
        f.type_text("clk");
        session.next();

        f.assert_selections(&[sel(1, 7, 1, 11)]);
        f.type_text("std_logic");
        session.next();

        f.assert_selections(&[sel(1, 16, 1, 16)]);
        f.type_text(" := '1'");
        session.next();

        f.assert_value("clk : std_logic := '1';\n");
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(2, 1, 2, 1)]);
    });
}

#[test]
fn snippet_tab_stop_selection_issue_96545_transform_adjacent_to_previous_placeholder() {
    with_fixture(|f| {
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("${1:{}${2:fff}${1/{/}/}", None);
        session.insert(None);

        f.assert_selections(&[sel(1, 1, 1, 2), sel(1, 5, 1, 6)]);
        session.next();

        f.assert_value("{fff}");
        f.assert_selections(&[sel(1, 2, 1, 5)]);
        f.type_text("ggg");
        session.next();

        f.assert_value("{ggg}");
        assert!(session.is_at_last_placeholder());
        f.assert_selections(&[sel(1, 6, 1, 6)]);
    });
}

#[test]
fn snippet_tab_stop_selection_issue_96545() {
    with_fixture(|f| {
        f.model.set_value("".into());
        let session = f.session("${1:{}${2:fff}${1/[\\{]/}/}$0", None);
        session.insert(None);
        f.assert_value("{fff{");

        f.assert_selections(&[sel(1, 1, 1, 2), sel(1, 5, 1, 6)]);
        session.next();
        f.assert_selections(&[sel(1, 2, 1, 5)]);
    });
}

#[test]
fn snippet_placeholder_index_incorrect_after_using_2_plus_snippets_in_a_row_that_each_end_with_a_placeholder_30769() {
    with_fixture(|f| {
        f.model.set_value("".into());
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        let session = f.session("test ${1:replaceme}", None);
        session.insert(None);

        f.type_text("1");
        f.type_text("\n");
        f.assert_value("test 1\n");

        session.merge("test ${1:replaceme}", None);
        f.type_text("2");
        f.type_text("\n");

        f.assert_value("test 1\ntest 2\n");

        session.merge("test ${1:replaceme}", None);
        f.type_text("3");
        f.type_text("\n");

        f.assert_value("test 1\ntest 2\ntest 3\n");

        session.merge("test ${1:replaceme}", None);
        f.type_text("4");
        f.type_text("\n");

        f.assert_value("test 1\ntest 2\ntest 3\ntest 4\n");
    });
}

#[test]
fn snippet_variable_text_isnt_whitespace_normalised_31124() {
    with_fixture(|f| {
        f.model.set_value(["start", "\t\t-one", "\t\t-two", "end"].join("\n").into());

        f.model.update_options(TextModelUpdateOptions { insert_spaces: Some(false), ..Default::default() });
        f.editor.set_selection(sel(2, 2, 3, 7).into(), None);

        f.session("<div>\n\t$TM_SELECTED_TEXT\n</div>$0", None).insert(None);

        let mut expected = ["start", "\t<div>", "\t\t\t-one", "\t\t\t-two", "\t</div>", "end"].join("\n");

        f.assert_value(&expected);

        f.model.set_value(["start", "\t\t-one", "\t-two", "end"].join("\n").into());

        f.model.update_options(TextModelUpdateOptions { insert_spaces: Some(false), ..Default::default() });
        f.editor.set_selection(sel(2, 2, 3, 7).into(), None);

        f.session("<div>\n\t$TM_SELECTED_TEXT\n</div>$0", None).insert(None);

        expected = ["start", "\t<div>", "\t\t\t-one", "\t\t-two", "\t</div>", "end"].join("\n");

        f.assert_value(&expected);
    });
}

#[test]
fn selecting_text_from_left_to_right_and_choosing_item_messes_up_code_31199() {
    with_fixture(|f| {
        f.model.set_value("console.log".into());

        let mut actual = SnippetSession::adjust_selection(&f.model, &sel(1, 12, 1, 9), 3, 0);
        assert_eq!(actual, sel(1, 9, 1, 6));

        actual = SnippetSession::adjust_selection(&f.model, &sel(1, 9, 1, 12), 3, 0);
        assert_eq!(actual, sel(1, 9, 1, 12));

        f.editor.set_selections(&[sel(1, 9, 1, 12)], None, None);
        f.session(
            "far",
            Some(SnippetSessionInsertOptions {
                overwrite_before: 3,
                overwrite_after: 0,
                adjust_whitespace: true,
                clipboard_text: None,
                overtyping_capturer: None,
            }),
        )
        .insert(None);
        f.assert_value("console.far");
    });
}

#[test]
fn tabs_dont_get_replaced_with_spaces_in_snippet_transformations_103818() {
    with_fixture(|f| {
        f.model.set_value("\n{\n  \n}".into());
        f.model.update_options(TextModelUpdateOptions {
            insert_spaces: Some(true),
            indent_size: Some(IndentSize::Size(2)),
            ..Default::default()
        });
        f.editor.set_selections(&[sel(1, 1, 1, 1), sel(3, 6, 3, 6)], None, None);
        let session = f.session(
            &[
                "function animate () {",
                "\tvar ${1:a} = 12;",
                "\tconsole.log(${1/(.*)/\n\t\t$1\n\t/})",
                "}",
            ]
            .join("\n"),
            None,
        );

        session.insert(None);

        f.assert_value(
            &[
                "function animate () {",
                "  var a = 12;",
                "  console.log(a)",
                "}",
                "{",
                "  function animate () {",
                "    var a = 12;",
                "    console.log(a)",
                "  }",
                "}",
            ]
            .join("\n"),
        );

        f.type_text("bbb");
        session.next();

        f.assert_value(
            &[
                "function animate () {",
                "  var bbb = 12;",
                "  console.log(",
                "    bbb",
                "  )",
                "}",
                "{",
                "  function animate () {",
                "    var bbb = 12;",
                "    console.log(",
                "      bbb",
                "    )",
                "  }",
                "}",
            ]
            .join("\n"),
        );
    });
}

mod create_edits_and_snippets_from_edits {
    use super::*;

    fn edit(range: Range, template: &str) -> SnippetEdit {
        SnippetEdit { range, template: JsString::from(template), keep_whitespace: None }
    }

    /// `SnippetSession.createEditsAndSnippetsFromEdits(editor, edits, true, true, undefined, undefined, service)`.
    fn create(f: &Fixture, edits: &[SnippetEdit]) -> je_editor::snippet::snippet_session::EditsAndSnippets {
        SnippetSession::create_edits_and_snippets_from_edits(
            f.editor.widget(),
            edits,
            true,
            &SnippetSessionInsertOptions::default(),
            &f.services,
        )
    }

    #[test]
    fn empty() {
        with_fixture(|f| {
            let result = create(f, &[]);

            assert!(result.edits.is_empty());
            assert!(result.snippets.is_empty());
        });
    }

    #[test]
    fn basic() {
        with_fixture(|f| {
            f.model.set_value("foo(\"bar\")".into());

            let result = create(
                f,
                &[
                    edit(Range::new(1, 5, 1, 9), "$1"),
                    edit(Range::new(1, 1, 1, 1), "const ${1:new_const} = \"bar\""),
                ],
            );

            assert_eq!(result.edits.len(), 2);
            assert_eq!(result.edits[0].range, Range::new(1, 1, 1, 1));
            assert_eq!(result.edits[0].text, text("const new_const = \"bar\""));
            assert_eq!(result.edits[1].range, Range::new(1, 5, 1, 9));
            assert_eq!(result.edits[1].text, text("new_const"));

            assert_eq!(result.snippets.len(), 1);
            assert!(!result.snippets[0].is_trivial_snippet());
        });
    }

    #[test]
    fn with_selection_variable() {
        with_fixture(|f| {
            f.model.set_value("Some text and a selection".into());
            f.editor.set_selections(&[sel(1, 17, 1, 26)], None, None);

            let result = create(f, &[edit(Range::new(1, 17, 1, 26), "wrapped <$SELECTION>")]);

            assert_eq!(result.edits.len(), 1);
            assert_eq!(result.edits[0].range, Range::new(1, 17, 1, 26));
            assert_eq!(result.edits[0].text, text("wrapped <selection>"));

            assert_eq!(result.snippets.len(), 1);
            assert!(result.snippets[0].is_trivial_snippet());
        });
    }

    #[test]
    fn tm_selected_text_resolves_per_edit_not_the_primary_selection_206121() {
        with_fixture(|f| {
            f.model.set_value("aaa\nbbb\nccc".into());
            // primary selection covers "aaa", but the edits target other lines
            f.editor.set_selections(&[sel(1, 1, 1, 4)], None, None);

            let result = create(
                f,
                &[
                    edit(Range::new(2, 1, 2, 4), "[$TM_SELECTED_TEXT]"),
                    edit(Range::new(3, 1, 3, 4), "[$TM_SELECTED_TEXT]"),
                ],
            );

            assert_eq!(result.edits.len(), 2);
            assert_eq!(result.edits[0].range, Range::new(2, 1, 2, 4));
            assert_eq!(result.edits[0].text, text("[bbb]"));
            assert_eq!(result.edits[1].range, Range::new(3, 1, 3, 4));
            assert_eq!(result.edits[1].text, text("[ccc]"));
        });
    }

    #[test]
    fn tm_line_number_resolves_per_edit() {
        with_fixture(|f| {
            f.model.set_value("a\nb\nc".into());
            f.editor.set_selections(&[sel(1, 1, 1, 1)], None, None);

            let result = create(
                f,
                &[edit(Range::new(1, 2, 1, 2), "$TM_LINE_NUMBER"), edit(Range::new(3, 2, 3, 2), "$TM_LINE_NUMBER")],
            );

            assert_eq!(result.edits.len(), 2);
            assert_eq!(result.edits[0].text, text("1"));
            assert_eq!(result.edits[1].text, text("3"));
        });
    }

    #[test]
    fn per_edit_resolution_does_not_corrupt_earlier_edits_when_value_lengths_differ() {
        with_fixture(|f| {
            // 100 lines so $TM_LINE_NUMBER produces values of differing widths (1 vs 100)
            let lines: Vec<String> = (0..100).map(|i| format!("line{}", i + 1)).collect();
            f.model.set_value(lines.join("\n").into());
            f.editor.set_selections(&[sel(1, 1, 1, 1)], None, None);

            let result = create(
                f,
                &[edit(Range::new(1, 2, 1, 2), "$TM_LINE_NUMBER"), edit(Range::new(100, 2, 100, 2), "$TM_LINE_NUMBER")],
            );

            assert_eq!(result.edits.len(), 2);
            assert_eq!(result.edits[0].text, text("1"));
            assert_eq!(result.edits[1].text, text("100"));
        });
    }

    #[test]
    fn cursor_number_uses_caller_supplied_edit_order_not_range_sorted_order() {
        with_fixture(|f| {
            f.model.set_value("xx\nyy".into());
            f.editor.set_selections(&[sel(1, 1, 1, 1)], None, None);

            let result = create(
                f,
                &[edit(Range::new(2, 3, 2, 3), "$CURSOR_NUMBER"), edit(Range::new(1, 3, 1, 3), "$CURSOR_NUMBER")],
            );

            assert_eq!(result.edits.len(), 2);
            assert_eq!(result.edits[0].range, Range::new(1, 3, 1, 3));
            assert_eq!(result.edits[0].text, text("2"));
            assert_eq!(result.edits[1].range, Range::new(2, 3, 2, 3));
            assert_eq!(result.edits[1].text, text("1"));
        });
    }

    #[test]
    fn cross_edit_placeholder_backfill_resolves_variables_in_earlier_edit() {
        with_fixture(|f| {
            // parseFragment backfills the earlier $1 with a clone of the later default;
            // the cloned $TM_LINE_NUMBER lives outside the second edit's newNodes
            f.model.set_value("aaa\nbbb".into());
            f.editor.set_selections(&[sel(1, 1, 1, 1)], None, None);

            let result = create(
                f,
                &[edit(Range::new(1, 2, 1, 2), "$1"), edit(Range::new(2, 2, 2, 2), "${1:$TM_LINE_NUMBER}")],
            );

            let mut has_unresolved_variable = false;
            let inner_snippet = result.snippets[0].snippet();
            inner_snippet.walk(|marker| {
                if marker.as_variable().is_some() && marker.children().is_empty() {
                    has_unresolved_variable = true;
                }
                true
            });
            assert!(!has_unresolved_variable, "backfilled $TM_LINE_NUMBER in earlier edit should be resolved");
        });
    }
}
