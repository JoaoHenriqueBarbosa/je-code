//! Porte parcial de `src/vs/platform/actions/browser/menuEntryActionViewItem.ts`: só a lógica pura de modelo que distribui os
//! grupos de um menu em ações primárias e secundárias (`PrimaryAndSecondaryActions`, `fillInActions`,
//! `getContextMenuActions` e `getFlatContextMenuActions`). Os view items (`MenuEntryActionViewItem`,
//! `SubmenuEntryActionViewItem`...) dependem de widgets e ficam fora: a UI é desenhada pela árvore nativa.
//!
//! Divergência: `shouldInlineSubmenu` e `useSeparatorsInPrimaryActions` de `fillInActionBarActions` não entram, porque
//! `getContextMenuActions` e `getFlatContextMenuActions` usam os valores padrão (nunca inline, sem separador no primário).

use je_base::actions::{ActionRef, Separator};

use super::actions::{MenuAction, MenuActionGroup};

/// `PrimaryAndSecondaryActions`.
#[derive(Default)]
pub struct PrimaryAndSecondaryActions {
    pub primary: Vec<ActionRef>,
    pub secondary: Vec<ActionRef>,
}

/// O `IAction` que representa a entrada do menu: o item, ou a `SubmenuAction` por baixo do submenu.
fn menu_action_as_action(action: &MenuAction) -> ActionRef {
    match action {
        MenuAction::Item(item) => item.clone(),
        MenuAction::Submenu(submenu) => submenu.as_action(),
    }
}

/// `fillInActions` com os padrões do original (`shouldInlineSubmenu` sempre falso, sem separador entre os grupos primários).
/// O alvo é o par primário e secundário; `flat` (o alvo `IAction[]`) junta os dois baldes no secundário.
fn fill_in_actions(groups: &[MenuActionGroup], is_primary_action: &dyn Fn(&str) -> bool, flat: bool) -> PrimaryAndSecondaryActions {
    let mut primary: Vec<ActionRef> = Vec::new();
    let mut secondary: Vec<ActionRef> = Vec::new();

    for (group, actions) in groups {
        let to_primary = is_primary_action(group);
        // Com o alvo achatado, os dois baldes são a mesma lista: o separador entra quando ela já tem algo.
        let bucket: &mut Vec<ActionRef> = if flat || !to_primary { &mut secondary } else { &mut primary };
        if !to_primary && !bucket.is_empty() {
            bucket.push(Separator::new());
        }
        for action in actions {
            bucket.push(menu_action_as_action(action));
        }
    }

    PrimaryAndSecondaryActions { primary, secondary }
}

/// O predicado de `getContextMenuActionsImpl`: o grupo informado, ou `navigation`.
fn primary_group_predicate(primary_group: Option<&str>) -> impl Fn(&str) -> bool + '_ {
    move |group| group == primary_group.unwrap_or("navigation")
}

/// `getContextMenuActions(groups, primaryGroup?)`.
pub fn get_context_menu_actions(groups: &[MenuActionGroup], primary_group: Option<&str>) -> PrimaryAndSecondaryActions {
    fill_in_actions(groups, &primary_group_predicate(primary_group), false)
}

/// `getFlatContextMenuActions(groups, primaryGroup?)`: os dois baldes numa lista só.
pub fn get_flat_context_menu_actions(groups: &[MenuActionGroup], primary_group: Option<&str>) -> Vec<ActionRef> {
    fill_in_actions(groups, &primary_group_predicate(primary_group), true).secondary
}
