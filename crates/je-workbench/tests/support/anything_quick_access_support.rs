//! A bancada do `anythingQuickAccess.test.ts`: os dublês dos serviços que o `workbenchInstantiationService` e os `stub` do
//! original montam, e o [`AnythingFixture`] que os junta num [`AnythingQuickAccessProvider`].
//!
//! Notas de tradução:
//! - **Serviços reais.** `ModelService`, `LanguageService`, `FileService`, `WorkingCopyService`, `UriIdentityService`, a
//!   configuração (`TestConfigurationService`), o `QueryBuilder` e o `OpenerService` são os do porte; os serviços de teste do
//!   workbench (`TestPathService`, `TestEnvironmentService`, `TestContextService`, `MockLabelService` e
//!   `NullFilesConfigurationService`) vêm de `je_workbench::test`.
//! - **`instantiationService.stub(...)`.** Um serviço trocado depois de o provedor ter sido criado só vale para o provedor
//!   seguinte; o [`AnythingFixture::create_provider`] captura os serviços na criação, como a injeção do original.
//! - **`upcastPartial`/`mock`.** Os métodos que o original deixa de fora (o objeto parcial lança `TypeError` ao ser chamado)
//!   falham com `panic!`.

use je_instantiation::Injectable;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::LanguageService;
use je_editor::model_service::ModelService;
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::notification::notification::{
    INotificationService, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource, NotificationSourceFilter,
    NotificationsFilter, NoOpNotification, PromptChoiceKind, PromptOptions, StatusHandle, StatusMessageOptions,
};
use je_platform::opener::opener::{
    IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
    AllowContributedOpeners,
};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use je_workbench::common::editor::{EditorInputOrUntyped, GroupIdentifier, UntypedEditorInput};
use je_workbench::contrib::search::browser::anything_quick_access::{
    AnythingQuickAccessProvider, AnythingQuickAccessServices, EditorSymbolPick, IGotoSymbolQuickAccess, IHelpPicks, IPickerEditorState, ISymbolsQuickAccess,
    IAnythingQuickAccessProviderRunOptions,
};
use je_workbench::services::editor::editor_groups_service::{
    FindGroupScope, GroupDirection, GroupRef, GroupsOrder, IEditorGroup, IEditorGroupsService, IEditorPart, IModalEditorPart,
};
use je_workbench::services::editor::editor_service::{ActiveTextEditorControl, IEditorService};
use je_workbench::services::history::history::{GoFilter, IHistoryService};
use je_workbench::services::layout::layout_service::{MAIN_WINDOW, WindowId};
use je_workbench::services::search::query_builder::QueryBuilder;
use je_workbench::services::search::search::{
    IAITextQuery, IFileQuery, ISearchComplete, ISearchResultProvider, ISearchService, ITextQuery, SearchProgressCallback, SearchProviderType,
    SharedResourceSet, TextSearchSplitSyncAsync,
};
use je_workbench::services::textmodel_resolver::resolver_service::{I_TEXT_MODEL_SERVICE, ITextModelService, TextModelReference};
use je_workbench::services::working_copy::working_copy_service::WorkingCopyService;
use je_workbench::test::browser::workbench_test_services::{MockLabelService, TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::{NullFilesConfigurationService, TestContextService};
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, ModalEditorPartOptions};

// ---------------------------------------------------------------------------------------------------------
// Grupos de editores: `IEditorGroupsService.activeGroup`, `getGroup` e `activateGroup` do caso.
// ---------------------------------------------------------------------------------------------------------

/// `new class extends mock<IEditorGroup>() { override readonly id = N; }`.
pub struct TestGroup {
    id: GroupIdentifier,
}

impl TestGroup {
    pub fn new(id: GroupIdentifier) -> Rc<Self> {
        Rc::new(Self { id })
    }
}

impl IEditorGroup for TestGroup {
    fn id(&self) -> GroupIdentifier {
        self.id
    }

    fn window_id(&self) -> WindowId {
        MAIN_WINDOW
    }

