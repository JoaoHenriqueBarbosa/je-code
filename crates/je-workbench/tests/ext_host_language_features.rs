//! Porte de `src/vs/workbench/api/test/browser/extHostLanguageFeatures.test.ts` (suíte `ExtHostLanguageFeatures`), caso a caso e na mesma ordem.
//!
//! PORTADOS (23 de 65 `test(` do original, contando cada caso dos laços `for (const declinedRequests of [0, 2])` como dois):
//! `AutoClosingPair.beforeText requires proposed API`, `DocumentDropEdits does not cache an empty edit array without release IDs`,
//! `DocumentSymbols` x4 (incluindo `Quick Outline ... #138502`),
//! `Occurrences` x4, `Quick Fix` x3 e `#29469`, `Parameter Hints` x2, `Suggest` x5 e `Selection Ranges` x2.
//! Divergência de tradução: o `#29469` devolve `[undefined, null, comando]` e o `Vec` não tem elementos ausentes, então o caso
//! devolve só o comando (o `if (!candidate) continue` do adaptador não existe no porte).
//!
//! FORA, e por quê (nenhum vira `#[test]` enquanto não puder ser fiel):
//! - Função do editor ainda sem porte: `getCodeLensModel` (CodeLens x3), `getDefinitionsAtPosition`,
//!   `getImplementationsAtPosition`, `getTypeDefinitionsAtPosition`, `getDeclarationsAtPosition`, `getReferencesAtPosition`
//!   (Definition x4, Declaration, Implementation, Type Definition, References x3), `getHoversPromise` (HoverProvider x4),
//!   `getWorkspaceSymbols` (Navigate types x2), `rename` (Rename x5 e os dois #98352), `getDocumentFormattingEditsUntilResult`,
//!   `getDocumentRangeFormattingEditsUntilResult` e `getOnTypeFormattingEdits` (Format Doc x3, Format Range x3, Format on
//!   Type), `getLinks` (Links x2) e `getColors` (Document colors).
//! - Os casos de completação em linha (`InlineCompletion`) não existem nesta suíte do original; o porte da
//!   `InlineCompletionAdapter` e do `ExtensionBackedInlineCompletionsProvider` está pendente (ver os dois arquivos de origem).
//! - Os casos `DocumentDropEdits releases cache IDs after N declined requests` e `DocumentPasteEdits releases cache IDs after N
//!   declined requests` espionam `extHost.$releaseDocumentOnDropEdits` e `$releasePasteEdits` (`sinon.spy`): a spy exige um
//!   proxy de `ExtHostLanguageFeaturesShape` que repasse o método ao host e registre as chamadas, e o dublê desse trait inteiro
//!   (cerca de 75 métodos) não existe. `DocumentPasteEdits does not cache empty results without release IDs` espiona
//!   `Cache.prototype.add`, que não tem equivalente.
//!
//! Notas de tradução:
//! - `new TestRPCProtocol()` é o [`TestMainContext`]. O `getProxy` do original é preguiçoso (o host e o lado do workbench se
//!   conhecem), então o [`MainThreadLanguageFeatures`] pede o proxy do host a cada uso e é criado antes do host. O
//!   `MainThreadCommands` real precisa de `IExtensionService` (sem dublê no porte): os comandos de que o caso precisa
//!   (nenhum é executado) são o dublê [`UnusedMainThreadCommands`]. `MainThreadDocuments` e `MainThreadDiagnostics` são
//!   dublês que falham se chamados (nenhum caso os chama).
//! - `setUnexpectedErrorHandler(() => { })` e o `ensureNoDisposablesAreLeakedInTestSuite` são o guarda criado no começo de cada
//!   caso.
//! - `IMarkerService` e `IUriIdentityService` do `TestInstantiationService`: o serviço de identidade é o dublê que devolve a
//!   própria URI (`asCanonicalUri(uri) { return uri; }`). `ILanguageService`, `ILanguageConfigurationService` e
//!   `IInlineCompletionsUnificationService` não são registrados no original (o `TestInstantiationService` não é estrito e os resolve
//!   como `undefined`), então o [`MainThreadLanguageFeatures`] os recebe como `None`.
//! - `sinon.stub(mainThread, '$setLanguageConfiguration')`: o [`StubbedSetLanguageConfiguration`] é o proxy do lado do workbench que
//!   repassa tudo ao [`MainThreadLanguageFeatures`] real, menos `$setLanguageConfiguration`, que só conta a chamada (`callCount`).
//!   Se o `MainThreadLanguageFeaturesShape` ganhar um método, o repasse dele entra na lista do `forward_to_inner!`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::data_transfer::VSDataTransfer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::code_action::code_action::get_code_actions;
use je_editor::code_action::languages::CodeActionTriggerType;
use je_editor::code_action::platform::{ActionSet, ProgressNone};
use je_editor::code_action::types::{CodeActionTrigger, CodeActionTriggerSource};
use je_editor::config::editor_options::smart_select::SmartSelectOptions;
use je_editor::language_features_service::{DocumentDropEditProvider, LanguageFeaturesService};
use je_editor::language_providers::DocumentHighlightKind;
use je_editor::languages::{CompletionContext, CompletionItemKind};
use je_editor::model::Eol;
use je_editor::outline_model::OutlineModel;
use je_editor::parameter_hints::languages::{
    SignatureHelpContext as EditorSignatureHelpContext, SignatureHelpTriggerKind as EditorSignatureHelpTriggerKind,
};
use je_editor::parameter_hints::provide_signature_help::provide_signature_help;
use je_editor::position::Position;
use je_editor::smart_select::provide_selection_ranges;
use je_editor::suggest::suggest::{CompletionOptions, provide_suggestion_items};
use je_editor::text_model::TextModel;
use je_editor::word_highlighter::get_occurrences_at_position;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::NullLogService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::api::browser::main_thread_language_features::MainThreadLanguageFeatures;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_diagnostics::ExtHostDiagnostics;
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_file_system_info::ExtHostFileSystemInfo;
use je_workbench::api::common::ext_host_language_features::ExtHostLanguageFeatures;
use je_workbench::api::common::ext_host_language_features_api::{
    ApiAutoClosingPair, ApiCodeActionContext, ApiCodeActionProvider, ApiCompletionItemProvider, ApiCompletionList, ApiCompletionResult, ApiDocumentDropEditProvider,
    ApiDocumentHighlightProvider, ApiDocumentSymbolProvider, ApiDocumentSymbols, ApiLanguageConfiguration, ApiProviderResult, ApiRangeOrSelection, ApiSelectionRangeProvider,
    ApiSignatureHelpContext, ApiSignatureHelpProvider, ApiSignatureHelpRef, CommandOrCodeAction, CompletionItemRef, DocumentEditRef,
    IExtHostApiDeprecationService, OneOrMany, SignatureHelpMetadataOrTriggerChars,
};
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, DocumentsAndEditorsDelta, ExtHostContext, ExtHostDocumentsAndEditorsShape, ExtHostLanguageFeaturesShape, ICodeActionProviderMetadataDto,
    IDocumentDropEditProviderMetadata, IDocumentFilterDto, IExtHostTelemetry, ILanguageConfigurationDto, IMarkerChange, IPasteEditProviderMetadataDto,
    ISignatureHelpProviderMetadataDto, MainContext, MainThreadCommandsShape, MainThreadDiagnosticsShape, MainThreadDocumentsShape, MainThreadLanguageFeaturesShape,
    ModelAddedData, CreateDocumentOptions, OpenDocumentOptions,
};
use je_workbench::api::common::ext_host_type_converters::{DocumentFilterInput, DocumentSelectorInput};
use je_workbench::api::common::ext_host_types as types;
use je_workbench::services::extensions::extensions::null_extension_description;
use je_extensions::scanner::ExtensionDescription;
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

