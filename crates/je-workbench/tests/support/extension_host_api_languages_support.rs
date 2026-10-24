//! O lado do workbench de `vscode.languages` para os testes de ponta a ponta `extension_host_api_languages_*.rs`: o binder liga os
//! documentos de texto reais (`MainThreadDocumentsAndEditors` sobre o `ModelService`, como em `extension_host_api_text_editors.rs`), o
//! `MainThreadDiagnostics` REAL (sobre o `MarkerService` e o `UriIdentityService` reais), um `MainThreadLanguages` que registra as chamadas
//! (o real pede o `ILanguageService` e o `ITextModelService` completos) e um `MainThreadLanguageFeatures` que registra os handles de cada
//! `$register*` (o real pede o `LanguageFeaturesService` do editor, que os testes de unidade de `ext_host_language_features.rs` já
//! exercitam). O teste chama os `$provide*` do host pelo proxy `ExtHostLanguageFeaturesShape` que o contexto em processo entrega.
//!
//! Quem inclui este arquivo declara no mesmo módulo pai `harness`, `main_thread_support` e `editor_support` (ver `extension_host_api_text_editors.rs`).

#![allow(dead_code, unused_imports)]

use je_instantiation::Injectable;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model_service::ModelService;
use je_editor::position::Position as EditorPosition;
use je_editor::text_model::TextModel;
use je_editor::undo_redo_service::UndoRedoService;
use je_exthost::in_process::IMainThreadBinder;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::markers::IMarkerService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::api::browser::main_thread_diagnostics::MainThreadDiagnostics;
use je_workbench::api::browser::main_thread_documents_and_editors::MainThreadDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostLanguageFeaturesShape, IDocumentDropEditProviderMetadata, IDocumentFilterDto, ILanguageConfigurationDto, ILanguageStatus, IMainContext,
    IPasteEditProviderMetadataDto, ISignatureHelpProviderMetadataDto, ISyntaxHighlightingResultDto, MainContext, MainThreadDiagnosticsShape, MainThreadLanguageFeaturesShape,
    MainThreadLanguagesShape, TokenAtPositionDto, ICodeActionProviderMetadataDto,
};
use je_workbench::api::common::ext_host_types as types;
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::{TestFileService, TestTextResourcePropertiesService};

use super::editor_support::test_clipboard_service::TestClipboardService;
use super::editor_support::test_code_editor::I_CODE_EDITOR_SERVICE;
use super::editor_support::test_code_editor_service::TestCodeEditorService;
use super::editor_support::test_color_theme_service::TestThemeService;
use super::main_thread_support::{NoEventWorkingCopyFileService, StubPaneCompositePartService, StubQuickDiffModelService, StubTextFileService, UnusedTextModelService};

/// As linguagens que o `MainThreadLanguages` anuncia ao host (o real lê do `ILanguageService`).
pub const LANGUAGE_IDS: [&str; 3] = ["plaintext", "typescript", "rust"];

/// O `MainThreadLanguagesShape` que registra as chamadas (ver o cabeçalho do módulo).
#[derive(Default)]
pub struct RecordingLanguages {
    pub calls: RefCell<Vec<String>>,
}

impl IDisposable for RecordingLanguages {
    fn dispose(&self) {}
}

impl MainThreadLanguagesShape for RecordingLanguages {
    fn rpc_change_language(&self, resource: URI, language_id: &str) -> Promise<()> {
        self.calls.borrow_mut().push(format!("changeLanguage {} {language_id}", resource.path()));
        Promise::resolved(())
    }

    fn rpc_tokens_at_position(&self, _resource: URI, _position: EditorPosition) -> Promise<Option<TokenAtPositionDto>> {
        Promise::resolved(None)
    }

    fn rpc_compute_full_syntax_highlighting(&self, _source: &str, _language_id: &str) -> Promise<ISyntaxHighlightingResultDto> {
        Promise::resolved(ISyntaxHighlightingResultDto { tokens: Vec::new(), color_map: Vec::new() })
    }

