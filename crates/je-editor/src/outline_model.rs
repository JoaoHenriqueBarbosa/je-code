//! Porte de `src/vs/editor/contrib/documentSymbols/browser/outlineModel.ts`: a árvore de símbolos de um documento
//! (`OutlineModel`, com os grupos por provedor `OutlineGroup` e os elementos `OutlineElement`, todos `TreeElement`) e o
//! serviço que a cria e guarda em cache (`IOutlineModelService`, `OutlineModelService`).
//!
//! Representação:
//! - `TreeElement` (classe abstrata com três subclasses que se apontam pelo `parent` e pelo `children`) é o enum
//!   [`TreeElement`], uma alça `Rc` para cada tipo concreto. O `parent` guarda um `Weak` (o pai é dono dos filhos, e o
//!   filho só aponta para ele: um `Rc` nas duas direções formaria um ciclo). `children` (`Map<string, TreeElement>`) é
//!   um [`Children`], um mapa ordenado por inserção compartilhado por referência: o `children = this._groups` de
//!   `_compact` e a atribuição de `merge` fazem os dois campos apontarem para o mesmo `Map`, e a identidade importa.
//! - Os métodos estáticos de `TreeElement` (`findId`, `getElementById`, `size`, `empty`) e o `remove` são métodos de
//!   [`TreeElement`]; o `string | DocumentSymbol` do `findId` é [`IdCandidate`].
//! - `OutlineModel.create(registry, textModel, token)` é [`OutlineModel::create`]; o construtor protegido é público
//!   ([`OutlineModel::new`]), porque o teste do original estende a classe para criar um modelo vazio e para pôr
//!   `_groups` igual a `children` (`readyForTesting`, que é [`OutlineModel::ready_for_testing`]).
//! - `IOutlineMarker` (os quatro números do intervalo mais a `severity`) é [`OutlineMarker`], com o intervalo num
//!   [`Range`]. O `marker: { count, topSev }` do elemento é [`OutlineElementMarker`]. `updateMarker` altera a lista
//!   recebida como o original (o que um elemento "rouba" sai dela): o `coalesceInPlace` que limpa os buracos é o
//!   `drain` do trecho roubado, que é sempre contíguo.
//! - `DocumentSymbol` é o de `language_providers`; os símbolos dos elementos são cópias (o original compartilha a
//!   referência).
//! - O `async getOrCreate` é [`OutlineModelService::get_or_create`], que devolve a promessa; o `try/finally` é o `finally` da
//!   promessa do cache.
//!
//! Divergências:
//! - **`IModelService`.** O serviço só lê `onModelRemoved`, que o recorte `IModelService` do porte ainda não tem; a fatia
//!   que ele usa é [`ModelRemovedSource`], entregue pelo construtor.
//! - O `registerSingleton(IOutlineModelService, OutlineModelService, ...)` do fim do arquivo não existe: o serviço é
//!   criado por quem o usa. O identificador do serviço é [`I_OUTLINE_MODEL_SERVICE`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::arrays::binary_search2;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::on_unexpected_external_error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::map::{LRUCache, OrderedMap};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::strings::common_prefix_length;
use je_base::uri::URI;
use je_platform::markers::MarkerSeverity;

use crate::language_feature_debounce::{DebounceConfig, IFeatureDebounceInformation, ILanguageFeatureDebounceService};
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_providers::{DocumentSymbol, DocumentSymbolProvider};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `Map<string, TreeElement>`: o mapa de filhos, ordenado por inserção e compartilhado por referência.
pub type Children = Rc<RefCell<OrderedMap<String, TreeElement>>>;

fn new_children() -> Children {
    Rc::new(RefCell::new(OrderedMap::new()))
}

/// Os valores de um [`Children`], numa cópia que quem a lê pode percorrer enquanto o mapa muda.
fn child_list(children: &Children) -> Vec<TreeElement> {
    children.borrow().values().cloned().collect()
}

/// `TreeElement`: o modelo, um grupo ou um elemento.
#[derive(Clone)]
pub enum TreeElement {
    Model(Rc<OutlineModel>),
    Group(Rc<OutlineGroup>),
    Element(Rc<OutlineElement>),
}

impl From<Rc<OutlineModel>> for TreeElement {
    fn from(model: Rc<OutlineModel>) -> Self {
        TreeElement::Model(model)
    }
}