/// O `MainThreadCommands` do caso (ver as notas de tradução): nenhum comando é executado.
struct UnusedMainThreadCommands;

impl IDisposable for UnusedMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for UnusedMainThreadCommands {
    fn rpc_register_command(&self, _id: &str) {}

    fn rpc_unregister_command(&self, _id: &str) {}

    fn rpc_fire_command_activation_event(&self, _id: &str) {}

    fn rpc_execute_command(&self, id: &str, _args: CommandArgs, _retry: bool) -> Promise<Dynamic> {
        panic!("$executeCommand({id}) não é esperado")
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        Promise::resolved(Vec::new())
    }
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

/// `sinon.stub(mainThread, '$setLanguageConfiguration')`: o proxy do lado do workbench que repassa tudo ao
/// [`MainThreadLanguageFeatures`] real, menos `$setLanguageConfiguration`, que só conta a chamada.
struct StubbedSetLanguageConfiguration {
    inner: Rc<MainThreadLanguageFeatures>,
    /// `setLanguageConfiguration.callCount`.
    call_count: Rc<Cell<u32>>,
}

impl IDisposable for StubbedSetLanguageConfiguration {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

/// Repassa ao `self.inner` os métodos de [`MainThreadLanguageFeaturesShape`] que o dublê não substitui.
macro_rules! forward_to_inner {
    ($($method:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(
            fn $method(&self, $($arg: $ty),*) $(-> $ret)? {
                self.inner.$method($($arg),*)
            }
        )*
    };
}

impl MainThreadLanguageFeaturesShape for StubbedSetLanguageConfiguration {
    forward_to_inner! {
        rpc_unregister(handle: u32);
        rpc_register_document_symbol_provider(handle: u32, selector: Vec<IDocumentFilterDto>, label: &str);
        rpc_register_code_lens_support(handle: u32, selector: Vec<IDocumentFilterDto>, event_handle: Option<u32>);
        rpc_emit_code_lens_event(event_handle: u32, event: Option<Dynamic>);
        rpc_register_definition_support(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_declaration_support(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_implementation_support(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_type_definition_support(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_hover_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_document_highlight_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_multi_document_highlight_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_reference_support(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_evaluatable_expression_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_inline_values_provider(handle: u32, selector: Vec<IDocumentFilterDto>, event_handle: Option<u32>);
        rpc_emit_inline_values_event(event_handle: u32, event: Option<Dynamic>);
        rpc_register_linked_editing_range_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_code_action_support(
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            metadata: ICodeActionProviderMetadataDto,
            display_name: &str,
            extension_id: &str,
            supports_resolve: bool
        );
        rpc_register_paste_edit_provider(handle: u32, selector: Vec<IDocumentFilterDto>, metadata: IPasteEditProviderMetadataDto);
        rpc_register_document_formatting_support(handle: u32, selector: Vec<IDocumentFilterDto>, extension_id: &ExtensionIdentifier, display_name: &str);
        rpc_register_range_formatting_support(
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            extension_id: &ExtensionIdentifier,
            display_name: &str,
            support_ranges: bool
        );
        rpc_register_on_type_formatting_support(
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            auto_format_trigger_characters: Vec<String>,
            extension_id: &ExtensionIdentifier
        );
        rpc_register_navigate_type_support(handle: u32, supports_resolve: bool);
        rpc_register_rename_support(handle: u32, selector: Vec<IDocumentFilterDto>, supports_resolve_initial_values: bool);
        rpc_register_new_symbol_names_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_document_semantic_tokens_provider(
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            legend: types::SemanticTokensLegend,
            event_handle: Option<u32>
        );
        rpc_emit_document_semantic_tokens_event(event_handle: u32);
        rpc_register_document_range_semantic_tokens_provider(
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            legend: types::SemanticTokensLegend,
            event_handle: Option<u32>
        );
        rpc_emit_document_range_semantic_tokens_event(event_handle: u32);
        rpc_register_completions_provider(
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            trigger_characters: Vec<String>,
            supports_resolve_details: bool,
            extension_id: &ExtensionIdentifier
        );
        rpc_register_signature_help_provider(handle: u32, selector: Vec<IDocumentFilterDto>, metadata: ISignatureHelpProviderMetadataDto);
        rpc_register_inlay_hints_provider(
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            supports_resolve: bool,
            event_handle: Option<u32>,
            display_name: Option<&str>
        );
        rpc_emit_inlay_hints_event(event_handle: u32);
        rpc_register_document_link_provider(handle: u32, selector: Vec<IDocumentFilterDto>, supports_resolve: bool);
        rpc_register_document_color_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_folding_range_provider(handle: u32, selector: Vec<IDocumentFilterDto>, extension_id: &ExtensionIdentifier, event_handle: Option<u32>);
        rpc_emit_folding_range_event(event_handle: u32, event: Option<Dynamic>);
        rpc_register_selection_range_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_call_hierarchy_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_type_hierarchy_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
        rpc_register_document_on_drop_edit_provider(handle: u32, selector: Vec<IDocumentFilterDto>, metadata: Option<IDocumentDropEditProviderMetadata>);
        rpc_resolve_paste_file_data(handle: u32, request_id: u32, data_id: &str) -> Promise<VSBuffer>;
        rpc_resolve_document_on_drop_file_data(handle: u32, request_id: u32, data_id: &str) -> Promise<VSBuffer>;
    }

    fn rpc_set_language_configuration(&self, _handle: u32, _language_id: &str, _configuration: ILanguageConfigurationDto) {
        self.call_count.set(self.call_count.get() + 1);
    }
}

// ---- o ambiente (`setup` e `teardown`) ----------------------------------------------------------------

/// `const defaultSelector = { scheme: 'far' }`.
fn default_selector() -> DocumentSelectorInput {
    DocumentSelectorInput::Filter(DocumentFilterInput { scheme: Some("far".to_owned()), ..DocumentFilterInput::default() })
}

struct Env {
    model: Rc<TextModel>,
    ext_host: Rc<ExtHostLanguageFeatures>,
    main_thread: Rc<MainThreadLanguageFeatures>,
    language_features_service: LanguageFeaturesService,
    rpc_protocol: Rc<TestMainContext>,
    disposables: Vec<DisposableRef>,
    /// O guarda de vazamentos nasce depois do `setup` (no original o `ensureNoDisposablesAreLeakedInTestSuite()` é
    /// registrado depois dele) e confere depois do `teardown`.
    guard: Option<LeakGuard>,
}

impl Env {
    /// `setup(() => { ... })`.
    fn new() -> Env {
        Env::with_set_language_configuration_calls(None)
    }

    /// O `setup`, com `sinon.stub(mainThread, '$setLanguageConfiguration')` quando há um contador de chamadas.
    fn with_set_language_configuration_calls(set_language_configuration_calls: Option<Rc<Cell<u32>>>) -> Env {
        let model = create_text_model_with(
            ["This is the first line", "This is the second line", "This is the third line"].join("\n").as_str(),
            None,
            Default::default(),
            Some(URI::parse("far://testing/file.a", false)),
        );

        let rpc_protocol = Rc::new(TestMainContext::new());
        let language_features_service = LanguageFeaturesService::new();

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

        rpc_protocol.set(&MainContext::MAIN_THREAD_COMMANDS, Rc::new(UnusedMainThreadCommands) as Rc<dyn MainThreadCommandsShape>);
        let commands = ExtHostCommands::new(rpc_protocol.as_ref(), NullLogService::shared(), Rc::new(TestExtHostTelemetry));
        rpc_protocol.set(&ExtHostContext::EXT_HOST_COMMANDS, commands.clone() as Rc<dyn je_workbench::api::common::ext_host_protocol::ExtHostCommandsShape>);

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
        let main_thread_proxy: Rc<dyn MainThreadLanguageFeaturesShape> = match set_language_configuration_calls {
            Some(call_count) => Rc::new(StubbedSetLanguageConfiguration { inner: main_thread.clone(), call_count }),
            None => main_thread.clone(),
        };
        rpc_protocol.set(&MainContext::MAIN_THREAD_LANGUAGE_FEATURES, main_thread_proxy);

        let ext_host = ExtHostLanguageFeatures::new(
            rpc_protocol.as_ref(),
            None,
            ext_host_documents,
            commands,
            diagnostics,
            NullLogService::shared(),
            Rc::new(NullApiDeprecationService),
            Rc::new(TestExtHostTelemetry),
        );
        rpc_protocol.set(&ExtHostContext::EXT_HOST_LANGUAGE_FEATURES, ext_host.clone() as Rc<dyn ExtHostLanguageFeaturesShape>);

        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        // `originalErrorHandler = errorHandler.getUnexpectedErrorHandler(); setUnexpectedErrorHandler(() => { });`
        je_base::errors::set_unexpected_error_handler(|_| {});
        Env { model, ext_host, main_thread, language_features_service, rpc_protocol, disposables: Vec::new(), guard: Some(guard) }
    }

    /// `disposables.add(value)`.
    fn add(&mut self, disposable: DisposableRef) {
        self.disposables.push(disposable);
    }

    /// `teardown(() => { disposables.clear(); model.dispose(); mainThread.dispose(); return rpcProtocol.sync(); })`.
    async fn teardown(self) {
        for disposable in &self.disposables {
            disposable.dispose();
        }
        self.model.dispose();
        self.main_thread.dispose();
        self.rpc_protocol.sync().await.expect("sync");
        // `setUnexpectedErrorHandler(originalErrorHandler)` (no `suiteTeardown`): de volta ao coletor do runner, e só depois a
        // conferência de vazamentos.
        common::record_unexpected_errors();
        drop(self.guard);
    }
}

/// Os provedores de símbolos do original são classes anônimas que devolvem um valor fixo ou lançam.
struct SymbolProvider(Box<dyn Fn() -> ApiProviderResult<ApiDocumentSymbols>>);

impl ApiDocumentSymbolProvider for SymbolProvider {
    fn provide_document_symbols(&self, _document: &je_workbench::api::common::ext_host_document_data::TextDocument, _token: CancellationToken) -> ApiProviderResult<ApiDocumentSymbols> {
        (self.0)()
    }
}

fn symbol_information(name: &str, kind: types::SymbolKind, range: types::Range) -> types::SymbolInformation {
    let location = types::Location::new(None, Some(types::RangeOrPosition::Range(range)));
    types::SymbolInformation::new(name, kind, None, location).expect("SymbolInformation")
}

fn range(start_line: f64, start_character: f64, end_line: f64, end_character: f64) -> types::Range {
    types::Range::new(types::Position::new(start_line, start_character).expect("Position"), types::Position::new(end_line, end_character).expect("Position"))
}

// ---- casos --------------------------------------------------------------------------------------------

#[test]
fn auto_closing_pair_before_text_requires_proposed_api() {
    run_with_faked_timers(async move {
        let call_count = Rc::new(Cell::new(0));
        let mut env = Env::with_set_language_configuration_calls(Some(Rc::clone(&call_count)));
        let default_extension = null_extension_description();
        let braces = |before_text: Option<Rc<JsRegExp>>| ApiLanguageConfiguration {
            auto_closing_pairs: Some(vec![ApiAutoClosingPair { open: "{".to_owned(), close: "}".to_owned(), not_in: None, before_text }]),
            ..ApiLanguageConfiguration::default()
        };

        let registration = env.ext_host.set_language_configuration(&default_extension, "testLanguage", &braces(None)).expect("setLanguageConfiguration");
        env.add(registration);

        let configuration = braces(Some(Rc::new(JsRegExp::from_str("\\{$", "").expect("RegExp"))));
        let error = env
            .ext_host
            .set_language_configuration(&default_extension, "testLanguage", &configuration)
            .err()
            .expect("setLanguageConfiguration deveria lançar");
        assert!(error.message.contains("CANNOT use API proposal: autoClosingPairBeforeText"), "{}", error.message);

        let mut extension_with_proposal = null_extension_description();
        extension_with_proposal.manifest.enabled_api_proposals = Some(vec!["autoClosingPairBeforeText".to_owned()]);
        let registration = env.ext_host.set_language_configuration(&extension_with_proposal, "testLanguage", &configuration).expect("setLanguageConfiguration");
        env.add(registration);
        env.rpc_protocol.sync().await.expect("sync");
        assert_eq!(call_count.get(), 2);
        env.teardown().await;
    });
}

#[test]
fn document_drop_edits_does_not_cache_an_empty_edit_array_without_release_ids() {
    run_with_faked_timers(async move {
        struct EmptyDropProvider;
        impl ApiDocumentDropEditProvider for EmptyDropProvider {
            fn provide_document_drop_edits(
                &self,
                _document: &je_workbench::api::common::ext_host_document_data::TextDocument,
                _position: &types::Position,
                _data_transfer: &Rc<types::DataTransfer>,
                _token: CancellationToken,
            ) -> ApiProviderResult<OneOrMany<DocumentEditRef>> {
                MaybePromise::Value(Some(OneOrMany::Many(Vec::new())))
            }
        }

        let mut env = Env::new();
        let registration = env.ext_host.register_document_on_drop_edit_provider(&null_extension_description(), &default_selector(), Rc::new(EmptyDropProvider), None);
        env.add(registration);
        env.rpc_protocol.sync().await.expect("sync");
        let provider = Rc::clone(&env.language_features_service.document_drop_edit_provider.all(&env.model)[0]);
        let result = provider
            .provide_document_drop_edits(&env.model, Position::new(1, 1), &Rc::new(VSDataTransfer::new()), &CancellationToken::None)
            .await
            .expect("provideDocumentDropEdits");
        assert!(result.is_none());
        env.teardown().await;
    });
}

#[test]
fn document_symbols_register_deregister() {
    run_with_faked_timers(async move {
        let env = Env::new();
        assert_eq!(env.language_features_service.document_symbol_provider.all(&env.model).len(), 0);
        let d1 = env.ext_host.register_document_symbol_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SymbolProvider(Box::new(|| MaybePromise::Value(Some(ApiDocumentSymbols::Information(Vec::new())))))),
            None,
        );

        env.rpc_protocol.sync().await.expect("sync");
        assert_eq!(env.language_features_service.document_symbol_provider.all(&env.model).len(), 1);
        d1.dispose();
        env.rpc_protocol.sync().await.expect("sync");
        env.teardown().await;
    });
}

#[test]
fn document_symbols_evil_provider() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let evil = env.ext_host.register_document_symbol_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SymbolProvider(Box::new(|| MaybePromise::Promise(Promise::rejected(Error::new("evil document symbol provider")))))),
            None,
        );
        env.add(evil);
        let good = env.ext_host.register_document_symbol_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SymbolProvider(Box::new(|| {
                MaybePromise::Value(Some(ApiDocumentSymbols::Information(vec![symbol_information("test", types::SymbolKind::Field, range(0.0, 0.0, 0.0, 0.0))])))
            }))),
            None,
        );
        env.add(good);

