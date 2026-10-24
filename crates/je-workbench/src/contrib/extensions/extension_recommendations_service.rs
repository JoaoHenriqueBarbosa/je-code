//! Porte de `src/vs/workbench/contrib/extensions/browser/extensionRecommendationsService.ts`: o serviço que reúne os
//! recomendadores (workspace, arquivo, configuração, executável, mapa de teclas, idioma, web e remoto) e responde as
//! consultas de recomendação.
//!
//! Notas de tradução:
//! - **Injeção de dependência.** Os serviços do construtor e os dos recomendadores que ele cria
//!   (`instantiationService.createInstance(...)`) entram agrupados em [`ExtensionRecommendationsServiceParts`].
//! - **`IRemoteExtensionsScannerService` e `IUserDataInitializationService`.** Só `whenExtensionsReady` e
//!   `whenInitializationFinished` são usados; os serviços completos ainda não têm porte, então os contratos reduzidos
//!   estão neste arquivo. `None` é o serviço não registrado (o `TestInstantiationService` entrega `undefined`): a chamada
//!   lança `TypeError` dentro do `try` de `activate`, o `catch` a engole e nada é esperado (nem o `lifecycleService.when`).
//! - **`activationPromise`** é a promessa que `activate()` devolve; como `activate` precisa do serviço já montado, ela é
//!   guardada depois da construção ([`ExtensionRecommendationsService::activation_promise`]).
//! - **Telemetria de instalação por recomendação.** A `GalleryExtension` do porte não carrega o `telemetryData`
//!   (`extensionGallery:install:recommendations` espalha `...e.source.telemetryData`); o evento leva `id` e `name`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::ITelemetryData;
use je_base::arrays::shuffle;
use je_base::async_::{async_fn, timeout};
use je_base::dynamic::Dynamic;
use je_base::event::{self, Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::product::IStringDictionary;
use je_base::promise::{CancelablePromise, Promise};
use je_extensions::management::InstallOperation;
use je_extensions::manifest::ExtensionIdentifierInfo;
use je_extensions::util::are_same_extensions;
use je_platform::environment::environment::EnvironmentService;
use je_platform::extension_management::extension_tips::IExtensionTipsService;
use je_platform::extension_recommendations::extension_recommendations::{IExtensionRecommendationNotificationService, RecommendedExtension};
use je_platform::files::i_file_service::IFileService;
use je_platform::notification::INotificationService;
use je_platform::product::IProductService;
use je_platform::storage::StorageService;
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;

use super::config_based_recommendations::ConfigBasedRecommendations;
use super::exe_based_recommendations::ExeBasedRecommendations;
use super::extension_recommendations::{ExtensionRecommendation, ExtensionRecommendations};
use super::extensions::IExtension;
use super::extensions_service::ExtensionsWorkbenchService;
use super::file_based_recommendations::{FileBasedRecommendations, FileBasedRecommendationsParts};
use super::keymap_recommendations::{KeymapRecommendations, new_keymap_recommendations};
use super::language_recommendations::{LanguageRecommendations, new_language_recommendations};
use super::remote_recommendations::RemoteRecommendations;
use super::web_recommendations::WebRecommendations;
use super::workspace_recommendations::{WorkspaceRecommendations, WorkspaceRecommendationsParts};
use crate::services::extension_management::extension_gallery_service::IExtensionGalleryService;
use crate::services::extension_management::extension_install_service::IWorkbenchExtensionInstallService;
use crate::services::extension_management::extension_management::{IExtensionManagementServerService, InstallExtensionResult, InstallExtensionSource};
use crate::services::extension_recommendations::extension_recommendations::{
    IExtensionIgnoredRecommendationsService, IExtensionRecommendationReason, IExtensionRecommendationsService, ImportantAndOthers,
};
use crate::services::extension_recommendations::workspace_extensions_config::IWorkspaceExtensionsConfigService;
use crate::services::lifecycle::lifecycle::{ILifecycleService, LifecyclePhase};
use crate::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;

/// `IRemoteExtensionsScannerService` (`platform/remote/common/remoteExtensionsScanner.ts`), reduzido a `whenExtensionsReady`.
pub trait IRemoteExtensionsScannerService {
    fn when_extensions_ready(&self) -> Promise<()>;
}

/// `IUserDataInitializationService` (`workbench/services/userData/browser/userDataInit.ts`), reduzido a
/// `whenInitializationFinished`.
pub trait IUserDataInitializationService {
    fn when_initialization_finished(&self) -> Promise<()>;
}

/// Os serviços do construtor e os dos recomendadores que ele cria.
pub struct ExtensionRecommendationsServiceParts {
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub gallery_service: Rc<dyn IExtensionGalleryService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub extension_management_service: Rc<dyn IWorkbenchExtensionInstallService>,
    pub extension_recommendations_management_service: Rc<dyn IExtensionIgnoredRecommendationsService>,
    pub extension_recommendation_notification_service: Rc<dyn IExtensionRecommendationNotificationService>,
    pub extensions_workbench_service: Rc<ExtensionsWorkbenchService>,
    pub remote_extensions_scanner_service: Option<Rc<dyn IRemoteExtensionsScannerService>>,
    pub user_data_initialization_service: Option<Rc<dyn IUserDataInitializationService>>,

    // Os serviços dos recomendadores.
    pub extension_tips_service: Rc<dyn IExtensionTipsService>,
    pub workspace_context_service: Rc<dyn IWorkspaceContextService>,
    pub workspace_extensions_config_service: Rc<dyn IWorkspaceExtensionsConfigService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub file_service: Rc<dyn IFileService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub product_service: Rc<dyn IProductService>,
    pub extension_management_server_service: Rc<dyn IExtensionManagementServerService>,
    pub model_service: Rc<dyn je_editor::services_model::IModelService>,
    pub language_service: Rc<dyn je_editor::language_service::ILanguageService>,
    pub storage_service: Rc<dyn StorageService>,
    pub untitled_text_editor_service: Rc<dyn IUntitledTextEditorService>,
}

/// `ExtensionRecommendationsService`.
pub struct ExtensionRecommendationsService {
    this: Weak<ExtensionRecommendationsService>,
    base: Rc<Disposable>,

    // Recommendations
    file_based_recommendations: Rc<FileBasedRecommendations>,
    workspace_recommendations: Rc<WorkspaceRecommendations>,
    config_based_recommendations: Rc<ConfigBasedRecommendations>,
    exe_based_recommendations: Rc<ExeBasedRecommendations>,
    keymap_recommendations: Rc<KeymapRecommendations>,
    web_recommendations: Rc<WebRecommendations>,
    language_recommendations: Rc<LanguageRecommendations>,
    remote_recommendations: Rc<RemoteRecommendations>,

    activation_promise: RefCell<Option<Promise<()>>>,
    session_seed: Cell<f64>,

    on_did_change_recommendations: Rc<Emitter<()>>,

    p: ExtensionRecommendationsServiceParts,
}

impl ExtensionRecommendationsService {
    /// `constructor(...)`.
    pub fn new(parts: ExtensionRecommendationsServiceParts) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_recommendations = Emitter::<()>::new();
        base.register(on_did_change_recommendations.clone());

        let workspace_recommendations = WorkspaceRecommendations::new(WorkspaceRecommendationsParts {
            workspace_extensions_config_service: parts.workspace_extensions_config_service.clone(),
            context_service: parts.workspace_context_service.clone(),
            uri_identity_service: parts.uri_identity_service.clone(),
            file_service: parts.file_service.clone(),
            workbench_extension_management_service: parts.extension_management_service.clone(),
            notification_service: parts.notification_service.clone(),
        });
        base.register(workspace_recommendations.clone());
        let file_based_recommendations = FileBasedRecommendations::new(FileBasedRecommendationsParts {
            extensions_workbench_service: parts.extensions_workbench_service.clone(),
            model_service: parts.model_service.clone(),
            language_service: parts.language_service.clone(),
            product_service: parts.product_service.clone(),
            storage_service: parts.storage_service.clone(),
            extension_recommendation_notification_service: parts.extension_recommendation_notification_service.clone(),
            extension_ignored_recommendations_service: parts.extension_recommendations_management_service.clone(),
            workspace_context_service: parts.workspace_context_service.clone(),
            untitled_text_editor_service: parts.untitled_text_editor_service.clone(),
        });
        base.register(file_based_recommendations.clone());
        let config_based_recommendations = ConfigBasedRecommendations::new(parts.extension_tips_service.clone(), parts.workspace_context_service.clone());
        base.register(config_based_recommendations.clone());
        let exe_based_recommendations = ExeBasedRecommendations::new(parts.extension_tips_service.clone());
        let keymap_recommendations = new_keymap_recommendations(parts.product_service.clone());
        let web_recommendations = WebRecommendations::new(parts.product_service.clone(), parts.extension_management_server_service.clone());
        let language_recommendations = new_language_recommendations(parts.product_service.clone());
        let remote_recommendations = RemoteRecommendations::new(parts.product_service.clone());

        let enabled = parts.gallery_service.is_enabled() && !parts.environment_service.is_extension_development();
        let session_seed = if enabled { je_base::runtime::date_now() as f64 } else { 0.0 };

        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base,
            file_based_recommendations,
            workspace_recommendations,
            config_based_recommendations,
            exe_based_recommendations,
            keymap_recommendations,
            web_recommendations,
            language_recommendations,
            remote_recommendations,
            activation_promise: RefCell::new(None),
            session_seed: Cell::new(session_seed),
            on_did_change_recommendations,
            p: parts,
        });

        if !enabled {
            *this.activation_promise.borrow_mut() = Some(Promise::resolved(()));
            return this;
        }

        // Activation
        let activation = this.activate();
        *this.activation_promise.borrow_mut() = Some(activation);

        let weak = Rc::downgrade(&this);
        this.base.register(this.p.extension_management_service.on_did_install_extensions().listen(move |results| {
            if let Some(this) = weak.upgrade() {
                this.on_did_install_extensions(results);
            }
        }));
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `activationPromise`.
    pub fn activation_promise(&self) -> Promise<()> {
        self.activation_promise.borrow().clone().unwrap_or_else(|| Promise::resolved(()))
    }

    /// `activate()`.
    fn activate(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // `try { await Promise.allSettled([...]) } catch (error) { /* ignore */ }`: um serviço ausente lança antes de
            // montar o vetor, e nada é esperado.
            let remote_ready = this.p.remote_extensions_scanner_service.as_ref().map(|service| service.when_extensions_ready());
            if let (Some(remote_ready), Some(user_data_initialization_service)) = (remote_ready, this.p.user_data_initialization_service.as_ref()) {
                let initialized = user_data_initialization_service.when_initialization_finished();
                let restored = this.p.lifecycle_service.when(LifecyclePhase::Restored);
                Promise::all_settled(vec![remote_ready, initialized, restored]).await?;
            }

            // activate all recommendations
            Promise::all(vec![
                this.workspace_recommendations.activate(),
                this.config_based_recommendations.activate(),
                this.file_based_recommendations.activate(),
                this.keymap_recommendations.activate(),
                this.language_recommendations.activate(),
                this.web_recommendations.activate(),
                this.remote_recommendations.activate(),
            ])
            .await?;

            let emitter = this.on_did_change_recommendations.clone();
            this.base.register(
                event::any(vec![
                    this.workspace_recommendations.on_did_change_recommendations(),
                    this.config_based_recommendations.on_did_change_recommendations(),
                    this.p.extension_recommendations_management_service.on_did_change_ignored_recommendations(),
                ])
                .listen(move |_| emitter.fire(())),
            );

            let _ = this.prompt_workspace_recommendations();
            Ok(())
        })
    }

    /// `isEnabled()`.
    fn is_enabled(&self) -> bool {
        self.p.gallery_service.is_enabled() && !self.p.environment_service.is_extension_development()
    }

    /// `activateProactiveRecommendations()`.
    fn activate_proactive_recommendations(&self) -> Promise<()> {
        Promise::all(vec![self.exe_based_recommendations.activate(), self.config_based_recommendations.activate()]).then(|_| ())
    }

    /// `onDidInstallExtensions(results)`.
    fn on_did_install_extensions(&self, results: &[InstallExtensionResult]) {
        for e in results {
            if let (Some(InstallExtensionSource::Gallery(gallery)), true) = (&e.source, e.operation == InstallOperation::Install) {
                let ext_recommendations = self.get_all_recommendations_with_reason();
                if let Some(recommendation_reason) = ext_recommendations.get(&gallery.identifier.id.to_lowercase()) {
                    let data = ITelemetryData {
                        from: None,
                        target: None,
                        extra: vec![
                            ("id".to_owned(), Dynamic::string(&gallery.identifier.id)),
                            ("name".to_owned(), Dynamic::string(&gallery.name)),
                            ("recommendationReason".to_owned(), Dynamic::Number(recommendation_reason.reason_id as u8 as f64)),
                        ],
                    };
                    self.p.telemetry_service.public_log("extensionGallery:install:recommendations", Some(data));
                }
            }
        }
    }

    /// `toExtensionIds(recommendations)`.
    fn to_extension_ids(&self, recommendations: &[ExtensionRecommendation]) -> Vec<String> {
        let mut extension_ids: Vec<String> = Vec::new();
        for recommendation in recommendations {
            if let RecommendedExtension::Gallery(extension) = &recommendation.extension {
                if self.is_extension_allowed_to_be_recommended(extension) && !extension_ids.contains(&extension.to_lowercase()) {
                    extension_ids.push(extension.to_lowercase());
                }
            }
        }
        extension_ids
    }

    /// `isExtensionAllowedToBeRecommended(extensionId)`.
    fn is_extension_allowed_to_be_recommended(&self, extension_id: &str) -> bool {
        !self.p.extension_recommendations_management_service.ignored_recommendations().contains(&extension_id.to_lowercase())
    }

    /// `promptWorkspaceRecommendations()`.
    fn prompt_workspace_recommendations(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let installed = this.p.extensions_workbench_service.query_local(None).await?;
            let mut allowed_recommendations: Vec<RecommendedExtension> = this.workspace_recommendations.recommendations().into_iter().map(|recommendation| recommendation.extension).collect();
            for recommendation in this.config_based_recommendations.important_recommendations() {
                let still_needed = recommendation
                    .when_not_installed
                    .as_ref()
                    .is_none_or(|ids| ids.iter().all(|id| installed.iter().all(|local| !are_same_extensions(&local.identifier(), &ExtensionIdentifierInfo::new(id.clone())))));
                if still_needed {
                    allowed_recommendations.push(recommendation.recommendation.extension);
                }
            }
            let allowed_recommendations: Vec<RecommendedExtension> = allowed_recommendations
                .into_iter()
                .filter(|extension| match extension {
                    RecommendedExtension::Gallery(id) => this.is_extension_allowed_to_be_recommended(id),
                    RecommendedExtension::Resource(_) => true,
                })
                .collect();

            if !allowed_recommendations.is_empty() {
                this.register_p(timeout(5000)).await?;
                this.p.extension_recommendation_notification_service.prompt_workspace_recommendations(allowed_recommendations).await?;
            }
            Ok(())
        })
    }

    /// `_registerP(o)`.
    fn register_p<T: Clone + 'static>(&self, promise: CancelablePromise<T>) -> CancelablePromise<T> {
        let to_cancel = promise.clone();
        self.base.register(to_disposable(move || to_cancel.cancel()));
        promise
    }
}

