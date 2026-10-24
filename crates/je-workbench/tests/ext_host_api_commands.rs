//! Porte de `src/vs/workbench/api/test/browser/extHostApiCommands.test.ts` (suíte `ExtHostLanguageFeatureCommands`), caso a caso e na mesma ordem.
//!
//! PORTADOS (os 51 dos 51 `test(` e `testApiCmd(` do original): `WorkspaceSymbols, invalid arguments`, `WorkspaceSymbols, back and forth`,
//! `executeWorkspaceSymbolProvider should accept empty string, #39522`, `executeFormatDocumentProvider, back and forth`,
//! `vscode.prepareRename`, `vscode.executeDocumentRenameProvider`, `Definition` x4, `Declaration` x2, `Type Definition` x3,
//! `Implementation` x3, `reference search, back and forth`, `"vscode.executeDocumentHighlights" API has stopped returning
//! DocumentHighlight[]#200056`, `Outline, back and forth`, `vscode.executeDocumentSymbolProvider command only returns
//! SymbolInformation[] rather than DocumentSymbol[] #57984`, os sete de `suggest` (`triggerCharacter is null when completion provider
//! is called programmatically #159914`, `Suggest, back and forth`, `Suggest, return CompletionList !array`, `Suggest, resolve
//! completion items`, `"vscode.executeCompletionItemProvider" doesnot return a preselect field #53749`, `executeCompletionItemProvider
//! doesn't capture commitCharacters #58228` e `vscode.executeCompletionItemProvider returns the wrong CompletionItemKinds in insiders
//! #95715`), `Parameter Hints, back and forth`, os cinco de `quickfix` (`QuickFix, back and forth`, os três `vscode.executeCodeActionProvider
//! ...` e `resolving code action`), os dois de `CodeLens` (`back and forth` e `resolve`), os três de links (`Links, back and forth`,
//! `What's the condition for DocumentLink target to be undefined? #106308` e `DocumentLink[] vscode.executeLinkProvider returns lack
//! tooltip #213970`), `Color provider`, `"TypeError: e.onCancellationRequested is not a function" calling hover provider in Insiders
//! #54174`, os três de `inline hints` (`Inlay Hints, back and forth`, `Inline Hints, merge` e `Inline Hints, bad provider`), os três de
//! `Selection Range` (`back and forth` e os dois `#91852`), `CallHierarchy, back and forth`, `prepareCallHierarchy throws TypeError if
//! clangd returns empty result #137415` e `TypeHierarchy, back and forth`.
//!
//! Nada fica de fora. Onde o original usa uma capacidade do TypeScript que o Rust não tem, a tradução está nas notas abaixo.
//!
//! Notas de tradução:
//! - `class MyAction extends types.CodeAction { }` e o `assert.ok(action instanceof MyAction)` do `resolving code action`: o
//!   `resolveCodeAction` confere que recebeu o mesmo objeto (`Rc::ptr_eq`) que o `provideCodeActions` devolveu, o que é o que a
//!   subclasse garante.
//! - `arguments: [document, range]` dos casos de `vscode.executeCodeActionProvider`: o `document` (um [`TextDocument`]) não é um
//!   [`Dynamic`], e entra o recurso dele; os casos só leem `arguments[1]`.
//! - `throw new Error()` do `provideInlayHints` do `Inline Hints, bad provider` é a promessa rejeitada que o adaptador do host
//!   devolveria do `throw` dentro da função `async`.
//! - `vscode.WorkspaceEdit` (resultado de `vscode.executeDocumentRenameProvider`) é um objeto marcado `WorkspaceEdit` com as funções
//!   `has(uri)` e `get(uri)`, a parte `Maplike` que o caso lê.
//! - `new TestRPCProtocol()` é o [`TestMainContext`]. Os dois lados dos comandos se conhecem (o `MainThreadCommands` pede o proxy do host
//!   e o `ExtHostCommands` o do workbench, os dois na construção), então o proxy de `MainThreadCommands` registrado primeiro é o
//!   [`LateMainThreadCommands`], que repassa tudo ao `MainThreadCommands` real depois de ele ser criado.
//! - `ICommandService` do `TestInstantiationService` é o [`TestCommandService`]: busca o comando em `CommandsRegistry` e o roda dentro
//!   do `insta.invokeFunction`. O retorno `Promise` do handler (a promessa embrulhada de `promise_to_dynamic`) é desembrulhado.
//! - `IExtensionService` é o [`TestExtensionService`] (`activateByEvent` resolve e `activationEventIsDone` é verdadeiro).
//! - `ILanguageFeaturesService` é o [`LanguageFeaturesService`] do ambiente (o mesmo que o `MainThreadLanguageFeatures` preenche), com o
//!   identificador `I_LANGUAGE_FEATURES_SERVICE`. `IModelService` (`getModel() { return model; }`), `ITextModelService`
//!   (`createModelReference` devolve a referência imortal do modelo), `IEditorWorkerService` (`computeMoreMinimalEdits` devolve as
//!   próprias edições), `IConfigurationService` (o `TestConfigurationService` vazio) e `IOutlineModelService` (o `OutlineModelService`
//!   de verdade) entram no mesmo `ServiceCollection`.
//! - Os `import '...'` do topo do original, que registram os comandos `_execute*`, são a chamada de `register_editor_execute_commands`.
//! - `setUnexpectedErrorHandler(() => { })` e o `ensureNoDisposablesAreLeakedInTestSuite` são o guarda criado no fim do `setup`. O
//!   `suiteTeardown` (que descarta o `mainThread`, o serviço de outline e o `insta`) roda no `teardown` de cada caso.
//! - O resultado de `executeCommand` é um [`Dynamic`]: `instanceof URI` é `is_instance_of(.., "Uri")`, `instanceof types.Range` é
//!   `is_instance_of(.., "Range")`, e o `instanceof` das classes que o porte não tem como `ApiValue` (`SymbolInformation`,
//!   `DocumentSymbol`) é a leitura da propriedade `$$class`.
//! - `assertRejects(() => commands.executeCommand(...))` é a conferência de que a promessa rejeita.
//! - `commands.executeCommand<T>(id, ...args)` é `commands.execute_command(id, args)`; `new types.Position(0, 0)` e `model.uri` entram
//!   como os valores da API (`ApiValue::to_dynamic`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::async_::timeout_with_token;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit as EditorTextEdit;
use je_editor::editor_extensions::{I_TEXT_MODEL_SERVICE, ITextModelService, ModelReference};
use je_editor::editor_worker::{I_EDITOR_WORKER_SERVICE, IEditorWorkerService, WordRanges};
use je_editor::language_feature_debounce::LanguageFeatureDebounceService;
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::language_service::ILanguageSelection;
use je_editor::model::{Eol, TextModelCreationOptions};
use je_editor::outline_model::{I_OUTLINE_MODEL_SERVICE, IOutlineModelService, ModelRemovedSource, OutlineModelService};
use je_editor::range::Range as EditorRange;
use je_editor::services_model::{I_MODEL_SERVICE, IModelService};
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::commands::commands::{CommandEvent, ICommandService, commands_registry};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::api::browser::main_thread_commands::{MainThreadCommands, promise_from_dynamic};
use je_workbench::api::browser::main_thread_language_features::MainThreadLanguageFeatures;
use je_workbench::api::common::editor_execute_commands::{I_LANGUAGE_FEATURES_SERVICE, register_editor_execute_commands};
use je_workbench::api::common::ext_host_api_commands::ApiCommands;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_diagnostics::ExtHostDiagnostics;
use je_workbench::api::common::ext_host_document_data::{IExtHostDocumentSaveDelegate, TextDocument};
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_file_system_info::ExtHostFileSystemInfo;
use je_workbench::api::common::ext_host_language_features::ExtHostLanguageFeatures;
use je_workbench::api::common::ext_host_language_features_api::{
    ApiCodeActionContext, ApiCodeActionProvider, ApiCodeActionRef, ApiCallHierarchyProvider, ApiCodeLensProvider, ApiColorPresentationContext, ApiCompletionItemProvider,
    ApiCompletionList, ApiCompletionResult, ApiDocumentColorProvider, ApiDocumentFormattingEditProvider, ApiDocumentHighlightProvider, ApiDocumentLinkProvider,
    ApiDocumentSymbolProvider, ApiDocumentSymbols, ApiHoverContext, ApiHoverProvider, ApiInlayHintsProvider, ApiLocationLinkProvider, ApiLocationResult, ApiRangeOrSelection,
    ApiTypeHierarchyProvider, CodeLensRef, CommandOrCodeAction, CompletionItemRef, DocumentLinkRef, InlayHintRef, OneOrMany,
    ApiPrepareRename, ApiProviderResult, ApiReferenceProvider, ApiRenameProvider, ApiSelectionRangeProvider, ApiSignatureHelpContext, ApiSignatureHelpProvider,
    ApiSignatureHelpRef, ApiWorkspaceSymbolProvider, DefinitionLink, IExtHostApiDeprecationService, LocationOrLink, SignatureHelpMetadataOrTriggerChars,
};
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, CreateDocumentOptions, DocumentsAndEditorsDelta, ExtHostCommandsShape, ExtHostContext, ExtHostDocumentsAndEditorsShape, ExtHostLanguageFeaturesShape,
    FormattingOptions as ApiFormattingOptions, IExtHostTelemetry, IMarkerChange, MainContext, MainThreadCommandsShape, MainThreadDiagnosticsShape,
    MainThreadDocumentsShape, MainThreadLanguageFeaturesShape, ModelAddedData, OpenDocumentOptions, ReferenceContext,
};
use je_workbench::api::common::ext_host_type_converters::{DocumentFilterInput, DocumentSelectorInput};
use je_workbench::api::common::ext_host_types as types;
use je_workbench::api::common::ext_host_values::{ApiValue, is_instance_of};
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService, null_extension_description};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use support::ext_host_workspace_support::TestMainContext;
use test_text_model::create_text_model_with;

// ---- os dublês ----------------------------------------------------------------------------------------

/// `new class extends mock<IUriIdentityService>() { override asCanonicalUri(uri) { return uri; } }`: o resto do serviço não existe.
struct IdentityUriIdentityService;

impl IUriIdentityService for IdentityUriIdentityService {
    fn ext_uri(&self) -> je_base::resources::ExtUri {
        panic!("extUri is not a function")
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// `new class extends mock<IExtHostTelemetry>() { override onExtensionError(): boolean { return true; } }`.
struct TestExtHostTelemetry;

impl IExtHostTelemetry for TestExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        true
    }
}

/// `NullApiDeprecationService`.
struct NullApiDeprecationService;

impl IExtHostApiDeprecationService for NullApiDeprecationService {
    fn report(&self, _api_id: &str, _extension: &ExtensionDescription, _migration_suggestion: &str) {}
}

/// `MainThreadDocuments` que falha se for chamado.
struct UnusedMainThreadDocuments;

impl IDisposable for UnusedMainThreadDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for UnusedMainThreadDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("$trySaveDocument is not a function")
    }
}

impl MainThreadDocumentsShape for UnusedMainThreadDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        panic!("$tryCreateDocument is not a function")
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        panic!("$tryOpenDocument is not a function")
    }
}

/// `MainThreadDiagnostics` que falha se for chamado.
struct UnusedMainThreadDiagnostics;

impl IDisposable for UnusedMainThreadDiagnostics {
    fn dispose(&self) {}
}

impl MainThreadDiagnosticsShape for UnusedMainThreadDiagnostics {
    fn rpc_change_many(&self, _owner: &str, _entries: Vec<IMarkerChange>) {
        panic!("$changeMany is not a function")
    }

    fn rpc_clear(&self, _owner: &str) {
        panic!("$clear is not a function")
    }
}

/// O proxy de `MainThreadCommands` que o `ExtHostCommands` pede na construção, antes de o `MainThreadCommands` existir (ver as notas
/// de tradução).
#[derive(Default)]
struct LateMainThreadCommands {
    inner: RefCell<Option<Rc<MainThreadCommands>>>,
    /// Os `$registerCommand` que o construtor do `ExtHostCommands` dispara antes de o `MainThreadCommands` existir: no
    /// `TestRPCProtocol` do original a chamada é adiada para uma microtarefa, já com o objeto registrado.
    pending_registrations: RefCell<Vec<String>>,
}

impl LateMainThreadCommands {
    fn get(&self) -> Rc<MainThreadCommands> {
        Rc::clone(self.inner.borrow().as_ref().expect("MainThreadCommands ainda não foi criado"))
    }

