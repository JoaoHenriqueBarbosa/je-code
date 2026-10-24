//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/eventDispatcher.ts`: o despachante dos eventos de view do
//! notebook (`NotebookEventDispatcher`).
//!
//! Divergências:
//! - `extends Disposable` é a composição com o `Disposable` base; o `switch (e.type)` é o `match` sobre [`NotebookViewEvent`].

use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::contrib::notebook::browser::notebook_view_events::{
    NotebookCellStateChangedEvent, NotebookLayoutChangedEvent, NotebookMetadataChangedEvent, NotebookViewEvent,
};

/// `NotebookEventDispatcher`.
pub struct NotebookEventDispatcher {
    base: Rc<Disposable>,
    on_did_change_layout: Rc<Emitter<NotebookLayoutChangedEvent>>,
    on_did_change_metadata: Rc<Emitter<NotebookMetadataChangedEvent>>,
    on_did_change_cell_state: Rc<Emitter<NotebookCellStateChangedEvent>>,
}

impl NotebookEventDispatcher {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_layout = Emitter::new();
        let on_did_change_metadata = Emitter::new();
        let on_did_change_cell_state = Emitter::new();
        base.register(on_did_change_layout.clone());
        base.register(on_did_change_metadata.clone());
        base.register(on_did_change_cell_state.clone());
        Rc::new(Self { base, on_did_change_layout, on_did_change_metadata, on_did_change_cell_state })
    }

    pub fn on_did_change_layout(&self) -> Event<NotebookLayoutChangedEvent> {
        self.on_did_change_layout.event()
    }

    pub fn on_did_change_metadata(&self) -> Event<NotebookMetadataChangedEvent> {
        self.on_did_change_metadata.event()
    }

    pub fn on_did_change_cell_state(&self) -> Event<NotebookCellStateChangedEvent> {
        self.on_did_change_cell_state.event()
    }

    pub fn emit(&self, events: Vec<NotebookViewEvent>) {
        for e in events {
            match e {
                NotebookViewEvent::LayoutChanged(e) => self.on_did_change_layout.fire(e),
                NotebookViewEvent::MetadataChanged(e) => self.on_did_change_metadata.fire(e),
                NotebookViewEvent::CellStateChanged(e) => self.on_did_change_cell_state.fire(e),
            }
        }
    }
}

impl IDisposable for NotebookEventDispatcher {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookEventDispatcher"
    }
}
