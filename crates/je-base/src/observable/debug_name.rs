//! Porte de `observableInternal/debugName.ts`. O nome de debug sai do nome explícito e do dono. Não há
//! `referenceFn` (o JS lê o `/** @description */` do código-fonte do fecho) nem `findKey` (procura o
//! campo do dono que aponta para o observável): não existem em Rust.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static COUNT_PER_NAME: RefCell<HashMap<String, u32>> = RefCell::new(HashMap::new());
    static COUNT_PER_CLASS_NAME: RefCell<HashMap<String, u32>> = RefCell::new(HashMap::new());
}

struct OwnerInner {
    class_name: Option<String>,
    id: RefCell<Option<String>>,
}

/// `DebugOwner`: o objeto dono do observável, identificado só pelo nome da classe.
#[derive(Clone)]
pub struct DebugOwner(Rc<OwnerInner>);

impl DebugOwner {
    pub fn new(class_name: &str) -> Self {
        Self(Rc::new(OwnerInner { class_name: Some(class_name.to_owned()), id: RefCell::new(None) }))
    }

    /// O objeto simples (`getClassName` devolve `undefined`, vira `Object`).
    pub fn plain() -> Self {
        Self(Rc::new(OwnerInner { class_name: None, id: RefCell::new(None) }))
    }

    fn format(&self) -> String {
        if let Some(id) = &*self.0.id.borrow() {
            return id.clone();
        }
        let class_name = self.0.class_name.clone().unwrap_or_else(|| "Object".into());
        let count = COUNT_PER_CLASS_NAME.with(|m| {
            let mut m = m.borrow_mut();
            let c = m.entry(class_name.clone()).or_insert(0);
            *c += 1;
            *c
        });
        let result = if count == 1 { class_name } else { format!("{class_name}#{count}") };
        *self.0.id.borrow_mut() = Some(result.clone());
        result
    }
}

/// `DebugNameSource`.
#[derive(Clone)]
pub enum DebugNameSource {
    Name(String),
    Lazy(Rc<dyn Fn() -> Option<String>>),
}

impl From<&str> for DebugNameSource {
    fn from(s: &str) -> Self {
        Self::Name(s.to_owned())
    }
}

/// `IDebugNameData`.
#[derive(Clone, Default)]
pub struct DebugNameOptions {
    pub owner: Option<DebugOwner>,
    pub debug_name: Option<DebugNameSource>,
}

impl DebugNameOptions {
    pub fn named(name: &str) -> Self {
        Self { owner: None, debug_name: Some(name.into()) }
    }

    pub fn owned(owner: DebugOwner) -> Self {
        Self { owner: Some(owner), debug_name: None }
    }
}

/// `DebugNameData`. O cache do nome (a `WeakMap` do original) fica aqui, um por observável.
#[derive(Default)]
pub struct DebugNameData {
    owner: Option<DebugOwner>,
    source: Option<DebugNameSource>,
    cached: RefCell<Option<String>>,
}

impl From<DebugNameOptions> for DebugNameData {
    fn from(o: DebugNameOptions) -> Self {
        Self::new(o.owner, o.debug_name)
    }
}

impl DebugNameData {
    pub fn new(owner: Option<DebugOwner>, source: Option<DebugNameSource>) -> Self {
        Self { owner, source, cached: RefCell::new(None) }
    }

    /// `getDebugName(target, data)`.
    pub fn get_debug_name(&self) -> Option<String> {
        if let Some(c) = &*self.cached.borrow() {
            return Some(c.clone());
        }
        let name = self.compute()?;
        let count = COUNT_PER_NAME.with(|m| {
            let mut m = m.borrow_mut();
            let c = m.entry(name.clone()).or_insert(0);
            *c += 1;
            *c
        });
        let result = if count == 1 { name } else { format!("{name}#{count}") };
        *self.cached.borrow_mut() = Some(result.clone());
        Some(result)
    }

    fn compute(&self) -> Option<String> {
        let owner_str = self.owner.as_ref().map(|o| format!("{}.", o.format())).unwrap_or_default();
        match &self.source {
            Some(DebugNameSource::Name(n)) => Some(format!("{owner_str}{n}")),
            Some(DebugNameSource::Lazy(f)) => f().map(|r| format!("{owner_str}{r}")),
            None => None,
        }
    }
}
