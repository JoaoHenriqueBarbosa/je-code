//! Porte do registro de `src/vs/workbench/services/extensionManagement/common/extensionFeatures.ts`: o
//! `IExtensionFeatureDescriptor` e o `ExtensionFeaturesRegistry`, de onde a consulta de extensões
//! (`contrib/extensions/common/extensionQuery.ts`) lê os ids dos recursos de `@contribute:`.
//!
//! Divergências:
//! - O `renderer` (`SyncDescriptor<IExtensionFeatureRenderer>`) do descritor fica de fora, junto com os tipos de
//!   renderização (`IExtensionFeatureMarkdownRenderer`, `IExtensionFeatureTableRenderer`, `IRowData`,
//!   `IRenderedData`): eles dependem de `IMarkdownString`, `ResolvedKeybinding` e `Color` desenhados na UI de
//!   extensões. Entram com o editor de extensões, que é quem os lê.
//! - `IExtensionFeaturesManagementService` e `IExtensionFeatureAccessData` também ficam para o serviço de
//!   gerenciamento (não são lidos pelo registro).
//! - `IStringDictionary<boolean>` de `access.extensionsList` é uma lista de pares que guarda a ordem de inserção.
//! - O `Registry.add(Extensions.ExtensionFeaturesRegistry, ...)` global é um `thread_local`, como o registro de
//!   configuração ([`je_platform::configuration::registry::configuration_registry`]) e o de pontos de extensão.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::themables::ThemeIcon;

/// `Extensions.ExtensionFeaturesRegistry`: a chave do registro no `Registry` do original.
pub const EXTENSION_FEATURES_REGISTRY_ID: &str = "workbench.registry.extensionFeatures";

/// O campo `access` de `IExtensionFeatureDescriptor`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionFeatureAccess {
    pub can_toggle: Option<bool>,
    pub require_user_consent: Option<bool>,
    pub extensions_list: Option<Vec<(String, bool)>>,
}

/// `IExtensionFeatureDescriptor`, sem o `renderer`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionFeatureDescriptor {
    pub id: String,
    pub label: String,
    /// Rótulo dos dados de acesso, se for diferente do título do recurso. Aparece no gráfico de acesso como
    /// "There were ${accessCount} ${accessLabel} requests from this extension".
    pub access_data_label: Option<String>,
    pub description: Option<String>,
    pub icon: Option<ThemeIcon>,
    pub access: ExtensionFeatureAccess,
}

/// `ExtensionFeaturesRegistry`: os recursos ficam na ordem em que foram registrados (a ordem do `Map` do original).
#[derive(Default)]
pub struct ExtensionFeaturesRegistry {
    extension_features: RefCell<Vec<Rc<ExtensionFeatureDescriptor>>>,
}

impl ExtensionFeaturesRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// `registerExtensionFeature`: um id repetido lança, como o original. O disposable retirado do registro
    /// remove o recurso pelo id.
    pub fn register_extension_feature(self: &Rc<Self>, descriptor: ExtensionFeatureDescriptor) -> DisposableRef {
        if self.extension_features.borrow().iter().any(|feature| feature.id == descriptor.id) {
            errors::throw(Error::new(format!("Extension feature with id '{}' already exists", descriptor.id)));
        }
        let id = descriptor.id.clone();
        self.extension_features.borrow_mut().push(Rc::new(descriptor));
        let registry = Rc::clone(self);
        to_disposable(move || registry.extension_features.borrow_mut().retain(|feature| feature.id != id))
    }

    /// `getExtensionFeature`.
    pub fn get_extension_feature(&self, id: &str) -> Option<Rc<ExtensionFeatureDescriptor>> {
        self.extension_features.borrow().iter().find(|feature| feature.id == id).cloned()
    }

    /// `getExtensionFeatures`.
    pub fn get_extension_features(&self) -> Vec<Rc<ExtensionFeatureDescriptor>> {
        self.extension_features.borrow().clone()
    }
}

thread_local! {
    static EXTENSION_FEATURES_REGISTRY: Rc<ExtensionFeaturesRegistry> = Rc::new(ExtensionFeaturesRegistry::new());
}

/// `Registry.as<IExtensionFeaturesRegistry>(Extensions.ExtensionFeaturesRegistry)`.
pub fn extension_features_registry() -> Rc<ExtensionFeaturesRegistry> {
    EXTENSION_FEATURES_REGISTRY.with(Rc::clone)
}