impl From<Rc<OutlineGroup>> for TreeElement {
    fn from(group: Rc<OutlineGroup>) -> Self {
        TreeElement::Group(group)
    }
}

impl From<Rc<OutlineElement>> for TreeElement {
    fn from(element: Rc<OutlineElement>) -> Self {
        TreeElement::Element(element)
    }
}

/// O `Weak` de um [`TreeElement`], o que o `parent` guarda.
#[derive(Clone)]
enum WeakTreeElement {
    Model(Weak<OutlineModel>),
    Group(Weak<OutlineGroup>),
    Element(Weak<OutlineElement>),
}

impl WeakTreeElement {
    fn upgrade(&self) -> Option<TreeElement> {
        match self {
            WeakTreeElement::Model(model) => model.upgrade().map(TreeElement::Model),
            WeakTreeElement::Group(group) => group.upgrade().map(TreeElement::Group),
            WeakTreeElement::Element(element) => element.upgrade().map(TreeElement::Element),
        }
    }
}

/// O `parent` de um elemento da árvore (`TreeElement | undefined`).
type ParentLink = RefCell<Option<WeakTreeElement>>;

/// `string | DocumentSymbol`, o `candidate` de `TreeElement.findId`.
#[derive(Clone, Copy)]
pub enum IdCandidate<'a> {
    Name(&'a str),
    Symbol(&'a DocumentSymbol),
}

impl TreeElement {
    fn downgrade(&self) -> WeakTreeElement {
        match self {
            TreeElement::Model(model) => WeakTreeElement::Model(Rc::downgrade(model)),
            TreeElement::Group(group) => WeakTreeElement::Group(Rc::downgrade(group)),
            TreeElement::Element(element) => WeakTreeElement::Element(Rc::downgrade(element)),
        }
    }

    /// `id`.
    pub fn id(&self) -> String {
        match self {
            TreeElement::Model(model) => model.id().to_owned(),
            TreeElement::Group(group) => group.id.clone(),
            TreeElement::Element(element) => element.id.clone(),
        }
    }

    /// `children`.
    pub fn children(&self) -> Children {
        match self {
            TreeElement::Model(model) => model.children(),
            TreeElement::Group(group) => Rc::clone(&group.children),
            TreeElement::Element(element) => Rc::clone(&element.children),
        }
    }

    /// `parent`.
    pub fn parent(&self) -> Option<TreeElement> {
        let link = match self {
            TreeElement::Model(_) => return None,
            TreeElement::Group(group) => &group.parent,
            TreeElement::Element(element) => &element.parent,
        };
        link.borrow().as_ref().and_then(WeakTreeElement::upgrade)
    }

    /// `parent = parent`: o modelo não tem pai (`readonly parent = undefined`).
    fn set_parent(&self, parent: Option<&TreeElement>) {
        let link = match self {
            TreeElement::Model(_) => return,
            TreeElement::Group(group) => &group.parent,
            TreeElement::Element(element) => &element.parent,
        };
        *link.borrow_mut() = parent.map(TreeElement::downgrade);
    }

    /// `remove()`.
    pub fn remove(&self) {
        if let Some(parent) = self.parent() {
            let id = self.id();
            parent.children().borrow_mut().remove(&id);
        }
    }

    /// `TreeElement.findId(candidate, container)`.
    pub fn find_id(candidate: IdCandidate<'_>, container: &TreeElement) -> String {
        // complex id-computation which contains the origin/extension,
        // the parent path, and some dedupe logic when names collide
        let container_id = container.id();
        let children = container.children();
        let candidate_id = match candidate {
            IdCandidate::Name(name) => format!("{container_id}/{name}"),
            IdCandidate::Symbol(symbol) => {
                let candidate_id = format!("{container_id}/{}", symbol.name);
                if children.borrow().get(&candidate_id).is_some() {
                    format!(
                        "{container_id}/{}_{}_{}",
                        symbol.name, symbol.range.start_line_number, symbol.range.start_column
                    )
                } else {
                    candidate_id
                }
            }
        };

        let mut id = candidate_id.clone();
        let mut i = 0;
        while children.borrow().get(&id).is_some() {
            id = format!("{candidate_id}_{i}");
            i += 1;
        }

        id
    }

    /// `TreeElement.getElementById(id, element)`.
    pub fn get_element_by_id(id: &str, element: &TreeElement) -> Option<TreeElement> {
        if id.is_empty() {
            return None;
        }
        let id_units: Vec<u16> = id.encode_utf16().collect();
        let element_id_units: Vec<u16> = element.id().encode_utf16().collect();
        let len = common_prefix_length(&id_units, &element_id_units);
        if len == id_units.len() {
            return Some(element.clone());
        }
        if len < element_id_units.len() {
            return None;
        }
        for child in child_list(&element.children()) {
            if let Some(candidate) = TreeElement::get_element_by_id(id, &child) {
                return Some(candidate);
            }
        }
        None
    }

    /// `TreeElement.size(element)`.
    pub fn size(element: &TreeElement) -> usize {
        1 + child_list(&element.children()).iter().map(TreeElement::size).sum::<usize>()
    }

    /// `TreeElement.empty(element)`.
    pub fn empty(element: &TreeElement) -> bool {
        element.children().borrow().is_empty()
    }

    /// O elemento, se for um [`OutlineElement`].
    pub fn as_element(&self) -> Option<Rc<OutlineElement>> {
        match self {
            TreeElement::Element(element) => Some(Rc::clone(element)),
            _ => None,
        }
    }

    /// O grupo, se for um [`OutlineGroup`].
    pub fn as_group(&self) -> Option<Rc<OutlineGroup>> {
        match self {
            TreeElement::Group(group) => Some(Rc::clone(group)),
            _ => None,
        }
    }
}

/// `IOutlineMarker`: o intervalo do marcador e a gravidade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineMarker {
    pub range: Range,
    pub severity: MarkerSeverity,
}

