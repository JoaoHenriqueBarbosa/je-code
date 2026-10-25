//! Porte de `src/vs/editor/common/services/markerDecorationsService.ts`: transforma os marcadores (diagnósticos) do
//! `IMarkerService` em decorações dos modelos abertos.
//!
//! Divergências de forma:
//! - O original compara marcadores por referência ("marker instances are not recreated when different owners update").
//!   O `MarkerService` do porte devolve cópias em cada `read`, então a identidade de um marcador é a sua forma textual
//!   completa (`{:?}`, dono, recurso e todos os campos); dois marcadores idênticos do mesmo dono valem um só.
//! - `_createDecorationRange` ajusta o `startColumn` do marcador recebido; aqui o ajuste vale para a cópia guardada na
//!   decoração (a que `getMarker` e `getLiveMarkers` devolvem), não para o marcador do serviço de marcadores.
//! - `Number.MAX_VALUE` em `endColumn` é qualquer coluna final a partir de `i32::MAX` (as colunas do editor são `i32`).
//! - O `Set<Range>` de supressões, que distingue objetos, é uma lista com um identificador por supressão.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::collections::diff_sets;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::map::{BidirectionalMap, ResourceMap};
use je_base::uri::URI;
use je_platform::markers::{IMarkerService, Marker, MarkerReadOptions, MarkerSeverity, MarkerTag};

use crate::cursor_common::CursorSimpleModel;
use crate::interval_tree::ClassName;
use crate::model::{
    DecorationColor, MinimapPosition, ModelDecoration, ModelDecorationMinimapInit, ModelDecorationOptionsInit,
    ModelDecorationOverviewRulerInit, ModelDeltaDecoration, OverviewRulerLane, TrackedRangeStickiness,
};
use crate::range::Range;
use crate::services::marker_decorations::{IMarkerDecorationsService, MarkerLimit};
use crate::services_model::IModelService;
use crate::text_model::{ModelDecorationOptions, TextModel};

/// `overviewRulerInfo`, `overviewRulerWarning` e `overviewRulerError` (`editorColorRegistry.ts`).
const OVERVIEW_RULER_INFO: &str = "editorOverviewRuler.infoForeground";
const OVERVIEW_RULER_WARNING: &str = "editorOverviewRuler.warningForeground";
const OVERVIEW_RULER_ERROR: &str = "editorOverviewRuler.errorForeground";

/// `minimapInfo`, `minimapWarning` e `minimapError` (`colorRegistry.ts`).
const MINIMAP_INFO: &str = "minimap.infoHighlight";
const MINIMAP_WARNING: &str = "minimap.warningHighlight";
const MINIMAP_ERROR: &str = "minimap.errorHighlight";

/// `MarkerDecorations.MAX_MARKER_DECORATIONS_LIMIT`.
const MAX_MARKER_DECORATIONS_LIMIT: i64 = 5000;

/// `MarkerDecorationsService`.
pub struct MarkerDecorationsService {
    me: Weak<MarkerDecorationsService>,
    base: Rc<Disposable>,
    marker_service: Rc<dyn IMarkerService>,

    on_did_change_marker: Rc<Emitter<Rc<TextModel>>>,
    on_did_change_marker_limit: Rc<Emitter<Rc<TextModel>>>,

    suppressed_ranges: RefCell<ResourceMap<Vec<(u64, Range)>>>,
    next_suppression_id: Cell<u64>,

    marker_decorations: RefCell<ResourceMap<Rc<MarkerDecorations>>>,
}

