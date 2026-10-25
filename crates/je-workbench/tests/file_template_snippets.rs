//! Porte de `src/vs/workbench/contrib/snippets/test/browser/fileTemplateSnippets.test.ts` (suíte `ApplyFileSnippetAction`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` e `ensureNoDisposablesAreLeakedInTestSuite()` são o [`Fixture::new`] e o guarda de vazamentos criado no
//!   começo do caso (o `disposables` da suíte é o store do guarda, descartado ao fim).
//! - `createCodeEditorServices(disposables, services)` com o `TestLanguageConfigurationService` é o `create_code_editor_services`
//!   do `je-editor/tests/support`, cujo serviço de configuração de linguagem recebe a linguagem `csharp` com os comentários
//!   `//` e `/* */`. `langService.registerLanguage({ id: 'csharp', extensions: ['.cs'] })` é o `register_language` do
//!   `ILanguageService` do mesmo contêiner.
//! - `instantiateTextModel(instantiationService, '', null, {}, URI.parse('untitled:Untitled-1'))` é o
//!   `TextModel::new_with_language_service` com os serviços de linguagem do contêiner (o modelo precisa conhecer `csharp` para
//!   o `getLanguageIdAtPosition` do resolvedor de comentários), e `instantiateTestCodeEditor` é o de mesmo nome do suporte.
//! - `editor.registerAndInstantiateContribution(SnippetController2.ID, SnippetController2)` é o de mesmo nome com o
//!   `SnippetController2Services` (log nulo, o `MockContextKeyService`, o `ILanguageConfigurationService` do contêiner, um
//!   `ILabelService` que não é consultado e nenhum `IWorkspaceContextService`).
//! - Os `stub(...)` com `new class extends mock<...>()` são os dublês [`StubSnippetsService`], [`FirstItemQuickInputService`] e
//!   [`ActiveEditorService`]; o método que o `mock<>()` deixa indefinido falha com `panic!`.
//! - `override async pick(...)` resolve o primeiro item que não é separador (`resolved.find(p => p.type !== 'separator')`).
//! - `await instantiationService.invokeFunction(accessor => new ApplyFileSnippetAction().run(accessor))` é o
//!   `invoke_function` com `apply_file_snippet` (o corpo de `run`), esperado dentro de `run_with_faked_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

/// O suporte de editor do `je-editor/tests/support` que o `create_code_editor_services` precisa, no mesmo módulo pai (os arquivos
/// se referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use editor_support::test_code_editor::{
    CodeEditorServices, TestCodeEditor, create_code_editor_services, instantiate_test_code_editor,
};
use editor_support::test_text_model::resolve_options;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::observable::base::IObservable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::language_configuration::{CommentRule, LanguageConfiguration, LineComment};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::snippet::snippet_controller2::{SnippetController2, SnippetController2Services};
use je_editor::snippet::snippet_session::SnippetServices;
use je_editor::snippet::snippet_variables::{ILabelService, UriLabelOptions};
use je_editor::text_model::{TextModel, TextModelServices};
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use je_platform::quickinput::quick_input::{
    I_QUICK_INPUT_SERVICE, IInputBox, IInputOptions, IKeyMods, IPickOptions, IQuickInputService, IQuickNavigateConfiguration,
    IQuickPick, IQuickPickItem, IQuickTree, QuickInputAlignment, QuickInputAlignmentSetting, QuickInputAnchor,
    QuickInputAnchorPosition, QuickInputButtonRef, QuickInputHideReason, QuickInputRef, QuickPickCreateOptions, QuickPickInput,
    QuickPicks,
};
use je_workbench::common::editor::{
    EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput,
};
use je_workbench::contrib::snippets::commands::file_template_snippets::apply_file_snippet;
use je_workbench::contrib::snippets::snippets::{I_SNIPPETS_SERVICE, ISnippetGetOptions, ISnippetsService};
use je_workbench::contrib::snippets::snippets_file::{Snippet, SnippetFile, SnippetSource};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, I_EDITOR_SERVICE, IActiveCodeEditor, IEditorService, SaveAllEditorsOptions,
    SaveEditorsOptions, SaveEditorsResult,
};

/// `new class extends mock<ISnippetsService>() { override async getSnippets() { ... } }`.
struct StubSnippetsService;

