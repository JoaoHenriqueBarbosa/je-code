//! Porte de `src/vs/editor/contrib/snippet/test/browser/snippetController2.old.test.ts` (suíte `SnippetController`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` que cada `#[test]` cria no começo (o original o faz
//!   por caso, e um caso pode chamar `snippetTest` várias vezes).
//! - `snippetTest(cb, lines)` é o [`snippet_test`]: cria o `ContextKeyService` (o `IContextKeyService` do original vem do
//!   `createCodeEditorServices`, que o porte do harness não tem), o editor com `withTestCodeEditor`, desliga
//!   `insertSpaces` no modelo, registra o [`TestSnippetController`] com `registerAndInstantiateContribution`, roda o
//!   callback e descarta o controlador e o serviço de chaves.
//! - `TestSnippetController extends SnippetController2` é o [`TestSnippetController`]: guarda o controlador, o seu
//!   `TestLanguageConfigurationService` (descartado junto) e o `isInSnippetMode()`, e tem `Deref` para o controlador. O
//!   `dispose` é idempotente porque o editor também descarta a contribuição registrada (o `Disposable.dispose` do
//!   original também o é).
//! - `new class extends mock<ILabelService>() { }` é o `ClosureLabelService::without_methods` e o
//!   `IWorkspaceContextService` sem métodos é o `WorkspaceOnlyService` (que só define `getWorkspace()`).
//! - `editor.trigger('test', 'type', { text })` é o `type_text`; `first.equalsRange({...})` compara o intervalo da
//!   seleção (sem a direção) com o esperado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod snippet_doubles;
mod support;

use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::code_editor_widget::{Handler, ICodeEditor, IEditorContribution, TypePayload};
use je_editor::model::TextModelUpdateOptions;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::snippet::snippet_controller2::{
    PartialSnippetInsertOptions, SnippetController2, SnippetController2Services,
};
use je_editor::snippet::snippet_session::SnippetServices;
use je_editor::text_model::TextModel;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::log::NullLogService;
use je_platform::workspace::Workspace;
use snippet_doubles::{ClosureLabelService, WorkspaceOnlyService};
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};
use support::test_language_configuration_service::TestLanguageConfigurationService;

fn sel(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Selection {
    Selection::new(start_line_number, start_column, end_line_number, end_column)
}

/// `{ overwriteBefore }`.
fn overwrite_before(value: i32) -> Option<PartialSnippetInsertOptions> {
    Some(PartialSnippetInsertOptions { overwrite_before: Some(value), ..Default::default() })
}

/// `selection.equalsRange({ startLineNumber, startColumn, endLineNumber, endColumn })`.
fn assert_range(selection: &Selection, start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) {
    assert_eq!(**selection, Range::new(start_line_number, start_column, end_line_number, end_column), "{selection}");
}

/// `TestSnippetController`.
struct TestSnippetController {
    controller: Rc<SnippetController2>,
    language_configuration_service: Rc<TestLanguageConfigurationService>,
    context_key_service: Rc<ContextKeyService>,
    disposed: Cell<bool>,
}

impl Deref for TestSnippetController {
    type Target = SnippetController2;

    fn deref(&self) -> &SnippetController2 {
        &self.controller
    }
}

impl TestSnippetController {
    /// `isInSnippetMode()`: `SnippetController2.InSnippetMode.getValue(this._contextKeyService)!`.
    fn is_in_snippet_mode(&self) -> bool {
        match SnippetController2::in_snippet_mode().get_value(&*self.context_key_service) {
            je_base::dynamic::Dynamic::Bool(value) => value,
            other => panic!("a chave inSnippetMode não é booleana: {other:?}"),
        }
    }
}

impl IDisposable for TestSnippetController {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.controller.dispose();
        self.language_configuration_service.dispose();
    }
}

impl IEditorContribution for TestSnippetController {}

const DEFAULT_LINES: [&str; 5] = ["function test() {", "\tvar x = 3;", "\tvar arr = [];", "\t", "}"];

