//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/find/findFilters.ts`: os filtros da busca do notebook
//! (`NotebookFindFilters`).
//!
//! Divergências:
//! - `FindReplaceState<NotebookFindFilters>` guarda e devolve cópias dos filtros: o tipo é `Clone`, e o emissor `onDidChange` é compartilhado
//!   entre as cópias (`Rc<Emitter>`); quem cria o filtro o registra (`_register` do original) com [`NotebookFindFilters::dispose`].
//! - Os `get`/`set` de cada propriedade são pares `x()` e `set_x(...)`.
//! - `FindReplaceFilters::update(value)` do estado é o `update(v)` do original.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_editor::find::find_state::FindReplaceFilters;

use crate::contrib::notebook::common::notebook_common::{INotebookFindScope, NotebookFindScopeType};

/// `INotebookFindChangeEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct INotebookFindChangeEvent {
    pub markup_input: Option<bool>,
    pub markup_preview: Option<bool>,
    pub code_input: Option<bool>,
    pub code_output: Option<bool>,
    pub find_scope: Option<bool>,
}

/// `NotebookFindFilters`.
#[derive(Clone)]
pub struct NotebookFindFilters {
    on_did_change: Rc<Emitter<INotebookFindChangeEvent>>,
    markup_input: Rc<Cell<bool>>,
    markup_preview: Rc<Cell<bool>>,
    code_input: Rc<Cell<bool>>,
    code_output: Rc<Cell<bool>>,
    find_scope: Rc<RefCell<INotebookFindScope>>,
    initial_markup_input: bool,
    initial_markup_preview: bool,
    initial_code_input: bool,
    initial_code_output: bool,
}

impl NotebookFindFilters {
    pub fn new(markup_input: bool, markup_preview: bool, code_input: bool, code_output: bool, find_scope: INotebookFindScope) -> Self {
        Self {
            on_did_change: Emitter::new(),
            markup_input: Rc::new(Cell::new(markup_input)),
            markup_preview: Rc::new(Cell::new(markup_preview)),
            code_input: Rc::new(Cell::new(code_input)),
            code_output: Rc::new(Cell::new(code_output)),
            find_scope: Rc::new(RefCell::new(find_scope)),
            initial_markup_input: markup_input,
            initial_markup_preview: markup_preview,
            initial_code_input: code_input,
            initial_code_output: code_output,
        }
    }

    /// Os valores iniciais do original (`_markupInput = true` e os demais, com `findScope: None`).
    pub fn with_defaults() -> Self {
        Self::new(true, true, true, true, INotebookFindScope { find_scope_type: NotebookFindScopeType::None, selected_cell_ranges: None, selected_text_ranges: None })
    }

    pub fn on_did_change(&self) -> Event<INotebookFindChangeEvent> {
        self.on_did_change.event()
    }

    pub fn markup_input(&self) -> bool {
        self.markup_input.get()
    }

    pub fn set_markup_input(&self, value: bool) {
        if self.markup_input.get() != value {
            self.markup_input.set(value);
            self.on_did_change.fire(INotebookFindChangeEvent { markup_input: Some(value), ..Default::default() });
        }
    }

    pub fn markup_preview(&self) -> bool {
        self.markup_preview.get()
    }

    pub fn set_markup_preview(&self, value: bool) {
        if self.markup_preview.get() != value {
            self.markup_preview.set(value);
            self.on_did_change.fire(INotebookFindChangeEvent { markup_preview: Some(value), ..Default::default() });
        }
    }

    pub fn code_input(&self) -> bool {
        self.code_input.get()
    }

    pub fn set_code_input(&self, value: bool) {
        if self.code_input.get() != value {
            self.code_input.set(value);
            self.on_did_change.fire(INotebookFindChangeEvent { code_input: Some(value), ..Default::default() });
        }
    }

    pub fn code_output(&self) -> bool {
        self.code_output.get()
    }

    pub fn set_code_output(&self, value: bool) {
        if self.code_output.get() != value {
            self.code_output.set(value);
            self.on_did_change.fire(INotebookFindChangeEvent { code_output: Some(value), ..Default::default() });
        }
    }

    pub fn find_scope(&self) -> INotebookFindScope {
        self.find_scope.borrow().clone()
    }

    /// `set findScope(value)`: o `!==` do original é a identidade do objeto, e aqui todo valor novo é uma troca.
    pub fn set_find_scope(&self, value: INotebookFindScope) {
        *self.find_scope.borrow_mut() = value;
        self.on_did_change.fire(INotebookFindChangeEvent { find_scope: Some(true), ..Default::default() });
    }

    pub fn is_modified(&self) -> bool {
        // do not include findInSelection or either selectedRanges in the check. This will incorrectly mark the filter icon as modified
        self.markup_input.get() != self.initial_markup_input
            || self.markup_preview.get() != self.initial_markup_preview
            || self.code_input.get() != self.initial_code_input
            || self.code_output.get() != self.initial_code_output
    }

    pub fn update(&self, v: &NotebookFindFilters) {
        self.markup_input.set(v.markup_input());
        self.markup_preview.set(v.markup_preview());
        self.code_input.set(v.code_input());
        self.code_output.set(v.code_output());
        *self.find_scope.borrow_mut() = v.find_scope();
    }
}

impl FindReplaceFilters for NotebookFindFilters {
    fn update(&mut self, value: &Self) {
        NotebookFindFilters::update(self, value);
    }
}

impl IDisposable for NotebookFindFilters {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookFindFilters"
    }
}