impl MarkerDecorationsService {
    pub fn new(model_service: Rc<dyn IModelService>, marker_service: Rc<dyn IMarkerService>) -> Rc<Self> {
        let service = Rc::new_cyclic(|me: &Weak<MarkerDecorationsService>| {
            let base = Disposable::new();
            let on_did_change_marker: Rc<Emitter<Rc<TextModel>>> = Emitter::new();
            base.register(on_did_change_marker.clone());
            let on_did_change_marker_limit: Rc<Emitter<Rc<TextModel>>> = Emitter::new();
            base.register(on_did_change_marker_limit.clone());

            let added = me.clone();
            base.register(model_service.on_model_added().listen(move |model| {
                if let Some(service) = added.upgrade() {
                    service.on_model_added(model);
                }
            }));
            let removed = me.clone();
            base.register(model_service.on_model_removed().listen(move |model| {
                if let Some(service) = removed.upgrade() {
                    service.on_model_removed(model);
                }
            }));
            let changed = me.clone();
            base.register(marker_service.on_marker_changed().listen(move |resources| {
                if let Some(service) = changed.upgrade() {
                    service.handle_marker_change(resources);
                }
            }));

            Self {
                me: me.clone(),
                base,
                marker_service,
                on_did_change_marker,
                on_did_change_marker_limit,
                suppressed_ranges: RefCell::new(ResourceMap::new()),
                next_suppression_id: Cell::new(0),
                marker_decorations: RefCell::new(ResourceMap::new()),
            }
        });
        for model in model_service.get_models() {
            service.on_model_added(&model);
        }
        service
    }

    /// `_handleMarkerChange(changedResources)`.
    fn handle_marker_change(&self, changed_resources: &[URI]) {
        for resource in changed_resources {
            let marker_decorations = self.marker_decorations.borrow().get(resource).cloned();
            if let Some(marker_decorations) = marker_decorations {
                self.update_decorations(&marker_decorations);
            }
        }
    }

    /// `_onModelAdded(model)`.
    fn on_model_added(&self, model: &Rc<TextModel>) {
        let marker_decorations = Rc::new(MarkerDecorations::new(model.clone()));
        self.marker_decorations.borrow_mut().set(model.uri(), marker_decorations.clone());
        self.update_decorations(&marker_decorations);
    }

    /// `_onModelRemoved(model)`.
    fn on_model_removed(&self, model: &Rc<TextModel>) {
        let uri = model.uri();
        let marker_decorations = self.marker_decorations.borrow().get(&uri).cloned();
        if let Some(marker_decorations) = marker_decorations {
            marker_decorations.dispose();
            self.marker_decorations.borrow_mut().delete(&uri);
        }

        // clean up markers for internal, transient models
        let scheme = uri.scheme().to_string_lossy();
        if scheme == "inmemory" || scheme == "internal" || scheme == "vscode" {
            let read = MarkerReadOptions { resource: Some(uri.clone()), ..Default::default() };
            for owner in self.marker_service.read(Some(&read)).into_iter().map(|marker| marker.owner) {
                self.marker_service.remove(&owner, std::slice::from_ref(&uri));
            }
        }
    }

    /// `_updateDecorations(markerDecorations)`.
    fn update_decorations(&self, marker_decorations: &MarkerDecorations) {
        let uri = marker_decorations.model.uri();
        let read = MarkerReadOptions { resource: Some(uri.clone()), take: Some(MAX_MARKER_DECORATIONS_LIMIT + 1), ..Default::default() };
        let mut markers = self.marker_service.read(Some(&read));
        let is_marker_limit_exceeded = markers.len() as i64 > MAX_MARKER_DECORATIONS_LIMIT;
        if is_marker_limit_exceeded {
            markers.truncate(MAX_MARKER_DECORATIONS_LIMIT as usize);
        }

        // filter markers from suppressed ranges
        let suppressed: Option<Vec<Range>> = self.suppressed_ranges.borrow().get(&uri).map(|ranges| ranges.iter().map(|(_, range)| *range).collect());
        if let Some(suppressed) = suppressed {
            markers.retain(|marker| {
                let marker_range = lift_range(marker);
                !suppressed.iter().any(|candidate| Range::are_intersecting_or_touching(candidate, &marker_range))
            });
        }

        if marker_decorations.update(markers) {
            self.on_did_change_marker.fire(marker_decorations.model.clone());
        }
        let is_markers_source_truncated = self.marker_service.is_source_truncated(&uri);
        if marker_decorations.update_marker_limit(is_markers_source_truncated, is_marker_limit_exceeded) {
            self.on_did_change_marker_limit.fire(marker_decorations.model.clone());
        }
    }

