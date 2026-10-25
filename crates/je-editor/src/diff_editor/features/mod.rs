//! Porte de `src/vs/editor/browser/widget/diffEditor/features/`: as features do editor de diff. A fatia 3a traz o
//! `hideUnchangedRegionsFeature.ts` ([`hide_unchanged_regions_feature`]) e o `revertButtonsFeature.ts` ([`revert_buttons_feature`]).
//!
//! Elas atendem o contrato [`crate::diff_editor::feature_contracts::DiffEditorFeatures`] assim: `create_hide_unchanged_regions` é
//! [`hide_unchanged_regions_feature::HideUnchangedRegionsFeature::new`] (o `Rc` dela é um `Rc<dyn IHideUnchangedRegionsFeature>`) e
//! `create_revert_buttons` é [`revert_buttons_feature::RevertButtonsFeature::new`] (o `Rc` dela é um `DisposableRef`). O trait tem os
//! sete métodos e só se implementa inteiro, então a implementação que o `DiffEditorWidgetArgs::features` recebe nasce na fatia 3b,
//! junto das features que faltam, e chama esses construtores.
//!
//! Fatia seguinte (ainda não portada):
//! - **Fatia 3b:** `movedBlocksLinesFeature.ts`, `gutterFeature.ts` (que usa o [`crate::diff_editor::utils::editor_gutter`] daqui),
//!   `overviewRulerFeature.ts`, o `accessibleDiffViewer` e o `diffEditorViewZones`.

pub mod hide_unchanged_regions_feature;
pub mod revert_buttons_feature;