        env.rpc_protocol.sync().await.expect("sync");
        let value = OutlineModel::create(&env.language_features_service.document_symbol_provider, &env.model, &CancellationToken::None)
            .await
            .expect("OutlineModel.create")
            .as_list_of_document_symbols();
        assert_eq!(value.len(), 1);
        env.teardown().await;
    });
}

#[test]
fn document_symbols_data_conversion() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_document_symbol_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SymbolProvider(Box::new(|| {
                MaybePromise::Value(Some(ApiDocumentSymbols::Information(vec![symbol_information("test", types::SymbolKind::Field, range(0.0, 0.0, 0.0, 0.0))])))
            }))),
            None,
        );
        env.add(registration);

        env.rpc_protocol.sync().await.expect("sync");
        let value = OutlineModel::create(&env.language_features_service.document_symbol_provider, &env.model, &CancellationToken::None)
            .await
            .expect("OutlineModel.create")
            .as_list_of_document_symbols();
        assert_eq!(value.len(), 1);
        let entry = &value[0];
        assert_eq!(entry.name, "test");
        assert_eq!(entry.range, je_editor::range::Range::new(1, 1, 1, 1));
        env.teardown().await;
    });
}

#[test]
fn quick_outline_uses_a_not_ideal_sorting_138502() {
    run_with_faked_timers(async move {
        // (name, startLineNumber, startColumn, endLineNumber, endColumn)
        let symbols: [(&str, f64, f64, f64, f64); 6] = [
            ("containers", 1.0, 1.0, 4.0, 26.0),
            ("container 0", 2.0, 5.0, 5.0, 1.0),
            ("name", 2.0, 5.0, 2.0, 16.0),
            ("ports", 3.0, 5.0, 5.0, 1.0),
            ("ports 0", 4.0, 9.0, 4.0, 26.0),
            ("containerPort", 4.0, 9.0, 4.0, 26.0),
        ];

        let mut env = Env::new();
        let registration = env.ext_host.register_document_symbol_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SymbolProvider(Box::new(move || {
                MaybePromise::Value(Some(ApiDocumentSymbols::Information(
                    symbols
                        .iter()
                        .map(|(name, start_line, start_column, end_line, end_column)| {
                            symbol_information(name, types::SymbolKind::Object, range(start_line - 1.0, start_column - 1.0, end_line - 1.0, end_column - 1.0))
                        })
                        .collect(),
                )))
            }))),
            None,
        );
        env.add(registration);

        env.rpc_protocol.sync().await.expect("sync");

        let value = OutlineModel::create(&env.language_features_service.document_symbol_provider, &env.model, &CancellationToken::None)
            .await
            .expect("OutlineModel.create")
            .as_list_of_document_symbols();

        assert_eq!(value.len(), 6);
        let names: Vec<&str> = value.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["containers", "container 0", "name", "ports", "ports 0", "containerPort"]);
        env.teardown().await;
    });
}

