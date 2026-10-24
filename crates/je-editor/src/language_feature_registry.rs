//! Porte de `src/vs/editor/common/languageFeatureRegistry.ts`: o registro de provedores de um recurso de linguagem
//! (`LanguageFeatureRegistry<T>`), que escolhe e ordena os provedores que casam com um modelo pelo
//! [`score`](crate::language_selector::score) do seletor.
//!
//! Representação:
//! - `T` é o tipo do provedor, em geral um `dyn Trait`; o provedor é guardado em `Rc<T>`, e [`LanguageFeatureRegistry::all`]
//!   e as demais consultas devolvem os `Rc` (o objeto do original, compartilhado).
//! - O estado mutável (`_clock`, `_entries`, `_lastCandidate`) mora num `Rc` interno, porque o `IDisposable` que
//!   `register` devolve precisa tirar a entrada do registro depois que ele já foi emprestado a outros.
//! - `Entry._score` é um `Cell<i32>` e a identidade da entrada (o `indexOf(entry)`) é a do `Rc` ([`Rc::ptr_eq`]).
//! - `NotebookInfoResolver` é o `Rc<dyn Fn(&URI) -> Option<NotebookInfo>>` opcional do construtor.
//! - `ReadonlySet<string>` de `registeredLanguageIds` é um `Vec<String>` sem repetição na ordem de inserção, a mesma
//!   que o `Set` enumera (ver `select_language_ids`).
//!
//! `shouldSynchronizeModel` (de `model.ts`) vive aqui por ser o único consumidor no porte até agora.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::uri::URI;

use crate::language_selector::{LanguageSelector, score, select_language_ids};
use crate::text_model::TextModel;

/// `shouldSynchronizeModel(model)`.
pub fn should_synchronize_model(model: &TextModel) -> bool {
    !model.is_too_large_for_syncing() && !model.is_for_simple_widget()
}

struct Entry<T: ?Sized> {
    selector: LanguageSelector,
    provider: Rc<T>,
    score: Cell<i32>,
    time: usize,
}

fn is_exclusive(selector: &LanguageSelector) -> bool {
    match selector {
        LanguageSelector::Id(_) => false,
        LanguageSelector::Array(items) => items.iter().all(is_exclusive),
        LanguageSelector::Filter(filter) => filter.exclusive,
    }
}

fn is_builtin_selector(selector: &LanguageSelector) -> bool {
    match selector {
        LanguageSelector::Id(_) => false,
        LanguageSelector::Array(items) => items.iter().any(is_builtin_selector),
        LanguageSelector::Filter(filter) => filter.is_builtin,
    }
}

/// `NotebookInfo`.
#[derive(Clone)]
pub struct NotebookInfo {
    pub uri: URI,
    pub type_: String,
}

/// `NotebookInfoResolver`.
pub type NotebookInfoResolver = Rc<dyn Fn(&URI) -> Option<NotebookInfo>>;

struct MatchCandidate {
    uri: URI,
    language_id: String,
    notebook_uri: Option<URI>,
    notebook_type: Option<String>,
    recursive: bool,
}

impl MatchCandidate {
    fn equals(&self, other: &MatchCandidate) -> bool {
        self.notebook_type == other.notebook_type
            && self.language_id == other.language_id
            && self.uri.to_string(false) == other.uri.to_string(false)
            && self.notebook_uri.as_ref().map(|uri| uri.to_string(false))
                == other.notebook_uri.as_ref().map(|uri| uri.to_string(false))
            && self.recursive == other.recursive
    }
}

/// `_compareByScoreAndTime`: maior pontuação primeiro, provedor não embutido antes do embutido e o mais novo antes.
fn compare_by_score_and_time<T: ?Sized>(a: &Rc<Entry<T>>, b: &Rc<Entry<T>>) -> Ordering {
    let by_score = b.score.get().cmp(&a.score.get());
    if by_score != Ordering::Equal {
        return by_score;
    }

    // De-prioritize built-in providers
    match (is_builtin_selector(&a.selector), is_builtin_selector(&b.selector)) {
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }

    b.time.cmp(&a.time)
}