    /// Liga o `MainThreadCommands` real e entrega o que ficou adiado.
    fn bind(&self, main_thread_commands: Rc<MainThreadCommands>) {
        *self.inner.borrow_mut() = Some(Rc::clone(&main_thread_commands));
        for id in self.pending_registrations.take() {
            main_thread_commands.rpc_register_command(&id);
        }
    }
}

impl IDisposable for LateMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for LateMainThreadCommands {
    fn rpc_register_command(&self, id: &str) {
        if self.inner.borrow().is_none() {
            self.pending_registrations.borrow_mut().push(id.to_owned());
            return;
        }
        self.get().rpc_register_command(id);
    }

    fn rpc_unregister_command(&self, id: &str) {
        self.get().rpc_unregister_command(id);
    }

    fn rpc_fire_command_activation_event(&self, id: &str) {
        self.get().rpc_fire_command_activation_event(id);
    }

    fn rpc_execute_command(&self, id: &str, args: CommandArgs, retry: bool) -> Promise<Dynamic> {
        self.get().rpc_execute_command(id, args, retry)
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        self.get().rpc_get_commands()
    }
}

/// `ICommandService` do teste: `executeCommand(id, ...args)` busca o comando em `CommandsRegistry` e roda o handler no `insta`.
struct TestCommandService {
    insta: RefCell<Weak<InstantiationService>>,
}

impl ICommandService for TestCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        panic!("onWillExecuteCommand is not defined")
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        panic!("onDidExecuteCommand is not defined")
    }

    fn execute_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        let Some(command) = commands_registry().get_command(id) else {
            return Promise::rejected(Error::new(format!("{id} NOT known")));
        };
        let Some(insta) = self.insta.borrow().upgrade() else {
            return Promise::rejected(Error::new("o serviço de instanciação do teste já foi descartado"));
        };
        match errors::catch(|| insta.invoke_function(|accessor| (command.handler)(accessor, &args))) {
            Ok(result) => match promise_from_dynamic(&result) {
                Some(promise) => promise,
                None => Promise::resolved(result),
            },
            Err(error) => Promise::rejected(error),
        }
    }
}

/// `new class extends mock<IExtensionService>() { override async activateByEvent() {} override activationEventIsDone() { return true; } }`.
struct TestExtensionService;

impl IExtensionService for TestExtensionService {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        Promise::resolved(())
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        true
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        panic!("getExtension is not a function")
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        panic!("onDidRegisterExtensions is not defined")
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        panic!("whenInstalledExtensionsRegistered is not a function")
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        panic!("extensions is not defined")
    }
}

/// `new class extends mock<IModelService>() { override getModel() { return model; } override onModelRemoved = Event.None; }`.
struct TestModelService {
    model: Rc<TextModel>,
}

impl IModelService for TestModelService {
    fn get_model(&self, _resource: &URI) -> Option<Rc<TextModel>> {
        Some(Rc::clone(&self.model))
    }

    fn get_models(&self) -> Vec<Rc<TextModel>> {
        panic!("getModels is not a function")
    }

    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        panic!("onModelAdded is not defined")
    }

    fn create_model(&self, _value: TextSource, _language_selection: Option<&dyn ILanguageSelection>, _resource: Option<&URI>, _is_for_simple_widget: bool) -> Rc<TextModel> {
        panic!("createModel is not a function")
    }

    fn update_model(&self, _model: &TextModel, _value: TextSource, _reason: Option<TextModelEditSource>) {
        panic!("updateModel is not a function")
    }

    fn destroy_model(&self, _resource: &URI) {
        panic!("destroyModel is not a function")
    }

    fn get_creation_options(&self, _language: &str, _resource: Option<&URI>, _is_for_simple_widget: bool) -> TextModelCreationOptions {
        panic!("getCreationOptions is not a function")
    }
}

/// O `onModelRemoved = Event.None` do `IModelService` que o `OutlineModelService` ouve.
struct NoModelRemoved {
    emitter: Rc<Emitter<Rc<TextModel>>>,
}

impl ModelRemovedSource for NoModelRemoved {
    fn on_model_removed(&self) -> Event<Rc<TextModel>> {
        self.emitter.event()
    }
}

/// `new class extends mock<ITextModelService>() { override async createModelReference() { return new ImmortalReference(...); } }`.
struct TestTextModelService {
    model: Rc<TextModel>,
}

impl ITextModelService for TestTextModelService {
    fn create_model_reference(&self, _resource: &URI) -> Promise<ModelReference> {
        Promise::resolved(ModelReference::new(Rc::clone(&self.model), je_base::lifecycle::none()))
    }
}

/// `new class extends mock<IEditorWorkerService>() { override async computeMoreMinimalEdits(_uri, edits) { return edits || undefined; } }`.
struct NullWorkerService;

impl IEditorWorkerService for NullWorkerService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        panic!("canComputeWordRanges is not a function")
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: EditorRange) -> Promise<Option<WordRanges>> {
        panic!("computeWordRanges is not a function")
    }

    fn compute_more_minimal_edits(&self, _resource: &URI, edits: Option<Vec<EditorTextEdit>>) -> Promise<Option<Vec<EditorTextEdit>>> {
        Promise::resolved(edits)
    }
}

// ---- o ambiente (`suiteSetup`, `setup` e `teardown`) ----------------------------------------------------

/// `const defaultSelector = { scheme: 'far' }`.
fn default_selector() -> DocumentSelectorInput {
    DocumentSelectorInput::Filter(DocumentFilterInput { scheme: Some("far".to_owned()), ..DocumentFilterInput::default() })
}

struct Env {
    model: Rc<TextModel>,
    /// O `insta` do `suiteSetup`: o `TestCommandService` só guarda um `Weak` dele.
    insta: Rc<InstantiationService>,
    ext_host: Rc<ExtHostLanguageFeatures>,
    main_thread: Rc<MainThreadLanguageFeatures>,
    main_thread_commands: Rc<MainThreadCommands>,
    outline_service: Rc<OutlineModelService>,
    commands: Rc<ExtHostCommands>,
    rpc_protocol: Rc<TestMainContext>,
    disposables: Vec<DisposableRef>,
    /// O guarda de vazamentos nasce depois do `suiteSetup` (no original o `ensureNoDisposablesAreLeakedInTestSuite()` é registrado
    /// depois dele) e confere depois do `teardown`.
    guard: Option<LeakGuard>,
}

impl Env {
    /// `suiteSetup(() => { ... })`.
    fn new() -> Env {
        let model = create_text_model_with(
            ["This is the first line", "This is the second line", "This is the third line"].join("\n").as_str(),
            None,
            Default::default(),
            Some(URI::parse("far://testing/file.b", false)),
        );

        let rpc_protocol = Rc::new(TestMainContext::new());
        let language_features_service = LanguageFeaturesService::new();

        // Use IInstantiationService to get typechecking when instantiating
        let command_service = Rc::new(TestCommandService { insta: RefCell::new(Weak::new()) });
        let services = ServiceCollection::new();
        services.set(I_LANGUAGE_FEATURES_SERVICE, Rc::new(language_features_service.clone()));
        services.set(I_MODEL_SERVICE, Rc::new(TestModelService { model: Rc::clone(&model) }) as Rc<dyn IModelService>);
        services.set(I_TEXT_MODEL_SERVICE, Rc::new(TestTextModelService { model: Rc::clone(&model) }) as Rc<dyn ITextModelService>);
        services.set(I_EDITOR_WORKER_SERVICE, Rc::new(NullWorkerService) as Rc<dyn IEditorWorkerService>);
        services.set(I_CONFIGURATION_SERVICE, TestConfigurationService::new(None) as Rc<dyn IConfigurationService>);

        let environment_service = TestEnvironmentService::new();
        let debounce_service = LanguageFeatureDebounceService::new(NullLogService::shared(), &*environment_service);
        let outline_service = Rc::new(OutlineModelService::new(
            (*language_features_service.document_symbol_provider).clone(),
            &debounce_service,
            &NoModelRemoved { emitter: Emitter::new() },
        ));
        services.set(I_OUTLINE_MODEL_SERVICE, Rc::clone(&outline_service) as Rc<dyn IOutlineModelService>);

        let insta = InstantiationService::new(services);
        *command_service.insta.borrow_mut() = Rc::downgrade(&insta);

        let ext_host_documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc_protocol.clone(), NullLogService::shared()));
        rpc_protocol.set(&MainContext::MAIN_THREAD_DOCUMENTS, Rc::new(UnusedMainThreadDocuments) as Rc<dyn MainThreadDocumentsShape>);
        let eol = model.get_eol();
        let eol_text = match eol {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
        };
        ext_host_documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
            added_documents: Some(vec![ModelAddedData {
                is_dirty: false,
                version_id: model.get_version_id(),
                language_id: model.get_language_id(),
                uri: model.uri(),
                lines: model.get_value().to_string().split(eol_text).map(JsString::from).collect(),
                eol,
                encoding: "utf8".to_owned(),
            }]),
            ..DocumentsAndEditorsDelta::default()
        });
        let ext_host_documents = ExtHostDocuments::new(rpc_protocol.as_ref(), ext_host_documents_and_editors.clone());
        rpc_protocol.set(&ExtHostContext::EXT_HOST_DOCUMENTS, ext_host_documents.clone() as Rc<dyn je_workbench::api::common::ext_host_protocol::ExtHostDocumentsShape>);

        let late_main_thread_commands = Rc::new(LateMainThreadCommands::default());
        rpc_protocol.set(&MainContext::MAIN_THREAD_COMMANDS, Rc::clone(&late_main_thread_commands) as Rc<dyn MainThreadCommandsShape>);
        let commands = ExtHostCommands::new(rpc_protocol.as_ref(), NullLogService::shared(), Rc::new(TestExtHostTelemetry));
        rpc_protocol.set(&ExtHostContext::EXT_HOST_COMMANDS, commands.clone() as Rc<dyn ExtHostCommandsShape>);
        let main_thread_commands = MainThreadCommands::new(rpc_protocol.as_ref(), command_service, Rc::new(TestExtensionService));
        late_main_thread_commands.bind(Rc::clone(&main_thread_commands));
        register_editor_execute_commands();
        ApiCommands::register(&commands).expect("ExtHostApiCommands.register");

        rpc_protocol.set(&MainContext::MAIN_THREAD_DIAGNOSTICS, Rc::new(UnusedMainThreadDiagnostics) as Rc<dyn MainThreadDiagnosticsShape>);
        let diagnostics = Rc::new(ExtHostDiagnostics::new(
            rpc_protocol.as_ref(),
            NullLogService::shared(),
            Rc::new(ExtHostFileSystemInfo::new()),
            ext_host_documents_and_editors.clone(),
        ));
        rpc_protocol.set(&ExtHostContext::EXT_HOST_DIAGNOSTICS, diagnostics.clone() as Rc<dyn je_workbench::api::common::ext_host_protocol::ExtHostDiagnosticsShape>);

        // O lado do workbench primeiro: ele só pede o proxy do host quando um provedor é registrado.
        let main_thread = MainThreadLanguageFeatures::new(
            rpc_protocol.clone(),
            None,
            None,
            language_features_service.clone(),
            Rc::new(IdentityUriIdentityService) as Rc<dyn IUriIdentityService>,
            None,
        );
        rpc_protocol.set(&MainContext::MAIN_THREAD_LANGUAGE_FEATURES, main_thread.clone() as Rc<dyn MainThreadLanguageFeaturesShape>);

        let ext_host = ExtHostLanguageFeatures::new(
            rpc_protocol.as_ref(),
            None,
            ext_host_documents,
            commands.clone(),
            diagnostics,
            NullLogService::shared(),
            Rc::new(NullApiDeprecationService),
            Rc::new(TestExtHostTelemetry),
        );
        rpc_protocol.set(&ExtHostContext::EXT_HOST_LANGUAGE_FEATURES, ext_host.clone() as Rc<dyn ExtHostLanguageFeaturesShape>);

        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        // `originalErrorHandler = errorHandler.getUnexpectedErrorHandler(); setUnexpectedErrorHandler(() => { });`
        je_base::errors::set_unexpected_error_handler(|_| {});
        Env { model, insta, ext_host, main_thread, main_thread_commands, outline_service, commands, rpc_protocol, disposables: Vec::new(), guard: Some(guard) }
    }

    /// `disposables.push(value)`.
    fn add(&mut self, disposable: DisposableRef) {
        self.disposables.push(disposable);
    }

    /// `commands.executeCommand<T>(id, ...args)`.
    async fn execute(&self, id: &str, args: Vec<Dynamic>) -> Result<Dynamic, Error> {
        self.commands.execute_command(id, args).await
    }

    /// `rpcProtocol.sync()`.
    async fn sync(&self) {
        self.rpc_protocol.sync().await.expect("sync");
    }

    /// `teardown(() => { disposables = dispose(disposables); return rpcProtocol.sync(); })` e o `suiteTeardown`.
    async fn teardown(self) {
        // O timer de 100 ms que descarta o resultado de `_executeCodeLensProvider` dispara antes de o registro do provedor ser
        // descartado (no mocha o relógio é o real e o caso seguinte demora mais do que isso): o adaptador ainda existe quando o
        // `$releaseCodeLenses` chega ao host.
        timeout_with_token(100, &CancellationToken::None).await.expect("timeout");
        for disposable in &self.disposables {
            disposable.dispose();
        }
        self.sync().await;
        // `suiteTeardown`
        self.model.dispose();
        self.main_thread.dispose();
        self.outline_service.dispose();
        self.main_thread_commands.dispose();
        self.insta.dispose();
        // `setUnexpectedErrorHandler(originalErrorHandler)`: de volta ao coletor do runner, e só depois a conferência de vazamentos.
        common::record_unexpected_errors();
        drop(self.guard);
    }
}