    /// O `toDisposable` de `addMarkerSuppression`: tira a supressão `id` de `uri`.
    fn remove_marker_suppression(&self, uri: &URI, id: u64) {
        {
            let mut suppressed_ranges = self.suppressed_ranges.borrow_mut();
            let Some(mut ranges) = suppressed_ranges.get(uri).cloned() else {
                return;
            };
            ranges.retain(|(candidate, _)| *candidate != id);
            if ranges.is_empty() {
                suppressed_ranges.delete(uri);
            } else {
                suppressed_ranges.set(uri.clone(), ranges);
            }
        }
        self.handle_marker_change(std::slice::from_ref(uri));
    }
}

impl IMarkerDecorationsService for MarkerDecorationsService {
    fn on_did_change_marker(&self) -> Event<Rc<TextModel>> {
        self.on_did_change_marker.event()
    }

    fn on_did_change_marker_limit(&self) -> Event<Rc<TextModel>> {
        self.on_did_change_marker_limit.event()
    }

    fn get_marker(&self, uri: &URI, decoration: &ModelDecoration) -> Option<Marker> {
        let marker_decorations = self.marker_decorations.borrow().get(uri).cloned();
        marker_decorations.and_then(|marker_decorations| marker_decorations.get_marker(decoration))
    }

    fn get_live_markers(&self, uri: &URI) -> Vec<(Range, Marker)> {
        let marker_decorations = self.marker_decorations.borrow().get(uri).cloned();
        marker_decorations.map_or_else(Vec::new, |marker_decorations| marker_decorations.get_markers())
    }

    fn get_marker_limits(&self, uri: &URI) -> Option<MarkerLimit> {
        let marker_decorations = self.marker_decorations.borrow().get(uri).cloned();
        marker_decorations.map(|marker_decorations| marker_decorations.marker_limit())
    }

    fn add_marker_suppression(&self, uri: &URI, range: Range) -> DisposableRef {
        let id = self.next_suppression_id.get();
        self.next_suppression_id.set(id + 1);
        {
            let mut suppressed_ranges = self.suppressed_ranges.borrow_mut();
            let mut ranges = suppressed_ranges.get(uri).cloned().unwrap_or_default();
            ranges.push((id, range));
            suppressed_ranges.set(uri.clone(), ranges);
        }
        self.handle_marker_change(std::slice::from_ref(uri));

        let service = self.me.clone();
        let uri = uri.clone();
        to_disposable(move || {
            if let Some(service) = service.upgrade() {
                service.remove_marker_suppression(&uri, id);
            }
        })
    }
}

impl IDisposable for MarkerDecorationsService {
    fn dispose(&self) {
        self.base.dispose();
        let values: Vec<Rc<MarkerDecorations>> = self.marker_decorations.borrow().values().cloned().collect();
        for value in values {
            value.dispose();
        }
        self.marker_decorations.borrow_mut().clear();
    }
}

/// `Range.lift(marker)`: as colunas e linhas do marcador cabem em `i32` com o teto de `Number.MAX_VALUE`.
fn lift_range(marker: &Marker) -> Range {
    let clamp = |value: i64| i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX });
    Range::new(clamp(marker.start_line_number), clamp(marker.start_column), clamp(marker.end_line_number), clamp(marker.end_column))
}

fn has_marker_tag(marker: &Marker, tag: MarkerTag) -> bool {
    marker.tags.as_ref().is_some_and(|tags| tags.contains(&tag))
}

/// A identidade de um marcador (ver as divergências do módulo).
fn marker_key(marker: &Marker) -> String {
    format!("{marker:?}")
}

struct MarkerDecorationsState {
    /// marcador (chave) <-> id da decoração
    map: BidirectionalMap<String, String>,
    markers: HashMap<String, Marker>,
    is_marker_limit_exceeded: bool,
    is_markers_source_truncated: bool,
}

/// `MarkerDecorations`.
struct MarkerDecorations {
    model: Rc<TextModel>,
    state: RefCell<MarkerDecorationsState>,
}

