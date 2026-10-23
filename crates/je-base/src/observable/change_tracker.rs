//! Porte de `observableInternal/changeTracker.ts`.
//!
//! O resumo de mudanças (`TChangeSummary`) é acessado por `&S`: no original o mesmo objeto é entregue ao
//! `handleChange` (que o altera) enquanto o `beforeUpdate` ainda o está preenchendo, então o resumo usa
//! mutabilidade interior (`RefCell`/`Cell`).
//!
//! `recordChanges` do TypeScript devolve um resumo tipado por chave (`{ [key]: valor, changes: [...] }`),
//! algo que o Rust não expressa. Aqui [`record_changes`] guarda valores e mudanças dinamicamente
//! ([`RecordedChanges`]), lidos por chave e tipo.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use super::base::{AnyObservable, IObservable, IReader};
use crate::errors::{self, Error};

/// `IChangeContext`.
pub trait IChangeContext {
    fn changed_observable(&self) -> &dyn AnyObservable;

    /// `change` (`&()` se indefinida).
    fn change_any(&self) -> &dyn Any;

    /// `didChange(observable)`: o observável causou a mudança?
    fn did_change(&self, observable: &dyn AnyObservable) -> bool {
        self.changed_observable().id() == observable.id()
    }
}

impl dyn IChangeContext + '_ {
    /// A mudança tipada; `None` se for indefinida (`undefined`) ou de outro tipo.
    pub fn change<C: 'static>(&self) -> Option<&C> {
        if super::base::is_undefined_change(self.change_any()) {
            return None;
        }
        self.change_any().downcast_ref::<C>()
    }
}

/// `IChangeTracker<TChangeSummary>`.
pub trait IChangeTracker<S> {
    fn create_change_summary(&self, previous: Option<&S>) -> S;

    fn handle_change(&self, ctx: &dyn IChangeContext, change_summary: &S) -> bool;

    fn before_update(&self, _reader: &dyn IReader, _change_summary: &S) {}
}

/// Um `IChangeTracker` montado de fechos.
pub struct FnChangeTracker<S> {
    #[allow(clippy::type_complexity)]
    pub create: Box<dyn Fn(Option<&S>) -> S>,
    #[allow(clippy::type_complexity)]
    pub handle: Box<dyn Fn(&dyn IChangeContext, &S) -> bool>,
    #[allow(clippy::type_complexity)]
    pub before: Option<Box<dyn Fn(&dyn IReader, &S)>>,
}

impl<S: 'static> FnChangeTracker<S> {
    pub fn new(create: impl Fn(Option<&S>) -> S + 'static, handle: impl Fn(&dyn IChangeContext, &S) -> bool + 'static) -> Rc<Self> {
        Rc::new(Self { create: Box::new(create), handle: Box::new(handle), before: None })
    }
}

impl<S> IChangeTracker<S> for FnChangeTracker<S> {
    fn create_change_summary(&self, previous: Option<&S>) -> S {
        (self.create)(previous)
    }

    fn handle_change(&self, ctx: &dyn IChangeContext, change_summary: &S) -> bool {
        (self.handle)(ctx, change_summary)
    }

    fn before_update(&self, reader: &dyn IReader, change_summary: &S) {
        if let Some(b) = &self.before {
            b(reader, change_summary);
        }
    }
}

/// Uma mudança registrada: a chave e o delta (`None` se indefinido).
#[derive(Clone)]
pub struct RecordedChange {
    pub key: &'static str,
    pub change: Option<Rc<dyn Any>>,
}

impl RecordedChange {
    pub fn change<C: 'static>(&self) -> Option<&C> {
        self.change.as_ref()?.downcast_ref::<C>()
    }
}

/// O resumo de `recordChanges`: o último valor de cada observável e a lista de mudanças.
#[derive(Default)]
pub struct RecordedChanges {
    pub changes: RefCell<Vec<RecordedChange>>,
    values: RefCell<Vec<(&'static str, Rc<dyn Any>)>>,
}

impl RecordedChanges {
    /// O valor lido na última atualização.
    pub fn value<T: Clone + 'static>(&self, key: &str) -> Option<T> {
        self.values.borrow().iter().find(|(k, _)| *k == key).and_then(|(_, v)| v.downcast_ref::<T>().cloned())
    }
}