// ---- auxiliares -----------------------------------------------------------------------------------------

/// `new types.Range(startLine, startCharacter, endLine, endCharacter)`.
fn range(start_line: f64, start_character: f64, end_line: f64, end_character: f64) -> types::Range {
    types::Range::new(types::Position::new(start_line, start_character).expect("Position"), types::Position::new(end_line, end_character).expect("Position"))
}

/// `new types.Position(line, character)` como o valor que a extensão passa ao comando.
fn position(line: f64, character: f64) -> Dynamic {
    types::Position::new(line, character).expect("Position").to_dynamic()
}

/// `new types.SymbolInformation(name, kind, range)`.
fn symbol_information(name: &str, kind: types::SymbolKind, range: types::Range) -> types::SymbolInformation {
    types::SymbolInformation::from_range(name, kind, range, None, None).expect("SymbolInformation")
}

/// `new types.Location(uri, range)`.
fn api_location(uri: &URI, range: types::Range) -> types::Location {
    types::Location::new(Some(uri.clone()), Some(types::RangeOrPosition::Range(range)))
}

/// `provideX(): any { return value; }`.
fn found<T: Clone + 'static>(value: T) -> ApiProviderResult<T> {
    MaybePromise::Value(Some(value))
}

/// O elemento de uma lista (`Array`) do resultado.
fn items(value: &Dynamic) -> Vec<Dynamic> {
    match value {
        Dynamic::Array(items) => items.to_vec(),
        other => panic!("esperava uma lista, veio {}", other.type_of()),
    }
}

/// O `number` de uma propriedade.
fn number(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(number) => *number,
        other => panic!("esperava um número, veio {}", other.type_of()),
    }
}

/// O `string` de uma propriedade.
fn text(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava um texto, veio {}", other.type_of()),
    }
}

/// `value instanceof Class` das classes que o porte não tem em `CLASSES`: a marca `$$class`.
fn has_class(value: &Dynamic, class: &str) -> bool {
    matches!(value.get_str("$$class"), Dynamic::String(name) if name.to_string_lossy() == class)
}

/// `range.start.line`, `range.start.character`, `range.end.line` e `range.end.character`.
fn assert_range(value: &Dynamic, start_line: f64, start_character: f64, end_line: f64, end_character: f64) {
    let (start, end) = (value.get_str("start"), value.get_str("end"));
    assert_eq!(number(&start.get_str("line")), start_line);
    assert_eq!(number(&start.get_str("character")), start_character);
    assert_eq!(number(&end.get_str("line")), end_line);
    assert_eq!(number(&end.get_str("character")), end_character);
}

/// `isLocation(value)`: `candidate.uri instanceof URI && candidate.range instanceof types.Range`.
fn is_location(value: &Dynamic) -> bool {
    is_instance_of(&value.get_str("uri"), "Uri") && is_instance_of(&value.get_str("range"), "Range")
}

/// `assertRejects(() => commands.executeCommand(id, ...args))`.
async fn assert_rejects(env: &Env, id: &str, args: Vec<Dynamic>) {
    assert!(env.execute(id, args).await.is_err(), "Expected rejection");
}

/// Os quatro `assertRejects` dos casos `..., invalid arguments`: sem argumento, `null`, `undefined` e `true, false`.
async fn assert_rejects_invalid_arguments(env: &Env, id: &str) {
    assert_rejects(env, id, vec![]).await;
    assert_rejects(env, id, vec![Dynamic::Null]).await;
    assert_rejects(env, id, vec![Dynamic::Undefined]).await;
    assert_rejects(env, id, vec![Dynamic::Bool(true), Dynamic::Bool(false)]).await;
}

/// O provedor de localização (`provideDefinition`, `provideDeclaration`, `provideTypeDefinition` e `provideImplementation`).
struct LocationProvider(Box<dyn Fn() -> ApiProviderResult<ApiLocationResult>>);

impl LocationProvider {
    fn new(provide: impl Fn() -> ApiProviderResult<ApiLocationResult> + 'static) -> Rc<Self> {
        Rc::new(Self(Box::new(provide)))
    }
}

impl ApiLocationLinkProvider for LocationProvider {
    fn provide_location_links(&self, _document: &TextDocument, _position: &types::Position, _token: CancellationToken) -> ApiProviderResult<ApiLocationResult> {
        (self.0)()
    }
}

/// `registerDefinitionProvider`, `registerDeclarationProvider`, `registerTypeDefinitionProvider` e `registerImplementationProvider`.
type RegisterLocationProvider = fn(&ExtHostLanguageFeatures, &ExtensionDescription, &DocumentSelectorInput, Rc<dyn ApiLocationLinkProvider>) -> DisposableRef;

/// `provideX(doc) { return new types.Location(doc.uri, new types.Range(1, 0, 0, 0)); }` (um valor só, depois outro igual, depois três).
async fn location_back_and_forth(register: RegisterLocationProvider, command_id: &str) {
    let mut env = Env::new();
    let extension = null_extension_description();
    let uri = env.model.uri();

    let first_uri = uri.clone();
    let registration = register(
        &env.ext_host,
        &extension,
        &default_selector(),
        LocationProvider::new(move || found(ApiLocationResult::Single(LocationOrLink::Location(api_location(&first_uri, range(1.0, 0.0, 0.0, 0.0)))))),
    );
    env.add(registration);
    // duplicate result will get removed
    let second_uri = uri.clone();
    let registration = register(
        &env.ext_host,
        &extension,
        &default_selector(),
        LocationProvider::new(move || found(ApiLocationResult::Single(LocationOrLink::Location(api_location(&second_uri, range(1.0, 0.0, 0.0, 0.0)))))),
    );
    env.add(registration);
    let third_uri = uri.clone();
    let registration = register(
        &env.ext_host,
        &extension,
        &default_selector(),
        LocationProvider::new(move || {
            found(ApiLocationResult::Many(
                [range(2.0, 0.0, 0.0, 0.0), range(3.0, 0.0, 0.0, 0.0), range(4.0, 0.0, 0.0, 0.0)]
                    .into_iter()
                    .map(|range| LocationOrLink::Location(api_location(&third_uri, range)))
                    .collect(),
            ))
        }),
    );
    env.add(registration);

    env.sync().await;
    let values = env.execute(command_id, vec![uri.to_dynamic(), position(0.0, 0.0)]).await.expect("executeCommand");
    let values = items(&values);
    assert_eq!(values.len(), 4);
    for value in &values {
        assert!(is_instance_of(&value.get_str("uri"), "Uri"));
        assert!(is_instance_of(&value.get_str("range"), "Range"));
    }
    env.teardown().await;
}

/// O caso `... Link`: um `Location` e um `LocationLink` literal (`targetUri`, `targetRange`, `targetSelectionRange`, `originSelectionRange`).
async fn location_link(register: RegisterLocationProvider, command_id: &str) {
    let mut env = Env::new();
    let uri = env.model.uri();

    let provider_uri = uri.clone();
    let registration = register(
        &env.ext_host,
        &null_extension_description(),
        &default_selector(),
        LocationProvider::new(move || {
            found(ApiLocationResult::Many(vec![
                LocationOrLink::Location(api_location(&provider_uri, range(0.0, 0.0, 0.0, 0.0))),
                LocationOrLink::Link(DefinitionLink {
                    origin_selection_range: Some(range(2.0, 2.0, 2.0, 2.0)),
                    target_uri: provider_uri.clone(),
                    target_range: range(1.0, 0.0, 0.0, 0.0),
                    target_selection_range: Some(range(1.0, 1.0, 1.0, 1.0)),
                }),
            ]))
        }),
    );
    env.add(registration);

    env.sync().await;
    let values = env.execute(command_id, vec![uri.to_dynamic(), position(0.0, 0.0)]).await.expect("executeCommand");
    let values = items(&values);
    assert_eq!(values.len(), 2);
    for value in &values {
        if is_location(value) {
            assert!(is_instance_of(&value.get_str("uri"), "Uri"));
            assert!(is_instance_of(&value.get_str("range"), "Range"));
        } else {
            assert!(is_instance_of(&value.get_str("targetUri"), "Uri"));
            assert!(is_instance_of(&value.get_str("targetRange"), "Range"));
            assert!(is_instance_of(&value.get_str("targetSelectionRange"), "Range"));
            assert!(is_instance_of(&value.get_str("originSelectionRange"), "Range"));
        }
    }
    env.teardown().await;
}

// ---- workspace symbols ----------------------------------------------------------------------------------

/// `provideWorkspaceSymbols(query) { return [...]; }`.
struct WorkspaceSymbolProvider(Box<dyn Fn(&str) -> ApiProviderResult<Vec<types::SymbolInformation>>>);

impl ApiWorkspaceSymbolProvider for WorkspaceSymbolProvider {
    fn provide_workspace_symbols(&self, query: &str, _token: CancellationToken) -> ApiProviderResult<Vec<types::SymbolInformation>> {
        (self.0)(query)
    }
}

#[test]
fn workspace_symbols_invalid_arguments() {
    run_with_faked_timers(async move {
        let env = Env::new();
        assert_rejects(&env, "vscode.executeWorkspaceSymbolProvider", vec![]).await;
        assert_rejects(&env, "vscode.executeWorkspaceSymbolProvider", vec![Dynamic::Null]).await;
        assert_rejects(&env, "vscode.executeWorkspaceSymbolProvider", vec![Dynamic::Undefined]).await;
        assert_rejects(&env, "vscode.executeWorkspaceSymbolProvider", vec![Dynamic::Bool(true)]).await;
        env.teardown().await;
    });
}

#[test]
fn workspace_symbols_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_workspace_symbol_provider(
            &null_extension_description(),
            Rc::new(WorkspaceSymbolProvider(Box::new(|query| {
                found(vec![
                    types::SymbolInformation::from_range(query, types::SymbolKind::Array, range(0.0, 0.0, 1.0, 1.0), Some(URI::parse("far://testing/first", false)), None)
                        .expect("SymbolInformation"),
                    types::SymbolInformation::from_range(query, types::SymbolKind::Array, range(0.0, 0.0, 1.0, 1.0), Some(URI::parse("far://testing/second", false)), None)
                        .expect("SymbolInformation"),
                ])
            }))),
        );
        env.add(registration);

        let registration = env.ext_host.register_workspace_symbol_provider(
            &null_extension_description(),
            Rc::new(WorkspaceSymbolProvider(Box::new(|query| {
                found(vec![types::SymbolInformation::from_range(
                    query,
                    types::SymbolKind::Array,
                    range(0.0, 0.0, 1.0, 1.0),
                    Some(URI::parse("far://testing/first", false)),
                    None,
                )
                .expect("SymbolInformation")])
            }))),
        );
        env.add(registration);

        env.sync().await;
        let value = env.execute("vscode.executeWorkspaceSymbolProvider", vec![Dynamic::string("testing")]).await.expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 2); // de-duped
        for info in &value {
            assert!(has_class(info, "SymbolInformation"));
            assert_eq!(text(&info.get_str("name")), "testing");
            assert_eq!(number(&info.get_str("kind")), f64::from(types::SymbolKind::Array.value()));
        }
        env.teardown().await;
    });
}

#[test]
fn execute_workspace_symbol_provider_should_accept_empty_string_39522() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_workspace_symbol_provider(
            &null_extension_description(),
            Rc::new(WorkspaceSymbolProvider(Box::new(|_query| {
                found(vec![
                    types::SymbolInformation::from_range("hello", types::SymbolKind::Array, range(0.0, 0.0, 0.0, 0.0), Some(URI::parse("foo:bar", false)), None)
                        .expect("SymbolInformation"),
                ])
            }))),
        );
        env.add(registration);

        env.sync().await;
        let symbols = env.execute("vscode.executeWorkspaceSymbolProvider", vec![Dynamic::string("")]).await.expect("executeCommand");
        assert_eq!(items(&symbols).len(), 1);

        env.sync().await;
        let symbols = env.execute("vscode.executeWorkspaceSymbolProvider", vec![Dynamic::string("*")]).await.expect("executeCommand");
        assert_eq!(items(&symbols).len(), 1);
        env.teardown().await;
    });
}

