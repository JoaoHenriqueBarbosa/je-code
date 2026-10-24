//! Porte de `src/vs/workbench/contrib/notebook/browser/diff/eventDispatcher.ts`: o despachante de eventos de layout do editor
//! de diff de notebooks (`NotebookDiffEditorEventDispatcher`).
//!
//! Divergências:
//! - `NotebookDiffViewEvent` (a união discriminada por `type`) é o enum [`NotebookDiffViewEvent`]; o `type` de cada classe é a
//!   variante, e [`NotebookDiffViewEventType`] existe para quem lê o discriminante (como em `notebook_view_events`).
//! - `Disposable` é o campo `base`; `NotebookDiffEditorEventDispatcher::new` devolve `Rc<Self>`.

use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use super::notebook_diff_editor_browser::IDiffElementLayoutInfo;
use crate::contrib::notebook::browser::notebook_view_events::{NotebookLayoutChangeEvent, NotebookLayoutInfo};

/// `NotebookDiffViewEventType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookDiffViewEventType {
    LayoutChanged = 1,
    CellLayoutChanged = 2,
}

/// `NotebookDiffLayoutChangedEvent`.
#[derive(Clone, Debug)]
pub struct NotebookDiffLayoutChangedEvent {
    pub source: NotebookLayoutChangeEvent,
    pub value: NotebookLayoutInfo,
}

/// `NotebookCellLayoutChangedEvent`.
#[derive(Clone, Debug)]
pub struct NotebookCellLayoutChangedEvent {
    pub source: IDiffElementLayoutInfo,
}

/// `NotebookDiffViewEvent`.
#[derive(Clone, Debug)]
pub enum NotebookDiffViewEvent {
    LayoutChanged(NotebookDiffLayoutChangedEvent),
    CellLayoutChanged(NotebookCellLayoutChangedEvent),
}

impl NotebookDiffViewEvent {
    /// O campo `type` do original.
    pub fn type_(&self) -> NotebookDiffViewEventType {
        match self {
            NotebookDiffViewEvent::LayoutChanged(_) => NotebookDiffViewEventType::LayoutChanged,
            NotebookDiffViewEvent::CellLayoutChanged(_) => NotebookDiffViewEventType::CellLayoutChanged,
        }
    }
}

/// `NotebookDiffEditorEventDispatcher`.
pub struct NotebookDiffEditorEventDispatcher {
    base: Rc<Disposable>,
    on_did_change_layout: Rc<Emitter<NotebookDiffLayoutChangedEvent>>,
    on_did_change_cell_layout: Rc<Emitter<NotebookCellLayoutChangedEvent>>,
}

impl NotebookDiffEditorEventDispatcher {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_layout = Emitter::<NotebookDiffLayoutChangedEvent>::new();
        let on_did_change_cell_layout = Emitter::<NotebookCellLayoutChangedEvent>::new();
        base.register(on_did_change_layout.clone());
        base.register(on_did_change_cell_layout.clone());
        Rc::new(Self { base, on_did_change_layout, on_did_change_cell_layout })
    }

    pub fn on_did_change_layout(&self) -> Event<NotebookDiffLayoutChangedEvent> {
        self.on_did_change_layout.event()
    }

    pub fn on_did_change_cell_layout(&self) -> Event<NotebookCellLayoutChangedEvent> {
        self.on_did_change_cell_layout.event()
    }

    pub fn emit(&self, events: Vec<NotebookDiffViewEvent>) {
        for event in events {
            match event {
                NotebookDiffViewEvent::LayoutChanged(event) => self.on_did_change_layout.fire(event),
                NotebookDiffViewEvent::CellLayoutChanged(event) => self.on_did_change_cell_layout.fire(event),
            }
        }
    }
}

impl IDisposable for NotebookDiffEditorEventDispatcher {
    fn dispose(&self) {
        self.base.dispose();
    }
}
