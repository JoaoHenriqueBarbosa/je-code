//! Porte de `src/vs/editor/common/tokenizationRegistry.ts`: o registro de suportes de tokenização por
//! linguagem, com fábricas preguiçosas.
//!
//! Divergência: o mapa de cores guarda as cores como texto CSS (`#RRGGBB`), porque o `je-editor` não depende
//! do `je-theme` (onde mora o `Color`); `getDefaultBackground` devolve o texto.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;

use crate::encoded_token_attributes::color_id;
use crate::languages::{ITokenizationSupport, TokenizationSupportChangedEvent};

/// `ILazyTokenizationSupport<TSupport>`.
pub trait LazyTokenizationSupport<TSupport: PartialSupport + 'static> {
    /// `tokenizationSupport`: a promessa do suporte (ou `None`).
    fn tokenization_support(&self) -> Promise<Option<TSupport>>;
}

/// O suporte guardado no registro global do editor.
pub type SupportRef = Rc<dyn ITokenizationSupport>;

struct FactoryData<TSupport: PartialSupport + 'static> {
    registry: std::rc::Weak<Inner<TSupport>>,
    language_id: String,
    factory: Rc<dyn LazyTokenizationSupport<TSupport>>,
    is_disposed: Cell<bool>,
    resolve_promise: RefCell<Option<Promise<()>>>,
    is_resolved: Cell<bool>,
    registration: RefCell<Vec<DisposableRef>>,
}

impl<TSupport: PartialSupport + 'static> FactoryData<TSupport> {
    fn resolve(self: &Rc<Self>) -> Promise<()> {
        if let Some(p) = self.resolve_promise.borrow().clone() {
            return p;
        }
        let me = self.clone();
        let promise = self.factory.tokenization_support().then(move |value| {
            me.is_resolved.set(true);
            if let (Some(value), false) = (value, me.is_disposed.get())
                && let Some(registry) = me.registry.upgrade()
            {
                let registration = Inner::register(&registry, &me.language_id, value);
                me.registration.borrow_mut().push(registration);
            }
        });
        *self.resolve_promise.borrow_mut() = Some(promise.clone());
        promise
    }

    fn dispose(&self) {
        self.is_disposed.set(true);
        for d in self.registration.borrow_mut().drain(..) {
            d.dispose();
        }
    }
}

struct Inner<TSupport: PartialSupport + 'static> {
    supports: RefCell<HashMap<String, TSupport>>,
    /// A ordem de inserção dos suportes (o `Map` do JS itera na ordem em que as chaves entraram).
    order: RefCell<Vec<String>>,
    factories: RefCell<HashMap<String, Rc<FactoryData<TSupport>>>>,
    on_did_change: Rc<Emitter<TokenizationSupportChangedEvent>>,
    color_map: RefCell<Option<Vec<String>>>,
}

impl<TSupport: PartialSupport + 'static> Inner<TSupport> {
    fn handle_change(&self, language_ids: Vec<String>) {
        self.on_did_change.fire(TokenizationSupportChangedEvent { changed_languages: language_ids, changed_color_map: false });
    }

    fn register(this: &Rc<Self>, language_id: &str, support: TSupport) -> DisposableRef {
        {
            let mut order = this.order.borrow_mut();
            if !order.iter().any(|l| l == language_id) {
                order.push(language_id.to_owned());
            }
        }
        this.supports.borrow_mut().insert(language_id.to_owned(), support.clone());
        this.handle_change(vec![language_id.to_owned()]);
        let (weak, language_id, support) = (Rc::downgrade(this), language_id.to_owned(), support);
        to_disposable(move || {
            let Some(this) = weak.upgrade() else { return };
            let same = this.supports.borrow().get(&language_id).is_some_and(|s| s.same_support(&support));
            if !same {
                return;
            }
            this.supports.borrow_mut().remove(&language_id);
            this.order.borrow_mut().retain(|l| *l != language_id);
            this.handle_change(vec![language_id.clone()]);
        })
    }
}

/// A identidade de um suporte (o `!==` do original compara referências).
pub trait PartialSupport: Clone {
    fn same_support(&self, other: &Self) -> bool;
}

impl PartialSupport for SupportRef {
    fn same_support(&self, other: &Self) -> bool {
        Rc::ptr_eq(self, other)
    }
}

