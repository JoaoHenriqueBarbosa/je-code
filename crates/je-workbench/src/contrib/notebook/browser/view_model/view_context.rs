//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/viewContext.ts`: o que um view model de célula lê do editor
//! (`ViewContext`).
//!
//! Divergências:
//! - `IBaseCellEditorOptions` (em `notebookBrowser.ts`) é o trait [`IBaseCellEditorOptions`] com só o que os view models
//!   leem do `value: IEditorOptions`, que é `wordWrap` (`estimateEditorHeight`).
//! - `readonly getBaseCellEditorOptions: (language: string) => IBaseCellEditorOptions` é um `Box<dyn Fn>` e o acessor é
//!   [`ViewContext::get_base_cell_editor_options`].

use std::rc::Rc;

use super::event_dispatcher::NotebookEventDispatcher;
use crate::contrib::notebook::browser::notebook_options::NotebookOptions;

/// `IBaseCellEditorOptions`, só com o `value.wordWrap`.
pub trait IBaseCellEditorOptions {
    /// `value.wordWrap` (`'off' | 'on' | 'wordWrapColumn' | 'bounded'`).
    fn word_wrap(&self) -> String;
}

/// `ViewContext`.
pub struct ViewContext {
    pub notebook_options: Rc<NotebookOptions>,
    pub event_dispatcher: Rc<NotebookEventDispatcher>,
    get_base_cell_editor_options: Box<dyn Fn(&str) -> Rc<dyn IBaseCellEditorOptions>>,
}

impl ViewContext {
    pub fn new(
        notebook_options: Rc<NotebookOptions>,
        event_dispatcher: Rc<NotebookEventDispatcher>,
        get_base_cell_editor_options: impl Fn(&str) -> Rc<dyn IBaseCellEditorOptions> + 'static,
    ) -> Rc<Self> {
        Rc::new(Self { notebook_options, event_dispatcher, get_base_cell_editor_options: Box::new(get_base_cell_editor_options) })
    }

    pub fn get_base_cell_editor_options(&self, language: &str) -> Rc<dyn IBaseCellEditorOptions> {
        (self.get_base_cell_editor_options)(language)
    }
}
