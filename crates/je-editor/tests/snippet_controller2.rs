//! Porte de `src/vs/editor/contrib/snippet/test/browser/snippetController2.test.ts` (suíte `SnippetController2` e a
//! subsuíte `createEditsAndSnippetsFromEdits`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` e `ensureNoDisposablesAreLeakedInTestSuite()` são o `with_fixture`: cria o `LeakGuard`, o
//!   `MockContextKeyService` (aqui o `ContextKeyService` de `je-platform` com o `TestConfigurationService`), o modelo, o
//!   editor de teste e as seleções iniciais, roda o caso e descarta o modelo, o controlador, o editor e o serviço de
//!   chaves (o `teardown` do original descarta o modelo e o controlador; o editor e o serviço pertencem ao
//!   `LeakGuard` do porte).
//! - `instaService.createInstance(SnippetController2, editor)` é o `Fixture::create_controller`, que injeta o
//!   `SnippetController2Services` (log nulo, o `ContextKeyService`, o `ILanguageConfigurationService` do editor de teste, o
//!   `ILabelService` sem métodos e o `IWorkspaceContextService` que só define `getWorkspace()`).
//! - `SnippetController2.InSnippetMode.getValue(service)` é o `get_value` do `RawContextKey` (o valor é booleano).
//! - `editor.trigger('test', 'type', ...)` é o `type_text`; `editor.trigger('test', 'cut' | Handler.Cut, ...)` é o
//!   `trigger` com o handler `cut`; `editor.trigger('test', 'tab', {})` e `editor.runCommand(CoreEditingCommands.Tab, null)`
//!   são o comando `tab` do `core_editing_commands` (o `NoExtensionsRegistry` do editor de teste não resolve o id);
//!   `editor.runCommand(CoreEditingCommands.Undo, null)` é o `undo` do mesmo módulo.
//! - `assertSelections` e os `assert.deepStrictEqual(editor.getSelections(), [...])` comparam a lista inteira das seleções
//!   do editor com a esperada (com a direção, como o `toString` da `Selection`).
//! - `test.skip` é o `#[ignore]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod snippet_doubles;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_editor::code_editor_widget::{Handler, ICodeEditor, IEditorCommand, TypePayload};
use je_editor::core_commands::EditorOrNativeTextInputCommand;
use je_editor::core_commands::core_editing_commands;
use je_editor::model::{EndOfLineSequence, TextModelUpdateOptions};
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::snippet::snippet_controller2::{
    PartialSnippetInsertOptions, SnippetController2, SnippetController2Services,
};
use je_editor::snippet::snippet_session::{SnippetEdit, SnippetServices};
use je_editor::text_model::TextModel;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::{IContextKeyService, RawContextKey};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::log::NullLogService;
use je_platform::workspace::Workspace;
use snippet_doubles::{ClosureLabelService, WorkspaceOnlyService};
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, create_test_code_editor};
use support::test_text_model::create_text_model;