/// `TokenizationRegistry<TSupport>`.
pub struct TokenizationRegistry<TSupport: PartialSupport + 'static> {
    inner: Rc<Inner<TSupport>>,
}

impl<TSupport: PartialSupport + 'static> TokenizationRegistry<TSupport> {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                supports: RefCell::new(HashMap::new()),
                order: RefCell::new(Vec::new()),
                factories: RefCell::new(HashMap::new()),
                on_did_change: Emitter::new(),
                color_map: RefCell::new(None),
            }),
        }
    }

    pub fn on_did_change(&self) -> Event<TokenizationSupportChangedEvent> {
        self.inner.on_did_change.event()
    }

    pub fn handle_change(&self, language_ids: Vec<String>) {
        self.inner.handle_change(language_ids);
    }

    pub fn register(&self, language_id: &str, support: TSupport) -> DisposableRef {
        Inner::register(&self.inner, language_id, support)
    }

    pub fn get(&self, language_id: &str) -> Option<TSupport> {
        self.inner.supports.borrow().get(language_id).cloned()
    }

    pub fn register_factory(&self, language_id: &str, factory: Rc<dyn LazyTokenizationSupport<TSupport>>) -> DisposableRef {
        if let Some(old) = self.inner.factories.borrow_mut().remove(language_id) {
            old.dispose();
        }
        let my_data = Rc::new(FactoryData {
            registry: Rc::downgrade(&self.inner),
            language_id: language_id.to_owned(),
            factory,
            is_disposed: Cell::new(false),
            resolve_promise: RefCell::new(None),
            is_resolved: Cell::new(false),
            registration: RefCell::new(Vec::new()),
        });
        self.inner.factories.borrow_mut().insert(language_id.to_owned(), my_data.clone());
        let (weak, language_id) = (Rc::downgrade(&self.inner), language_id.to_owned());
        to_disposable(move || {
            let Some(inner) = weak.upgrade() else { return };
            let current = inner.factories.borrow().get(&language_id).cloned();
            match current {
                Some(v) if Rc::ptr_eq(&v, &my_data) => {
                    inner.factories.borrow_mut().remove(&language_id);
                    v.dispose();
                }
                _ => {}
            }
        })
    }

    /// `getOrCreate(languageId)`.
    pub fn get_or_create(&self, language_id: &str) -> Promise<Option<TSupport>> {
        if let Some(support) = self.get(language_id) {
            return Promise::resolved(Some(support));
        }
        let factory = self.inner.factories.borrow().get(language_id).cloned();
        let Some(factory) = factory else { return Promise::resolved(None) };
        if factory.is_resolved.get() {
            return Promise::resolved(None);
        }
        let (inner, language_id) = (self.inner.clone(), language_id.to_owned());
        factory.resolve().then(move |()| inner.supports.borrow().get(&language_id).cloned())
    }

    pub fn is_resolved(&self, language_id: &str) -> bool {
        if self.get(language_id).is_some() {
            return true;
        }
        match self.inner.factories.borrow().get(language_id) {
            None => true,
            Some(f) => f.is_resolved.get(),
        }
    }

    pub fn set_color_map(&self, color_map: Vec<String>) {
        *self.inner.color_map.borrow_mut() = Some(color_map);
        let changed = self.inner.order.borrow().clone();
        self.inner.on_did_change.fire(TokenizationSupportChangedEvent { changed_languages: changed, changed_color_map: true });
    }

    pub fn get_color_map(&self) -> Option<Vec<String>> {
        self.inner.color_map.borrow().clone()
    }

    pub fn get_default_background(&self) -> Option<String> {
        let map = self.inner.color_map.borrow();
        let map = map.as_ref()?;
        map.get(color_id::DEFAULT_BACKGROUND as usize).cloned()
    }
}

impl<TSupport: PartialSupport + 'static> Default for TokenizationRegistry<TSupport> {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static GLOBAL_REGISTRY: Rc<TokenizationRegistry<SupportRef>> = Rc::new(TokenizationRegistry::new());
}

/// `TokenizationRegistry`: o registro global do editor (um por thread de UI).
pub fn tokenization_registry() -> Rc<TokenizationRegistry<SupportRef>> {
    GLOBAL_REGISTRY.with(Rc::clone)
}
