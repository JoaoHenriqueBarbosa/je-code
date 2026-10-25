//! A implementação do contrato [`DiffEditorFeatures`]: cria as sete features do `DiffEditorWidget` com os construtores que a fatia 3
//! portou, na ordem e com os argumentos que o widget passa (`createInstance(readHotReloadableExport(Feature, reader), ...)`).
//!
//! Divergências:
//! - **`DiffEditorViewZones`** (`components/diffEditorViewZones/diffEditorViewZones.ts`, com o `renderLines.ts`, o
//!   `inlineDiffDeletedCodeMargin.ts` e o `copySelection.ts`) ainda não foi portado. Enquanto não é, a criação dele entra pelo
//!   [`CreateDiffEditorViewZones`] do construtor, com os mesmos argumentos do contrato; quando o porte existir, o construtor passa a
//!   chamá-lo direto e esse parâmetro some.
//! - **A barra de ações da calha** (`MenuWorkbenchToolBar`) mora no `je-workbench`: o construtor recebe a
//!   [`DiffGutterToolbarFactory`] que a cria (ver `features::gutter_feature`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::DisposableRef;
use je_base::observable::IObservable;
use je_widgets::sash::BoundarySashes;

use crate::config::editor_options::layout_info::EditorLayoutInfo;
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::diff_editor::ScrollAnchorRunner;
use crate::diff_editor::components::accessible_diff_viewer::{AccessibleDiffViewer, AccessibleDiffViewerModelFromEditors};
use crate::diff_editor::components::diff_editor_sash::SashLayout;
use crate::diff_editor::feature_contracts::{
    DiffEditorFeatureContext, DiffEditorFeatures, IAccessibleDiffViewer, IDiffEditorGutter, IDiffEditorViewZones, IHideUnchangedRegionsFeature,
    IMovedBlocksLinesFeature, IOverviewRulerFeature, IsUpdatingViewZones, SetAccessibleDiffViewerVisible,
};
use crate::diff_editor::features::gutter_feature::{DiffEditorGutter, DiffGutterToolbarFactory};
use crate::diff_editor::features::hide_unchanged_regions_feature::HideUnchangedRegionsFeature;
use crate::diff_editor::features::moved_blocks_lines_feature::MovedBlocksLinesFeature;
use crate::diff_editor::features::overview_ruler_feature::OverviewRulerFeature;
use crate::diff_editor::features::revert_buttons_feature::RevertButtonsFeature;
use crate::diff_editor_options::DiffEditorVariant;
use crate::diff_editor_utils::ViewZoneIdSet;

/// A criação do `DiffEditorViewZones`: `(context, isUpdatingViewZones, origViewZoneIdsToIgnore, modViewZoneIdsToIgnore)`.
pub type CreateDiffEditorViewZones =
    Rc<dyn Fn(&DiffEditorFeatureContext, IsUpdatingViewZones, ViewZoneIdSet, ViewZoneIdSet) -> Rc<dyn IDiffEditorViewZones>>;

/// As sete features do `DiffEditorWidget`.
pub struct DiffEditorFeaturesImpl {
    create_gutter_toolbar: DiffGutterToolbarFactory,
    create_view_zones: CreateDiffEditorViewZones,
    hide_unchanged_regions: RefCell<Weak<HideUnchangedRegionsFeature>>,
}

impl DiffEditorFeaturesImpl {
    pub fn new(create_gutter_toolbar: DiffGutterToolbarFactory, create_view_zones: CreateDiffEditorViewZones) -> Rc<Self> {
        Rc::new(Self { create_gutter_toolbar, create_view_zones, hide_unchanged_regions: RefCell::new(Weak::new()) })
    }

    /// A feature de regiões sem mudanças que o widget criou por último, enquanto ela está viva (o widget é quem a segura).
    pub fn hide_unchanged_regions(&self) -> Option<Rc<HideUnchangedRegionsFeature>> {
        self.hide_unchanged_regions.borrow().upgrade()
    }
}

impl DiffEditorFeatures for DiffEditorFeaturesImpl {
    fn create_overview_ruler(
        &self,
        context: &DiffEditorFeatureContext,
        root_width: IObservable<f64>,
        root_height: IObservable<f64>,
        modified_editor_layout_info: IObservable<EditorLayoutInfo>,
    ) -> Rc<dyn IOverviewRulerFeature> {
        OverviewRulerFeature::new(context, root_width, root_height, modified_editor_layout_info)
    }

    fn create_hide_unchanged_regions(
        &self,
        context: &DiffEditorFeatureContext,
        run_with_original_editor_scroll_anchor: Option<ScrollAnchorRunner>,
        run_with_modified_editor_scroll_anchor: Option<ScrollAnchorRunner>,
        variant: DiffEditorVariant,
    ) -> Rc<dyn IHideUnchangedRegionsFeature> {
        let feature = HideUnchangedRegionsFeature::new(context, run_with_original_editor_scroll_anchor, run_with_modified_editor_scroll_anchor, variant);
        *self.hide_unchanged_regions.borrow_mut() = Rc::downgrade(&feature);
        feature
    }

    fn create_view_zones(
        &self,
        context: &DiffEditorFeatureContext,
        is_updating_view_zones: IsUpdatingViewZones,
        orig_view_zone_ids_to_ignore: ViewZoneIdSet,
        mod_view_zone_ids_to_ignore: ViewZoneIdSet,
    ) -> Rc<dyn IDiffEditorViewZones> {
        (self.create_view_zones)(context, is_updating_view_zones, orig_view_zone_ids_to_ignore, mod_view_zone_ids_to_ignore)
    }

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
    ) -> Rc<dyn IAccessibleDiffViewer> {
        AccessibleDiffViewer::new(
            visible,
            set_visible,
            can_close,
            width,
            height,
            diffs,
            Rc::new(AccessibleDiffViewerModelFromEditors::new(context.editors.clone())),
            &context.instantiation_service,
        )
    }

    fn create_gutter(
        &self,
        context: &DiffEditorFeatureContext,
        sash_layout: Rc<SashLayout>,
        boundary_sashes: IObservable<Option<Rc<BoundarySashes>>>,
    ) -> Rc<dyn IDiffEditorGutter> {
        DiffEditorGutter::new(context, sash_layout, boundary_sashes, self.create_gutter_toolbar.clone())
    }

    fn create_moved_blocks_lines(
        &self,
        context: &DiffEditorFeatureContext,
        original_editor_layout_info: IObservable<EditorLayoutInfo>,
        modified_editor_layout_info: IObservable<EditorLayoutInfo>,
    ) -> Rc<dyn IMovedBlocksLinesFeature> {
        MovedBlocksLinesFeature::new(context, original_editor_layout_info, modified_editor_layout_info)
    }

    fn create_revert_buttons(&self, context: &DiffEditorFeatureContext) -> DisposableRef {
        RevertButtonsFeature::new(context)
    }
}