fn sel(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Selection {
    Selection::new(start_line_number, start_column, end_line_number, end_column)
}

fn edit(range: Range, template: &str) -> SnippetEdit {
    SnippetEdit { range, template: JsString::from(template), keep_whitespace: None }
}

/// `{ adjustWhitespace }`.
fn adjust_whitespace(value: bool) -> Option<PartialSnippetInsertOptions> {
    Some(PartialSnippetInsertOptions { adjust_whitespace: Some(value), ..Default::default() })
}

/// O que `getContextState()` devolve.
#[derive(Debug, PartialEq, Eq)]
struct ContextState {
    in_snippet: bool,
    has_prev: bool,
    has_next: bool,
}

/// O que o `setup` deixa em variáveis da suíte.
struct Fixture {
    model: Rc<TextModel>,
    editor: Rc<TestCodeEditor>,
    context_keys: Rc<ContextKeyService>,
    snippet_services: Rc<SnippetServices>,
    /// `ctrl`, o controlador criado pelo caso (descartado no `teardown`).
    controller: RefCell<Option<Rc<SnippetController2>>>,
}

impl Fixture {
    /// `ctrl = instaService.createInstance(SnippetController2, editor)`.
    fn create_controller(&self) -> Rc<SnippetController2> {
        let context_key_service: Rc<dyn IContextKeyService> = self.context_keys.clone();
        let controller = SnippetController2::new(
            self.editor.widget(),
            SnippetController2Services {
                log_service: NullLogService::shared(),
                choice_host: None,
                context_key_service,
                snippet_services: self.snippet_services.clone(),
            },
        );
        *self.controller.borrow_mut() = Some(controller.clone());
        controller
    }

    /// `editor.trigger('test', 'type', { text })`.
    fn type_text(&self, value: &str) {
        self.editor.trigger(Some("test"), Handler::TYPE, Some(TypePayload { text: JsString::from(value) }.to_dynamic()));
    }

    /// `editor.trigger('test', 'cut' | Handler.Cut, ...)`.
    fn cut(&self) {
        self.editor.trigger(Some("test"), Handler::CUT, None);
    }

    /// `editor.trigger('test', 'tab', {})` e `editor.runCommand(CoreEditingCommands.Tab, null)`.
    fn tab(&self) {
        let tab = core_editing_commands::instance().tab.clone();
        let _ = IEditorCommand::run_editor_command(&*tab, None, self.editor.widget(), &Dynamic::Null);
    }

    /// `editor.runCommand(CoreEditingCommands.Undo, null)`.
    fn undo(&self) {
        let undo = core_editing_commands::instance().undo.clone();
        let _ = EditorOrNativeTextInputCommand::run_editor_command(&*undo, None, self.editor.widget(), &Dynamic::Null);
    }

    /// `model.setValue(value)`.
    fn set_value(&self, value: &str) {
        self.model.set_value(value.into());
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

    /// `RawContextKey.getValue(service)`: o valor booleano da chave.
    fn key_value(&self, key: RawContextKey) -> bool {
        match key.get_value(&*self.context_keys) {
            Dynamic::Bool(value) => value,
            other => panic!("a chave de contexto não é booleana: {other:?}"),
        }
    }

    /// `getContextState()`.
    fn context_state(&self) -> ContextState {
        ContextState {
            in_snippet: self.key_value(SnippetController2::in_snippet_mode()),
            has_prev: self.key_value(SnippetController2::has_prev_tabstop()),
            has_next: self.key_value(SnippetController2::has_next_tabstop()),
        }
    }

    /// `assertContextKeys(service, inSnippet, hasPrev, hasNext)`.
    fn assert_context_keys(&self, in_snippet: bool, has_prev: bool, has_next: bool) {
        let state = self.context_state();
        assert_eq!(state.in_snippet, in_snippet, "inSnippetMode");
        assert_eq!(state.has_prev, has_prev, "HasPrevTabstop");
        assert_eq!(state.has_next, has_next, "HasNextTabstop");
    }
}

fn with_fixture(test: impl FnOnce(&Fixture)) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let context_keys = ContextKeyService::new(TestConfigurationService::new(None));
    let model = create_text_model("if\n    $state\nfi");
    let editor = create_test_code_editor(Some(model.clone()), TestCodeEditorInstantiationOptions::default());
    let workspace = Workspace::new("foo", Vec::new(), false, None, Rc::new(|_: &je_base::uri::URI| false), None);
    let snippet_services = Rc::new(SnippetServices {
        language_configuration_service: editor.widget().language_configuration_service(),
        label_service: ClosureLabelService::without_methods(),
        workspace_service: Some(WorkspaceOnlyService::new(workspace)),
    });
    editor.set_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)], None, None);
    assert_eq!(model.get_eol(), je_editor::model::Eol::Lf);

    let fixture = Fixture { model, editor, context_keys, snippet_services, controller: RefCell::new(None) };
    test(&fixture);

    // teardown
    fixture.model.dispose();
    let controller = fixture.controller.borrow_mut().take();
    if let Some(controller) = controller {
        controller.dispose();
    }
    fixture.editor.dispose();
    fixture.context_keys.dispose();
}

#[test]
fn creation() {
    with_fixture(|f| {
        f.create_controller();
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn insert_insert_abort() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        ctrl.insert("foo${1:bar}foo$0", None);
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);

        ctrl.cancel(false);
        f.assert_context_keys(false, false, false);
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);
    });
}

#[test]
fn insert_insert_tab_tab_done() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        ctrl.insert("${1:one}${2:two}$0", None);
        f.assert_context_keys(true, false, true);

        ctrl.next();
        f.assert_context_keys(true, true, true);

        ctrl.next();
        f.assert_context_keys(false, false, false);

        f.type_text("\t");
        assert!(!f.key_value(SnippetController2::in_snippet_mode()));
        assert!(!f.key_value(SnippetController2::has_next_tabstop()));
        assert!(!f.key_value(SnippetController2::has_prev_tabstop()));
    });
}

