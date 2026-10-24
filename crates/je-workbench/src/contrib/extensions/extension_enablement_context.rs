//! Porte de `src/vs/workbench/contrib/extensions/browser/extensionEnablementContext.ts`: as chaves de contexto
//! `extensionEnabled:<id>` que espelham o conjunto de extensões instaladas e habilitadas.
//!
//! Divergências:
//! - `ExtensionEnablementContextKeysContribution extends Disposable` guarda um [`Disposable`] (como as demais contribuições
//!   do porte). `IWorkbenchContribution` ainda não foi portado: o registro da contribuição fica para a montagem do workbench.
//! - Os ouvintes capturam um `Weak` da contribuição (`Rc::new_cyclic`), no lugar do `this`.
//! - `IContextKey<boolean>` é o `IContextKey` do porte, com o valor como [`Dynamic`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};

use crate::services::extensions::extensions::IExtensionService;

/// Prefix for the per-extension when-clause context keys that report whether an extension is installed and enabled. The
/// full key is `extensionEnabled:<lowercased extension id>`, e.g. `extensionEnabled:ms-python.python`.
///
/// Because context keys are matched by exact string, the extension id is always lowercased. When clauses must therefore
/// use the lowercased id (for example `extensionEnabled:github.copilot`, not `extensionEnabled:GitHub.copilot`).
pub const EXTENSION_ENABLED_CONTEXT_KEY_PREFIX: &str = "extensionEnabled:";

/// Mirrors the set of installed and enabled extensions into context keys so that `when` clauses can gate on the presence
/// of another extension without having to activate it. A key `extensionEnabled:<id>` is `true` while the extension is
/// registered (installed and enabled) and `false` once it is removed (uninstalled or disabled).
pub struct ExtensionEnablementContextKeysContribution {
    base: Rc<Disposable>,
    context_keys: RefCell<HashMap<String, Rc<dyn IContextKey>>>,
    context_key_service: Rc<dyn IContextKeyService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl ExtensionEnablementContextKeysContribution {
    pub fn new(context_key_service: Rc<dyn IContextKeyService>, extension_service: Rc<dyn IExtensionService>) -> Rc<Self> {
        let this = Rc::new_cyclic(|_| Self {
            base: Disposable::new(),
            context_keys: RefCell::new(HashMap::new()),
            context_key_service,
            extension_service: extension_service.clone(),
        });

        // Reconcile against the authoritative set of registered extensions rather than trusting individual change deltas.
        // `IExtensionService.extensions` is empty until the initial registration completes (signalled by
        // `onDidRegisterExtensions`, which is not guaranteed to have fired by the `Restored` phase), and change deltas can
        // also report `added` extensions that registry validation then rejects (e.g. dependency loops) without a matching
        // `removed` entry. Recomputing from the final collection keeps the keys correct in both cases.
        this.reconcile();
        let weak = Rc::downgrade(&this);
        this.base.register(extension_service.on_did_register_extensions().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.reconcile();
                }
            }
        }));
        this.base.register(extension_service.on_did_change_extensions().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.reconcile();
            }
        }));
        this
    }

    fn reconcile(&self) {
        let enabled_keys: HashSet<String> = self
            .extension_service
            .extensions()
            .iter()
            .map(|extension| format!("{EXTENSION_ENABLED_CONTEXT_KEY_PREFIX}{}", ExtensionIdentifier::to_key(&extension.identifier.value)))
            .collect();

        // Update every key we already track to reflect the current registry.
        for (key, context_key) in self.context_keys.borrow().iter() {
            context_key.set(Dynamic::from(enabled_keys.contains(key)));
        }

        // Create keys for extensions that became enabled since the last reconcile.
        for key in enabled_keys {
            if !self.context_keys.borrow().contains_key(&key) {
                let context_key = self.context_key_service.create_key(&key, Dynamic::from(true));
                self.context_keys.borrow_mut().insert(key, context_key);
            }
        }
    }
}

impl IDisposable for ExtensionEnablementContextKeysContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}
