//! Porte parcial de `src/vs/workbench/services/panecomposite/browser/panecomposite.ts`: o `IPaneCompositePartService`.
//!
//! Divergências:
//! - O contrato traz o que as partes e as ações do workbench usam: os eventos, `getActivePaneComposite`, `openPaneComposite`,
//!   `getRegistryId`, `getPartId`, as listas de identificadores (fixados, visíveis e todos), `hideActivePaneComposite` e
//!   `getLastActivePaneCompositeId`. Ficam de fora `getPaneComposite`, `getPaneComposites` (devolvem o
//!   `PaneCompositeDescriptor` de `browser/panecomposite.ts`, sem porte) e `getProgressIndicator` (precisa do
//!   `IPaneComposite` completo, com partes de DOM) e entram com o porte das partes de painéis.
//! - `openPaneComposite(id: string | undefined, ...)` recebe `Option<&str>`; o `focus?` ausente é `false`.
//! - `IPaneComposite` é o trait [`IPaneComposite`] com o que o computador de estado pergunta do painel: o
//!   `panel instanceof AbstractTextEditor` seguido de `isCodeEditor(panel.getControl())` é
//!   [`IPaneComposite::text_editor_code_control`] (`None` para o painel que não é um editor de texto de código).
//! - O evento `{ composite: IPaneComposite; viewContainerLocation: ViewContainerLocation }` é o [`PaneCompositeEvent`].

use std::rc::Rc;

use je_base::event::Event;
use je_base::promise::Promise;
use je_editor::code_editor_widget::CodeEditorWidget;
use je_instantiation::service_id;

use crate::common::views::ViewContainerLocation;
use crate::services::layout::layout_service::Parts;

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

    /// `getRegistryId(viewContainerLocation)`.
    fn get_registry_id(&self, view_container_location: ViewContainerLocation) -> String;

    /// `getPartId(viewContainerLocation)`.
    fn get_part_id(&self, view_container_location: ViewContainerLocation) -> Parts;

    /// `openPaneComposite(id, viewContainerLocation, focus?)`: abre o painel e passa o foco do teclado se `focus`.
    fn open_pane_composite(&self, id: Option<&str>, view_container_location: ViewContainerLocation, focus: bool) -> Promise<Option<Rc<dyn IPaneComposite>>>;

    /// `getPinnedPaneCompositeIds(viewContainerLocation)`: na ordem visual.
    fn get_pinned_pane_composite_ids(&self, view_container_location: ViewContainerLocation) -> Vec<String>;

    /// `getVisiblePaneCompositeIds(viewContainerLocation)`: na ordem visual.
    fn get_visible_pane_composite_ids(&self, view_container_location: ViewContainerLocation) -> Vec<String>;

    /// `getPaneCompositeIds(viewContainerLocation)`: todos, na ordem visual.
    fn get_pane_composite_ids(&self, view_container_location: ViewContainerLocation) -> Vec<String>;

    /// `hideActivePaneComposite(viewContainerLocation)`.
    fn hide_active_pane_composite(&self, view_container_location: ViewContainerLocation);

    /// `getLastActivePaneCompositeId(viewContainerLocation)`.
    fn get_last_active_pane_composite_id(&self, view_container_location: ViewContainerLocation) -> String;
}

service_id!(pub I_PANE_COMPOSITE_PART_SERVICE: dyn IPaneCompositePartService = "paneCompositePartService");