#[test]
fn insert_insert_cursor_moves_out_left_right() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        ctrl.insert("foo${1:bar}foo$0", None);
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);

        // bad selection change
        f.editor.set_selections(&[sel(1, 12, 1, 12), sel(2, 16, 2, 16)], None, None);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn insert_insert_cursor_moves_out_up_down() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        ctrl.insert("foo${1:bar}foo$0", None);
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);

        // bad selection change
        f.editor.set_selections(&[sel(2, 4, 2, 7), sel(3, 8, 3, 11)], None, None);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn insert_insert_cursors_collapse() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        ctrl.insert("foo${1:bar}foo$0", None);
        assert!(f.key_value(SnippetController2::in_snippet_mode()));
        f.assert_selections(&[sel(1, 4, 1, 7), sel(2, 8, 2, 11)]);

        // bad selection change
        f.editor.set_selections(&[sel(1, 4, 1, 7)], None, None);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn insert_insert_plain_text_no_snippet_mode() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        ctrl.insert("foobar", None);
        f.assert_context_keys(false, false, false);
        f.assert_selections(&[sel(1, 7, 1, 7), sel(2, 11, 2, 11)]);
    });
}

#[test]
fn insert_delete_snippet_text() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        ctrl.insert("${1:foobar}$0", None);
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 1, 1, 7), sel(2, 5, 2, 11)]);

        f.cut();
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 1, 1, 1), sel(2, 5, 2, 5)]);

        f.type_text("abc");
        f.assert_context_keys(true, false, true);

        ctrl.next();
        f.assert_context_keys(false, false, false);

        f.tab();
        f.assert_context_keys(false, false, false);

        // editor.trigger('test', 'type', { text: 'abc' });
        // assertContextKeys(contextKeys, false, false, false);
    });
}

#[test]
fn insert_nested_trivial_snippet() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        ctrl.insert("${1:foo}bar$0", None);
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 1, 1, 4), sel(2, 5, 2, 8)]);

        ctrl.insert("FOO$0", None);
        f.assert_selections(&[sel(1, 4, 1, 4), sel(2, 8, 2, 8)]);
        f.assert_context_keys(true, false, true);

        ctrl.next();
        f.assert_selections(&[sel(1, 7, 1, 7), sel(2, 11, 2, 11)]);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn insert_nested_snippet() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        ctrl.insert("${1:foobar}$0", None);
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 1, 1, 7), sel(2, 5, 2, 11)]);

        ctrl.insert("far$1boo$0", None);
        f.assert_selections(&[sel(1, 4, 1, 4), sel(2, 8, 2, 8)]);
        f.assert_context_keys(true, false, true);

        ctrl.next();
        f.assert_selections(&[sel(1, 7, 1, 7), sel(2, 11, 2, 11)]);
        f.assert_context_keys(true, true, true);

        ctrl.next();
        f.assert_selections(&[sel(1, 7, 1, 7), sel(2, 11, 2, 11)]);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn insert_nested_plain_text() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        ctrl.insert("${1:foobar}$0", None);
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 1, 1, 7), sel(2, 5, 2, 11)]);

        ctrl.insert("farboo", None);
        f.assert_selections(&[sel(1, 7, 1, 7), sel(2, 11, 2, 11)]);
        f.assert_context_keys(true, false, true);

        ctrl.next();
        f.assert_selections(&[sel(1, 7, 1, 7), sel(2, 11, 2, 11)]);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn nested_snippets_without_final_placeholder_jumps_to_next_outer_placeholder_27898() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        ctrl.insert("for(const ${1:element} of ${2:array}) {$0}", None);
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 11, 1, 18), sel(2, 15, 2, 22)]);

        ctrl.next();
        f.assert_context_keys(true, true, true);
        f.assert_selections(&[sel(1, 22, 1, 27), sel(2, 26, 2, 31)]);

        ctrl.insert("document", None);
        f.assert_context_keys(true, true, true);
        f.assert_selections(&[sel(1, 30, 1, 30), sel(2, 34, 2, 34)]);

        ctrl.next();
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn inconsistent_tab_stop_behaviour_with_recursive_snippets_and_tab_shift_tab_27543() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        ctrl.insert("1_calize(${1:nl}, '${2:value}')$0", None);

        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 10, 1, 12), sel(2, 14, 2, 16)]);

        ctrl.insert("2_calize(${1:nl}, '${2:value}')$0", None);

        f.assert_selections(&[sel(1, 19, 1, 21), sel(2, 23, 2, 25)]);

        ctrl.next(); // inner `value`
        f.assert_selections(&[sel(1, 24, 1, 29), sel(2, 28, 2, 33)]);

        ctrl.next(); // inner `$0`
        f.assert_selections(&[sel(1, 31, 1, 31), sel(2, 35, 2, 35)]);

        ctrl.next(); // outer `value`
        f.assert_selections(&[sel(1, 34, 1, 39), sel(2, 38, 2, 43)]);

        ctrl.prev(); // inner `$0`
        f.assert_selections(&[sel(1, 31, 1, 31), sel(2, 35, 2, 35)]);
    });
}

