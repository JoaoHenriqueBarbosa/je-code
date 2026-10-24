//! Porte de `src/vs/workbench/contrib/extensions/browser/workspaceRecommendations.ts`: as recomendações do
//! `.vscode/extensions.json` e as extensões de workspace (`.vscode/extensions/*`) das pastas abertas.
//!
//! Nota de tradução: o `IWorkbenchExtensionManagementService.getExtensions(locations)` do original é o
//! `get_resource_extensions` do [`IWorkbenchExtensionInstallService`].

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::async_::RunOnceScheduler;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::management::is_extension_identifier;
use je_platform::files::files::FileChangeType;
use je_platform::files::i_file_service::IFileService;
use je_platform::notification::{INotificationService, NotificationMessage};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;

use super::extension_recommendations::{Activation, ExtensionRecommendation, ExtensionRecommendations};
use crate::services::extension_management::extension_install_service::IWorkbenchExtensionInstallService;
use crate::services::extension_recommendations::extension_recommendations::{ExtensionRecommendationReason, IExtensionRecommendationReason};
use crate::services::extension_recommendations::workspace_extensions_config::{IExtensionsConfigContent, IWorkspaceExtensionsConfigService};

const WORKSPACE_EXTENSIONS_FOLDER: &str = ".vscode/extensions";

/// Os serviços que o construtor do original recebe por decorador.
pub struct WorkspaceRecommendationsParts {
    pub workspace_extensions_config_service: Rc<dyn IWorkspaceExtensionsConfigService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub file_service: Rc<dyn IFileService>,
    pub workbench_extension_management_service: Rc<dyn IWorkbenchExtensionInstallService>,
    pub notification_service: Rc<dyn INotificationService>,
}

fn workspace_reason() -> IExtensionRecommendationReason {
    IExtensionRecommendationReason {
        reason_id: ExtensionRecommendationReason::Workspace,
        reason_text: localize("workspaceRecommendation", "This extension is recommended by users of the current workspace.", &[]),
    }
}

/// `WorkspaceRecommendations`.
pub struct WorkspaceRecommendations {
    this: Weak<WorkspaceRecommendations>,
    base: Rc<Disposable>,
    activation: Activation,
    recommendations: RefCell<Vec<ExtensionRecommendation>>,
    on_did_change_recommendations: Rc<Emitter<()>>,
    ignored_recommendations: RefCell<Vec<String>>,
    workspace_extensions: RefCell<Vec<URI>>,
    on_did_change_workspace_extensions_scheduler: Rc<RunOnceScheduler>,
    p: WorkspaceRecommendationsParts,
}

