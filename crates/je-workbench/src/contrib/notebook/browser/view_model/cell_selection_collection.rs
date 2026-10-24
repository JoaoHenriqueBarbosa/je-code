//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/cellSelectionCollection.ts`: o foco e as seleções de células de
//! um notebook (`NotebookCellSelectionCollection`).
//!
//! Divergências:
//! - `extends Disposable` é a composição com o `Disposable` base, como as demais classes do porte; o `Emitter<string>` de
//!   `onDidChangeSelection` guarda o `source` (`'view' | 'model'`) como `String`, e `setState` recebe o `&str`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::contrib::notebook::common::notebook_range::{ICellRange, cell_ranges_equal};

/// Challenge is List View talks about `element`, which needs extra work to convert to ICellRange as we support Folding and Cell Move
pub struct NotebookCellSelectionCollection {
    base: Rc<Disposable>,
    on_did_change_selection: Rc<Emitter<String>>,
    primary: RefCell<ICellRange>,
    selections: RefCell<Vec<ICellRange>>,
}

impl NotebookCellSelectionCollection {
    pub fn new() -> Self {
        let base = Disposable::new();
        let on_did_change_selection = Emitter::new();
        base.register(on_did_change_selection.clone());
        Self {
            base,
            on_did_change_selection,
            primary: RefCell::new(ICellRange { start: 0, end: 0 }),
            selections: RefCell::new(vec![ICellRange { start: 0, end: 0 }]),
        }
    }

    pub fn on_did_change_selection(&self) -> Event<String> {
        self.on_did_change_selection.event()
    }

    pub fn selections(&self) -> Vec<ICellRange> {
        self.selections.borrow().clone()
    }

    pub fn focus(&self) -> ICellRange {
        *self.primary.borrow()
    }

    pub fn set_state(&self, primary: Option<ICellRange>, selections: &[ICellRange], force_event_emit: bool, source: &str) {
        let valid_primary = primary.unwrap_or(ICellRange { start: 0, end: 0 });
        let valid_selections = if !selections.is_empty() { selections.to_vec() } else { vec![ICellRange { start: 0, end: 0 }] };

        let changed = !cell_ranges_equal(&[valid_primary], &[*self.primary.borrow()]) || !cell_ranges_equal(&self.selections.borrow(), &valid_selections);

        *self.primary.borrow_mut() = valid_primary;
        *self.selections.borrow_mut() = valid_selections;
        if changed || force_event_emit {
            self.on_did_change_selection.fire(source.to_owned());
        }
    }

    pub fn set_selections(&self, selections: &[ICellRange], force_event_emit: bool, source: &str) {
        let primary = *self.primary.borrow();
        self.set_state(Some(primary), selections, force_event_emit, source);
    }
}

impl Default for NotebookCellSelectionCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl IDisposable for NotebookCellSelectionCollection {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookCellSelectionCollection"
    }
}
