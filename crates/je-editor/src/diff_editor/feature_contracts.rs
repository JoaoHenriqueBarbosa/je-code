//! Os contratos que o `DiffEditorWidget` (`diffEditorWidget.ts`) tem com o que a fatia 3 porta: as features (`hideUnchangedRegions`,
//! `movedBlocksLines`, `gutter`, `revertButtons`, `overviewRuler`), o `accessibleDiffViewer` e o `diffEditorViewZones`.
//!
//! O widget cria cada uma delas como o original faz (`createInstance(readHotReloadableExport(Feature, reader), ...)`, com os mesmos
//! argumentos e na mesma ordem), mas esses tipos ainda não existem no porte; o que o widget lê de cada um é o trait
//! `I<Nome>` deste módulo e a criação é um método de [`DiffEditorFeatures`]. A fatia 3 implementa os traits nos tipos que porta e
//! entrega uma [`DiffEditorFeatures`] ao `DiffEditorWidgetArgs::features`; nada no widget muda.
//!
//! Divergências:
//! - **Argumentos comuns.** Os argumentos que todas as features recebem do widget (`_editors`, `_diffModel`, `_options`,
//!   `_instantiationService`, o `HTMLElement` raiz e o próprio `DiffEditorWidget` quando o original o passa) são o
//!   [`DiffEditorFeatureContext`]; cada método de [`DiffEditorFeatures`] recebe o contexto e o que é só dele.
//! - **DOM vira dado.** O `elements.root` (`HTMLElement`) é o `NodeId` da árvore de `je-ui` junto do [`Ui`]; o
//!   `elements.accessibleDiffViewer` (um `div` absoluto que cobre a raiz) é a própria raiz, e o visual de cada feature é o dado que o
//!   pintor lê (as classes `has-diff-overview` e `diffViewport`, por exemplo, ficam com a feature).
//! - **`this`.** O `DiffEditorWidget` que o original passa a `DiffEditorViewZones` e a `RevertButtonsFeature` é um `Weak`
//!   ([`DiffEditorFeatureContext::diff_editor`]): as features são criadas dentro do construtor, antes de o `Rc` do widget existir, e
//!   sobem o `Weak` quando precisam do widget (depois da construção).
//! - **Zonas de visão.** `{ orig: IObservableViewZone[]; mod: IObservableViewZone[] }` e `{ origViewZones; modViewZones }` têm o mesmo
//!   formato e são o [`ObservableViewZonePair`] (em `Rc`, porque a igualdade estrita do observável é a identidade do objeto).

use std::rc::{Rc, Weak};

use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::observable::{IObservable, ITransaction};
use je_instantiation::InstantiationService;
use je_ui::{NodeId, Ui};
use je_widgets::sash::BoundarySashes;

use crate::config::editor_options::layout_info::EditorLayoutInfo;
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::diff_editor::ScrollAnchorRunner;
use crate::diff_editor::components::diff_editor_editors::DiffEditorEditors;
use crate::diff_editor::components::diff_editor_sash::SashLayout;
use crate::diff_editor::diff_editor_widget::DiffEditorWidget;
use crate::diff_editor_options::{DiffEditorOptions, DiffEditorVariant};
use crate::diff_editor_utils::{IObservableViewZone, ViewZoneIdSet};
use crate::diff_editor_view_model::DiffEditorViewModel;

/// Os argumentos que o `DiffEditorWidget` passa a todas as features (ver as divergências do módulo).
#[derive(Clone)]
pub struct DiffEditorFeatureContext {
    pub ui: Ui,
    /// O `this.elements.root`.
    pub root: NodeId,
    pub editors: Rc<DiffEditorEditors>,
    pub diff_model: IObservable<Option<Rc<DiffEditorViewModel>>>,
    pub options: Rc<DiffEditorOptions>,
    /// O `_instantiationService` do widget (o filho com o serviço de chaves de contexto do próprio widget).
    pub instantiation_service: Rc<InstantiationService>,
    pub diff_editor: Weak<DiffEditorWidget>,
}

/// As duas listas de zonas de visão (a do editor original e a do modificado).
pub struct ObservableViewZonePair {
    pub orig: Vec<Rc<dyn IObservableViewZone>>,
    pub modified: Vec<Rc<dyn IObservableViewZone>>,
}

/// `OverviewRulerFeature`: o que o widget lê dela.
pub trait IOverviewRulerFeature: IDisposable {
    /// `width` (`ENTIRE_DIFF_OVERVIEW_WIDTH`).
    fn width(&self) -> f64;
}

/// `MovedBlocksLinesFeature`: o que o widget lê dela.
pub trait IMovedBlocksLinesFeature: IDisposable {
    /// `width`.
    fn width(&self) -> IObservable<f64>;
}

/// `DiffEditorGutter`: o que o widget lê dela.
pub trait IDiffEditorGutter: IDisposable {
    /// `width`.
    fn width(&self) -> IObservable<f64>;

