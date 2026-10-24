//! Porte parcial de `src/vs/workbench/api/browser/mainThreadDocuments.ts`: só o [`BoundModelReferenceCollection`], a
//! coleção que segura as referências de modelo que o host abriu e as solta por idade, por tamanho total e por
//! quantidade. O `ModelTracker` e o `MainThreadDocuments` do arquivo dependem do `ITextModel`, do `ITextModelService` e
//! do `ITextFileService` do workbench e ficam para quando esses serviços chegarem ao host de extensões.
//!
//! Divergências:
//! - `IReference<unknown>` só tem o `dispose` lido (o `object` nunca é usado), então `add` recebe a [`DisposableRef`]
//!   da referência.
//! - `dispose()` do original é `dispose(this._data)`, que itera o array enquanto cada entrada se remove dele (e pula
//!   entradas alternadas). Aqui o descarte é de uma cópia das entradas, que é o que a intenção pede: nenhuma entrada
//!   fica com o timer vivo.
//! - Os parâmetros opcionais do construtor (`maxAge`, `maxLength`, `maxSize`) são os de [`BoundModelReferenceLimits`], com
//!   o `Default` com os valores do original.
//! - `setTimeout` e `clearTimeout` são os de `je_base::runtime` (o relógio virtual dos testes os controla).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::resources::ExtUri;
use je_base::runtime::{clear_timeout, set_timeout, TimerId};
use je_base::uri::URI;

/// Os limites do construtor: `_maxAge`, `_maxLength` e `_maxSize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundModelReferenceLimits {
    /// Descarta pela idade, em milissegundos.
    pub max_age: u64,
    /// Descarta pelo tamanho total.
    pub max_length: usize,
    /// Descarta pelo número de referências.
    pub max_size: usize,
}

impl Default for BoundModelReferenceLimits {
    fn default() -> Self {
        Self { max_age: 1000 * 60 * 3, max_length: 1024 * 1024 * 80, max_size: 50 }
    }
}

/// O elemento de `_data`: `{ uri, length, dispose }`.
struct Entry {
    id: u64,
    uri: URI,
    length: usize,
    reference: DisposableRef,
    handle: Cell<Option<TimerId>>,
}

#[derive(Default)]
struct State {
    data: Vec<Rc<Entry>>,
    length: usize,
    next_id: u64,
}

/// `entry.dispose`: tira a entrada de `_data` (se ainda estiver lá), solta a referência e cancela o timer.
fn dispose_entry(state: &RefCell<State>, entry: &Entry) {
    let present = state.borrow().data.iter().any(|candidate| candidate.id == entry.id);
    if !present {
        return;
    }
    state.borrow_mut().length -= entry.length;
    entry.reference.dispose();
    if let Some(handle) = entry.handle.take() {
        clear_timeout(handle);
    }
    state.borrow_mut().data.retain(|candidate| candidate.id != entry.id);
}

/// `BoundModelReferenceCollection`.
pub struct BoundModelReferenceCollection {
    ext_uri: ExtUri,
    limits: BoundModelReferenceLimits,
    state: Rc<RefCell<State>>,
}

impl BoundModelReferenceCollection {
    pub fn new(ext_uri: ExtUri, limits: BoundModelReferenceLimits) -> Self {
        Self { ext_uri, limits, state: Rc::default() }
    }

    /// `remove`.
    pub fn remove(&self, uri: &URI) {
        // copy the entries because dispose will modify the list
        let entries: Vec<Rc<Entry>> = self.state.borrow().data.clone();
        for entry in entries {
            if self.ext_uri.is_equal_or_parent(&entry.uri, uri, false, false) {
                dispose_entry(&self.state, &entry);
            }
        }
    }

    /// `add`: `length` é o `length = 0` do original quando omitido.
    pub fn add(&self, uri: URI, reference: DisposableRef, length: usize) {
        let entry = {
            let mut state = self.state.borrow_mut();
            state.next_id += 1;
            Rc::new(Entry { id: state.next_id, uri, length, reference, handle: Cell::new(None) })
        };
        let timer_state = self.state.clone();
        let timer_entry = entry.clone();
        entry.handle.set(Some(set_timeout(move || dispose_entry(&timer_state, &timer_entry), self.limits.max_age)));

        {
            let mut state = self.state.borrow_mut();
            state.data.push(entry);
            state.length += length;
        }
        self.cleanup();
    }

    fn cleanup(&self) {
        // clean-up wrt total length
        while self.state.borrow().length > self.limits.max_length {
            let first = self.state.borrow().data[0].clone();
            dispose_entry(&self.state, &first);
        }
        // clean-up wrt number of documents
        let extra_size = (self.limits.max_size as f64 * 1.2).ceil() as usize;
        let count = self.state.borrow().data.len();
        if count >= extra_size {
            let extra: Vec<Rc<Entry>> = self.state.borrow().data[..extra_size - self.limits.max_size].to_vec();
            for entry in extra {
                dispose_entry(&self.state, &entry);
            }
        }
    }
}

impl IDisposable for BoundModelReferenceCollection {
    fn dispose(&self) {
        let entries: Vec<Rc<Entry>> = self.state.borrow().data.clone();
        for entry in entries {
            dispose_entry(&self.state, &entry);
        }
    }
}