impl WorkspaceRecommendations {
    /// `constructor(...)`.
    pub fn new(parts: WorkspaceRecommendationsParts) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_recommendations = Emitter::<()>::new();
        base.register(on_did_change_recommendations.clone());
        Rc::new_cyclic(|weak: &Weak<WorkspaceRecommendations>| {
            let scheduler_weak = weak.clone();
            let scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(this) = scheduler_weak.upgrade() {
                        let _ = this.on_did_change_workspace_extensions_folders();
                    }
                },
                1000,
            );
            base.register(scheduler.clone());
            Self {
                this: weak.clone(),
                base: base.clone(),
                activation: Activation::default(),
                recommendations: RefCell::new(Vec::new()),
                on_did_change_recommendations,
                ignored_recommendations: RefCell::new(Vec::new()),
                workspace_extensions: RefCell::new(Vec::new()),
                on_did_change_workspace_extensions_scheduler: scheduler,
                p: parts,
            }
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o recomendador está vivo enquanto é usado")
    }

    /// `onDidChangeRecommendations`.
    pub fn on_did_change_recommendations(&self) -> Event<()> {
        self.on_did_change_recommendations.event()
    }

    /// `get ignoredRecommendations()`.
    pub fn ignored_recommendations(&self) -> Vec<String> {
        self.ignored_recommendations.borrow().clone()
    }

    /// `onDidChangeWorkspaceExtensionsFolders()`.
    fn on_did_change_workspace_extensions_folders(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let existing = this.workspace_extensions.borrow().clone();
            let fetched = this.fetch_workspace_extensions().await?;
            *this.workspace_extensions.borrow_mut() = fetched.clone();
            let ext_uri = this.p.uri_identity_service.ext_uri();
            let equal = existing.len() == fetched.len() && existing.iter().zip(fetched.iter()).all(|(a, b)| ext_uri.is_equal(Some(a), Some(b), false));
            if !equal {
                this.on_did_change_extensions_configs().await?;
            }
            Ok(())
        })
    }

    /// `fetchWorkspaceExtensions()`.
    fn fetch_workspace_extensions(&self) -> Promise<Vec<URI>> {
        let this = self.rc();
        async_fn(async move {
            let mut workspace_extensions: Vec<URI> = Vec::new();
            let ext_uri = this.p.uri_identity_service.ext_uri();
            for workspace_folder in this.p.context_service.get_workspace().folders() {
                let extensions_location = ext_uri.join_path(&workspace_folder.uri, &[WORKSPACE_EXTENSIONS_FOLDER]);
                // `try { ... } catch (error) { // ignore }`
                if let Ok(stat) = this.p.file_service.resolve(&extensions_location, None).await {
                    for extension in stat.children.unwrap_or_default() {
                        if !extension.is_directory {
                            continue;
                        }
                        workspace_extensions.push(extension.resource);
                    }
                }
            }
            if !workspace_extensions.is_empty() {
                let resource_extensions = this.p.workbench_extension_management_service.get_resource_extensions(workspace_extensions).await?;
                return Ok(resource_extensions.into_iter().map(|extension| extension.location).collect());
            }
            Ok(Vec::new())
        })
    }

    /// Parse all extensions.json files, fetch workspace recommendations, filter out invalid and unwanted ones.
    fn fetch(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let extensions_configs = this.p.workspace_extensions_config_service.get_extensions_configs().await?;

            let (invalid_recommendations, message) = Self::validate_extensions(&extensions_configs);
            if !invalid_recommendations.is_empty() {
                this.p.notification_service.warn(vec![NotificationMessage::Text(format!(
                    "The {} extension(s) below, in workspace recommendations have issues:\n{}",
                    invalid_recommendations.len(),
                    message
                ))]);
            }

            let mut recommendations: Vec<ExtensionRecommendation> = Vec::new();
            let mut ignored_recommendations: Vec<String> = Vec::new();

            for extensions_config in &extensions_configs {
                for unwanted_recommendation in extensions_config.unwanted_recommendations.iter().flatten() {
                    if !invalid_recommendations.contains(unwanted_recommendation) {
                        ignored_recommendations.push(unwanted_recommendation.clone());
                    }
                }
                for extension_id in extensions_config.recommendations.iter().flatten() {
                    if !invalid_recommendations.contains(extension_id) {
                        recommendations.push(ExtensionRecommendation::gallery(extension_id.clone(), workspace_reason()));
                    }
                }
            }

            for extension in this.workspace_extensions.borrow().iter() {
                recommendations.push(ExtensionRecommendation { extension: je_platform::extension_recommendations::extension_recommendations::RecommendedExtension::Resource(extension.clone()), reason: workspace_reason() });
            }

            *this.recommendations.borrow_mut() = recommendations;
            *this.ignored_recommendations.borrow_mut() = ignored_recommendations;
            Ok(())
        })
    }

    /// `validateExtensions(contents)`: os identificadores inválidos e a mensagem sobre eles.
    fn validate_extensions(contents: &[IExtensionsConfigContent]) -> (Vec<String>, String) {
        let mut invalid_extensions: Vec<String> = Vec::new();
        let mut message = String::new();

        let mut all_recommendations: Vec<&String> = Vec::new();
        for recommendation in contents.iter().flat_map(|content| content.recommendations.iter().flatten()) {
            if !all_recommendations.contains(&recommendation) {
                all_recommendations.push(recommendation);
            }
        }
        for extension_id in all_recommendations {
            if !is_extension_identifier(extension_id) {
                invalid_extensions.push(extension_id.clone());
                message.push_str(&format!("{extension_id} (bad format) Expected: <provider>.<name>\n"));
            }
        }
        (invalid_extensions, message)
    }

    /// `onDidChangeExtensionsConfigs()`.
    fn on_did_change_extensions_configs(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.fetch().await?;
            this.on_did_change_recommendations.fire(());
            Ok(())
        })
    }
}

impl ExtensionRecommendations for WorkspaceRecommendations {
    fn recommendations(&self) -> Vec<ExtensionRecommendation> {
        self.recommendations.borrow().clone()
    }

    fn activation(&self) -> &Activation {
        &self.activation
    }

    fn do_activate(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let fetched = this.fetch_workspace_extensions().await?;
            *this.workspace_extensions.borrow_mut() = fetched;
            this.fetch().await?;

            let weak = Rc::downgrade(&this);
            this.base.register(this.p.workspace_extensions_config_service.on_did_change_extensions_configs().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    let _ = this.on_did_change_extensions_configs();
                }
            }));
            let ext_uri = this.p.uri_identity_service.ext_uri();
            for folder in this.p.context_service.get_workspace().folders() {
                this.base.register(this.p.file_service.watch(&ext_uri.join_path(&folder.uri, &[WORKSPACE_EXTENSIONS_FOLDER]), None));
            }

            let weak = Rc::downgrade(&this);
            this.base.register(this.p.context_service.on_did_change_workspace_folders().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_workspace_extensions_scheduler.schedule(None);
                }
            }));

            let weak = Rc::downgrade(&this);
            this.base.register(this.p.file_service.on_did_files_change().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                let ext_uri = this.p.uri_identity_service.ext_uri();
                if this
                    .p
                    .context_service
                    .get_workspace()
                    .folders()
                    .iter()
                    .any(|folder| e.affects(&ext_uri.join_path(&folder.uri, &[WORKSPACE_EXTENSIONS_FOLDER]), &[FileChangeType::Added, FileChangeType::Deleted]))
                {
                    this.on_did_change_workspace_extensions_scheduler.schedule(None);
                }
            }));
            Ok(())
        })
    }
}

impl IDisposable for WorkspaceRecommendations {
    fn dispose(&self) {
        self.base.dispose();
    }
}