#[test]
fn snippet_tabstop_selecting_content_of_previously_entered_variable_only_works_when_separated_by_space_23728() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        ctrl.insert("import ${2:${1:module}} from '${1:module}'$0", None);

        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 8, 1, 14), sel(1, 21, 1, 27)]);

        ctrl.insert("foo", None);
        f.assert_selections(&[sel(1, 11, 1, 11), sel(1, 21, 1, 21)]);

        ctrl.next(); // ${2:...}
        f.assert_selections(&[sel(1, 8, 1, 11)]);
    });
}

#[test]
fn html_snippets_combine_32211() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        f.set_value("");
        f.model.update_options(TextModelUpdateOptions {
            insert_spaces: Some(false),
            tab_size: Some(4),
            trim_auto_whitespace: Some(false),
            ..Default::default()
        });
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        ctrl.insert(
            "\n\t\t\t<!DOCTYPE html>\n\t\t\t<html lang=\"en\">\n\t\t\t<head>\n\t\t\t\t<meta charset=\"UTF-8\">\n\t\t\t\t<meta name=\"viewport\" content=\"width=${2:device-width}, initial-scale=${3:1.0}\">\n\t\t\t\t<meta http-equiv=\"X-UA-Compatible\" content=\"${5:ie=edge}\">\n\t\t\t\t<title>${7:Document}</title>\n\t\t\t</head>\n\t\t\t<body>\n\t\t\t\t${8}\n\t\t\t</body>\n\t\t\t</html>\n\t\t",
            None,
        );
        ctrl.next();
        ctrl.next();
        ctrl.next();
        ctrl.next();
        f.assert_selections(&[sel(11, 5, 11, 5)]);

        ctrl.insert("<input type=\"${2:text}\">", None);
        f.assert_selections(&[sel(11, 18, 11, 22)]);
    });
}

#[test]
fn problems_with_nested_snippet_insertion_39594() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        ctrl.insert("$1 = ConvertTo-Json $1", None);
        f.assert_selections(&[sel(1, 1, 1, 1), sel(1, 19, 1, 19)]);

        f.editor.set_selection(sel(1, 19, 1, 19).into(), None);

        // snippet mode should stop because $1 has two occurrences
        // and we only have one selection left
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn problems_with_nested_snippet_insertion_39594_part2() {
    with_fixture(|f| {
        // ensure selection-change-to-cancel logic isn't too aggressive
        let ctrl = f.create_controller();

        f.set_value("a-\naaa-");
        f.editor.set_selections(&[sel(2, 5, 2, 5), sel(1, 3, 1, 3)], None, None);

        ctrl.insert("log($1);$0", None);
        f.assert_selections(&[sel(2, 9, 2, 9), sel(1, 7, 1, 7)]);
        f.assert_context_keys(true, false, true);
    });
}

#[test]
fn nested_snippets_terminating_abruptly_in_vscode_1_19_2_42012() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        ctrl.insert("var ${2:${1:name}} = ${1:name} + 1;${0}", None);

        f.assert_selections(&[sel(1, 5, 1, 9), sel(1, 12, 1, 16)]);
        f.assert_context_keys(true, false, true);

        ctrl.next();
        f.assert_context_keys(true, true, true);
    });
}

