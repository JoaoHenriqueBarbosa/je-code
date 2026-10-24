//! Dublês de serviço e construtores de dados do `extensionRecommendationsService.test.ts` (suíte
//! `ExtensionRecommendationsService Test`).
//!
//! Notas de tradução:
//! - `TestInstantiationService` (`stub`, `get`, `createInstance`) é o [`RecommendationsFixture`]. O `setup` do original
//!   reaproveita o do `extensionsWorkbenchService.test.ts` ([`WorkbenchFixture`]: o `disposableStore`, o serviço de
//!   gerenciamento, a galeria, o armazenamento, o `ExtensionsWorkbenchService`) e acrescenta o que é desta suíte: o sistema de
//!   arquivos em memória no esquema `vscode-tests`, o `UriIdentityService` real, o `TestConfigurationService`, o serviço de
//!   notificação que registra o `prompt` (`TestNotificationService2`), os serviços de modelo e de idioma reais (o literal do
//!   original só tem `getModels` e `onModelAdded`) e o serviço de produto com `extensionRecommendations`.
//! - **`setUpFolder`** cria, como o original, o `WorkspaceExtensionsConfigService`, o `ExtensionIgnoredRecommendationsService`
//!   e o `ExtensionRecommendationNotificationService` sobre o workspace da pasta.
//! - **Serviços não registrados.** O `TestInstantiationService` entrega `undefined` ao que o `setup` não registra
//!   (`IQuickInputService`, `IJSONEditingService`, `IRemoteExtensionsScannerService`, `IUserDataInitializationService`); os
//!   campos opcionais dos parâmetros são `None` nesses casos (ver as notas dos módulos de produção).
//! - **`TestExtensionTipsService`** é o `NativeExtensionTipsService`. O original o cria no `setup`, antes do serviço de
//!   notificação de recomendações existir; aqui ele nasce em [`RecommendationsFixture::create_service`], com o serviço de
//!   notificação já criado (o serviço de dicas só o usa para avisar executáveis, e o produto do teste não declara nenhum).
//! - **`IOpener`.** `ExtensionsWorkbenchService.open/openSearch` não existem no porte: [`NoOpExtensionsOpener`] é o que o
//!   serviço de notificação chama ao clicar em "Install" ou "Show Recommendations", ações que nenhum caso aciona.
//! - **Relógio.** O corpo roda dentro de `run_with_faked_timers`, como o `runWithFakedTimers({ useFakeTimers: true })`; o
//!   `teardown` descarta o `disposableStore` e espera um `timeout(0)`.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::paging::IPager;
use je_base::product::{IExtensionRecommendations, IFileOpenCondition, IFilePathCondition, IStringDictionary};
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_editor::language_service::LanguageService;
use je_editor::model_service::ModelService;
use je_editor::undo_redo_service::UndoRedoService;
use je_extensions::management::{
    ExtensionInfo, ExtensionQueryOptions, ExtensionsControlManifest, GalleryExtension, GalleryExtensionAssets, GalleryExtensionVersion, InstallOperation, ProductVersion,
    QueryOptions, StatisticType,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, TargetPlatform};
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::notification::{
    INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource, NotificationSourceFilter,
    NotificationsFilter, PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};
use je_platform::product::{IProductService, ProductService};
use je_platform::resources::{join_path, uri_file};
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::workspace::test_workspace;
use je_workbench::contrib::extensions::extension_recommendation_notification_service::{
    ExtensionRecommendationNotificationService, ExtensionRecommendationNotificationServiceParts, IExtensionsOpener,
};
use je_workbench::contrib::extensions::extension_recommendations_service::{ExtensionRecommendationsService, ExtensionRecommendationsServiceParts};
use je_workbench::contrib::extensions::extensions_workbench_service::Extension;
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::extension_management::extension_gallery_service::IExtensionGalleryService;
use je_workbench::services::extension_management::extension_management::{ExtensionRef, IExtensionManagementServer, IExtensionManagementServerService};
use je_workbench::services::extension_management::extension_tips_service::{NativeExtensionTipsService, NativeExtensionTipsServiceParts, WindowEvents};
use je_workbench::services::extension_recommendations::extension_ignored_recommendations_service::ExtensionIgnoredRecommendationsService;
use je_workbench::services::extension_recommendations::workspace_extensions_config::{WorkspaceExtensionsConfigService, WorkspaceExtensionsConfigServiceParts};
use je_workbench::services::untitled::untitled_text_editor_service::UntitledTextEditorService;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestLifecycleService, TestNotificationService, TestTextResourcePropertiesService};
use serde_json::{Value, json};