/// `OutlineElement.marker`: `{ count, topSev }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutlineElementMarker {
    pub count: usize,
    pub top_sev: MarkerSeverity,
}

/// `OutlineElement`.
pub struct OutlineElement {
    pub id: String,
    parent: ParentLink,
    pub symbol: DocumentSymbol,
    pub children: Children,
    marker: Cell<Option<OutlineElementMarker>>,
}

impl OutlineElement {
    pub fn new(id: &str, parent: Option<&TreeElement>, symbol: DocumentSymbol) -> Rc<Self> {
        Rc::new(Self {
            id: id.to_owned(),
            parent: RefCell::new(parent.map(TreeElement::downgrade)),
            symbol,
            children: new_children(),
            marker: Cell::new(None),
        })
    }

    /// `marker`.
    pub fn marker(&self) -> Option<OutlineElementMarker> {
        self.marker.get()
    }
}

/// `OutlineGroup`.
pub struct OutlineGroup {
    pub id: String,
    parent: ParentLink,
    pub label: String,
    pub order: usize,
    pub children: Children,
}

impl OutlineGroup {
    pub fn new(id: &str, parent: Option<&TreeElement>, label: &str, order: usize) -> Rc<Self> {
        Rc::new(Self {
            id: id.to_owned(),
            parent: RefCell::new(parent.map(TreeElement::downgrade)),
            label: label.to_owned(),
            order,
            children: new_children(),
        })
    }

    /// `getItemEnclosingPosition(position)`.
    pub fn get_item_enclosing_position(&self, position: Position) -> Option<Rc<OutlineElement>> {
        Self::item_enclosing_position(position, &self.children)
    }

    /// `_getItemEnclosingPosition(position, children)`.
    fn item_enclosing_position(position: Position, children: &Children) -> Option<Rc<OutlineElement>> {
        for item in child_list(children).iter().filter_map(TreeElement::as_element) {
            if !item.symbol.range.contains_position(&position) {
                continue;
            }
            return Self::item_enclosing_position(position, &item.children).or(Some(item));
        }
        None
    }

    /// `updateMarker(marker)`.
    pub fn update_marker(&self, markers: &mut Vec<OutlineMarker>) {
        for child in child_list(&self.children).iter().filter_map(TreeElement::as_element) {
            Self::update_marker_of(markers, &child);
        }
    }