#[test]
fn placeholders_order_58267() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        ctrl.insert("\\pth{$1}$0", None);

        f.assert_selections(&[sel(1, 6, 1, 6)]);
        f.assert_context_keys(true, false, true);

        ctrl.insert("\\itv{${1:left}}{${2:right}}{${3:left_value}}{${4:right_value}}$0", None);
        f.assert_selections(&[sel(1, 11, 1, 15)]);

        ctrl.next();
        f.assert_selections(&[sel(1, 17, 1, 22)]);

        ctrl.next();
        f.assert_selections(&[sel(1, 24, 1, 34)]);

        ctrl.next();
        f.assert_selections(&[sel(1, 36, 1, 47)]);

        ctrl.next();
        f.assert_selections(&[sel(1, 48, 1, 48)]);

        ctrl.next();
        f.assert_selections(&[sel(1, 49, 1, 49)]);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn must_tab_through_deleted_tab_stops_in_snippets_31619() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);
        ctrl.insert("foo${1:a${2:bar}baz}end$0", None);
        f.assert_selections(&[sel(1, 4, 1, 11)]);

        f.cut();
        f.assert_selections(&[sel(1, 4, 1, 4)]);

        ctrl.next();
        f.assert_selections(&[sel(1, 7, 1, 7)]);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn cancelling_snippet_mode_should_discard_added_cursors_68512_soft_cancel() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        ctrl.insert(
            ".REGION ${2:FUNCTION_NAME}\nCREATE.FUNCTION ${1:VOID} ${2:FUNCTION_NAME}(${3:})\n\t${4:}\nEND\n.ENDREGION$0",
            None,
        );
        f.assert_selections(&[sel(2, 17, 2, 21)]);

        ctrl.next();
        f.assert_selections(&[sel(1, 9, 1, 22), sel(2, 22, 2, 35)]);
        f.assert_context_keys(true, true, true);

        f.editor.set_selections(&[sel(1, 22, 1, 22), sel(2, 35, 2, 35)], None, None);
        f.assert_context_keys(true, true, true);

        f.editor.set_selections(&[sel(2, 1, 2, 1), sel(2, 36, 2, 36)], None, None);
        f.assert_context_keys(false, false, false);
        f.assert_selections(&[sel(2, 1, 2, 1), sel(2, 36, 2, 36)]);
    });
}

#[test]
fn cancelling_snippet_mode_should_discard_added_cursors_68512_hard_cancel() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        ctrl.insert(
            ".REGION ${2:FUNCTION_NAME}\nCREATE.FUNCTION ${1:VOID} ${2:FUNCTION_NAME}(${3:})\n\t${4:}\nEND\n.ENDREGION$0",
            None,
        );
        f.assert_selections(&[sel(2, 17, 2, 21)]);

        ctrl.next();
        f.assert_selections(&[sel(1, 9, 1, 22), sel(2, 22, 2, 35)]);
        f.assert_context_keys(true, true, true);

        f.editor.set_selections(&[sel(1, 22, 1, 22), sel(2, 35, 2, 35)], None, None);
        f.assert_context_keys(true, true, true);

        ctrl.cancel(true);
        f.assert_context_keys(false, false, false);
        f.assert_selections(&[sel(1, 22, 1, 22)]);
    });
}

#[test]
fn user_defined_snippet_tab_stops_ignored_72862() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        ctrl.insert("export default $1", None);
        f.assert_context_keys(true, false, true);
    });
}

#[test]
fn optional_tabstop_in_snippets_72358() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        ctrl.insert("${1:prop: {$2\\},}\nmore$0", None);
        f.assert_context_keys(true, false, true);

        f.assert_selections(&[sel(1, 1, 1, 10)]);
        f.cut();

        f.assert_selections(&[sel(1, 1, 1, 1)]);

        ctrl.next();
        f.assert_selections(&[sel(2, 5, 2, 5)]);
        f.assert_context_keys(false, false, false);
    });
}

#[test]
fn issue_90135_confusing_trim_whitespace_edits() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.tab();

        ctrl.insert("\nfoo", None);
        f.assert_selections(&[sel(2, 8, 2, 8)]);
    });
}

#[test]
fn issue_145727_insert_snippet_can_put_snippet_selections_in_wrong_positions_1_of_2() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.tab();

        ctrl.insert("\naProperty: aClass<${2:boolean}> = new aClass<${2:boolean}>();\n", adjust_whitespace(false));
        f.assert_selections(&[sel(2, 19, 2, 26), sel(2, 41, 2, 48)]);
    });
}

