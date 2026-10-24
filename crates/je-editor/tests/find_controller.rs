//! Porte de `src/vs/editor/contrib/find/test/browser/findController.test.ts` (suítes `FindController` e `FindController
//! query options persistence`), caso a caso e na mesma ordem. Os três casos comentados do original (`/* ... */`, só
//! valem no macOS) não são casos.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso ([`run_test`]), que também
//!   monta os serviços da suíte (`serviceCollection`: o `IStorageService`, o `IContextKeyService` e o
//!   `IClipboardService`) e os descarta no fim.
//! - O `InMemoryStorageService` do original é compartilhado por todos os casos de cada suíte, e as opções de consulta
//!   guardadas (`editor.isRegex`, `editor.matchCase`, `editor.wholeWord`) passam de um caso para o seguinte. Os casos do
//!   Rust rodam em threads separadas, então cada um nasce com o estado que o original teria ao chegar nele
//!   ([`StoredOptions`]): na suíte `FindController` só `editor.isRegex` muda (`#6149` o liga, `#41027` o desliga com o
//!   `toggleRegex`, `#18111` o liga de novo, e os seguintes herdam `true`); na suíte de persistência os valores iniciais
//!   são `isRegex` falso, `matchCase` falso e `wholeWord` verdadeiro (as `store` do corpo da suíte rodam antes dos casos),
//!   e o caso `matchCase` deixa `matchCase` verdadeiro e o `toggling options is saved` deixa `isRegex` verdadeiro.
//! - `TestFindController extends CommonFindController` é [`create_test_find_controller`]: o `CommonFindController` real
//!   com o `_start` sobrescrito ([`CommonFindController::set_start_override`]), que chama o `super._start(opts)` (sem o
//!   `newState`, como o original) e depois marca `hasFocus` e a chave `findInputFocussed`. `hasFocus` é o `Cell` de
//!   [`TestFindController`]. O `_updateHistoryDelayer = new Delayer<void>(50)` é [`CommonFindController::set_update_history_delayer`].
//! - `editor.registerAndInstantiateContribution(TestFindController.ID, TestFindController)` é
//!   `register_and_instantiate_contribution` com o construtor de [`create_test_find_controller`].
//! - `editor.runAction(action)` é `run_action` com o [`ActionAdapter`] (o papel de `ITestEditorAction`: o
//!   `action.run(accessor, editor, args)` do original). `executeAction(instantiationService, editor, action, args)` é
//!   [`execute_action`] (o `runEditorCommand`, que entrega `args || {}`).
//! - `withAsyncTestCodeEditor(..., async (editor) => {...})` é `with_test_code_editor` com as microtarefas esvaziadas
//!   ([`settle`]) em cada `await`. O que o original faz sem `await` (os casos `#109756` e `#27083`) roda sem o `settle`.
//! - `new Selection(...)` no `searchScope` vira o `Range` equivalente (`Selection extends Range` e o estado guarda
//!   `Range[]`). `findState.searchString` é `JsString`, comparado com texto.
//! - `Delayer`, `clipboardState` e o `platform.isMacintosh` (as `serviceCollection.set(IClipboardService, ...)`) não valem
//!   fora do macOS: o `IClipboardService` é um `TestClipboardService` simples, sem efeito porque `globalFindClipboard` é
//!   falso.
//! - `findState.searchString.split(/\r\n|\r|\n/g)` é a troca de `\r\n` por `\n` e a divisão em `\n` e `\r`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::errors::{Error, throw};
use je_base::async_::Delayer;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_base::symbols::Delay;
use je_editor::code_editor_widget::{CodeEditorWidget, ExecuteEditsSource, ICodeEditor};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::EditOperation;
use je_editor::editor_extensions::EditorAction;
use je_editor::find::find_controller::{
    CommonFindController, CommonFindControllerServices, FindStartFocusAction, IClipboardService, IFindStartOptions, I_CLIPBOARD_SERVICE,
    SeedSearchStringMode, SelectionMatchFindAction, StartFindWithSelectionAction, next_match_find_action, start_find_action,
    start_find_replace_action,
};
use je_editor::find::find_model::context_find_input_focused;
use je_editor::find::find_state::INewFindReplaceState;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_instantiation::{ServiceCollection, ServicesAccessor};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKey, IContextKeyService};
use je_platform::storage::{AbstractStorageService, I_STORAGE_SERVICE, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use support::test_code_editor::{
    CodeEditorServices, ITestEditorAction, TestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor,
};

/// O `IClipboardService` da suíte: o objeto `{ readFindText, writeFindText } as any` do original. Os demais membros não
/// existem nele, e chamá-los lança, como chamar um membro ausente no JavaScript.
struct TestClipboardService {
    state: RefCell<JsString>,
}

impl IClipboardService for TestClipboardService {
    fn read_find_text(&self) -> Promise<JsString> {
        Promise::resolved(self.state.borrow().clone())
    }

    fn write_find_text(&self, text: &JsString) -> Promise<()> {
        *self.state.borrow_mut() = text.clone();
        Promise::resolved(())
    }

    fn write_text(&self, _text: &str) -> Promise<()> {
        throw(Error::new("clipboardService.writeText is not a function"))
    }

    fn read_text(&self) -> Promise<String> {
        throw(Error::new("clipboardService.readText is not a function"))
    }

    fn write_resources(&self, _resources: Vec<URI>) -> Promise<()> {
        throw(Error::new("clipboardService.writeResources is not a function"))
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        throw(Error::new("clipboardService.readResources is not a function"))
    }

    fn has_resources(&self) -> Promise<bool> {
        throw(Error::new("clipboardService.hasResources is not a function"))
    }
}

/// As opções de consulta guardadas no `InMemoryStorageService` quando o caso começa (`None` é uma chave ausente).
#[derive(Clone, Copy, Default)]
struct StoredOptions {
    is_regex: Option<bool>,
    match_case: Option<bool>,
    whole_word: Option<bool>,
}

/// Os `const` da suíte: os serviços que o `serviceCollection` entrega ao editor.
struct Suite {
    disposables: Rc<DisposableStore>,
    storage_service: Rc<AbstractStorageService>,
    context_key_service: Rc<ContextKeyService>,
    clipboard_service: Rc<TestClipboardService>,
}

impl Suite {
    /// O `serviceCollection` do caso, com os serviços da suíte.
    fn service_collection(&self) -> ServiceCollection {
        let storage: Rc<dyn StorageService> = Rc::new(self.storage_service.clone());
        let context_key_service: Rc<dyn IContextKeyService> = self.context_key_service.clone();
        let clipboard: Rc<dyn IClipboardService> = self.clipboard_service.clone();
        ServiceCollection::new()
            .with(I_STORAGE_SERVICE, storage)
            .with(I_CONTEXT_KEY_SERVICE, context_key_service)
            .with(I_CLIPBOARD_SERVICE, clipboard)
    }

    /// As opções de instanciação do editor de teste: `{ serviceCollection, find: {...} }`.
    fn options(&self, find: Vec<(&str, Dynamic)>) -> TestCodeEditorInstantiationOptions {
        let entries: Vec<(&str, Dynamic)> = if find.is_empty() { Vec::new() } else { vec![("find", Dynamic::object(find))] };
        TestCodeEditorInstantiationOptions { options: Dynamic::object(entries), service_collection: Some(self.service_collection()) }
    }
}

/// O que o mocha faz em volta de cada `test(...)`: o `ensureNoDisposablesAreLeakedInTestSuite()`, os serviços da suíte
/// (com o estado do `InMemoryStorageService` que o original teria ao chegar no caso), o corpo e o descarte.
fn run_test(stored: StoredOptions, body: impl FnOnce(&Suite)) {
    // O carregamento dos módulos `coreCommands` e `findController` (que registram comandos e ações) acontece antes do
    // guarda, como o `import` do original.
    je_editor::core_commands::register_core_commands();
    je_editor::find::find_controller::start_find_action();
    je_editor::find::find_controller::next_match_find_action();
    je_editor::find::find_controller::previous_match_find_action();
    je_editor::find::find_controller::start_find_replace_action();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let storage_service = disposables.add_rc(InMemoryStorageService::new());
    for (key, value) in [("editor.isRegex", stored.is_regex), ("editor.matchCase", stored.match_case), ("editor.wholeWord", stored.whole_word)] {
        if let Some(value) = value {
            StorageService::store(&storage_service, key, value.into(), StorageScope::Workspace, StorageTarget::User);
        }
    }
    let context_key_service = disposables.add_rc(ContextKeyService::new(TestConfigurationService::new(None)));
    let suite = Suite {
        disposables,
        storage_service,
        context_key_service,
        clipboard_service: Rc::new(TestClipboardService { state: RefCell::new(JsString::new()) }),
    };
    body(&suite);
    suite.disposables.dispose();
}

/// `TestFindController`: o controlador comum com o `_start` do `TestFindController` instalado.
struct TestFindController {
    controller: Rc<CommonFindController>,
    /// `hasFocus`.
    has_focus: Rc<Cell<bool>>,
}

impl Deref for TestFindController {
    type Target = CommonFindController;

    fn deref(&self) -> &CommonFindController {
        &self.controller
    }
}

/// `editor.registerAndInstantiateContribution(TestFindController.ID, TestFindController)`.
fn create_test_find_controller(editor: &TestCodeEditor) -> TestFindController {
    let has_focus = Rc::new(Cell::new(false));
    let controller = editor.register_and_instantiate_contribution(CommonFindController::ID, |widget, instantiation_service| {
        let (services, context_key_service) = instantiation_service.invoke_function(|accessor| {
            (CommonFindControllerServices::from_accessor(accessor, Rc::new(|_message: &str| {})), accessor.get(I_CONTEXT_KEY_SERVICE))
        });
        let controller = CommonFindController::new(widget, services);
        let find_input_focused = context_find_input_focused().bind_to(&*context_key_service);
        controller.set_update_history_delayer(Rc::new(Delayer::new(Delay::Millis(50))));

        let has_focus = has_focus.clone();
        controller.set_start_override(Rc::new(
            move |controller: &Rc<CommonFindController>, opts: IFindStartOptions, _new_state: Option<INewFindReplaceState>| {
                let (has_focus, find_input_focused) = (has_focus.clone(), find_input_focused.clone());
                controller.start_base(opts, None).then(move |()| {
                    if opts.should_focus != FindStartFocusAction::NoFocusChange {
                        has_focus.set(true);
                    }

                    let input_focused = opts.should_focus == FindStartFocusAction::FocusFindInput;
                    find_input_focused.set(Dynamic::Bool(input_focused));
                })
            },
        ));
        controller
    });
    TestFindController { controller, has_focus }
}

fn from_selection(slc: &Selection) -> [i32; 4] {
    [slc.start_line_number, slc.start_column, slc.end_line_number, slc.end_column]
}

/// O `await`: esvazia as microtarefas e confere que a promessa assentou sem erro.
fn settle_promise(promise: &Promise<()>) {
    runtime::run_microtasks();
    match promise.result() {
        Some(Ok(())) => {}
        Some(Err(error)) => panic!("a promessa foi rejeitada: {error:?}"),
        None => panic!("a promessa não assentou"),
    }
}

/// O `await` do que `editor.runAction(...)` devolve.
fn settle(result: MaybePromise<()>) {
    match result {
        MaybePromise::Promise(promise) => settle_promise(&promise),
        MaybePromise::Value(()) => runtime::run_microtasks(),
    }
}

/// `ITestEditorAction` de uma `EditorAction`: o `action.run(accessor, editor, args)` do `runAction`.
struct ActionAdapter<'a>(&'a dyn EditorAction);

