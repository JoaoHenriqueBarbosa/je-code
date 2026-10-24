//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/viewParts/notebookEditorToolbar.ts`: só a camada lógica
//! que cabe a uma UI nativa sem DOM, ou seja `IActionModel`, as constantes de largura e as funções de cálculo de
//! ações visíveis e de overflow (`workbenchCalculateActions`, `workbenchDynamicCalculateActions` e o
//! `actionOverflowHelper` que as duas usam). A classe `NotebookEditorWorkbenchToolbar` (toolbars do DOM, menus,
//! `ResizeObserver`, cache de tamanhos) é widget de notebook e fica para a Fase 6.
//!
//! Divergências:
//! - As funções recebem o vetor de modelos por valor e devolvem o vetor resultante (o original muta o `renderLabel` dos
//!   mesmos objetos e devolve o mesmo array); o valor devolvido é o mesmo.
//! - `size` é `number` no original, aqui `f64` (larguras medidas podem ser fracionárias).
//! - `action instanceof Separator` é `as_any().is::<Separator>()`.

use je_base::actions::{ActionRef, Separator};

/// Largura de uma ação só com ícone.
pub const ICON_ONLY_ACTION_WIDTH: f64 = 21.0;
/// Largura do botão de "mais ações".
pub const TOGGLE_MORE_ACTION_WIDTH: f64 = 21.0;
/// Espaço à direita de cada ação.
pub const ACTION_PADDING: f64 = 8.0;

/// `IActionModel`.
#[derive(Clone)]
pub struct IActionModel {
    pub action: ActionRef,
    pub size: f64,
    pub visible: bool,
    pub render_label: bool,
}

impl IActionModel {
    fn is_separator(&self) -> bool {
        self.action.as_any().is::<Separator>()
    }
}

/// O `{ primaryActions, secondaryActions }` que as funções de cálculo devolvem.
#[derive(Clone)]
pub struct CalculatedActions {
    pub primary_actions: Vec<IActionModel>,
    pub secondary_actions: Vec<ActionRef>,
}

/// `workbenchCalculateActions`.
pub fn workbench_calculate_actions(
    initial_primary_actions: Vec<IActionModel>,
    initial_secondary_actions: Vec<ActionRef>,
    left_toolbar_container_max_width: f64,
) -> CalculatedActions {
    action_overflow_helper(initial_primary_actions, initial_secondary_actions, left_toolbar_container_max_width, false)
}

/// `workbenchDynamicCalculateActions`.
pub fn workbench_dynamic_calculate_actions(
    mut initial_primary_actions: Vec<IActionModel>,
    initial_secondary_actions: Vec<ActionRef>,
    left_toolbar_container_max_width: f64,
) -> CalculatedActions {
    if initial_primary_actions.is_empty() {
        return CalculatedActions { primary_actions: Vec::new(), secondary_actions: initial_secondary_actions };
    }

    // find true length of array, add 1 for each primary actions, ignoring an item when size = 0
    let visible_action_length = initial_primary_actions.iter().filter(|action| action.size != 0.0).count() as f64;

    // step 1: try to fit all primary actions
    let total_width_with_labels =
        initial_primary_actions.iter().map(|action| action.size).sum::<f64>() + (visible_action_length - 1.0) * ACTION_PADDING;
    if total_width_with_labels <= left_toolbar_container_max_width {
        for action in &mut initial_primary_actions {
            action.render_label = true;
        }
        return action_overflow_helper(initial_primary_actions, initial_secondary_actions, left_toolbar_container_max_width, false);
    }

    // step 2: check if they fit without labels
    if visible_action_length * ICON_ONLY_ACTION_WIDTH + (visible_action_length - 1.0) * ACTION_PADDING > left_toolbar_container_max_width {
        for action in &mut initial_primary_actions {
            action.render_label = false;
        }
        return action_overflow_helper(initial_primary_actions, initial_secondary_actions, left_toolbar_container_max_width, true);
    }

    // step 3: render as many actions as possible with labels, rest without.
    let mut sum = 0.0;
    let mut last_action_with_label: i64 = -1;
    for i in 0..initial_primary_actions.len() {
        sum += initial_primary_actions[i].size + ACTION_PADDING;

        if initial_primary_actions[i].is_separator() {
            // find group separator
            let remaining_items = initial_primary_actions[i + 1..].iter().filter(|action| action.size != 0.0).count() as f64; // todo: need to exclude size 0 items from this
            let new_total_sum = sum
                + if remaining_items == 0.0 {
                    0.0
                } else {
                    remaining_items * ICON_ONLY_ACTION_WIDTH + (remaining_items - 1.0) * ACTION_PADDING
                };
            if new_total_sum <= left_toolbar_container_max_width {
                last_action_with_label = i as i64;
            }
        }
    }

    // icons only don't fit either
    if last_action_with_label < 0 {
        for action in &mut initial_primary_actions {
            action.render_label = false;
        }
        return action_overflow_helper(initial_primary_actions, initial_secondary_actions, left_toolbar_container_max_width, true);
    }

    // render labels for the actions that have space
    for (index, action) in initial_primary_actions.iter_mut().enumerate() {
        action.render_label = (index as i64) <= last_action_with_label;
    }
    CalculatedActions { primary_actions: initial_primary_actions, secondary_actions: initial_secondary_actions }
}

