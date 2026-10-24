//! Porte de `src/vs/editor/contrib/suggest/browser/suggestAlternatives.ts`: a navegação entre sugestões alternativas
//! já aceitas (`SuggestAlternatives`), com a chave de contexto `hasOtherSuggestions`.
//!
//! Representação:
//! - `static readonly OtherSuggestions` é o `thread_local` lido por [`SuggestAlternatives::other_suggestions`] (o
//!   `RawContextKey` guarda o nome, o valor padrão e a expressão `Defined`; `bindTo(service)` é o do `RawContextKey`).
//! - `ISelectedSuggestion` é o de `suggest_widget_contract.rs`. O `acceptNext` é um `Rc<dyn Fn(&ISelectedSuggestion)>`
//!   (o retorno `unknown` do original é descartado).
//! - O editor é o `Rc<CodeEditorWidget>` (`ICodeEditor` não é objeto de trait, tem métodos genéricos). O ouvinte da
//!   posição do cursor guarda só um [`Weak`] da instância: [`SuggestAlternatives::new`] devolve `Rc<Self>`.
//! - O `try { ... } finally { this._ignore = false }` de `_move` é um [`errors::catch`] que restaura `_ignore` e
//!   relança o erro (um erro do original é um pânico no porte).
//! - `model.items` é a lista filtrada de [`CompletionModel::items`]; os índices são `usize`.
//!
//! Divergências:
//! - `this._listener` sobrescrito por um segundo `set` sem `reset` fica sem descarte, como no original: o ouvinte
//!   antigo só chama `reset`, e sai com o editor.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_platform::contextkey::{IContextKey, IContextKeyService, RawContextKey};

use crate::code_editor_widget::CodeEditorWidget;
use crate::suggest::completion_model::CompletionModel;
use crate::suggest::suggest_widget_contract::ISelectedSuggestion;

thread_local! {
    static OTHER_SUGGESTIONS: RawContextKey = RawContextKey::new("hasOtherSuggestions", false, je_platform::contextkey::ContextKeyMeta::None);
}

/// `acceptNext`.
pub type AcceptNext = Rc<dyn Fn(&ISelectedSuggestion)>;

/// `SuggestAlternatives`.
pub struct SuggestAlternatives {
    editor: Rc<CodeEditorWidget>,
    ck_other_suggestions: Rc<dyn IContextKey>,
    index: Cell<usize>,
    model: RefCell<Option<Rc<CompletionModel>>>,
    accept_next: RefCell<Option<AcceptNext>>,
    listener: RefCell<Option<DisposableRef>>,
    ignore: Cell<bool>,
    me: Weak<SuggestAlternatives>,
}

impl SuggestAlternatives {
    /// `SuggestAlternatives.OtherSuggestions`.
    pub fn other_suggestions() -> RawContextKey {
        OTHER_SUGGESTIONS.with(Clone::clone)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, context_key_service: &dyn IContextKeyService) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            editor: Rc::clone(editor),
            ck_other_suggestions: Self::other_suggestions().bind_to(context_key_service),
            index: Cell::new(0),
            model: RefCell::new(None),
            accept_next: RefCell::new(None),
            listener: RefCell::new(None),
            ignore: Cell::new(false),
            me: me.clone(),
        })
    }

    pub fn reset(&self) {
        self.ck_other_suggestions.reset();
        let listener = self.listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
        *self.model.borrow_mut() = None;
        *self.accept_next.borrow_mut() = None;
        self.ignore.set(false);
    }

    /// `set({ model, index }, acceptNext)`.
    pub fn set(&self, selected: &ISelectedSuggestion, accept_next: AcceptNext) {
        let ISelectedSuggestion { model, index, .. } = selected;

        // no suggestions -> nothing to do
        if model.items().is_empty() {
            self.reset();
            return;
        }

        // no alternative suggestions -> nothing to do
        let next_index = Self::move_index(true, model, *index);
        if next_index == *index {
            self.reset();
            return;
        }

        *self.accept_next.borrow_mut() = Some(accept_next);
        *self.model.borrow_mut() = Some(Rc::clone(model));
        self.index.set(*index);
        let weak = self.me.clone();
        let listener = self.editor.on_did_change_cursor_position().listen(move |_| {
            if let Some(alternatives) = weak.upgrade()
                && !alternatives.ignore.get()
            {
                alternatives.reset();
            }
        });
        *self.listener.borrow_mut() = Some(listener);
        self.ck_other_suggestions.set(Dynamic::Bool(true));
    }

    /// `_moveIndex(fwd, model, index)` (ver "Divergências").
    fn move_index(fwd: bool, model: &CompletionModel, index: usize) -> usize {
        let items = model.items();
        let length = items.len();
        let mut new_index = index;
        for _ in 0..length {
            new_index = (new_index + length + if fwd { 1 } else { length - 1 }) % length;
            if new_index == index {
                break;
            }
            if items[new_index].completion.borrow().additional_text_edits.is_none() {
                break;
            }
        }
        new_index
    }

    pub fn next(&self) {
        self.move_to(true);
    }

    pub fn prev(&self) {
        self.move_to(false);
    }

    fn move_to(&self, fwd: bool) {
        let Some(model) = self.model.borrow().clone() else {
            // nothing to reason about
            return;
        };
        self.ignore.set(true);
        let result = errors::catch(|| {
            let index = Self::move_index(fwd, &model, self.index.get());
            self.index.set(index);
            let item = model.items()[index].clone();
            let accept_next = self.accept_next.borrow().clone().expect("o `acceptNext` vale enquanto há modelo");
            accept_next(&ISelectedSuggestion { index, item, model: Rc::clone(&model) });
        });
        self.ignore.set(false);
        if let Err(error) = result {
            errors::throw(error);
        }
    }
}

impl IDisposable for SuggestAlternatives {
    fn dispose(&self) {
        self.reset();
    }
}
