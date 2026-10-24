//! Porte de `src/vs/workbench/api/test/browser/extHostLanguageFeatures.test.ts` (suíte `ExtHostLanguageFeatures`), caso a caso e na mesma ordem.
//!
//! PORTADOS (58 de 65 `test(` do original, contando cada caso dos laços `for (const declinedRequests of [0, 2])` como dois):
//! `AutoClosingPair.beforeText requires proposed API`, `DocumentDropEdits does not cache an empty edit array without release IDs`,
//! `DocumentSymbols` x4 (incluindo `Quick Outline ... #138502`), `Occurrences` x4, `Quick Fix` x3 e `#29469`, `Parameter Hints` x2,
//! `Suggest` x5 e `Selection Ranges` x2 (23, no corpo do arquivo).
//! No módulo `navigation_and_formatting`, ao fim do arquivo (35): CodeLens x3, Definition x4, Declaration, Implementation, Type
//! Definition, HoverProvider x4, References x3, Navigate types x2, Rename x6 (os quatro de `rename` e os dois #98352), Format Doc x3,
//! Format Range x3, Format on Type, Links x2 e Document colors.
//! Divergência de tradução: o `#29469` devolve `[undefined, null, comando]` e o `Vec` não tem elementos ausentes, então o caso
//! devolve só o comando (o `if (!candidate) continue` do adaptador não existe no porte).
//!
//! FORA, e por quê (nenhum vira `#[test]` enquanto não puder ser fiel):
//! - Os comandos `_execute*`, `FormattingConflicts` e `formatDocumentWith*`, que precisam de `ServicesAccessor`.
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

// ---- bloco da fatia w141: lentes de código, localização, hover, referências, símbolos do workspace, renomeação,
// ---- formatação, links e cores. Os imports ficam no módulo para não colidir com os dos outros blocos do arquivo.

mod navigation_and_formatting {
    use std::cell::RefCell;

    use super::*;
    use je_base::uri::UriComponents;
    use je_editor::code_action::languages::TextEdit as EditorTextEdit;
    use je_editor::codelens::get_code_lens_model;
    use je_editor::color::{DefaultColorDecoratorsEnablement, get_colors};
    use je_editor::editor_worker::{IEditorWorkerService, WordRanges};
    use je_editor::format::{
        get_document_formatting_edits_until_result, get_document_range_formatting_edits_until_result, get_on_type_formatting_edits,
    };
    use je_editor::get_links::get_links;
    use je_editor::goto_symbol::{get_location_links_at_position, get_references_at_position};
    use je_editor::hover::get_hover::get_hovers_promise;
    use je_editor::language_features_service::{FormattingOptions as EditorFormattingOptions, IColor};
    use je_editor::languages::LinkUrl;
    use je_editor::model::EndOfLineSequence;
    use je_editor::range::Range as EditorRange;
    use je_editor::rename::rename;
    use je_workbench::api::browser::main_thread_language_features::registries::get_workspace_symbols;
    use je_workbench::api::common::ext_host_document_data::TextDocument;
    use je_workbench::api::common::ext_host_language_features_api::{
        ApiCodeLensProvider, ApiColorPresentationContext, ApiDocumentColorProvider, ApiDocumentFormattingEditProvider, ApiDocumentLinkProvider,
        ApiDocumentRangeFormattingEditProvider, ApiHoverContext, ApiHoverProvider, ApiLocationLinkProvider, ApiLocationResult,
        ApiOnTypeFormattingEditProvider, ApiPrepareRename, ApiReferenceProvider, ApiRenameProvider, ApiWorkspaceSymbolProvider, CodeLensRef,
        DocumentLinkRef, LocationOrLink,
    };
    use je_workbench::api::common::ext_host_protocol::{FormattingOptions as ApiFormattingOptions, ReferenceContext};

    // ---- auxiliares -----------------------------------------------------------------------------------