use super::extension_enablement_support::common::{fail_on_unexpected_errors, run_with_faked_timers, timeout};
use super::extension_enablement_support::{StubUserDataSyncEnablementService, an_extension_management_server_service};
use super::extensions_workbench_service_support::{GalleryProperties, LocalProperties, StubGalleryService, WorkbenchFixture, a_gallery_extension_with, a_local_extension_with, asset};

// ---------------------------------------------------------------------------------------------------------------------
// Dados.
// ---------------------------------------------------------------------------------------------------------------------

/// `ROOT = URI.file('tests').with({ scheme: 'vscode-tests' })`.
pub fn root() -> URI {
    uri_file("tests").with(&UriChange { scheme: Field::from("vscode-tests"), ..UriChange::default() })
}

/// O `assets` completo dos dois itens de `mockExtensionGallery`.
fn mock_assets() -> GalleryExtensionAssets {
    GalleryExtensionAssets {
        manifest: Some(asset("uri:manifest", "fallback:manifest")),
        readme: Some(asset("uri:readme", "fallback:readme")),
        changelog: Some(asset("uri:changelog", "fallback:changlog")),
        download: asset("uri:download", "fallback:download"),
        icon: Some(asset("uri:icon", "fallback:icon")),
        license: Some(asset("uri:license", "fallback:license")),
        repository: Some(asset("uri:repository", "fallback:repository")),
        signature: Some(asset("uri:signature", "fallback:signature")),
        core_translations: Vec::new(),
    }
}

/// `mockExtensionGallery`.
pub fn mock_extension_gallery() -> Vec<GalleryExtension> {
    vec![
        a_gallery_extension_with(
            "MockExtension1",
            GalleryProperties {
                display_name: Some("Mock Extension 1"),
                version: Some("1.5"),
                publisher_id: Some("mockPublisher1Id"),
                publisher: Some("mockPublisher1"),
                publisher_display_name: Some("Mock Publisher 1"),
                description: Some("Mock Description"),
                install_count: Some(1000.0),
                rating: Some(4.0),
                rating_count: Some(100.0),
                ..GalleryProperties::default()
            },
            Some(&["pub.1"]),
            Some(mock_assets()),
        ),
        a_gallery_extension_with(
            "MockExtension2",
            GalleryProperties {
                display_name: Some("Mock Extension 2"),
                version: Some("1.5"),
                publisher_id: Some("mockPublisher2Id"),
                publisher: Some("mockPublisher2"),
                publisher_display_name: Some("Mock Publisher 2"),
                description: Some("Mock Description"),
                install_count: Some(1000.0),
                rating: Some(4.0),
                rating_count: Some(100.0),
                ..GalleryProperties::default()
            },
            Some(&["pub.1", "pub.2"]),
            Some(mock_assets()),
        ),
    ]
}

/// `mockExtensionLocal`: as duas extensões instaladas, com o identificador das da galeria.
pub fn mock_extension_local(gallery: &[GalleryExtension]) -> Vec<ExtensionRef> {
    gallery
        .iter()
        .map(|extension| {
            a_local_extension_with(
                &extension.name,
                json!({ "name": extension.name, "publisher": extension.publisher, "version": extension.version }),
                LocalProperties { identifier: Some(extension.identifier.clone()), ..LocalProperties::default() },
            )
        })
        .collect()
}

/// `mockTestData.recommendedExtensions`.
pub const RECOMMENDED_EXTENSIONS: [&str; 5] = ["mockPublisher1.mockExtension1", "MOCKPUBLISHER2.mockextension2", "badlyformattedextension", "MOCKPUBLISHER2.mockextension2", "unknown.extension"];

/// `mockTestData.validRecommendedExtensions`.
pub const VALID_RECOMMENDED_EXTENSIONS: [&str; 2] = ["mockPublisher1.mockExtension1", "MOCKPUBLISHER2.mockextension2"];