impl MarkerDecorations {
    fn new(model: Rc<TextModel>) -> Self {
        Self {
            model,
            state: RefCell::new(MarkerDecorationsState {
                map: BidirectionalMap::new(),
                markers: HashMap::new(),
                is_marker_limit_exceeded: false,
                is_markers_source_truncated: false,
            }),
        }
    }

    /// O `toDisposable` registrado no construtor: solta as decorações do modelo e limpa o mapa.
    fn dispose(&self) {
        let ids: Vec<String> = self.state.borrow().map.values().cloned().collect();
        self.model.delta_decorations(&ids, &[], 0);
        let mut state = self.state.borrow_mut();
        state.map.clear();
        state.markers.clear();
    }

    /// `markerLimit`: o estado atual de limite dos diagnósticos.
    fn marker_limit(&self) -> MarkerLimit {
        let state = self.state.borrow();
        MarkerLimit {
            is_markers_source_truncated: state.is_markers_source_truncated,
            is_markers_truncated: state.is_marker_limit_exceeded.then_some(MAX_MARKER_DECORATIONS_LIMIT),
        }
    }

    fn update(&self, markers: Vec<Marker>) -> bool {
        // The marker service gives the same marker content when different owners update, so the keys are compared
        // to find out what changed since the last update.
        let mut seen = HashSet::new();
        let mut incoming: Vec<(String, Marker)> = Vec::new();
        for marker in markers {
            let key = marker_key(&marker);
            if seen.insert(key.clone()) {
                incoming.push((key, marker));
            }
        }
        let after: Vec<String> = incoming.iter().map(|(key, _)| key.clone()).collect();
        let before: Vec<String> = self.state.borrow().map.keys().cloned().collect();
        let diff = diff_sets(&before, &after);

        if diff.added.is_empty() && diff.removed.is_empty() {
            return false;
        }

        let old_ids: Vec<String> = {
            let state = self.state.borrow();
            diff.removed.iter().map(|key| state.map.get(key).cloned().expect("a decoração do marcador removido existe")).collect()
        };
        let mut added: Vec<(String, Marker)> = Vec::new();
        for key in &diff.added {
            let position = incoming.iter().position(|(candidate, _)| candidate == key).expect("o marcador adicionado veio da leitura");
            added.push(incoming.swap_remove(position));
        }
        let new_decorations: Vec<ModelDeltaDecoration> = added
            .iter_mut()
            .map(|(_, marker)| ModelDeltaDecoration { range: self.create_decoration_range(marker), options: Self::create_decoration_option(marker) })
            .collect();

        let ids = self.model.delta_decorations(&old_ids, &new_decorations, 0);
        let mut state = self.state.borrow_mut();
        for removed_key in &diff.removed {
            state.map.delete(removed_key);
            state.markers.remove(removed_key);
        }
        for (id, (key, marker)) in ids.into_iter().zip(added) {
            state.map.set(key.clone(), id);
            state.markers.insert(key, marker);
        }
        true
    }

    fn update_marker_limit(&self, is_markers_source_truncated: bool, is_marker_limit_exceeded: bool) -> bool {
        let mut state = self.state.borrow_mut();
        if state.is_markers_source_truncated == is_markers_source_truncated && state.is_marker_limit_exceeded == is_marker_limit_exceeded {
            return false;
        }
        state.is_markers_source_truncated = is_markers_source_truncated;
        state.is_marker_limit_exceeded = is_marker_limit_exceeded;
        true
    }

    fn get_marker(&self, decoration: &ModelDecoration) -> Option<Marker> {
        let state = self.state.borrow();
        state.map.get_key(&decoration.id).and_then(|key| state.markers.get(key)).cloned()
    }

    fn get_markers(&self) -> Vec<(Range, Marker)> {
        let state = self.state.borrow();
        let mut res = Vec::new();
        state.map.for_each(|id, key, _| {
            if let Some(range) = self.model.get_decoration_range(id) {
                res.push((range, state.markers[key].clone()));
            }
        });
        res
    }

