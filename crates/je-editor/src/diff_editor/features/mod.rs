//! Porte de `src/vs/editor/browser/widget/diffEditor/features/`: as features do editor de diff. A fatia 3a trouxe o
//! `hideUnchangedRegionsFeature.ts` ([`hide_unchanged_regions_feature`]) e o `revertButtonsFeature.ts` ([`revert_buttons_feature`]); a
//! fatia 3b traz o `movedBlocksLinesFeature.ts` ([`moved_blocks_lines_feature`]), o `gutterFeature.ts` ([`gutter_feature`], que usa o
//! [`crate::diff_editor::utils::editor_gutter`]) e o `overviewRulerFeature.ts` ([`overview_ruler_feature`]).
//!
//! Elas atendem o contrato [`crate::diff_editor::feature_contracts::DiffEditorFeatures`] assim: `create_hide_unchanged_regions` é
//! [`hide_unchanged_regions_feature::HideUnchangedRegionsFeature::new`], `create_revert_buttons` é
//! [`revert_buttons_feature::RevertButtonsFeature::new`], `create_overview_ruler` é
//! [`overview_ruler_feature::OverviewRulerFeature::new`], `create_gutter` é [`gutter_feature::DiffEditorGutter::new`] e
//! `create_moved_blocks_lines` é [`moved_blocks_lines_feature::MovedBlocksLinesFeature::new`]; o `create_accessible_diff_viewer` é do
//! [`crate::diff_editor::components::accessible_diff_viewer`]. A implementação inteira do contrato é a
//! [`crate::diff_editor::diff_editor_features::DiffEditorFeaturesImpl`].

pub mod gutter_feature;
pub mod hide_unchanged_regions_feature;
pub mod moved_blocks_lines_feature;
pub mod overview_ruler_feature;
pub mod revert_buttons_feature;