    /// `_updateMarker(markers, item)`.
    fn update_marker_of(markers: &mut Vec<OutlineMarker>, item: &Rc<OutlineElement>) {
        item.marker.set(None);

        // find the proper start index to check for item/marker overlap.
        let idx = binary_search2(markers.len(), |i| {
            f64::from(Range::compare_ranges_using_starts(Some(&markers[i].range), Some(&item.symbol.range)))
        });
        let mut start: usize;
        if idx < 0 {
            start = (!idx) as usize;
            if start > 0 && Range::are_intersecting(&markers[start - 1].range, &item.symbol.range) {
                start -= 1;
            }
        } else {
            start = idx as usize;
        }

        let mut end = start;
        let mut my_top_sev: Option<MarkerSeverity> = None;

        while end < markers.len() && Range::are_intersecting(&item.symbol.range, &markers[end].range) {
            // remove markers intersecting with this outline element
            // and store them in a 'private' array.
            let marker = markers[end];
            // `!myTopSev || marker.severity > myTopSev`: a gravidade 0 é falsa no JavaScript.
            if my_top_sev.is_none_or(|top| top.0 == 0 || marker.severity.0 > top.0) {
                my_top_sev = Some(marker.severity);
            }
            end += 1;
        }
        let mut my_markers: Vec<OutlineMarker> = markers.drain(start..end).collect();

        // Recurse into children and let them match markers that have matched
        // this outline element. This might remove markers from this element and
        // therefore we remember that we have had markers. That allows us to render
        // the dot, saying 'this element has children with markers'
        for child in child_list(&item.children).iter().filter_map(TreeElement::as_element) {
            Self::update_marker_of(&mut my_markers, &child);
        }

        if let Some(top_sev) = my_top_sev.filter(|top| top.0 != 0) {
            item.marker.set(Some(OutlineElementMarker { count: my_markers.len(), top_sev }));
        }
    }
}

/// `OutlineModel`.
pub struct OutlineModel {
    pub uri: URI,
    groups: RefCell<Children>,
    children: RefCell<Children>,
}

/// `OutlineModel.create`: a lista de provedores é a mesma? (`equals(a, b)` compara por identidade.)
fn same_providers(a: &[Rc<dyn DocumentSymbolProvider>], b: &[Rc<dyn DocumentSymbolProvider>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Rc::ptr_eq(a, b))
}

impl OutlineModel {
    /// `OutlineModel.create(registry, textModel, token)`.
    pub fn create(
        registry: &LanguageFeatureRegistry<dyn DocumentSymbolProvider>,
        text_model: &Rc<TextModel>,
        token: &CancellationToken,
    ) -> Promise<Rc<OutlineModel>> {
        let cts = CancellationTokenSource::new(Some(token));
        let result = OutlineModel::new(text_model.uri());
        let result_element = TreeElement::Model(Rc::clone(&result));
        let provider = registry.ordered(text_model, false);
        let cts_token = cts.token();

        let promises: Vec<Promise<()>> = provider
            .iter()
            .enumerate()
            .map(|(index, provider)| {
                let id = TreeElement::find_id(IdCandidate::Name(&format!("provider_{index}")), &result_element);
                let group = OutlineGroup::new(
                    &id,
                    Some(&result_element),
                    provider.display_name().unwrap_or("Unknown Outline Provider"),
                    index,
                );

                let result = Rc::clone(&result);
                let filled_group = Rc::clone(&group);
                provider
                    .provide_document_symbols(text_model, &cts_token)
                    .then_result(move |symbols| {
                        match symbols {
                            Ok(symbols) => {
                                for info in symbols.unwrap_or_default() {
                                    OutlineModel::make_outline_element(&info, &TreeElement::Group(Rc::clone(&filled_group)));
                                }
                            }
                            Err(error) => on_unexpected_external_error(&error),
                        }
                        Ok(filled_group)
                    })
                    .then(move |group| {
                        let element = TreeElement::Group(group);
                        if !TreeElement::empty(&element) {
                            result.groups.borrow().borrow_mut().insert(id, element);
                        } else {
                            element.remove();
                        }
                    })
            })
            .collect();

        let listener = {
            let event = registry.on_did_change();
            let (registry, text_model, cts, provider) =
                (registry.clone(), Rc::clone(text_model), cts.clone(), provider.clone());
            event.listen(move |_| {
                let new_provider = registry.ordered(&text_model, false);
                if !same_providers(&new_provider, &provider) {
                    cts.cancel();
                }
            })
        };

        let (registry, text_model, token) = (registry.clone(), Rc::clone(text_model), token.clone());
        Promise::all(promises)
            .then_promise(move |_| {
                if cts_token.is_cancellation_requested() && !token.is_cancellation_requested() {
                    OutlineModel::create(&registry, &text_model, &token)
                } else {
                    Promise::resolved(result.compact())
                }
            })
            .finally(move || {
                cts.dispose();
                listener.dispose();
                cts.dispose();
            })
    }

