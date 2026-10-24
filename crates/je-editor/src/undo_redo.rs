//! Porte de `src/vs/platform/undoRedo/common/undoRedo.ts`: os tipos do serviço de desfazer e refazer.
//!
//! Pendente: o elemento de workspace (`IWorkspaceUndoRedoElement`, várias URIs numa operação só, com
//! `split` e `prepareUndoRedo` assíncrono) e o `registerUriComparisonKeyComputer`. Nenhum consumidor do
//! `TextModel` os usa; entram junto da camada de workspace.
// TODO(integração): mover para o crate je-platform (platform/undoRedo/common).

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use je_base::uri::URI;

/// `IResourceUndoRedoElement`: uma operação de desfazer sobre um recurso.
pub trait UndoRedoElement: Any {
    fn resource(&self) -> URI;
    fn label(&self) -> String;
    fn code(&self) -> String;
    /// `confirmBeforeUndo`.
    fn confirm_before_undo(&self) -> bool {
        false
    }
    fn undo(&self);
    fn redo(&self);
    fn as_any(&self) -> &dyn Any;
}

/// `ResourceEditStackSnapshot`.
#[derive(Clone)]
pub struct ResourceEditStackSnapshot {
    pub resource: URI,
    pub elements: Vec<u64>,
}

thread_local! {
    static GROUP_ID: Cell<u64> = const { Cell::new(1) };
    static SOURCE_ID: Cell<u64> = const { Cell::new(1) };
}

/// `UndoRedoGroup`: elementos do mesmo grupo são desfeitos e refeitos juntos.
///
/// Divergência: o original passa a mesma referência de objeto adiante (o `NotebookTextModel` guarda o grupo num elemento
/// que só é empilhado depois). Aqui o contador de ordem é compartilhado (`Rc`) e o grupo é `Clone`: uma cópia avança
/// a mesma ordem que o original.
#[derive(Clone, Debug)]
pub struct UndoRedoGroup {
    pub id: u64,
    order: Rc<Cell<u64>>,
}

impl UndoRedoGroup {
    pub fn new() -> Self {
        Self {
            id: GROUP_ID.with(|id| {
                let current = id.get();
                id.set(current + 1);
                current
            }),
            order: Rc::new(Cell::new(1)),
        }
    }

    /// `UndoRedoGroup.None`: o grupo 0, que não agrupa nada.
    pub fn none() -> Self {
        Self {
            id: 0,
            order: Rc::new(Cell::new(1)),
        }
    }

    pub fn next_order(&self) -> u64 {
        if self.id == 0 {
            return 0;
        }
        let order = self.order.get();
        self.order.set(order + 1);
        order
    }
}

impl Default for UndoRedoGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// `UndoRedoSource`: quem originou o elemento; desfazer por origem pede confirmação ao trocar.
#[derive(Debug)]
pub struct UndoRedoSource {
    pub id: u64,
    order: Cell<u64>,
}

impl UndoRedoSource {
    pub fn new() -> Self {
        Self {
            id: SOURCE_ID.with(|id| {
                let current = id.get();
                id.set(current + 1);
                current
            }),
            order: Cell::new(1),
        }
    }

    /// `UndoRedoSource.None`.
    pub fn none() -> Self {
        Self {
            id: 0,
            order: Cell::new(1),
        }
    }

    pub fn next_order(&self) -> u64 {
        if self.id == 0 {
            return 0;
        }
        let order = self.order.get();
        self.order.set(order + 1);
        order
    }
}

impl Default for UndoRedoSource {
    fn default() -> Self {
        Self::new()
    }
}
