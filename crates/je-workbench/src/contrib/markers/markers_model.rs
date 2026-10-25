//! Porte de `src/vs/workbench/contrib/markers/browser/markersModel.ts`: o modelo do painel de problemas
//! (`MarkersModel`), os marcadores agrupados por recurso (`ResourceMarkers`), o marcador (`Marker`), o item de tabela
//! (`MarkerTableItem`) e a informação relacionada (`RelatedInformation`).
//!
//! Notas de tradução:
//! - **`IMarker` e `IRelatedInformation`.** São o `Marker` e o `RelatedInformation` de `je_platform::markers::markers`
//!   (importados como [`IMarker`] e [`IRelatedInformation`]). O marcador cru é compartilhado (`Rc<IMarker>`), porque o
//!   original guarda e devolve a mesma referência (`actuals[0].marker === marker6`).
//! - **`Set<ResourceMarkers>`.** `MarkerChangesEvent` guarda `Vec<Rc<ResourceMarkers>>` sem repetição por identidade, na
//!   ordem de inserção (a de um `Set`).
//! - **Elementos compartilhados.** `ResourceMarkers`, `Marker` e `RelatedInformation` são `Rc`, e `MarkerElement` é o
//!   enum da união `ResourceMarkers | Marker | RelatedInformation`. `MarkerElement` compara e faz hash pelo ponteiro do `Rc`
//!   (o `===` do JavaScript), porque é o elemento da árvore do painel.
//! - **`MarkerTableItem extends Marker`.** É a composição: o campo `base` é o `Marker` que o construtor original recebe em
//!   `super(marker.id, marker.marker, marker.relatedInformation)`.
//! - **`relatedInformation` do `Marker`.** O campo `readonly` do original é preenchido no construtor; o `null` que o teste
//!   passa e depois troca por atribuição é, no porte, a lista dada ao construtor.
//! - **`toString()`.** O `JSON.stringify(objeto, null, '\t')` do original sobre `{ ...this.marker, resource: <path>,
//!   relatedInformation }` é o `JSON.stringify` com recuo de um espaço do `Dynamic` cujo recuo inicial de cada linha vira
//!   tabulação (o texto de um valor nunca tem quebra de linha crua, o JSON as escapa, então só o recuo muda). A ordem das
//!   chaves é a do objeto literal que o oráculo produz (`owner`, `resource`, `severity`, `message`, linhas e colunas,
//!   `source`, `relatedInformation`), com as chaves opcionais restantes (`code`, `modelVersionId`, `tags`, `origin`) depois;
//!   a informação relacionada sai como `startLineNumber`, `startColumn`, `endLineNumber`, `endColumn`, `message`,
//!   `resource`. O alvo de um `code` com `target` sai com os campos de `URI#toJSON` que o objeto guarda.
//! - **`localeCompare`.** É o [`locale_compare`] do `je-base`; `MarkerSeverity.compare` devolve um número, convertido em
//!   [`Ordering`] pelo sinal.
//! - **`hash(values)`.** Os valores (texto e número) formam um `Dynamic` de lista.
//! - **`IMarkerData.makeKey(rawMarker)`.** É o [`MarkerData::make_key`] sobre um `MarkerData` com os campos do marcador cru
//!   que entram na chave.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::Rc;

use je_base::collator::locale_compare;
use je_base::dynamic::{Dynamic, json_stringify_indented};
use je_base::event::{Emitter, Event};
use je_base::filters::Match;
use je_base::hash::hash;
use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::map::{OrderedMap, ResourceMap};
use je_base::resources::ExtUri;
use je_base::strings::split_lines;
use je_base::uri::{Field, UriChange, URI};
use je_editor::range::Range;
use je_platform::markers::marker_service::unsupported_schemas;
use je_platform::markers::markers::{Marker as IMarker, MarkerCode, MarkerData, MarkerSeverity, RelatedInformation as IRelatedInformation};
use je_platform::resources::{ext_uri, fs_path};

/// `MarkerElement`.
#[derive(Clone)]
pub enum MarkerElement {
    ResourceMarkers(Rc<ResourceMarkers>),
    Marker(Rc<Marker>),
    RelatedInformation(Rc<RelatedInformation>),
}