/// `snippetTest(cb, lines)`.
fn snippet_test(cb: impl FnOnce(&Rc<TestCodeEditor>, &str, &TestSnippetController), lines: Option<&[&str]>) {
    let lines: &[&str] = lines.unwrap_or(&DEFAULT_LINES);
    let context_keys = ContextKeyService::new(TestConfigurationService::new(None));

    with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _services| {
        editor
            .get_model()
            .expect("o editor tem o modelo")
            .update_options(TextModelUpdateOptions { insert_spaces: Some(false), ..Default::default() });
        let snippet_controller = editor.register_and_instantiate_contribution(SnippetController2::ID, |widget, _services| {
            let language_configuration_service = TestLanguageConfigurationService::new();
            let workspace = Workspace::new("workspace-id", Vec::new(), false, None, Rc::new(|_: &je_base::uri::URI| false), None);
            let context_key_service: Rc<dyn IContextKeyService> = context_keys.clone();
            let controller = SnippetController2::new(
                widget,
                SnippetController2Services {
                    log_service: NullLogService::shared(),
                    choice_host: None,
                    context_key_service,
                    snippet_services: Rc::new(SnippetServices {
                        language_configuration_service: language_configuration_service.clone(),
                        label_service: ClosureLabelService::without_methods(),
                        workspace_service: Some(WorkspaceOnlyService::new(workspace)),
                    }),
                },
            );
            Rc::new(TestSnippetController {
                controller,
                language_configuration_service,
                context_key_service: context_keys.clone(),
                disposed: Cell::new(false),
            })
        });
        let template = [
            "for (var ${1:index}; $1 < ${2:array}.length; $1++) {",
            "\tvar element = $2[$1];",
            "\t$0",
            "}",
        ]
        .join("\n");

        cb(editor, &template, &snippet_controller);
        snippet_controller.dispose();
    });
    context_keys.dispose();
}

fn model_of(editor: &TestCodeEditor) -> Rc<TextModel> {
    editor.get_model().expect("o editor tem o modelo")
}

/// `assert.strictEqual(editor.getModel()!.getLineContent(line), expected)`.
fn assert_line(editor: &TestCodeEditor, line_number: i32, expected: &str) {
    assert_eq!(model_of(editor).get_line_content(line_number).to_string_lossy(), expected);
}

/// `assert.strictEqual(editor.getModel()!.getValue(), expected)`.
fn assert_value(editor: &TestCodeEditor, expected: &str) {
    assert_eq!(model_of(editor).get_value().to_string_lossy(), expected);
}

/// `editor.trigger('test', 'type', { text })`.
fn type_text(editor: &TestCodeEditor, text: &str) {
    editor.trigger(Some("test"), Handler::TYPE, Some(TypePayload { text: JsString::from(text) }.to_dynamic()));
}

#[test]
fn simple_accepted() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, template, snippet_controller| {
            editor.set_position(Position::new(4, 2), None);

            snippet_controller.insert(template, None);
            assert_line(editor, 4, "\tfor (var index; index < array.length; index++) {");
            assert_line(editor, 5, "\t\tvar element = array[index];");
            assert_line(editor, 6, "\t\t");
            assert_line(editor, 7, "\t}");

            type_text(editor, "i");
            assert_line(editor, 4, "\tfor (var i; i < array.length; i++) {");
            assert_line(editor, 5, "\t\tvar element = array[i];");
            assert_line(editor, 6, "\t\t");
            assert_line(editor, 7, "\t}");

            snippet_controller.next();
            type_text(editor, "arr");
            assert_line(editor, 4, "\tfor (var i; i < arr.length; i++) {");
            assert_line(editor, 5, "\t\tvar element = arr[i];");
            assert_line(editor, 6, "\t\t");
            assert_line(editor, 7, "\t}");

            snippet_controller.prev();
            type_text(editor, "j");
            assert_line(editor, 4, "\tfor (var j; j < arr.length; j++) {");
            assert_line(editor, 5, "\t\tvar element = arr[j];");
            assert_line(editor, 6, "\t\t");
            assert_line(editor, 7, "\t}");

            snippet_controller.next();
            snippet_controller.next();
            assert_eq!(editor.get_position(), Some(Position::new(6, 3)));
        },
        None,
    );
}

#[test]
fn simple_canceled() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, template, snippet_controller| {
            editor.set_position(Position::new(4, 2), None);

            snippet_controller.insert(template, None);
            assert_line(editor, 4, "\tfor (var index; index < array.length; index++) {");
            assert_line(editor, 5, "\t\tvar element = array[index];");
            assert_line(editor, 6, "\t\t");
            assert_line(editor, 7, "\t}");

            snippet_controller.cancel(false);
            assert_eq!(editor.get_position(), Some(Position::new(4, 16)));
        },
        None,
    );
}