    /// `ProviderResult` que devolve o valor.
    fn found<T: Clone + 'static>(value: T) -> ApiProviderResult<T> {
        MaybePromise::Value(Some(value))
    }

    /// `throw new Error(message)` do provedor (uma promessa rejeitada).
    fn evil<T: Clone + 'static>(message: &str) -> ApiProviderResult<T> {
        MaybePromise::Promise(Promise::rejected(Error::new(message)))
    }

    /// `provideX(): any { return undefined; }`.
    fn nothing<T: Clone + 'static>() -> ApiProviderResult<T> {
        MaybePromise::Value(None)
    }

    fn api_location(uri: &URI, range: types::Range) -> types::Location {
        types::Location::new(Some(uri.clone()), Some(types::RangeOrPosition::Range(range)))
    }

    fn many_locations(locations: Vec<types::Location>) -> ApiProviderResult<ApiLocationResult> {
        found(ApiLocationResult::Many(locations.into_iter().map(LocationOrLink::Location).collect()))
    }

    fn single_location(location: types::Location) -> ApiProviderResult<ApiLocationResult> {
        found(ApiLocationResult::Single(LocationOrLink::Location(location)))
    }

    fn uri_text(uri: &URI) -> String {
        uri.to_string(false).to_string_lossy()
    }

    fn lens(range: types::Range, command: Option<types::Command>) -> CodeLensRef {
        Rc::new(RefCell::new(types::CodeLens::new(range, command)))
    }

    fn editor_options(tab_size: f64) -> EditorFormattingOptions {
        EditorFormattingOptions { tab_size, insert_spaces: true }
    }

    // ---- os provedores (as classes anônimas do original) ---------------------------------------------

    /// `vscode.CodeLensProvider`. O `resolveCodeLens` do caso "do not resolve a resolved lens" falha se for chamado.
    struct LensProvider {
        provide: Box<dyn Fn() -> ApiProviderResult<Vec<CodeLensRef>>>,
        fails_on_resolve: bool,
    }

    impl LensProvider {
        fn new(provide: impl Fn() -> ApiProviderResult<Vec<CodeLensRef>> + 'static) -> Rc<Self> {
            Rc::new(Self { provide: Box::new(provide), fails_on_resolve: false })
        }
    }

    impl ApiCodeLensProvider for LensProvider {
        fn provide_code_lenses(&self, _document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<Vec<CodeLensRef>> {
            (self.provide)()
        }

        fn has_resolve_code_lens(&self) -> bool {
            self.fails_on_resolve
        }

        fn resolve_code_lens(&self, _code_lens: &CodeLensRef, _token: CancellationToken) -> ApiProviderResult<CodeLensRef> {
            // `assert.ok(false, 'do not resolve')`
            evil("do not resolve")
        }
    }

    /// `vscode.DefinitionProvider`, `DeclarationProvider`, `ImplementationProvider` e `TypeDefinitionProvider`.
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

    /// `vscode.HoverProvider`.
    struct HoverProvider(Box<dyn Fn() -> ApiProviderResult<types::VerboseHover>>);

    impl HoverProvider {
        fn new(provide: impl Fn() -> ApiProviderResult<types::VerboseHover> + 'static) -> Rc<Self> {
            Rc::new(Self(Box::new(provide)))
        }
    }

    impl ApiHoverProvider for HoverProvider {
        fn provide_hover(
            &self,
            _document: &TextDocument,
            _position: &types::Position,
            _token: CancellationToken,
            _context: Option<&ApiHoverContext>,
        ) -> ApiProviderResult<types::VerboseHover> {
            (self.0)()
        }
    }

    /// `new types.Hover(text, range?)`.
    fn hover(text: &str, hover_range: Option<types::Range>) -> ApiProviderResult<types::VerboseHover> {
        found(types::VerboseHover::new(vec![types::MarkedString::Text(text.to_owned())], hover_range, None, None))
    }

    /// `vscode.ReferenceProvider`.
    struct ReferenceProvider(Box<dyn Fn() -> ApiProviderResult<Vec<types::Location>>>);

    impl ReferenceProvider {
        fn new(provide: impl Fn() -> ApiProviderResult<Vec<types::Location>> + 'static) -> Rc<Self> {
            Rc::new(Self(Box::new(provide)))
        }
    }

    impl ApiReferenceProvider for ReferenceProvider {
        fn provide_references(
            &self,
            _document: &TextDocument,
            _position: &types::Position,
            _context: &ReferenceContext,
            _token: CancellationToken,
        ) -> ApiProviderResult<Vec<types::Location>> {
            (self.0)()
        }
    }

    /// `vscode.WorkspaceSymbolProvider`. `has_resolve` é a presença do `resolveWorkspaceSymbol`.
    struct WorkspaceSymbolProvider {
        provide: Box<dyn Fn() -> ApiProviderResult<Vec<types::SymbolInformation>>>,
        has_resolve: bool,
    }

    impl WorkspaceSymbolProvider {
        fn new(has_resolve: bool, provide: impl Fn() -> ApiProviderResult<Vec<types::SymbolInformation>> + 'static) -> Rc<Self> {
            Rc::new(Self { provide: Box::new(provide), has_resolve })
        }
    }

    impl ApiWorkspaceSymbolProvider for WorkspaceSymbolProvider {
        fn provide_workspace_symbols(&self, _query: &str, _token: CancellationToken) -> ApiProviderResult<Vec<types::SymbolInformation>> {
            (self.provide)()
        }

        fn has_resolve_workspace_symbol(&self) -> bool {
            self.has_resolve
        }

        fn resolve_workspace_symbol(&self, symbol: &types::SymbolInformation, _token: CancellationToken) -> ApiProviderResult<types::SymbolInformation> {
            found(symbol.clone())
        }
    }

    /// `vscode.RenameProvider`. `prepare` é o `prepareRename` opcional.
    struct RenameProvider {
        prepare: Option<Box<dyn Fn(&TextDocument, &types::Position) -> ApiProviderResult<ApiPrepareRename>>>,
        edits: Box<dyn Fn() -> ApiProviderResult<types::WorkspaceEdit>>,
    }

    impl RenameProvider {
        fn new(edits: impl Fn() -> ApiProviderResult<types::WorkspaceEdit> + 'static) -> Self {
            Self { prepare: None, edits: Box::new(edits) }
        }

        fn with_prepare(mut self, prepare: impl Fn(&TextDocument, &types::Position) -> ApiProviderResult<ApiPrepareRename> + 'static) -> Rc<Self> {
            self.prepare = Some(Box::new(prepare));
            Rc::new(self)
        }
    }

    impl ApiRenameProvider for RenameProvider {
        fn provide_rename_edits(&self, _document: &TextDocument, _position: &types::Position, _new_name: &str, _token: CancellationToken) -> ApiProviderResult<types::WorkspaceEdit> {
            (self.edits)()
        }

        fn has_prepare_rename(&self) -> bool {
            self.prepare.is_some()
        }

        fn prepare_rename(&self, document: &TextDocument, position: &types::Position, _token: CancellationToken) -> ApiProviderResult<ApiPrepareRename> {
            (self.prepare.as_ref().expect("prepareRename"))(document, position)
        }
    }

    /// `workspaceEdit.replace(uri, range, text)` sobre uma edição nova.
    fn replace_edit(uri: &URI, ranges: &[types::Range]) -> types::WorkspaceEdit {
        let mut edit = types::WorkspaceEdit::new();
        for range in ranges {
            edit.replace(uri, *range, "testing", None);
        }
        edit
    }

    /// `vscode.DocumentFormattingEditProvider`.
    struct FormattingProvider(Box<dyn Fn() -> ApiProviderResult<Vec<types::TextEdit>>>);

    impl FormattingProvider {
        fn new(provide: impl Fn() -> ApiProviderResult<Vec<types::TextEdit>> + 'static) -> Rc<Self> {
            Rc::new(Self(Box::new(provide)))
        }
    }

    impl ApiDocumentFormattingEditProvider for FormattingProvider {
        fn provide_document_formatting_edits(&self, _document: &TextDocument, _options: &ApiFormattingOptions, _token: CancellationToken) -> ApiProviderResult<Vec<types::TextEdit>> {
            (self.0)()
        }
    }

    /// `vscode.DocumentRangeFormattingEditProvider`.
    struct RangeFormattingProvider(Box<dyn Fn() -> ApiProviderResult<Vec<types::TextEdit>>>);

    impl RangeFormattingProvider {
        fn new(provide: impl Fn() -> ApiProviderResult<Vec<types::TextEdit>> + 'static) -> Rc<Self> {
            Rc::new(Self(Box::new(provide)))
        }
    }

    impl ApiDocumentRangeFormattingEditProvider for RangeFormattingProvider {
        fn provide_document_range_formatting_edits(
            &self,
            _document: &TextDocument,
            _range: &types::Range,
            _options: &ApiFormattingOptions,
            _token: CancellationToken,
        ) -> ApiProviderResult<Vec<types::TextEdit>> {
            (self.0)()
        }
    }

    /// `vscode.OnTypeFormattingEditProvider` que devolve o `ch` recebido (`arguments[2]`).
    struct EchoOnTypeProvider;

    impl ApiOnTypeFormattingEditProvider for EchoOnTypeProvider {
        fn provide_on_type_formatting_edits(
            &self,
            _document: &TextDocument,
            _position: &types::Position,
            ch: &str,
            _options: &ApiFormattingOptions,
            _token: CancellationToken,
        ) -> ApiProviderResult<Vec<types::TextEdit>> {
            found(vec![types::TextEdit::replace(range(0.0, 0.0, 0.0, 0.0), ch)])
        }
    }

    /// `vscode.DocumentLinkProvider`.
    struct LinkProvider(Box<dyn Fn() -> ApiProviderResult<Vec<DocumentLinkRef>>>);

    impl LinkProvider {
        fn new(provide: impl Fn() -> ApiProviderResult<Vec<DocumentLinkRef>> + 'static) -> Rc<Self> {
            Rc::new(Self(Box::new(provide)))
        }
    }

    impl ApiDocumentLinkProvider for LinkProvider {
        fn provide_document_links(&self, _document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<Vec<DocumentLinkRef>> {
            (self.0)()
        }
    }

    /// `new types.DocumentLink(new types.Range(0, 0, 1, 1), URI.parse('foo:bar#3'))`.
    fn document_link() -> types::DocumentLink {
        types::DocumentLink::new(range(0.0, 0.0, 1.0, 1.0), Some(URI::parse("foo:bar#3", false))).expect("DocumentLink")
    }

    /// `vscode.DocumentColorProvider` (as apresentações são sempre vazias).
    struct ColorProvider;

    impl ApiDocumentColorProvider for ColorProvider {
        fn provide_document_colors(&self, _document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<Vec<types::ColorInformation>> {
            found(vec![types::ColorInformation::new(range(0.0, 0.0, 0.0, 20.0), types::Color::new(0.1, 0.2, 0.3, 0.4)).expect("ColorInformation")])
        }

        fn provide_color_presentations(
            &self,
            _color: &types::Color,
            _context: &ApiColorPresentationContext,
            _token: CancellationToken,
        ) -> ApiProviderResult<Vec<types::ColorPresentation>> {
            found(Vec::new())
        }
    }

    /// `NullWorkerService`: `computeMoreMinimalEdits(resource, edits) { return Promise.resolve(edits ?? undefined); }`.
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

    fn null_worker_service() -> Rc<dyn IEditorWorkerService> {
        Rc::new(NullWorkerService)
    }

    // ---- code lens ------------------------------------------------------------------------------------

    #[test]
    fn code_lens_evil_provider() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let bad = env.ext_host.register_code_lens_provider(&null_extension_description(), &default_selector(), LensProvider::new(|| evil("evil")));
            env.add(bad);
            let good = env.ext_host.register_code_lens_provider(
                &null_extension_description(),
                &default_selector(),
                LensProvider::new(|| found(vec![lens(range(0.0, 0.0, 0.0, 0.0), None)])),
            );
            env.add(good);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_code_lens_model(&env.language_features_service.code_lens_provider, &env.model, &CancellationToken::None).await.expect("getCodeLensModel");
            assert_eq!(value.lenses.len(), 1);
            value.dispose();
            env.teardown().await;
        });
    }

    #[test]
    fn code_lens_do_not_resolve_a_resolved_lens() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let provider = Rc::new(LensProvider {
                provide: Box::new(|| found(vec![lens(range(0.0, 0.0, 0.0, 0.0), Some(types::Command::new("Title", "id")))])),
                fails_on_resolve: true,
            });
            let registration = env.ext_host.register_code_lens_provider(&null_extension_description(), &default_selector(), provider);
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_code_lens_model(&env.language_features_service.code_lens_provider, &env.model, &CancellationToken::None).await.expect("getCodeLensModel");
            assert_eq!(value.lenses.len(), 1);
            let data = &value.lenses[0];
            let symbol = data.provider.resolve_code_lens(&env.model, &data.symbol, &CancellationToken::None).await.expect("resolveCodeLens").expect("symbol");
            let command = symbol.command.expect("command");
            assert_eq!(command.id, "id");
            assert_eq!(command.title, "Title");
            value.dispose();
            env.teardown().await;
        });
    }

    #[test]
    fn code_lens_missing_command() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env.ext_host.register_code_lens_provider(
                &null_extension_description(),
                &default_selector(),
                LensProvider::new(|| found(vec![lens(range(0.0, 0.0, 0.0, 0.0), None)])),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_code_lens_model(&env.language_features_service.code_lens_provider, &env.model, &CancellationToken::None).await.expect("getCodeLensModel");
            assert_eq!(value.lenses.len(), 1);
            let data = &value.lenses[0];
            let symbol = data.provider.resolve_code_lens(&env.model, &data.symbol, &CancellationToken::None).await.expect("resolveCodeLens");
            assert!(symbol.is_none());
            value.dispose();
            env.teardown().await;
        });
    }

    // ---- definition, declaration, implementation, type definition -------------------------------------

    #[test]
    fn definition_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let registration = env.ext_host.register_definition_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(move || many_locations(vec![api_location(&uri, range(1.0, 2.0, 3.0, 4.0))])),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_location_links_at_position(&env.language_features_service.definition_provider, &env.model, Position::new(1, 1), false, &CancellationToken::None)
                .await
                .expect("getDefinitionsAtPosition");
            assert_eq!(value.len(), 1);
            let entry = &value[0];
            assert_eq!(entry.range, EditorRange::new(2, 3, 4, 5));
            assert_eq!(uri_text(&entry.uri), uri_text(&env.model.uri()));
            env.teardown().await;
        });
    }

    #[test]
    fn definition_one_or_many() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let (first_uri, second_uri) = (env.model.uri(), env.model.uri());
            let first = env.ext_host.register_definition_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(move || many_locations(vec![api_location(&first_uri, range(1.0, 1.0, 1.0, 1.0))])),
            );
            env.add(first);
            let second = env.ext_host.register_definition_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(move || single_location(api_location(&second_uri, range(2.0, 1.0, 1.0, 1.0)))),
            );
            env.add(second);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_location_links_at_position(&env.language_features_service.definition_provider, &env.model, Position::new(1, 1), false, &CancellationToken::None)
                .await
                .expect("getDefinitionsAtPosition");
            assert_eq!(value.len(), 2);
            env.teardown().await;
        });
    }

    #[test]
    fn definition_registration_order() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let first = env.ext_host.register_definition_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(|| many_locations(vec![api_location(&URI::parse("far://first", false), range(2.0, 3.0, 4.0, 5.0))])),
            );
            env.add(first);
            let second = env.ext_host.register_definition_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(|| single_location(api_location(&URI::parse("far://second", false), range(1.0, 2.0, 3.0, 4.0)))),
            );
            env.add(second);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_location_links_at_position(&env.language_features_service.definition_provider, &env.model, Position::new(1, 1), false, &CancellationToken::None)
                .await
                .expect("getDefinitionsAtPosition");
            assert_eq!(value.len(), 2);
            assert_eq!(value[0].uri.authority().to_string_lossy(), "second");
            assert_eq!(value[1].uri.authority().to_string_lossy(), "first");
            env.teardown().await;
        });
    }

    #[test]
    fn definition_evil_provider() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let bad = env.ext_host.register_definition_provider(&null_extension_description(), &default_selector(), LocationProvider::new(|| evil("evil provider")));
            env.add(bad);
            let good = env.ext_host.register_definition_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(move || single_location(api_location(&uri, range(1.0, 1.0, 1.0, 1.0)))),
            );
            env.add(good);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_location_links_at_position(&env.language_features_service.definition_provider, &env.model, Position::new(1, 1), false, &CancellationToken::None)
                .await
                .expect("getDefinitionsAtPosition");
            assert_eq!(value.len(), 1);
            env.teardown().await;
        });
    }

    #[test]
    fn declaration_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let registration = env.ext_host.register_declaration_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(move || many_locations(vec![api_location(&uri, range(1.0, 2.0, 3.0, 4.0))])),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_location_links_at_position(&env.language_features_service.declaration_provider, &env.model, Position::new(1, 1), false, &CancellationToken::None)
                .await
                .expect("getDeclarationsAtPosition");
            assert_eq!(value.len(), 1);
            let entry = &value[0];
            assert_eq!(entry.range, EditorRange::new(2, 3, 4, 5));
            assert_eq!(uri_text(&entry.uri), uri_text(&env.model.uri()));
            env.teardown().await;
        });
    }

    #[test]
    fn implementation_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let registration = env.ext_host.register_implementation_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(move || many_locations(vec![api_location(&uri, range(1.0, 2.0, 3.0, 4.0))])),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_location_links_at_position(&env.language_features_service.implementation_provider, &env.model, Position::new(1, 1), false, &CancellationToken::None)
                .await
                .expect("getImplementationsAtPosition");
            assert_eq!(value.len(), 1);
            let entry = &value[0];
            assert_eq!(entry.range, EditorRange::new(2, 3, 4, 5));
            assert_eq!(uri_text(&entry.uri), uri_text(&env.model.uri()));
            env.teardown().await;
        });
    }

    #[test]
    fn type_definition_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let registration = env.ext_host.register_type_definition_provider(
                &null_extension_description(),
                &default_selector(),
                LocationProvider::new(move || many_locations(vec![api_location(&uri, range(1.0, 2.0, 3.0, 4.0))])),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_location_links_at_position(&env.language_features_service.type_definition_provider, &env.model, Position::new(1, 1), false, &CancellationToken::None)
                .await
                .expect("getTypeDefinitionsAtPosition");
            assert_eq!(value.len(), 1);
            let entry = &value[0];
            assert_eq!(entry.range, EditorRange::new(2, 3, 4, 5));
            assert_eq!(uri_text(&entry.uri), uri_text(&env.model.uri()));
            env.teardown().await;
        });
    }

    // ---- hover ----------------------------------------------------------------------------------------

    #[test]
    fn hover_provider_word_range_at_pos() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env.ext_host.register_hover_provider(&null_extension_description(), &default_selector(), HoverProvider::new(|| hover("Hello", None)));
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let hovers = get_hovers_promise(&env.language_features_service.hover_provider, &env.model, Position::new(1, 1), &CancellationToken::None, false)
                .await
                .expect("getHoversPromise");
            assert_eq!(hovers.len(), 1);
            assert_eq!(hovers[0].range, Some(EditorRange::new(1, 1, 1, 5)));
            env.teardown().await;
        });
    }

    #[test]
    fn hover_provider_given_range() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env
                .ext_host
                .register_hover_provider(&null_extension_description(), &default_selector(), HoverProvider::new(|| hover("Hello", Some(range(3.0, 0.0, 8.0, 7.0)))));
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let hovers = get_hovers_promise(&env.language_features_service.hover_provider, &env.model, Position::new(1, 1), &CancellationToken::None, false)
                .await
                .expect("getHoversPromise");
            assert_eq!(hovers.len(), 1);
            assert_eq!(hovers[0].range, Some(EditorRange::new(4, 1, 9, 8)));
            env.teardown().await;
        });
    }

    #[test]
    fn hover_provider_registration_order() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let first = env.ext_host.register_hover_provider(&null_extension_description(), &default_selector(), HoverProvider::new(|| hover("registered first", None)));
            env.add(first);
            let second = env.ext_host.register_hover_provider(&null_extension_description(), &default_selector(), HoverProvider::new(|| hover("registered second", None)));
            env.add(second);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_hovers_promise(&env.language_features_service.hover_provider, &env.model, Position::new(1, 1), &CancellationToken::None, false)
                .await
                .expect("getHoversPromise");
            assert_eq!(value.len(), 2);
            assert_eq!(value[0].contents[0].value.to_string_lossy(), "registered second");
            assert_eq!(value[1].contents[0].value.to_string_lossy(), "registered first");
            env.teardown().await;
        });
    }

    #[test]
    fn hover_provider_evil_provider() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let bad = env.ext_host.register_hover_provider(&null_extension_description(), &default_selector(), HoverProvider::new(|| evil("evil")));
            env.add(bad);
            let good = env.ext_host.register_hover_provider(&null_extension_description(), &default_selector(), HoverProvider::new(|| hover("Hello", None)));
            env.add(good);

            env.rpc_protocol.sync().await.expect("sync");
            let hovers = get_hovers_promise(&env.language_features_service.hover_provider, &env.model, Position::new(1, 1), &CancellationToken::None, false)
                .await
                .expect("getHoversPromise");
            assert_eq!(hovers.len(), 1);
            env.teardown().await;
        });
    }

    // ---- references -----------------------------------------------------------------------------------

    #[test]
    fn references_registration_order() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let first = env.ext_host.register_reference_provider(
                &null_extension_description(),
                &default_selector(),
                ReferenceProvider::new(|| found(vec![api_location(&URI::parse("far://register/first", false), range(0.0, 0.0, 0.0, 0.0))])),
            );
            env.add(first);
            let second = env.ext_host.register_reference_provider(
                &null_extension_description(),
                &default_selector(),
                ReferenceProvider::new(|| found(vec![api_location(&URI::parse("far://register/second", false), range(0.0, 0.0, 0.0, 0.0))])),
            );
            env.add(second);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_references_at_position(&env.language_features_service.reference_provider, &env.model, Position::new(1, 2), false, false, &CancellationToken::None)
                .await
                .expect("getReferencesAtPosition");
            assert_eq!(value.len(), 2);
            assert_eq!(value[0].uri.path().to_string_lossy(), "/second");
            assert_eq!(value[1].uri.path().to_string_lossy(), "/first");
            env.teardown().await;
        });
    }

    #[test]
    fn references_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let registration = env.ext_host.register_reference_provider(
                &null_extension_description(),
                &default_selector(),
                ReferenceProvider::new(move || {
                    found(vec![types::Location::new(Some(uri.clone()), Some(types::RangeOrPosition::Position(types::Position::new(0.0, 0.0).expect("Position"))))])
                }),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_references_at_position(&env.language_features_service.reference_provider, &env.model, Position::new(1, 2), false, false, &CancellationToken::None)
                .await
                .expect("getReferencesAtPosition");
            assert_eq!(value.len(), 1);
            let item = &value[0];
            assert_eq!(item.range, EditorRange::new(1, 1, 1, 1));
            assert_eq!(uri_text(&item.uri), uri_text(&env.model.uri()));
            env.teardown().await;
        });
    }

    #[test]
    fn references_evil_provider() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let bad = env.ext_host.register_reference_provider(&null_extension_description(), &default_selector(), ReferenceProvider::new(|| evil("evil")));
            env.add(bad);
            let good = env.ext_host.register_reference_provider(
                &null_extension_description(),
                &default_selector(),
                ReferenceProvider::new(move || found(vec![api_location(&uri, range(0.0, 0.0, 0.0, 0.0))])),
            );
            env.add(good);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_references_at_position(&env.language_features_service.reference_provider, &env.model, Position::new(1, 2), false, false, &CancellationToken::None)
                .await
                .expect("getReferencesAtPosition");
            assert_eq!(value.len(), 1);
            env.teardown().await;
        });
    }

    // ---- navigate types -------------------------------------------------------------------------------

    #[test]
    fn navigate_types_evil_provider() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let bad = env.ext_host.register_workspace_symbol_provider(&null_extension_description(), WorkspaceSymbolProvider::new(false, || evil("evil")));
            env.add(bad);
            let good = env.ext_host.register_workspace_symbol_provider(
                &null_extension_description(),
                WorkspaceSymbolProvider::new(false, || {
                    found(vec![types::SymbolInformation::from_range("testing", types::SymbolKind::Array, range(0.0, 0.0, 1.0, 1.0), None, None).expect("SymbolInformation")])
                }),
            );
            env.add(good);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_workspace_symbols("", &CancellationToken::None).await.expect("getWorkspaceSymbols");
            assert_eq!(value.len(), 1);
            assert_eq!(value[0].symbol.name, "testing");
            env.teardown().await;
        });
    }

    #[test]
    fn navigate_types_de_duplicate_results() {
        run_with_faked_timers(async move {
            let uri = URI::from(&UriComponents { scheme: Some("foo".into()), path: Some("/some/path".into()), ..UriComponents::default() }, false);
            let symbol = |kind: types::SymbolKind, uri: &URI, symbol_range: Option<types::Range>| {
                let location = types::Location::new(Some(uri.clone()), symbol_range.map(types::RangeOrPosition::Range));
                types::SymbolInformation::new("ONE", kind, None, location).expect("SymbolInformation")
            };

            let mut env = Env::new();
            let first_uri = uri.clone();
            let first = env.ext_host.register_workspace_symbol_provider(
                &null_extension_description(),
                WorkspaceSymbolProvider::new(false, move || found(vec![symbol(types::SymbolKind::Array, &first_uri, Some(range(0.0, 0.0, 1.0, 1.0)))])),
            );
            env.add(first);
            let second_uri = uri.clone();
            let second = env.ext_host.register_workspace_symbol_provider(
                &null_extension_description(),
                // get de-duped
                WorkspaceSymbolProvider::new(false, move || found(vec![symbol(types::SymbolKind::Array, &second_uri, Some(range(0.0, 0.0, 1.0, 1.0)))])),
            );
            env.add(second);
            let third_uri = uri.clone();
            let third = env.ext_host.register_workspace_symbol_provider(
                &null_extension_description(),
                // NO dedupe because of resolve
                WorkspaceSymbolProvider::new(true, move || found(vec![symbol(types::SymbolKind::Array, &third_uri, None)])),
            );
            env.add(third);
            let fourth_uri = uri.clone();
            let fourth = env.ext_host.register_workspace_symbol_provider(
                &null_extension_description(),
                // NO dedupe because of kind
                WorkspaceSymbolProvider::new(false, move || found(vec![symbol(types::SymbolKind::Struct, &fourth_uri, Some(range(0.0, 0.0, 1.0, 1.0)))])),
            );
            env.add(fourth);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_workspace_symbols("", &CancellationToken::None).await.expect("getWorkspaceSymbols");
            assert_eq!(value.len(), 3);
            env.teardown().await;
        });
    }

    // ---- rename ---------------------------------------------------------------------------------------

    #[test]
    fn rename_evil_provider_0_of_2() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            // `throw new class Foo { }`: sem mensagem, que o adaptador não converte em `rejectReason` e relança.
            let registration = env.ext_host.register_rename_provider(&null_extension_description(), &default_selector(), Rc::new(RenameProvider::new(|| evil(""))));
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let outcome = rename(&env.language_features_service.rename_provider, &env.model, Position::new(1, 1), "newName").await;
            assert!(outcome.is_err());
            env.teardown().await;
        });
    }

    #[test]
    fn rename_evil_provider_1_of_2() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env.ext_host.register_rename_provider(&null_extension_description(), &default_selector(), Rc::new(RenameProvider::new(|| evil("evil"))));
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = rename(&env.language_features_service.rename_provider, &env.model, Position::new(1, 1), "newName").await.expect("rename");
            assert_eq!(value.reject_reason.as_deref(), Some("evil"));
            env.teardown().await;
        });
    }

    #[test]
    fn rename_evil_provider_2_of_2() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let bad = env
                .ext_host
                .register_rename_provider(&null_extension_description(), &DocumentSelectorInput::Language("*".to_owned()), Rc::new(RenameProvider::new(|| evil("evil"))));
            env.add(bad);
            let good = env.ext_host.register_rename_provider(
                &null_extension_description(),
                &default_selector(),
                Rc::new(RenameProvider::new(move || found(replace_edit(&uri, &[range(0.0, 0.0, 0.0, 0.0)])))),
            );
            env.add(good);

            env.rpc_protocol.sync().await.expect("sync");
            let value = rename(&env.language_features_service.rename_provider, &env.model, Position::new(1, 1), "newName").await.expect("rename");
            assert_eq!(value.workspace_edit.expect("edits").edits.len(), 1);
            env.teardown().await;
        });
    }

    #[test]
    fn rename_ordering() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let uri = env.model.uri();
            let least_relevant = env.ext_host.register_rename_provider(
                &null_extension_description(),
                &DocumentSelectorInput::Language("*".to_owned()),
                Rc::new(RenameProvider::new(move || found(replace_edit(&uri, &[range(0.0, 0.0, 0.0, 0.0), range(1.0, 0.0, 1.0, 0.0)])))),
            );
            env.add(least_relevant);
            let most_relevant = env.ext_host.register_rename_provider(&null_extension_description(), &default_selector(), Rc::new(RenameProvider::new(nothing)));
            env.add(most_relevant);

            env.rpc_protocol.sync().await.expect("sync");
            let value = rename(&env.language_features_service.rename_provider, &env.model, Position::new(1, 1), "newName").await.expect("rename");
            // least relevant rename provider
            assert_eq!(value.workspace_edit.expect("edits").edits.len(), 2);
            env.teardown().await;
        });
    }

    #[test]
    fn multiple_rename_providers_do_not_respect_all_possible_prepare_rename_handlers_1_of_2_98352() {
        run_with_faked_timers(async move {
            let called = Rc::new(RefCell::new([false; 4]));
            let mut env = Env::new();

            let (prepare_called, edits_called) = (Rc::clone(&called), Rc::clone(&called));
            let first = RenameProvider::new(move || {
                edits_called.borrow_mut()[1] = true;
                nothing()
            })
            .with_prepare(move |document, position| {
                prepare_called.borrow_mut()[0] = true;
                let word_range = document.get_word_range_at_position(position, None).expect("getWordRangeAtPosition");
                MaybePromise::Value(word_range.map(ApiPrepareRename::Range))
            });
            let registration = env.ext_host.register_rename_provider(&null_extension_description(), &default_selector(), first);
            env.add(registration);

            let (prepare_called, edits_called) = (Rc::clone(&called), Rc::clone(&called));
            let second = RenameProvider::new(move || {
                edits_called.borrow_mut()[3] = true;
                nothing()
            })
            .with_prepare(move |_document, _position| {
                prepare_called.borrow_mut()[2] = true;
                evil("Cannot rename this symbol2.")
            });
            let registration = env.ext_host.register_rename_provider(&null_extension_description(), &default_selector(), second);
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            rename(&env.language_features_service.rename_provider, &env.model, Position::new(1, 1), "newName").await.expect("rename");

            assert_eq!(*called.borrow(), [true, true, true, false]);
            env.teardown().await;
        });
    }

    #[test]
    fn multiple_rename_providers_do_not_respect_all_possible_prepare_rename_handlers_2_of_2_98352() {
        run_with_faked_timers(async move {
            let called = Rc::new(RefCell::new([false; 3]));
            let mut env = Env::new();

            let (prepare_called, edits_called) = (Rc::clone(&called), Rc::clone(&called));
            let first = RenameProvider::new(move || {
                edits_called.borrow_mut()[1] = true;
                nothing()
            })
            .with_prepare(move |document, position| {
                prepare_called.borrow_mut()[0] = true;
                let word_range = document.get_word_range_at_position(position, None).expect("getWordRangeAtPosition");
                MaybePromise::Value(word_range.map(ApiPrepareRename::Range))
            });
            let registration = env.ext_host.register_rename_provider(&null_extension_description(), &default_selector(), first);
            env.add(registration);

            let edits_called = Rc::clone(&called);
            let registration = env.ext_host.register_rename_provider(
                &null_extension_description(),
                &default_selector(),
                Rc::new(RenameProvider::new(move || {
                    edits_called.borrow_mut()[2] = true;
                    found(types::WorkspaceEdit::new())
                })),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            rename(&env.language_features_service.rename_provider, &env.model, Position::new(1, 1), "newName").await.expect("rename");

            // first provider has NO prepare which means it is taken by default
            assert_eq!(*called.borrow(), [false, false, true]);
            env.teardown().await;
        });
    }

    // ---- format ---------------------------------------------------------------------------------------

    #[test]
    fn format_doc_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env.ext_host.register_document_formatting_edit_provider(
                &null_extension_description(),
                &default_selector(),
                FormattingProvider::new(|| {
                    found(vec![types::TextEdit::replace(range(0.0, 0.0, 0.0, 0.0), "testing"), types::TextEdit::set_end_of_line(types::EndOfLine::LF)])
                }),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_document_formatting_edits_until_result(&null_worker_service(), &env.language_features_service, &env.model, editor_options(4.0), &CancellationToken::None)
                .await
                .expect("getDocumentFormattingEditsUntilResult")
                .expect("edits");
            assert_eq!(value.len(), 2);
            let (first, second) = (&value[0], &value[1]);
            assert_eq!(first.text, "testing");
            assert_eq!(first.range, EditorRange::new(1, 1, 1, 1));
            assert_eq!(second.eol, Some(EndOfLineSequence::Lf));
            assert_eq!(second.text, "");
            assert_eq!(second.range, EditorRange::new(1, 1, 1, 1));
            env.teardown().await;
        });
    }

    #[test]
    fn format_doc_evil_provider() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env
                .ext_host
                .register_document_formatting_edit_provider(&null_extension_description(), &default_selector(), FormattingProvider::new(|| evil("evil")));
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            get_document_formatting_edits_until_result(&null_worker_service(), &env.language_features_service, &env.model, editor_options(4.0), &CancellationToken::None)
                .await
                .expect("getDocumentFormattingEditsUntilResult");
            env.teardown().await;
        });
    }

    #[test]
    fn format_doc_order() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let first = env
                .ext_host
                .register_document_formatting_edit_provider(&null_extension_description(), &default_selector(), FormattingProvider::new(nothing));
            env.add(first);
            let second = env.ext_host.register_document_formatting_edit_provider(
                &null_extension_description(),
                &default_selector(),
                FormattingProvider::new(|| found(vec![types::TextEdit::replace(range(0.0, 0.0, 0.0, 0.0), "testing")])),
            );
            env.add(second);
            let third = env
                .ext_host
                .register_document_formatting_edit_provider(&null_extension_description(), &default_selector(), FormattingProvider::new(nothing));
            env.add(third);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_document_formatting_edits_until_result(&null_worker_service(), &env.language_features_service, &env.model, editor_options(4.0), &CancellationToken::None)
                .await
                .expect("getDocumentFormattingEditsUntilResult")
                .expect("edits");
            assert_eq!(value.len(), 1);
            assert_eq!(value[0].text, "testing");
            assert_eq!(value[0].range, EditorRange::new(1, 1, 1, 1));
            env.teardown().await;
        });
    }

    #[test]
    fn format_range_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env.ext_host.register_document_range_formatting_edit_provider(
                &null_extension_description(),
                &default_selector(),
                RangeFormattingProvider::new(|| found(vec![types::TextEdit::replace(range(0.0, 0.0, 0.0, 0.0), "testing")])),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_document_range_formatting_edits_until_result(
                &null_worker_service(),
                &env.language_features_service,
                &env.model,
                EditorRange::new(1, 1, 1, 1),
                editor_options(4.0),
                &CancellationToken::None,
            )
            .await
            .expect("getDocumentRangeFormattingEditsUntilResult")
            .expect("edits");
            assert_eq!(value.len(), 1);
            assert_eq!(value[0].text, "testing");
            assert_eq!(value[0].range, EditorRange::new(1, 1, 1, 1));
            env.teardown().await;
        });
    }

    #[test]
    fn format_range_plus_format_doc() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let range_one = env.ext_host.register_document_range_formatting_edit_provider(
                &null_extension_description(),
                &default_selector(),
                RangeFormattingProvider::new(|| found(vec![types::TextEdit::replace(range(0.0, 0.0, 0.0, 0.0), "range")])),
            );
            env.add(range_one);
            let range_two = env.ext_host.register_document_range_formatting_edit_provider(
                &null_extension_description(),
                &default_selector(),
                RangeFormattingProvider::new(|| found(vec![types::TextEdit::replace(range(2.0, 3.0, 4.0, 5.0), "range2")])),
            );
            env.add(range_two);
            let document = env.ext_host.register_document_formatting_edit_provider(
                &null_extension_description(),
                &default_selector(),
                FormattingProvider::new(|| found(vec![types::TextEdit::replace(range(0.0, 0.0, 1.0, 1.0), "doc")])),
            );
            env.add(document);
            env.rpc_protocol.sync().await.expect("sync");
            let value = get_document_range_formatting_edits_until_result(
                &null_worker_service(),
                &env.language_features_service,
                &env.model,
                EditorRange::new(1, 1, 1, 1),
                editor_options(4.0),
                &CancellationToken::None,
            )
            .await
            .expect("getDocumentRangeFormattingEditsUntilResult")
            .expect("edits");
            assert_eq!(value.len(), 1);
            let first = &value[0];
            assert_eq!(first.text, "range2");
            assert_eq!(first.range.start_line_number, 3);
            assert_eq!(first.range.start_column, 4);
            assert_eq!(first.range.end_line_number, 5);
            assert_eq!(first.range.end_column, 6);
            env.teardown().await;
        });
    }

    #[test]
    fn format_range_evil_provider() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env
                .ext_host
                .register_document_range_formatting_edit_provider(&null_extension_description(), &default_selector(), RangeFormattingProvider::new(|| evil("evil")));
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            get_document_range_formatting_edits_until_result(
                &null_worker_service(),
                &env.language_features_service,
                &env.model,
                EditorRange::new(1, 1, 1, 1),
                editor_options(4.0),
                &CancellationToken::None,
            )
            .await
            .expect("getDocumentRangeFormattingEditsUntilResult");
            env.teardown().await;
        });
    }

    #[test]
    fn format_on_type_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env.ext_host.register_on_type_formatting_edit_provider(
                &null_extension_description(),
                &default_selector(),
                Rc::new(EchoOnTypeProvider),
                vec![";".to_owned()],
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_on_type_formatting_edits(
                &null_worker_service(),
                &env.language_features_service,
                &env.model,
                Position::new(1, 1),
                ";",
                editor_options(2.0),
                &CancellationToken::None,
            )
            .await
            .expect("getOnTypeFormattingEdits")
            .expect("edits");
            assert_eq!(value.len(), 1);
            assert_eq!(value[0].text, ";");
            assert_eq!(value[0].range, EditorRange::new(1, 1, 1, 1));
            env.teardown().await;
        });
    }

    // ---- links ----------------------------------------------------------------------------------------

    #[test]
    fn links_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env.ext_host.register_document_link_provider(
                &null_extension_description(),
                &default_selector(),
                LinkProvider::new(|| {
                    let mut link = document_link();
                    link.tooltip = Some("tooltip".to_owned());
                    found(vec![Rc::new(RefCell::new(link))])
                }),
            );
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let list = get_links(&env.language_features_service.link_provider, &env.model, &CancellationToken::None).await.expect("getLinks");
            env.add(list.clone());
            let links = list.links();
            assert_eq!(links.len(), 1);
            let first = &links[0];
            assert!(matches!(first.url(), Some(LinkUrl::Uri(url)) if uri_text(&url) == "foo:bar#3"));
            assert_eq!(first.range(), EditorRange::new(1, 1, 2, 2));
            assert_eq!(first.tooltip().as_deref(), Some("tooltip"));
            env.teardown().await;
        });
    }

    #[test]
    fn links_evil_provider() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let good = env.ext_host.register_document_link_provider(
                &null_extension_description(),
                &default_selector(),
                LinkProvider::new(|| found(vec![Rc::new(RefCell::new(document_link()))])),
            );
            env.add(good);
            let bad = env.ext_host.register_document_link_provider(&null_extension_description(), &default_selector(), LinkProvider::new(|| evil("")));
            env.add(bad);

            env.rpc_protocol.sync().await.expect("sync");
            let list = get_links(&env.language_features_service.link_provider, &env.model, &CancellationToken::None).await.expect("getLinks");
            env.add(list.clone());
            let links = list.links();
            assert_eq!(links.len(), 1);
            let first = &links[0];
            assert!(matches!(first.url(), Some(LinkUrl::Uri(url)) if uri_text(&url) == "foo:bar#3"));
            assert_eq!(first.range(), EditorRange::new(1, 1, 2, 2));
            env.teardown().await;
        });
    }

    // ---- colors ---------------------------------------------------------------------------------------

    #[test]
    fn document_colors_data_conversion() {
        run_with_faked_timers(async move {
            let mut env = Env::new();
            let registration = env.ext_host.register_color_provider(&null_extension_description(), &default_selector(), Rc::new(ColorProvider));
            env.add(registration);

            env.rpc_protocol.sync().await.expect("sync");
            let value = get_colors(&env.language_features_service.color_provider, &env.model, &CancellationToken::None, DefaultColorDecoratorsEnablement::Auto)
                .await
                .expect("getColors");
            assert_eq!(value.len(), 1);
            let first = &value[0];
            assert_eq!(first.color_info.color, IColor { red: 0.1, green: 0.2, blue: 0.3, alpha: 0.4 });
            assert_eq!(first.color_info.range, EditorRange::new(1, 1, 1, 21));
            env.teardown().await;
        });
    }
}