/// A identidade do `===` do JavaScript: os elementos da árvore do painel (chaves do `Map` por objeto e do modelo da
/// árvore) são iguais quando são o mesmo objeto.
impl PartialEq for MarkerElement {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::ResourceMarkers(a), Self::ResourceMarkers(b)) => Rc::ptr_eq(a, b),
            (Self::Marker(a), Self::Marker(b)) => Rc::ptr_eq(a, b),
            (Self::RelatedInformation(a), Self::RelatedInformation(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for MarkerElement {}

impl std::hash::Hash for MarkerElement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::ResourceMarkers(element) => Rc::as_ptr(element).hash(state),
            Self::Marker(element) => Rc::as_ptr(element).hash(state),
            Self::RelatedInformation(element) => Rc::as_ptr(element).hash(state),
        }
    }
}

/// O sinal de um número de comparação do original como [`Ordering`].
fn ordering_of(value: i64) -> Ordering {
    value.cmp(&0)
}

/// `compareMarkersByUri`.
pub fn compare_markers_by_uri(a: &IMarker, b: &IMarker) -> i32 {
    ext_uri().compare(&a.resource, &b.resource, false)
}

fn compare_resource_markers(a: &Rc<ResourceMarkers>, b: &Rc<ResourceMarkers>) -> Ordering {
    let first_marker_of_a = a.markers().into_iter().next();
    let first_marker_of_b = b.markers().into_iter().next();
    let mut res = Ordering::Equal;
    if let (Some(first_a), Some(first_b)) = (first_marker_of_a, first_marker_of_b) {
        res = ordering_of(MarkerSeverity::compare(first_a.marker.severity, first_b.marker.severity));
    }
    if res == Ordering::Equal {
        res = locale_compare(&a.path, &b.path).then_with(|| locale_compare(&a.name, &b.name));
    }
    res
}

/// `ResourceMarkers`.
pub struct ResourceMarkers {
    pub id: String,
    pub resource: URI,
    pub path: String,
    pub name: String,

    markers_map: RefCell<ResourceMap<Vec<Rc<Marker>>>>,
    cached_markers: RefCell<Option<Vec<Rc<Marker>>>>,
    total: Cell<usize>,
}

impl ResourceMarkers {
    pub fn new(id: String, resource: URI) -> Self {
        let path = fs_path(&resource);
        let name = ext_uri().basename(&resource, None);
        Self { id, resource, path, name, markers_map: RefCell::new(ResourceMap::new()), cached_markers: RefCell::new(None), total: Cell::new(0) }
    }

    pub fn markers(&self) -> Vec<Rc<Marker>> {
        if let Some(cached) = &*self.cached_markers.borrow() {
            return cached.clone();
        }
        let ext = ext_uri();
        let mut markers: Vec<Rc<Marker>> = self.markers_map.borrow().values().flatten().cloned().collect();
        markers.sort_by(|a, b| Self::compare_markers(&ext, a, b));
        *self.cached_markers.borrow_mut() = Some(markers.clone());
        markers
    }

    pub fn has(&self, uri: &URI) -> bool {
        self.markers_map.borrow().has(uri)
    }

    pub fn set(&self, uri: &URI, marker: Vec<Rc<Marker>>) {
        self.delete(uri);
        if !marker.is_empty() {
            self.total.set(self.total.get() + marker.len());
            self.markers_map.borrow_mut().set(uri.clone(), marker);
            *self.cached_markers.borrow_mut() = None;
        }
    }

    pub fn delete(&self, uri: &URI) {
        let removed = self.markers_map.borrow().get(uri).map(Vec::len);
        if let Some(length) = removed {
            self.total.set(self.total.get() - length);
            *self.cached_markers.borrow_mut() = None;
            self.markers_map.borrow_mut().delete(uri);
        }
    }

    pub fn total(&self) -> usize {
        self.total.get()
    }

    fn compare_markers(ext: &ExtUri, a: &Marker, b: &Marker) -> Ordering {
        ordering_of(MarkerSeverity::compare(a.marker.severity, b.marker.severity))
            .then_with(|| ext.compare(a.resource(), b.resource(), false).cmp(&0))
            .then_with(|| i64::from(Range::compare_ranges_using_starts(Some(&a.range()), Some(&b.range()))).cmp(&0))
    }
}

/// `Marker`.
pub struct Marker {
    pub id: String,
    pub marker: Rc<IMarker>,
    pub related_information: Vec<Rc<RelatedInformation>>,
    lines: RefCell<Option<Vec<JsString>>>,
}

impl Marker {
    pub fn new(id: impl Into<String>, marker: Rc<IMarker>, related_information: Vec<Rc<RelatedInformation>>) -> Self {
        Self { id: id.into(), marker, related_information, lines: RefCell::new(None) }
    }