/// `{ pathGlob, important? }`.
fn path_condition(path_glob: &str, important: bool) -> IFileOpenCondition {
    IFileOpenCondition::Path(IFilePathCondition {
        path_glob: path_glob.to_owned(),
        content_pattern: None,
        important: important.then_some(true),
        when_installed: None,
        when_not_installed: None,
        extra: Default::default(),
    })
}

/// O `IProductService` do `setup`: só o `extensionRecommendations`.
pub fn product_with_extension_recommendations() -> ProductService {
    let entry = |conditions: Vec<IFileOpenCondition>| IExtensionRecommendations { on_file_open: Some(conditions), on_settings_editor_open: None, extra: Default::default() };
    let mut recommendations: IStringDictionary<IExtensionRecommendations> = IndexMap::new();
    recommendations.insert("ms-python.python".to_owned(), entry(vec![path_condition("{**/*.py}", true)]));
    recommendations.insert("ms-vscode.PowerShell".to_owned(), entry(vec![path_condition("{**/*.ps,**/*.ps1}", true)]));
    recommendations.insert(
        "ms-dotnettools.csharp".to_owned(),
        entry(vec![path_condition("{**/*.cs,**/project.json,**/global.json,**/*.csproj,**/*.sln,**/appsettings.json}", false)]),
    );
    recommendations.insert(
        "msjsdiag.debugger-for-chrome".to_owned(),
        entry(vec![path_condition("{**/*.ts,**/*.tsx,**/*.js,**/*.jsx,**/*.es6,**/*.mjs,**/*.cjs,**/.babelrc}", false)]),
    );
    recommendations.insert("lukehoban.Go".to_owned(), entry(vec![path_condition("**/*.go", false)]));
    ProductService { extension_recommendations: Some(recommendations), ..ProductService::default() }
}

// ---------------------------------------------------------------------------------------------------------------------
// Dublês.
// ---------------------------------------------------------------------------------------------------------------------

/// `TestNotificationService2`: o `TestNotificationService` que também registra o `prompt` e dispara `promptedEmitter`.
pub struct PromptTrackingNotificationService {
    inner: TestNotificationService,
    prompted: Cell<bool>,
    prompted_emitter: Rc<Emitter<()>>,
}

impl PromptTrackingNotificationService {
    pub fn new(prompted_emitter: Rc<Emitter<()>>) -> Rc<Self> {
        Rc::new(Self { inner: TestNotificationService, prompted: Cell::new(false), prompted_emitter })
    }

    /// `prompted`.
    pub fn prompted(&self) -> bool {
        self.prompted.get()
    }
}

impl INotificationService for PromptTrackingNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        self.inner.on_did_change_filter()
    }

    fn set_filter(&self, filter: NotificationFilterSetting) {
        self.inner.set_filter(filter);
    }

    fn get_filter(&self, source: Option<&NotificationSource>) -> NotificationsFilter {
        self.inner.get_filter(source)
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        self.inner.get_filters()
    }

    fn remove_filter(&self, source_id: &str) {
        self.inner.remove_filter(source_id);
    }

    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        self.inner.notify(notification)
    }

    fn info(&self, message: Vec<NotificationMessage>) {
        self.inner.info(message);
    }

    fn warn(&self, message: Vec<NotificationMessage>) {
        self.inner.warn(message);
    }

    fn error(&self, message: Vec<NotificationMessage>) {
        self.inner.error(message);
    }

    fn prompt(&self, severity: Severity, message: NotificationMessage, choices: Vec<PromptChoiceKind>, options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        self.prompted.set(true);
        self.prompted_emitter.fire(());
        self.inner.prompt(severity, message, choices, options)
    }

    fn status(&self, message: NotificationMessage, options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        self.inner.status(message, options)
    }
}

/// O `IExtensionGalleryService` do `setup` com `isEnabled` e `query` trocáveis: `{ query: galleryQuerySpy, isEnabled: () => false }`
/// do primeiro caso. As demais chamadas vão para a galeria do `setup`.
pub struct ToggleGallery {
    inner: Rc<StubGalleryService>,
    enabled: Cell<bool>,
    query_calls: Cell<usize>,
}

