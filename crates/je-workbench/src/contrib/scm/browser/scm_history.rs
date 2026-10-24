//! Porte parcial de `src/vs/workbench/contrib/scm/browser/scmHistory.ts`: o cálculo do grafo do histórico
//! (`toISCMHistoryItemViewModelArray`, `getHistoryItemIndex`, `compareHistoryItemRefs` e a inserção dos itens de mudanças de
//! entrada e de saída), as cores do grafo e as medidas das raias.
//!
//! Fora do porte (a UI é nativa, sem DOM nem SVG): `renderSCMHistoryItemGraph`, `renderSCMHistoryGraphPlaceholder` (montam
//! elementos SVG) e `toHistoryItemHoverContent` (renderiza o markdown do hover em HTML).
//!
//! Notas de tradução:
//! - **`registerColor`.** As cores já estão no registro do `je-theme` (`registered_colors.rs`, origem `scmHistory.ts`); aqui
//!   ficam os ids como constantes (`ColorIdentifier` é o texto do id) e [`COLOR_REGISTRY`] é o vetor `colorRegistry`.
//! - **`colorMap`.** O `Map<string, ColorIdentifier | undefined>` é [`ColorMap`] (`HashMap<String, Option<String>>`): a chave
//!   presente com valor `None` é o `undefined` explícito que `colorMap.has(id)` distingue de chave ausente.
//! - **Parâmetros opcionais.** `addIncomingChanges` e `addOutgoingChanges` ausentes valem `false`; `mergeBase` ausente ou vazio
//!   desliga os itens de mudanças (o `&& mergeBase` do original).
//! - **`deepClone`** de nós de raia é o `clone`.
//! - **Tamanho do texto.** `displayId?.length` é o tamanho em unidades UTF-16.

use std::collections::HashMap;

use je_base::arrays_find::find_last_idx;
use je_base::nls::localize;
use je_base::numbers::rot;

use crate::contrib::scm::common::history::{
    HistoryItemViewModelKind, ISCMHistoryItem, ISCMHistoryItemGraphNode, ISCMHistoryItemRef, ISCMHistoryItemViewModel,
    SCM_INCOMING_HISTORY_ITEM_ID, SCM_OUTGOING_HISTORY_ITEM_ID,
};

pub const SWIMLANE_HEIGHT: u32 = 22;
pub const SWIMLANE_WIDTH: u32 = 11;

/// History item reference colors (local, remote, base).
pub const HISTORY_ITEM_REF_COLOR: &str = "scmGraph.historyItemRefColor";
pub const HISTORY_ITEM_REMOTE_REF_COLOR: &str = "scmGraph.historyItemRemoteRefColor";
pub const HISTORY_ITEM_BASE_REF_COLOR: &str = "scmGraph.historyItemBaseRefColor";

/// History item hover color.
pub const HISTORY_ITEM_HOVER_DEFAULT_LABEL_FOREGROUND: &str = "scmGraph.historyItemHoverDefaultLabelForeground";
pub const HISTORY_ITEM_HOVER_DEFAULT_LABEL_BACKGROUND: &str = "scmGraph.historyItemHoverDefaultLabelBackground";
pub const HISTORY_ITEM_HOVER_LABEL_FOREGROUND: &str = "scmGraph.historyItemHoverLabelForeground";
pub const HISTORY_ITEM_HOVER_ADDITIONS_FOREGROUND: &str = "scmGraph.historyItemHoverAdditionsForeground";
pub const HISTORY_ITEM_HOVER_DELETIONS_FOREGROUND: &str = "scmGraph.historyItemHoverDeletionsForeground";

/// History graph color registry.
pub const COLOR_REGISTRY: [&str; 5] = [
    "scmGraph.foreground1",
    "scmGraph.foreground2",
    "scmGraph.foreground3",
    "scmGraph.foreground4",
    "scmGraph.foreground5",
];

/// `Map<string, ColorIdentifier | undefined>`.
pub type ColorMap = HashMap<String, Option<String>>;

fn get_label_color_identifier(history_item: &ISCMHistoryItem, color_map: &ColorMap) -> Option<String> {
    if history_item.id == SCM_INCOMING_HISTORY_ITEM_ID {
        return Some(HISTORY_ITEM_REMOTE_REF_COLOR.to_string());
    } else if history_item.id == SCM_OUTGOING_HISTORY_ITEM_ID {
        return Some(HISTORY_ITEM_REF_COLOR.to_string());
    }
    for reference in history_item.references.iter().flatten() {
        if let Some(Some(color_identifier)) = color_map.get(&reference.id) {
            return Some(color_identifier.clone());
        }
    }

    None
}