// ---- provedores dos casos seguintes -------------------------------------------------------------------

/// `'*'`, o seletor mais fraco.
fn wildcard_selector() -> DocumentSelectorInput {
    DocumentSelectorInput::Language("*".to_owned())
}

/// Os `DocumentHighlightProvider` do original são classes anônimas que devolvem um valor fixo ou lançam.
struct HighlightProvider(Box<dyn Fn() -> ApiProviderResult<Vec<types::DocumentHighlight>>>);

impl ApiDocumentHighlightProvider for HighlightProvider {
    fn provide_document_highlights(
        &self,
        _document: &je_workbench::api::common::ext_host_document_data::TextDocument,
        _position: &types::Position,
        _token: CancellationToken,
    ) -> ApiProviderResult<Vec<types::DocumentHighlight>> {
        (self.0)()
    }
}

/// `provideDocumentHighlights() { return [new types.DocumentHighlight(new types.Range(0, 0, 0, end))]; }`.
fn highlight_provider_until(end_character: f64) -> Rc<HighlightProvider> {
    Rc::new(HighlightProvider(Box::new(move || {
        MaybePromise::Value(Some(vec![types::DocumentHighlight::new(range(0.0, 0.0, 0.0, end_character), None)]))
    })))
}

/// Os `CodeActionProvider` do original: um valor fixo ou uma falha.
struct CodeActionsProvider(Box<dyn Fn() -> ApiProviderResult<Vec<CommandOrCodeAction>>>);