impl ToggleGallery {
    pub fn new(inner: Rc<StubGalleryService>) -> Rc<Self> {
        Rc::new(Self { inner, enabled: Cell::new(true), query_calls: Cell::new(0) })
    }

    /// `isEnabled: () => enabled`.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
    }

    /// `galleryQuerySpy.callCount`.
    pub fn query_calls(&self) -> usize {
        self.query_calls.get()
    }
}

impl IExtensionGalleryService for ToggleGallery {
    fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    fn query(&self, options: QueryOptions, token: CancellationToken) -> Promise<Rc<dyn IPager<GalleryExtension>>> {
        self.query_calls.set(self.query_calls.get() + 1);
        self.inner.query(options, token)
    }

    fn get_extensions(&self, extension_infos: Vec<ExtensionInfo>, options: ExtensionQueryOptions, token: CancellationToken) -> Promise<Vec<GalleryExtension>> {
        self.inner.get_extensions(extension_infos, options, token)
    }

    fn is_extension_compatible(&self, extension: &GalleryExtension, include_pre_release: bool, target_platform: TargetPlatform, product_version: Option<ProductVersion>) -> Promise<bool> {
        self.inner.is_extension_compatible(extension, include_pre_release, target_platform, product_version)
    }

    fn get_compatible_extension(
        &self,
        extension: &GalleryExtension,
        include_pre_release: bool,
        target_platform: TargetPlatform,
        product_version: Option<ProductVersion>,
    ) -> Promise<Option<GalleryExtension>> {
        self.inner.get_compatible_extension(extension, include_pre_release, target_platform, product_version)
    }

    fn get_all_compatible_versions(&self, extension_identifier: &ExtensionIdentifierInfo, include_pre_release: bool, target_platform: TargetPlatform) -> Promise<Vec<GalleryExtensionVersion>> {
        self.inner.get_all_compatible_versions(extension_identifier, include_pre_release, target_platform)
    }

    fn get_all_versions(&self, extension_identifier: &ExtensionIdentifierInfo) -> Promise<Vec<GalleryExtensionVersion>> {
        self.inner.get_all_versions(extension_identifier)
    }

    fn download(&self, extension: &GalleryExtension, location: &URI, operation: InstallOperation) -> Promise<()> {
        self.inner.download(extension, location, operation)
    }

    fn download_signature_archive(&self, extension: &GalleryExtension, location: &URI) -> Promise<()> {
        self.inner.download_signature_archive(extension, location)
    }

    fn report_statistic(&self, publisher: &str, name: &str, version: &str, statistic_type: StatisticType) -> Promise<()> {
        self.inner.report_statistic(publisher, name, version, statistic_type)
    }