impl ISnippetsService for StubSnippetsService {
    fn get_snippet_files(&self) -> Promise<Vec<Rc<SnippetFile>>> {
        panic!("getSnippetFiles is not a function")
    }

    fn is_enabled(&self, _snippet: &Snippet) -> bool {
        panic!("isEnabled is not a function")
    }

    fn update_enablement(&self, _snippet: &Snippet, _enabled: bool) {
        panic!("updateEnablement is not a function")
    }

    fn update_usage_timestamp(&self, _snippet: &Snippet) {
        panic!("updateUsageTimestamp is not a function")
    }

    fn get_snippets(&self, _language_id: Option<&str>, _resource_uri: Option<&URI>, _opt: Option<&ISnippetGetOptions>) -> Promise<Vec<Rc<Snippet>>> {
        Promise::resolved(vec![Rc::new(Snippet::new(
            true,
            vec!["csharp".to_string()],
            "comment template",
            "comment template",
            "comment template",
            "$BLOCK_COMMENT_START block $BLOCK_COMMENT_END\n$LINE_COMMENT line",
            "user",
            SnippetSource::User,
            &generate_uuid(),
            None,
            None,
            None,
        ))])
    }

    fn get_snippets_sync(&self, _language_id: &str, _resource_uri: Option<&URI>, _opt: Option<&ISnippetGetOptions>) -> Vec<Rc<Snippet>> {
        panic!("getSnippetsSync is not a function")
    }
}

/// `new class extends mock<IQuickInputService>() { override async pick(...) { ... } }`.
struct FirstItemQuickInputService;