    fn rpc_set_language_status(&self, _handle: u32, _status: ILanguageStatus) {}

    fn rpc_remove_language_status(&self, _handle: u32) {}
}

/// Gera os `$register*` que só guardam o `(nome, handle)` do registro.
macro_rules! record_registrations {
    ($($method:ident($($arg:ident: $ty:ty),*);)*) => {
        $(
            #[allow(unused_variables)]
            fn $method(&self, handle: u32, $($arg: $ty),*) {
                self.registered.borrow_mut().push((stringify!($method), handle));
            }
        )*
    };
}

/// Gera os métodos que o teste não exercita e que não fazem nada.
macro_rules! ignore_calls {
    ($($method:ident($($arg:ident: $ty:ty),*);)*) => {
        $(
            #[allow(unused_variables)]
            fn $method(&self, $($arg: $ty),*) {}
        )*
    };
}

/// O `MainThreadLanguageFeaturesShape` que registra os handles de cada `$register*`.
#[derive(Default)]
pub struct RecordingLanguageFeatures {
    pub registered: RefCell<Vec<(&'static str, u32)>>,
    pub unregistered: RefCell<Vec<u32>>,
}

impl RecordingLanguageFeatures {
    /// Os handles dos registros de `kind` (o nome do método `rpc_register_*`), na ordem em que chegaram.
    pub fn handles_of(&self, kind: &str) -> Vec<u32> {
        self.registered.borrow().iter().filter(|(name, _)| *name == kind).map(|(_, handle)| *handle).collect()
    }

    /// O handle do primeiro registro de `kind`.
    pub fn handle_of(&self, kind: &str) -> u32 {
        *self.handles_of(kind).first().unwrap_or_else(|| panic!("nenhum registro de {kind}: {:?}", self.registered.borrow()))
    }
}

impl IDisposable for RecordingLanguageFeatures {
    fn dispose(&self) {}
}

impl MainThreadLanguageFeaturesShape for RecordingLanguageFeatures {
    fn rpc_unregister(&self, handle: u32) {
        self.unregistered.borrow_mut().push(handle);
    }

    record_registrations! {
        rpc_register_document_symbol_provider(selector: Vec<IDocumentFilterDto>, label: &str);
        rpc_register_code_lens_support(selector: Vec<IDocumentFilterDto>, event_handle: Option<u32>);
        rpc_register_definition_support(selector: Vec<IDocumentFilterDto>);
        rpc_register_declaration_support(selector: Vec<IDocumentFilterDto>);
        rpc_register_implementation_support(selector: Vec<IDocumentFilterDto>);
        rpc_register_type_definition_support(selector: Vec<IDocumentFilterDto>);
        rpc_register_hover_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_document_highlight_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_multi_document_highlight_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_reference_support(selector: Vec<IDocumentFilterDto>);
        rpc_register_evaluatable_expression_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_inline_values_provider(selector: Vec<IDocumentFilterDto>, event_handle: Option<u32>);
        rpc_register_linked_editing_range_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_code_action_support(
            selector: Vec<IDocumentFilterDto>,
            metadata: ICodeActionProviderMetadataDto,
            display_name: &str,
            extension_id: &str,
            supports_resolve: bool
        );
        rpc_register_paste_edit_provider(selector: Vec<IDocumentFilterDto>, metadata: IPasteEditProviderMetadataDto);
        rpc_register_document_formatting_support(selector: Vec<IDocumentFilterDto>, extension_id: &ExtensionIdentifier, display_name: &str);
        rpc_register_range_formatting_support(selector: Vec<IDocumentFilterDto>, extension_id: &ExtensionIdentifier, display_name: &str, support_ranges: bool);
        rpc_register_on_type_formatting_support(selector: Vec<IDocumentFilterDto>, auto_format_trigger_characters: Vec<String>, extension_id: &ExtensionIdentifier);
        rpc_register_navigate_type_support(supports_resolve: bool);
        rpc_register_rename_support(selector: Vec<IDocumentFilterDto>, supports_resolve_initial_values: bool);
        rpc_register_new_symbol_names_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_document_semantic_tokens_provider(selector: Vec<IDocumentFilterDto>, legend: types::SemanticTokensLegend, event_handle: Option<u32>);
        rpc_register_document_range_semantic_tokens_provider(selector: Vec<IDocumentFilterDto>, legend: types::SemanticTokensLegend, event_handle: Option<u32>);
        rpc_register_completions_provider(
            selector: Vec<IDocumentFilterDto>,
            trigger_characters: Vec<String>,
            supports_resolve_details: bool,
            extension_id: &ExtensionIdentifier
        );
        rpc_register_signature_help_provider(selector: Vec<IDocumentFilterDto>, metadata: ISignatureHelpProviderMetadataDto);
        rpc_register_inlay_hints_provider(selector: Vec<IDocumentFilterDto>, supports_resolve: bool, event_handle: Option<u32>, display_name: Option<&str>);
        rpc_register_document_link_provider(selector: Vec<IDocumentFilterDto>, supports_resolve: bool);
        rpc_register_document_color_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_folding_range_provider(selector: Vec<IDocumentFilterDto>, extension_id: &ExtensionIdentifier, event_handle: Option<u32>);
        rpc_register_selection_range_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_call_hierarchy_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_type_hierarchy_provider(selector: Vec<IDocumentFilterDto>);
        rpc_register_document_on_drop_edit_provider(selector: Vec<IDocumentFilterDto>, metadata: Option<IDocumentDropEditProviderMetadata>);
    }

