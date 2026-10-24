//! Porte de `src/vs/workbench/services/extensionRecommendations/common/extensionIgnoredRecommendationsService.ts`: as
//! recomendações ignoradas globalmente (no armazenamento do perfil) e as ignoradas pelo workspace
//! (`unwantedRecommendations` do `.vscode/extensions.json`).
//!
//! Nota de tradução: o `async` de `initIgnoredWorkspaceRecommendations` é uma promessa solta (`async_fn`), como a chamada
//! sem `await` do construtor do original.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use serde_json::Value;

use super::extension_recommendations::{IExtensionIgnoredRecommendationsService, IgnoredRecommendationChangeNotification};
use super::workspace_extensions_config::IWorkspaceExtensionsConfigService;

const IGNORED_RECOMMENDATIONS_STORAGE_KEY: &str = "extensionsAssistant/ignored_recommendations";

/// `distinct(array)` de strings, na ordem da primeira ocorrência.
fn distinct(values: Vec<String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for value in values {
        if !result.contains(&value) {
            result.push(value);
        }
    }
    result
}

/// `ExtensionIgnoredRecommendationsService`.
pub struct ExtensionIgnoredRecommendationsService {
    this: Weak<ExtensionIgnoredRecommendationsService>,
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,
    on_did_change_ignored_recommendations: Rc<Emitter<()>>,
    on_did_change_global_ignored_recommendation: Rc<Emitter<IgnoredRecommendationChangeNotification>>,
    global_ignored_recommendations: RefCell<Vec<String>>,
    ignored_workspace_recommendations: RefCell<Vec<String>>,
    ignored_recommendations_value: RefCell<Option<String>>,
    workspace_extensions_config_service: Rc<dyn IWorkspaceExtensionsConfigService>,
    storage_service: Rc<dyn StorageService>,
}