impl IExtensionRecommendationsService for ExtensionRecommendationsService {
    fn on_did_change_recommendations(&self) -> Event<()> {
        self.on_did_change_recommendations.event()
    }

    fn get_all_recommendations_with_reason(&self) -> IStringDictionary<IExtensionRecommendationReason> {
        // Activate proactive recommendations
        let _ = self.activate_proactive_recommendations();

        let mut output: IStringDictionary<IExtensionRecommendationReason> = IStringDictionary::new();

        let mut all_recommendations: Vec<ExtensionRecommendation> = Vec::new();
        all_recommendations.extend(self.config_based_recommendations.recommendations());
        all_recommendations.extend(self.exe_based_recommendations.recommendations());
        all_recommendations.extend(self.file_based_recommendations.recommendations());
        all_recommendations.extend(self.workspace_recommendations.recommendations());
        all_recommendations.extend(self.keymap_recommendations.recommendations());
        all_recommendations.extend(self.language_recommendations.recommendations());
        all_recommendations.extend(self.web_recommendations.recommendations());

        for ExtensionRecommendation { extension, reason } in all_recommendations {
            if let RecommendedExtension::Gallery(extension) = extension {
                if self.is_extension_allowed_to_be_recommended(&extension) {
                    output.insert(extension.to_lowercase(), reason);
                }
            }
        }
        output
    }