    pub fn resource(&self) -> &URI {
        &self.marker.resource
    }

    /// `get range(): IRange`: o próprio marcador, sem normalizar início e fim.
    pub fn range(&self) -> Range {
        Range {
            start_line_number: self.marker.start_line_number as i32,
            start_column: self.marker.start_column as i32,
            end_line_number: self.marker.end_line_number as i32,
            end_column: self.marker.end_column as i32,
        }
    }

    pub fn lines(&self) -> Vec<JsString> {
        if let Some(lines) = &*self.lines.borrow() {
            return lines.clone();
        }
        let message = JsString::from(self.marker.message.as_str());
        let lines: Vec<JsString> = split_lines(message.units()).into_iter().map(JsString::from).collect();
        *self.lines.borrow_mut() = Some(lines.clone());
        lines
    }

    /// `toString()`.
    pub fn to_string(&self) -> String {
        let marker = &self.marker;
        let number = |value: i64| Dynamic::Number(value as f64);
        let mut entries: Vec<(&str, Dynamic)> = vec![
            ("owner", Dynamic::string(&marker.owner)),
            ("resource", Dynamic::String(marker.resource.path().clone())),
            ("severity", Dynamic::Number(f64::from(marker.severity.0))),
            ("message", Dynamic::string(&marker.message)),
            ("startLineNumber", number(marker.start_line_number)),
            ("startColumn", number(marker.start_column)),
            ("endLineNumber", number(marker.end_line_number)),
            ("endColumn", number(marker.end_column)),
        ];
        if let Some(source) = &marker.source {
            entries.push(("source", Dynamic::string(source)));
        }
        if !self.related_information.is_empty() {
            entries.push(("relatedInformation", Dynamic::array(self.related_information.iter().map(|related| related_information_json(&related.raw)))));
        }
        if let Some(code) = &marker.code {
            entries.push(("code", code_json(code)));
        }
        if let Some(model_version_id) = marker.model_version_id {
            entries.push(("modelVersionId", number(model_version_id)));
        }
        if let Some(tags) = &marker.tags {
            entries.push(("tags", Dynamic::array(tags.iter().map(|tag| Dynamic::Number(f64::from(*tag as u8))))));
        }
        if let Some(origin) = &marker.origin {
            entries.push(("origin", Dynamic::string(origin)));
        }
        stringify_with_tabs(&Dynamic::object(entries))
    }
}

/// `{ ...raw, resource: raw.resource.path }`.
fn related_information_json(raw: &IRelatedInformation) -> Dynamic {
    Dynamic::object([
        ("startLineNumber", Dynamic::Number(raw.start_line_number as f64)),
        ("startColumn", Dynamic::Number(raw.start_column as f64)),
        ("endLineNumber", Dynamic::Number(raw.end_line_number as f64)),
        ("endColumn", Dynamic::Number(raw.end_column as f64)),
        ("message", Dynamic::string(&raw.message)),
        ("resource", Dynamic::String(raw.resource.path().clone())),
    ])
}

/// `code?: string | { value: string; target: URI }` como o `JSON.stringify` o vê.
fn code_json(code: &MarkerCode) -> Dynamic {
    match code {
        MarkerCode::Text(text) => Dynamic::string(text),
        MarkerCode::Target { value, target } => {
            let state = target.to_json();
            let mut uri: Vec<(&str, Dynamic)> = Vec::new();
            if let Some(mid) = state.mid {
                uri.push(("$mid", Dynamic::Number(f64::from(mid))));
            }
            if let Some(fs_path) = &state.fs_path {
                uri.push(("fsPath", Dynamic::String(fs_path.clone())));
                if let Some(sep) = state.sep {
                    uri.push(("_sep", Dynamic::Number(f64::from(sep))));
                }
            }
            if let Some(external) = &state.external {
                uri.push(("external", Dynamic::String(external.clone())));
            }
            let components = &state.components;
            for (key, value) in [
                ("path", &components.path),
                ("scheme", &components.scheme),
                ("authority", &components.authority),
                ("query", &components.query),
                ("fragment", &components.fragment),
            ] {
                if let Some(value) = value {
                    uri.push((key, Dynamic::String(value.clone())));
                }
            }
            Dynamic::object([("value", Dynamic::string(value)), ("target", Dynamic::object(uri))])
        }
    }
}