impl ApiCodeActionProvider for CodeActionsProvider {
    fn provide_code_actions(
        &self,
        _document: &je_workbench::api::common::ext_host_document_data::TextDocument,
        _range_or_selection: &ApiRangeOrSelection,
        _context: &ApiCodeActionContext,
        _token: CancellationToken,
    ) -> ApiProviderResult<Vec<CommandOrCodeAction>> {
        (self.0)()
    }
}

/// Os `SignatureHelpProvider` do original: um valor fixo ou uma falha.
struct SignatureProvider(Box<dyn Fn() -> ApiProviderResult<ApiSignatureHelpRef>>);

impl ApiSignatureHelpProvider for SignatureProvider {
    fn provide_signature_help(
        &self,
        _document: &je_workbench::api::common::ext_host_document_data::TextDocument,
        _position: &types::Position,
        _token: CancellationToken,
        _context: &ApiSignatureHelpContext,
    ) -> ApiProviderResult<ApiSignatureHelpRef> {
        (self.0)()
    }
}

/// Os `CompletionItemProvider` do original: um valor fixo ou uma falha.
struct CompletionProvider(Box<dyn Fn() -> ApiProviderResult<ApiCompletionResult>>);

impl ApiCompletionItemProvider for CompletionProvider {
    fn provide_completion_items(
        &self,
        _document: &je_workbench::api::common::ext_host_document_data::TextDocument,
        _position: &types::Position,
        _token: CancellationToken,
        _context: &types::CompletionContext,
    ) -> ApiProviderResult<ApiCompletionResult> {
        (self.0)()
    }
}

