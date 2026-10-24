//! Porte de `src/vs/editor/common/services/treeViewsDnd.ts`: o serviço que guarda, por operação de arrastar, a
//! promessa da transferência de dados que o host de extensões montou, para o `drop` da mesma árvore reaproveitar os
//! itens originais em vez de recriá-los a partir do DTO.
//!
//! Divergências:
//! - `TreeViewsDnDService<T>` é genérico no `T` da transferência, como o original; a promessa guarda `Option<T>`
//!   (`Promise<T | undefined>`), então `T` precisa ser [`Clone`] (no JS o objeto circula por referência; aqui é um
//!   `Rc`).
//! - O `Map<string, Promise<T | undefined>>` privado (`_dragOperations`) é lido pelos testes do `ExtHostTreeViews`
//!   por `Reflect.get`. Aqui os métodos [`TreeViewsDnDService::drag_operations`] e
//!   [`TreeViewsDnDService::drag_operation`] expõem o mesmo conteúdo, na ordem de inserção do `Map`.
//! - `_serviceBrand` não existe: o contrato é o trait [`ITreeViewsDnDService`].

use std::cell::RefCell;

use je_base::promise::Promise;

/// `ITreeViewsDnDService<T>`.
pub trait ITreeViewsDnDService<T: Clone + 'static> {
    /// `removeDragOperationTransfer(uuid)`: tira a operação do serviço e a devolve (uma `uuid` vazia ou desconhecida
    /// não devolve nada).
    fn remove_drag_operation_transfer(&self, uuid: Option<&str>) -> Option<Promise<Option<T>>>;

    /// `addDragOperationTransfer(uuid, transferPromise)`.
    fn add_drag_operation_transfer(&self, uuid: &str, transfer_promise: Promise<Option<T>>);
}

/// `TreeViewsDnDService<T>`.
pub struct TreeViewsDnDService<T: Clone + 'static> {
    /// `_dragOperations`, com a ordem de inserção de um `Map` (regravar uma chave existente mantém a posição).
    drag_operations: RefCell<Vec<(String, Promise<Option<T>>)>>,
}

impl<T: Clone + 'static> Default for TreeViewsDnDService<T> {
    fn default() -> Self {
        Self { drag_operations: RefCell::new(Vec::new()) }
    }
}

impl<T: Clone + 'static> TreeViewsDnDService<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// O conteúdo de `_dragOperations`, na ordem de inserção.
    pub fn drag_operations(&self) -> Vec<(String, Promise<Option<T>>)> {
        self.drag_operations.borrow().clone()
    }

    /// `_dragOperations.get(uuid)`.
    pub fn drag_operation(&self, uuid: &str) -> Option<Promise<Option<T>>> {
        self.drag_operations.borrow().iter().find(|(key, _)| key == uuid).map(|(_, operation)| operation.clone())
    }
}

impl<T: Clone + 'static> ITreeViewsDnDService<T> for TreeViewsDnDService<T> {
    fn remove_drag_operation_transfer(&self, uuid: Option<&str>) -> Option<Promise<Option<T>>> {
        let uuid = uuid.filter(|uuid| !uuid.is_empty())?;
        let mut operations = self.drag_operations.borrow_mut();
        let index = operations.iter().position(|(key, _)| key == uuid)?;
        Some(operations.remove(index).1)
    }

    fn add_drag_operation_transfer(&self, uuid: &str, transfer_promise: Promise<Option<T>>) {
        let mut operations = self.drag_operations.borrow_mut();
        match operations.iter_mut().find(|(key, _)| key == uuid) {
            Some(entry) => entry.1 = transfer_promise,
            None => operations.push((uuid.to_owned(), transfer_promise)),
        }
    }
}

/// `DraggedTreeItemsIdentifier`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraggedTreeItemsIdentifier {
    pub identifier: String,
}

impl DraggedTreeItemsIdentifier {
    pub fn new(identifier: impl Into<String>) -> Self {
        Self { identifier: identifier.into() }
    }
}