// ---- formatting -----------------------------------------------------------------------------------------

/// `provideDocumentFormattingEdits() { return [types.TextEdit.insert(new types.Position(0, 0), '42')]; }`.
struct InsertFortyTwoFormatter;

impl ApiDocumentFormattingEditProvider for InsertFortyTwoFormatter {
    fn provide_document_formatting_edits(&self, _document: &TextDocument, _options: &ApiFormattingOptions, _token: CancellationToken) -> ApiProviderResult<Vec<types::TextEdit>> {
        found(vec![types::TextEdit::insert(types::Position::new(0.0, 0.0).expect("Position"), "42")])
    }
}

#[test]
fn execute_format_document_provider_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_document_formatting_edit_provider(&null_extension_description(), &default_selector(), Rc::new(InsertFortyTwoFormatter));
        env.add(registration);

        env.sync().await;
        // `{ insertSpaces: false, tabSize: 4 } satisfies FormattingOptions`
        let options = Dynamic::object([("insertSpaces", Dynamic::Bool(false)), ("tabSize", Dynamic::Number(4.0))]);
        let edits = env.execute("vscode.executeFormatDocumentProvider", vec![env.model.uri().to_dynamic(), options]).await.expect("executeCommand");
        assert_eq!(items(&edits).len(), 1);
        env.teardown().await;
    });
}

// ---- rename ---------------------------------------------------------------------------------------------

/// O `RenameProvider` com `prepareRename` do caso `vscode.prepareRename`.
struct PrepareRenameProvider {
    uri: URI,
}

impl ApiRenameProvider for PrepareRenameProvider {
    fn provide_rename_edits(&self, _document: &TextDocument, position: &types::Position, new_name: &str, _token: CancellationToken) -> ApiProviderResult<types::WorkspaceEdit> {
        // `edit.insert(document.uri, position, newName)`
        let mut edit = types::WorkspaceEdit::new();
        edit.replace(&self.uri, types::Range::new(*position, *position), new_name, None);
        found(edit)
    }

    fn has_prepare_rename(&self) -> bool {
        true
    }

    fn prepare_rename(&self, _document: &TextDocument, _position: &types::Position, _token: CancellationToken) -> ApiProviderResult<ApiPrepareRename> {
        found(ApiPrepareRename::RangeWithPlaceholder { range: range(0.0, 12.0, 0.0, 24.0), placeholder: Some("foooPlaceholder".to_owned()) })
    }
}

#[test]
fn vscode_prepare_rename() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_rename_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(PrepareRenameProvider { uri: env.model.uri() }),
        );
        env.add(registration);

        env.sync().await;

        let data = env.execute("vscode.prepareRename", vec![env.model.uri().to_dynamic(), position(0.0, 12.0)]).await.expect("executeCommand");

        assert!(!data.is_nullish());
        assert_eq!(text(&data.get_str("placeholder")), "foooPlaceholder");
        assert_range(&data.get_str("range"), 0.0, 12.0, 0.0, 24.0);
        env.teardown().await;
    });
}

/// `new class implements vscode.RenameProvider { provideRenameEdits(document, position, newName) { ...insert...; return edit; } }`.
struct InsertRenameProvider;

impl ApiRenameProvider for InsertRenameProvider {
    fn provide_rename_edits(&self, document: &TextDocument, position: &types::Position, new_name: &str, _token: CancellationToken) -> ApiProviderResult<types::WorkspaceEdit> {
        let mut edit = types::WorkspaceEdit::new();
        edit.insert(document.uri(), *position, new_name, None);
        found(edit)
    }
}

#[test]
fn vscode_execute_document_rename_provider() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_rename_provider(&null_extension_description(), &default_selector(), Rc::new(InsertRenameProvider));
        env.add(registration);

        env.sync().await;

        let edit = env
            .execute("vscode.executeDocumentRenameProvider", vec![env.model.uri().to_dynamic(), position(0.0, 12.0), Dynamic::string("newNameOfThis")])
            .await
            .expect("executeCommand");

        assert!(!edit.is_nullish());
        let uri = env.model.uri().to_dynamic();
        let has = edit.get_str("has");
        let Dynamic::Function(has) = &has else { panic!("esperava uma função, veio {}", has.type_of()) };
        assert!(matches!(has.call(&[uri.clone()]), Dynamic::Bool(true)));
        let get = edit.get_str("get");
        let Dynamic::Function(get) = &get else { panic!("esperava uma função, veio {}", get.type_of()) };
        let text_edits = items(&get.call(&[uri]));
        assert_eq!(text_edits.len(), 1);
        assert_eq!(text(&text_edits[0].get_str("newText")), "newNameOfThis");
        env.teardown().await;
    });
}

// ---- definition -----------------------------------------------------------------------------------------

#[test]
fn definition_invalid_arguments() {
    run_with_faked_timers(async move {
        let env = Env::new();
        assert_rejects_invalid_arguments(&env, "vscode.executeDefinitionProvider").await;
        env.teardown().await;
    });
}

#[test]
fn definition_back_and_forth() {
    run_with_faked_timers(location_back_and_forth(ExtHostLanguageFeatures::register_definition_provider, "vscode.executeDefinitionProvider"));
}

#[test]
fn definition_back_and_forth_sorting_and_de_duping() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_definition_provider(
            &null_extension_description(),
            &default_selector(),
            LocationProvider::new(|| {
                found(ApiLocationResult::Single(LocationOrLink::Location(api_location(&URI::parse("file:///b", false), range(1.0, 0.0, 0.0, 0.0)))))
            }),
        );
        env.add(registration);
        // duplicate result will get removed
        let registration = env.ext_host.register_definition_provider(
            &null_extension_description(),
            &default_selector(),
            LocationProvider::new(|| {
                found(ApiLocationResult::Single(LocationOrLink::Location(api_location(&URI::parse("file:///b", false), range(1.0, 0.0, 0.0, 0.0)))))
            }),
        );
        env.add(registration);
        let registration = env.ext_host.register_definition_provider(
            &null_extension_description(),
            &default_selector(),
            LocationProvider::new(|| {
                found(ApiLocationResult::Many(vec![
                    LocationOrLink::Location(api_location(&URI::parse("file:///a", false), range(2.0, 0.0, 0.0, 0.0))),
                    LocationOrLink::Location(api_location(&URI::parse("file:///c", false), range(3.0, 0.0, 0.0, 0.0))),
                    LocationOrLink::Location(api_location(&URI::parse("file:///d", false), range(4.0, 0.0, 0.0, 0.0))),
                ]))
            }),
        );
        env.add(registration);

        env.sync().await;
        let values = env.execute("vscode.executeDefinitionProvider", vec![env.model.uri().to_dynamic(), position(0.0, 0.0)]).await.expect("executeCommand");
        let values = items(&values);
        assert_eq!(values.len(), 4);

        let path = |value: &Dynamic| text(&value.get_str("uri").get_str("path"));
        assert_eq!(path(&values[0]), "/a");
        assert_eq!(path(&values[1]), "/b");
        assert_eq!(path(&values[2]), "/c");
        assert_eq!(path(&values[3]), "/d");
        env.teardown().await;
    });
}

#[test]
fn definition_link() {
    run_with_faked_timers(location_link(ExtHostLanguageFeatures::register_definition_provider, "vscode.executeDefinitionProvider"));
}

// ---- declaration ----------------------------------------------------------------------------------------

#[test]
fn declaration_back_and_forth() {
    run_with_faked_timers(location_back_and_forth(ExtHostLanguageFeatures::register_declaration_provider, "vscode.executeDeclarationProvider"));
}

#[test]
fn declaration_link() {
    run_with_faked_timers(location_link(ExtHostLanguageFeatures::register_declaration_provider, "vscode.executeDeclarationProvider"));
}

// ---- type definition ------------------------------------------------------------------------------------

#[test]
fn type_definition_invalid_arguments() {
    run_with_faked_timers(async move {
        let env = Env::new();
        assert_rejects_invalid_arguments(&env, "vscode.executeTypeDefinitionProvider").await;
        env.teardown().await;
    });
}

#[test]
fn type_definition_back_and_forth() {
    run_with_faked_timers(location_back_and_forth(ExtHostLanguageFeatures::register_type_definition_provider, "vscode.executeTypeDefinitionProvider"));
}

#[test]
fn type_definition_link() {
    run_with_faked_timers(location_link(ExtHostLanguageFeatures::register_type_definition_provider, "vscode.executeTypeDefinitionProvider"));
}

// ---- implementation -------------------------------------------------------------------------------------

#[test]
fn implementation_invalid_arguments() {
    run_with_faked_timers(async move {
        let env = Env::new();
        assert_rejects_invalid_arguments(&env, "vscode.executeImplementationProvider").await;
        env.teardown().await;
    });
}

#[test]
fn implementation_back_and_forth() {
    run_with_faked_timers(location_back_and_forth(ExtHostLanguageFeatures::register_implementation_provider, "vscode.executeImplementationProvider"));
}

#[test]
fn implementation_definition_link() {
    run_with_faked_timers(location_link(ExtHostLanguageFeatures::register_implementation_provider, "vscode.executeImplementationProvider"));
}

// ---- references -----------------------------------------------------------------------------------------

/// `provideReferences() { return [new types.Location(URI.parse('some:uri/path'), new types.Range(0, 1, 0, 5))]; }`.
struct SingleReferenceProvider;

impl ApiReferenceProvider for SingleReferenceProvider {
    fn provide_references(&self, _document: &TextDocument, _position: &types::Position, _context: &ReferenceContext, _token: CancellationToken) -> ApiProviderResult<Vec<types::Location>> {
        found(vec![api_location(&URI::parse("some:uri/path", false), range(0.0, 1.0, 0.0, 5.0))])
    }
}

#[test]
fn reference_search_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_reference_provider(&null_extension_description(), &default_selector(), Rc::new(SingleReferenceProvider));
        env.add(registration);

        let values = env.execute("vscode.executeReferenceProvider", vec![env.model.uri().to_dynamic(), position(0.0, 0.0)]).await.expect("executeCommand");
        let values = items(&values);
        assert_eq!(values.len(), 1);
        let first = &values[0];
        assert_eq!(URI::from_dynamic(&first.get_str("uri")).expect("Uri").to_string(false).to_string_lossy(), "some:uri/path");
        assert_range(&first.get_str("range"), 0.0, 1.0, 0.0, 5.0);
        env.teardown().await;
    });
}

// ---- document highlights --------------------------------------------------------------------------------

/// `provideDocumentHighlights() { return [new types.DocumentHighlight(new types.Range(0, 17, 0, 25), types.DocumentHighlightKind.Read)]; }`.
struct ReadHighlightProvider;

impl ApiDocumentHighlightProvider for ReadHighlightProvider {
    fn provide_document_highlights(&self, _document: &TextDocument, _position: &types::Position, _token: CancellationToken) -> ApiProviderResult<Vec<types::DocumentHighlight>> {
        found(vec![types::DocumentHighlight::new(range(0.0, 17.0, 0.0, 25.0), Some(types::DocumentHighlightKind::Read))])
    }
}

#[test]
fn execute_document_highlights_api_has_stopped_returning_document_highlight_array_200056() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_document_highlight_provider(&null_extension_description(), &default_selector(), Rc::new(ReadHighlightProvider));
        env.add(registration);

        env.sync().await;

        let values = env.execute("vscode.executeDocumentHighlights", vec![env.model.uri().to_dynamic(), position(0.0, 0.0)]).await.expect("executeCommand");
        assert!(matches!(values, Dynamic::Array(_)));
        let values = items(&values);
        assert_eq!(values.len(), 1);
        let first = &values[0];
        assert_range(&first.get_str("range"), 0.0, 17.0, 0.0, 25.0);
        env.teardown().await;
    });
}

// ---- outline --------------------------------------------------------------------------------------------

/// `provideDocumentSymbols(): any { return ...; }`.
struct SymbolProvider(Box<dyn Fn() -> ApiProviderResult<ApiDocumentSymbols>>);