/// `[new types.CompletionItem(label)]`.
fn completion_items(labels: &[&str]) -> ApiProviderResult<ApiCompletionResult> {
    MaybePromise::Value(Some(ApiCompletionResult::Items(labels.iter().map(|label| completion_item(label)).collect())))
}

/// `new types.CompletionItem(label)`.
fn completion_item(label: &str) -> CompletionItemRef {
    Rc::new(RefCell::new(types::CompletionItem::new(types::CompletionLabel::Text(label.to_owned()), None)))
}

/// `new CompletionOptions(undefined, new Set().add(CompletionItemKind.Snippet))`.
fn snippet_filter_options() -> CompletionOptions {
    CompletionOptions { kind_filter: [CompletionItemKind::Snippet].into_iter().collect(), ..CompletionOptions::default() }
}

struct SelectionRangesProvider(Box<dyn Fn() -> ApiProviderResult<Vec<types::SelectionRange>>>);

impl ApiSelectionRangeProvider for SelectionRangesProvider {
    fn provide_selection_ranges(
        &self,
        _document: &je_workbench::api::common::ext_host_document_data::TextDocument,
        _positions: &[types::Position],
        _token: CancellationToken,
    ) -> ApiProviderResult<Vec<types::SelectionRange>> {
        (self.0)()
    }
}

/// `{ type: CodeActionTriggerType.Invoke, triggerAction }`.
fn invoke_trigger(trigger_action: CodeActionTriggerSource) -> CodeActionTrigger {
    CodeActionTrigger::new(CodeActionTriggerType::Invoke, trigger_action)
}

/// `{ triggerKind: SignatureHelpTriggerKind.Invoke, isRetrigger: false }`.
fn invoke_signature_help_context() -> EditorSignatureHelpContext {
    EditorSignatureHelpContext {
        trigger_kind: EditorSignatureHelpTriggerKind::Invoke,
        trigger_character: None,
        is_retrigger: false,
        active_signature_help: None,
    }
}

// ---- Occurrences --------------------------------------------------------------------------------------

/// O corpo comum dos casos `Occurrences`: o único destaque do modelo, que o caso confere.
async fn single_highlight(env: &Env, end_column: i32) {
    let value = get_occurrences_at_position(&env.language_features_service.document_highlight_provider, &env.model, Position::new(1, 2), &CancellationToken::None, false)
        .await
        .expect("getOccurrencesAtPosition");
    assert_eq!(value.size(), 1);
    let entry = &value.values().next().expect("um recurso")[0];
    assert_eq!(entry.range, je_editor::range::Range::new(1, 1, 1, end_column));
    assert_eq!(entry.kind, Some(DocumentHighlightKind::Text));
}

#[test]
fn occurrences_data_conversion() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_document_highlight_provider(&null_extension_description(), &default_selector(), highlight_provider_until(4.0));
        env.add(registration);

        env.rpc_protocol.sync().await.expect("sync");
        single_highlight(&env, 5).await;
        env.teardown().await;
    });
}

#[test]
fn occurrences_order_1_2() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let first = env.ext_host.register_document_highlight_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(HighlightProvider(Box::new(|| MaybePromise::Value(None)))),
        );
        env.add(first);
        let second = env.ext_host.register_document_highlight_provider(&null_extension_description(), &wildcard_selector(), highlight_provider_until(4.0));
        env.add(second);

        env.rpc_protocol.sync().await.expect("sync");
        single_highlight(&env, 5).await;
        env.teardown().await;
    });
}

#[test]
fn occurrences_order_2_2() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let first = env.ext_host.register_document_highlight_provider(&null_extension_description(), &default_selector(), highlight_provider_until(2.0));
        env.add(first);
        let second = env.ext_host.register_document_highlight_provider(&null_extension_description(), &wildcard_selector(), highlight_provider_until(4.0));
        env.add(second);

        env.rpc_protocol.sync().await.expect("sync");
        single_highlight(&env, 3).await;
        env.teardown().await;
    });
}

#[test]
fn occurrences_evil_provider() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let evil = env.ext_host.register_document_highlight_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(HighlightProvider(Box::new(|| MaybePromise::Promise(Promise::rejected(Error::new("evil")))))),
        );
        env.add(evil);
        let good = env.ext_host.register_document_highlight_provider(&null_extension_description(), &default_selector(), highlight_provider_until(4.0));
        env.add(good);

        env.rpc_protocol.sync().await.expect("sync");
        let value = get_occurrences_at_position(&env.language_features_service.document_highlight_provider, &env.model, Position::new(1, 2), &CancellationToken::None, false)
            .await
            .expect("getOccurrencesAtPosition");
        assert_eq!(value.size(), 1);
        env.teardown().await;
    });
}

// ---- Quick Fix ----------------------------------------------------------------------------------------

#[test]
fn quick_fix_command_data_conversion() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CodeActionsProvider(Box::new(|| {
                MaybePromise::Value(Some(vec![
                    CommandOrCodeAction::Command(types::Command::new("Testing1", "test1")),
                    CommandOrCodeAction::Command(types::Command::new("Testing2", "test2")),
                ]))
            }))),
            None,
        );
        env.add(registration);

        env.rpc_protocol.sync().await.expect("sync");
        let value = get_code_actions(
            &env.language_features_service.code_action_provider,
            &env.model,
            env.model.get_full_model_range().into(),
            &invoke_trigger(CodeActionTriggerSource::QuickFix),
            Rc::new(ProgressNone),
            &CancellationToken::None,
        )
        .await
        .expect("getCodeActions");
        let actions = value.valid_actions();
        assert_eq!(actions.len(), 2);
        let (first, second) = (&actions[0], &actions[1]);
        assert_eq!(first.action.borrow().title, "Testing1");
        assert_eq!(first.action.borrow().command.as_ref().expect("command").id, "test1");
        assert_eq!(second.action.borrow().title, "Testing2");
        assert_eq!(second.action.borrow().command.as_ref().expect("command").id, "test2");
        value.dispose();
        env.teardown().await;
    });
}

