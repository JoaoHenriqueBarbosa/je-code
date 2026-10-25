//! Porte de `src/vs/platform/quickinput/browser/tree/quickInputTree.ts`: os dados de filtro da árvore e o estado
//! agregado de um pai.

use je_base::filters::Match;

use crate::quickinput::quick_input::QuickTreeItemChecked;

/// `IQuickTreeFilterData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuickTreeFilterData {
    pub label_highlights: Option<Vec<Match>>,
    pub description_highlights: Option<Vec<Match>>,
}

/// `getParentNodeState(parentChildren)`: o estado do pai a partir do `checked` dos filhos (`None` é o `undefined`, que o
/// `default` do `switch` trata como desmarcado).
pub fn get_parent_node_state(children: impl IntoIterator<Item = Option<QuickTreeItemChecked>>) -> QuickTreeItemChecked {
    let mut contains_checks = false;
    let mut contains_unchecks = false;
    let mut contains_mixed = false;

    for checked in children {
        match checked {
            Some(QuickTreeItemChecked::Mixed) => contains_mixed = true,
            Some(QuickTreeItemChecked::Bool(true)) => contains_checks = true,
            _ => contains_unchecks = true,
        }
        if contains_checks && contains_unchecks && contains_mixed {
            break;
        }
    }
    if contains_unchecks {
        if contains_mixed || contains_checks {
            QuickTreeItemChecked::Mixed
        } else {
            QuickTreeItemChecked::Bool(false)
        }
    } else if contains_mixed {
        QuickTreeItemChecked::Mixed
    } else {
        QuickTreeItemChecked::Bool(contains_checks)
    }
}
