//! Porte parcial de `src/vs/platform/dnd/browser/dnd.ts`: os formatos de transferência do código (`CodeDataTransfers`) e
//! a gravação dos marcadores de problemas no `dataTransfer` de um arrasto (`MarkerTransferData`, `fillInMarkersDragData`),
//! que o arrastar e soltar do painel de problemas usa.
//!
//! Notas de tradução:
//! - **`DragEvent`.** É o [`DragEvent`] de `je-widgets`, cujo `dataTransfer` é o do `je-ui` (sempre presente, ao contrário
//!   do `DragEvent.dataTransfer` anulável do DOM, onde o original usa `e.dataTransfer?.setData`).
//! - **`JSON.stringify(data)` de um `IMarker`.** Os marcadores são objetos literais criados por `MarkerService._toMarker`, e
//!   o `JSON.stringify` os escreve na ordem de criação: `resource`, `owner`, `code`, `severity`, `message`, `source`,
//!   `startLineNumber`, `startColumn`, `endLineNumber`, `endColumn`, `relatedInformation`, `modelVersionId`, `tags`,
//!   `origin`, sem as chaves `undefined`. Uma `URI` sai como o `URI#toJSON` a escreve. O valor intermediário é o
//!   [`Marshalled`] do `je-base`, que já tem essas duas regras.
//! - **`{ uri: UriComponents }`.** O `UriComponents` que o painel passa é o próprio `URI` do recurso (o `toJSON` dele), por isso
//!   [`MarkerTransferData::Uri`] guarda um [`URI`].
//! - Fica de fora o resto de `dnd.ts` (`extractEditorsDropData` e companhia, `LocalSelectionTransfer`, o registro de
//!   contribuições, os símbolos, as saídas de célula de caderno e a referência de chat), que o painel de problemas não usa.

use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::marshalling::{Marshalled, json_stringify};
use je_base::uri::URI;
use je_platform::markers::markers::{Marker as IMarker, MarkerCode, RelatedInformation as IRelatedInformation};
use je_widgets::dnd::DragEvent;

/// `CodeDataTransfers`: os tipos de transferência que só o código conhece.
pub struct CodeDataTransfers;

impl CodeDataTransfers {
    pub const EDITORS: &'static str = "CodeEditors";
    pub const FILES: &'static str = "CodeFiles";
    pub const SYMBOLS: &'static str = "application/vnd.code.symbols";
    pub const MARKERS: &'static str = "application/vnd.code.diagnostics";
    pub const NOTEBOOK_CELL_OUTPUT: &'static str = "notebook-cell-output";
    pub const SCM_HISTORY_ITEM: &'static str = "scm-history-item";
    pub const CHAT_REFERENCE: &'static str = "application/vnd.code.chat-reference";
}

/// `MarkerTransferData`: `IMarker | { uri: UriComponents }`.
#[derive(Clone)]
pub enum MarkerTransferData {
    Marker(Rc<IMarker>),
    Uri(URI),
}

fn text(value: &str) -> Marshalled {
    Marshalled::String(JsString::from(value))
}

fn number(value: i64) -> Marshalled {
    Marshalled::Number(value as f64)
}

/// Um valor que some do objeto quando é `undefined`.
fn optional<T>(value: Option<T>, convert: impl FnOnce(T) -> Marshalled) -> Marshalled {
    value.map_or(Marshalled::Undefined, convert)
}

fn object(entries: Vec<(&str, Marshalled)>) -> Marshalled {
    Marshalled::Object(entries.into_iter().map(|(key, value)| (JsString::from(key), value)).collect())
}

/// `code?: string | { value: string; target: URI }`.
fn code_value(code: &MarkerCode) -> Marshalled {
    match code {
        MarkerCode::Text(value) => text(value),
        MarkerCode::Target { value, target } => object(vec![("value", text(value)), ("target", Marshalled::Uri(target.clone()))]),
    }
}

/// `IRelatedInformation`.
fn related_information_value(related: &IRelatedInformation) -> Marshalled {
    object(vec![
        ("resource", Marshalled::Uri(related.resource.clone())),
        ("message", text(&related.message)),
        ("startLineNumber", number(related.start_line_number)),
        ("startColumn", number(related.start_column)),
        ("endLineNumber", number(related.end_line_number)),
        ("endColumn", number(related.end_column)),
    ])
}

/// `IMarker`, na ordem das chaves de `MarkerService._toMarker` (ver as notas do módulo).
fn marker_value(marker: &IMarker) -> Marshalled {
    object(vec![
        ("resource", Marshalled::Uri(marker.resource.clone())),
        ("owner", text(&marker.owner)),
        ("code", optional(marker.code.as_ref(), code_value)),
        ("severity", Marshalled::Number(f64::from(marker.severity.0))),
        ("message", text(&marker.message)),
        ("source", optional(marker.source.as_deref(), text)),
        ("startLineNumber", number(marker.start_line_number)),
        ("startColumn", number(marker.start_column)),
        ("endLineNumber", number(marker.end_line_number)),
        ("endColumn", number(marker.end_column)),
        ("relatedInformation", optional(marker.related_information.as_ref(), |related| Marshalled::Array(related.iter().map(related_information_value).collect()))),
        ("modelVersionId", optional(marker.model_version_id, number)),
        ("tags", optional(marker.tags.as_ref(), |tags| Marshalled::Array(tags.iter().map(|tag| Marshalled::Number(f64::from(*tag as u8))).collect()))),
        ("origin", optional(marker.origin.as_deref(), text)),
    ])
}

impl MarkerTransferData {
    /// O valor que o `JSON.stringify` vê.
    fn to_marshalled(&self) -> Marshalled {
        match self {
            Self::Marker(marker) => marker_value(marker),
            Self::Uri(uri) => object(vec![("uri", Marshalled::Uri(uri.clone()))]),
        }
    }
}

/// `setDataAsJSON(e, kind, data)`.
fn set_data_as_json(e: &DragEvent, kind: &str, data: &Marshalled) {
    if let Some(json) = json_stringify(data) {
        e.data_transfer.set_data(kind, &json.to_string_lossy());
    }
}

/// `fillInMarkersDragData(markerData, e)`.
pub fn fill_in_markers_drag_data(marker_data: &[MarkerTransferData], e: &DragEvent) {
    set_data_as_json(e, CodeDataTransfers::MARKERS, &Marshalled::Array(marker_data.iter().map(MarkerTransferData::to_marshalled).collect()));
}