    fn active_editor(&self) -> Option<je_workbench::common::editor::EditorInputRef> {
        panic!("IEditorGroup.activeEditor não é usado por este teste")
    }

    fn editors(&self) -> Vec<je_workbench::common::editor::EditorInputRef> {
        panic!("IEditorGroup.editors não é usado por este teste")
    }

    fn is_locked(&self) -> bool {
        panic!("IEditorGroup.isLocked não é usado por este teste")
    }

    fn is_active(&self, _editor: &EditorInputOrUntyped) -> bool {
        panic!("IEditorGroup.isActive não é usado por este teste")
    }
}

/// O estado compartilhado do serviço de grupos do caso.
pub struct GroupsState {
    pub source_group: Rc<TestGroup>,
    pub other_group: Rc<TestGroup>,
    pub active_group: RefCell<Rc<TestGroup>>,
    pub activated_groups: RefCell<Vec<GroupIdentifier>>,
    /// `assert.ok(allowActiveGroupAccess, 'This picker must not access the active editor group')`.
    pub allow_active_group_access: Cell<bool>,
}

impl GroupsState {
    pub fn new() -> Rc<Self> {
        let source_group = TestGroup::new(1);
        let other_group = TestGroup::new(2);
        Rc::new(Self {
            active_group: RefCell::new(source_group.clone()),
            source_group,
            other_group,
            activated_groups: RefCell::new(Vec::new()),
            allow_active_group_access: Cell::new(true),
        })
    }

    fn by_id(&self, id: GroupIdentifier) -> Rc<TestGroup> {
        if id == self.source_group.id { self.source_group.clone() } else { self.other_group.clone() }
    }
}

/// O `IEditorGroupsService` do `setup` (`{ get activeGroup, getGroup, activateGroup }`).
pub struct TestGroupsService {
    pub state: Rc<GroupsState>,
}

impl IEditorPart for TestGroupsService {
    fn window_id(&self) -> WindowId {
        MAIN_WINDOW
    }

    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        assert!(self.state.allow_active_group_access.get(), "This picker must not access the active editor group");
        self.state.active_group.borrow().clone()
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        panic!("IEditorGroupsService.groups não é usado por este teste")
    }
}

impl IEditorGroupsService for TestGroupsService {
    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        assert!(self.state.allow_active_group_access.get(), "This picker must not access the active editor group");
        self.state.active_group.borrow().clone()
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        panic!("IEditorGroupsService.groups não é usado por este teste")
    }

    fn get_groups(&self, _order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        panic!("IEditorGroupsService.getGroups não é usado por este teste")
    }

    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        Some(self.state.by_id(identifier))
    }

    fn find_group(&self, _scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        panic!("IEditorGroupsService.findGroup não é usado por este teste")
    }

    fn add_group(&self, _location: Option<GroupRef>, _direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        panic!("IEditorGroupsService.addGroup não é usado por este teste")
    }

    fn main_part(&self) -> Rc<dyn IEditorPart> {
        panic!("IEditorGroupsService.mainPart não é usado por este teste")
    }

    fn create_auxiliary_editor_part(&self, _options: Option<AuxiliaryEditorOptions>) -> Promise<Rc<dyn IEditorPart>> {
        panic!("IEditorGroupsService.createAuxiliaryEditorPart não é usado por este teste")
    }

    fn create_modal_editor_part(&self, _options: Option<ModalEditorPartOptions>) -> Promise<Rc<dyn IModalEditorPart>> {
        panic!("IEditorGroupsService.createModalEditorPart não é usado por este teste")
    }

    fn active_modal_editor_part(&self) -> Option<Rc<dyn IModalEditorPart>> {
        None
    }

    /// `activateGroup: group => { ... }`.
    fn activate_group(&self, group: GroupRef) -> Rc<dyn IEditorGroup> {
        let id = match group {
            GroupRef::Id(id) => id,
            GroupRef::Group(group) => group.id(),
        };
        self.state.activated_groups.borrow_mut().push(id);
        let active = self.state.by_id(id);
        *self.state.active_group.borrow_mut() = active.clone();
        active
    }
}