    /// `_makeOutlineElement(info, container)`.
    fn make_outline_element(info: &DocumentSymbol, container: &TreeElement) {
        let id = TreeElement::find_id(IdCandidate::Symbol(info), container);
        let res = OutlineElement::new(&id, Some(container), info.clone());
        if let Some(children) = &info.children {
            for child_info in children {
                OutlineModel::make_outline_element(child_info, &TreeElement::Element(Rc::clone(&res)));
            }
        }
        let id = res.id.clone();
        container.children().borrow_mut().insert(id, TreeElement::Element(res));
    }

    /// `OutlineModel.get(element)`: o modelo que contém o elemento.
    pub fn get(element: Option<&TreeElement>) -> Option<Rc<OutlineModel>> {
        let mut element = element.cloned();
        while let Some(current) = element {
            if let TreeElement::Model(model) = &current {
                return Some(Rc::clone(model));
            }
            element = current.parent();
        }
        None
    }

    /// `new OutlineModel(uri)`.
    pub fn new(uri: URI) -> Rc<Self> {
        let groups = new_children();
        Rc::new(Self { uri, children: RefCell::new(new_children()), groups: RefCell::new(groups) })
    }

    /// `readonly id = 'root'`.
    pub fn id(&self) -> &str {
        "root"
    }

    /// `children`.
    pub fn children(&self) -> Children {
        Rc::clone(&self.children.borrow())
    }

    /// O `this._groups = this.children as any` do `readyForTesting` do teste do original.
    pub fn ready_for_testing(&self) {
        *self.groups.borrow_mut() = self.children();
    }

    /// `_compact()`.
    fn compact(self: &Rc<Self>) -> Rc<Self> {
        let groups = Rc::clone(&self.groups.borrow());
        let mut count = 0;
        let entries: Vec<(String, TreeElement)> =
            groups.borrow().iter().map(|(key, group)| (key.clone(), group.clone())).collect();
        for (key, group) in entries {
            if TreeElement::empty(&group) {
                // empty
                groups.borrow_mut().remove(&key);
            } else {
                count += 1;
            }
        }
        if count != 1 {
            //
            *self.children.borrow_mut() = Rc::clone(&groups);
        } else {
            // adopt all elements of the first group
            let group = groups.borrow().values().next().cloned().expect("há exatamente um grupo");
            let me = TreeElement::Model(Rc::clone(self));
            for child in child_list(&group.children()) {
                child.set_parent(Some(&me));
                self.children().borrow_mut().insert(child.id(), child);
            }
        }
        Rc::clone(self)
    }

    /// `merge(other)`.
    pub fn merge(&self, other: &OutlineModel) -> bool {
        if self.uri.to_string(false) != other.uri.to_string(false) {
            return false;
        }
        if self.groups.borrow().borrow().len() != other.groups.borrow().borrow().len() {
            return false;
        }
        *self.groups.borrow_mut() = Rc::clone(&other.groups.borrow());
        *self.children.borrow_mut() = other.children();
        true
    }

    /// `getItemEnclosingPosition(position, context?)`.
    pub fn get_item_enclosing_position(
        &self,
        position: Position,
        context: Option<&Rc<OutlineElement>>,
    ) -> Option<Rc<OutlineElement>> {
        let mut preferred_group: Option<Rc<OutlineGroup>> = None;
        if let Some(context) = context {
            let mut candidate = TreeElement::Element(Rc::clone(context)).parent();
            while preferred_group.is_none() {
                let Some(current) = candidate else {
                    break;
                };
                if let TreeElement::Group(group) = &current {
                    preferred_group = Some(Rc::clone(group));
                }
                candidate = current.parent();
            }
        }

        let mut result: Option<Rc<OutlineElement>> = None;
        let groups = child_list(&self.groups.borrow());
        for group in groups.iter().filter_map(TreeElement::as_group) {
            result = group.get_item_enclosing_position(position);
            if result.is_some() && preferred_group.as_ref().is_none_or(|preferred| Rc::ptr_eq(preferred, &group)) {
                break;
            }
        }
        result
    }