impl ApiDocumentSymbolProvider for SymbolProvider {
    fn provide_document_symbols(&self, _document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<ApiDocumentSymbols> {
        (self.0)()
    }
}

#[test]
fn outline_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_document_symbol_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SymbolProvider(Box::new(|| {
                found(ApiDocumentSymbols::Information(vec![
                    symbol_information("testing1", types::SymbolKind::Enum, range(1.0, 0.0, 1.0, 0.0)),
                    symbol_information("testing2", types::SymbolKind::Enum, range(0.0, 1.0, 0.0, 3.0)),
                ]))
            }))),
            None,
        );
        env.add(registration);

        env.sync().await;
        let values = env.execute("vscode.executeDocumentSymbolProvider", vec![env.model.uri().to_dynamic()]).await.expect("executeCommand");
        let values = items(&values);
        assert_eq!(values.len(), 2);
        let (first, second) = (&values[0], &values[1]);
        assert!(has_class(first, "SymbolInformation"));
        assert!(has_class(second, "SymbolInformation"));
        assert_eq!(text(&first.get_str("name")), "testing2");
        assert_eq!(text(&second.get_str("name")), "testing1");
        env.teardown().await;
    });
}

#[test]
fn execute_document_symbol_provider_command_only_returns_symbol_information_array_rather_than_document_symbol_array_57984() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_document_symbol_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SymbolProvider(Box::new(|| {
                found(ApiDocumentSymbols::Information(vec![symbol_information("SymbolInformation", types::SymbolKind::Enum, range(1.0, 0.0, 1.0, 0.0))]))
            }))),
            None,
        );
        env.add(registration);
        let registration = env.ext_host.register_document_symbol_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SymbolProvider(Box::new(|| {
                let mut root = types::DocumentSymbol::new("DocumentSymbol", "DocumentSymbol#detail", types::SymbolKind::Enum, range(1.0, 0.0, 1.0, 0.0), range(1.0, 0.0, 1.0, 0.0))
                    .expect("DocumentSymbol");
                root.children = vec![
                    types::DocumentSymbol::new(
                        "DocumentSymbol#child",
                        "DocumentSymbol#detail#child",
                        types::SymbolKind::Enum,
                        range(1.0, 0.0, 1.0, 0.0),
                        range(1.0, 0.0, 1.0, 0.0),
                    )
                    .expect("DocumentSymbol"),
                ];
                found(ApiDocumentSymbols::Symbols(vec![root]))
            }))),
            None,
        );
        env.add(registration);

        env.sync().await;
        let values = env.execute("vscode.executeDocumentSymbolProvider", vec![env.model.uri().to_dynamic()]).await.expect("executeCommand");
        let values = items(&values);
        assert_eq!(values.len(), 2);
        let (first, second) = (&values[0], &values[1]);
        assert!(has_class(first, "SymbolInformation"));
        assert!(!has_class(first, "DocumentSymbol"));
        assert!(has_class(second, "SymbolInformation"));
        assert_eq!(text(&first.get_str("name")), "DocumentSymbol");
        assert_eq!(items(&first.get_str("children")).len(), 1);
        assert_eq!(text(&second.get_str("name")), "SymbolInformation");
        env.teardown().await;
    });
}

// ---- suggest --------------------------------------------------------------------------------------------

/// `provideCompletionItems(_doc, _pos, _tok, context): any { return ...; }` e, quando existe, `resolveCompletionItem(item) { ...; return item; }`.
struct CompletionsProvider {
    provide: Box<dyn Fn(&types::CompletionContext) -> ApiCompletionResult>,
    resolve: Option<Box<dyn Fn()>>,
}

impl CompletionsProvider {
    fn new(provide: impl Fn(&types::CompletionContext) -> ApiCompletionResult + 'static) -> Rc<Self> {
        Rc::new(Self { provide: Box::new(provide), resolve: None })
    }
}

impl ApiCompletionItemProvider for CompletionsProvider {
    fn provide_completion_items(
        &self,
        _document: &TextDocument,
        _position: &types::Position,
        _token: CancellationToken,
        context: &types::CompletionContext,
    ) -> ApiProviderResult<ApiCompletionResult> {
        found((self.provide)(context))
    }

    fn has_resolve_completion_item(&self) -> bool {
        self.resolve.is_some()
    }

    fn resolve_completion_item(&self, item: &CompletionItemRef, _token: CancellationToken) -> ApiProviderResult<CompletionItemRef> {
        if let Some(resolve) = &self.resolve {
            resolve();
        }
        found(Rc::clone(item))
    }
}

/// `new types.CompletionItem(label, kind)`.
fn completion_item(label: &str, kind: Option<types::CompletionItemKind>) -> CompletionItemRef {
    Rc::new(RefCell::new(types::CompletionItem::new(types::CompletionLabel::Text(label.to_owned()), kind)))
}

/// `commands.executeCommand<vscode.CompletionList>('vscode.executeCompletionItemProvider', model.uri, new types.Position(0, 4), ...args)`.
async fn execute_completion_item_provider(env: &Env, extra_args: Vec<Dynamic>) -> Dynamic {
    let mut args = vec![env.model.uri().to_dynamic(), position(0.0, 4.0)];
    args.extend(extra_args);
    env.execute("vscode.executeCompletionItemProvider", args).await.expect("executeCommand")
}

/// `list.items`.
fn completion_list_items(list: &Dynamic) -> Vec<Dynamic> {
    assert!(has_class(list, "CompletionList"));
    items(&list.get_str("items"))
}

#[test]
fn trigger_character_is_null_when_completion_provider_is_called_programmatically_159914() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let actual_context: Rc<RefCell<Option<types::CompletionContext>>> = Rc::default();
        let captured = Rc::clone(&actual_context);
        let registration = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            CompletionsProvider::new(move |context| {
                *captured.borrow_mut() = Some(context.clone());
                ApiCompletionResult::Items(Vec::new())
            }),
            Vec::new(),
        );
        env.add(registration);

        env.sync().await;

        execute_completion_item_provider(&env, vec![]).await;

        let actual_context = actual_context.borrow().clone().expect("o provedor recebeu o contexto");
        assert_eq!(actual_context, types::CompletionContext { trigger_kind: types::CompletionTriggerKind::Invoke, trigger_character: None });
        env.teardown().await;
    });
}

#[test]
fn suggest_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            CompletionsProvider::new(|_| {
                let a = completion_item("item1", None);
                a.borrow_mut().documentation = Some(types::Documentation::Markdown(types::MarkdownString::new(Some("hello_md_string"), false)));
                let b = completion_item("item2", None);
                // overwite after
                b.borrow_mut().text_edit = Some(types::TextEdit::replace(range(0.0, 4.0, 0.0, 8.0), "foo"));
                let c = completion_item("item3", None);
                // overwite before & after
                c.borrow_mut().text_edit = Some(types::TextEdit::replace(range(0.0, 1.0, 0.0, 6.0), "foobar"));

                // snippet string!
                let d = completion_item("item4", None);
                // overwite before
                d.borrow_mut().range = Some(types::CompletionRange::Range(range(0.0, 1.0, 0.0, 4.0)));
                d.borrow_mut().insert_text = Some(types::InsertText::Snippet(types::SnippetString::new(Some("foo$0bar"))));
                ApiCompletionResult::Items(vec![a, b, c, d])
            }),
            Vec::new(),
        );
        env.add(registration);

        env.sync().await;

        let list = execute_completion_item_provider(&env, vec![]).await;
        let values = completion_list_items(&list);
        assert_eq!(values.len(), 4);
        let (first, second, third, fourth) = (&values[0], &values[1], &values[2], &values[3]);
        assert_eq!(text(&first.get_str("label")), "item1");
        assert!(matches!(first.get_str("textEdit"), Dynamic::Undefined)); // no text edit, default ranges
        assert!(!is_instance_of(&first.get_str("range"), "Range"));
        assert_eq!(text(&first.get_str("documentation").get_str("value")), "hello_md_string");
        assert_eq!(text(&second.get_str("label")), "item2");
        assert_eq!(text(&second.get_str("textEdit").get_str("newText")), "foo");
        assert_range(&second.get_str("textEdit").get_str("range"), 0.0, 4.0, 0.0, 8.0);
        assert_eq!(text(&third.get_str("label")), "item3");
        assert_eq!(text(&third.get_str("textEdit").get_str("newText")), "foobar");
        assert_range(&third.get_str("textEdit").get_str("range"), 0.0, 1.0, 0.0, 6.0);
        assert_eq!(text(&fourth.get_str("label")), "item4");
        assert!(matches!(fourth.get_str("textEdit"), Dynamic::Undefined));
        let fourth_range = fourth.get_str("range");
        assert!(is_instance_of(&fourth_range, "Range"));
        assert_range(&fourth_range, 0.0, 1.0, 0.0, 4.0);
        let insert_text = fourth.get_str("insertText");
        assert!(is_instance_of(&insert_text, "SnippetString"));
        assert_eq!(text(&insert_text.get_str("value")), "foo$0bar");
        env.teardown().await;
    });
}

#[test]
fn suggest_return_completion_list_not_array() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            CompletionsProvider::new(|_| {
                let a = completion_item("item1", None);
                let b = completion_item("item2", None);
                ApiCompletionResult::List(ApiCompletionList { is_incomplete: Some(true), items: vec![a, b] })
            }),
            Vec::new(),
        );
        env.add(registration);

        env.sync().await;

        let list = execute_completion_item_provider(&env, vec![]).await;

        assert!(has_class(&list, "CompletionList"));
        assert!(matches!(list.get_str("isIncomplete"), Dynamic::Bool(true)));
        env.teardown().await;
    });
}

#[test]
fn suggest_resolve_completion_items() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let resolve_count = Rc::new(Cell::new(0_u32));
        let counter = Rc::clone(&resolve_count);
        let registration = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CompletionsProvider {
                provide: Box::new(|_| {
                    let items = ["item1", "item2", "item3", "item4"].map(|label| completion_item(label, None));
                    ApiCompletionResult::List(ApiCompletionList { is_incomplete: Some(false), items: items.to_vec() })
                }),
                resolve: Some(Box::new(move || counter.set(counter.get() + 1))),
            }),
            Vec::new(),
        );
        env.add(registration);

        env.sync().await;

        let list = execute_completion_item_provider(
            &env,
            vec![
                Dynamic::Undefined,
                Dynamic::Number(2.0), // maxItemsToResolve
            ],
        )
        .await;

        assert!(has_class(&list, "CompletionList"));
        assert_eq!(resolve_count.get(), 2);
        env.teardown().await;
    });
}

#[test]
fn execute_completion_item_provider_returns_a_preselect_field_53749() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            CompletionsProvider::new(|_| {
                let a = completion_item("item1", None);
                a.borrow_mut().preselect = Some(true);
                let b = completion_item("item2", None);
                let c = completion_item("item3", None);
                c.borrow_mut().preselect = Some(true);
                let d = completion_item("item4", None);
                ApiCompletionResult::List(ApiCompletionList { is_incomplete: Some(false), items: vec![a, b, c, d] })
            }),
            Vec::new(),
        );
        env.add(registration);

        env.sync().await;

        let list = execute_completion_item_provider(&env, vec![Dynamic::Undefined]).await;

        let values = completion_list_items(&list);
        assert_eq!(values.len(), 4);

        let (a, b, c, d) = (&values[0], &values[1], &values[2], &values[3]);
        assert!(matches!(a.get_str("preselect"), Dynamic::Bool(true)));
        assert!(matches!(b.get_str("preselect"), Dynamic::Undefined));
        assert!(matches!(c.get_str("preselect"), Dynamic::Bool(true)));
        assert!(matches!(d.get_str("preselect"), Dynamic::Undefined));
        env.teardown().await;
    });
}

#[test]
fn execute_completion_item_provider_does_not_capture_commit_characters_58228() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            CompletionsProvider::new(|_| {
                let a = completion_item("item1", None);
                a.borrow_mut().commit_characters = Some(vec!["a".to_owned(), "b".to_owned()]);
                let b = completion_item("item2", None);
                ApiCompletionResult::List(ApiCompletionList { is_incomplete: Some(false), items: vec![a, b] })
            }),
            Vec::new(),
        );
        env.add(registration);

        env.sync().await;

        let list = execute_completion_item_provider(&env, vec![Dynamic::Undefined]).await;

        let values = completion_list_items(&list);
        assert_eq!(values.len(), 2);

        let (a, b) = (&values[0], &values[1]);
        assert_eq!(items(&a.get_str("commitCharacters")).iter().map(text).collect::<Vec<_>>(), ["a", "b"]);
        assert!(matches!(b.get_str("commitCharacters"), Dynamic::Undefined));
        env.teardown().await;
    });
}