// ---------------------------------------------------------------------------------------------------------
// Abridor e notificações
// ---------------------------------------------------------------------------------------------------------

/// O que `open` registrou: `{ resource, options }`.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenedLink {
    pub resource: String,
    pub from_user_gesture: Option<bool>,
    pub allow_contributed_openers: Option<bool>,
    pub open_to_side: Option<bool>,
    pub preserve_focus: Option<bool>,
    pub pinned: Option<bool>,
}

/// O `IOpenerService` do `setup`: registra a chamada de `open` e devolve `handled` ou rejeita com `openError`.
pub struct TestOpenerService {
    pub opened: RefCell<Vec<OpenedLink>>,
    pub handled: Cell<bool>,
    pub open_error: RefCell<Option<Error>>,
}

impl TestOpenerService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { opened: RefCell::new(Vec::new()), handled: Cell::new(true), open_error: RefCell::new(None) })
    }
}

impl IOpenerService for TestOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        panic!("IOpenerService.registerOpener não é usado por este teste")
    }

    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        panic!("IOpenerService.registerValidator não é usado por este teste")
    }

    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        panic!("IOpenerService.registerExternalUriResolver não é usado por este teste")
    }

    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        panic!("IOpenerService.setDefaultExternalOpener não é usado por este teste")
    }

    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        panic!("IOpenerService.registerExternalOpener não é usado por este teste")
    }

    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        let resource_text = match resource {
            OpenResource::Text(text) => text.to_string_lossy(),
            OpenResource::Uri(uri) => uri.to_string(false).to_string_lossy(),
        };
        self.opened.borrow_mut().push(OpenedLink {
            resource: resource_text,
            from_user_gesture: options.and_then(|options| options.from_user_gesture),
            allow_contributed_openers: options.and_then(|options| match &options.allow_contributed_openers {
                Some(AllowContributedOpeners::Bool(value)) => Some(*value),
                _ => None,
            }),
            open_to_side: options.and_then(|options| options.open_to_side),
            preserve_focus: options.and_then(|options| options.editor_options.as_ref().and_then(|editor_options| editor_options.preserve_focus)),
            pinned: options.and_then(|options| options.editor_options.as_ref().and_then(|editor_options| editor_options.pinned)),
        });
        if let Some(error) = self.open_error.borrow().clone() {
            return Promise::rejected(error);
        }
        Promise::resolved(self.handled.get())
    }

    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        panic!("IOpenerService.resolveExternalUri não é usado por este teste")
    }
}

/// O que `warn` e `error` registraram: `message` (texto ou erro).
#[derive(Clone, Debug, PartialEq)]
pub enum RecordedNotification {
    Text(String),
    Error(Error),
}

/// O `INotificationService` do `setup` (`{ warn, error }`).
pub struct RecordingNotificationService {
    pub notifications: RefCell<Vec<RecordedNotification>>,
}

impl RecordingNotificationService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { notifications: RefCell::new(Vec::new()) })
    }

    fn record(&self, messages: Vec<NotificationMessage>) {
        for message in messages {
            match message {
                NotificationMessage::Text(text) => self.notifications.borrow_mut().push(RecordedNotification::Text(text)),
                NotificationMessage::Error(error) => self.notifications.borrow_mut().push(RecordedNotification::Error(error)),
                _ => panic!("o teste só registra texto e erro"),
            }
        }
    }
}

