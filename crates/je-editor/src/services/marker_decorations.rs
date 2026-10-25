//! Porte de `src/vs/editor/common/services/markerDecorations.ts`: o contrato `IMarkerDecorationsService`.
//!
//! Divergências de forma:
//! - `ITextModel` é o [`TextModel`] concreto, e `IMarker` é o [`Marker`] do `je-platform`; `getMarker` e `getLiveMarkers`
//!   devolvem cópias do marcador (o original devolve a mesma referência).
//! - `IMarkerLimit.isMarkersTruncated: number | undefined` é `Option<i64>`.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::uri::URI;
use je_platform::markers::Marker;

use crate::model::ModelDecoration;
use crate::range::Range;
use crate::text_model::TextModel;

/// `IMarkerLimit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkerLimit {
    /// Are the marker sources (ie. diagnostics) for a model truncated?
    pub is_markers_source_truncated: bool,
    /// Are the marker decorations for a model truncated?
    pub is_markers_truncated: Option<i64>,
}

/// `IMarkerDecorationsService`.
pub trait IMarkerDecorationsService {
    /// `onDidChangeMarker`.
    fn on_did_change_marker(&self) -> Event<Rc<TextModel>>;

    /// `onDidChangeMarkerLimit`: dispara quando o estado de truncamento dos diagnósticos ou o de limite excedido muda.
    fn on_did_change_marker_limit(&self) -> Event<Rc<TextModel>>;

    /// `getMarker(uri, decoration)`.
    fn get_marker(&self, uri: &URI, decoration: &ModelDecoration) -> Option<Marker>;

    /// `getLiveMarkers(uri)`.
    fn get_live_markers(&self, uri: &URI) -> Vec<(Range, Marker)>;

    /// `getMarkerLimits(uri)`: o estado de limite de um modelo.
    fn get_marker_limits(&self, uri: &URI) -> Option<MarkerLimit>;

    /// `addMarkerSuppression(uri, range)`.
    fn add_marker_suppression(&self, uri: &URI, range: Range) -> DisposableRef;
}

je_instantiation::service_id!(pub I_MARKER_DECORATIONS_SERVICE: dyn IMarkerDecorationsService = "markerDecorationsService");