/// `JSON.stringify(value, null, '\t')`: o recuo de um espaço por nível vira uma tabulação por nível.
fn stringify_with_tabs(value: &Dynamic) -> String {
    let text = json_stringify_indented(value, 1).unwrap_or_default();
    text.split('\n')
        .map(|line| {
            let content = line.trim_start_matches(' ');
            format!("{}{content}", "\t".repeat(line.len() - content.len()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `MarkerTableItem`.
pub struct MarkerTableItem {
    pub base: Marker,
    pub source_matches: Option<Vec<Match>>,
    pub code_matches: Option<Vec<Match>>,
    pub message_matches: Option<Vec<Match>>,
    pub file_matches: Option<Vec<Match>>,
}

impl MarkerTableItem {
    pub fn new(
        marker: &Marker,
        source_matches: Option<Vec<Match>>,
        code_matches: Option<Vec<Match>>,
        message_matches: Option<Vec<Match>>,
        file_matches: Option<Vec<Match>>,
    ) -> Self {
        Self {
            base: Marker::new(marker.id.clone(), marker.marker.clone(), marker.related_information.clone()),
            source_matches,
            code_matches,
            message_matches,
            file_matches,
        }
    }
}

/// `RelatedInformation`.
pub struct RelatedInformation {
    pub id: String,
    pub marker: Rc<IMarker>,
    pub raw: IRelatedInformation,
}

impl RelatedInformation {
    pub fn new(id: impl Into<String>, marker: Rc<IMarker>, raw: IRelatedInformation) -> Self {
        Self { id: id.into(), marker, raw }
    }
}

/// `MarkerChangesEvent`.
#[derive(Clone, Default)]
pub struct MarkerChangesEvent {
    pub added: Vec<Rc<ResourceMarkers>>,
    pub removed: Vec<Rc<ResourceMarkers>>,
    pub updated: Vec<Rc<ResourceMarkers>>,
}

/// `Set#add`: sem repetir a mesma instância.
fn add_to_set(set: &mut Vec<Rc<ResourceMarkers>>, item: &Rc<ResourceMarkers>) {
    if !set.iter().any(|existing| Rc::ptr_eq(existing, item)) {
        set.push(item.clone());
    }
}

/// `IMarkerData.makeKey(rawMarker)` para um marcador cru.
fn make_marker_key(raw: &IMarker) -> String {
    MarkerData {
        code: raw.code.clone(),
        severity: raw.severity,
        message: raw.message.clone(),
        source: raw.source.clone(),
        start_line_number: raw.start_line_number,
        start_column: raw.start_column,
        end_line_number: raw.end_line_number,
        end_column: raw.end_column,
        model_version_id: None,
        related_information: None,
        tags: None,
        origin: None,
    }
    .make_key()
}

/// `MarkersModel`.
pub struct MarkersModel {
    cached_sorted_resources: RefCell<Option<Vec<Rc<ResourceMarkers>>>>,
    on_did_change: Rc<Emitter<MarkerChangesEvent>>,
    resources_by_uri: RefCell<OrderedMap<JsString, Rc<ResourceMarkers>>>,
    total: Cell<usize>,
}

impl Default for MarkersModel {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkersModel {
    pub fn new() -> Self {
        Self {
            cached_sorted_resources: RefCell::new(None),
            on_did_change: Emitter::new(),
            resources_by_uri: RefCell::new(OrderedMap::new()),
            total: Cell::new(0),
        }
    }

    pub fn on_did_change(&self) -> Event<MarkerChangesEvent> {
        self.on_did_change.event()
    }

    pub fn resource_markers(&self) -> Vec<Rc<ResourceMarkers>> {
        if let Some(cached) = &*self.cached_sorted_resources.borrow() {
            return cached.clone();
        }
        let mut resources: Vec<Rc<ResourceMarkers>> = self.resources_by_uri.borrow().values().cloned().collect();
        resources.sort_by(compare_resource_markers);
        *self.cached_sorted_resources.borrow_mut() = Some(resources.clone());
        resources
    }

    pub fn reset(&self) {
        let mut removed: Vec<Rc<ResourceMarkers>> = Vec::new();
        for resource_marker in self.resources_by_uri.borrow().values() {
            add_to_set(&mut removed, resource_marker);
        }
        self.resources_by_uri.borrow_mut().clear();
        *self.cached_sorted_resources.borrow_mut() = None;
        self.total.set(0);
        self.on_did_change.fire(MarkerChangesEvent { removed, added: Vec::new(), updated: Vec::new() });
    }

    pub fn total(&self) -> usize {
        self.total.get()
    }

    pub fn get_resource_markers(&self, resource: &URI) -> Option<Rc<ResourceMarkers>> {
        let key = ext_uri().get_comparison_key(resource, true);
        self.resources_by_uri.borrow().get(&key).cloned()
    }

    pub fn set_resource_markers(&self, resources_markers: Vec<(URI, Vec<Rc<IMarker>>)>) {
        let mut change = MarkerChangesEvent::default();
        let ext = ext_uri();
        let unsupported = unsupported_schemas();
        for (resource, raw_markers) in resources_markers {
            if unsupported.contains(resource.scheme().to_string_lossy().as_str()) {
                continue;
            }

            let key = ext.get_comparison_key(&resource, true);
            let mut resource_markers = self.resources_by_uri.borrow().get(&key).cloned();

            if !raw_markers.is_empty() {
                // update, add
                let resource_markers = match resource_markers.take() {
                    None => {
                        let resource_markers_id = Self::id(vec![Dynamic::String(resource.to_string(false))]);
                        let created = Rc::new(ResourceMarkers::new(
                            resource_markers_id,
                            resource.with(&UriChange { fragment: Field::Clear, ..UriChange::default() }),
                        ));
                        self.resources_by_uri.borrow_mut().insert(key.clone(), created.clone());
                        add_to_set(&mut change.added, &created);
                        created
                    }
                    Some(existing) => {
                        add_to_set(&mut change.updated, &existing);
                        existing
                    }
                };
                // Deduplicate markers with identical source, code, severity, message
                // and range so that a diagnostic reported by both a task problem
                // matcher and a language extension is only shown once (#244424).
                let mut processed_marker_keys: Vec<String> = Vec::new();
                let mut markers: Vec<Rc<Marker>> = Vec::new();
                for raw_marker in raw_markers {
                    let marker_key = make_marker_key(&raw_marker) + &raw_marker.resource.to_string(false).to_string_lossy();
                    if processed_marker_keys.contains(&marker_key) {
                        continue;
                    }
                    processed_marker_keys.push(marker_key.clone());

                    let marker_id = Self::id(vec![
                        Dynamic::string(&resource_markers.id),
                        Dynamic::string(&marker_key),
                        Dynamic::Number(0.0),
                        Dynamic::String(raw_marker.resource.to_string(false)),
                    ]);

                    let related_information: Vec<Rc<RelatedInformation>> = raw_marker
                        .related_information
                        .iter()
                        .flatten()
                        .enumerate()
                        .map(|(index, r)| {
                            Rc::new(RelatedInformation::new(
                                Self::id(vec![
                                    Dynamic::string(&marker_id),
                                    Dynamic::String(r.resource.to_string(false)),
                                    Dynamic::Number(r.start_line_number as f64),
                                    Dynamic::Number(r.start_column as f64),
                                    Dynamic::Number(r.end_line_number as f64),
                                    Dynamic::Number(r.end_column as f64),
                                    Dynamic::Number(index as f64),
                                ]),
                                raw_marker.clone(),
                                r.clone(),
                            ))
                        })
                        .collect();

                    markers.push(Rc::new(Marker::new(marker_id, raw_marker, related_information)));
                }

                self.total.set(self.total.get() - resource_markers.total());
                resource_markers.set(&resource, markers);
                self.total.set(self.total.get() + resource_markers.total());
            } else if let Some(resource_markers) = resource_markers {
                // clear
                self.total.set(self.total.get() - resource_markers.total());
                resource_markers.delete(&resource);
                self.total.set(self.total.get() + resource_markers.total());
                if resource_markers.total() == 0 {
                    self.resources_by_uri.borrow_mut().remove(&key);
                    add_to_set(&mut change.removed, &resource_markers);
                } else {
                    add_to_set(&mut change.updated, &resource_markers);
                }
            }
        }

        *self.cached_sorted_resources.borrow_mut() = None;
        if !change.added.is_empty() || !change.removed.is_empty() || !change.updated.is_empty() {
            self.on_did_change.fire(change);
        }
    }

    /// `id(...values)`: o hash dos valores como texto.
    fn id(values: Vec<Dynamic>) -> String {
        hash(&Dynamic::array(values)).to_string()
    }
}

impl IDisposable for MarkersModel {
    fn dispose(&self) {
        self.on_did_change.dispose();
        self.resources_by_uri.borrow_mut().clear();
    }
}