    fn create_decoration_range(&self, raw_marker: &mut Marker) -> Range {
        let model = &self.model;
        let mut ret = lift_range(raw_marker);

        if raw_marker.severity == MarkerSeverity::HINT && !has_marker_tag(raw_marker, MarkerTag::Unnecessary) && !has_marker_tag(raw_marker, MarkerTag::Deprecated) {
            // * never render hints on multiple lines
            // * make enough space for three dots
            ret = ret.set_end_position(ret.start_line_number, ret.start_column + 2);
        }

        ret = model.validate_range(&ret);

        if ret.is_empty() {
            let last_non_whitespace_column = model.get_line_last_non_whitespace_column(ret.start_line_number);
            let max_column = if last_non_whitespace_column != 0 { last_non_whitespace_column } else { model.get_line_max_column(ret.start_line_number) };

            if max_column == 1 || ret.end_column >= max_column {
                // empty line or behind eol
                // keep the range as is, it will be rendered 1ch wide
                return ret;
            }

            if let Some(word) = model.get_word_at_position(ret.get_start_position()) {
                ret = Range::new(ret.start_line_number, word.start_column, ret.end_line_number, word.end_column);
            }
        } else if raw_marker.end_column >= i64::from(i32::MAX) && raw_marker.start_column == 1 && ret.start_line_number == ret.end_line_number {
            let min_column = model.get_line_first_non_whitespace_column(lift_range(raw_marker).start_line_number);
            if min_column < ret.end_column {
                ret = Range::new(ret.start_line_number, min_column, ret.end_line_number, ret.end_column);
                raw_marker.start_column = i64::from(min_column);
            }
        }
        ret
    }

    fn create_decoration_option(marker: &Marker) -> Rc<ModelDecorationOptions> {
        let theme = |id: &str| Some(DecorationColor::Theme(id.to_owned()));
        let inline_minimap = |id: &str| ModelDecorationMinimapInit {
            color: theme(id),
            dark_color: None,
            position: MinimapPosition::Inline,
            section_header_style: None,
            section_header_text: None,
        };

        let class_name: Option<ClassName>;
        let mut color: Option<DecorationColor> = None;
        let z_index: i32;
        let mut minimap: Option<ModelDecorationMinimapInit> = None;

        match marker.severity {
            MarkerSeverity::HINT => {
                class_name = if has_marker_tag(marker, MarkerTag::Deprecated) {
                    None
                } else if has_marker_tag(marker, MarkerTag::Unnecessary) {
                    Some(ClassName::EditorUnnecessaryDecoration)
                } else {
                    Some(ClassName::EditorHintDecoration)
                };
                z_index = 0;
            }
            MarkerSeverity::INFO => {
                class_name = Some(ClassName::EditorInfoDecoration);
                color = theme(OVERVIEW_RULER_INFO);
                z_index = 10;
                minimap = Some(inline_minimap(MINIMAP_INFO));
            }
            MarkerSeverity::WARNING => {
                class_name = Some(ClassName::EditorWarningDecoration);
                color = theme(OVERVIEW_RULER_WARNING);
                z_index = 20;
                minimap = Some(inline_minimap(MINIMAP_WARNING));
            }
            _ => {
                class_name = Some(ClassName::EditorErrorDecoration);
                color = theme(OVERVIEW_RULER_ERROR);
                z_index = 30;
                minimap = Some(inline_minimap(MINIMAP_ERROR));
            }
        }

        let mut inline_class_name: Option<ClassName> = None;
        if has_marker_tag(marker, MarkerTag::Unnecessary) {
            inline_class_name = Some(ClassName::EditorUnnecessaryInlineDecoration);
        }
        if has_marker_tag(marker, MarkerTag::Deprecated) {
            inline_class_name = Some(ClassName::EditorDeprecatedInlineDecoration);
        }

        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "marker-decoration".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            class_name: class_name.map(|name| name.as_str().to_owned()),
            show_if_collapsed: true,
            overview_ruler: Some(ModelDecorationOverviewRulerInit { color, dark_color: None, position: Some(OverviewRulerLane::Right) }),
            minimap,
            z_index,
            inline_class_name: inline_class_name.map(|name| name.as_str().to_owned()),
            ..Default::default()
        })
    }
}
