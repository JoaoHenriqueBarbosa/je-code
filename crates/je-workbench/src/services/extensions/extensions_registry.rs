//! O pedaço de `src/vs/workbench/services/extensions/common/extensionsRegistry.ts` de que o
//! `extensionManifestPropertiesService` e o `AbstractExtensionService` precisam: o registro dos pontos de extensão
//! (`ExtensionsRegistry`), com o nome, o `defaultExtensionKind` e o `canHandleResolver` de cada um, o tratador de cada ponto
//! (`setHandler`, `acceptUsers`), o `ExtensionMessageCollector` e o `ExtensionPointUserDelta`.
//!
//! Divergências:
//! - O original acopla ao registro o esquema JSON do `package.json` (`schema`, `schemaRegistry`) e o gerador de eventos de
//!   ativação implícitos. Nada disso é lido pelo serviço de extensões; entra junto com o `jsonSchema` do descritor. Os tipos
//!   que os tratadores recebem ([`ExtensionMessageCollector`], [`ExtensionPointUser`], [`ExtensionPointDelta`]) já estavam
//!   aqui, porque os registros que consomem pontos de extensão (o `ProblemMatcherRegistry`) os leem.
//! - O `Registry.as(PRExtensions.ExtensionsRegistry)` global é um `thread_local`, como o registro de
//!   configuração ([`je_platform::configuration::registry::configuration_registry`]).
//! - `ExtensionPoint<T>` não é genérico: o tratador e os usuários vivem como JSON ([`serde_json::Value`]), que é o formato em
//!   que o `AbstractExtensionService` entrega as contribuições (`desc.contributes[name]`); quem tipa o valor lê o JSON.
//!   `IExtensionPointUser.description` é o [`ExtensionPointUser::description`].
//! - `ExtensionMessageCollector` (a classe) é o trait [`ExtensionMessageCollector`] e a implementação do original é a
//!   [`ExtensionMessageCollectorImpl`]. O erro que `setHandler` lança (`Handler already set!`) é um `errors::throw`, como o
//!   do `registerExtensionPoint`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_extensions::manifest::{ExtensionIdentifierSet, ExtensionKind};
use je_platform::notification::notification::Severity;
use serde_json::Value;

use super::extension_description_registry::SharedExtensionDescription;
use super::extensions::IMessage;

/// `IExtensionPointHandler<T>`.
pub type ExtensionPointHandler = Rc<dyn Fn(&[ExtensionPointUser<Value>], &ExtensionPointDelta<Value>)>;

struct PointState {
    handler: Option<ExtensionPointHandler>,
    users: Option<Vec<ExtensionPointUser<Value>>>,
    delta: Option<ExtensionPointDelta<Value>>,
}

/// `ExtensionPoint` (`IExtensionPoint`).
pub struct ExtensionPoint {
    pub name: String,
    pub default_extension_kind: Option<Vec<ExtensionKind>>,
    pub can_handle_resolver: Option<bool>,
    state: Rc<RefCell<PointState>>,
}

impl std::fmt::Debug for ExtensionPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionPoint")
            .field("name", &self.name)
            .field("default_extension_kind", &self.default_extension_kind)
            .field("can_handle_resolver", &self.can_handle_resolver)
            .finish()
    }
}

impl PartialEq for ExtensionPoint {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.default_extension_kind == other.default_extension_kind && self.can_handle_resolver == other.can_handle_resolver
    }
}

impl Eq for ExtensionPoint {}

impl ExtensionPoint {
    fn new(name: String, default_extension_kind: Option<Vec<ExtensionKind>>, can_handle_resolver: Option<bool>) -> Self {
        Self { name, default_extension_kind, can_handle_resolver, state: Rc::new(RefCell::new(PointState { handler: None, users: None, delta: None })) }
    }

    /// `setHandler(handler)`: lança `Handler already set!` se já há um tratador.
    pub fn set_handler(&self, handler: ExtensionPointHandler) -> DisposableRef {
        if self.state.borrow().handler.is_some() {
            errors::throw(Error::new("Handler already set!"));
        }
        self.state.borrow_mut().handler = Some(handler);
        Self::handle(&self.state);

        let state = self.state.clone();
        to_disposable(move || {
            state.borrow_mut().handler = None;
        })
    }

