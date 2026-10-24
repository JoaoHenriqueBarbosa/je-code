//! Porte de `EditorDecorationsCollection` (`src/vs/editor/browser/widget/codeEditor/codeEditorWidget.ts`), a classe
//! que o `createDecorationsCollection` do editor devolve (`IEditorDecorationsCollection`, de `editorCommon.ts`): uma
//! coleção de decorações do editor, que o dono troca (`set`), acrescenta (`append`) e esvazia (`clear`) sem perder o
//! controle de quais ids são dele.
//!
//! Divergências:
//! - O original recebe o `ICodeEditor` e o guarda; aqui a coleção guarda um [`Weak`] do [`CodeEditorWidget`], porque
//!   quem a cria é, em geral, uma contribuição que o próprio editor possui (um `Rc` formaria um ciclo). Sem o editor
//!   (já solto), a coleção se comporta como um editor sem modelo: `get_range` devolve `None`, `get_ranges` uma lista
//!   vazia e `set`/`append` não alteram nada.
//! - `createDecorationsCollection(decorations?)` do editor é [`EditorDecorationsCollection::new`], porque o
//!   `CodeEditorWidget` não tem o método (a coleção depende do trait `ICodeEditor`, que ela só consome).
//! - `onDidChange(listener, thisArgs, disposables)`: sem `thisArgs` (o fecho já captura o que precisar) e sem a lista
//!   `disposables` (quem recebe o `DisposableRef` o registra onde quiser).
//! - O `try`/`finally` que desliga `_isChangingDecorations` é um guarda de escopo (`Drop`), que também o desliga se o
//!   `deltaDecorations` entrar em pânico.
//! - O estado `_isChangingDecorations` é um `Rc<Cell<bool>>` porque o ouvinte de `onDidChange`, criado a partir de
//!   `&self`, precisa dele depois de a chamada voltar.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::lifecycle::{DisposableRef, to_disposable};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::model::{ModelDecoration, ModelDeltaDecoration};
use crate::range::Range;
use crate::text_model_events::ModelDecorationsChangedEvent;

/// Desliga `_isChangingDecorations` ao sair do escopo (o `finally` do original).
struct ChangingScope<'a>(&'a Cell<bool>);

impl<'a> ChangingScope<'a> {
    fn enter(flag: &'a Cell<bool>) -> Self {
        flag.set(true);
        Self(flag)
    }
}

impl Drop for ChangingScope<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// `EditorDecorationsCollection`.
pub struct EditorDecorationsCollection {
    editor: Weak<CodeEditorWidget>,
    decoration_ids: RefCell<Vec<String>>,
    is_changing_decorations: Rc<Cell<bool>>,
}

impl EditorDecorationsCollection {
    /// `new EditorDecorationsCollection(editor, decorations)` (e o `createDecorationsCollection(decorations?)` do editor):
    /// só aplica as decorações iniciais se houver alguma.
    pub fn new(editor: &Rc<CodeEditorWidget>, decorations: Option<&[ModelDeltaDecoration]>) -> Self {
        let collection = Self {
            editor: Rc::downgrade(editor),
            decoration_ids: RefCell::new(Vec::new()),
            is_changing_decorations: Rc::new(Cell::new(false)),
        };
        if let Some(decorations) = decorations.filter(|decorations| !decorations.is_empty()) {
            collection.set(decorations);
        }
        collection
    }

    /// `length`.
    pub fn length(&self) -> usize {
        self.decoration_ids.borrow().len()
    }

    /// `onDidChange(listener)`: avisa as mudanças de decoração que não vêm da própria coleção.
    pub fn on_did_change(&self, listener: impl Fn(&ModelDecorationsChangedEvent) + 'static) -> DisposableRef {
        let Some(editor) = self.editor.upgrade() else {
            return to_disposable(|| {});
        };
        let is_changing_decorations = Rc::clone(&self.is_changing_decorations);
        editor.on_did_change_model_decorations().listen(move |e| {
            if is_changing_decorations.get() {
                return;
            }
            listener(e);
        })
    }

    /// `getRange(index)`.
    pub fn get_range(&self, index: usize) -> Option<Range> {
        let model = self.editor.upgrade()?.get_model()?;
        let decoration_ids = self.decoration_ids.borrow();
        if index >= decoration_ids.len() {
            return None;
        }
        model.get_decoration_range(&decoration_ids[index])
    }

    /// `getRanges()`.
    pub fn get_ranges(&self) -> Vec<Range> {
        let Some(model) = self.editor.upgrade().and_then(|editor| editor.get_model()) else {
            return Vec::new();
        };
        self.decoration_ids.borrow().iter().filter_map(|decoration_id| model.get_decoration_range(decoration_id)).collect()
    }

    /// `has(decoration)`.
    pub fn has(&self, decoration: &ModelDecoration) -> bool {
        self.decoration_ids.borrow().contains(&decoration.id)
    }

    /// `clear()`.
    pub fn clear(&self) {
        if self.decoration_ids.borrow().is_empty() {
            // nothing to do
            return;
        }
        self.set(&[]);
    }

    /// `set(newDecorations)`: troca todas as decorações da coleção e devolve os ids novos.
    pub fn set(&self, new_decorations: &[ModelDeltaDecoration]) -> Vec<String> {
        if let Some(editor) = self.editor.upgrade() {
            let _scope = ChangingScope::enter(&self.is_changing_decorations);
            editor.change_decorations(|accessor| {
                let old_ids = self.decoration_ids.borrow().clone();
                let new_ids = accessor.delta_decorations(&old_ids, new_decorations);
                *self.decoration_ids.borrow_mut() = new_ids;
            });
        }
        self.decoration_ids.borrow().clone()
    }

    /// `append(newDecorations)`: acrescenta decorações à coleção e devolve só os ids novos.
    pub fn append(&self, new_decorations: &[ModelDeltaDecoration]) -> Vec<String> {
        let mut new_decoration_ids = Vec::new();
        if let Some(editor) = self.editor.upgrade() {
            let _scope = ChangingScope::enter(&self.is_changing_decorations);
            editor.change_decorations(|accessor| {
                new_decoration_ids = accessor.delta_decorations(&[], new_decorations);
                self.decoration_ids.borrow_mut().extend(new_decoration_ids.iter().cloned());
            });
        }
        new_decoration_ids
    }
}
