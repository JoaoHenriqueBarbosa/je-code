//! Porte de `observableInternal/commonFacade/deps.ts` mais os auxiliares que o JS dá de graça:
//! igualdade estrita (`===`), o `Set` com iteração viva e o `try/finally`.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;

/// O `===` do JavaScript. Valores primitivos comparam por valor, `Rc` por identidade (um objeto do JS é
/// um `Rc` aqui), `NaN` nunca é igual a si mesmo.
pub trait StrictEquals {
    fn strict_equals(&self, other: &Self) -> bool;
}

macro_rules! strict_equals_by_eq {
    ($($t:ty),* $(,)?) => {$(
        impl $crate::observable::deps::StrictEquals for $t {
            fn strict_equals(&self, other: &Self) -> bool { self == other }
        }
    )*};
}

strict_equals_by_eq!((), bool, char, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64, String, &'static str);

impl<T: ?Sized> StrictEquals for Rc<T> {
    fn strict_equals(&self, other: &Self) -> bool {
        Rc::ptr_eq(self, other)
    }
}

impl<T: StrictEquals> StrictEquals for Option<T> {
    fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (None, None) => true,
            (Some(a), Some(b)) => a.strict_equals(b),
            _ => false,
        }
    }
}

/// `EqualityComparer<T>`.
pub type EqualityComparer<T> = Rc<dyn Fn(&T, &T) -> bool>;

/// `strictEquals`.
pub fn strict_equals<T: StrictEquals + 'static>() -> EqualityComparer<T> {
    Rc::new(|a, b| a.strict_equals(b))
}

/// `try { body } finally { fin }`: o pânico segue depois do `fin`.
pub fn try_finally<R>(body: impl FnOnce() -> R, fin: impl FnOnce()) -> R {
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(r) => {
            fin();
            r
        }
        Err(p) => {
            fin();
            panic::resume_unwind(p)
        }
    }
}

/// O `Set` do JS: ordem de inserção, e a iteração é viva (quem entra durante o laço é visitado, quem sai
/// antes de ser visitado é pulado). A chave é a identidade do valor (endereço).
pub struct LiveSet<V: Clone> {
    next_seq: u64,
    by_key: HashMap<usize, (u64, V)>,
    order: BTreeMap<u64, usize>,
}

impl<V: Clone> Default for LiveSet<V> {
    fn default() -> Self {
        Self { next_seq: 1, by_key: HashMap::new(), order: BTreeMap::new() }
    }
}

impl<V: Clone> LiveSet<V> {
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    pub fn has(&self, key: usize) -> bool {
        self.by_key.contains_key(&key)
    }

    /// `add`: devolve `true` se entrou (não existia).
    pub fn add(&mut self, key: usize, value: V) -> bool {
        if self.by_key.contains_key(&key) {
            return false;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.order.insert(seq, key);
        self.by_key.insert(key, (seq, value));
        true
    }

    pub fn delete(&mut self, key: usize) -> bool {
        match self.by_key.remove(&key) {
            Some((seq, _)) => {
                self.order.remove(&seq);
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        self.by_key.clear();
        self.order.clear();
    }

    pub fn snapshot(&self) -> Vec<V> {
        self.order.values().map(|k| self.by_key[k].1.clone()).collect()
    }

    fn next_after(&self, after: u64) -> Option<(u64, V)> {
        let (seq, key) = self.order.range(after + 1..).next()?;
        Some((*seq, self.by_key[key].1.clone()))
    }
}

/// `for (const x of set) f(x)` com iteração viva. `f` pode mexer no conjunto; `f` devolve `false` para
/// o `break`.
pub fn for_each_live<V: Clone>(set: &RefCell<LiveSet<V>>, mut f: impl FnMut(V) -> bool) {
    let mut last = 0;
    loop {
        let next = set.borrow().next_after(last);
        let Some((seq, value)) = next else { break };
        last = seq;
        if !f(value) {
            break;
        }
    }
}
