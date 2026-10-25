//! Porte de `src/vs/workbench/test/browser/componentFixtures/fixtureFocus.ts`: o serviço de interação do usuário dos
//! fixtures de componentes (foco simulado por padrão) e a regra de dar o foco ao alvo do fixture.
//!
//! Notas de tradução:
//! - Os parâmetros com valor padrão do original (`overrideFocus = true`, `simulateHover = false`) são obrigatórios aqui;
//!   quem chamava sem argumentos passa `true, false`.
//! - A subclasse anônima do `UserInteractionService` que só sobrescreve o `createHoverTracker` é o campo de hover simulado
//!   do [`UserInteractionService`] (ver `platform/user_interaction`).
//! - `{ focus(): void }` é o trait [`Focusable`], que o nó da árvore nativa ([`UiElement`]) implementa com o foco da `Ui`.

use std::rc::Rc;

use crate::platform::user_interaction::user_interaction_service::{IUserInteractionService, MockUserInteractionService, UiElement, UserInteractionService};

/// `{ focus(): void }`: o que o fixture sabe fazer com o alvo do foco.
pub trait Focusable {
    fn focus(&self);
}

impl Focusable for UiElement {
    /// `element.focus()`: o nó recebe o foco da `Ui` (o `Blur` do anterior e o `Focus` deste são despachados).
    fn focus(&self) {
        self.ui().focus(self.node());
    }
}

/// `createFixtureUserInteractionService(overrideFocus, simulateHover)`: com `override_focus` o foco é simulado como
/// verdadeiro (o fixture fica determinístico); sem ele, o foco acompanha a `Ui`. O hover é simulado em ambos quando
/// `simulate_hover`.
pub fn create_fixture_user_interaction_service(override_focus: bool, simulate_hover: bool) -> Rc<dyn IUserInteractionService> {
    if override_focus {
        let service: Rc<dyn IUserInteractionService> = MockUserInteractionService::new(true, simulate_hover);
        return service;
    }
    let service: Rc<dyn IUserInteractionService> = UserInteractionService::with_simulated_hover(simulate_hover);
    service
}

/// `applyFixtureFocus(overrideFocus, target)`: só mexe no foco quando o fixture o controla.
pub fn apply_fixture_focus(override_focus: bool, target: &dyn Focusable) {
    if override_focus {
        target.focus();
    }
}
