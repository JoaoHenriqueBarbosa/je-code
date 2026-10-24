//! Porte da parte lógica de `src/vs/workbench/contrib/files/browser/views/explorerViewer.ts`: o
//! `ICompressedNavigationController` e o `CompressedNavigationController`.
//!
//! Divergências:
//! - **DOM.** A UI é nativa: o `templateData` (`container`, `label.onDidRender`) e os `labels: HTMLElement[]` não existem. O
//!   `updateLabels` (atributos `aria-label`, `aria-level`, `aria-expanded` e a classe `active`) é só apresentação e fica
//!   com o widget que desenhar o rótulo; o controlador guarda o índice, a profundidade e o estado colapsado.
//! - **`static ID`.** Contador de instâncias: [`CompressedNavigationController::next_id`] (o `ID++` do original).

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;

use crate::contrib::files::explorer_model::ExplorerItem;

/// `ICompressedNavigationController`.
pub trait ICompressedNavigationController {
    fn current(&self) -> Rc<ExplorerItem>;
    fn current_id(&self) -> String;
    fn items(&self) -> Vec<Rc<ExplorerItem>>;
    fn index(&self) -> usize;
    fn count(&self) -> usize;
    fn on_did_change(&self) -> Event<()>;
    fn previous(&self);
    fn next(&self);
    fn first(&self);
    fn last(&self);
    fn set_index(&self, index: i64);
    fn update_collapsed(&self, collapsed: bool);
}

thread_local! {
    /// `CompressedNavigationController.ID`.
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

/// `CompressedNavigationController`.
pub struct CompressedNavigationController {
    id: String,
    items: Vec<Rc<ExplorerItem>>,
    index: Cell<usize>,
    depth: usize,
    collapsed: Cell<bool>,
    on_did_change: Rc<Emitter<()>>,
}

impl CompressedNavigationController {
    /// `CompressedNavigationController.ID++`.
    pub fn next_id() -> u64 {
        NEXT_ID.with(|id| {
            let value = id.get();
            id.set(value + 1);
            value
        })
    }

    /// `constructor(id, items, templateData, depth, collapsed)`, sem o `templateData` (ver o topo do módulo).
    pub fn new(id: impl Into<String>, items: Vec<Rc<ExplorerItem>>, depth: usize, collapsed: bool) -> Rc<Self> {
        let index = items.len().saturating_sub(1);
        Rc::new(Self { id: id.into(), items, index: Cell::new(index), depth, collapsed: Cell::new(collapsed), on_did_change: Emitter::new() })
    }

    /// `depth`.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// `collapsed`.
    pub fn collapsed(&self) -> bool {
        self.collapsed.get()
    }
}

impl ICompressedNavigationController for CompressedNavigationController {
    fn current(&self) -> Rc<ExplorerItem> {
        self.items[self.index.get()].clone()
    }

    fn current_id(&self) -> String {
        format!("{}_{}", self.id, self.index.get())
    }

    fn items(&self) -> Vec<Rc<ExplorerItem>> {
        self.items.clone()
    }

    fn index(&self) -> usize {
        self.index.get()
    }

    fn count(&self) -> usize {
        self.items.len()
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn previous(&self) {
        if self.index.get() == 0 {
            return;
        }

        self.set_index(self.index.get() as i64 - 1);
    }

    fn next(&self) {
        if self.index.get() + 1 >= self.items.len() {
            return;
        }

        self.set_index(self.index.get() as i64 + 1);
    }

    fn first(&self) {
        if self.index.get() == 0 {
            return;
        }

        self.set_index(0);
    }

    fn last(&self) {
        if self.index.get() + 1 == self.items.len() {
            return;
        }

        self.set_index(self.items.len() as i64 - 1);
    }

    fn set_index(&self, index: i64) {
        if index < 0 || index >= self.items.len() as i64 {
            return;
        }

        self.index.set(index as usize);
        self.on_did_change.fire(());
    }

    fn update_collapsed(&self, collapsed: bool) {
        self.collapsed.set(collapsed);
    }
}

impl IDisposable for CompressedNavigationController {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}