#[test]
fn issue_145727_insert_snippet_can_put_snippet_selections_in_wrong_positions_2_of_2() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.tab();

        ctrl.insert("\naProperty: aClass<${2:boolean}> = new aClass<${2:boolean}>();\n", None);
        // This will insert \n    aProperty....
        f.assert_selections(&[sel(2, 23, 2, 30), sel(2, 45, 2, 52)]);
    });
}

#[test]
fn leading_tab_by_snippets_wont_replace_by_spaces_101870() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.model.update_options(TextModelUpdateOptions {
            insert_spaces: Some(true),
            tab_size: Some(4),
            ..Default::default()
        });
        ctrl.insert("\tHello World\n\tNew Line", None);
        f.assert_value("    Hello World\n    New Line");
    });
}

#[test]
fn leading_tab_by_snippets_wont_replace_by_spaces_101870_part_2() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.model.update_options(TextModelUpdateOptions {
            insert_spaces: Some(true),
            tab_size: Some(4),
            ..Default::default()
        });
        ctrl.insert("\tHello World\n\tNew Line\n${1:\tmore}", None);
        f.assert_value("    Hello World\n    New Line\n    more");
    });
}

#[test]
#[ignore = "test.skip no original"]
fn snippet_transformation_does_not_work_after_inserting_variable_using_intellisense_112362() {
    with_fixture(|f| {
        {
            // HAPPY - no nested snippet
            let ctrl = f.create_controller();
            f.set_value("");
            f.model.update_options(TextModelUpdateOptions {
                insert_spaces: Some(true),
                tab_size: Some(4),
                ..Default::default()
            });
            ctrl.insert("$1\n\n${1/([A-Za-z0-9]+): ([A-Za-z]+).*/$1: '$2',/gm}", None);

            f.assert_selections(&[sel(1, 1, 1, 1), sel(3, 1, 3, 1)]);
            f.type_text("foo: number;");
            ctrl.next();
            f.assert_value("foo: number;\n\nfoo: 'number',");
        }

        let ctrl = f.create_controller();
        f.set_value("");
        f.model.update_options(TextModelUpdateOptions {
            insert_spaces: Some(true),
            tab_size: Some(4),
            ..Default::default()
        });
        ctrl.insert("$1\n\n${1/([A-Za-z0-9]+): ([A-Za-z]+).*/$1: '$2',/gm}", None);

        f.assert_selections(&[sel(1, 1, 1, 1), sel(3, 1, 3, 1)]);
        f.type_text("foo: ");
        ctrl.insert("number;", None);
        ctrl.next();
        f.assert_value("foo: number;\n\nfoo: 'number',");
        // editor.trigger('test', 'type', { text: ';' });
    });
}

// #region createEditsAndSnippetsFromEdits

#[test]
fn create_edits_and_snippets_from_edits_apply_tab_done() {
    with_fixture(|f| {
        let ctrl = f.create_controller();

        f.set_value("foo(\"bar\")");

        ctrl.apply(
            vec![
                edit(Range::new(1, 5, 1, 10), "$1"),
                edit(Range::new(1, 1, 1, 1), "const ${1:new_const} = \"bar\";\n"),
            ],
            None,
        );

        f.assert_value("const new_const = \"bar\";\nfoo(new_const)");
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 7, 1, 16), sel(2, 5, 2, 14)]);

        ctrl.next();
        f.assert_context_keys(false, false, false);
        f.assert_selections(&[sel(2, 14, 2, 14)]);
    });
}

#[test]
fn create_edits_and_snippets_from_edits_apply_tab_done_with_special_final_tabstop() {
    with_fixture(|f| {
        f.set_value("foo(\"bar\")");

        let ctrl = f.create_controller();
        ctrl.apply(
            vec![
                edit(Range::new(1, 5, 1, 10), "$1"),
                edit(Range::new(1, 1, 1, 1), "const ${1:new_const}$0 = \"bar\";\n"),
            ],
            None,
        );

        f.assert_value("const new_const = \"bar\";\nfoo(new_const)");
        f.assert_context_keys(true, false, true);
        f.assert_selections(&[sel(1, 7, 1, 16), sel(2, 5, 2, 14)]);

        ctrl.next();
        f.assert_context_keys(false, false, false);
        f.assert_selections(&[sel(1, 16, 1, 16)]);
    });
}