impl ITestEditorAction for ActionAdapter<'_> {
    fn run(&self, accessor: &ServicesAccessor, editor: &Rc<CodeEditorWidget>, args: Option<&Dynamic>) -> MaybePromise<()> {
        let args = args.cloned().unwrap_or(Dynamic::Undefined);
        MaybePromise::Promise(self.0.run(Some(accessor), editor, &args))
    }
}

/// `editor.runAction(action)`.
fn run_action(editor: &TestCodeEditor, action: &dyn EditorAction) -> MaybePromise<()> {
    editor.run_action(&ActionAdapter(action), None)
}

/// `executeAction(instantiationService, editor, action, args)`: o `runEditorCommand` da ação, que entrega `args || {}`.
fn execute_action(services: &Rc<CodeEditorServices>, editor: &TestCodeEditor, action: &dyn EditorAction) -> Promise<()> {
    let args = Dynamic::object(std::iter::empty::<(&str, Dynamic)>());
    services.invoke_function(|accessor| action.run(Some(accessor), editor.widget(), &args))
}

/// `findController.start({...})` com as opções que os casos repetem: sem semente, sem escopo e com `loop`.
fn start_options(should_focus: FindStartFocusAction, update_search_scope: bool) -> IFindStartOptions {
    IFindStartOptions {
        force_reveal_replace: false,
        seed_search_string_from_selection: SeedSearchStringMode::None,
        seed_search_string_from_non_empty_selection: false,
        seed_search_string_from_global_clipboard: false,
        should_focus,
        should_animate: false,
        update_search_scope,
        r#loop: true,
    }
}