// test('Stops when deleting lines above', ...), 'Stops when deleting lines below', 'Stops when inserting lines
// above' e 'Stops when inserting lines below' estão comentados no original.

#[test]
fn stops_when_calling_model_set_value() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, code_snippet, snippet_controller| {
            editor.set_position(Position::new(4, 2), None);
            snippet_controller.insert(code_snippet, None);

            model_of(editor).set_value("goodbye".into());

            assert!(!snippet_controller.is_in_snippet_mode());
        },
        None,
    );
}

#[test]
fn stops_when_undoing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, code_snippet, snippet_controller| {
            editor.set_position(Position::new(4, 2), None);
            snippet_controller.insert(code_snippet, None);

            model_of(editor).undo();

            assert!(!snippet_controller.is_in_snippet_mode());
        },
        None,
    );
}

#[test]
fn stops_when_moving_cursor_outside() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, code_snippet, snippet_controller| {
            editor.set_position(Position::new(4, 2), None);
            snippet_controller.insert(code_snippet, None);

            editor.set_position(Position::new(1, 1), None);

            assert!(!snippet_controller.is_in_snippet_mode());
        },
        None,
    );
}

#[test]
fn stops_when_disconnecting_editor_model() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, code_snippet, snippet_controller| {
            editor.set_position(Position::new(4, 2), None);
            snippet_controller.insert(code_snippet, None);

            editor.set_model(None);

            assert!(!snippet_controller.is_in_snippet_mode());
        },
        None,
    );
}

#[test]
fn stops_when_disposing_editor() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, code_snippet, snippet_controller| {
            editor.set_position(Position::new(4, 2), None);
            snippet_controller.insert(code_snippet, None);

            snippet_controller.dispose();

            assert!(!snippet_controller.is_in_snippet_mode());
        },
        None,
    );
}

#[test]
fn final_tabstop_with_multiple_selections() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, _code_snippet, snippet_controller| {
            editor.set_selections(&[sel(1, 1, 1, 1), sel(2, 1, 2, 1)], None, None);

            let code_snippet = "foo$0";
            snippet_controller.insert(code_snippet, None);

            let selections = editor.get_selections().expect("o editor tem o modelo");
            assert_eq!(selections.len(), 2);
            let [first, second] = selections.as_slice() else { unreachable!() };
            assert_range(first, 1, 4, 1, 4);
            assert_range(second, 2, 4, 2, 4);
        },
        None,
    );

    snippet_test(
        |editor, _code_snippet, snippet_controller| {
            editor.set_selections(&[sel(1, 1, 1, 1), sel(2, 1, 2, 1)], None, None);

            let code_snippet = "foo$0bar";
            snippet_controller.insert(code_snippet, None);

            let selections = editor.get_selections().expect("o editor tem o modelo");
            assert_eq!(selections.len(), 2);
            let [first, second] = selections.as_slice() else { unreachable!() };
            assert_range(first, 1, 4, 1, 4);
            assert_range(second, 2, 4, 2, 4);
        },
        None,
    );

    snippet_test(
        |editor, _code_snippet, snippet_controller| {
            editor.set_selections(&[sel(1, 1, 1, 1), sel(1, 5, 1, 5)], None, None);

            let code_snippet = "foo$0bar";
            snippet_controller.insert(code_snippet, None);

            let selections = editor.get_selections().expect("o editor tem o modelo");
            assert_eq!(selections.len(), 2);
            let [first, second] = selections.as_slice() else { unreachable!() };
            assert_range(first, 1, 4, 1, 4);
            assert_range(second, 1, 14, 1, 14);
        },
        None,
    );

    snippet_test(
        |editor, _code_snippet, snippet_controller| {
            editor.set_selections(&[sel(1, 1, 1, 1), sel(1, 5, 1, 5)], None, None);

            let code_snippet = "foo\n$0\nbar";
            snippet_controller.insert(code_snippet, None);

            let selections = editor.get_selections().expect("o editor tem o modelo");
            assert_eq!(selections.len(), 2);
            let [first, second] = selections.as_slice() else { unreachable!() };
            assert_range(first, 2, 1, 2, 1);
            assert_range(second, 4, 1, 4, 1);
        },
        None,
    );

    snippet_test(
        |editor, _code_snippet, snippet_controller| {
            editor.set_selections(&[sel(1, 1, 1, 1), sel(1, 5, 1, 5)], None, None);

            let code_snippet = "foo\n$0\nbar";
            snippet_controller.insert(code_snippet, None);

            let selections = editor.get_selections().expect("o editor tem o modelo");
            assert_eq!(selections.len(), 2);
            let [first, second] = selections.as_slice() else { unreachable!() };
            assert_range(first, 2, 1, 2, 1);
            assert_range(second, 4, 1, 4, 1);
        },
        None,
    );

    snippet_test(
        |editor, _code_snippet, snippet_controller| {
            editor.set_selections(&[sel(2, 7, 2, 7)], None, None);

            let code_snippet = "xo$0r";
            snippet_controller.insert(code_snippet, overwrite_before(1));

            let selections = editor.get_selections().expect("o editor tem o modelo");
            assert_eq!(selections.len(), 1);
            assert_range(&editor.get_selection().expect("o editor tem o modelo"), 2, 8, 2, 8);
        },
        None,
    );
}