#[test]
fn quick_fix_code_action_data_conversion() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CodeActionsProvider(Box::new(|| {
                let mut action = types::CodeAction::new("Testing1", Some(types::CodeActionKind::empty().append("test.scope")));
                action.command = Some(types::Command::new("Testing1Command", "test1"));
                MaybePromise::Value(Some(vec![CommandOrCodeAction::CodeAction(Rc::new(RefCell::new(action)))]))
            }))),
            None,
        );
        env.add(registration);

        env.rpc_protocol.sync().await.expect("sync");
        let value = get_code_actions(
            &env.language_features_service.code_action_provider,
            &env.model,
            env.model.get_full_model_range().into(),
            &invoke_trigger(CodeActionTriggerSource::Default),
            Rc::new(ProgressNone),
            &CancellationToken::None,
        )
        .await
        .expect("getCodeActions");
        let actions = value.valid_actions();
        assert_eq!(actions.len(), 1);
        let action = actions[0].action.borrow();
        assert_eq!(action.title, "Testing1");
        let command = action.command.as_ref().expect("command");
        assert_eq!(command.title, "Testing1Command");
        assert_eq!(command.id, "test1");
        assert_eq!(action.kind.as_deref(), Some("test.scope"));
        drop(action);
        value.dispose();
        env.teardown().await;
    });
}

#[test]
fn cannot_read_property_id_of_undefined_29469() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        // `[undefined, null, { command: 'test', title: 'Testing' }]`: o `Vec` não tem elementos ausentes (ver a nota do
        // `CodeActionAdapter`), então só o terceiro elemento existe.
        let registration = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CodeActionsProvider(Box::new(|| MaybePromise::Value(Some(vec![CommandOrCodeAction::Command(types::Command::new("Testing", "test"))]))))),
            None,
        );
        env.add(registration);

        env.rpc_protocol.sync().await.expect("sync");
        let value = get_code_actions(
            &env.language_features_service.code_action_provider,
            &env.model,
            env.model.get_full_model_range().into(),
            &invoke_trigger(CodeActionTriggerSource::Default),
            Rc::new(ProgressNone),
            &CancellationToken::None,
        )
        .await
        .expect("getCodeActions");
        assert_eq!(value.valid_actions().len(), 1);
        value.dispose();
        env.teardown().await;
    });
}

#[test]
fn quick_fix_evil_provider() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let evil = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CodeActionsProvider(Box::new(|| MaybePromise::Promise(Promise::rejected(Error::new("evil")))))),
            None,
        );
        env.add(evil);
        let good = env.ext_host.register_code_action_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CodeActionsProvider(Box::new(|| MaybePromise::Value(Some(vec![CommandOrCodeAction::Command(types::Command::new("Testing", "test"))]))))),
            None,
        );
        env.add(good);

        env.rpc_protocol.sync().await.expect("sync");
        let value = get_code_actions(
            &env.language_features_service.code_action_provider,
            &env.model,
            env.model.get_full_model_range().into(),
            &invoke_trigger(CodeActionTriggerSource::QuickFix),
            Rc::new(ProgressNone),
            &CancellationToken::None,
        )
        .await
        .expect("getCodeActions");
        assert_eq!(value.valid_actions().len(), 1);
        value.dispose();
        env.teardown().await;
    });
}

// ---- Parameter Hints ----------------------------------------------------------------------------------

#[test]
fn parameter_hints_order() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let first = env.ext_host.register_signature_help_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SignatureProvider(Box::new(|| MaybePromise::Value(None)))),
            SignatureHelpMetadataOrTriggerChars::TriggerCharacters(Vec::new()),
        );
        env.add(first);
        let second = env.ext_host.register_signature_help_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SignatureProvider(Box::new(|| MaybePromise::Value(Some(Rc::new(RefCell::new(types::SignatureHelp::default()))))))),
            SignatureHelpMetadataOrTriggerChars::TriggerCharacters(Vec::new()),
        );
        env.add(second);

        env.rpc_protocol.sync().await.expect("sync");
        let value = provide_signature_help(
            &env.language_features_service.signature_help_provider,
            &env.model,
            Position::new(1, 1),
            invoke_signature_help_context(),
            &CancellationToken::None,
        )
        .await
        .expect("provideSignatureHelp");
        assert!(value.is_some());
        env.teardown().await;
    });
}

#[test]
fn parameter_hints_evil_provider() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let evil = env.ext_host.register_signature_help_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SignatureProvider(Box::new(|| MaybePromise::Promise(Promise::rejected(Error::new("evil")))))),
            SignatureHelpMetadataOrTriggerChars::TriggerCharacters(Vec::new()),
        );
        env.add(evil);

        env.rpc_protocol.sync().await.expect("sync");
        let value = provide_signature_help(
            &env.language_features_service.signature_help_provider,
            &env.model,
            Position::new(1, 1),
            invoke_signature_help_context(),
            &CancellationToken::None,
        )
        .await
        .expect("provideSignatureHelp");
        assert!(value.is_none());
        env.teardown().await;
    });
}

// ---- Suggest ------------------------------------------------------------------------------------------