#[test]
fn create_edits_and_snippets_from_edits_apply_tab_tab_done() {
    with_fixture(|f| {
        f.set_value("foo\nbar");

        let ctrl = f.create_controller();
        ctrl.apply(
            vec![
                edit(Range::new(1, 4, 1, 4), "${3}"),
                edit(Range::new(2, 4, 2, 4), "$3"),
                edit(Range::new(1, 1, 1, 1), "### ${2:Header}\n"),
            ],
            None,
        );

        f.assert_value("### Header\nfoo\nbar");
        assert_eq!(f.context_state(), ContextState { in_snippet: true, has_prev: false, has_next: true });
        f.assert_selections(&[sel(1, 5, 1, 11)]);

        ctrl.next();
        assert_eq!(f.context_state(), ContextState { in_snippet: true, has_prev: true, has_next: true });
        f.assert_selections(&[sel(2, 4, 2, 4), sel(3, 4, 3, 4)]);

        ctrl.next();
        assert_eq!(f.context_state(), ContextState { in_snippet: false, has_prev: false, has_next: false });
        f.assert_selections(&[sel(3, 4, 3, 4)]);
    });
}

#[test]
fn create_edits_and_snippets_from_edits_nested_into_apply_works() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("onetwo");

        f.editor.set_selections(&[sel(1, 1, 1, 1), sel(2, 1, 2, 1)], None, None);

        ctrl.apply(vec![edit(Range::new(1, 7, 1, 7), "$0${1:three}")], None);

        f.assert_value("onetwothree");
        assert_eq!(f.context_state(), ContextState { in_snippet: true, has_prev: false, has_next: true });
        f.assert_selections(&[sel(1, 7, 1, 12)]);

        ctrl.insert("foo$1bar$1", None);
        f.assert_value("onetwofoobar");
        f.assert_selections(&[sel(1, 10, 1, 10), sel(1, 13, 1, 13)]);
        assert_eq!(f.context_state(), ContextState { in_snippet: true, has_prev: false, has_next: true });

        ctrl.next();
        assert_eq!(f.context_state(), ContextState { in_snippet: true, has_prev: true, has_next: true });
        f.assert_selections(&[sel(1, 13, 1, 13)]);

        ctrl.next();
        assert_eq!(f.context_state(), ContextState { in_snippet: false, has_prev: false, has_next: false });
        f.assert_selections(&[sel(1, 7, 1, 7)]);
    });
}

#[test]
fn create_edits_and_snippets_from_edits_nested_into_insert_abort_outer_snippet() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("one\ntwo");

        f.editor.set_selections(&[sel(1, 1, 1, 1), sel(2, 1, 2, 1)], None, None);

        ctrl.insert("foo${1:bar}bazz${1:bang}", None);
        f.assert_selections(&[sel(1, 4, 1, 7), sel(1, 11, 1, 14), sel(2, 4, 2, 7), sel(2, 11, 2, 14)]);
        assert_eq!(f.context_state(), ContextState { in_snippet: true, has_prev: false, has_next: true });

        ctrl.apply(vec![edit(Range::new(1, 4, 1, 7), "$0A")], None);

        f.assert_value("fooAbazzbarone\nfoobarbazzbartwo");
        assert_eq!(f.context_state(), ContextState { in_snippet: false, has_prev: false, has_next: false });
        f.assert_selections(&[sel(1, 4, 1, 4)]);
    });
}

#[test]
fn create_edits_and_snippets_from_edits_nested_into_insert_abort_outer_snippet_2() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("one\ntwo");

        f.editor.set_selections(&[sel(1, 1, 1, 1), sel(2, 1, 2, 1)], None, None);

        ctrl.insert("foo${1:bar}bazz${1:bang}", None);
        f.assert_selections(&[sel(1, 4, 1, 7), sel(1, 11, 1, 14), sel(2, 4, 2, 7), sel(2, 11, 2, 14)]);
        assert_eq!(f.context_state(), ContextState { in_snippet: true, has_prev: false, has_next: true });

        let edits = vec![
            edit(Range::new(1, 4, 1, 7), "A"),
            edit(Range::new(1, 11, 1, 14), "B"),
            edit(Range::new(2, 4, 2, 7), "C"),
            edit(Range::new(2, 11, 2, 14), "D"),
        ];
        ctrl.apply(edits, None);

        f.assert_value("fooAbazzBone\nfooCbazzDtwo");
        assert_eq!(f.context_state(), ContextState { in_snippet: false, has_prev: false, has_next: false });
        f.assert_selections(&[sel(1, 5, 1, 5), sel(1, 10, 1, 10), sel(2, 5, 2, 5), sel(2, 10, 2, 10)]);
    });
}