type Reader = Box<dyn Fn(&dyn IReader) -> Rc<dyn Any>>;

struct Recorded {
    key: &'static str,
    observable: Rc<dyn AnyObservable>,
    read: Reader,
    copy_change: Box<dyn Fn(&dyn Any) -> Option<Rc<dyn Any>>>,
}

/// `recordChanges(obs)`. Monte com [`RecordBuilder`]. A chave `changes` é reservada.
pub struct RecordChanges {
    items: Vec<Recorded>,
}

/// Monta o `recordChanges`: cada observável com o tipo do seu delta.
#[derive(Default)]
pub struct RecordBuilder {
    items: Vec<Recorded>,
}

pub fn record_changes() -> RecordBuilder {
    RecordBuilder::default()
}

impl RecordBuilder {
    /// Acrescenta `key: observable`, com `C` o tipo do delta.
    pub fn add<T: Clone + 'static, C: Clone + 'static>(mut self, key: &'static str, observable: &IObservable<T>) -> Self {
        let o = observable.clone();
        self.items.push(Recorded {
            key,
            observable: observable.to_any_observable(),
            read: Box::new(move |r| Rc::new(o.read(r)) as Rc<dyn Any>),
            copy_change: Box::new(|c| {
                if super::base::is_undefined_change(c) { None } else { c.downcast_ref::<C>().map(|c| Rc::new(c.clone()) as Rc<dyn Any>) }
            }),
        });
        self
    }

    pub fn build(self) -> Rc<RecordChanges> {
        Rc::new(RecordChanges { items: self.items })
    }
}

impl IChangeTracker<RecordedChanges> for RecordChanges {
    fn create_change_summary(&self, _previous: Option<&RecordedChanges>) -> RecordedChanges {
        RecordedChanges::default()
    }

    fn handle_change(&self, ctx: &dyn IChangeContext, summary: &RecordedChanges) -> bool {
        for item in &self.items {
            if ctx.did_change(&*item.observable) {
                summary.changes.borrow_mut().push(RecordedChange { key: item.key, change: (item.copy_change)(ctx.change_any()) });
            }
        }
        true
    }

    fn before_update(&self, reader: &dyn IReader, summary: &RecordedChanges) {
        for item in &self.items {
            if item.key == "changes" {
                errors::throw(Error::bug_indicating(Some("property name \"changes\" is reserved for change tracking")));
            }
            let v = (item.read)(reader);
            let mut values = summary.values.borrow_mut();
            match values.iter_mut().find(|(k, _)| *k == item.key) {
                Some(slot) => slot.1 = v,
                None => values.push((item.key, v)),
            }
        }
    }
}

/// `recordChangesLazy(getObs)`: como [`record_changes`], mas os observáveis só são pedidos na primeira
/// necessidade.
pub fn record_changes_lazy(get_obs: impl Fn() -> RecordBuilder + 'static) -> Rc<LazyRecordChanges> {
    Rc::new(LazyRecordChanges { get: Box::new(get_obs), inner: RefCell::new(None) })
}

pub struct LazyRecordChanges {
    get: Box<dyn Fn() -> RecordBuilder>,
    inner: RefCell<Option<Rc<RecordChanges>>>,
}

impl LazyRecordChanges {
    fn resolve(&self) -> Rc<RecordChanges> {
        if self.inner.borrow().is_none() {
            let built = (self.get)().build();
            *self.inner.borrow_mut() = Some(built);
        }
        self.inner.borrow().clone().expect("acabou de ser criado")
    }
}

impl IChangeTracker<RecordedChanges> for LazyRecordChanges {
    fn create_change_summary(&self, previous: Option<&RecordedChanges>) -> RecordedChanges {
        RecordChanges { items: Vec::new() }.create_change_summary(previous)
    }

    fn handle_change(&self, ctx: &dyn IChangeContext, summary: &RecordedChanges) -> bool {
        self.resolve().handle_change(ctx, summary)
    }

    fn before_update(&self, reader: &dyn IReader, summary: &RecordedChanges) {
        self.resolve().before_update(reader, summary);
    }
}
