//! Porte de `src/vs/workbench/api/test/browser/extHostLanguageFeatures.test.ts` (suíte `ExtHostLanguageFeatures`), caso a caso e na mesma ordem.
//!
//! PORTADOS (5 de 65 `test(` do original, contando cada caso dos laços `for (const declinedRequests of [0, 2])` como dois):
//! `DocumentDropEdits does not cache an empty edit array without release IDs`, `DocumentSymbols, register/deregister`,
//! `DocumentSymbols, evil provider`, `DocumentSymbols, data conversion` e `Quick Outline uses a not ideal sorting, #138502`.
//!
//! FORA, e por quê (nenhum vira `#[test]` enquanto não puder ser fiel):
//! - Função do editor ainda sem porte: `getCodeLensModel` (CodeLens x3), `getDefinitionsAtPosition`,
//!   `getImplementationsAtPosition`, `getTypeDefinitionsAtPosition`, `getDeclarationsAtPosition`, `getReferencesAtPosition`
//!   (Definition x4, Declaration, Implementation, Type Definition, References x3), `getHoversPromise` (HoverProvider x4),
//!   `getWorkspaceSymbols` (Navigate types x2), `rename` (Rename x5 e os dois #98352), `getDocumentFormattingEditsUntilResult`,
//!   `getDocumentRangeFormattingEditsUntilResult` e `getOnTypeFormattingEdits` (Format Doc x3, Format Range x3, Format on
//!   Type), `getLinks` (Links x2) e `getColors` (Document colors).
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
