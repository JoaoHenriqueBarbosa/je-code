//! Porte de `src/vs/workbench/contrib/testing/browser/explorerProjections/testingViewState.ts`: o estado de colapso
//! serializado da árvore do explorador.
//!
//! Divergências:
//! - **`id: TestId | string`** são duas funções: [`is_collapsed_in_serialized_test_tree`] (texto, que o converte) e
//!   [`is_collapsed_in_serialized_test_tree_for_id`] (o `TestId`).
//! - **`children?: { [localId: string]: ... }`** é um `HashMap`; `hasOwnProperty(part)` é `contains_key`.

use std::collections::HashMap;

use crate::contrib::testing::common::test_id::TestId;

/// `ISerializedTestTreeCollapseState`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ISerializedTestTreeCollapseState {
    pub collapsed: Option<bool>,
    pub children: Option<HashMap<String, ISerializedTestTreeCollapseState>>,
}

/// Gets whether the given test ID is collapsed.
pub fn is_collapsed_in_serialized_test_tree(serialized: &ISerializedTestTreeCollapseState, id: &str) -> Option<bool> {
    is_collapsed_in_serialized_test_tree_for_id(serialized, &TestId::from_string(id))
}

/// Gets whether the given test ID is collapsed.
pub fn is_collapsed_in_serialized_test_tree_for_id(serialized: &ISerializedTestTreeCollapseState, id: &TestId) -> Option<bool> {
    let mut node = serialized;
    for part in id.path() {
        node = node.children.as_ref()?.get(part)?;
    }

    node.collapsed
}