#[test]
fn execute_completion_item_provider_returns_the_wrong_completion_item_kinds_in_insiders_95715() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            CompletionsProvider::new(|_| {
                ApiCompletionResult::Items(vec![
                    completion_item("My Method", Some(types::CompletionItemKind::Method)),
                    completion_item("My Property", Some(types::CompletionItemKind::Property)),
                ])
            }),
            Vec::new(),
        );
        env.add(registration);

        env.sync().await;

        let list = execute_completion_item_provider(&env, vec![Dynamic::Undefined]).await;

        let values = completion_list_items(&list);
        assert_eq!(values.len(), 2);

        let (a, b) = (&values[0], &values[1]);
        assert_eq!(number(&a.get_str("kind")), f64::from(types::CompletionItemKind::Method.value()));
        assert_eq!(number(&b.get_str("kind")), f64::from(types::CompletionItemKind::Property.value()));
        env.teardown().await;
    });
}

// ---- signature help -------------------------------------------------------------------------------------

/// `new class implements vscode.SignatureHelpProvider { provideSignatureHelp(_document, _position, _token, context) { ... } }`.
struct InvokedSignatureHelpProvider;

impl ApiSignatureHelpProvider for InvokedSignatureHelpProvider {
    fn provide_signature_help(
        &self,
        _document: &TextDocument,
        _position: &types::Position,
        _token: CancellationToken,
        context: &ApiSignatureHelpContext,
    ) -> ApiProviderResult<ApiSignatureHelpRef> {
        let kind = if context.trigger_kind == types::SignatureHelpTriggerKind::Invoke { "invoked" } else { "unknown" };
        // `${context.triggerCharacter}` de um `undefined` é o texto "undefined"
        let documentation = format!("{kind} {}", context.trigger_character.as_deref().unwrap_or("undefined"));
        found(Rc::new(RefCell::new(types::SignatureHelp {
            active_signature: 0.0,
            active_parameter: 1.0,
            signatures: vec![types::SignatureInformation {
                label: "abc".to_owned(),
                documentation: Some(types::Documentation::Text(documentation)),
                parameters: Vec::new(),
                active_parameter: None,
            }],
        })))
    }
}

#[test]
fn parameter_hints_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_signature_help_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(InvokedSignatureHelpProvider),
            SignatureHelpMetadataOrTriggerChars::TriggerCharacters(Vec::new()),
        );
        env.add(registration);

        env.sync().await;

        let first_value = env
            .execute("vscode.executeSignatureHelpProvider", vec![env.model.uri().to_dynamic(), position(0.0, 1.0), Dynamic::string(",")])
            .await
            .expect("executeCommand");
        assert_eq!(number(&first_value.get_str("activeSignature")), 0.0);
        assert_eq!(number(&first_value.get_str("activeParameter")), 1.0);
        let signatures = items(&first_value.get_str("signatures"));
        assert_eq!(signatures.len(), 1);
        assert_eq!(text(&signatures[0].get_str("label")), "abc");
        assert_eq!(text(&signatures[0].get_str("documentation")), "invoked ,");
        env.teardown().await;
    });
}

// ---- quickfix -------------------------------------------------------------------------------------------

/// `provideCodeActions(document, rangeOrSelection): any { return [...]; }` e, quando existe, `resolveCodeAction(action) { ...; return action; }`.
struct CodeActionsProvider {
    provide: Box<dyn Fn(&TextDocument, &ApiRangeOrSelection) -> Vec<CommandOrCodeAction>>,
    resolve: Option<Box<dyn Fn(&ApiCodeActionRef)>>,
}

impl CodeActionsProvider {
    fn new(provide: impl Fn(&TextDocument, &ApiRangeOrSelection) -> Vec<CommandOrCodeAction> + 'static) -> Rc<Self> {
        Rc::new(Self { provide: Box::new(provide), resolve: None })
    }
}

impl ApiCodeActionProvider for CodeActionsProvider {
    fn provide_code_actions(
        &self,
        document: &TextDocument,
        range_or_selection: &ApiRangeOrSelection,
        _context: &ApiCodeActionContext,
        _token: CancellationToken,
    ) -> ApiProviderResult<Vec<CommandOrCodeAction>> {
        found((self.provide)(document, range_or_selection))
    }

    fn has_resolve_code_action(&self) -> bool {
        self.resolve.is_some()
    }

    fn resolve_code_action(&self, code_action: &ApiCodeActionRef, _token: CancellationToken) -> ApiProviderResult<ApiCodeActionRef> {
        if let Some(resolve) = &self.resolve {
            resolve(code_action);
        }
        found(Rc::clone(code_action))
    }
}

/// `{ command: { arguments: [document, range], command: 'command', title: 'command_title' }, kind: types.CodeActionKind.Empty.append('foo'),
/// title: 'title' }`. O `document` não é um `Dynamic`: entra o recurso dele.
fn code_action_with_command(document: &TextDocument, range_or_selection: &ApiRangeOrSelection) -> types::CodeAction {
    let mut action = types::CodeAction::new("title", Some(types::CodeActionKind::empty().append("foo")));
    let mut command = types::Command::new("command_title", "command");
    command.arguments = Some(vec![
        document.uri().to_dynamic(),
        match range_or_selection {
            ApiRangeOrSelection::Range(range) => range.to_dynamic(),
            ApiRangeOrSelection::Selection(selection) => selection.to_dynamic(),
        },
    ]);
    action.command = Some(command);
    action
}

/// `new types.Selection(0, 0, 1, 1)`.
fn selection_0_0_1_1() -> types::Selection {
    types::Selection::new(types::Position::new(0.0, 0.0).expect("Position"), types::Position::new(1.0, 1.0).expect("Position"))
}

#[test]
fn quick_fix_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            CodeActionsProvider::new(|_, _| {
                let mut command = types::Command::new("Title", "testing");
                command.arguments = Some(vec![Dynamic::from(1.0), Dynamic::from(2.0), Dynamic::Bool(true)]);
                vec![CommandOrCodeAction::Command(command)]
            }),
            None,
        );
        env.add(registration);

        env.sync().await;
        let value = env
            .execute("vscode.executeCodeActionProvider", vec![env.model.uri().to_dynamic(), range(0.0, 0.0, 1.0, 1.0).to_dynamic()])
            .await
            .expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        let first = &value[0];
        assert_eq!(text(&first.get_str("title")), "Title");
        assert_eq!(text(&first.get_str("command")), "testing");
        let arguments = items(&first.get_str("arguments"));
        assert_eq!(number(&arguments[0]), 1.0);
        assert_eq!(number(&arguments[1]), 2.0);
        assert!(matches!(arguments[2], Dynamic::Bool(true)));
        assert_eq!(arguments.len(), 3);
        env.teardown().await;
    });
}

#[test]
fn execute_code_action_provider_results_seem_to_be_missing_their_command_property_45124() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            CodeActionsProvider::new(|document, range_or_selection| {
                vec![CommandOrCodeAction::CodeAction(Rc::new(RefCell::new(code_action_with_command(document, range_or_selection))))]
            }),
            None,
        );
        env.add(registration);

        env.sync().await;
        let value = env
            .execute("vscode.executeCodeActionProvider", vec![env.model.uri().to_dynamic(), range(0.0, 0.0, 1.0, 1.0).to_dynamic()])
            .await
            .expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        let first = &value[0];
        let command = first.get_str("command");
        assert!(!command.is_nullish());
        assert_eq!(text(&command.get_str("command")), "command");
        assert_eq!(text(&command.get_str("title")), "command_title");
        assert_eq!(text(&first.get_str("kind").get_str("value")), "foo");
        assert_eq!(text(&first.get_str("title")), "title");
        env.teardown().await;
    });
}

#[test]
fn execute_code_action_provider_passes_range_to_provider_although_selection_is_passed_in_77997() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            CodeActionsProvider::new(|document, range_or_selection| {
                vec![CommandOrCodeAction::CodeAction(Rc::new(RefCell::new(code_action_with_command(document, range_or_selection))))]
            }),
            None,
        );
        env.add(registration);

        let selection = selection_0_0_1_1();

        env.sync().await;
        let value = env
            .execute("vscode.executeCodeActionProvider", vec![env.model.uri().to_dynamic(), selection.to_dynamic()])
            .await
            .expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        let first = &value[0];
        let command = first.get_str("command");
        assert!(!command.is_nullish());
        let argument = items(&command.get_str("arguments")).remove(1);
        assert!(is_instance_of(&argument, "Selection"));
        assert!(types::Selection::from_dynamic(&argument).expect("Selection").is_equal(&selection));
        env.teardown().await;
    });
}

#[test]
fn execute_code_action_provider_results_seem_to_be_missing_their_is_preferred_property_78098() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            CodeActionsProvider::new(|document, range_or_selection| {
                let mut action = code_action_with_command(document, range_or_selection);
                action.is_preferred = Some(true);
                vec![CommandOrCodeAction::CodeAction(Rc::new(RefCell::new(action)))]
            }),
            None,
        );
        env.add(registration);

        let selection = selection_0_0_1_1();

        env.sync().await;
        let value = env
            .execute("vscode.executeCodeActionProvider", vec![env.model.uri().to_dynamic(), selection.to_dynamic()])
            .await
            .expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        assert!(matches!(value[0].get_str("isPreferred"), Dynamic::Bool(true)));
        env.teardown().await;
    });
}

#[test]
fn resolving_code_action() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let did_call_resolve = Rc::new(Cell::new(0_u32));
        // `class MyAction extends types.CodeAction { }` e `assert.ok(action instanceof MyAction)`: o `resolveCodeAction` recebe o próprio
        // objeto que o `provideCodeActions` devolveu (a identidade do `Rc`).
        let provided: Rc<RefCell<Option<ApiCodeActionRef>>> = Rc::default();
        let (provided_in_provide, provided_in_resolve, counter) = (Rc::clone(&provided), Rc::clone(&provided), Rc::clone(&did_call_resolve));
        let registration = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CodeActionsProvider {
                provide: Box::new(move |_, _| {
                    let action = Rc::new(RefCell::new(types::CodeAction::new("title", Some(types::CodeActionKind::empty().append("foo")))));
                    *provided_in_provide.borrow_mut() = Some(Rc::clone(&action));
                    vec![CommandOrCodeAction::CodeAction(action)]
                }),
                resolve: Some(Box::new(move |action| {
                    assert!(Rc::ptr_eq(action, provided_in_resolve.borrow().as_ref().expect("provideCodeActions já rodou")));

                    counter.set(counter.get() + 1);
                    action.borrow_mut().title = "resolved title".to_owned();
                    action.borrow_mut().edit = Some(types::WorkspaceEdit::new());
                })),
            }),
            None,
        );
        env.add(registration);

        let selection = selection_0_0_1_1();

        env.sync().await;

        let value = env
            .execute(
                "vscode.executeCodeActionProvider",
                vec![env.model.uri().to_dynamic(), selection.to_dynamic(), Dynamic::Undefined, Dynamic::Number(1000.0)],
            )
            .await
            .expect("executeCommand");
        assert_eq!(did_call_resolve.get(), 1);
        let value = items(&value);
        assert_eq!(value.len(), 1);

        let first = &value[0];
        assert_eq!(text(&first.get_str("title")), "title"); // does NOT change
        assert!(!first.get_str("edit").is_nullish()); // is set
        env.teardown().await;
    });
}

// ---- code lens ------------------------------------------------------------------------------------------

/// `provideCodeLenses(): any { return [...]; }` e, quando existe, `resolveCodeLens(codeLens) { ...; return codeLens; }`.
struct LensesProvider {
    lenses: Box<dyn Fn() -> Vec<CodeLensRef>>,
    resolve: Option<Box<dyn Fn(&CodeLensRef)>>,
}

impl ApiCodeLensProvider for LensesProvider {
    fn provide_code_lenses(&self, _document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<Vec<CodeLensRef>> {
        found((self.lenses)())
    }

    fn has_resolve_code_lens(&self) -> bool {
        self.resolve.is_some()
    }

    fn resolve_code_lens(&self, code_lens: &CodeLensRef, _token: CancellationToken) -> ApiProviderResult<CodeLensRef> {
        if let Some(resolve) = &self.resolve {
            resolve(code_lens);
        }
        found(Rc::clone(code_lens))
    }
}

/// `new types.CodeLens(new types.Range(0, 0, 1, 1), command)`.
fn code_lens(command: Option<types::Command>) -> CodeLensRef {
    Rc::new(RefCell::new(types::CodeLens::new(range(0.0, 0.0, 1.0, 1.0), command)))
}

#[test]
fn code_lens_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        // `{ foo() { }, bar() { }, big: extHost }`: o `extHost` não é um `Dynamic`, `big` é um objeto aninhado qualquer.
        let complex_arg = Dynamic::object([
            ("foo", Dynamic::from(FunctionRef::native("foo", 0, |_| Dynamic::Undefined))),
            ("bar", Dynamic::from(FunctionRef::native("bar", 0, |_| Dynamic::Undefined))),
            ("big", Dynamic::object([("name", Dynamic::string("extHost"))])),
        ]);
        let argument = complex_arg.clone();