    /// `getItemById(id)`.
    pub fn get_item_by_id(self: &Rc<Self>, id: &str) -> Option<TreeElement> {
        TreeElement::get_element_by_id(id, &TreeElement::Model(Rc::clone(self)))
    }

    /// `updateMarker(marker)`.
    pub fn update_marker(&self, marker: &mut [OutlineMarker]) {
        // sort markers by start range so that we can use
        // outline element starts for quicker look up
        marker.sort_by(|a, b| Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range)).cmp(&0));

        for group in child_list(&self.groups.borrow()).iter().filter_map(TreeElement::as_group) {
            group.update_marker(&mut marker.to_vec());
        }
    }

    /// `getTopLevelSymbols()`.
    pub fn get_top_level_symbols(&self) -> Vec<DocumentSymbol> {
        let mut roots: Vec<DocumentSymbol> = Vec::new();
        for child in child_list(&self.children()) {
            match &child {
                TreeElement::Element(element) => roots.push(element.symbol.clone()),
                other => {
                    roots.extend(child_list(&other.children()).iter().filter_map(TreeElement::as_element).map(|e| e.symbol.clone()))
                }
            }
        }
        roots.sort_by(|a, b| Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range)).cmp(&0));
        roots
    }

    /// `asListOfDocumentSymbols()`.
    pub fn as_list_of_document_symbols(&self) -> Vec<DocumentSymbol> {
        let roots = self.get_top_level_symbols();
        let mut bucket: Vec<DocumentSymbol> = Vec::new();
        OutlineModel::flatten_document_symbols(&mut bucket, &roots, "");
        bucket.sort_by(|a, b| {
            let by_start = Position::compare(&a.range.get_start_position(), &b.range.get_start_position());
            let order = if by_start != 0 {
                by_start
            } else {
                Position::compare(&b.range.get_end_position(), &a.range.get_end_position())
            };
            order.cmp(&0)
        });
        bucket
    }

    /// `_flattenDocumentSymbols(bucket, entries, overrideContainerLabel)`.
    fn flatten_document_symbols(bucket: &mut Vec<DocumentSymbol>, entries: &[DocumentSymbol], override_container_label: &str) {
        for entry in entries {
            bucket.push(DocumentSymbol {
                kind: entry.kind,
                tags: entry.tags.clone(),
                name: entry.name.clone(),
                detail: entry.detail.clone(),
                container_name: Some(
                    entry
                        .container_name
                        .clone()
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| override_container_label.to_owned()),
                ),
                range: entry.range,
                selection_range: entry.selection_range,
                children: None, // we flatten it...
            });

            // Recurse over children
            if let Some(children) = &entry.children {
                OutlineModel::flatten_document_symbols(bucket, children, &entry.name);
            }
        }
    }
}

/// O recorte de `IModelService` que o [`OutlineModelService`] lê (ver "Divergências").
pub trait ModelRemovedSource {
    /// `onModelRemoved`.
    fn on_model_removed(&self) -> Event<Rc<TextModel>>;
}

/// `IOutlineModelService`.
pub trait IOutlineModelService {
    /// `getOrCreate(model, token)`.
    fn get_or_create(&self, model: &Rc<TextModel>, token: &CancellationToken) -> Promise<Rc<OutlineModel>>;

    /// `getDebounceValue(textModel)`.
    fn get_debounce_value(&self, text_model: &TextModel) -> f64;

    /// `getCachedModels()`.
    fn get_cached_models(&self) -> Vec<Rc<OutlineModel>>;
}

je_instantiation::service_id!(pub I_OUTLINE_MODEL_SERVICE: dyn IOutlineModelService = "IOutlineModelService");

/// `CacheEntry`.
struct CacheEntry {
    version_id: i64,
    provider: Vec<Rc<dyn DocumentSymbolProvider>>,