impl INotificationService for RecordingNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        panic!("INotificationService.onDidChangeFilter não é usado por este teste")
    }

    fn set_filter(&self, _filter: NotificationFilterSetting) {
        panic!("INotificationService.setFilter não é usado por este teste")
    }

    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        panic!("INotificationService.getFilter não é usado por este teste")
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        panic!("INotificationService.getFilters não é usado por este teste")
    }

    fn remove_filter(&self, _source_id: &str) {
        panic!("INotificationService.removeFilter não é usado por este teste")
    }

    fn notify(&self, _notification: Notification) -> Rc<dyn NotificationHandle> {
        panic!("INotificationService.notify não é usado por este teste")
    }

    fn info(&self, _message: Vec<NotificationMessage>) {
        panic!("INotificationService.info não é usado por este teste")
    }

    fn warn(&self, message: Vec<NotificationMessage>) {
        self.record(message);
    }

    fn error(&self, message: Vec<NotificationMessage>) {
        self.record(message);
    }

    fn prompt(&self, _severity: je_base::severity::Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        let _ = NoOpNotification::new();
        panic!("INotificationService.prompt não é usado por este teste")
    }

    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        panic!("INotificationService.status não é usado por este teste")
    }
}

// ---------------------------------------------------------------------------------------------------------
// Busca, histórico, rótulos e modelos
// ---------------------------------------------------------------------------------------------------------

/// `{ fileSearch: async () => ({ results: [], messages: [] }), clearCache: async () => { } }`.
pub struct TestSearchService;

impl ISearchService for TestSearchService {
    fn text_search(&self, _query: &ITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        panic!("ISearchService.textSearch não é usado por este teste")
    }

    fn ai_text_search(&self, _query: &IAITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        panic!("ISearchService.aiTextSearch não é usado por este teste")
    }

    fn get_ai_name(&self) -> Promise<Option<String>> {
        panic!("ISearchService.getAIName não é usado por este teste")
    }

    fn text_search_split_sync_async(
        &self,
        _query: &ITextQuery,
        _token: Option<CancellationToken>,
        _on_progress: Option<SearchProgressCallback>,
        _notebook_files_to_ignore: Option<SharedResourceSet>,
        _async_notebook_files_to_ignore: Option<Promise<SharedResourceSet>>,
    ) -> TextSearchSplitSyncAsync {
        panic!("ISearchService.textSearchSplitSyncAsync não é usado por este teste")
    }

    fn file_search(&self, _query: &IFileQuery, _token: Option<CancellationToken>) -> Promise<ISearchComplete> {
        Promise::resolved(ISearchComplete { limit_hit: None, messages: Vec::new(), stats: None, results: Vec::new(), exit: None, ai_keywords: None })
    }

    fn scheme_has_file_search_provider(&self, _scheme: &str) -> bool {
        panic!("ISearchService.schemeHasFileSearchProvider não é usado por este teste")
    }

    fn clear_cache(&self, _cache_key: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn register_search_result_provider(&self, _scheme: &str, _provider_type: SearchProviderType, _provider: Rc<dyn ISearchResultProvider>) -> DisposableRef {
        panic!("ISearchService.registerSearchResultProvider não é usado por este teste")
    }
}

/// O `IHistoryService` sem histórico (`workbenchInstantiationService` entrega um histórico vazio ao caso).
pub struct TestHistoryService;

impl IHistoryService for TestHistoryService {
    fn go_forward(&self, _filter: Option<GoFilter>) -> Promise<()> {
        panic!("IHistoryService.goForward não é usado por este teste")
    }

    fn go_back(&self, _filter: Option<GoFilter>) -> Promise<()> {
        panic!("IHistoryService.goBack não é usado por este teste")
    }

    fn go_previous(&self, _filter: Option<GoFilter>) -> Promise<()> {
        panic!("IHistoryService.goPrevious não é usado por este teste")
    }

    fn go_last(&self, _filter: Option<GoFilter>) -> Promise<()> {
        panic!("IHistoryService.goLast não é usado por este teste")
    }

    fn reopen_last_closed_editor(&self) -> Promise<()> {
        panic!("IHistoryService.reopenLastClosedEditor não é usado por este teste")
    }

    fn get_history(&self) -> Vec<EditorInputOrUntyped> {
        Vec::new()
    }

    fn remove_from_history(&self, _input: &EditorInputOrUntyped) {}

    fn get_last_active_workspace_root(&self, _scheme_filter: Option<&str>, _authority_filter: Option<&str>) -> Option<URI> {
        None
    }

    fn get_last_active_file(&self, _scheme_filter: &str, _authority_filter: Option<&str>) -> Option<URI> {
        None
    }