    /// `layout(left)`.
    fn layout(&self, left: f64);
}

/// `HideUnchangedRegionsFeature`: o que o widget lê dela.
pub trait IHideUnchangedRegionsFeature: IDisposable {
    /// `viewZones`.
    fn view_zones(&self) -> IObservable<Rc<ObservableViewZonePair>>;

    /// `isUpdatingHiddenAreas`.
    fn is_updating_hidden_areas(&self) -> bool;
}

/// `DiffEditorViewZones`: o que o widget lê dela.
pub trait IDiffEditorViewZones: IDisposable {
    /// `viewZones`.
    fn view_zones(&self) -> IObservable<Rc<ObservableViewZonePair>>;
}

/// `AccessibleDiffViewer`: o que o widget chama nele.
pub trait IAccessibleDiffViewer: IDisposable {
    /// `next()`.
    fn next(&self);

    /// `prev()`.
    fn prev(&self);

    /// `close()`.
    fn close(&self);
}

/// O `_setVisible: (visible: boolean, tx: ITransaction | undefined) => void` do `AccessibleDiffViewer`.
pub type SetAccessibleDiffViewerVisible = Rc<dyn Fn(bool, Option<&dyn ITransaction>)>;

/// `() => boolean`, o `isUpdatingViewZones` que o `DiffEditorViewZones` consulta.
pub type IsUpdatingViewZones = Rc<dyn Fn() -> bool>;

/// A criação das features, na ordem e com os argumentos de `new DiffEditorWidget(...)`. A fatia 3 implementa este trait.
pub trait DiffEditorFeatures {
    /// `OverviewRulerFeature(editors, root, diffModel, rootWidth, rootHeight, modifiedEditorLayoutInfo)`.
    fn create_overview_ruler(
        &self,
        context: &DiffEditorFeatureContext,
        root_width: IObservable<f64>,
        root_height: IObservable<f64>,
        modified_editor_layout_info: IObservable<EditorLayoutInfo>,
    ) -> Rc<dyn IOverviewRulerFeature>;

    /// `HideUnchangedRegionsFeature(editors, diffModel, options, runWithOriginalEditorScrollAnchor,
    /// runWithModifiedEditorScrollAnchor, variant)`.
    fn create_hide_unchanged_regions(
        &self,
        context: &DiffEditorFeatureContext,
        run_with_original_editor_scroll_anchor: Option<ScrollAnchorRunner>,
        run_with_modified_editor_scroll_anchor: Option<ScrollAnchorRunner>,
        variant: DiffEditorVariant,
    ) -> Rc<dyn IHideUnchangedRegionsFeature>;

    /// `DiffEditorViewZones(window, editors, diffModel, options, diffEditor, isUpdatingViewZones, origViewZoneIdsToIgnore,
    /// modViewZoneIdsToIgnore)`.
    fn create_view_zones(
        &self,
        context: &DiffEditorFeatureContext,
        is_updating_view_zones: IsUpdatingViewZones,
        orig_view_zone_ids_to_ignore: ViewZoneIdSet,
        mod_view_zone_ids_to_ignore: ViewZoneIdSet,
    ) -> Rc<dyn IDiffEditorViewZones>;

    /// `AccessibleDiffViewer(parentNode, visible, setVisible, canClose, width, height, diffs, models)`, com o
    /// `AccessibleDiffViewerModelFromEditors(editors)` no lugar do `models`.
    #[allow(clippy::too_many_arguments)]
    fn create_accessible_diff_viewer(
        &self,
        context: &DiffEditorFeatureContext,
        visible: IObservable<bool>,
        set_visible: SetAccessibleDiffViewerVisible,
        can_close: IObservable<bool>,
        width: IObservable<f64>,
        height: IObservable<f64>,
        diffs: IObservable<Option<Rc<Vec<DetailedLineRangeMapping>>>>,
    ) -> Rc<dyn IAccessibleDiffViewer>;

    /// `DiffEditorGutter(root, diffModel, editors, options, sashLayout, boundarySashes)`.
    fn create_gutter(
        &self,
        context: &DiffEditorFeatureContext,
        sash_layout: Rc<SashLayout>,
        boundary_sashes: IObservable<Option<Rc<BoundarySashes>>>,
    ) -> Rc<dyn IDiffEditorGutter>;

    /// `MovedBlocksLinesFeature(root, diffModel, originalEditorLayoutInfo, modifiedEditorLayoutInfo, editors)`.
    fn create_moved_blocks_lines(
        &self,
        context: &DiffEditorFeatureContext,
        original_editor_layout_info: IObservable<EditorLayoutInfo>,
        modified_editor_layout_info: IObservable<EditorLayoutInfo>,
    ) -> Rc<dyn IMovedBlocksLinesFeature>;

    /// `RevertButtonsFeature(editors, diffModel, options, diffEditor)`.
    fn create_revert_buttons(&self, context: &DiffEditorFeatureContext) -> DisposableRef;
}
