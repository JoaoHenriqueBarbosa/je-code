//! Os registros e tipos de `src/vs/workbench/contrib` que o `MainThreadLanguageFeatures` usa:
//! `contrib/callHierarchy/common/callHierarchy.ts` (`CallHierarchyProviderRegistry`),
//! `contrib/typeHierarchy/common/typeHierarchy.ts` (`TypeHierarchyProviderRegistry`) e
//! `contrib/search/common/search.ts` (`IWorkspaceSymbol`, `IWorkspaceSymbolProvider`, `WorkspaceSymbolProviderRegistry`).
//!
//! Divergências:
//! - `CallHierarchyItem` e `TypeHierarchyItem` são o [`HierarchyItemDto`] do protocolo (`data as callh.CallHierarchyItem` é
//!   um `cast` no original: o revive é a identidade). `IncomingCall` e `OutgoingCall` são o [`IncomingCallDto`] e o
//!   [`OutgoingCallDto`].
//! - Os registros de módulo (`const CallHierarchyProviderRegistry = new LanguageFeatureRegistry<...>()`) são
//!   `thread_local`, devolvidos por funções.
//! - `ProviderResult<T[]>` de `provideWorkspaceSymbols` é uma `Promise<Vec<IWorkspaceSymbol>>`: o único provedor do
//!   porte (o do `MainThreadLanguageFeatures`) sempre responde com uma promessa e uma lista.
//! - `getWorkspaceSymbols` e `WorkspaceSymbolItem` não são portados aqui.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_features_service::{Location, Registry};
use je_editor::language_providers::{ProviderResult, SymbolKind, SymbolTag};
use je_editor::position::Position;
use je_editor::text_model::TextModel;

use crate::api::common::ext_host_type_converters::symbols::{HierarchyItemDto, IncomingCallDto, OutgoingCallDto};

/// `CallHierarchyItem`.
pub type CallHierarchyItem = HierarchyItemDto;

/// `CallHierarchySession`: `{ roots, dispose }`.
#[derive(Clone)]
pub struct CallHierarchySession {
    pub roots: Vec<CallHierarchyItem>,
    dispose: Rc<dyn Fn()>,
}

impl CallHierarchySession {
    pub fn new(roots: Vec<CallHierarchyItem>, dispose: impl Fn() + 'static) -> Self {
        Self { roots, dispose: Rc::new(dispose) }
    }
}

impl IDisposable for CallHierarchySession {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `CallHierarchyProvider`.
pub trait CallHierarchyProvider {
    /// `prepareCallHierarchy(document, position, token)`.
    fn prepare_call_hierarchy(&self, document: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<Rc<CallHierarchySession>>;

    /// `provideIncomingCalls(item, token)`.
    fn provide_incoming_calls(&self, item: &CallHierarchyItem, token: &CancellationToken) -> ProviderResult<Vec<IncomingCallDto>>;

    /// `provideOutgoingCalls(item, token)`.
    fn provide_outgoing_calls(&self, item: &CallHierarchyItem, token: &CancellationToken) -> ProviderResult<Vec<OutgoingCallDto>>;
}

/// `TypeHierarchyItem`.
pub type TypeHierarchyItem = HierarchyItemDto;

/// `TypeHierarchySession`: `{ roots, dispose }`.
#[derive(Clone)]
pub struct TypeHierarchySession {
    pub roots: Vec<TypeHierarchyItem>,
    dispose: Rc<dyn Fn()>,
}

impl TypeHierarchySession {
    pub fn new(roots: Vec<TypeHierarchyItem>, dispose: impl Fn() + 'static) -> Self {
        Self { roots, dispose: Rc::new(dispose) }
    }
}

impl IDisposable for TypeHierarchySession {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `TypeHierarchyProvider`.
pub trait TypeHierarchyProvider {
    /// `prepareTypeHierarchy(document, position, token)`.
    fn prepare_type_hierarchy(&self, document: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<Rc<TypeHierarchySession>>;

    /// `provideSupertypes(item, token)`.
    fn provide_supertypes(&self, item: &TypeHierarchyItem, token: &CancellationToken) -> ProviderResult<Vec<TypeHierarchyItem>>;

    /// `provideSubtypes(item, token)`.
    fn provide_subtypes(&self, item: &TypeHierarchyItem, token: &CancellationToken) -> ProviderResult<Vec<TypeHierarchyItem>>;
}

thread_local! {
    static CALL_HIERARCHY_PROVIDER_REGISTRY: Registry<dyn CallHierarchyProvider> = Rc::new(LanguageFeatureRegistry::new(None));
    static TYPE_HIERARCHY_PROVIDER_REGISTRY: Registry<dyn TypeHierarchyProvider> = Rc::new(LanguageFeatureRegistry::new(None));
    static WORKSPACE_SYMBOL_SUPPORTS: RefCell<Vec<Rc<dyn IWorkspaceSymbolProvider>>> = const { RefCell::new(Vec::new()) };
}

/// `CallHierarchyProviderRegistry`.
pub fn call_hierarchy_provider_registry() -> Registry<dyn CallHierarchyProvider> {
    CALL_HIERARCHY_PROVIDER_REGISTRY.with(Rc::clone)
}

/// `TypeHierarchyProviderRegistry`.
pub fn type_hierarchy_provider_registry() -> Registry<dyn TypeHierarchyProvider> {
    TYPE_HIERARCHY_PROVIDER_REGISTRY.with(Rc::clone)
}

/// `IWorkspaceSymbol`.
#[derive(Clone, Debug)]
pub struct IWorkspaceSymbol {
    pub name: String,
    pub container_name: Option<String>,
    pub kind: SymbolKind,
    pub tags: Option<Vec<SymbolTag>>,
    pub location: Location,
    /// O `cacheId` do `IWorkspaceSymbolDto` (`CachedSessionItem`), que o `resolveWorkspaceSymbol` devolve ao host.
    pub cache_id: Option<(u32, u32)>,
}

/// `IWorkspaceSymbolProvider`.
pub trait IWorkspaceSymbolProvider {
    /// `provideWorkspaceSymbols(search, token)`.
    fn provide_workspace_symbols(&self, search: &str, token: &CancellationToken) -> Promise<Vec<IWorkspaceSymbol>>;

    /// Se o provedor implementa o `resolveWorkspaceSymbol?` opcional.
    fn has_resolve_workspace_symbol(&self) -> bool {
        false
    }

    /// `resolveWorkspaceSymbol(item, token)`.
    fn resolve_workspace_symbol(&self, _item: &IWorkspaceSymbol, _token: &CancellationToken) -> ProviderResult<IWorkspaceSymbol> {
        Promise::resolved(None)
    }
}

/// `WorkspaceSymbolProviderRegistry`.
pub struct WorkspaceSymbolProviderRegistry;

impl WorkspaceSymbolProviderRegistry {
    /// `register(provider)`.
    pub fn register(provider: Rc<dyn IWorkspaceSymbolProvider>) -> DisposableRef {
        WORKSPACE_SYMBOL_SUPPORTS.with(|supports| supports.borrow_mut().push(Rc::clone(&provider)));
        to_disposable(move || {
            WORKSPACE_SYMBOL_SUPPORTS.with(|supports| {
                let mut supports = supports.borrow_mut();
                if let Some(index) = supports.iter().position(|support| Rc::ptr_eq(support, &provider)) {
                    supports.remove(index);
                }
            });
        })
    }

    /// `all()`.
    pub fn all() -> Vec<Rc<dyn IWorkspaceSymbolProvider>> {
        WORKSPACE_SYMBOL_SUPPORTS.with(|supports| supports.borrow().clone())
    }
}