fn action_overflow_helper(
    initial_primary_actions: Vec<IActionModel>,
    initial_secondary_actions: Vec<ActionRef>,
    left_toolbar_container_max_width: f64,
    icon_only: bool,
) -> CalculatedActions {
    let mut render_actions: Vec<IActionModel> = Vec::new();
    let mut overflow: Vec<ActionRef> = Vec::new();

    let mut current_size = 0.0;
    let mut non_zero_action = false;
    let mut container_full = false;

    if initial_primary_actions.is_empty() {
        return CalculatedActions { primary_actions: Vec::new(), secondary_actions: initial_secondary_actions };
    }

    for action_model in initial_primary_actions {
        let item_size = if icon_only {
            if action_model.size == 0.0 { 0.0 } else { ICON_ONLY_ACTION_WIDTH }
        } else {
            action_model.size
        };

        // if two separators in a row, ignore the second
        if action_model.is_separator() && render_actions.last().is_some_and(IActionModel::is_separator) {
            continue;
        }

        // if a separator is the first nonZero action, ignore it
        if action_model.is_separator() && !non_zero_action {
            continue;
        }

        if current_size + item_size <= left_toolbar_container_max_width && !container_full {
            current_size += ACTION_PADDING + item_size;
            let is_separator = action_model.is_separator();
            render_actions.push(action_model);
            if item_size != 0.0 {
                non_zero_action = true;
            }
            if is_separator {
                non_zero_action = false;
            }
        } else {
            container_full = true;
            if item_size == 0.0 {
                // size 0 implies a hidden item, keep in primary to allow for Workbench to handle visibility
                render_actions.push(action_model);
            } else {
                if action_model.is_separator() {
                    // never push a separator to overflow
                    continue;
                }
                overflow.push(action_model.action);
            }
        }
    }

    for i in (1..render_actions.len()).rev() {
        if render_actions[i].size == 0.0 {
            continue;
        }
        if render_actions[i].is_separator() {
            render_actions.remove(i);
        }
        break;
    }

    if render_actions.last().is_some_and(IActionModel::is_separator) {
        render_actions.pop();
    }

    if !overflow.is_empty() {
        overflow.push(Separator::new());
    }

    if icon_only {
        // if icon only mode, don't render both (+ code) and (+ markdown) buttons. remove of markdown action
        if let Some(markdown_index) = render_actions.iter().position(|a| a.action.id() == "notebook.cell.insertMarkdownCellBelow") {
            render_actions.remove(markdown_index);
        }
    }

    overflow.extend(initial_secondary_actions);
    CalculatedActions { primary_actions: render_actions, secondary_actions: overflow }
}