    ignore_calls! {
        rpc_emit_code_lens_event(event_handle: u32, event: Option<Dynamic>);
        rpc_emit_inline_values_event(event_handle: u32, event: Option<Dynamic>);
        rpc_emit_document_semantic_tokens_event(event_handle: u32);
        rpc_emit_document_range_semantic_tokens_event(event_handle: u32);
        rpc_emit_inlay_hints_event(event_handle: u32);
        rpc_emit_folding_range_event(event_handle: u32, event: Option<Dynamic>);
        rpc_set_language_configuration(handle: u32, language_id: &str, configuration: ILanguageConfigurationDto);
    }

    fn rpc_resolve_paste_file_data(&self, _handle: u32, _request_id: u32, _data_id: &str) -> Promise<VSBuffer> {
        panic!("o teste não resolve dados de arquivo colado")
    }

    fn rpc_resolve_document_on_drop_file_data(&self, _handle: u32, _request_id: u32, _data_id: &str) -> Promise<VSBuffer> {
        panic!("o teste não resolve dados de arquivo solto")
    }
}

/// O que o teste usa do lado do workbench.
pub struct Observed {
    pub model_service: Rc<ModelService>,
    pub marker_service: Rc<MarkerService>,
    pub languages: Rc<RecordingLanguages>,
    pub language_features: Rc<RecordingLanguageFeatures>,
    /// O proxy do `ExtHostLanguageFeatures` do host (o encaminhador `Late`, que repassa ao serviço real): o teste chama os `$provide*`
    /// por ele, como faria o `MainThreadLanguageFeatures` do workbench.
    pub features_proxy: Rc<RefCell<Option<Rc<dyn ExtHostLanguageFeaturesShape>>>>,
}

impl Observed {
    pub fn features_proxy(&self) -> Rc<dyn ExtHostLanguageFeaturesShape> {
        self.features_proxy.borrow().clone().expect("o binder ainda não rodou")
    }
}

/// Liga os clientes `MainThread*` de `vscode.languages` (ver o cabeçalho do módulo).
pub struct LanguagesBinder {
    model_service: Rc<ModelService>,
    code_editor_service: Rc<TestCodeEditorService>,
    file_service: Rc<TestFileService>,
    marker_service: Rc<MarkerService>,
    languages: Rc<RecordingLanguages>,
    language_features: Rc<RecordingLanguageFeatures>,
    features_proxy: Rc<RefCell<Option<Rc<dyn ExtHostLanguageFeaturesShape>>>>,
}

impl IMainThreadBinder for LanguagesBinder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let file_service: Rc<dyn IFileService> = self.file_service.clone();
        let uri_identity_service = UriIdentityService::create((), (file_service.clone(),));
        let documents_and_editors = MainThreadDocumentsAndEditors::new(
            &context,
            self.model_service.clone(),
            Rc::new(StubTextFileService),
            Rc::new(TestEditorService),
            self.code_editor_service.registry().clone(),
            file_service,
            Rc::new(UnusedTextModelService),
            TestEditorGroupsService::new(None),
            Rc::new(StubPaneCompositePartService),
            TestEnvironmentService::new(),
            Rc::new(NoEventWorkingCopyFileService),
            uri_identity_service.clone(),
            Rc::new(TestClipboardService::new()),
            TestPathService::new(None, None),
            TestConfigurationService::new(None),
            Rc::new(StubQuickDiffModelService),
        );