#[test]
fn final_tabstop_11742_simple() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selection(sel(1, 19, 1, 19).into(), None);

            let code_snippet = "{{% url_**$1** %}}";
            controller.insert(code_snippet, overwrite_before(2));

            assert_eq!(editor.get_selections().expect("o editor tem o modelo").len(), 1);
            assert_range(&editor.get_selection().expect("o editor tem o modelo"), 1, 27, 1, 27);
            assert_value(editor, "example example {{% url_**** %}}");
        },
        Some(&["example example sc"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selection(sel(1, 3, 1, 3).into(), None);

            let code_snippet = ["afterEach((done) => {", "\t${1}test", "});"].join("\n");

            controller.insert(code_snippet.as_str(), overwrite_before(2));

            assert_eq!(editor.get_selections().expect("o editor tem o modelo").len(), 1);
            assert_range(&editor.get_selection().expect("o editor tem o modelo"), 2, 2, 2, 2);
            assert_value(editor, "afterEach((done) => {\n\ttest\n});");
        },
        Some(&["af"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selection(sel(1, 3, 1, 3).into(), None);

            let code_snippet = ["afterEach((done) => {", "${1}\ttest", "});"].join("\n");

            controller.insert(code_snippet.as_str(), overwrite_before(2));

            assert_eq!(editor.get_selections().expect("o editor tem o modelo").len(), 1);
            assert_range(&editor.get_selection().expect("o editor tem o modelo"), 2, 1, 2, 1);
            assert_value(editor, "afterEach((done) => {\n\ttest\n});");
        },
        Some(&["af"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selection(sel(1, 9, 1, 9).into(), None);

            let code_snippet = ["aft${1}er"].join("\n");

            controller.insert(code_snippet.as_str(), overwrite_before(8));

            assert_value(editor, "after");
            assert_eq!(editor.get_selections().expect("o editor tem o modelo").len(), 1);
            assert_range(&editor.get_selection().expect("o editor tem o modelo"), 1, 4, 1, 4);
        },
        Some(&["afterone"]),
    );
}

#[test]
fn final_tabstop_11742_different_indents() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(2, 4, 2, 4), sel(1, 3, 1, 3)], None, None);

            let code_snippet = ["afterEach((done) => {", "\t${0}test", "});"].join("\n");

            controller.insert(code_snippet.as_str(), overwrite_before(2));

            let selections = editor.get_selections().expect("o editor tem o modelo");
            assert_eq!(selections.len(), 2);
            let [first, second] = selections.as_slice() else { unreachable!() };

            assert_range(first, 5, 3, 5, 3);
            assert_range(second, 2, 2, 2, 2);
        },
        Some(&["af", "\taf"]),
    );
}