    promise_cnt: Cell<i64>,
    source: CancellationTokenSource,
    promise: Promise<Rc<OutlineModel>>,
    model: RefCell<Option<Rc<OutlineModel>>>,
}

/// `OutlineModelService`.
pub struct OutlineModelService {
    disposables: Rc<DisposableStore>,
    language_features: LanguageFeatureRegistry<dyn DocumentSymbolProvider>,
    debounce_information: Rc<dyn IFeatureDebounceInformation>,
    cache: Rc<LRUCache<String, Rc<CacheEntry>>>,
}

impl OutlineModelService {
    /// `new OutlineModelService(languageFeaturesService, debounces, modelService)`, com o registro de
    /// `documentSymbolProvider` no lugar do `ILanguageFeaturesService`.
    pub fn new(
        language_features: LanguageFeatureRegistry<dyn DocumentSymbolProvider>,
        debounces: &dyn ILanguageFeatureDebounceService,
        model_service: &dyn ModelRemovedSource,
    ) -> Self {
        let debounce_information = debounces.for_feature(
            Rc::new(language_features.clone()),
            "DocumentSymbols",
            Some(DebounceConfig { min: Some(350.0), ..DebounceConfig::default() }),
        );
        let cache: Rc<LRUCache<String, Rc<CacheEntry>>> = Rc::new(LRUCache::new(15, 0.7));

        // don't cache outline models longer than their text model
        let disposables = DisposableStore::new();
        let removed_cache = Rc::clone(&cache);
        disposables.add(model_service.on_model_removed().listen(move |text_model| {
            removed_cache.delete(&text_model.id().to_owned());
        }));

        Self { disposables, language_features, debounce_information, cache }
    }
}

impl IDisposable for OutlineModelService {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

impl IOutlineModelService for OutlineModelService {
    fn get_or_create(&self, text_model: &Rc<TextModel>, token: &CancellationToken) -> Promise<Rc<OutlineModel>> {
        let registry = &self.language_features;
        let provider = registry.ordered(text_model, false);
        let model_id = text_model.id().to_owned();

        let mut data = self.cache.get(&model_id);
        let stale = data
            .as_ref()
            .is_none_or(|data| data.version_id != text_model.get_version_id() || !same_providers(&data.provider, &provider));
        if stale {
            let source = CancellationTokenSource::new(None);
            let entry = Rc::new(CacheEntry {
                version_id: text_model.get_version_id(),
                provider,
                promise_cnt: Cell::new(0),
                source: source.clone(),
                promise: OutlineModel::create(registry, text_model, &source.token()),
                model: RefCell::new(None),
            });
            self.cache.set(model_id.clone(), Rc::clone(&entry));

            let now = runtime::date_now();
            let resolved_entry = Rc::clone(&entry);
            let debounce_information = Rc::clone(&self.debounce_information);
            let debounced_model = Rc::clone(text_model);
            let failed_cache = Rc::clone(&self.cache);
            let failed_id = model_id.clone();
            let _ = entry
                .promise
                .then(move |outline_model| {
                    *resolved_entry.model.borrow_mut() = Some(outline_model);
                    debounce_information.update(&debounced_model, runtime::date_now().saturating_sub(now) as f64);
                })
                .catch(move |_error| {
                    failed_cache.delete(&failed_id);
                });
            data = Some(entry);
        }
        let data = data.expect("a entrada do cache existe");

        if let Some(model) = data.model.borrow().clone() {
            // resolved -> return data
            return Promise::resolved(model);
        }

        // increase usage counter
        data.promise_cnt.set(data.promise_cnt.get() + 1);

        let counted = Rc::clone(&data);
        let cache = Rc::clone(&self.cache);
        let listener = token.on_cancellation_requested().listen(move |_| {
            // last -> cancel provider request, remove cached promise
            counted.promise_cnt.set(counted.promise_cnt.get() - 1);
            if counted.promise_cnt.get() == 0 {
                counted.source.cancel();
                cache.delete(&model_id);
            }
        });

        data.promise.finally(move || listener.dispose())
    }

    fn get_debounce_value(&self, text_model: &TextModel) -> f64 {
        self.debounce_information.get(text_model)
    }

    fn get_cached_models(&self) -> Vec<Rc<OutlineModel>> {
        self.cache.values().filter_map(|entry| entry.model.borrow().clone()).collect()
    }
}