fn search_change(search_string: &str) -> INewFindReplaceState {
    INewFindReplaceState { search_string: Some(JsString::from(search_string)), ..INewFindReplaceState::default() }
}

fn text_lines<'a>(lines: &'a [&'a str]) -> &'a [&'a str] {
    lines
}

// #region Suíte `FindController`

#[test]
fn issue_1857_f3_find_next_acts_like_find_under_cursor() {
    run_test(StoredOptions::default(), |suite| {
        with_test_code_editor(text_lines(&["ABC", "ABC", "XYZ", "ABC"]), suite.options(vec![]), |editor, _, services| {
            // The cursor is at the very top, of the file, at the first ABC
            let find_controller = create_test_find_controller(editor);
            let find_state = find_controller.get_state();
            let next_match_find_action = next_match_find_action();

            // I hit Ctrl+F to show the Find dialog
            settle_promise(&execute_action(services, editor, &*start_find_action()));

            // I type ABC.
            find_state.change(search_change("A"), true, None);
            find_state.change(search_change("AB"), true, None);
            find_state.change(search_change("ABC"), true, None);

            // The first ABC is highlighted.
            assert_eq!(from_selection(&editor.get_selection().unwrap()), [1, 1, 1, 4]);

            // I hit Esc to exit the Find dialog.
            find_controller.close_find_widget();
            find_controller.has_focus.set(false);

            // The cursor is now at end of the first line, with ABC on that line highlighted.
            assert_eq!(from_selection(&editor.get_selection().unwrap()), [1, 1, 1, 4]);

            // I hit delete to remove it and change the text to XYZ.
            editor.push_undo_stop();
            editor.execute_edits(ExecuteEditsSource::Name(Some("test")), vec![EditOperation::delete(Range::new(1, 1, 1, 4))], None);
            editor.execute_edits(ExecuteEditsSource::Name(Some("test")), vec![EditOperation::insert(Position::new(1, 1), "XYZ")], None);
            editor.push_undo_stop();

            // At this point the text editor looks like this:
            //   XYZ
            //   ABC
            //   XYZ
            //   ABC
            assert_eq!(editor.get_model().unwrap().get_line_content(1), "XYZ");

            // The cursor is at end of the first line.
            assert_eq!(from_selection(&editor.get_selection().unwrap()), [1, 4, 1, 4]);

            // I hit F3 to "Find Next" to find the next occurrence of ABC, but instead it searches for XYZ.
            settle(run_action(editor, &*next_match_find_action));

            assert_eq!(find_state.search_string(), "ABC");
            assert!(!find_controller.has_focus.get());

            find_controller.dispose();
        });
    });
}

#[test]
fn issue_3090_f3_does_not_loop_with_two_matches_on_a_single_line() {
    run_test(StoredOptions::default(), |suite| {
        with_test_code_editor(text_lines(&["import nls = require('vs/nls');"]), suite.options(vec![]), |editor, _, _| {
            let find_controller = create_test_find_controller(editor);
            let next_match_find_action = next_match_find_action();

            editor.set_position(Position::new(1, 9), None);

            settle(run_action(editor, &*next_match_find_action));
            assert_eq!(from_selection(&editor.get_selection().unwrap()), [1, 26, 1, 29]);

            settle(run_action(editor, &*next_match_find_action));
            assert_eq!(from_selection(&editor.get_selection().unwrap()), [1, 8, 1, 11]);

            find_controller.dispose();
        });
    });
}

#[test]
fn issue_6149_auto_escape_highlighted_text_for_search_and_replace_regex_mode() {
    run_test(StoredOptions::default(), |suite| {
        with_test_code_editor(
            text_lines(&["var x = (3 * 5)", "var y = (3 * 5)", "var z = (3  * 5)"]),
            suite.options(vec![]),
            |editor, _, services| {
                let find_controller = create_test_find_controller(editor);
                let next_match_find_action = next_match_find_action();

                editor.set_selection(Selection::new(1, 9, 1, 13).into(), None);

                find_controller.toggle_regex();
                settle_promise(&execute_action(services, editor, &*start_find_action()));

                settle(run_action(editor, &*next_match_find_action));
                assert_eq!(from_selection(&editor.get_selection().unwrap()), [2, 9, 2, 13]);

                settle(run_action(editor, &*next_match_find_action));
                assert_eq!(from_selection(&editor.get_selection().unwrap()), [1, 9, 1, 13]);

                find_controller.dispose();
            },
        );
    });
}

#[test]
fn issue_41027_dont_replace_find_input_value_on_replace_action_if_find_input_is_active() {
    // O `#6149` deixou `editor.isRegex` verdadeiro no armazenamento compartilhado, e o `toggleRegex` daqui o desliga.
    run_test(StoredOptions { is_regex: Some(true), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(text_lines(&["test"]), suite.options(vec![]), |editor, _, services| {
            let test_regex_string = "tes.";
            let find_controller = create_test_find_controller(editor);
            let next_match_find_action = next_match_find_action();

            find_controller.toggle_regex();
            find_controller.set_search_string(JsString::from(test_regex_string));
            settle_promise(&find_controller.controller.start(start_options(FindStartFocusAction::FocusFindInput, false), None));
            settle(run_action(editor, &*next_match_find_action));
            settle_promise(&execute_action(services, editor, &*start_find_replace_action()));

            assert_eq!(find_controller.get_state().search_string(), test_regex_string);

            find_controller.dispose();
        });
    });
}

#[test]
fn editor_find_close_on_result_closes_find_widget_when_a_match_is_found_from_explicit_navigation() {
    run_test(StoredOptions { is_regex: Some(false), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(
            text_lines(&["ABC", "ABC", "XYZ"]),
            suite.options(vec![("closeOnResult", Dynamic::Bool(true))]),
            |editor, _, services| {
                let find_controller = create_test_find_controller(editor);
                let find_state = find_controller.get_state();

                settle_promise(&execute_action(services, editor, &*start_find_action()));
                assert!(find_state.is_revealed());

                find_state.change(search_change("ABC"), true, None);
                settle(run_action(editor, &*next_match_find_action()));

                assert!(!find_state.is_revealed());
                find_controller.dispose();
            },
        );
    });
}

#[test]
fn editor_find_close_on_result_keeps_find_widget_open_when_no_match_is_found() {
    run_test(StoredOptions { is_regex: Some(false), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(
            text_lines(&["ABC", "DEF", "XYZ"]),
            suite.options(vec![("closeOnResult", Dynamic::Bool(true))]),
            |editor, _, services| {
                let find_controller = create_test_find_controller(editor);
                let find_state = find_controller.get_state();

                settle_promise(&execute_action(services, editor, &*start_find_action()));
                assert!(find_state.is_revealed());

                find_state.change(search_change("NO_MATCH"), true, None);
                settle(run_action(editor, &*next_match_find_action()));

                assert_eq!(find_state.matches_count(), 0);
                assert!(find_state.is_revealed());
                find_controller.dispose();
            },
        );
    });
}

#[test]
fn editor_find_close_on_result_disabled_keeps_find_widget_open_after_navigation() {
    run_test(StoredOptions { is_regex: Some(false), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(
            text_lines(&["ABC", "ABC", "XYZ"]),
            suite.options(vec![("closeOnResult", Dynamic::Bool(false))]),
            |editor, _, services| {
                let find_controller = create_test_find_controller(editor);
                let find_state = find_controller.get_state();

                settle_promise(&execute_action(services, editor, &*start_find_action()));
                assert!(find_state.is_revealed());

                find_state.change(search_change("ABC"), true, None);
                settle(run_action(editor, &*next_match_find_action()));

                assert!(find_state.is_revealed());
                find_controller.dispose();
            },
        );
    });
}

#[test]
fn issue_9043_clear_search_scope_when_find_widget_is_hidden() {
    run_test(StoredOptions { is_regex: Some(false), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(
            text_lines(&["var x = (3 * 5)", "var y = (3 * 5)", "var z = (3 * 5)"]),
            suite.options(vec![]),
            |editor, _, _| {
                let find_controller = create_test_find_controller(editor);
                settle_promise(&find_controller.controller.start(start_options(FindStartFocusAction::NoFocusChange, false), None));

                assert_eq!(find_controller.get_state().search_scope(), None);

                find_controller.get_state().change(
                    INewFindReplaceState { search_scope: Some(Some(vec![Range::new(1, 1, 1, 5)])), ..INewFindReplaceState::default() },
                    false,
                    None,
                );

                assert_eq!(find_controller.get_state().search_scope(), Some(vec![Range::new(1, 1, 1, 5)]));

                find_controller.close_find_widget();
                assert_eq!(find_controller.get_state().search_scope(), None);
            },
        );
    });
}

#[test]
fn issue_18111_regex_replace_with_single_space_replaces_with_no_space() {
    run_test(StoredOptions { is_regex: Some(false), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(
            text_lines(&["HRESULT OnAmbientPropertyChange(DISPID   dispid);"]),
            suite.options(vec![]),
            |editor, _, services| {
                let find_controller = create_test_find_controller(editor);

                settle_promise(&execute_action(services, editor, &*start_find_action()));

                find_controller.get_state().change(
                    INewFindReplaceState {
                        search_string: Some(JsString::from("\\b\\s{3}\\b")),
                        replace_string: Some(JsString::from(" ")),
                        is_regex: Some(true),
                        ..INewFindReplaceState::default()
                    },
                    false,
                    None,
                );
                find_controller.move_to_next_match();

                let selections: Vec<[i32; 4]> = editor.get_selections().unwrap().iter().map(from_selection).collect();
                assert_eq!(selections, vec![[1, 39, 1, 42]]);

                find_controller.replace();

                assert_eq!(editor.get_value(None), "HRESULT OnAmbientPropertyChange(DISPID dispid);");

                find_controller.dispose();
            },
        );
    });
}

#[test]
fn issue_24714_regular_expression_with_caret_in_search_and_replace() {
    // `#18111` deixou `editor.isRegex` verdadeiro no armazenamento compartilhado.
    run_test(StoredOptions { is_regex: Some(true), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(text_lines(&["", "line2", "line3"]), suite.options(vec![]), |editor, _, services| {
            let find_controller = create_test_find_controller(editor);

            settle_promise(&execute_action(services, editor, &*start_find_action()));

            find_controller.get_state().change(
                INewFindReplaceState {
                    search_string: Some(JsString::from("^")),
                    replace_string: Some(JsString::from("x")),
                    is_regex: Some(true),
                    ..INewFindReplaceState::default()
                },
                false,
                None,
            );
            find_controller.move_to_next_match();

            let selections: Vec<[i32; 4]> = editor.get_selections().unwrap().iter().map(from_selection).collect();
            assert_eq!(selections, vec![[2, 1, 2, 1]]);

            find_controller.replace();

            assert_eq!(editor.get_value(None), "\nxline2\nline3");

            find_controller.dispose();
        });
    });
}

#[test]
fn issue_38232_find_next_selection_regex_enabled() {
    run_test(StoredOptions { is_regex: Some(true), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(text_lines(&["([funny]", "", "([funny]"]), suite.options(vec![]), |editor, _, _| {
            let find_controller = create_test_find_controller(editor);
            let next_selection_match_find_action = SelectionMatchFindAction::next();

            // toggle regex
            find_controller.get_state().change(INewFindReplaceState { is_regex: Some(true), ..INewFindReplaceState::default() }, false, None);

            // change selection
            editor.set_selection(Selection::new(1, 1, 1, 9).into(), None);

            // cmd+f3
            settle(run_action(editor, &next_selection_match_find_action));

            let selections: Vec<[i32; 4]> = editor.get_selections().unwrap().iter().map(from_selection).collect();
            assert_eq!(selections, vec![[3, 1, 3, 9]]);

            find_controller.dispose();
        });
    });
}

#[test]
fn issue_38232_find_next_selection_regex_enabled_find_widget_open() {
    run_test(StoredOptions { is_regex: Some(true), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(text_lines(&["([funny]", "", "([funny]"]), suite.options(vec![]), |editor, _, services| {
            let find_controller = create_test_find_controller(editor);
            let next_selection_match_find_action = SelectionMatchFindAction::next();

            // cmd+f - open find widget
            settle_promise(&execute_action(services, editor, &*start_find_action()));

            // toggle regex
            find_controller.get_state().change(INewFindReplaceState { is_regex: Some(true), ..INewFindReplaceState::default() }, false, None);

            // change selection
            editor.set_selection(Selection::new(1, 1, 1, 9).into(), None);

            // cmd+f3
            settle(run_action(editor, &next_selection_match_find_action));

            let selections: Vec<[i32; 4]> = editor.get_selections().unwrap().iter().map(from_selection).collect();
            assert_eq!(selections, vec![[3, 1, 3, 9]]);

            find_controller.dispose();
        });
    });
}

#[test]
fn issue_47400_cmd_e_supports_feeding_multiple_line_of_text_into_the_find_widget() {
    run_test(StoredOptions { is_regex: Some(true), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(text_lines(&["ABC", "ABC", "XYZ", "ABC", "ABC"]), suite.options(vec![]), |editor, _, services| {
            let find_controller = create_test_find_controller(editor);

            // change selection
            editor.set_selection(Selection::new(1, 1, 1, 1).into(), None);

            // cmd+f - open find widget
            settle_promise(&execute_action(services, editor, &*start_find_action()));

            editor.set_selection(Selection::new(1, 1, 2, 4).into(), None);
            let start_find_with_selection_action = StartFindWithSelectionAction::new();
            settle(run_action(editor, &start_find_with_selection_action));
            let find_state = find_controller.get_state();

            let search_string = find_state.search_string().to_string_lossy().replace("\r\n", "\n");
            assert_eq!(search_string.split(['\n', '\r']).collect::<Vec<_>>(), vec!["ABC", "ABC"]);

            editor.set_selection(Selection::new(3, 1, 3, 1).into(), None);
            settle(run_action(editor, &start_find_with_selection_action));

            find_controller.dispose();
        });
    });
}

#[test]
fn issue_109756_cmd_e_with_empty_cursor_should_always_work() {
    run_test(StoredOptions { is_regex: Some(true), ..StoredOptions::default() }, |suite| {
        with_test_code_editor(text_lines(&["ABC", "ABC", "XYZ", "ABC", "ABC"]), suite.options(vec![]), |editor, _, _| {
            let find_controller = create_test_find_controller(editor);
            editor.set_selection(Selection::new(1, 2, 1, 2).into(), None);

            let start_find_with_selection_action = StartFindWithSelectionAction::new();
            let pending = run_action(editor, &start_find_with_selection_action);

            let find_state = find_controller.get_state();
            assert_eq!(find_state.search_string(), "ABC");
            settle(pending);
            find_controller.dispose();
        });
    });
}

// #endregion

// #region Suíte `FindController query options persistence`

/// O estado inicial dos casos da suíte de persistência: as `store` do corpo da suíte (`isRegex` e `matchCase` falsos,
/// `wholeWord` verdadeiro) rodam antes de qualquer caso.
fn persistence_options() -> StoredOptions {
    StoredOptions { is_regex: Some(false), match_case: Some(false), whole_word: Some(true) }
}

#[test]
fn match_case() {
    run_test(persistence_options(), |suite| {
        with_test_code_editor(text_lines(&["abc", "ABC", "XYZ", "ABC"]), suite.options(vec![]), |editor, _, services| {
            StorageService::store(&suite.storage_service, "editor.matchCase", true.into(), StorageScope::Workspace, StorageTarget::User);
            // The cursor is at the very top, of the file, at the first ABC
            let find_controller = create_test_find_controller(editor);
            let find_state = find_controller.get_state();

            // I hit Ctrl+F to show the Find dialog
            settle_promise(&execute_action(services, editor, &*start_find_action()));

            // I type ABC.
            find_state.change(search_change("ABC"), true, None);
            // The second ABC is highlighted as matchCase is true.
            assert_eq!(from_selection(&editor.get_selection().unwrap()), [2, 1, 2, 4]);

            find_controller.dispose();
        });
    });
}

#[test]
fn whole_word() {
    // O caso `matchCase` deixou `editor.matchCase` verdadeiro no armazenamento.
    run_test(StoredOptions { match_case: Some(true), ..persistence_options() }, |suite| {
        with_test_code_editor(text_lines(&["ABC", "AB", "XYZ", "ABC"]), suite.options(vec![]), |editor, _, services| {
            // The cursor is at the very top, of the file, at the first ABC
            let find_controller = create_test_find_controller(editor);
            let find_state = find_controller.get_state();

            // I hit Ctrl+F to show the Find dialog
            settle_promise(&execute_action(services, editor, &*start_find_action()));

            // I type AB.
            find_state.change(search_change("AB"), true, None);
            // The second AB is highlighted as wholeWord is true.
            assert_eq!(from_selection(&editor.get_selection().unwrap()), [2, 1, 2, 3]);

            find_controller.dispose();
        });
    });
}

#[test]
fn toggling_options_is_saved() {
    run_test(StoredOptions { match_case: Some(true), ..persistence_options() }, |suite| {
        with_test_code_editor(text_lines(&["ABC", "AB", "XYZ", "ABC"]), suite.options(vec![]), |editor, _, _| {
            // The cursor is at the very top, of the file, at the first ABC
            let find_controller = create_test_find_controller(editor);
            find_controller.toggle_regex();
            assert_eq!(suite.storage_service.get_boolean("editor.isRegex", StorageScope::Workspace, None), Some(true));

            find_controller.dispose();
        });
    });
}

/// Os casos seguintes herdam do `toggling options is saved` o `editor.isRegex` verdadeiro.
fn persistence_options_after_toggling() -> StoredOptions {
    StoredOptions { is_regex: Some(true), match_case: Some(true), whole_word: Some(true) }
}

/// A `find` dos casos de escopo: `{ autoFindInSelection: 'always', globalFindClipboard: false }`.
fn auto_find_in_selection(mode: &str) -> Vec<(&'static str, Dynamic)> {
    vec![("autoFindInSelection", Dynamic::string(mode)), ("globalFindClipboard", Dynamic::Bool(false))]
}

#[test]
fn issue_27083_update_search_scope_once_find_widget_becomes_visible() {
    run_test(persistence_options_after_toggling(), |suite| {
        with_test_code_editor(
            text_lines(&["var x = (3 * 5)", "var y = (3 * 5)", "var z = (3 * 5)"]),
            suite.options(auto_find_in_selection("always")),
            |editor, _, _| {
                // clipboardState = '';
                let find_controller = create_test_find_controller(editor);
                let find_config = start_options(FindStartFocusAction::NoFocusChange, true);

                editor.set_selection(Range::new(1, 1, 2, 1).into(), None);
                let _ = find_controller.controller.start(find_config, None);
                assert_eq!(find_controller.get_state().search_scope(), Some(vec![Range::new(1, 1, 2, 1)]));

                find_controller.close_find_widget();

                editor.set_selections(&[Selection::new(1, 1, 2, 1), Selection::new(2, 1, 2, 5)], None, None);
                let _ = find_controller.controller.start(find_config, None);
                assert_eq!(
                    find_controller.get_state().search_scope(),
                    Some(vec![Range::new(1, 1, 2, 1), Range::new(2, 1, 2, 5)])
                );
                runtime::run_microtasks();
            },
        );
    });
}

#[test]
fn issue_58604_do_not_update_search_scope_if_it_is_empty() {
    run_test(persistence_options_after_toggling(), |suite| {
        with_test_code_editor(
            text_lines(&["var x = (3 * 5)", "var y = (3 * 5)", "var z = (3 * 5)"]),
            suite.options(auto_find_in_selection("always")),
            |editor, _, _| {
                // clipboardState = '';
                editor.set_selection(Range::new(1, 2, 1, 2).into(), None);
                let find_controller = create_test_find_controller(editor);

                settle_promise(&find_controller.controller.start(start_options(FindStartFocusAction::NoFocusChange, true), None));

                assert_eq!(find_controller.get_state().search_scope(), None);
            },
        );
    });
}

#[test]
fn issue_58604_update_search_scope_if_it_is_not_empty() {
    run_test(persistence_options_after_toggling(), |suite| {
        with_test_code_editor(
            text_lines(&["var x = (3 * 5)", "var y = (3 * 5)", "var z = (3 * 5)"]),
            suite.options(auto_find_in_selection("always")),
            |editor, _, _| {
                // clipboardState = '';
                editor.set_selection(Range::new(1, 2, 1, 3).into(), None);
                let find_controller = create_test_find_controller(editor);

                settle_promise(&find_controller.controller.start(start_options(FindStartFocusAction::NoFocusChange, true), None));

                assert_eq!(find_controller.get_state().search_scope(), Some(vec![Range::new(1, 2, 1, 3)]));
            },
        );
    });
}

#[test]
fn issue_27083_find_in_selection_when_multiple_lines_are_selected() {
    run_test(persistence_options_after_toggling(), |suite| {
        with_test_code_editor(
            text_lines(&["var x = (3 * 5)", "var y = (3 * 5)", "var z = (3 * 5)"]),
            suite.options(auto_find_in_selection("multiline")),
            |editor, _, _| {
                // clipboardState = '';
                editor.set_selection(Range::new(1, 6, 2, 1).into(), None);
                let find_controller = create_test_find_controller(editor);

                settle_promise(&find_controller.controller.start(start_options(FindStartFocusAction::NoFocusChange, true), None));

                assert_eq!(find_controller.get_state().search_scope(), Some(vec![Range::new(1, 6, 2, 1)]));
            },
        );
    });
}

// #endregion