#[test]
fn final_tabstop_11890_stay_at_the_beginning() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(1, 5, 1, 5)], None, None);

            let code_snippet = ["afterEach((done) => {", "${1}\ttest", "});"].join("\n");

            controller.insert(code_snippet.as_str(), overwrite_before(2));

            let selections = editor.get_selections().expect("o editor tem o modelo");
            assert_eq!(selections.len(), 1);
            let [first] = selections.as_slice() else { unreachable!() };

            assert_range(first, 2, 3, 2, 3);
        },
        Some(&["  af"]),
    );
}

#[test]
fn final_tabstop_no_tabstop() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(1, 3, 1, 3)], None, None);

            let code_snippet = "afterEach";

            controller.insert(code_snippet, overwrite_before(2));

            assert_range(&editor.get_selection().expect("o editor tem o modelo"), 1, 10, 1, 10);
        },
        Some(&["af", "\taf"]),
    );
}

#[test]
fn multiple_cursor_and_overwrite_before_after_issue_11060() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(1, 7, 1, 7), sel(2, 4, 2, 4)], None, None);

            let code_snippet = "_foo";
            controller.insert(code_snippet, overwrite_before(1));
            assert_value(editor, "this._foo\nabc_foo");
        },
        Some(&["this._", "abc"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(1, 7, 1, 7), sel(2, 4, 2, 4)], None, None);

            let code_snippet = "XX";
            controller.insert(code_snippet, overwrite_before(1));
            assert_value(editor, "this.XX\nabcXX");
        },
        Some(&["this._", "abc"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(1, 7, 1, 7), sel(2, 4, 2, 4), sel(3, 5, 3, 5)], None, None);

            let code_snippet = "_foo";
            controller.insert(code_snippet, overwrite_before(1));
            assert_value(editor, "this._foo\nabc_foo\ndef_foo");
        },
        Some(&["this._", "abc", "def_"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(
                &[
                    sel(1, 7, 1, 7), // primary at `this._`
                    sel(2, 4, 2, 4),
                    sel(3, 6, 3, 6),
                ],
                None,
                None,
            );

            let code_snippet = "._foo";
            controller.insert(code_snippet, overwrite_before(2));
            assert_value(editor, "this._foo\nabc._foo\ndef._foo");
        },
        Some(&["this._", "abc", "def._"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(
                &[
                    sel(3, 6, 3, 6), // primary at `def._`
                    sel(1, 7, 1, 7),
                    sel(2, 4, 2, 4),
                ],
                None,
                None,
            );

            let code_snippet = "._foo";
            controller.insert(code_snippet, overwrite_before(2));
            assert_value(editor, "this._foo\nabc._foo\ndef._foo");
        },
        Some(&["this._", "abc", "def._"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(
                &[
                    sel(2, 4, 2, 4), // primary at `abc`
                    sel(3, 6, 3, 6),
                    sel(1, 7, 1, 7),
                ],
                None,
                None,
            );

            let code_snippet = "._foo";
            controller.insert(code_snippet, overwrite_before(2));
            assert_value(editor, "this._._foo\na._foo\ndef._._foo");
        },
        Some(&["this._", "abc", "def._"]),
    );
}

#[test]
fn multiple_cursor_and_overwrite_before_after_16277() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(1, 5, 1, 5), sel(2, 5, 2, 5)], None, None);

            let code_snippet = "document";
            controller.insert(code_snippet, overwrite_before(3));
            assert_value(editor, "{document}\n{document && true}");
        },
        Some(&["{foo}", "{foo && true}"]),
    );
}

#[test]
fn insert_snippet_twice_19449() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(1, 1, 1, 1)], None, None);

            let code_snippet = "for (var ${1:i}=0; ${1:i}<len; ${1:i}++) { $0 }";
            controller.insert(code_snippet, None);
            assert_value(editor, "for (var i=0; i<len; i++) {  }for (var i=0; i<len; i++) {  }");
        },
        Some(&["for (var i=0; i<len; i++) {  }"]),
    );

    snippet_test(
        |editor, _code_snippet, controller| {
            editor.set_selections(&[sel(1, 1, 1, 1)], None, None);

            let code_snippet = "for (let ${1:i}=0; ${1:i}<len; ${1:i}++) { $0 }";
            controller.insert(code_snippet, None);
            assert_value(editor, "for (let i=0; i<len; i++) {  }for (var i=0; i<len; i++) {  }");
        },
        Some(&["for (var i=0; i<len; i++) {  }"]),
    );
}
