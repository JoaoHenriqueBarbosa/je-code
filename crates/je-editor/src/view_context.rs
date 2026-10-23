//! Porte de `src/vs/editor/common/viewModel/viewContext.ts`: `ViewContext`, o que cada parte da visão recebe
//! (configuração, modelo de visão, layout e tema).
//!
//! Divergências:
//! - `IViewModel` tem tipos associados (ver `view_model_types.rs`), então não serve como objeto de trait: o
//!   `ViewContext` é genérico no modelo de visão (`VM`) e guarda um `Rc<VM>`. O `ViewEventHandler` que
//!   `addEventHandler` recebe é o tipo associado `VM::EventHandler`, em `Rc`.
//! - `IEditorConfiguration` é o trait [`EditorConfiguration`] de `cursor_common.rs` (só as opções calculadas),
//!   em `Rc<dyn _>`; `IViewLayout` é `Rc<dyn IViewLayout>`, o que `IViewModel::view_layout` devolve.
//! - `IColorTheme` é o `je_theme::ColorTheme` em `Rc` (ver `editor_theme.rs`).
//! - `addEventHandler` e `removeEventHandler` são repasses ao modelo de visão no original e ficam, porque
//!   são a API do contexto que as partes da visão chamam.

use std::rc::Rc;

use je_theme::ColorTheme;

use crate::cursor_common::EditorConfiguration;
use crate::editor_theme::EditorTheme;
use crate::view_model_types::{IViewLayout, IViewModel};

/// `ViewContext`.
pub struct ViewContext<VM: IViewModel> {
    pub configuration: Rc<dyn EditorConfiguration>,
    pub view_model: Rc<VM>,
    pub view_layout: Rc<dyn IViewLayout>,
    pub theme: EditorTheme,
}

impl<VM: IViewModel> ViewContext<VM> {
    pub fn new(configuration: Rc<dyn EditorConfiguration>, theme: Rc<ColorTheme>, model: Rc<VM>) -> Self {
        let view_layout = model.view_layout();
        Self { configuration, theme: EditorTheme::new(theme), view_model: model, view_layout }
    }

    pub fn add_event_handler(&self, event_handler: Rc<VM::EventHandler>) {
        self.view_model.add_view_event_handler(event_handler);
    }

    pub fn remove_event_handler(&self, event_handler: &Rc<VM::EventHandler>) {
        self.view_model.remove_view_event_handler(event_handler);
    }
}