// #endregion

#[test]
fn bug_cursor_position_0_with_user_snippets_163808() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");

        ctrl.insert("<Element1 Attr1=\"foo\" $1>\n  <Element2 Attr1=\"$2\"/>\n$0\"\n</Element1>", None);
        f.assert_selections(&[sel(1, 23, 1, 23)]);

        ctrl.insert("Qualifier=\"$0\"", None);
        f.assert_value("<Element1 Attr1=\"foo\" Qualifier=\"\">\n  <Element2 Attr1=\"\"/>\n\"\n</Element1>");
        f.assert_selections(&[sel(1, 34, 1, 34)]);
    });
}

#[test]
fn eol_sequence_crlf_shifts_tab_stop_in_is_file_template_snippets_167386() {
    with_fixture(|f| {
        let ctrl = f.create_controller();
        f.set_value("");
        f.model.set_eol(EndOfLineSequence::CrLf);

        ctrl.apply(
            vec![edit(
                f.model.get_full_model_range(),
                "line 54321${1:FOO}\nline 54321${1:FOO}\n(no tab stop)\nline 54321${1:FOO}\nline 54321",
            )],
            None,
        );

        f.assert_selections(&[sel(1, 11, 1, 14), sel(2, 11, 2, 14), sel(4, 11, 4, 14)]);
    });
}

#[test]
fn surround_with_code_action_snippets_use_incorrect_indentation_levels_and_styles_169319() {
    with_fixture(|f| {
        f.set_value("function foo(f, x, condition) {\n    f();\n    return x;\n}");
        let range = Range::new(2, 5, 3, 14);
        f.editor.set_selection(range.into(), None);
        let ctrl = f.create_controller();
        ctrl.apply(vec![edit(range, "if (${1:condition}) {\n\t$TM_SELECTED_TEXT$0\n}")], None);

        f.assert_value("function foo(f, x, condition) {\n    if (condition) {\n        f();\n        return x;\n    }\n}");
    });
}

#[test]
fn tm_selected_text_resolves_per_edit_not_the_last_selection_multi_cursor_wrap_snippet_206121() {
    with_fixture(|f| {
        f.set_value("aaa\nbbb\nccc");
        let ranges = [Range::new(1, 1, 1, 4), Range::new(2, 1, 2, 4), Range::new(3, 1, 3, 4)];
        let selections: Vec<Selection> = ranges
            .iter()
            .map(|r| sel(r.start_line_number, r.start_column, r.end_line_number, r.end_column))
            .collect();
        f.editor.set_selections(&selections, None, None);

        let ctrl = f.create_controller();
        ctrl.apply(ranges.iter().map(|range| edit(*range, "if(${1:cond}) {$TM_SELECTED_TEXT}$0")).collect(), None);

        f.assert_value("if(cond) {aaa}\nif(cond) {bbb}\nif(cond) {ccc}");
    });
}

#[test]
fn apply_with_multiple_cursors_threads_cursor_number_per_edit() {
    with_fixture(|f| {
        f.set_value("aa\nbb\ncc");

        let ctrl = f.create_controller();
        ctrl.apply(
            vec![
                edit(Range::new(1, 3, 1, 3), ":$CURSOR_NUMBER"),
                edit(Range::new(2, 3, 2, 3), ":$CURSOR_NUMBER"),
                edit(Range::new(3, 3, 3, 3), ":$CURSOR_NUMBER"),
            ],
            None,
        );

        f.assert_value("aa:1\nbb:2\ncc:3");
    });
}

#[test]
fn undo_restores_original_selection_after_apply_regression_for_170041() {
    with_fixture(|f| {
        f.set_value("Some text and more text");
        f.editor.set_selection(sel(1, 1, 1, 1).into(), None);

        let ctrl = f.create_controller();
        ctrl.apply(vec![edit(Range::new(1, 6, 1, 10), "${0:hi}")], None);

        f.assert_value("Some hi and more text");

        f.undo();

        f.assert_value("Some text and more text");
        f.assert_selections(&[sel(1, 1, 1, 1)]);
    });
}
