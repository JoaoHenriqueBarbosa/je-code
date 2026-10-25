//! Contrato de `src/vs/workbench/browser/parts/titlebar/menubarControl.ts` no recorte que a barra de atividades usa: o
//! `CustomMenubarControl` instalado no menu compacto (`window.menuBarVisibility: compact`).
//!
//! Divergências:
//! - **`CustomMenubarControl` como trait.** A classe do original monta o `MenuBar` de `base/browser/ui/menu/menubar.ts` num
//!   `HTMLElement`; aqui a barra de atividades só depende do que chama dela: `create(container)` e `toggleFocus()`. O
//!   `createInstance(CustomMenubarControl)` é a [`MenubarFactory`] injetada. O `MenuBar` nativo está em
//!   `je_widgets::menu_bar`; a ligação dele aos menus do `MenuId.MenubarMain` (o corpo de `CustomMenubarControl`) entra com o
//!   porte da barra de título.
//! - **`getMenuBarVisibility(configurationService)`** (`platform/window/common/window.ts`) já está no [`TitleBarHost`] como
//!   `get_menu_bar_visibility`; as constantes dos valores estão em [`MenuBarVisibility`].
//!
//! [`TitleBarHost`]: crate::services::layout::layout_service::TitleBarHost

use std::rc::Rc;

use je_base::lifecycle::IDisposable;

use crate::browser::parts::activitybar::activitybar_part::MenuBarContainer;

/// `MenuSettings.MenuBarVisibility`.
pub const MENU_BAR_VISIBILITY_SETTING: &str = "window.menuBarVisibility";

/// `MenuBarVisibility`: `'classic' | 'visible' | 'toggle' | 'hidden' | 'compact'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuBarVisibility {
    Classic,
    Visible,
    Toggle,
    Hidden,
    Compact,
}

impl MenuBarVisibility {
    /// O texto da configuração; `None` para um valor desconhecido.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "classic" => Some(Self::Classic),
            "visible" => Some(Self::Visible),
            "toggle" => Some(Self::Toggle),
            "hidden" => Some(Self::Hidden),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }
}

/// O `CustomMenubarControl` que a barra de atividades instala.
pub trait ICustomMenubarControl: IDisposable {
    /// `create(parent)`.
    fn create(&self, parent: &Rc<MenuBarContainer>);

    /// `toggleFocus()`.
    fn toggle_focus(&self);
}

/// `instantiationService.createInstance(CustomMenubarControl)`.
pub type MenubarFactory = Rc<dyn Fn() -> Rc<dyn ICustomMenubarControl>>;