struct State<T: ?Sized> {
    clock: Cell<usize>,
    entries: RefCell<Vec<Rc<Entry<T>>>>,
    last_candidate: RefCell<Option<MatchCandidate>>,
    on_did_change: Rc<Emitter<usize>>,
}

/// `LanguageFeatureRegistry<T>`.
pub struct LanguageFeatureRegistry<T: ?Sized + 'static> {
    state: Rc<State<T>>,
    notebook_info_resolver: Option<NotebookInfoResolver>,
}

impl<T: ?Sized + 'static> LanguageFeatureRegistry<T> {
    pub fn new(notebook_info_resolver: Option<NotebookInfoResolver>) -> Self {
        Self {
            state: Rc::new(State {
                clock: Cell::new(0),
                entries: RefCell::new(Vec::new()),
                last_candidate: RefCell::new(None),
                on_did_change: Emitter::new(),
            }),
            notebook_info_resolver,
        }
    }

    /// `onDidChange`: dispara com a quantidade de entradas.
    pub fn on_did_change(&self) -> Event<usize> {
        self.state.on_did_change.event()
    }

    /// `register(selector, provider)`.
    pub fn register(&self, selector: LanguageSelector, provider: Rc<T>) -> DisposableRef {
        let time = self.state.clock.get();
        self.state.clock.set(time + 1);
        let entry = Rc::new(Entry { selector, provider, score: Cell::new(-1), time });

        let count = {
            let mut entries = self.state.entries.borrow_mut();
            entries.push(Rc::clone(&entry));
            entries.len()
        };
        *self.state.last_candidate.borrow_mut() = None;
        self.state.on_did_change.fire(count);

        let state = Rc::clone(&self.state);
        to_disposable(move || {
            let count = {
                let mut entries = state.entries.borrow_mut();
                let Some(index) = entries.iter().position(|candidate| Rc::ptr_eq(candidate, &entry)) else {
                    return;
                };
                entries.remove(index);
                entries.len()
            };
            *state.last_candidate.borrow_mut() = None;
            state.on_did_change.fire(count);
        })
    }

    /// `has(model)`.
    pub fn has(&self, model: &TextModel) -> bool {
        !self.all(model).is_empty()
    }

    /// `all(model)`.
    pub fn all(&self, model: &TextModel) -> Vec<Rc<T>> {
        self.update_scores(model, false);
        self.state
            .entries
            .borrow()
            .iter()
            .filter(|entry| entry.score.get() > 0)
            .map(|entry| Rc::clone(&entry.provider))
            .collect()
    }

    /// `allNoModel()`.
    pub fn all_no_model(&self) -> Vec<Rc<T>> {
        self.state.entries.borrow().iter().map(|entry| Rc::clone(&entry.provider)).collect()
    }

    /// `registeredLanguageIds`.
    pub fn registered_language_ids(&self) -> Vec<String> {
        let mut result = Vec::new();
        for entry in self.state.entries.borrow().iter() {
            select_language_ids(&entry.selector, &mut result);
        }
        result
    }

    /// `ordered(model, recursive = false)`.
    pub fn ordered(&self, model: &TextModel, recursive: bool) -> Vec<Rc<T>> {
        self.ordered_entries(model, recursive).into_iter().map(|entry| Rc::clone(&entry.provider)).collect()
    }

    /// `orderedGroups(model)`: os provedores agrupados por pontuação igual, do grupo de maior pontuação ao menor.
    pub fn ordered_groups(&self, model: &TextModel) -> Vec<Vec<Rc<T>>> {
        let mut result: Vec<Vec<Rc<T>>> = Vec::new();
        let mut last_bucket_score: Option<i32> = None;

        for entry in self.ordered_entries(model, false) {
            match (result.last_mut(), last_bucket_score) {
                (Some(last_bucket), Some(last_score)) if last_score == entry.score.get() => {
                    last_bucket.push(Rc::clone(&entry.provider));
                }
                _ => {
                    last_bucket_score = Some(entry.score.get());
                    result.push(vec![Rc::clone(&entry.provider)]);
                }
            }
        }

        result
    }

    /// `_orderedForEach(model, recursive, callback)`: as entradas com pontuação positiva, já em ordem.
    fn ordered_entries(&self, model: &TextModel, recursive: bool) -> Vec<Rc<Entry<T>>> {
        self.update_scores(model, recursive);
        self.state.entries.borrow().iter().filter(|entry| entry.score.get() > 0).cloned().collect()
    }

    /// `_updateScores(model, recursive)`.
    fn update_scores(&self, model: &TextModel, recursive: bool) {
        let uri = model.uri();
        let notebook_info = self.notebook_info_resolver.as_ref().and_then(|resolver| resolver(&uri));

        // use the uri (scheme, pattern) of the notebook info iff we have one
        // otherwise it's the model's/document's uri
        let candidate = MatchCandidate {
            uri,
            language_id: model.get_language_id(),
            notebook_uri: notebook_info.as_ref().map(|info| info.uri.clone()),
            notebook_type: notebook_info.map(|info| info.type_),
            recursive,
        };

        if self.state.last_candidate.borrow().as_ref().is_some_and(|last| last.equals(&candidate)) {
            // nothing has changed
            return;
        }

        let synchronize = should_synchronize_model(model);
        {
            let entries = self.state.entries.borrow();
            for entry in entries.iter() {
                entry.score.set(score(
                    Some(&entry.selector),
                    &candidate.uri,
                    &candidate.language_id,
                    synchronize,
                    candidate.notebook_uri.as_ref(),
                    candidate.notebook_type.as_deref(),
                ));

                if is_exclusive(&entry.selector) && entry.score.get() > 0 {
                    if recursive {
                        entry.score.set(0);
                    } else {
                        // support for one exclusive selector that overwrites
                        // any other selector
                        for other in entries.iter() {
                            other.score.set(0);
                        }
                        entry.score.set(1000);
                        break;
                    }
                }
            }
        }
        *self.state.last_candidate.borrow_mut() = Some(candidate);

        // needs sorting
        self.state.entries.borrow_mut().sort_by(compare_by_score_and_time);
    }
}

