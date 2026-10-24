//! Porte de `src/vs/workbench/api/common/extHostNotebookEditor.ts`: o editor de notebook (`vscode.NotebookEditor`) do lado das
//! extensões.
//!
//! Divergências:
//! - `apiEditor` é o envoltório [`NotebookEditor`] (um `Rc` do [`ExtHostNotebookEditor`]); a identidade é
//!   [`NotebookEditor::ptr_eq`]. O `WeakMap` estático `apiEditorsToExtHost` é [`NotebookEditor::ext_host`] (o envoltório
//!   guarda o objeto do host).
//! - Os métodos `_acceptVisibility`, `_acceptVisibleRanges`, `_acceptSelections` e `_acceptViewColumn` perdem o sublinhado
//!   (`accept_visibility`...). O setter de `selections` não confere `Array.isArray` nem `NotebookRange.isNotebookRange`: o
//!   tipo `Vec<NotebookRange>` garante os dois, então ele não lança `illegalArgument('selections')`.
//! - `selection` é `Option` (o `_selections[0]` de uma lista vazia é `undefined`). `replOptions` é `Option<NotebookReplOptions>`.
//! - As promessas dos proxies (`$tryRevealRange`, `$trySetSelections`) não são aguardadas, como no original.
//! - `[Symbol.for('debug.description')]` é `debug_description`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::errors;

use super::ext_host_notebook_document::{ExtHostNotebookDocument, NotebookDocument};
use super::ext_host_protocol::MainThreadNotebookEditorsShape;
use super::ext_host_type_converters as type_converters;
use super::ext_host_types as types;

/// `{ appendIndex: number }`: o `replOptions` de um editor de REPL.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotebookReplOptions {
    pub append_index: f64,
}

/// `ExtHostNotebookEditor`.
pub struct ExtHostNotebookEditor {
    pub id: String,
    proxy: Rc<dyn MainThreadNotebookEditorsShape>,
    pub notebook_data: Rc<ExtHostNotebookDocument>,
    visible_ranges: RefCell<Vec<types::NotebookRange>>,
    selections: RefCell<Vec<types::NotebookRange>>,
    view_column: Cell<Option<f64>>,
    view_type: String,
    visible: Cell<bool>,
}

impl ExtHostNotebookEditor {
    pub fn new(
        id: &str,
        proxy: Rc<dyn MainThreadNotebookEditorsShape>,
        notebook_data: Rc<ExtHostNotebookDocument>,
        visible_ranges: Vec<types::NotebookRange>,
        selections: Vec<types::NotebookRange>,
        view_column: Option<f64>,
        view_type: &str,
    ) -> ExtHostNotebookEditor {
        ExtHostNotebookEditor {
            id: id.to_owned(),
            proxy,
            notebook_data,
            visible_ranges: RefCell::new(visible_ranges),
            selections: RefCell::new(selections),
            view_column: Cell::new(view_column),
            view_type: view_type.to_owned(),
            visible: Cell::new(false),
        }
    }

    /// `apiEditor`.
    pub fn api_editor(self: &Rc<Self>) -> NotebookEditor {
        NotebookEditor(self.clone())
    }

    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    pub fn accept_visibility(&self, value: bool) {
        self.visible.set(value);
    }

    pub fn accept_visible_ranges(&self, value: Vec<types::NotebookRange>) {
        *self.visible_ranges.borrow_mut() = value;
    }

    pub fn accept_selections(&self, selections: Vec<types::NotebookRange>) {
        *self.selections.borrow_mut() = selections;
    }

    fn try_set_selections(&self, value: &[types::NotebookRange]) {
        let _ = self.proxy.rpc_try_set_selections(&self.id, value.iter().map(type_converters::NotebookRange::from).collect());
    }

    pub fn accept_view_column(&self, value: Option<f64>) {
        self.view_column.set(value);
    }
}

/// `vscode.NotebookEditor`.
#[derive(Clone)]
pub struct NotebookEditor(Rc<ExtHostNotebookEditor>);

impl NotebookEditor {
    /// A identidade (`a === b`).
    pub fn ptr_eq(a: &NotebookEditor, b: &NotebookEditor) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// `ExtHostNotebookEditor.apiEditorsToExtHost.get(editor)`.
    pub fn ext_host(&self) -> &Rc<ExtHostNotebookEditor> {
        &self.0
    }

    pub fn notebook(&self) -> NotebookDocument {
        self.0.notebook_data.api_notebook()
    }

    pub fn selection(&self) -> Option<types::NotebookRange> {
        self.0.selections.borrow().first().copied()
    }

    pub fn set_selection(&self, selection: types::NotebookRange) {
        self.set_selections(vec![selection]);
    }

    pub fn selections(&self) -> Vec<types::NotebookRange> {
        self.0.selections.borrow().clone()
    }

    pub fn set_selections(&self, value: Vec<types::NotebookRange>) {
        let selections = if value.is_empty() {
            vec![types::NotebookRange::new(0.0, 0.0).unwrap_or_else(|error| errors::throw(error))]
        } else {
            value
        };
        *self.0.selections.borrow_mut() = selections.clone();
        self.0.try_set_selections(&selections);
    }

    pub fn visible_ranges(&self) -> Vec<types::NotebookRange> {
        self.0.visible_ranges.borrow().clone()
    }

    pub fn reveal_range(&self, range: &types::NotebookRange, reveal_type: Option<types::NotebookEditorRevealType>) {
        let _ = self.0.proxy.rpc_try_reveal_range(
            &self.0.id,
            type_converters::NotebookRange::from(range),
            reveal_type.unwrap_or(types::NotebookEditorRevealType::Default),
        );
    }

    pub fn view_column(&self) -> Option<f64> {
        self.0.view_column.get()
    }

    pub fn repl_options(&self) -> Option<NotebookReplOptions> {
        if self.0.view_type == "repl" {
            return Some(NotebookReplOptions { append_index: self.notebook().cell_count() as f64 - 1.0 });
        }
        None
    }

    pub fn debug_description(&self) -> String {
        format!("NotebookEditor({})", self.notebook().uri().to_string(false))
    }
}