    fn open_next_recently_used_editor(&self, _group: Option<GroupIdentifier>) -> Promise<()> {
        panic!("IHistoryService.openNextRecentlyUsedEditor não é usado por este teste")
    }

    fn open_previously_used_editor(&self, _group: Option<GroupIdentifier>) -> Promise<()> {
        panic!("IHistoryService.openPreviouslyUsedEditor não é usado por este teste")
    }

    fn clear(&self) {}

    fn clear_recently_opened(&self) {}
}

/// O `ICustomEditorLabelService` sem rótulos personalizados.
pub struct TestCustomEditorLabelService;

impl je_workbench::services::editor::custom_editor_label_service::ICustomEditorLabelService for TestCustomEditorLabelService {
    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }

    fn get_name(&self, _resource: &URI) -> Option<String> {
        None
    }
}

/// O `ITextModelService` que o caso não usa.
pub struct TestTextModelService;

impl ITextModelService for TestTextModelService {
    fn create_model_reference(&self, _resource: &URI) -> Promise<TextModelReference> {
        panic!("ITextModelService.createModelReference não é usado por este teste")
    }
}

/// Os colaboradores de símbolos, de estado do editor e de ajuda, sem resultados.
pub struct TestSymbols;

impl ISymbolsQuickAccess for TestSymbols {
    fn get_symbol_picks(&self, _filter: &str, _skip_local: bool, _skip_sorting: bool, _delay: u64, _token: CancellationToken) -> Promise<Vec<je_platform::quickinput::quick_input::IQuickPickItem>> {
        Promise::resolved(Vec::new())
    }
}

impl IGotoSymbolQuickAccess for TestSymbols {
    fn get_symbol_picks(&self, _model: &Rc<TextModel>, _filter: &str, _extra_container_label: &str, _disposables: &Rc<DisposableStore>, _token: CancellationToken) -> Promise<Vec<EditorSymbolPick>> {
        Promise::resolved(Vec::new())
    }

    fn add_decorations(&self, _editor: &ActiveTextEditorControl, _range: &Range) {}

    fn clear_decorations(&self, _editor: &ActiveTextEditorControl) {}
}

/// O `PickerEditorState` sem editor: restaurar, abrir e resetar não fazem nada.
pub struct TestEditorViewState;

impl IPickerEditorState for TestEditorViewState {
    fn set(&self) {}