        let registration = env.ext_host.register_code_lens_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(LensesProvider {
                lenses: Box::new(move || {
                    let mut command = types::Command::new("Title", "cmd");
                    command.arguments = Some(vec![Dynamic::from(1.0), Dynamic::Bool(true), argument.clone()]);
                    vec![code_lens(Some(command))]
                }),
                resolve: None,
            }),
        );
        env.add(registration);

        env.sync().await;
        let value = env.execute("vscode.executeCodeLensProvider", vec![env.model.uri().to_dynamic()]).await.expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        let command = value[0].get_str("command");

        assert_eq!(text(&command.get_str("title")), "Title");
        assert_eq!(text(&command.get_str("command")), "cmd");
        let arguments = items(&command.get_str("arguments"));
        assert_eq!(number(&arguments[0]), 1.0);
        assert!(matches!(arguments[1], Dynamic::Bool(true)));
        assert!(arguments[2].strict_equals(&complex_arg));
        env.teardown().await;
    });
}

#[test]
fn code_lens_resolve() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let resolve_count = Rc::new(Cell::new(0_u32));
        let counter = Rc::clone(&resolve_count);
        let registration = env.ext_host.register_code_lens_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(LensesProvider {
                lenses: Box::new(|| {
                    vec![code_lens(None), code_lens(None), code_lens(None), code_lens(Some(types::Command::new("Already resolved", "fff")))]
                }),
                resolve: Some(Box::new(move |lens| {
                    lens.borrow_mut().command = Some(types::Command::new(counter.get().to_string(), "resolved"));
                    counter.set(counter.get() + 1);
                })),
            }),
        );
        env.add(registration);

        env.sync().await;

        let value = env.execute("vscode.executeCodeLensProvider", vec![env.model.uri().to_dynamic(), Dynamic::Number(2.0)]).await.expect("executeCommand");
        assert_eq!(items(&value).len(), 3); // the resolve argument defines the number of results being returned
        assert_eq!(resolve_count.get(), 2);

        resolve_count.set(0);
        let value = env.execute("vscode.executeCodeLensProvider", vec![env.model.uri().to_dynamic()]).await.expect("executeCommand");
        assert_eq!(items(&value).len(), 4);
        assert_eq!(resolve_count.get(), 0);
        env.teardown().await;
    });
}

// ---- links ----------------------------------------------------------------------------------------------

/// `provideDocumentLinks(): any { return [...]; }` e, quando existe, `resolveDocumentLink(link) { ...; return link; }`.
struct LinksProvider {
    links: Box<dyn Fn() -> Vec<DocumentLinkRef>>,
    resolve: Option<Box<dyn Fn(&DocumentLinkRef)>>,
}

impl ApiDocumentLinkProvider for LinksProvider {
    fn provide_document_links(&self, _document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<Vec<DocumentLinkRef>> {
        found((self.links)())
    }

    fn has_resolve_document_link(&self) -> bool {
        self.resolve.is_some()
    }

    fn resolve_document_link(&self, link: &DocumentLinkRef, _token: CancellationToken) -> ApiProviderResult<DocumentLinkRef> {
        if let Some(resolve) = &self.resolve {
            resolve(link);
        }
        found(Rc::clone(link))
    }
}

/// `new types.DocumentLink(new types.Range(0, 0, 0, 20), target)`.
fn document_link(target: Option<&str>, tooltip: Option<&str>) -> DocumentLinkRef {
    let mut link = types::DocumentLink::new(range(0.0, 0.0, 0.0, 20.0), target.map(|target| URI::parse(target, false))).expect("DocumentLink");
    link.tooltip = tooltip.map(str::to_owned);
    Rc::new(RefCell::new(link))
}

/// `uri + ''` e `uri.toString()`.
fn uri_text(value: &Dynamic) -> String {
    URI::from_dynamic(value).expect("Uri").to_string(false).to_string_lossy()
}

#[test]
fn links_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_document_link_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(LinksProvider { links: Box::new(|| vec![document_link(Some("foo:bar"), None)]), resolve: None }),
        );
        env.add(registration);

        env.sync().await;
        let value = env.execute("vscode.executeLinkProvider", vec![env.model.uri().to_dynamic()]).await.expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        let first = &value[0];

        assert_eq!(uri_text(&first.get_str("target")), "foo:bar");
        assert_range(&first.get_str("range"), 0.0, 0.0, 0.0, 20.0);
        env.teardown().await;
    });
}

#[test]
fn what_is_the_condition_for_document_link_target_to_be_undefined_106308() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_document_link_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(LinksProvider {
                links: Box::new(|| vec![document_link(None, None)]),
                resolve: Some(Box::new(|link| link.borrow_mut().target = Some(URI::parse("foo:bar", false)))),
            }),
        );
        env.add(registration);

        env.sync().await;

        let links1 = env.execute("vscode.executeLinkProvider", vec![env.model.uri().to_dynamic()]).await.expect("executeCommand");
        let links1 = items(&links1);
        assert_eq!(links1.len(), 1);
        assert!(links1[0].get_str("target").is_undefined());

        let links2 = env.execute("vscode.executeLinkProvider", vec![env.model.uri().to_dynamic(), Dynamic::Number(1000.0)]).await.expect("executeCommand");
        let links2 = items(&links2);
        assert_eq!(links2.len(), 1);
        assert_eq!(uri_text(&links2[0].get_str("target")), URI::parse("foo:bar", false).to_string(false).to_string_lossy());
        env.teardown().await;
    });
}

#[test]
fn document_link_array_execute_link_provider_returns_lack_tooltip_213970() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_document_link_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(LinksProvider { links: Box::new(|| vec![document_link(Some("foo:bar"), Some("Link Tooltip"))]), resolve: None }),
        );
        env.add(registration);

        env.sync().await;

        let links1 = env.execute("vscode.executeLinkProvider", vec![env.model.uri().to_dynamic()]).await.expect("executeCommand");
        let links1 = items(&links1);
        assert_eq!(links1.len(), 1);
        assert_eq!(text(&links1[0].get_str("tooltip")), "Link Tooltip");
        env.teardown().await;
    });
}

// ---- color provider -------------------------------------------------------------------------------------

/// `provideDocumentColors()` e `provideColorPresentations()` do caso `Color provider`.
struct ColorProvider;

impl ApiDocumentColorProvider for ColorProvider {
    fn provide_document_colors(&self, _document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<Vec<types::ColorInformation>> {
        found(vec![types::ColorInformation::new(range(0.0, 0.0, 0.0, 20.0), types::Color::new(0.1, 0.2, 0.3, 0.4)).expect("ColorInformation")])
    }

    fn provide_color_presentations(&self, _color: &types::Color, _context: &ApiColorPresentationContext, _token: CancellationToken) -> ApiProviderResult<Vec<types::ColorPresentation>> {
        let mut presentation = types::ColorPresentation::new("#ABC").expect("ColorPresentation");
        presentation.text_edit = Some(types::TextEdit::replace(range(1.0, 0.0, 1.0, 20.0), "#ABC"));
        presentation.additional_text_edits = Some(vec![types::TextEdit::insert(types::Position::new(2.0, 20.0).expect("Position"), "*")]);
        found(vec![presentation])
    }
}

#[test]
fn color_provider() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_color_provider(&null_extension_description(), &default_selector(), Rc::new(ColorProvider));
        env.add(registration);

        env.sync().await;
        let value = env.execute("vscode.executeDocumentColorProvider", vec![env.model.uri().to_dynamic()]).await.expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        let first = &value[0];

        let color = first.get_str("color");
        assert_eq!(number(&color.get_str("red")), 0.1);
        assert_eq!(number(&color.get_str("green")), 0.2);
        assert_eq!(number(&color.get_str("blue")), 0.3);
        assert_eq!(number(&color.get_str("alpha")), 0.4);
        assert_range(&first.get_str("range"), 0.0, 0.0, 0.0, 20.0);

        let color = types::Color::new(0.5, 0.6, 0.7, 0.8);
        let context = Dynamic::object([("uri", env.model.uri().to_dynamic()), ("range", range(0.0, 0.0, 0.0, 20.0).to_dynamic())]);
        let value = env.execute("vscode.executeColorPresentationProvider", vec![color.to_dynamic(), context]).await.expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        let first = &value[0];

        assert_eq!(text(&first.get_str("label")), "#ABC");
        let text_edit = first.get_str("textEdit");
        assert_eq!(text(&text_edit.get_str("newText")), "#ABC");
        assert_range(&text_edit.get_str("range"), 1.0, 0.0, 1.0, 20.0);
        let additional_text_edits = items(&first.get_str("additionalTextEdits"));
        assert_eq!(additional_text_edits.len(), 1);
        assert_range(&additional_text_edits[0].get_str("range"), 2.0, 20.0, 2.0, 20.0);
        env.teardown().await;
    });
}

// ---- hover ----------------------------------------------------------------------------------------------

/// `provideHover(): any { return new types.Hover('fofofofo'); }`.
struct StringHoverProvider;

impl ApiHoverProvider for StringHoverProvider {
    fn provide_hover(&self, _document: &TextDocument, _position: &types::Position, _token: CancellationToken, _context: Option<&ApiHoverContext>) -> ApiProviderResult<types::VerboseHover> {
        found(types::VerboseHover::new(vec![types::MarkedString::Text("fofofofo".to_owned())], None, None, None))
    }
}

#[test]
fn type_error_on_cancellation_requested_is_not_a_function_calling_hover_provider_in_insiders_54174() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_hover_provider(&null_extension_description(), &default_selector(), Rc::new(StringHoverProvider));
        env.add(registration);

        env.sync().await;
        let value = env.execute("vscode.executeHoverProvider", vec![env.model.uri().to_dynamic(), position(1.0, 1.0)]).await.expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        assert_eq!(items(&value[0].get_str("contents")).len(), 1);
        env.teardown().await;
    });
}

// ---- inline hints ---------------------------------------------------------------------------------------

/// `provideInlayHints(): any { return [...]; }` (ou o `throw new Error()` do provedor defeituoso, que no adaptador vira uma promessa
/// rejeitada).
struct InlayHintsProvider(Box<dyn Fn() -> ApiProviderResult<Vec<InlayHintRef>>>);

impl ApiInlayHintsProvider for InlayHintsProvider {
    fn provide_inlay_hints(&self, _document: &TextDocument, _range: &types::Range, _token: CancellationToken) -> ApiProviderResult<Vec<InlayHintRef>> {
        (self.0)()
    }
}

/// `new types.InlayHint(position, label, kind)` com o rótulo de texto.
fn inlay_hint(line: f64, character: f64, label: &str, kind: Option<types::InlayHintKind>) -> InlayHintRef {
    Rc::new(RefCell::new(types::InlayHint::new(
        types::Position::new(line, character).expect("Position"),
        types::InlayHintLabel::Text(label.to_owned()),
        kind,
    )))
}

/// `commands.executeCommand<vscode.InlayHint[]>('vscode.executeInlayHintProvider', model.uri, new types.Range(0, 0, 20, 20))`.
async fn execute_inlay_hint_provider(env: &Env) -> Vec<Dynamic> {
    let value = env
        .execute("vscode.executeInlayHintProvider", vec![env.model.uri().to_dynamic(), range(0.0, 0.0, 20.0, 20.0).to_dynamic()])
        .await
        .expect("executeCommand");
    items(&value)
}

/// `hint.position.line` e `hint.position.character`.
fn assert_position(value: &Dynamic, line: f64, character: f64) {
    let position = value.get_str("position");
    assert_eq!(number(&position.get_str("line")), line);
    assert_eq!(number(&position.get_str("character")), character);
}

#[test]
fn inlay_hints_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_inlay_hints_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(InlayHintsProvider(Box::new(|| found(vec![inlay_hint(0.0, 1.0, "Foo", None)])))),
        );
        env.add(registration);

        env.sync().await;

        let value = execute_inlay_hint_provider(&env).await;
        assert_eq!(value.len(), 1);

        let first = &value[0];
        assert_eq!(text(&first.get_str("label")), "Foo");
        assert_position(first, 0.0, 1.0);
        env.teardown().await;
    });
}