        let marker_service: Rc<dyn IMarkerService> = self.marker_service.clone();
        let diagnostics = MainThreadDiagnostics::new(&*context, marker_service, uri_identity_service);
        let diagnostics_shape: Rc<dyn MainThreadDiagnosticsShape> = diagnostics.clone();
        context.set(&MainContext::MAIN_THREAD_DIAGNOSTICS, diagnostics_shape);

        // O `MainThreadLanguages` real anuncia as linguagens no construtor.
        let languages_shape: Rc<dyn MainThreadLanguagesShape> = self.languages.clone();
        context.set(&MainContext::MAIN_THREAD_LANGUAGES, languages_shape);
        context.get_proxy(&ExtHostContext::EXT_HOST_LANGUAGES).rpc_accept_language_ids(LANGUAGE_IDS.iter().map(|id| (*id).to_owned()).collect());

        let features_shape: Rc<dyn MainThreadLanguageFeaturesShape> = self.language_features.clone();
        context.set(&MainContext::MAIN_THREAD_LANGUAGE_FEATURES, features_shape);
        *self.features_proxy.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_LANGUAGE_FEATURES));

        let documents_and_editors: Rc<dyn IDisposable> = documents_and_editors;
        let diagnostics: Rc<dyn IDisposable> = diagnostics;
        vec![documents_and_editors, diagnostics]
    }
}

/// O `setup` comum: o binder e o que o teste observa.
pub fn languages_setup() -> (Rc<dyn IMainThreadBinder>, Observed) {
    let config_service = TestConfigurationService::new(None);
    let configuration_service: Rc<dyn IConfigurationService> = config_service;
    let model_service = ModelService::new(configuration_service.clone(), TestTextResourcePropertiesService::new(configuration_service), UndoRedoService::for_tests());
    let marker_service = MarkerService::new();
    let languages = Rc::new(RecordingLanguages::default());
    let language_features = Rc::new(RecordingLanguageFeatures::default());
    let features_proxy = Rc::new(RefCell::new(None));
    let binder = LanguagesBinder {
        model_service: model_service.clone(),
        code_editor_service: Rc::new(TestCodeEditorService::new(TestThemeService::new_default())),
        file_service: TestFileService::new(),
        marker_service: marker_service.clone(),
        languages: languages.clone(),
        language_features: language_features.clone(),
        features_proxy: features_proxy.clone(),
    };
    (Rc::new(binder), Observed { model_service, marker_service, languages, language_features, features_proxy })
}

/// O URI de um arquivo do projeto de exemplo (`/proj/<name>`).
pub fn project_file(name: &str) -> URI {
    URI::file(format!("/proj/{name}"), je_platform::resources::host_platform())
}

/// Um modelo no `ModelService` do arquivo `name` do projeto de exemplo (vira um `TextDocument` na extensão).
pub fn create_document(observed: &Observed, name: &str, text: &str) -> Rc<TextModel> {
    observed.model_service.create_model(text, None, Some(&project_file(name)), false)
}