    fn restore(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn reset(&self) {}

    fn open_transient_editor(&self, _editor: UntypedEditorInput) -> Promise<()> {
        Promise::resolved(())
    }
}

pub struct TestHelpPicks;

impl IHelpPicks for TestHelpPicks {
    fn get_help_picks(&self, _query: &je_base::fuzzy_scorer::PreparedQuery, _run_options: &IAnythingQuickAccessProviderRunOptions) -> Vec<je_platform::quickinput::quick_input::IQuickPickItem> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------------------------------------
// O conjunto
// ---------------------------------------------------------------------------------------------------------

/// O `setup` do original.
pub struct AnythingFixture {
    pub groups: Rc<GroupsState>,
    pub opener: Rc<TestOpenerService>,
    pub notifications: Rc<RecordingNotificationService>,
    pub configuration_service: Rc<TestConfigurationService>,
    pub instantiation_service: Rc<InstantiationService>,
    services: AnythingQuickAccessServices,
}

impl AnythingFixture {
    pub fn new(store: &Rc<DisposableStore>) -> Self {
        let groups = GroupsState::new();
        let groups_service: Rc<dyn IEditorGroupsService> = Rc::new(TestGroupsService { state: groups.clone() });
        let opener = TestOpenerService::new();
        let notifications = RecordingNotificationService::new();

        let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
            (
                "workbench",
                Dynamic::object([(
                    "editor",
                    Dynamic::object([
                        ("enablePreview", Dynamic::Bool(true)),
                        ("enablePreviewFromQuickOpen", Dynamic::Bool(true)),
                        ("openSideBySideDirection", Dynamic::string("right")),
                    ]),
                )]),
            ),
            ("search", Dynamic::object(Vec::<(&str, Dynamic)>::new())),
        ])));
        store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();

        let file_service = store.add_rc(FileService::new(NullLogService::shared()));
        let file_service: Rc<dyn IFileService> = file_service;
        let uri_identity_service = store.add_rc(UriIdentityService::create((), (file_service.clone(),)));
        let uri_identity_service: Rc<dyn IUriIdentityService> = uri_identity_service;
        let language_service = store.add_rc(LanguageService::new(false));
        let model_service = store.add_rc(ModelService::new(
            configuration.clone(),
            je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService::new(configuration.clone()),
            UndoRedoService::for_tests(),
        ));
        let model_service: Rc<dyn IModelService> = model_service;
        let context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(None, None);
        let path_service = TestPathService::new(None, None);
        let working_copy_service = store.add_rc(WorkingCopyService::new());
        let working_copy_service: Rc<dyn je_workbench::services::working_copy::working_copy_service::IWorkingCopyService> = working_copy_service;

        let services = ServiceCollection::new()
            .with(I_CONFIGURATION_SERVICE, configuration.clone())
            .with(I_LOG_SERVICE, NullLogService::shared())
            .with(I_URI_IDENTITY_SERVICE, uri_identity_service.clone())
            .with(je_platform::workspace::I_WORKSPACE_CONTEXT_SERVICE, context_service.clone())
            .with(je_workbench::services::path::path_service::I_PATH_SERVICE, path_service.clone() as Rc<dyn je_workbench::services::path::path_service::IPathService>)
            .with(je_workbench::services::editor::editor_groups_service::I_EDITOR_GROUPS_SERVICE, groups_service.clone())
            .with(I_TEXT_MODEL_SERVICE, Rc::new(TestTextModelService) as Rc<dyn ITextModelService>);
        let instantiation_service = InstantiationService::new(services);
        let query_builder = instantiation_service.create_instance::<QueryBuilder>(());

        let symbols = Rc::new(TestSymbols);
        let editor_service: Rc<dyn IEditorService> = Rc::new(TestEditorService);
        let anything_services = AnythingQuickAccessServices {
            search_service: Rc::new(TestSearchService),
            context_service,
            path_service,
            environment_service: TestEnvironmentService::new(),
            file_service,
            label_service: MockLabelService::new(None),
            model_service,
            language_service,
            working_copy_service,
            configuration_service: configuration,
            editor_service,
            history_service: Rc::new(TestHistoryService),
            files_configuration_service: Rc::new(NullFilesConfigurationService),
            text_model_service: Rc::new(TestTextModelService),
            uri_identity_service,
            log_service: NullLogService::shared(),
            custom_editor_label_service: Rc::new(TestCustomEditorLabelService),
            opener_service: opener.clone(),
            notification_service: notifications.clone(),
            editor_groups_service: groups_service,
            workspace_symbols_quick_access: symbols.clone(),
            editor_symbols_quick_access: symbols,
            editor_view_state: Rc::new(TestEditorViewState),
            help_picks: Rc::new(TestHelpPicks),
            out_of_workspace_editor_resources: Rc::new(Vec::new),
        };
        let _ = query_builder;
        Self { groups, opener, notifications, configuration_service, instantiation_service, services: anything_services }
    }

    /// `instantiationService.createInstance(AnythingQuickAccessProvider)` com os serviços do momento.
    pub fn create_provider(&self) -> Rc<AnythingQuickAccessProvider> {
        let query_builder = self.instantiation_service.create_instance::<QueryBuilder>(());
        AnythingQuickAccessProvider::new(self.services.clone(), query_builder)
    }

    /// `instantiationService.stub(IOpenerService, service)`.
    pub fn stub_opener_service(&mut self, opener: Rc<dyn IOpenerService>) {
        self.services.opener_service = opener;
    }
}

/// `JsString` de um texto (para comparar rótulos).
pub fn js(text: &str) -> JsString {
    JsString::from(text)
}