/// `toISCMHistoryItemViewModelArray`.
#[allow(clippy::too_many_arguments)]
pub fn to_iscm_history_item_view_model_array(
    history_items: &[ISCMHistoryItem],
    color_map: Option<&ColorMap>,
    current_history_item_ref: Option<&ISCMHistoryItemRef>,
    current_history_item_remote_ref: Option<&ISCMHistoryItemRef>,
    current_history_item_base_ref: Option<&ISCMHistoryItemRef>,
    add_incoming_changes: bool,
    add_outgoing_changes: bool,
    merge_base: Option<&str>,
) -> Vec<ISCMHistoryItemViewModel> {
    let default_color_map = ColorMap::new();
    let color_map = color_map.unwrap_or(&default_color_map);
    let mut color_index: i64 = -1;
    let mut view_models: Vec<ISCMHistoryItemViewModel> = Vec::new();

    for history_item in history_items {
        let kind = if current_history_item_ref.and_then(|r| r.revision.as_deref()) == Some(history_item.id.as_str()) {
            HistoryItemViewModelKind::Head
        } else {
            HistoryItemViewModelKind::Node
        };
        let input_swimlanes: Vec<ISCMHistoryItemGraphNode> =
            view_models.last().map(|view_model| view_model.output_swimlanes.clone()).unwrap_or_default();
        let mut output_swimlanes: Vec<ISCMHistoryItemGraphNode> = Vec::new();

        let mut first_parent_added = false;

        // Add first parent to the output
        if !history_item.parent_ids.is_empty() {
            for node in &input_swimlanes {
                if node.id == history_item.id {
                    if !first_parent_added {
                        output_swimlanes.push(ISCMHistoryItemGraphNode {
                            id: history_item.parent_ids[0].clone(),
                            color: get_label_color_identifier(history_item, color_map).unwrap_or_else(|| node.color.clone()),
                        });
                        first_parent_added = true;
                    }

                    continue;
                }

                output_swimlanes.push(node.clone());
            }
        }

        // Add unprocessed parent(s) to the output
        for i in usize::from(first_parent_added)..history_item.parent_ids.len() {
            // Color index (label -> next color)
            let mut color_identifier: Option<String>;

            if i == 0 {
                color_identifier = get_label_color_identifier(history_item, color_map);
            } else {
                let history_item_parent = history_items.iter().find(|h| h.id == history_item.parent_ids[i]);
                color_identifier = history_item_parent.and_then(|parent| get_label_color_identifier(parent, color_map));
            }

            if color_identifier.as_deref().is_none_or(str::is_empty) {
                color_index = rot((color_index + 1) as f64, COLOR_REGISTRY.len() as f64) as i64;
                color_identifier = Some(COLOR_REGISTRY[color_index as usize].to_string());
            }

            output_swimlanes.push(ISCMHistoryItemGraphNode {
                id: history_item.parent_ids[i].clone(),
                color: color_identifier.unwrap_or_default(),
            });
        }

        // Add colors to references
        let mut references: Vec<ISCMHistoryItemRef> = history_item
            .references
            .iter()
            .flatten()
            .map(|reference| {
                let mut color: Option<String> = color_map.get(&reference.id).cloned().flatten();
                if color_map.contains_key(&reference.id) && color.is_none() {
                    // Find the history item in the input swimlanes
                    let input_index = input_swimlanes.iter().position(|node| node.id == history_item.id);

                    // Circle index - use the input swimlane index if present, otherwise add it to the end
                    let circle_index = input_index.unwrap_or(input_swimlanes.len());

                    // Circle color - use the output swimlane color if present, otherwise the input swimlane color
                    color = Some(if circle_index < output_swimlanes.len() {
                        output_swimlanes[circle_index].color.clone()
                    } else if circle_index < input_swimlanes.len() {
                        input_swimlanes[circle_index].color.clone()
                    } else {
                        HISTORY_ITEM_REF_COLOR.to_string()
                    });
                }

                ISCMHistoryItemRef { color, ..reference.clone() }
            })
            .collect();

        // Sort references
        references.sort_by(|ref1, ref2| {
            compare_history_item_refs(ref1, ref2, current_history_item_ref, current_history_item_remote_ref, current_history_item_base_ref).cmp(&0)
        });

        view_models.push(ISCMHistoryItemViewModel {
            history_item: ISCMHistoryItem { references: Some(references), ..history_item.clone() },
            kind,
            input_swimlanes,
            output_swimlanes,
        });
    }

    // Add incoming/outgoing changes history item view models. While working
    // with the view models is a little bit more complex, we are doing this
    // after creating the view models so that we can use the swimlane colors
    // to add the incoming/outgoing changes history items view models to the
    // correct swimlanes.
    add_incoming_outgoing_changes_history_items(
        &mut view_models,
        current_history_item_ref,
        current_history_item_remote_ref,
        add_incoming_changes,
        add_outgoing_changes,
        merge_base,
    );

    view_models
}

