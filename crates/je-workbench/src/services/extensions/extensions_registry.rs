//! O pedaço de `src/vs/workbench/services/extensions/common/extensionsRegistry.ts` de que o
//! `extensionManifestPropertiesService` precisa: o registro dos pontos de extensão (`ExtensionsRegistry`), com o
//! nome, o `defaultExtensionKind` e o `canHandleResolver` de cada um.
//!
//! Divergências:
//! - O original acopla ao registro o esquema JSON do `package.json` (`schema`, `schemaRegistry`), o gerador de
//!   eventos de ativação implícitos, o tratador e os usuários de cada ponto (`setHandler`, `acceptUsers`,
//!   `ExtensionPointUserDelta`, `ExtensionMessageCollector`). Nada disso é lido pelas propriedades do manifesto;
//!   entra quando o host de extensões for portado, junto com o `jsonSchema` do descritor.
//! - O `Registry.as(PRExtensions.ExtensionsRegistry)` global é um `thread_local`, como o registro de
//!   configuração ([`je_platform::configuration::registry::configuration_registry`]).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_extensions::manifest::ExtensionKind;

/// `ExtensionPoint` (`IExtensionPoint`): a parte que identifica o ponto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionPoint {
    pub name: String,
    pub default_extension_kind: Option<Vec<ExtensionKind>>,
    pub can_handle_resolver: Option<bool>,
}

/// `IExtensionPointDescriptor`, sem o esquema e o gerador de eventos de ativação.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionPointDescriptor {
    pub extension_point: String,
    pub default_extension_kind: Option<Vec<ExtensionKind>>,
    pub can_handle_resolver: Option<bool>,
}

/// `ExtensionsRegistryImpl`: os pontos ficam na ordem em que foram registrados (a ordem do `Map` do original).
#[derive(Default)]
pub struct ExtensionsRegistryImpl {
    extension_points: RefCell<Vec<Rc<ExtensionPoint>>>,
}

impl ExtensionsRegistryImpl {
    pub fn new() -> Self {
        Self::default()
    }

    /// `registerExtensionPoint`: um nome repetido lança, como o original.
    pub fn register_extension_point(&self, desc: ExtensionPointDescriptor) -> Rc<ExtensionPoint> {
        if self.extension_points.borrow().iter().any(|point| point.name == desc.extension_point) {
            errors::throw(Error::new(format!("Duplicate extension point: {}", desc.extension_point)));
        }
        let result = Rc::new(ExtensionPoint {
            name: desc.extension_point,
            default_extension_kind: desc.default_extension_kind,
            can_handle_resolver: desc.can_handle_resolver,
        });
        self.extension_points.borrow_mut().push(result.clone());
        result
    }

    /// `getExtensionPoints`.
    pub fn get_extension_points(&self) -> Vec<Rc<ExtensionPoint>> {
        self.extension_points.borrow().clone()
    }
}

thread_local! {
    static EXTENSIONS_REGISTRY: Rc<ExtensionsRegistryImpl> = Rc::new(ExtensionsRegistryImpl::new());
}

/// `ExtensionsRegistry`.
pub fn extensions_registry() -> Rc<ExtensionsRegistryImpl> {
    EXTENSIONS_REGISTRY.with(Rc::clone)
}