impl ExtensionIgnoredRecommendationsService {
    /// `constructor(workspaceExtensionsConfigService, storageService)`.
    pub fn new(workspace_extensions_config_service: Rc<dyn IWorkspaceExtensionsConfigService>, storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_did_change_ignored_recommendations = Emitter::<()>::new();
        let on_did_change_global_ignored_recommendation = Emitter::<IgnoredRecommendationChangeNotification>::new();
        base.register(on_did_change_ignored_recommendations.clone());
        base.register(on_did_change_global_ignored_recommendation.clone());

        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base: base.clone(),
            store: store.clone(),
            on_did_change_ignored_recommendations,
            on_did_change_global_ignored_recommendation,
            global_ignored_recommendations: RefCell::new(Vec::new()),
            ignored_workspace_recommendations: RefCell::new(Vec::new()),
            ignored_recommendations_value: RefCell::new(None),
            workspace_extensions_config_service,
            storage_service,
        });

        *this.global_ignored_recommendations.borrow_mut() = this.get_cached_ignored_recommendations();
        let weak = Rc::downgrade(&this);
        this.base.register(this.storage_service.on_did_change_value(StorageScope::Profile, Some(IGNORED_RECOMMENDATIONS_STORAGE_KEY), &this.store).listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_storage_change();
            }
        }));

        this.init_ignored_workspace_recommendations();
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `initIgnoredWorkspaceRecommendations()`: a promessa fica solta, como no original.
    fn init_ignored_workspace_recommendations(&self) {
        let this = self.rc();
        let _ = async_fn(async move {
            let unwanted = this.workspace_extensions_config_service.get_unwanted_recommendations().await?;
            *this.ignored_workspace_recommendations.borrow_mut() = unwanted;
            this.on_did_change_ignored_recommendations.fire(());
            let weak = Rc::downgrade(&this);
            this.base.register(this.workspace_extensions_config_service.on_did_change_extensions_configs().listen(move |_| {
                let Some(this) = weak.upgrade() else { return };
                let _ = async_fn({
                    let this = this.clone();
                    async move {
                        let unwanted = this.workspace_extensions_config_service.get_unwanted_recommendations().await?;
                        *this.ignored_workspace_recommendations.borrow_mut() = unwanted;
                        this.on_did_change_ignored_recommendations.fire(());
                        Ok(())
                    }
                });
            }));
            Ok(())
        });
    }

    /// `getCachedIgnoredRecommendations()`: `JSON.parse` do valor armazenado, em minúsculas.
    fn get_cached_ignored_recommendations(&self) -> Vec<String> {
        let value = self.ignored_recommendations_value();
        match serde_json::from_str::<Value>(&value) {
            Ok(Value::Array(items)) => items.iter().filter_map(|item| item.as_str().map(str::to_lowercase)).collect(),
            _ => Vec::new(),
        }
    }

    fn on_did_storage_change(&self) {
        if self.ignored_recommendations_value() != self.get_stored_ignored_recommendations_value() {
            // This checks if current window changed the value or not.
            *self.ignored_recommendations_value.borrow_mut() = None;
            *self.global_ignored_recommendations.borrow_mut() = self.get_cached_ignored_recommendations();
            self.on_did_change_ignored_recommendations.fire(());
        }
    }

    fn store_cached_ignored_recommendations(&self, ignored_recommendations: &[String]) {
        self.set_ignored_recommendations_value(serde_json::to_string(ignored_recommendations).unwrap_or_else(|_| "[]".to_owned()));
    }

    /// O getter `ignoredRecommendationsValue` (`!this._ignoredRecommendationsValue` também recarrega a cadeia vazia).
    fn ignored_recommendations_value(&self) -> String {
        let cached = self.ignored_recommendations_value.borrow().clone();
        match cached {
            Some(value) if !value.is_empty() => value,
            _ => {
                let value = self.get_stored_ignored_recommendations_value();
                *self.ignored_recommendations_value.borrow_mut() = Some(value.clone());
                value
            }
        }
    }

    /// O setter `ignoredRecommendationsValue`.
    fn set_ignored_recommendations_value(&self, value: String) {
        if self.ignored_recommendations_value() != value {
            *self.ignored_recommendations_value.borrow_mut() = Some(value.clone());
            self.set_stored_ignored_recommendations_value(&value);
        }
    }

    fn get_stored_ignored_recommendations_value(&self) -> String {
        self.storage_service.get(IGNORED_RECOMMENDATIONS_STORAGE_KEY, StorageScope::Profile, Some("[]")).unwrap_or_else(|| "[]".to_owned())
    }

    fn set_stored_ignored_recommendations_value(&self, value: &str) {
        self.storage_service.store(IGNORED_RECOMMENDATIONS_STORAGE_KEY, value.into(), StorageScope::Profile, StorageTarget::User);
    }
}

impl IExtensionIgnoredRecommendationsService for ExtensionIgnoredRecommendationsService {
    fn on_did_change_ignored_recommendations(&self) -> Event<()> {
        self.on_did_change_ignored_recommendations.event()
    }

    fn ignored_recommendations(&self) -> Vec<String> {
        let mut all = self.global_ignored_recommendations();
        all.extend(self.ignored_workspace_recommendations.borrow().iter().cloned());
        distinct(all)
    }

    fn on_did_change_global_ignored_recommendation(&self) -> Event<IgnoredRecommendationChangeNotification> {
        self.on_did_change_global_ignored_recommendation.event()
    }

    fn global_ignored_recommendations(&self) -> Vec<String> {
        self.global_ignored_recommendations.borrow().clone()
    }

    fn toggle_global_ignored_recommendation(&self, extension_id: &str, should_ignore: bool) {
        let extension_id = extension_id.to_lowercase();
        let ignored = self.global_ignored_recommendations.borrow().contains(&extension_id);
        if ignored == should_ignore {
            return;
        }

        let updated: Vec<String> = if should_ignore {
            let mut updated = self.global_ignored_recommendations.borrow().clone();
            updated.push(extension_id.clone());
            updated
        } else {
            self.global_ignored_recommendations.borrow().iter().filter(|id| **id != extension_id).cloned().collect()
        };
        *self.global_ignored_recommendations.borrow_mut() = updated.clone();
        self.store_cached_ignored_recommendations(&updated);
        self.on_did_change_global_ignored_recommendation.fire(IgnoredRecommendationChangeNotification { extension_id, is_recommended: !should_ignore });
        self.on_did_change_ignored_recommendations.fire(());
    }
}

impl IDisposable for ExtensionIgnoredRecommendationsService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