/// `getHistoryItemIndex`.
pub fn get_history_item_index(history_item_view_model: &ISCMHistoryItemViewModel) -> usize {
    let history_item = &history_item_view_model.history_item;
    let input_swimlanes = &history_item_view_model.input_swimlanes;

    // Find the history item in the input swimlanes
    let input_index = input_swimlanes.iter().position(|node| node.id == history_item.id);

    // Circle index - use the input swimlane index if present, otherwise add it to the end
    input_index.unwrap_or(input_swimlanes.len())
}

/// `'0'.repeat(displayId.length)`: o tamanho é o do JavaScript (unidades UTF-16).
fn zeroed_display_id(display_id: &str) -> String {
    "0".repeat(display_id.encode_utf16().count())
}

fn add_incoming_outgoing_changes_history_items(
    view_models: &mut Vec<ISCMHistoryItemViewModel>,
    current_history_item_ref: Option<&ISCMHistoryItemRef>,
    current_history_item_remote_ref: Option<&ISCMHistoryItemRef>,
    add_incoming_changes: bool,
    add_outgoing_changes: bool,
    merge_base: Option<&str>,
) {
    let Some(merge_base) = merge_base.filter(|merge_base| !merge_base.is_empty()) else {
        return;
    };
    let current_revision = current_history_item_ref.and_then(|r| r.revision.as_deref());
    let remote_revision = current_history_item_remote_ref.and_then(|r| r.revision.as_deref());
    if current_revision == remote_revision {
        return;
    }

    // Incoming changes node
    if let Some(remote_ref) = current_history_item_remote_ref.filter(|remote_ref| add_incoming_changes && remote_ref.revision.as_deref() != Some(merge_base)) {
        // Find the before/after indices using the merge base (might not be present if the merge base history item is not loaded yet)
        let before_history_item_index = find_last_idx(view_models, |vm, _| vm.output_swimlanes.iter().any(|node| node.id == merge_base), None);
        let after_history_item_index = view_models.iter().position(|vm| vm.history_item.id == merge_base);

        if let (Ok(before_history_item_index), Some(after_history_item_index)) = (usize::try_from(before_history_item_index), after_history_item_index) {
            // There is a known edge case in which the incoming changes have already
            // been merged. For this scenario, we will not be showing the incoming
            // changes history item. https://github.com/microsoft/vscode/issues/276064
            let before_parent_ids = &view_models[before_history_item_index].history_item.parent_ids;
            let incoming_change_merged = before_parent_ids.len() == 2 && before_parent_ids.iter().any(|parent_id| parent_id == merge_base);

            if !incoming_change_merged {
                // Update the before node so that the incoming and outgoing swimlanes
                // point to the `incoming-changes` node instead of the merge base
                let retarget = |node: &ISCMHistoryItemGraphNode| {
                    if node.id == merge_base && node.color == HISTORY_ITEM_REMOTE_REF_COLOR {
                        ISCMHistoryItemGraphNode { id: SCM_INCOMING_HISTORY_ITEM_ID.to_string(), ..node.clone() }
                    } else {
                        node.clone()
                    }
                };
                let before = &view_models[before_history_item_index];
                view_models[before_history_item_index] = ISCMHistoryItemViewModel {
                    input_swimlanes: before.input_swimlanes.iter().map(retarget).collect(),
                    output_swimlanes: before.output_swimlanes.iter().map(retarget).collect(),
                    ..before.clone()
                };

                // Create incoming changes node
                let input_swimlanes = view_models[before_history_item_index].output_swimlanes.clone();
                let output_swimlanes = view_models[after_history_item_index].input_swimlanes.clone();
                let display_id = view_models[0].history_item.display_id.as_deref().map_or_else(String::new, zeroed_display_id);

                let incoming_changes_history_item = ISCMHistoryItem {
                    id: SCM_INCOMING_HISTORY_ITEM_ID.to_string(),
                    display_id: Some(display_id),
                    parent_ids: vec![merge_base.to_string()],
                    author: Some(remote_ref.name.clone()),
                    subject: localize("incomingChanges", "Incoming Changes", &[]),
                    message: String::new(),
                    ..ISCMHistoryItem::default()
                };

                // Insert incoming changes node
                view_models.insert(
                    after_history_item_index,
                    ISCMHistoryItemViewModel {
                        history_item: incoming_changes_history_item,
                        kind: HistoryItemViewModelKind::IncomingChanges,
                        input_swimlanes,
                        output_swimlanes,
                    },
                );
            }
        }
    }

    // Outgoing changes node
    if let Some(current_ref) = current_history_item_ref {
        let Some(revision) = current_ref.revision.as_deref().filter(|revision| add_outgoing_changes && !revision.is_empty() && *revision != merge_base) else {
            return;
        };
        // Find the index of the current history item view model (might not be present if the current history item is not loaded yet)
        let current_history_item_ref_index =
            view_models.iter().position(|vm| vm.kind == HistoryItemViewModelKind::Head && vm.history_item.id == revision);

        if let Some(current_history_item_ref_index) = current_history_item_ref_index {
            // Create outgoing changes node
            let outgoing_changes_history_item = ISCMHistoryItem {
                id: SCM_OUTGOING_HISTORY_ITEM_ID.to_string(),
                display_id: view_models[0].history_item.display_id.as_deref().filter(|display_id| !display_id.is_empty()).map(zeroed_display_id),
                parent_ids: vec![revision.to_string()],
                author: Some(current_ref.name.clone()),
                subject: localize("outgoingChanges", "Outgoing Changes", &[]),
                message: String::new(),
                ..ISCMHistoryItem::default()
            };

            // Copy the input swimlanes from the current history item ref
            let input_swimlanes = view_models[current_history_item_ref_index].input_swimlanes.clone();

            // Copy the input swimlanes and add the current history item ref
            let mut output_swimlanes = input_swimlanes.clone();
            output_swimlanes.push(ISCMHistoryItemGraphNode { id: revision.to_string(), color: HISTORY_ITEM_REF_COLOR.to_string() });

            // Insert outgoing changes node
            view_models.insert(
                current_history_item_ref_index,
                ISCMHistoryItemViewModel {
                    history_item: outgoing_changes_history_item,
                    kind: HistoryItemViewModelKind::OutgoingChanges,
                    input_swimlanes,
                    output_swimlanes,
                },
            );

            // Update the input swimlane for the current history item
            // ref so that it connects with the outgoing changes node
            view_models[current_history_item_ref_index + 1]
                .input_swimlanes
                .push(ISCMHistoryItemGraphNode { id: revision.to_string(), color: HISTORY_ITEM_REF_COLOR.to_string() });
        }
    }
}

/// `compareHistoryItemRefs`.
pub fn compare_history_item_refs(
    ref1: &ISCMHistoryItemRef,
    ref2: &ISCMHistoryItemRef,
    current_history_item_ref: Option<&ISCMHistoryItemRef>,
    current_history_item_remote_ref: Option<&ISCMHistoryItemRef>,
    current_history_item_base_ref: Option<&ISCMHistoryItemRef>,
) -> i32 {
    let get_history_item_ref_order = |reference: &ISCMHistoryItemRef| -> i32 {
        if Some(reference.id.as_str()) == current_history_item_ref.map(|r| r.id.as_str()) {
            1
        } else if Some(reference.id.as_str()) == current_history_item_remote_ref.map(|r| r.id.as_str()) {
            2
        } else if Some(reference.id.as_str()) == current_history_item_base_ref.map(|r| r.id.as_str()) {
            3
        } else if reference.color.is_some() {
            4
        } else {
            99
        }
    };

    // Assign order (current > remote > base > color)
    let ref1_order = get_history_item_ref_order(ref1);
    let ref2_order = get_history_item_ref_order(ref2);

    ref1_order - ref2_order
}
