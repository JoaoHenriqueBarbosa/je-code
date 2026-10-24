//! Porte de `src/vs/workbench/common/editor/editorModel.ts`: o `EditorModel`, a contraparte pesada da entrada de editor.
//! Dependendo da entrada, ele resolve a partir de um sistema de arquivos, busca o conteúdo e pode permitir salvar ou
//! reverter. Os modelos de editor costumam ficar em cache por um tempo porque são caros de construir.
//!
//! Divergências:
//! - **Herança.** `EditorModel` é a struct [`EditorModel`] (o estado e o comportamento da classe) e o contrato que quem
//!   consome um modelo lê dele é o trait [`IEditorModel`] (que cobre também `IResolvableEditorModel`, de
//!   `platform/editor/common/editor.ts`: `resolve` e `isResolved`). Uma subclasse guarda um [`EditorModel`] e implementa o
//!   trait sobrescrevendo o que o original sobrescreve (ver `DiffEditorModel`).
//! - **`isDisposed`** lê o `DisposableStore` do `Disposable` base, como o `this._store.isDisposed` do original.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;

/// O contrato de um modelo de editor: `EditorModel` e `IResolvableEditorModel` (`IDisposable` com `resolve` e `isResolved`).
pub trait IEditorModel: IDisposable {
    /// Para o `instanceof` do original: o tipo concreto do modelo.
    fn as_any(&self) -> &dyn Any;

    /// `onWillDispose`.
    fn on_will_dispose(&self) -> Event<()>;

    /// Causes this model to resolve returning a promise when loading is completed.
    fn resolve(&self) -> Promise<()>;

    /// Returns whether this model was loaded or not.
    fn is_resolved(&self) -> bool;

    /// Find out if this model has been disposed.
    fn is_disposed(&self) -> bool;
}

/// `EditorModel`.
pub struct EditorModel {
    store: Rc<DisposableStore>,
    on_will_dispose: Rc<Emitter<()>>,
    resolved: Cell<bool>,
}

impl EditorModel {
    /// `new EditorModel()`.
    pub fn new() -> Rc<Self> {
        Rc::new(Self::new_inner())
    }

    /// O estado de um `EditorModel`, para as subclasses guardarem (o `super()` do construtor).
    pub fn new_inner() -> Self {
        let store = DisposableStore::new();
        let on_will_dispose = Emitter::new();
        store.add(on_will_dispose.clone());
        Self { store, on_will_dispose, resolved: Cell::new(false) }
    }
}

impl IDisposable for EditorModel {
    /// Subclasses should implement to free resources that have been claimed through loading.
    fn dispose(&self) {
        self.on_will_dispose.fire(());

        self.store.dispose();
    }
}

impl IEditorModel for EditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    fn resolve(&self) -> Promise<()> {
        self.resolved.set(true);
        Promise::resolved(())
    }

    fn is_resolved(&self) -> bool {
        self.resolved.get()
    }

    fn is_disposed(&self) -> bool {
        self.store.is_disposed()
    }
}