    fn get_config_based_recommendations(&self) -> Promise<ImportantAndOthers> {
        let this = self.rc();
        async_fn(async move {
            this.config_based_recommendations.activate().await?;
            let important: Vec<ExtensionRecommendation> = this.config_based_recommendations.important_recommendations().into_iter().map(|r| r.recommendation).collect();
            let others: Vec<ExtensionRecommendation> = this.config_based_recommendations.other_recommendations().into_iter().map(|r| r.recommendation).collect();
            Ok(ImportantAndOthers { important: this.to_extension_ids(&important), others: this.to_extension_ids(&others) })
        })
    }

    fn get_other_recommendations(&self) -> Promise<Vec<String>> {
        let this = self.rc();
        async_fn(async move {
            this.activation_promise().await?;
            this.activate_proactive_recommendations().await?;

            let mut recommendations: Vec<ExtensionRecommendation> = this.config_based_recommendations.other_recommendations().into_iter().map(|r| r.recommendation).collect();
            recommendations.extend(this.exe_based_recommendations.other_recommendations());
            recommendations.extend(this.web_recommendations.recommendations());

            let mut extension_ids = this.to_extension_ids(&recommendations);
            shuffle(&mut extension_ids, Some(this.session_seed.get()));
            Ok(extension_ids)
        })
    }