#[test]
fn inline_hints_merge() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_inlay_hints_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(InlayHintsProvider(Box::new(|| {
                let mut part = types::InlayHintLabelPart::new("Bar");
                part.tooltip = Some(types::Documentation::Text("part_tooltip".to_owned()));
                part.command = Some(types::Command::new("part", "cmd"));
                let mut hint = types::InlayHint::new(types::Position::new(10.0, 11.0).expect("Position"), types::InlayHintLabel::Parts(vec![part]), None);
                hint.tooltip = Some(types::Documentation::Text("hint_tooltip".to_owned()));
                hint.padding_left = Some(true);
                hint.padding_right = Some(false);
                found(vec![Rc::new(RefCell::new(hint))])
            }))),
        );
        env.add(registration);

        let registration = env.ext_host.register_inlay_hints_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(InlayHintsProvider(Box::new(|| {
                let hint = inlay_hint(0.0, 1.0, "Foo", Some(types::InlayHintKind::Parameter));
                hint.borrow_mut().text_edits = Some(vec![types::TextEdit::insert(types::Position::new(0.0, 0.0).expect("Position"), "Hello")]);
                found(vec![hint])
            }))),
        );
        env.add(registration);

        env.sync().await;

        let value = execute_inlay_hint_provider(&env).await;
        assert_eq!(value.len(), 2);

        let (first, second) = (&value[0], &value[1]);
        assert_eq!(text(&first.get_str("label")), "Foo");
        assert_position(first, 0.0, 1.0);
        let text_edits = items(&first.get_str("textEdits"));
        assert_eq!(text_edits.len(), 1);
        assert_eq!(text(&text_edits[0].get_str("newText")), "Hello");

        assert_position(second, 10.0, 11.0);
        assert!(matches!(second.get_str("paddingLeft"), Dynamic::Bool(true)));
        assert!(matches!(second.get_str("paddingRight"), Dynamic::Bool(false)));
        assert_eq!(text(&second.get_str("tooltip")), "hint_tooltip");

        let label = items(&second.get_str("label")).remove(0);
        assert!(has_class(&label, "InlayHintLabelPart"));
        assert_eq!(text(&label.get_str("value")), "Bar");
        assert_eq!(text(&label.get_str("tooltip")), "part_tooltip");
        let command = label.get_str("command");
        assert_eq!(text(&command.get_str("command")), "cmd");
        assert_eq!(text(&command.get_str("title")), "part");
        env.teardown().await;
    });
}

#[test]
fn inline_hints_bad_provider() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_inlay_hints_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(InlayHintsProvider(Box::new(|| found(vec![inlay_hint(0.0, 1.0, "Foo", None)])))),
        );
        env.add(registration);
        let registration = env.ext_host.register_inlay_hints_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(InlayHintsProvider(Box::new(|| MaybePromise::Promise(Promise::rejected(Error::new("Error")))))),
        );
        env.add(registration);

        env.sync().await;

        let value = execute_inlay_hint_provider(&env).await;
        assert_eq!(value.len(), 1);

        let first = &value[0];
        assert_eq!(text(&first.get_str("label")), "Foo");
        assert_position(first, 0.0, 1.0);
        env.teardown().await;
    });
}

// ---- selection ranges -----------------------------------------------------------------------------------

/// `provideSelectionRanges(_doc, positions) { return ...; }`.
struct SelectionRangesProvider(Box<dyn Fn(&[types::Position]) -> ApiProviderResult<Vec<types::SelectionRange>>>);

impl ApiSelectionRangeProvider for SelectionRangesProvider {
    fn provide_selection_ranges(&self, _document: &TextDocument, positions: &[types::Position], _token: CancellationToken) -> ApiProviderResult<Vec<types::SelectionRange>> {
        (self.0)(positions)
    }
}

/// `new types.SelectionRange(new types.Range(line, character, line, character))`.
fn empty_selection_range_at(position: &types::Position) -> types::SelectionRange {
    types::SelectionRange::new(range(position.line(), position.character(), position.line(), position.character()), None).expect("SelectionRange")
}

#[test]
fn selection_range_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_selection_range_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SelectionRangesProvider(Box::new(|_positions| {
                let parent = types::SelectionRange::new(range(0.0, 2.0, 0.0, 20.0), None).expect("SelectionRange");
                found(vec![types::SelectionRange::new(range(0.0, 10.0, 0.0, 18.0), Some(parent)).expect("SelectionRange")])
            }))),
        );
        env.add(registration);

        env.sync().await;
        let value = env
            .execute("vscode.executeSelectionRangeProvider", vec![env.model.uri().to_dynamic(), Dynamic::array(vec![position(0.0, 10.0)])])
            .await
            .expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        assert!(!value[0].get_str("parent").is_nullish());
        env.teardown().await;
    });
}

// ---- call hierarchy -------------------------------------------------------------------------------------

/// `new types.Range(0, 0, 0, 0)`.
fn zero_range() -> types::Range {
    range(0.0, 0.0, 0.0, 0.0)
}

/// `new types.CallHierarchyItem(types.SymbolKind.Constant, name, name, uri, new types.Range(0, 0, 0, 0), new types.Range(0, 0, 0, 0))`.
fn call_hierarchy_item(name: &str, uri: &URI) -> types::CallHierarchyItem {
    types::CallHierarchyItem::new(types::SymbolKind::Constant, name, name, uri.clone(), zero_range(), zero_range())
}

/// `new types.TypeHierarchyItem(types.SymbolKind.Constant, name, name, uri, new types.Range(0, 0, 0, 0), new types.Range(0, 0, 0, 0))`.
fn type_hierarchy_item(name: &str, uri: &URI) -> types::TypeHierarchyItem {
    types::TypeHierarchyItem::new(types::SymbolKind::Constant, name, name, uri.clone(), zero_range(), zero_range())
}

/// O `vscode.CallHierarchyProvider` do caso: o `prepareCallHierarchy` recebe o recurso do documento.
struct CallHierarchyTestProvider {
    prepare: Box<dyn Fn(&URI) -> OneOrMany<types::CallHierarchyItem>>,
    incoming: Box<dyn Fn(&URI) -> Vec<types::CallHierarchyIncomingCall>>,
    outgoing: Box<dyn Fn(&URI) -> Vec<types::CallHierarchyOutgoingCall>>,
}

impl ApiCallHierarchyProvider for CallHierarchyTestProvider {
    fn prepare_call_hierarchy(&self, document: &TextDocument, _position: &types::Position, _token: CancellationToken) -> ApiProviderResult<OneOrMany<types::CallHierarchyItem>> {
        found((self.prepare)(document.uri()))
    }

    fn provide_call_hierarchy_incoming_calls(&self, item: &types::CallHierarchyItem, _token: CancellationToken) -> ApiProviderResult<Vec<types::CallHierarchyIncomingCall>> {
        found((self.incoming)(&item.uri))
    }

    fn provide_call_hierarchy_outgoing_calls(&self, item: &types::CallHierarchyItem, _token: CancellationToken) -> ApiProviderResult<Vec<types::CallHierarchyOutgoingCall>> {
        found((self.outgoing)(&item.uri))
    }
}

#[test]
fn call_hierarchy_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_call_hierarchy_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CallHierarchyTestProvider {
                prepare: Box::new(|uri| OneOrMany::One(call_hierarchy_item("ROOT", uri))),
                incoming: Box::new(|uri| vec![types::CallHierarchyIncomingCall::new(call_hierarchy_item("INCOMING", uri), vec![zero_range()])]),
                outgoing: Box::new(|uri| vec![types::CallHierarchyOutgoingCall::new(call_hierarchy_item("OUTGOING", uri), vec![zero_range()])]),
            }),
        );
        env.add(registration);

        env.sync().await;

        let root = env.execute("vscode.prepareCallHierarchy", vec![env.model.uri().to_dynamic(), position(0.0, 0.0)]).await.expect("executeCommand");
        let root = items(&root);
        assert_eq!(root.len(), 1);
        assert_eq!(text(&root[0].get_str("name")), "ROOT");

        let incoming = env.execute("vscode.provideIncomingCalls", vec![root[0].clone()]).await.expect("executeCommand");
        let incoming = items(&incoming);
        assert_eq!(incoming.len(), 1);
        assert_eq!(text(&incoming[0].get_str("from").get_str("name")), "INCOMING");

        let outgoing = env.execute("vscode.provideOutgoingCalls", vec![root[0].clone()]).await.expect("executeCommand");
        let outgoing = items(&outgoing);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(text(&outgoing[0].get_str("to").get_str("name")), "OUTGOING");
        env.teardown().await;
    });
}

#[test]
fn prepare_call_hierarchy_throws_type_error_if_clangd_returns_empty_result_137415() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_call_hierarchy_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CallHierarchyTestProvider {
                prepare: Box::new(|_| OneOrMany::Many(Vec::new())),
                incoming: Box::new(|_| Vec::new()),
                outgoing: Box::new(|_| Vec::new()),
            }),
        );
        env.add(registration);

        env.sync().await;

        let root = env.execute("vscode.prepareCallHierarchy", vec![env.model.uri().to_dynamic(), position(0.0, 0.0)]).await.expect("executeCommand");
        assert_eq!(items(&root).len(), 0);
        env.teardown().await;
    });
}

// ---- type hierarchy -------------------------------------------------------------------------------------

/// `new class implements vscode.TypeHierarchyProvider { ... }` do caso `TypeHierarchy, back and forth`.
struct TypeHierarchyTestProvider;

impl ApiTypeHierarchyProvider for TypeHierarchyTestProvider {
    fn prepare_type_hierarchy(&self, document: &TextDocument, _position: &types::Position, _token: CancellationToken) -> ApiProviderResult<OneOrMany<types::TypeHierarchyItem>> {
        found(OneOrMany::Many(vec![type_hierarchy_item("ROOT", document.uri())]))
    }

    fn provide_type_hierarchy_supertypes(&self, item: &types::TypeHierarchyItem, _token: CancellationToken) -> ApiProviderResult<Vec<types::TypeHierarchyItem>> {
        found(vec![type_hierarchy_item("SUPER", &item.uri)])
    }

    fn provide_type_hierarchy_subtypes(&self, item: &types::TypeHierarchyItem, _token: CancellationToken) -> ApiProviderResult<Vec<types::TypeHierarchyItem>> {
        found(vec![type_hierarchy_item("SUB", &item.uri)])
    }
}

#[test]
fn type_hierarchy_back_and_forth() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_type_hierarchy_provider(&null_extension_description(), &default_selector(), Rc::new(TypeHierarchyTestProvider));
        env.add(registration);

        env.sync().await;

        let root = env.execute("vscode.prepareTypeHierarchy", vec![env.model.uri().to_dynamic(), position(0.0, 0.0)]).await.expect("executeCommand");
        let root = items(&root);
        assert_eq!(root.len(), 1);
        assert_eq!(text(&root[0].get_str("name")), "ROOT");

        let incoming = env.execute("vscode.provideSupertypes", vec![root[0].clone()]).await.expect("executeCommand");
        let incoming = items(&incoming);
        assert_eq!(incoming.len(), 1);
        assert_eq!(text(&incoming[0].get_str("name")), "SUPER");

        let outgoing = env.execute("vscode.provideSubtypes", vec![root[0].clone()]).await.expect("executeCommand");
        let outgoing = items(&outgoing);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(text(&outgoing[0].get_str("name")), "SUB");
        env.teardown().await;
    });
}

#[test]
fn selection_range_provider_on_inner_array_always_returns_outer_array_91852() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_selection_range_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SelectionRangesProvider(Box::new(|positions| found(vec![empty_selection_range_at(&positions[0])])))),
        );
        env.add(registration);

        env.sync().await;
        let value = env
            .execute("vscode.executeSelectionRangeProvider", vec![env.model.uri().to_dynamic(), Dynamic::array(vec![position(0.0, 10.0)])])
            .await
            .expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 1);
        assert_range(&value[0].get_str("range"), 0.0, 10.0, 0.0, 10.0);
        env.teardown().await;
    });
}

#[test]
fn more_element_test_of_selection_range_provider_on_inner_array_always_returns_outer_array_91852() {
    run_with_faked_timers(async move {
        let mut env = Env::new();

        let registration = env.ext_host.register_selection_range_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SelectionRangesProvider(Box::new(|positions| found(vec![empty_selection_range_at(&positions[0]), empty_selection_range_at(&positions[1])])))),
        );
        env.add(registration);

        env.sync().await;
        let value = env
            .execute(
                "vscode.executeSelectionRangeProvider",
                vec![env.model.uri().to_dynamic(), Dynamic::array(vec![position(0.0, 0.0), position(0.0, 10.0)])],
            )
            .await
            .expect("executeCommand");
        let value = items(&value);
        assert_eq!(value.len(), 2);
        assert_range(&value[0].get_str("range"), 0.0, 0.0, 0.0, 0.0);
        assert_range(&value[1].get_str("range"), 0.0, 10.0, 0.0, 10.0);
        env.teardown().await;
    });
}
