//! Dublês e ambiente dos testes de cache e de sessão do `ExtHostLanguageFeatures` (`extHostSignatureHelp.test.ts`,
//! `extHostWorkspaceSymbols.test.ts`, `extHostSemanticTokens.test.ts` e `extHostHierarchy.test.ts`).
//!
//! No original os quatro arquivos montam o mesmo `setup`: um `MainThreadLanguageFeaturesShape` sobrescrito só no
//! `$register*` do caso (que guarda o `handle`) e em `$unregister`, `SingleProxyRPCProtocol(proxy)`,
//! `new URITransformerService(null)` e `mock<ExtHostDocuments>`, `mock<ExtHostCommands>`, `mock<ExtHostDiagnostics>` e
//! `mock<IExtHostTelemetry>` com `onExtensionError` devolvendo `true`.
//!
//! Notas de tradução:
//! - `SingleProxyRPCProtocol(proxy)` devolve o mesmo proxy para qualquer identificador. O porte registra o dublê de
//!   [`MainContext::MAIN_THREAD_LANGUAGE_FEATURES`] e os de comandos, documentos e diagnósticos que o host pede ao ser criado
//!   (nenhum é chamado pelos casos).
//! - `ExtHostDocuments`, `ExtHostCommands` e `ExtHostDiagnostics` são structs concretas, sem `mock<T>()`: o ambiente usa as
//!   reais, com o documento do recurso do caso já aberto (o `getDocument()` do original devolve um `TextDocument` vazio
//!   qualquer, e os adaptadores só o repassam ao provedor).
//! - O `MainThreadLanguageFeaturesShape` do original só implementa o que o caso sobrescreve e o resto lança `TypeError`. O
//!   [`RecordingMainThreadLanguageFeatures`] sobrescreve a união dos cinco `$register*` dos quatro arquivos, que guardam o
//!   `handle` (cada caso registra um único provedor), mais `$unregister`; todo o resto falha com `panic!`.
//! - `store = ensureNoDisposablesAreLeakedInTestSuite()` é o [`LeakGuard`] do [`ExtHostEnv`], que nasce antes do
//!   `ExtHostLanguageFeatures` (que o `store.add` descarta no fim) e é o último campo, para conferir os vazamentos depois do resto.

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model::Eol;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_diagnostics::ExtHostDiagnostics;
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_file_system_info::ExtHostFileSystemInfo;
use je_workbench::api::common::ext_host_language_features::ExtHostLanguageFeatures;
use je_workbench::api::common::ext_host_language_features_api::IExtHostApiDeprecationService;
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, CreateDocumentOptions, DocumentsAndEditorsDelta, ExtHostDocumentsAndEditorsShape, ICodeActionProviderMetadataDto, IDocumentDropEditProviderMetadata,
    IDocumentFilterDto, IExtHostTelemetry, ILanguageConfigurationDto, IMarkerChange, IPasteEditProviderMetadataDto, ISignatureHelpProviderMetadataDto, MainContext,
    MainThreadCommandsShape, MainThreadDiagnosticsShape, MainThreadDocumentsShape, MainThreadLanguageFeaturesShape, ModelAddedData, OpenDocumentOptions,
};
use je_workbench::api::common::ext_host_type_converters::{DocumentFilterInput, DocumentSelectorInput};
use je_workbench::api::common::ext_host_types as types;

use super::ext_host_workspace_support::TestMainContext;

#[path = "../../../je-base/tests/common/mod.rs"]
mod base;

pub use base::{LeakGuard, run_with_faked_timers, timeout};

/// `{ scheme: 'test' }`, o seletor de todos os casos.
pub fn test_selector() -> DocumentSelectorInput {
    DocumentSelectorInput::Filter(DocumentFilterInput { scheme: Some("test".to_owned()), ..DocumentFilterInput::default() })
}

/// `new class extends mock<IExtHostTelemetry>() { override onExtensionError(): boolean { return true; } }`.
pub struct TestExtHostTelemetry;

impl IExtHostTelemetry for TestExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        true
    }
}

/// `NullApiDeprecationService`.
pub struct NullApiDeprecationService;

impl IExtHostApiDeprecationService for NullApiDeprecationService {
    fn report(&self, _api_id: &str, _extension: &ExtensionDescription, _migration_suggestion: &str) {}
}

/// O `mock<ExtHostCommands>` do original: nenhum comando é executado.
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

/// `MainThreadDocuments` que falha se for chamado (também usado por `extHostBulkEdits.test.ts`, no lugar do `SingleProxyRPCProtocol(null)`).
pub struct UnusedMainThreadDocuments;

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

/// O `new class extends mock<MainThreadLanguageFeaturesShape>()` dos casos (ver as notas do módulo).
#[derive(Default)]
pub struct RecordingMainThreadLanguageFeatures {
    handle: Cell<Option<u32>>,
}

impl RecordingMainThreadLanguageFeatures {
    /// O `handle` guardado pelo `$register*` do caso.
    pub fn handle(&self) -> u32 {
        self.handle.get().expect("nenhum provedor foi registrado")
    }
}

impl IDisposable for RecordingMainThreadLanguageFeatures {
    fn dispose(&self) {}
}

