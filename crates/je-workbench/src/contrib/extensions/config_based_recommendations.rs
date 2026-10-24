//! Porte de `src/vs/workbench/contrib/extensions/browser/configBasedRecommendations.ts`: as recomendações das dicas por
//! configuração das pastas do workspace.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::extension_management::extension_tips::{ConfigBasedExtensionTip, IExtensionTipsService};
use je_platform::workspace::{IWorkspaceContextService, WorkspaceFoldersChangeEvent};

use super::extension_recommendations::{Activation, ExtensionRecommendation, ExtensionRecommendations};
use crate::services::extension_recommendations::extension_recommendations::{ExtensionRecommendationReason, IExtensionRecommendationReason};

/// `ConfigBasedExtensionRecommendation` (`ExtensionRecommendation & { whenNotInstalled: string[] | undefined }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigBasedExtensionRecommendation {
    pub recommendation: ExtensionRecommendation,
    pub when_not_installed: Option<Vec<String>>,
}

/// `ConfigBasedRecommendations`.
pub struct ConfigBasedRecommendations {
    this: Weak<ConfigBasedRecommendations>,
    base: Rc<Disposable>,
    activation: Activation,
    important_tips: RefCell<Vec<ConfigBasedExtensionTip>>,
    other_tips: RefCell<Vec<ConfigBasedExtensionTip>>,
    on_did_change_recommendations: Rc<Emitter<()>>,
    other_recommendations: RefCell<Vec<ConfigBasedExtensionRecommendation>>,
    important_recommendations: RefCell<Vec<ConfigBasedExtensionRecommendation>>,
    extension_tips_service: Rc<dyn IExtensionTipsService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

impl ConfigBasedRecommendations {
    /// `constructor(extensionTipsService, workspaceContextService)`.
    pub fn new(extension_tips_service: Rc<dyn IExtensionTipsService>, workspace_context_service: Rc<dyn IWorkspaceContextService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_recommendations = Emitter::<()>::new();
        base.register(on_did_change_recommendations.clone());
        Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base,
            activation: Activation::default(),
            important_tips: RefCell::new(Vec::new()),
            other_tips: RefCell::new(Vec::new()),
            on_did_change_recommendations,
            other_recommendations: RefCell::new(Vec::new()),
            important_recommendations: RefCell::new(Vec::new()),
            extension_tips_service,
            workspace_context_service,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o recomendador está vivo enquanto é usado")
    }

    /// `onDidChangeRecommendations`.
    pub fn on_did_change_recommendations(&self) -> Event<()> {
        self.on_did_change_recommendations.event()
    }

    /// `get otherRecommendations()`.
    pub fn other_recommendations(&self) -> Vec<ConfigBasedExtensionRecommendation> {
        self.other_recommendations.borrow().clone()
    }

    /// `get importantRecommendations()`.
    pub fn important_recommendations(&self) -> Vec<ConfigBasedExtensionRecommendation> {
        self.important_recommendations.borrow().clone()
    }

    /// `fetch()`.
    fn fetch(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let workspace = this.workspace_context_service.get_workspace();
            let mut important_tips: Vec<ConfigBasedExtensionTip> = Vec::new();
            let mut other_tips: Vec<ConfigBasedExtensionTip> = Vec::new();
            // Os `Map<string, tip>` do original: a chave repetida troca o valor e mantém a posição.
            let upsert = |tips: &mut Vec<ConfigBasedExtensionTip>, tip: ConfigBasedExtensionTip| match tips.iter().position(|existing| existing.extension_id == tip.extension_id) {
                Some(index) => tips[index] = tip,
                None => tips.push(tip),
            };
            for folder in workspace.folders() {
                let config_based_tips = this.extension_tips_service.get_config_based_tips(&folder.uri).await?;
                for tip in config_based_tips {
                    if tip.important {
                        upsert(&mut important_tips, tip);
                    } else {
                        upsert(&mut other_tips, tip);
                    }
                }
            }
            let other_tips: Vec<ConfigBasedExtensionTip> =
                other_tips.into_iter().filter(|tip| !important_tips.iter().any(|important| important.extension_id == tip.extension_id)).collect();
            *this.other_recommendations.borrow_mut() = other_tips.iter().map(Self::to_extension_recommendation).collect();
            *this.important_recommendations.borrow_mut() = important_tips.iter().map(Self::to_extension_recommendation).collect();
            *this.important_tips.borrow_mut() = important_tips;
            *this.other_tips.borrow_mut() = other_tips;
            Ok(())
        })
    }

    /// `onWorkspaceFoldersChanged(event)`.
    fn on_workspace_folders_changed(&self, event: WorkspaceFoldersChangeEvent) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if !event.added.is_empty() {
                let old_important_recommended = this.important_tips.borrow().clone();
                this.fetch().await?;
                // Suggest only if at least one of the newly added recommendations was not suggested before.
                let has_new = this.important_tips.borrow().iter().any(|current| old_important_recommended.iter().all(|old| current.extension_id != old.extension_id));
                if has_new {
                    this.on_did_change_recommendations.fire(());
                }
            }
            Ok(())
        })
    }

    /// `toExtensionRecommendation(tip)`.
    fn to_extension_recommendation(tip: &ConfigBasedExtensionTip) -> ConfigBasedExtensionRecommendation {
        ConfigBasedExtensionRecommendation {
            recommendation: ExtensionRecommendation::gallery(
                tip.extension_id.clone(),
                IExtensionRecommendationReason {
                    reason_id: ExtensionRecommendationReason::WorkspaceConfig,
                    reason_text: localize("exeBasedRecommendation", "This extension is recommended because of the current workspace configuration", &[]),
                },
            ),
            when_not_installed: tip.when_not_installed.clone(),
        }
    }
}

impl ExtensionRecommendations for ConfigBasedRecommendations {
    fn recommendations(&self) -> Vec<ExtensionRecommendation> {
        let mut all = self.important_recommendations();
        all.extend(self.other_recommendations());
        all.into_iter().map(|recommendation| recommendation.recommendation).collect()
    }

    fn activation(&self) -> &Activation {
        &self.activation
    }

    fn do_activate(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.fetch().await?;
            let weak = Rc::downgrade(&this);
            this.base.register(this.workspace_context_service.on_did_change_workspace_folders().listen(move |event| {
                if let Some(this) = weak.upgrade() {
                    let _ = this.on_workspace_folders_changed(event.clone());
                }
            }));
            Ok(())
        })
    }
}

impl IDisposable for ConfigBasedRecommendations {
    fn dispose(&self) {
        self.base.dispose();
    }
}