#[test]
fn suggest_order_1_3() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let weak = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &wildcard_selector(),
            Rc::new(CompletionProvider(Box::new(|| completion_items(&["testing1"])))),
            Vec::new(),
        );
        env.add(weak);
        let strong = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CompletionProvider(Box::new(|| completion_items(&["testing2"])))),
            Vec::new(),
        );
        env.add(strong);

        env.rpc_protocol.sync().await.expect("sync");
        let value = provide_suggestion_items(
            &env.language_features_service.completion_provider,
            &env.model,
            Position::new(1, 1),
            snippet_filter_options(),
            CompletionContext::default(),
            &CancellationToken::None,
        )
        .await
        .expect("provideSuggestionItems");
        assert_eq!(value.items.len(), 1);
        assert_eq!(value.items[0].completion.borrow().insert_text, "testing2");
        value.disposable.dispose();
        env.teardown().await;
    });
}

#[test]
fn suggest_order_2_3() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        // weaker selector but result
        let weak = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &wildcard_selector(),
            Rc::new(CompletionProvider(Box::new(|| completion_items(&["weak-selector"])))),
            Vec::new(),
        );
        env.add(weak);
        // stronger selector but not a good result
        let strong = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CompletionProvider(Box::new(|| completion_items(&[])))),
            Vec::new(),
        );
        env.add(strong);

        env.rpc_protocol.sync().await.expect("sync");
        let value = provide_suggestion_items(
            &env.language_features_service.completion_provider,
            &env.model,
            Position::new(1, 1),
            snippet_filter_options(),
            CompletionContext::default(),
            &CancellationToken::None,
        )
        .await
        .expect("provideSuggestionItems");
        assert_eq!(value.items.len(), 1);
        assert_eq!(value.items[0].completion.borrow().insert_text, "weak-selector");
        value.disposable.dispose();
        env.teardown().await;
    });
}

#[test]
fn suggest_order_3_3() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let first = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CompletionProvider(Box::new(|| completion_items(&["strong-1"])))),
            Vec::new(),
        );
        env.add(first);
        let second = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CompletionProvider(Box::new(|| completion_items(&["strong-2"])))),
            Vec::new(),
        );
        env.add(second);

        env.rpc_protocol.sync().await.expect("sync");
        let value = provide_suggestion_items(
            &env.language_features_service.completion_provider,
            &env.model,
            Position::new(1, 1),
            snippet_filter_options(),
            CompletionContext::default(),
            &CancellationToken::None,
        )
        .await
        .expect("provideSuggestionItems");
        assert_eq!(value.items.len(), 2);
        assert_eq!(value.items[0].completion.borrow().insert_text, "strong-1"); // sort by label
        assert_eq!(value.items[1].completion.borrow().insert_text, "strong-2");
        value.disposable.dispose();
        env.teardown().await;
    });
}

#[test]
fn suggest_evil_provider() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let evil = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CompletionProvider(Box::new(|| MaybePromise::Promise(Promise::rejected(Error::new("evil")))))),
            Vec::new(),
        );
        env.add(evil);
        let good = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CompletionProvider(Box::new(|| completion_items(&["testing"])))),
            Vec::new(),
        );
        env.add(good);

        env.rpc_protocol.sync().await.expect("sync");
        let value = provide_suggestion_items(
            &env.language_features_service.completion_provider,
            &env.model,
            Position::new(1, 1),
            snippet_filter_options(),
            CompletionContext::default(),
            &CancellationToken::None,
        )
        .await
        .expect("provideSuggestionItems");
        assert_eq!(value.items[0].container.incomplete, Some(false));
        value.disposable.dispose();
        env.teardown().await;
    });
}

#[test]
fn suggest_completion_list() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_completion_item_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(CompletionProvider(Box::new(|| {
                MaybePromise::Value(Some(ApiCompletionResult::List(ApiCompletionList { is_incomplete: Some(true), items: vec![completion_item("hello")] })))
            }))),
            Vec::new(),
        );
        env.add(registration);

        env.rpc_protocol.sync().await.expect("sync");
        let model = provide_suggestion_items(
            &env.language_features_service.completion_provider,
            &env.model,
            Position::new(1, 1),
            snippet_filter_options(),
            CompletionContext::default(),
            &CancellationToken::None,
        )
        .await
        .expect("provideSuggestionItems");
        assert_eq!(model.items[0].container.incomplete, Some(true));
        model.disposable.dispose();
        env.teardown().await;
    });
}

// ---- Selection Ranges ---------------------------------------------------------------------------------

#[test]
fn selection_ranges_data_conversion() {
    run_with_faked_timers(async move {
        let mut env = Env::new();
        let registration = env.ext_host.register_selection_range_provider(
            &null_extension_description(),
            &default_selector(),
            Rc::new(SelectionRangesProvider(Box::new(|| {
                let parent = types::SelectionRange::new(range(0.0, 2.0, 0.0, 20.0), None).expect("SelectionRange");
                MaybePromise::Value(Some(vec![types::SelectionRange::new(range(0.0, 10.0, 0.0, 18.0), Some(parent)).expect("SelectionRange")]))
            }))),
        );
        env.add(registration);

        env.rpc_protocol.sync().await.expect("sync");
        let options = SmartSelectOptions { select_leading_and_trailing_whitespace: true, select_subwords: true };
        let ranges = provide_selection_ranges(&env.language_features_service.selection_range_provider, &env.model, &[Position::new(1, 17)], options, &CancellationToken::None)
            .await
            .expect("provideSelectionRanges");
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].len() >= 2);
        env.teardown().await;
    });
}

#[test]
fn selection_ranges_bad_data() {
    run_with_faked_timers(async move {
        let env = Env::new();
        // O pai (0,11 a 0,18) não contém o filho (0,10 a 0,18): o construtor rejeita.
        let parent = types::SelectionRange::new(range(0.0, 11.0, 0.0, 18.0), None).expect("SelectionRange");
        assert!(types::SelectionRange::new(range(0.0, 10.0, 0.0, 18.0), Some(parent)).is_err());
        env.teardown().await;
    });
}
