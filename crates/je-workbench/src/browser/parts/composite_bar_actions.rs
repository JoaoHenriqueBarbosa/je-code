//! Porte parcial de `src/vs/workbench/browser/parts/compositeBarActions.ts`: a `SwitchCompositeViewAction`, base das ações
//! `workbench.action.previousSideBarView` e `workbench.action.nextSideBarView` da barra de atividades.
//!
//! Divergências:
//! - O resto do arquivo (`CompositeBarAction`, `CompositeActionViewItem`, `ToggleCompositePinnedAction`...) monta DOM e
//!   entra com a barra de painéis nativa.
//! - `Action2` não é herdada: [`SwitchCompositeViewAction`] guarda o [`Action2Options`] do `desc` e implementa o trait.
//! - `await paneCompositeService.openPaneComposite(...)` não espera: o [`Promise`] devolvido fica com o serviço, que o
//!   resolve e trata a rejeição.

use je_base::dynamic::Dynamic;
use je_instantiation::ServicesAccessor;
use je_platform::actions::actions::{Action2, Action2Options};

use crate::common::views::ViewContainerLocation;
use crate::services::panecomposite::panecomposite::I_PANE_COMPOSITE_PART_SERVICE;

/// `SwitchCompositeViewAction`.
pub struct SwitchCompositeViewAction {
    desc: Action2Options,
    location: ViewContainerLocation,
    offset: i64,
}

impl SwitchCompositeViewAction {
    pub fn new(desc: Action2Options, location: ViewContainerLocation, offset: i64) -> Self {
        Self { desc, location, offset }
    }

    /// O identificador do painel que fica `offset` posições adiante do ativo, em volta da lista de visíveis; `None` se o
    /// ativo não está entre eles.
    pub fn target_composite_id(visible_composite_ids: &[String], active_composite_id: &str, offset: i64) -> Option<String> {
        let len = visible_composite_ids.len() as i64;
        let index = visible_composite_ids.iter().position(|id| id == active_composite_id)? as i64;
        Some(visible_composite_ids[((index + len + offset) % len) as usize].clone())
    }
}

impl Action2 for SwitchCompositeViewAction {
    fn desc(&self) -> Action2Options {
        self.desc.clone()
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let pane_composite_service = accessor.get(I_PANE_COMPOSITE_PART_SERVICE);

        let Some(active_composite) = pane_composite_service.get_active_pane_composite(self.location) else {
            return;
        };

        let visible_composite_ids = pane_composite_service.get_visible_pane_composite_ids(self.location);
        if let Some(target_composite_id) = Self::target_composite_id(&visible_composite_ids, &active_composite.get_id(), self.offset) {
            let _ = pane_composite_service.open_pane_composite(Some(&target_composite_id), self.location, true);
        }
    }
}