impl IQuickInputService for FirstItemQuickInputService {
    fn pick(&self, picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>> {
        async_fn(async move {
            let resolved = picks.into_promise().await?;
            Ok(resolved.into_iter().find_map(|pick| match pick {
                QuickPickInput::Item(item) => Some(item),
                QuickPickInput::Separator(_) => None,
            }))
        })
    }

    fn back_button(&self) -> QuickInputButtonRef {
        panic!("backButton is not defined")
    }

    fn on_show(&self) -> Event<()> {
        panic!("onShow is not defined")
    }

    fn on_hide(&self) -> Event<()> {
        panic!("onHide is not defined")
    }

    fn alignment(&self) -> IObservable<QuickInputAlignment> {
        panic!("alignment is not defined")
    }

    fn with_quick_input_anchor(
        &self,
        _anchor: Option<QuickInputAnchor>,
        _anchor_position: Option<QuickInputAnchorPosition>,
        _operation: Box<dyn FnOnce() -> Promise<Dynamic>>,
    ) -> Promise<Dynamic> {
        panic!("withQuickInputAnchor is not a function")
    }

    fn pick_many(&self, _picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>> {
        panic!("pick is not a function")
    }

    fn input(&self, _options: Option<&IInputOptions>, _token: Option<&CancellationToken>) -> Promise<Option<String>> {
        panic!("input is not a function")
    }

    fn create_quick_pick(&self, _options: Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick> {
        panic!("createQuickPick is not a function")
    }

    fn create_input_box(&self) -> Rc<dyn IInputBox> {
        panic!("createInputBox is not a function")
    }

    fn create_quick_tree(&self) -> Rc<dyn IQuickTree> {
        panic!("createQuickTree is not a function")
    }

    fn focus(&self) {
        panic!("focus is not a function")
    }

    fn toggle(&self) {
        panic!("toggle is not a function")
    }

    fn navigate(&self, _next: bool, _quick_navigate: Option<&IQuickNavigateConfiguration>) {
        panic!("navigate is not a function")
    }

    fn back(&self) -> Promise<()> {
        panic!("back is not a function")
    }

    fn accept(&self, _key_mods: Option<IKeyMods>) -> Promise<()> {
        panic!("accept is not a function")
    }

    fn cancel(&self, _reason: Option<QuickInputHideReason>) -> Promise<()> {
        panic!("cancel is not a function")
    }

    fn toggle_hover(&self) {
        panic!("toggleHover is not a function")
    }

    fn current_quick_input(&self) -> Option<QuickInputRef> {
        panic!("currentQuickInput is not defined")
    }

    fn set_alignment(&self, _alignment: QuickInputAlignmentSetting) {
        panic!("setAlignment is not a function")
    }
}

/// `new class extends mock<IEditorService>() { override activeTextEditorControl = editor; }`.
struct ActiveEditorService {
    editor: Rc<dyn IActiveCodeEditor>,
}

impl IEditorService for ActiveEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        panic!("visibleEditorPanes is not defined")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        panic!("activeEditor is not defined")
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        Some(ActiveTextEditorControl::Code(self.editor.clone()))
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        panic!("activeTextEditorLanguageId is not defined")
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        panic!("openEditor is not a function")
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        panic!("findEditors is not a function")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        panic!("save is not a function")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        panic!("saveAll is not a function")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        panic!("getEditors is not a function")
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        panic!("openEditors is not a function")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        panic!("isVisible is not a function")
    }
}

/// O `ILabelService` que o `SnippetController2` recebe e que a suíte não consulta.
struct UnusedLabelService;

impl ILabelService for UnusedLabelService {
    fn get_uri_label(&self, _resource: &URI, _options: &UriLabelOptions) -> String {
        panic!("ILabelService.getUriLabel não está definido neste mock")
    }
}

/// O que o `setup` deixa em variáveis da suíte.
struct Fixture {
    services: Rc<CodeEditorServices>,
    model: Rc<TextModel>,
}

impl Fixture {
    fn new(ds: &LeakGuard) -> Self {
        let store = ds.store();

        let services = create_code_editor_services(&store, None);
        let lang_config_service = services.language_configuration_service.clone();
        store.add(lang_config_service.register(
            "csharp",
            LanguageConfiguration {
                comments: Some(CommentRule {
                    line_comment: Some(LineComment::Token("//".to_string())),
                    block_comment: Some(("/*".to_string(), "*/".to_string())),
                }),
                ..LanguageConfiguration::default()
            },
            None,
        ));

        let lang_service = services.get(I_LANGUAGE_SERVICE);
        store.add(lang_service.register_language(LanguageExtensionPoint {
            id: "csharp".to_string(),
            extensions: Some(vec![".cs".to_string()]),
            ..LanguageExtensionPoint::default()
        }));

        let model = ds.add_rc(TextModel::new_with_language_service(
            "".into(),
            "plaintext",
            resolve_options(&RelaxedTextModelCreationOptions::default()),
            Some(URI::parse("untitled:Untitled-1".to_string(), false)),
            services.undo_redo_service.clone(),
            Some(TextModelServices { language_service: lang_service.clone(), language_configuration_service: lang_config_service.clone() }),
        ));
        let editor: Rc<TestCodeEditor> = ds.add_rc(instantiate_test_code_editor(
            &services,
            Some(model.clone()),
            &Dynamic::object(std::iter::empty::<(&str, Dynamic)>()),
        ));

        let context_keys = ds.add_rc(MockContextKeyService::new());
        let context_key_service: Rc<dyn IContextKeyService> = context_keys;
        let language_configuration_service: Rc<dyn ILanguageConfigurationService> = lang_config_service;
        let snippet_services = Rc::new(SnippetServices {
            language_configuration_service,
            label_service: Rc::new(UnusedLabelService),
            workspace_service: None,
        });
        let controller = editor.register_and_instantiate_contribution(SnippetController2::ID, |widget, _instantiation_service| {
            SnippetController2::new(
                widget,
                SnippetController2Services {
                    log_service: NullLogService::shared(),
                    choice_host: None,
                    context_key_service,
                    snippet_services,
                },
            )
        });
        ds.add_rc(controller);

        let snippets_service: Rc<dyn ISnippetsService> = Rc::new(StubSnippetsService);
        services.stub(I_SNIPPETS_SERVICE, snippets_service);
        let quick_input_service: Rc<dyn IQuickInputService> = Rc::new(FirstItemQuickInputService);
        services.stub(I_QUICK_INPUT_SERVICE, quick_input_service);
        let active_editor: Rc<dyn IActiveCodeEditor> = editor.widget().clone();
        let editor_service: Rc<dyn IEditorService> = Rc::new(ActiveEditorService { editor: active_editor });
        services.stub(I_EDITOR_SERVICE, editor_service);

        Self { services, model }
    }
}

#[test]
fn resolves_comment_variables_using_selected_file_template_language() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&ds);

    let services = fixture.services.clone();
    run_with_faked_timers(async move {
        let applied = services.invoke_function(|accessor| apply_file_snippet(accessor));
        applied.await.expect("a ação termina sem erro");
    });

    assert_eq!(fixture.model.get_value(), "/* block */\n// line");
}
