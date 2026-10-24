//! Porte de `src/vs/workbench/api/test/browser/extHostLanguageFeatures.test.ts` (suíte `ExtHostLanguageFeatures`), caso a caso e na mesma ordem.
//!
//! PORTADOS (5 de 65 `test(` do original, contando cada caso dos laços `for (const declinedRequests of [0, 2])` como dois):
//! `DocumentDropEdits does not cache an empty edit array without release IDs`, `DocumentSymbols, register/deregister`,
//! `DocumentSymbols, evil provider`, `DocumentSymbols, data conversion` e `Quick Outline uses a not ideal sorting, #138502`.
//! No módulo `navigation_and_formatting`, ao fim do arquivo (fatia w141, 35 casos): CodeLens x3, Definition x4, Declaration,
//! Implementation, Type Definition, HoverProvider x4, References x3, Navigate types x2, Rename x6 (os quatro de `rename` e
//! os dois #98352), Format Doc x3, Format Range x3, Format on Type, Links x2 e Document colors.
//!
//! FORA, e por quê (nenhum vira `#[test]` enquanto não puder ser fiel):
//! - Nenhum caso das funções de editor desta fatia sobra: `getCodeLensModel`, `getDefinitionsAtPosition` e as três irmãs
//!   (`getLocationLinksAtPosition` no porte, ver `goto_symbol`), `getReferencesAtPosition`, `getHoversPromise`,
//!   `getWorkspaceSymbols`, `rename`, as três funções de formatação, `getLinks` e `getColors` estão portadas.
//! - Função do editor que existe, mas cujo caso não coube nesta fatia (pendentes de porte, não de editor): `getOccurrencesAtPosition`
//!   (Occurrences x4), `getCodeActions` (Quick Fix x4 e #29469), `provideSignatureHelp` (Parameter Hints x2),
//!   `provideSuggestionItems` (Suggest x5) e `provideSelectionRanges` (Selection Ranges x2).
//! - `AutoClosingPair.beforeText requires proposed API`: depende de `$setLanguageConfiguration` do `MainThreadLanguageFeatures` e de
//!   `setLanguageConfiguration` do host, que ainda não têm porte (ver a lista de pendentes dos dois arquivos).
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
//!   própria URI (`asCanonicalUri(uri) { return uri; }`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::data_transfer::VSDataTransfer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::language_features_service::{DocumentDropEditProvider, LanguageFeaturesService};
use je_editor::model::Eol;
use je_editor::outline_model::OutlineModel;
use je_editor::position::Position;
use je_editor::text_model::TextModel;
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
    ApiDocumentDropEditProvider, ApiDocumentSymbolProvider, ApiDocumentSymbols, ApiProviderResult, DocumentEditRef, IExtHostApiDeprecationService, OneOrMany,
};
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, DocumentsAndEditorsDelta, ExtHostContext, ExtHostDocumentsAndEditorsShape, ExtHostLanguageFeaturesShape, IExtHostTelemetry, IMarkerChange,
    MainContext, MainThreadCommandsShape, MainThreadDiagnosticsShape, MainThreadDocumentsShape, MainThreadLanguageFeaturesShape, ModelAddedData,
    CreateDocumentOptions, OpenDocumentOptions,
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
            language_features_service.clone(),
            Rc::new(IdentityUriIdentityService) as Rc<dyn IUriIdentityService>,
        );
        rpc_protocol.set(&MainContext::MAIN_THREAD_LANGUAGE_FEATURES, main_thread.clone() as Rc<dyn MainThreadLanguageFeaturesShape>);

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