    /// `acceptUsers(users)`.
    pub fn accept_users(&self, users: Vec<ExtensionPointUser<Value>>) {
        {
            let mut state = self.state.borrow_mut();
            let delta = ExtensionPointDelta::compute(state.users.as_deref(), &users);
            state.delta = Some(delta);
            state.users = Some(users);
        }
        Self::handle(&self.state);
    }

    fn handle(state: &Rc<RefCell<PointState>>) {
        let (handler, users, delta) = {
            let state = state.borrow();
            match (&state.handler, &state.users, &state.delta) {
                (Some(handler), Some(users), Some(delta)) => (handler.clone(), users.clone(), delta.clone()),
                _ => return,
            }
        };

        if let Err(err) = errors::catch(|| handler(&users, &delta)) {
            errors::on_unexpected_error(&err);
        }
    }
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
        let result = Rc::new(ExtensionPoint::new(desc.extension_point, desc.default_extension_kind, desc.can_handle_resolver));
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

/// `ExtensionMessageCollector`: onde as mensagens de validação de uma extensão aparecem.
pub trait ExtensionMessageCollector {
    fn error(&self, message: &str);
    fn warn(&self, message: &str);
    fn info(&self, message: &str);
}

/// `new ExtensionMessageCollector(messageHandler, extension, extensionPointId)`.
pub struct ExtensionMessageCollectorImpl {
    message_handler: Rc<dyn Fn(IMessage)>,
    extension: SharedExtensionDescription,
    extension_point_id: String,
}

impl ExtensionMessageCollectorImpl {
    pub fn new(message_handler: Rc<dyn Fn(IMessage)>, extension: SharedExtensionDescription, extension_point_id: &str) -> Self {
        Self { message_handler, extension, extension_point_id: extension_point_id.to_owned() }
    }

    fn msg(&self, severity: Severity, message: &str) {
        (self.message_handler)(IMessage {
            severity,
            message: message.to_owned(),
            extension_id: self.extension.identifier.clone(),
            extension_point_id: self.extension_point_id.clone(),
        });
    }
}

impl ExtensionMessageCollector for ExtensionMessageCollectorImpl {
    fn error(&self, message: &str) {
        self.msg(Severity::Error, message);
    }

    fn warn(&self, message: &str) {
        self.msg(Severity::Warning, message);
    }

    fn info(&self, message: &str) {
        self.msg(Severity::Info, message);
    }
}

/// `IExtensionPointUser<T>`: o valor contribuído por uma extensão, a descrição dela e o coletor das mensagens dela.
#[derive(Clone)]
pub struct ExtensionPointUser<T> {
    pub description: SharedExtensionDescription,
    pub value: T,
    pub collector: Rc<dyn ExtensionMessageCollector>,
}

/// `ExtensionPointUserDelta<T>`: `delta.added` e `delta.removed`.
#[derive(Clone)]
pub struct ExtensionPointDelta<T> {
    pub added: Vec<ExtensionPointUser<T>>,
    pub removed: Vec<ExtensionPointUser<T>>,
}

impl<T: Clone> ExtensionPointDelta<T> {
    fn to_set(arr: &[ExtensionPointUser<T>]) -> ExtensionIdentifierSet {
        let mut result = ExtensionIdentifierSet::new(Vec::<String>::new());
        for user in arr {
            result.add(&user.description.identifier.value);
        }
        result
    }

    /// `ExtensionPointUserDelta.compute(previous, current)`.
    pub fn compute(previous: Option<&[ExtensionPointUser<T>]>, current: &[ExtensionPointUser<T>]) -> Self {
        let previous = match previous {
            Some(previous) if !previous.is_empty() => previous,
            _ => return Self { added: current.to_vec(), removed: Vec::new() },
        };
        if current.is_empty() {
            return Self { added: Vec::new(), removed: previous.to_vec() };
        }

        let previous_set = Self::to_set(previous);
        let current_set = Self::to_set(current);

        let added = current.iter().filter(|user| !previous_set.has(&user.description.identifier.value)).cloned().collect();
        let removed = previous.iter().filter(|user| !current_set.has(&user.description.identifier.value)).cloned().collect();

        Self { added, removed }
    }
}