    fn get_important_recommendations(&self) -> Promise<Vec<String>> {
        let this = self.rc();
        async_fn(async move {
            this.activate_proactive_recommendations().await?;

            let mut recommendations: Vec<ExtensionRecommendation> = this.file_based_recommendations.important_recommendations().into_iter().map(ExtensionRecommendation::from).collect();
            recommendations.extend(this.config_based_recommendations.important_recommendations().into_iter().map(|r| r.recommendation));
            recommendations.extend(this.exe_based_recommendations.important_recommendations());

            let mut extension_ids = this.to_extension_ids(&recommendations);
            shuffle(&mut extension_ids, Some(this.session_seed.get()));
            Ok(extension_ids)
        })
    }

    fn get_keymap_recommendations(&self) -> Vec<String> {
        self.to_extension_ids(&self.keymap_recommendations.recommendations())
    }

    fn get_language_recommendations(&self) -> Vec<String> {
        self.to_extension_ids(&self.language_recommendations.recommendations())
    }

    fn get_remote_recommendations(&self) -> Vec<String> {
        self.to_extension_ids(&self.remote_recommendations.recommendations())
    }

    fn get_workspace_recommendations(&self) -> Promise<Vec<RecommendedExtension>> {
        let this = self.rc();
        async_fn(async move {
            if !this.is_enabled() {
                return Ok(Vec::new());
            }
            this.workspace_recommendations.activate().await?;
            let mut result: Vec<RecommendedExtension> = Vec::new();
            for ExtensionRecommendation { extension, .. } in this.workspace_recommendations.recommendations() {
                match extension {
                    RecommendedExtension::Gallery(id) => {
                        let lower = id.to_lowercase();
                        if !result.contains(&RecommendedExtension::Gallery(lower.clone())) && this.is_extension_allowed_to_be_recommended(&id) {
                            result.push(RecommendedExtension::Gallery(lower));
                        }
                    }
                    resource => result.push(resource),
                }
            }
            Ok(result)
        })
    }

    fn get_exe_based_recommendations(&self, exe: Option<&str>) -> Promise<ImportantAndOthers> {
        let this = self.rc();
        let exe = exe.map(str::to_owned);
        async_fn(async move {
            this.exe_based_recommendations.activate().await?;
            let (important, others) = match exe.as_deref().filter(|exe| !exe.is_empty()) {
                Some(exe) => {
                    let found = this.exe_based_recommendations.get_recommendations(exe);
                    (found.important, found.others)
                }
                None => (this.exe_based_recommendations.important_recommendations(), this.exe_based_recommendations.other_recommendations()),
            };
            Ok(ImportantAndOthers { important: this.to_extension_ids(&important), others: this.to_extension_ids(&others) })
        })
    }

    fn get_file_based_recommendations(&self) -> Vec<String> {
        self.to_extension_ids(&self.file_based_recommendations.recommendations())
    }
}

impl IDisposable for ExtensionRecommendationsService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
