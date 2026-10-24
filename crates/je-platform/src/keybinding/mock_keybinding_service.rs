//! Porte parcial de `src/vs/platform/keybinding/test/common/mockKeybindingService.ts`: o `MockContextKeyService`, o
//! `MockScopableContextKeyService` e o `MockKeybindingContextKey`, os dublês de serviço de chaves de contexto que as
//! suítes do VS Code usam. Ficam no código (e não num auxiliar de teste) para os testes de todas as crates usarem o
//! mesmo, como o `TestConfigurationService`.
//!
//! Divergências:
//! - **`getContext(domNode)`** devolve `null` no original, e quem o usa lê `getValue` de `null` (um `TypeError`). O
//!   contrato do porte devolve um `IContext`: o do mock lança esse mesmo erro ao ler um valor. Uma subclasse que
//!   sobrescreve o `getContext` (como o `TasksMockContextKeyService`) usa [`MockContextKeyService::with_context`].
//! - **`contextId`** e **`getContextValuesContainer`** não existem no mock do original (são da implementação
//!   concreta, que o contrato do porte expõe): chamá-los lança.
//! - **`createScoped` e `createOverlay`** devolvem o próprio serviço (`this`).
//! - O `MockKeybindingService` do mesmo arquivo entra quando uma suíte portada o usar.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;

use crate::contextkey::{
    ContextKeyExpression, IContext, IContextKey, IContextKeyChangeEvent, IContextKeyService, IContextKeyServiceTarget, IScopedContextKeyService,
};

/// `MockKeybindingContextKey`.
struct MockKeybindingContextKey {
    default_value: Dynamic,
    value: RefCell<Dynamic>,
}

impl IContextKey for MockKeybindingContextKey {
    fn set(&self, value: Dynamic) {
        *self.value.borrow_mut() = value;
    }

    fn reset(&self) {
        *self.value.borrow_mut() = self.default_value.clone();
    }

    fn get(&self) -> Dynamic {
        self.value.borrow().clone()
    }
}

/// O `null` que o `getContext` do mock devolve: ler um valor dele é o `TypeError` do original.
struct NullContext;

impl IContext for NullContext {
    fn get_value(&self, _key: &str) -> Dynamic {
        throw(Error::type_error("Cannot read properties of null (reading 'getValue')"))
    }
}

/// `MockContextKeyService`.
pub struct MockContextKeyService {
    this: Weak<MockContextKeyService>,
    keys: RefCell<HashMap<String, Rc<MockKeybindingContextKey>>>,
    context: Option<Rc<dyn IContext>>,
    scopable: bool,
}

impl MockContextKeyService {
    /// `new MockContextKeyService()`.
    pub fn new() -> Rc<Self> {
        Self::build(None, false)
    }

    /// Uma subclasse que sobrescreve `getContext` para devolver `context`.
    pub fn with_context(context: Rc<dyn IContext>) -> Rc<Self> {
        Self::build(Some(context), false)
    }

    /// `new MockScopableContextKeyService()`: o `createScoped` devolve um serviço novo.
    pub fn scopable() -> Rc<Self> {
        Self::build(None, true)
    }

    fn build(context: Option<Rc<dyn IContext>>, scopable: bool) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), keys: RefCell::default(), context, scopable })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }
}

impl IDisposable for MockContextKeyService {
    fn dispose(&self) {}
}

impl IContextKeyService for MockContextKeyService {
    fn on_did_change_context(&self) -> Event<Rc<dyn IContextKeyChangeEvent>> {
        Event::none()
    }

    fn buffer_change_events(&self, callback: &dyn Fn()) {
        callback();
    }

    fn create_key(&self, key: &str, default_value: Dynamic) -> Rc<dyn IContextKey> {
        let ret = Rc::new(MockKeybindingContextKey { value: RefCell::new(default_value.clone()), default_value });
        self.keys.borrow_mut().insert(key.to_owned(), ret.clone());
        ret
    }

    fn context_matches_rules(&self, _rules: Option<&ContextKeyExpression>) -> bool {
        false
    }

    fn get_context_key_value(&self, key: &str) -> Dynamic {
        self.keys.borrow().get(key).map_or(Dynamic::Undefined, |value| value.get())
    }

    fn create_scoped(&self, _target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService> {
        if self.scopable { Self::scopable() } else { self.rc() }
    }

    fn create_overlay(&self, _overlay: Vec<(String, Dynamic)>) -> Rc<dyn IContextKeyService> {
        self.rc()
    }

    fn get_context(&self, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> Rc<dyn IContext> {
        self.context.clone().unwrap_or_else(|| Rc::new(NullContext))
    }

    fn update_parent(&self, _parent_context_key_service: Rc<dyn IContextKeyService>) {
        // no-op
    }

    fn context_id(&self) -> i64 {
        throw(Error::type_error("MockContextKeyService has no contextId"))
    }

    fn get_context_values_container(&self, _context_id: i64) -> Rc<dyn IContext> {
        throw(Error::type_error("mockContextKeyService.getContextValuesContainer is not a function"))
    }
}

impl IScopedContextKeyService for MockContextKeyService {}