/// Os métodos de [`MainThreadLanguageFeaturesShape`] que o dublê não sobrescreve: o `mock<T>()` lança `TypeError`.
macro_rules! unexpected_calls {
    ($($method:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(
            #[allow(unused_variables)]
            fn $method(&self, $($arg: $ty),*) $(-> $ret)? {
                panic!("{} is not a function", stringify!($method))
            }
        )*
    };
}

impl MainThreadLanguageFeaturesShape for RecordingMainThreadLanguageFeatures {
    /// `$unregister(): void { }`.
    fn rpc_unregister(&self, _handle: u32) {}

    /// `$registerSignatureHelpProvider(value: number): void { handle = value; }`.
    fn rpc_register_signature_help_provider(&self, handle: u32, _selector: Vec<IDocumentFilterDto>, _metadata: ISignatureHelpProviderMetadataDto) {
        self.handle.set(Some(handle));
    }

    /// `$registerNavigateTypeSupport(value: number): void { handle = value; }`.
    fn rpc_register_navigate_type_support(&self, handle: u32, _supports_resolve: bool) {
        self.handle.set(Some(handle));
    }

    /// `$registerDocumentSemanticTokensProvider(value: number): void { handle = value; }`.
    fn rpc_register_document_semantic_tokens_provider(
        &self,
        handle: u32,
        _selector: Vec<IDocumentFilterDto>,
        _legend: types::SemanticTokensLegend,
        _event_handle: Option<u32>,
    ) {
        self.handle.set(Some(handle));
    }

    /// `$registerCallHierarchyProvider(value: number): void { handle = value; }`.
    fn rpc_register_call_hierarchy_provider(&self, handle: u32, _selector: Vec<IDocumentFilterDto>) {
        self.handle.set(Some(handle));
    }

    /// `$registerTypeHierarchyProvider(value: number): void { handle = value; }`.
    fn rpc_register_type_hierarchy_provider(&self, handle: u32, _selector: Vec<IDocumentFilterDto>) {
        self.handle.set(Some(handle));
    }

    unexpected_calls! {
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
        rpc_register_rename_support(handle: u32, selector: Vec<IDocumentFilterDto>, supports_resolve_initial_values: bool);
        rpc_register_new_symbol_names_provider(handle: u32, selector: Vec<IDocumentFilterDto>);
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
        rpc_register_document_on_drop_edit_provider(handle: u32, selector: Vec<IDocumentFilterDto>, metadata: Option<IDocumentDropEditProviderMetadata>);
        rpc_resolve_paste_file_data(handle: u32, request_id: u32, data_id: &str) -> Promise<VSBuffer>;
        rpc_resolve_document_on_drop_file_data(handle: u32, request_id: u32, data_id: &str) -> Promise<VSBuffer>;
        rpc_set_language_configuration(handle: u32, language_id: &str, configuration: ILanguageConfigurationDto);
    }
}

/// O `setup` comum dos quatro arquivos: o host com o dublê acima e o documento do recurso aberto.
pub struct ExtHostEnv {
    pub ext_host: Rc<ExtHostLanguageFeatures>,
    proxy: Rc<RecordingMainThreadLanguageFeatures>,
    _rpc_protocol: Rc<TestMainContext>,
    /// `store`: o último campo, para a conferência de vazamentos vir depois do descarte do resto.
    guard: LeakGuard,
}

impl ExtHostEnv {
    /// `setup(() => { ... })`, com o documento de `resource` já presente no host de documentos.
    pub fn new(resource: &URI) -> ExtHostEnv {
        let rpc_protocol = Rc::new(TestMainContext::new());
        let proxy = Rc::new(RecordingMainThreadLanguageFeatures::default());

        let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc_protocol.clone(), NullLogService::shared()));
        rpc_protocol.set(&MainContext::MAIN_THREAD_DOCUMENTS, Rc::new(UnusedMainThreadDocuments) as Rc<dyn MainThreadDocumentsShape>);
        documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
            added_documents: Some(vec![ModelAddedData {
                is_dirty: false,
                version_id: 1,
                language_id: "typescript".to_owned(),
                uri: resource.clone(),
                lines: vec![JsString::from("")],
                eol: Eol::Lf,
                encoding: "utf8".to_owned(),
            }]),
            ..DocumentsAndEditorsDelta::default()
        });
        let documents = ExtHostDocuments::new(rpc_protocol.as_ref(), documents_and_editors.clone());

        rpc_protocol.set(&MainContext::MAIN_THREAD_COMMANDS, Rc::new(UnusedMainThreadCommands) as Rc<dyn MainThreadCommandsShape>);
        let commands = ExtHostCommands::new(rpc_protocol.as_ref(), NullLogService::shared(), Rc::new(TestExtHostTelemetry));

        rpc_protocol.set(&MainContext::MAIN_THREAD_DIAGNOSTICS, Rc::new(UnusedMainThreadDiagnostics) as Rc<dyn MainThreadDiagnosticsShape>);
        let diagnostics = Rc::new(ExtHostDiagnostics::new(
            rpc_protocol.as_ref(),
            NullLogService::shared(),
            Rc::new(ExtHostFileSystemInfo::new()),
            documents_and_editors.clone(),
        ));

        rpc_protocol.set(&MainContext::MAIN_THREAD_LANGUAGE_FEATURES, proxy.clone() as Rc<dyn MainThreadLanguageFeaturesShape>);

        let guard = base::ensure_no_disposables_are_leaked_in_test_suite();
        let ext_host = guard.add_rc(ExtHostLanguageFeatures::new(
            rpc_protocol.as_ref(),
            None,
            documents,
            commands,
            diagnostics,
            NullLogService::shared(),
            Rc::new(NullApiDeprecationService),
            Rc::new(TestExtHostTelemetry),
        ));
        ExtHostEnv { ext_host, proxy, _rpc_protocol: rpc_protocol, guard }
    }

    /// O `handle` guardado pelo `$register*` do caso.
    pub fn handle(&self) -> u32 {
        self.proxy.handle()
    }

    /// `store.add(value)`.
    pub fn add(&self, disposable: DisposableRef) {
        self.guard.add(disposable);
    }
}
