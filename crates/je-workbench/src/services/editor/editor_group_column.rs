//! Porte de `src/vs/workbench/services/editor/common/editorGroupColumn.ts`: endereçar grupos de editores por um sistema
//! de colunas, em que `0` é a primeira coluna.
//!
//! Divergências:
//! - **`EditorGroupColumn`** é `number` no original; aqui é `f64`, o mesmo tipo do `EditorGroupColumn` do protocolo da
//!   extension host (`api::common::ext_host_type_converters`), que repete as constantes `ACTIVE_GROUP` e `SIDE_GROUP` até
//!   passar a importar daqui. Uma coluna que não é um índice inteiro dentro da lista (`editorGroups[1.5]`, `NaN`,
//!   negativa) não acha grupo, como o acesso por índice do JavaScript.
//! - **`column = ACTIVE_GROUP`** (parâmetro com valor padrão) é `Option<EditorGroupColumn>`, em que `None` é o `undefined`.
//! - **`indexOf`** compara o `id` do grupo (ver [`editor_group_contracts`](super::editor_group_contracts)) e o `-1` de "não
//!   achou" é `-1.0`.

use je_platform::configuration::service::IConfigurationService;

use crate::common::editor::GroupIdentifier;
use crate::services::editor::editor_group_contracts::{
    ACTIVE_GROUP, AUX_WINDOW_GROUP, GroupRef, GroupsOrder, IEditorGroup, IEditorGroupsService, MODAL_GROUP, SIDE_GROUP, preferred_side_by_side_group_direction,
};

use std::rc::Rc;

/// `EditorGroupColumn`: a way to address editor groups through a column based system where `0` is the first column. Will
/// fallback to `SIDE_GROUP` in case the column is invalid.
pub type EditorGroupColumn = f64;

/// `groups[column]`: o grupo da coluna, quando ela é um índice inteiro dentro da lista.
fn group_in_column(groups: &[Rc<dyn IEditorGroup>], column: EditorGroupColumn) -> Option<Rc<dyn IEditorGroup>> {
    if column >= 0.0 && column.fract() == 0.0 { groups.get(column as usize).cloned() } else { None }
}

/// `columnToEditorGroup`: o identificador do grupo da coluna, `ACTIVE_GROUP` ou `SIDE_GROUP`.
pub fn column_to_editor_group(
    editor_group_service: &dyn IEditorGroupsService,
    configuration_service: &dyn IConfigurationService,
    column: Option<EditorGroupColumn>,
) -> GroupIdentifier {
    let column = column.unwrap_or(ACTIVE_GROUP as f64);

    if [ACTIVE_GROUP, SIDE_GROUP, AUX_WINDOW_GROUP, MODAL_GROUP].iter().any(|well_known| column == *well_known as f64) {
        return column as GroupIdentifier; // return early for when column is well known
    }

    let mut group_in_column_found = group_in_column(&editor_group_service.get_groups(GroupsOrder::GridAppearance), column);

    // If a column is asked for that does not exist, we create up to 9 columns in accordance
    // to what `ViewColumn` provides and otherwise fallback to `SIDE_GROUP`.

    if group_in_column_found.is_none() && column < 9.0 {
        let mut i = 0.0;
        while i <= column {
            let editor_groups = editor_group_service.get_groups(GroupsOrder::GridAppearance);
            if group_in_column(&editor_groups, i).is_none() {
                let previous = if i >= 1.0 { group_in_column(&editor_groups, i - 1.0) } else { None };
                editor_group_service.add_group(previous.map(GroupRef::Group), preferred_side_by_side_group_direction(configuration_service));
            }
            i += 1.0;
        }

        group_in_column_found = group_in_column(&editor_group_service.get_groups(GroupsOrder::GridAppearance), column);
    }

    group_in_column_found.map_or(SIDE_GROUP, |group| group.id()) // finally open to the side when group not found
}

/// `editorGroupToColumn`: a coluna do grupo (`-1` quando ele não está na grade).
pub fn editor_group_to_column(editor_group_service: &dyn IEditorGroupsService, editor_group: &GroupRef) -> EditorGroupColumn {
    let group = match editor_group {
        GroupRef::Id(id) => editor_group_service.get_group(*id),
        GroupRef::Group(group) => Some(group.clone()),
    };

    let target_id = group.unwrap_or_else(|| editor_group_service.active_group()).id();
    editor_group_service
        .get_groups(GroupsOrder::GridAppearance)
        .iter()
        .position(|candidate| candidate.id() == target_id)
        .map_or(-1.0, |index| index as f64)
}
