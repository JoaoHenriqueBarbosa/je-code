//! Porte parcial de `src/vs/workbench/services/panecomposite/browser/panecomposite.ts`: o `IPaneCompositePartService`.
//!
//! Divergências:
//! - O contrato traz só `onDidPaneCompositeOpen`, `onDidPaneCompositeClose` e `getActivePaneComposite`, que o
//!   `MainThreadDocumentAndEditorStateComputer` lê. O resto (`openPaneComposite`, `getPaneComposites`, `getProgressIndicator`,
//!   `hideActivePaneComposite`...) depende do `IPaneComposite` completo (partes de DOM) e entra com o porte das partes.
//! - `IPaneComposite` é o trait [`IPaneComposite`] com o que o computador de estado pergunta do painel: o
//!   `panel instanceof AbstractTextEditor` seguido de `isCodeEditor(panel.getControl())` é
//!   [`IPaneComposite::text_editor_code_control`] (`None` para o painel que não é um editor de texto de código).
//! - O evento `{ composite: IPaneComposite; viewContainerLocation: ViewContainerLocation }` é o [`PaneCompositeEvent`].

use std::rc::Rc;

use je_base::event::Event;
use je_editor::code_editor_widget::CodeEditorWidget;
use je_instantiation::service_id;

use crate::common::views::ViewContainerLocation;

/// `IPaneComposite`, no recorte que o workbench de documentos e editores usa.
pub trait IPaneComposite {
    /// `getId()`.
    fn get_id(&self) -> String;

    /// `panel instanceof AbstractTextEditor && isCodeEditor(panel.getControl())`: o editor de código que o painel
    /// controla, se ele é um editor de texto.
    fn text_editor_code_control(&self) -> Option<Rc<CodeEditorWidget>> {
        None
    }
}

/// O evento de `onDidPaneCompositeOpen` e `onDidPaneCompositeClose`.
#[derive(Clone)]
pub struct PaneCompositeEvent {
    pub composite: Rc<dyn IPaneComposite>,
    pub view_container_location: ViewContainerLocation,
}

/// `IPaneCompositePartService`.
pub trait IPaneCompositePartService {
    /// `onDidPaneCompositeOpen`.
    fn on_did_pane_composite_open(&self) -> Event<PaneCompositeEvent>;

    /// `onDidPaneCompositeClose`.
    fn on_did_pane_composite_close(&self) -> Event<PaneCompositeEvent>;

    /// `getActivePaneComposite(viewContainerLocation)`.
    fn get_active_pane_composite(&self, view_container_location: ViewContainerLocation) -> Option<Rc<dyn IPaneComposite>>;
}

service_id!(pub I_PANE_COMPOSITE_PART_SERVICE: dyn IPaneCompositePartService = "paneCompositePartService");
