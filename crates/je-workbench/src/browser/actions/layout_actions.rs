//! Porte parcial de `src/vs/workbench/browser/actions/layoutActions.ts`: as duas ações que a barra de atividades dispara pelo
//! menu de contexto, `ToggleSidebarPositionAction` e `ToggleSidebarVisibilityAction`, com o rótulo e o registro delas.
//!
//! Divergências:
//! - O resto do arquivo (mover a barra lateral para um lado, o editor, o painel, a barra lateral secundária, o modo Zen, o
//!   centro do layout, os ícones e os itens de menu que dependem de `registerIcon`) não é portado aqui; cada ação entra com
//!   a parte que ela governa.
//! - `alert(message)` de `aria` é o `alert` do `IAccessibilityService`.
//! - `ToggleSidebarPositionAction.run` devolve a `Promise` do `updateValue`; `Action2::run` descarta o retorno, então o
//!   resultado fica no `configurationService`.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{DisposableRef, combined_disposable};
use je_base::nls::localize;
use je_instantiation::ServicesAccessor;
use je_platform::accessibility::I_ACCESSIBILITY_SERVICE;
use je_platform::action::action_common_categories::Categories;
use je_platform::action::{Category, CommandAction, CommandActionTitle, CommandActionToggleInfo, Title, Toggled, localize2};
use je_platform::actions::actions::{Action2, Action2Keybinding, Action2MenuItem, Action2Options, register_action2};
use je_platform::actions::menu_ids;
use je_platform::commands::CommandMetadata;
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::common::contextkeys::{is_sessions_window_context, side_bar_visible_context};
use crate::services::layout::layout_service::{I_WORKBENCH_LAYOUT_SERVICE, IWorkbenchLayoutService, Parts, Position};

/// `sidebarPositionConfigurationKey`.
pub const SIDEBAR_POSITION_CONFIGURATION_KEY: &str = "workbench.sideBar.location";

/// `ToggleSidebarPositionAction`.
pub struct ToggleSidebarPositionAction;

impl ToggleSidebarPositionAction {
    pub const ID: &'static str = "workbench.action.toggleSidebarPosition";

    /// `ToggleSidebarPositionAction.LABEL`.
    pub fn label() -> String {
        localize("toggleSidebarPosition", "Toggle Primary Side Bar Position", &[])
    }

    /// `ToggleSidebarPositionAction.getLabel(layoutService)`.
    pub fn get_label(layout_service: &dyn IWorkbenchLayoutService) -> String {
        if layout_service.get_side_bar_position() == Position::Left {
            localize("moveSidebarRight", "Move Primary Side Bar Right", &[])
        } else {
            localize("moveSidebarLeft", "Move Primary Side Bar Left", &[])
        }
    }
}

impl Action2 for ToggleSidebarPositionAction {
    fn desc(&self) -> Action2Options {
        let title = localize2("toggleSidebarPosition", "Toggle Primary Side Bar Position");
        Action2Options {
            command: CommandAction {
                category: Some(Category::Localized(Categories::new().view)),
                precondition: Some(is_sessions_window_context().to_negated()),
                ..CommandAction::new(Self::ID, Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None }))
            },
            menu: Vec::new(),
            keybinding: Vec::new(),
            f1: true,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let layout_service = accessor.get(I_WORKBENCH_LAYOUT_SERVICE);
        let configuration_service = accessor.get(I_CONFIGURATION_SERVICE);

        let new_position_value = if layout_service.get_side_bar_position() == Position::Left { "right" } else { "left" };
        let _ = configuration_service.update_value(SIDEBAR_POSITION_CONFIGURATION_KEY, Dynamic::string(new_position_value), None, None);
    }
}

/// `ToggleSidebarVisibilityAction`.
pub struct ToggleSidebarVisibilityAction;

impl ToggleSidebarVisibilityAction {
    pub const ID: &'static str = "workbench.action.toggleSidebarVisibility";

    /// `ToggleSidebarVisibilityAction.LABEL`.
    pub fn label() -> String {
        localize("compositePart.hideSideBarLabel", "Hide Primary Side Bar", &[])
    }
}

impl Action2 for ToggleSidebarVisibilityAction {
    fn desc(&self) -> Action2Options {
        let title = localize2("toggleSidebar", "Toggle Primary Side Bar Visibility");
        let menu_item = |id, group: &str, order: f64| Action2MenuItem {
            id,
            null_precondition: false,
            alt: None,
            when: None,
            group: Some(group.to_owned()),
            order: Some(order),
            is_hidden_by_default: None,
        };
        Action2Options {
            command: CommandAction {
                category: Some(Category::Localized(Categories::new().view)),
                metadata: Some(CommandMetadata::new(localize("openAndCloseSidebar", "Open/Show and Close/Hide Sidebar", &[]).as_str())),
                toggled: Some(Toggled::Info(CommandActionToggleInfo {
                    condition: side_bar_visible_context().expr(),
                    icon: None,
                    tooltip: None,
                    title: Some(localize("primary sidebar", "Primary Side Bar", &[])),
                    mnemonic_title: Some(localize("primary sidebar mnemonic", "&&Primary Side Bar", &[])),
                })),
                ..CommandAction::new(Self::ID, Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None }))
            },
            menu: vec![
                menu_item(menu_ids::layout_control_menu_submenu(), "0_workbench_layout", 0.0),
                menu_item(menu_ids::menubar_appearance_menu(), "2_workbench_layout", 1.0),
            ],
            keybinding: vec![Action2Keybinding {
                keybindings: Keybindings { primary: Some(KeyMod::CTRL_CMD | KeyCode::KeyB.bits()), ..Keybindings::default() },
                weight: KeybindingWeight::WORKBENCH_CONTRIB,
                args: Dynamic::Undefined,
                when: None,
            }],
            f1: true,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let layout_service = accessor.get(I_WORKBENCH_LAYOUT_SERVICE);
        let is_currently_visible = layout_service.is_visible(Parts::SidebarPart, None);

        layout_service.set_part_hidden(is_currently_visible, Parts::SidebarPart);

        // Announce visibility change to screen readers
        let alert_message = if is_currently_visible {
            localize("sidebarHidden", "Primary Side Bar hidden", &[])
        } else {
            localize("sidebarVisible", "Primary Side Bar shown", &[])
        };
        accessor.get(I_ACCESSIBILITY_SERVICE).alert(&alert_message);
    }
}

/// Os dois `registerAction2` desta porção de `layoutActions.ts`.
pub fn register_layout_actions() -> DisposableRef {
    combined_disposable(vec![register_action2(Rc::new(ToggleSidebarPositionAction)), register_action2(Rc::new(ToggleSidebarVisibilityAction))])
}