    fn get_readme(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<String> {
        self.inner.get_readme(extension, token)
    }

    fn get_manifest(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<Option<Value>> {
        self.inner.get_manifest(extension, token)
    }

    fn get_changelog(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<String> {
        self.inner.get_changelog(extension, token)
    }

    fn get_core_translation(&self, extension: &GalleryExtension, language_id: &str) -> Promise<Option<Value>> {
        self.inner.get_core_translation(extension, language_id)
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        self.inner.get_extensions_control_manifest()
    }
}

/// O que `ExtensionsWorkbenchService.open` e `openSearch` fazem na interface: nada.
pub struct NoOpExtensionsOpener;

impl IExtensionsOpener for NoOpExtensionsOpener {
    fn open(&self, _extension: &Rc<Extension>, _pinned: bool) -> Promise<()> {
        Promise::resolved(())
    }

    fn open_search(&self, _value: &str) {}
}

// ---------------------------------------------------------------------------------------------------------------------
// O `setup`.
// ---------------------------------------------------------------------------------------------------------------------

/// O `TestInstantiationService` do `setup` da suíte `ExtensionRecommendationsService Test`.
pub struct RecommendationsFixture {
    /// O `setup` compartilhado com o `extensionsWorkbenchService.test.ts` (`disposableStore` em `base.guard`).
    pub base: Rc<WorkbenchFixture>,
    pub configuration: Rc<TestConfigurationService>,
    pub file_service: Rc<FileService>,
    pub uri_identity_service: Rc<UriIdentityService>,
    pub gallery: Rc<ToggleGallery>,
    pub lifecycle: Rc<TestLifecycleService>,
    pub notification: Rc<PromptTrackingNotificationService>,
    pub prompted_emitter: Rc<Emitter<()>>,
    pub model_service: Rc<ModelService>,
    pub language_service: Rc<LanguageService>,
    pub untitled_service: Rc<UntitledTextEditorService>,
    pub product: Rc<ProductService>,
    pub environment: RefCell<Rc<TestEnvironmentService>>,
    pub workbench_service: Rc<je_workbench::contrib::extensions::extensions_service::ExtensionsWorkbenchService>,
    pub workspace_service: RefCell<Option<Rc<TestContextService>>>,
    pub workspace_extensions_config_service: RefCell<Option<Rc<WorkspaceExtensionsConfigService>>>,
    pub ignored_recommendations_service: RefCell<Option<Rc<ExtensionIgnoredRecommendationsService>>>,
    pub notification_service: RefCell<Option<Rc<ExtensionRecommendationNotificationService>>>,
}

impl RecommendationsFixture {
    /// O `setup`.
    pub fn new() -> Rc<Self> {
        let base = WorkbenchFixture::new();
        let guard = &base.guard;

        let prompted_emitter = guard.add_rc(Emitter::<()>::new());
        let log = NullLogService::shared();
        let file_service = guard.add_rc(FileService::new(log));
        let file_system_provider = guard.add_rc(InMemoryFileSystemProvider::new());
        guard.add(file_service.register_provider(root().scheme().to_string_lossy().as_str(), file_system_provider).expect("registra o provedor em memória"));
        let file_service_dyn: Rc<dyn IFileService> = file_service.clone();
        let uri_identity_service = guard.add_rc(UriIdentityService::create((), (file_service_dyn.clone(),)));

        let configuration = TestConfigurationService::new(None);
        guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
        let configuration_dyn: Rc<dyn IConfigurationService> = configuration.clone();

        let lifecycle = guard.add_rc(TestLifecycleService::new());
        let gallery = ToggleGallery::new_over(&base);
        let product = Rc::new(product_with_extension_recommendations());

        // `instantiationService.stub(IProductService, { extensionRecommendations })` antes do `ExtensionsWorkbenchService`.
        base.stub_product_service((*product).clone());
        let workbench_service = base.create_workbench_service();

        // `stubPromise(IExtensionGalleryService, 'query' | 'getExtensions', mockExtensionGallery)`.
        base.gallery.stub_query(mock_extension_gallery());
        base.gallery.stub_get_extensions(mock_extension_gallery());

        let model_service = guard.add_rc(ModelService::new(configuration_dyn.clone(), TestTextResourcePropertiesService::new(configuration_dyn.clone()), UndoRedoService::for_tests()));
        let language_service = guard.add_rc(LanguageService::new(false));
        let instantiation_service = guard.add_rc(InstantiationService::new(ServiceCollection::new()));
        let untitled_service = guard.add_rc(UntitledTextEditorService::new(instantiation_service, configuration_dyn));

        let notification = PromptTrackingNotificationService::new(prompted_emitter.clone());

        // `testConfigurationService.setUserConfiguration(ConfigurationKey, { ignoreRecommendations: false })`.
        let this = Rc::new(Self {
            base: base.clone(),
            configuration,
            file_service,
            uri_identity_service,
            gallery,
            lifecycle,
            notification,
            prompted_emitter,
            model_service,
            language_service,
            untitled_service,
            product,
            environment: RefCell::new(TestEnvironmentService::new()),
            workbench_service,
            workspace_service: RefCell::new(None),
            workspace_extensions_config_service: RefCell::new(None),
            ignored_recommendations_service: RefCell::new(None),
            notification_service: RefCell::new(None),
        });
        this.set_extensions_configuration(&[("ignoreRecommendations", false)]);
        this
    }

    /// `testConfigurationService.setUserConfiguration(ConfigurationKey, { campo: valor, ... })`.
    pub fn set_extensions_configuration(&self, entries: &[(&str, bool)]) {
        let value = Dynamic::object(entries.iter().map(|(key, value)| (*key, Dynamic::Bool(*value))));
        let _ = self.configuration.set_user_configuration("extensions", value, None);
    }

    /// `instantiationService.stub(IEnvironmentService, { isExtensionDevelopment })`.
    pub fn stub_extension_development(&self, is_extension_development: bool) {
        *self.environment.borrow_mut() = TestEnvironmentService::with_overrides(EnvironmentOverrides { is_extension_development, ..EnvironmentOverrides::default() });
    }

    /// `instantiationService.get(IStorageService)`.
    pub fn storage(&self) -> Rc<je_platform::storage::AbstractStorageService> {
        self.base.storage.clone()
    }

    /// `setUpFolderWorkspace(folderName, recommendedExtensions, ignoredRecommendations)`.
    pub async fn set_up_folder_workspace(&self, folder_name: &str, recommended_extensions: &[&str], ignored_recommendations: &[&str]) {
        let file_service = self.file_service.clone();
        let folder_dir = join_path(&root(), &[folder_name]);
        let workspace_settings_dir = join_path(&folder_dir, &[".vscode"]);
        file_service.create_folder(&workspace_settings_dir).await.expect("createFolder");
        let config_path = join_path(&workspace_settings_dir, &["extensions.json"]);
        let content = serde_json::to_string_pretty(&json!({ "recommendations": recommended_extensions, "unwantedRecommendations": ignored_recommendations })).expect("JSON.stringify");
        file_service.write_file(&config_path, VSBuffer::from_string(&content).into(), None).await.expect("writeFile");

        let my_workspace = test_workspace(&[folder_dir]);
        let guard = &self.base.guard;
        let file_service_dyn: Rc<dyn IFileService> = file_service;
        let workspace_service = TestContextService::new(Some(my_workspace), None);
        *self.workspace_service.borrow_mut() = Some(workspace_service.clone());

        let config_service = guard.add_rc(WorkspaceExtensionsConfigService::new(WorkspaceExtensionsConfigServiceParts {
            workspace_context_service: workspace_service,
            file_service: file_service_dyn,
            quick_input_service: None,
            model_service: self.model_service.clone(),
            language_service: self.language_service.clone(),
            json_editing_service: None,
        }));
        *self.workspace_extensions_config_service.borrow_mut() = Some(config_service.clone());

        let ignored_service = guard.add_rc(ExtensionIgnoredRecommendationsService::new(config_service, Rc::new(self.base.storage.clone())));
        *self.ignored_recommendations_service.borrow_mut() = Some(ignored_service.clone());

        let environment: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
        let notification_service = guard.add_rc(ExtensionRecommendationNotificationService::new(ExtensionRecommendationNotificationServiceParts {
            configuration_service: self.configuration.clone(),
            storage_service: Rc::new(self.base.storage.clone()),
            notification_service: self.notification.clone(),
            telemetry_service: Rc::new(NullTelemetryServiceShape),
            extensions_workbench_service: self.workbench_service.clone(),
            extensions_opener: Rc::new(NoOpExtensionsOpener),
            extension_management_service: self.base.management.clone(),
            extension_enablement_service: self.base.enablement_service(),
            extension_ignored_recommendations_service: ignored_service,
            user_data_sync_enablement_service: Rc::new(StubUserDataSyncEnablementService { enabled: false }),
            workbench_environment_service: environment,
            uri_identity_service: self.uri_identity_service.clone(),
        }));
        *self.notification_service.borrow_mut() = Some(notification_service);
    }

    /// `disposableStore.add(instantiationService.createInstance(ExtensionRecommendationsService))`.
    pub fn create_service(&self) -> Rc<ExtensionRecommendationsService> {
        let guard = &self.base.guard;
        let notification_service = self.notification_service.borrow().clone().expect("setUpFolderWorkspace antes do serviço");
        let ignored_service = self.ignored_recommendations_service.borrow().clone().expect("setUpFolderWorkspace antes do serviço");
        let config_service = self.workspace_extensions_config_service.borrow().clone().expect("setUpFolderWorkspace antes do serviço");
        let workspace_service = self.workspace_service.borrow().clone().expect("setUpFolderWorkspace antes do serviço");
        let file_service_dyn: Rc<dyn IFileService> = self.file_service.clone();

        let local_server = Rc::new(IExtensionManagementServer { id: "local".to_owned(), label: "local".to_owned(), extension_management_service: self.base.management.clone() });
        let server_service: Rc<dyn IExtensionManagementServerService> = an_extension_management_server_service(Some(local_server), None, None);

        // `instantiationService.stub(IExtensionTipsService, createInstance(TestExtensionTipsService))`.
        let tips_service = guard.add_rc(NativeExtensionTipsService::new(NativeExtensionTipsServiceParts {
            user_home: uri_file("/tmp/tests-home"),
            window_events: WindowEvents { on_did_open_main_window: Event::none(), on_did_focus_main_window: Event::none() },
            telemetry_service: Rc::new(NullTelemetryServiceShape),
            extension_management_service: self.base.management.clone(),
            storage_service: Rc::new(self.base.storage.clone()),
            extension_recommendation_notification_service: notification_service.clone(),
            file_service: file_service_dyn.clone(),
            product_service: self.product.clone(),
        }));

        let product_service: Rc<dyn IProductService> = self.product.clone();
        guard.add_rc(ExtensionRecommendationsService::new(ExtensionRecommendationsServiceParts {
            lifecycle_service: self.lifecycle.clone(),
            gallery_service: self.gallery.clone(),
            telemetry_service: Rc::new(NullTelemetryServiceShape),
            environment_service: self.environment.borrow().clone(),
            extension_management_service: self.base.management.clone(),
            extension_recommendations_management_service: ignored_service,
            extension_recommendation_notification_service: notification_service,
            extensions_workbench_service: self.workbench_service.clone(),
            remote_extensions_scanner_service: None,
            user_data_initialization_service: None,
            extension_tips_service: tips_service,
            workspace_context_service: workspace_service,
            workspace_extensions_config_service: config_service,
            uri_identity_service: self.uri_identity_service.clone(),
            file_service: file_service_dyn,
            notification_service: self.notification.clone(),
            product_service,
            extension_management_server_service: server_service,
            model_service: self.model_service.clone(),
            language_service: self.language_service.clone(),
            storage_service: Rc::new(self.base.storage.clone()),
            untitled_text_editor_service: self.untitled_service.clone(),
        }))
    }
}

impl ToggleGallery {
    /// A galeria do `setup` com `isEnabled` verdadeiro: `instantiationService.stub(IExtensionGalleryService, 'isEnabled', true)`.
    pub fn new_over(base: &WorkbenchFixture) -> Rc<Self> {
        Self::new(base.gallery.clone())
    }
}

/// O corpo de um caso: o `setup`, o teste e o `teardown` (`disposableStore.dispose(); await timeout(0);`).
async fn run_case<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<RecommendationsFixture>) -> F) {
    let fixture = RecommendationsFixture::new();
    body(fixture.clone()).await;
    fixture.base.guard.store().dispose();
    timeout(0).await;
    drop(fixture);
}

/// `runWithFakedTimers<void>({ useFakeTimers: true }, async () => ...)` com o `setup` e o `teardown` da suíte.
pub fn run<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<RecommendationsFixture>) -> F + 'static) {
    run_with_faked_timers(run_case(body));
}

/// O caso sem `runWithFakedTimers` (o `Date.now()` é o do sistema): o laço roda as microtarefas e só os timers já vencidos,
/// como o mocha que espera uma promessa, e os timers de horas do `ExtensionsWorkbenchService` nunca vencem dentro do caso.
pub fn run_real_clock<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<RecommendationsFixture>) -> F + 'static) {
    const MAX_TURNS: usize = 100_000;
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    je_base::runtime::spawn_local(async move {
        run_case(body).await;
        finished.set(true);
    });
    for _ in 0..MAX_TURNS {
        je_base::runtime::run_microtasks();
        if done.get() {
            break;
        }
        je_base::runtime::turn();
    }
    fail_on_unexpected_errors();
    assert!(done.get(), "o teste assíncrono não terminou");
}

/// `Object.keys(testObject.getAllRecommendationsWithReason())`.
pub fn keys_of<T>(dictionary: &IndexMap<String, T>) -> Vec<String> {
    dictionary.keys().cloned().collect()
}
