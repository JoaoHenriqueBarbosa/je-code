//! Porte de `src/vs/editor/browser/widget/diffEditor/`: o editor de diff do Monaco. A fatia 1 trouxe os dois editores internos
//! (`components/diffEditorEditors.ts`), o sash (`components/diffEditorSash.ts`), as decorações
//! (`components/diffEditorDecorations.ts`), o editor delegante (`delegatingEditorImpl.ts`) e o editor de diff embutido
//! (`embeddedDiffEditorWidget.ts`), mais o `registrations.contribution.ts` de que as decorações dependem. A fatia 2 trouxe o
//! `diffEditorWidget.ts` ([`diff_editor_widget`]): o `DiffEditorWidget`, que implementa [`delegating_editor_impl::DelegatingEditor`],
//! [`embedded_diff_editor_widget::EmbeddedDiffEditorBase`] e o `IDiffEditorWidget` do editor de vários arquivos, e fica com a
//! criação dos editores internos, do `DiffEditorEditors` e do `DiffEditorSash`.
//!
//! Fatia 3a: `features/hideUnchangedRegionsFeature.ts` ([`features::hide_unchanged_regions_feature`]),
//! `features/revertButtonsFeature.ts` ([`features::revert_buttons_feature`]) e `utils/editorGutter.ts`
//! ([`utils::editor_gutter`]). O `utils.ts` do mesmo nível continua em [`crate::diff_editor_utils`], que ganhou o
//! `PlaceholderViewZone`, o `ManagedOverlayWidget` e o `ViewZoneOverlayWidget`. O `CodeEditorWidget` ganhou os widgets da margem de
//! glifos (`addGlyphMarginWidget` e afins) que o botão de reverter usa.
//!
//! Fatia 3b (esta): `features/movedBlocksLinesFeature.ts` ([`features::moved_blocks_lines_feature`]), `features/gutterFeature.ts`
//! ([`features::gutter_feature`]), `features/overviewRulerFeature.ts` ([`features::overview_ruler_feature`]),
//! `components/accessibleDiffViewer.ts` ([`components::accessible_diff_viewer`]) e a implementação do contrato
//! [`feature_contracts::DiffEditorFeatures`] ([`diff_editor_features::DiffEditorFeaturesImpl`]), que chama os construtores das sete
//! features. O `PixelRange` de `utils/editorGutter.ts` ganhou o `PixelRangeSet`, `tryCreate`, `clip` e `intersect`, e o
//! `selectedDiffs` do `RevertButtonsFeature` ([`features::revert_buttons_feature::selected_diffs_observable`]) é compartilhado com a
//! calha.
//!
//! Em parte: o `components/diffEditorViewZones/` ([`components::diff_editor_view_zones`]) tem o `renderLines.ts`,
//! `inlineDiffDeletedCodeMargin.ts` e `copySelection.ts`. Ainda não portado: o `diffEditorViewZones.ts` (`DiffEditorViewZones`); a
//! criação dele é o parâmetro [`diff_editor_features::CreateDiffEditorViewZones`] da implementação do contrato, que some com a parte 2.
//!
//! Onde ficam os arquivos `diff_editor_*` que já existiam em `crates/je-editor/src/`: `diff_editor_options`, `diff_editor_utils`,
//! `diff_editor_view_model` e `diff_provider_factory_service` continuam onde estão, porque outros
//! módulos e crates os referenciam por esses caminhos; mover é uma troca de `use` em todos eles, e fica para quando a fatia 2
//! fechar o widget. O `diff_editor_utils` ganhou aqui o `applyObservableDecorations`.
//!
//! Divergências desta fatia (detalhes em cada módulo):
//! - **DOM vira dado.** Os `HTMLElement` que o original passa (contêineres dos editores, nó do sash) não existem como tipo: o
//!   contêiner de cada editor é da fábrica [`components::diff_editor_editors::CreateInnerEditor`], que o liga ao editor novo, e o
//!   nó do sash é o `NodeId` da árvore de widgets de `je-ui`.
//! - **Constantes das features.** [`ENTIRE_DIFF_OVERVIEW_WIDTH`] (`OverviewRulerFeature.ENTIRE_DIFF_OVERVIEW_WIDTH`) e
//!   [`MOVED_CODE_BLOCK_PADDING`] (`MovedBlocksLinesFeature.movedCodeBlockPadding`) moram aqui até a fatia 3, que as reexporta
//!   nas features.
//! - **`IDiffCodeEditorWidgetOptions`** (de `diffEditorWidget.ts`) é [`DiffCodeEditorWidgetOptions`], aqui porque os componentes
//!   o recebem; o módulo do widget o reexporta.

use std::rc::Rc;

use crate::code_editor_widget::CodeEditorWidgetOptions;
use crate::multi_diff_editor_options::DiffEditorVariant;

pub mod components;
pub mod delegating_editor_impl;
pub mod diff_editor_features;
pub mod diff_editor_widget;
pub mod embedded_diff_editor_widget;
pub mod feature_contracts;
pub mod features;
pub mod registrations_contribution;
pub mod utils;

/// `OverviewRulerFeature.ONE_OVERVIEW_WIDTH`: a largura da régua de cada editor.
pub const ONE_OVERVIEW_WIDTH: f64 = 15.0;

/// `OverviewRulerFeature.ENTIRE_DIFF_OVERVIEW_WIDTH` (`ONE_OVERVIEW_WIDTH * 2`).
pub const ENTIRE_DIFF_OVERVIEW_WIDTH: f64 = ONE_OVERVIEW_WIDTH * 2.0;

/// `MovedBlocksLinesFeature.movedCodeBlockPadding`.
pub const MOVED_CODE_BLOCK_PADDING: i32 = 4;

/// O tipo de `runWithOriginalEditorScrollAnchor` e `runWithModifiedEditorScrollAnchor`: `(anchorLineNumber, update) => void`,
/// em que `update` roda uma vez, segurando a âncora de rolagem.
pub type ScrollAnchorRunner = Rc<dyn Fn(i32, Box<dyn FnOnce()>)>;

/// `IDiffCodeEditorWidgetOptions`.
#[derive(Default)]
pub struct DiffCodeEditorWidgetOptions {
    /// Controla a apresentação dos controles das regiões sem mudanças.
    pub variant: Option<DiffEditorVariant>,
    pub original_editor: Option<CodeEditorWidgetOptions>,
    pub modified_editor: Option<CodeEditorWidgetOptions>,
    pub run_with_original_editor_scroll_anchor: Option<ScrollAnchorRunner>,
    pub run_with_modified_editor_scroll_anchor: Option<ScrollAnchorRunner>,
}