impl<T: ?Sized + 'static> Default for LanguageFeatureRegistry<T> {
    fn default() -> Self {
        Self::new(None)
    }
}

impl<T: ?Sized + 'static> Clone for LanguageFeatureRegistry<T> {
    /// A cópia é o mesmo registro (duas referências ao mesmo objeto no JavaScript): compartilha as entradas, o
    /// relógio e o evento.
    fn clone(&self) -> Self {
        Self { state: Rc::clone(&self.state), notebook_info_resolver: self.notebook_info_resolver.clone() }
    }
}

/// O `LanguageFeatureRegistry<object>` do original (um registro de provedores sem olhar o tipo do provedor), que o
/// `ILanguageFeatureDebounceService` recebe: a identidade do registro e a dos provedores que casam com um modelo.
pub trait AnyLanguageFeatureRegistry {
    /// A identidade do registro (o `===` do JavaScript): a mesma para todas as cópias de um registro.
    fn identity(&self) -> usize;

    /// A identidade de cada provedor de `all(model)`, na ordem de `all`.
    fn provider_identities(&self, model: &TextModel) -> Vec<usize>;
}

impl<T: ?Sized + 'static> AnyLanguageFeatureRegistry for LanguageFeatureRegistry<T> {
    fn identity(&self) -> usize {
        Rc::as_ptr(&self.state).cast::<()>() as usize
    }

    fn provider_identities(&self, model: &TextModel) -> Vec<usize> {
        self.all(model).iter().map(|provider| Rc::as_ptr(provider).cast::<()>() as usize).collect()
    }
}
