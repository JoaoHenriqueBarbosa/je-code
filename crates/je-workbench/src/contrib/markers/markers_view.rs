//! Porte parcial de `src/vs/workbench/contrib/markers/browser/markersView.ts`: o contrato `IProblemsWidget`, que a tabela
//! (`markersTable.ts`, [`super::markers_table`]) e a árvore implementam, a árvore (`MarkersTree`) e o arrastar e soltar
//! dela (`MarkersListDnDHandler`).
//!
//! Estado (fatia 2a): as linhas 1 a 107 (as importações, `createResourceMarkersIterator`, `IMarkersPanelState` e
//! `IProblemsWidget`) e as linhas 922 até o fim (`MarkersTree` e `MarkersListDnDHandler`). **Falta a fatia 2b:** a classe
//! `MarkersView` como `ViewPane` (linhas 109 a 920: a fonte de dados, as ações da barra, o menu de contexto, o estado salvo e
//! a troca entre tabela e árvore) e os 3 casos de `test/browser/markersView.test.ts`.
//!
//! Notas de tradução:
//! - **Uniões `MarkerElement | MarkerTableItem`.** São o enum [`ProblemsElement`]. As uniões de eventos
//!   (`Event<ITreeEvent<MarkerElement | null>> | Event<ITableEvent<MarkerTableItem>>`) são um único tipo por evento
//!   ([`ProblemsWidgetEvent`], [`ProblemsContextMenuEvent`]), porque um trait não tem tipos de retorno em união; o `null` da
//!   árvore (a raiz) e os elementos da tabela são `Option<ProblemsElement>`.
//! - **`ITreeContextMenuEvent`.** Ver `table_types`: o menu de contexto da tabela é o do ponteiro, com o elemento, o índice
//!   (da tabela) e o evento de origem. Na árvore o `index` é `None`.
//! - **Estado mutável.** Os métodos recebem `&self`: quem implementa guarda o estado em `Cell` e `RefCell`.
//! - **`MarkersTree extends WorkbenchObjectTree`.** É a composição: o campo `tree` é a `WorkbenchObjectTree<MarkerElement,
//!   FilterData>` (acessível por [`MarkersTree::tree`]). Os parâmetros decorados do construtor (`@IInstantiationService`,
//!   `@IContextKeyService`, `@IListService`, `@IThemeService`, `@IConfigurationService`) são os [`WorkbenchTreeServices`], e
//!   as `IWorkbenchObjectTreeOptions` são o par `ObjectTreeOptions` e `IWorkbenchTreeOptions` (como a `WorkbenchObjectTree`).
//!   A chave de contexto `problemsVisibility` é ligada ao serviço de chaves do original (o do chamador, não o escopado da
//!   árvore).
//! - **`getHTMLElement().focus()`.** É o `dom_focus` da árvore (o foco do nó da lista, que respeita o cabeçalho fixo).
//! - **`set ariaLabel`.** A árvore do porte não tem o setter: o rótulo vai direto no nó da lista, como a tabela faz.
//! - **`container.style.height`, `classList.toggle('hidden')`.** São os auxiliares [`layout_container`],
//!   [`is_container_visible`] e [`toggle_container_visibility`], compartilhados com a tabela (o `.hidden` é `display: none`).
//! - **`null` como referência.** O `findMarkerNode` que não acha o marcador devolve `null`, a raiz da árvore; é `None`.
//! - **`ResourceListDnDHandler<MarkerElement | MarkerTableItem>`.** O [`MarkersListDnDHandler`] guarda o
//!   `ResourceListDnDHandler<ProblemsElement>` (a união) e implementa o `IListDragAndDrop` da árvore (`Option<MarkerElement>`,
//!   onde o `None` é a raiz, que não mapeia para recurso) convertendo os elementos. O `@IInstantiationService` da subclasse é o
//!   [`FillEditorsDragData`] injetado (ver `browser::dnd`). O `MarkerTableItem` é um `Marker` no original (`extends Marker`),
//!   então o `onWillDragElements` o trata como marcador.

use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_editor::opener_service::range_to_text_editor_selection;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::markers::markers::RelatedInformation as IRelatedInformation;
use je_platform::opener::editor_types::TextEditorSelection;
use je_platform::opener::opener::with_selection;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Ui};
use je_widgets::dnd::{DragEvent, IDragAndDropData};
use je_widgets::list::list_dnd::{ElementsDragAndDropData, IListDragAndDrop, ListViewTargetSector};
use je_widgets::list::list_types::{BrowserEvent, IListVirtualDelegate};
use je_widgets::tree::object_tree::{ObjectTreeMethods, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::ObjectTreeModelSetChildrenOptions;
use je_widgets::tree::tree_types::{IObjectTreeElement, ITreeRenderer, TreeDragOverResult, TreeEvent, TreeNodeApi, tree_error};

use crate::browser::dnd::{FillEditorsDragData, ResourceListDnDHandler};
use je_widgets::list::resource_navigator::IOpenEvent;
use crate::browser::workbench_tree::{IWorkbenchTreeOptions, WorkbenchObjectTree, WorkbenchTreeServices};
use crate::platform::dnd::dnd::{MarkerTransferData, fill_in_markers_drag_data};

use super::markers_common::markers_tree_visibility_context_key;
use super::markers_filter_options::FilterOptions;
use super::markers_model::{Marker, MarkerElement, MarkerTableItem, ResourceMarkers};
use super::markers_tree_viewer::{FilterData, MarkersTreeModel};

/// `createResourceMarkersIterator(resourceMarkers)`: os marcadores do recurso, cada um com a sua informação relacionada.
fn create_resource_markers_iterator(resource_markers: &ResourceMarkers) -> Vec<IObjectTreeElement<MarkerElement>> {
    resource_markers
        .markers()
        .into_iter()
        .map(|marker| {
            let children = marker.related_information.iter().map(|related| IObjectTreeElement::new(MarkerElement::RelatedInformation(related.clone()))).collect();
            IObjectTreeElement { children: Some(children), ..IObjectTreeElement::new(MarkerElement::Marker(marker)) }
        })
        .collect()
}

/// `MarkerElement | MarkerTableItem`.
#[derive(Clone)]
pub enum ProblemsElement {
    Element(MarkerElement),
    TableItem(Rc<MarkerTableItem>),
}

/// `ITreeEvent<MarkerElement | null> | ITableEvent<MarkerTableItem>`: os elementos e o evento de origem.
#[derive(Clone)]
pub struct ProblemsWidgetEvent {
    pub elements: Vec<Option<ProblemsElement>>,
    pub browser_event: Option<BrowserEvent>,
}

/// `ITreeContextMenuEvent<MarkerElement | null> | ITableContextMenuEvent<MarkerTableItem>`.
#[derive(Clone)]
pub struct ProblemsContextMenuEvent {
    pub element: Option<ProblemsElement>,
    pub index: Option<usize>,
    pub browser_event: BrowserEvent,
    /// O `browserEvent.isHandledByList`, compartilhado entre quem recebe o mesmo evento.
    pub is_handled_by_list: Rc<Cell<bool>>,
}

/// `IProblemsWidget`.
pub trait IProblemsWidget: IDisposable {
    fn context_key_service(&self) -> Rc<dyn IContextKeyService>;

    fn on_context_menu(&self) -> Event<ProblemsContextMenuEvent>;
    fn on_did_change_focus(&self) -> Event<ProblemsWidgetEvent>;
    fn on_did_change_selection(&self) -> Event<ProblemsWidgetEvent>;
    fn on_did_open(&self) -> Event<IOpenEvent<ProblemsElement>>;

    fn collapse_markers(&self);
    fn dom_focus(&self);
    fn filter_markers(&self, resource_markers: &[Rc<ResourceMarkers>], filter_options: Rc<FilterOptions>);
    fn get_focus(&self) -> Vec<Option<ProblemsElement>>;
    fn get_html_element(&self) -> NodeId;
    fn get_relative_top(&self, location: Option<&ProblemsElement>) -> Option<f64>;
    fn get_selection(&self) -> Vec<Option<ProblemsElement>>;
    fn get_visible_item_count(&self) -> usize;
    fn layout(&self, height: f64, width: f64);
    fn reset(&self, resource_markers: &[Rc<ResourceMarkers>]);
    fn reveal_markers(&self, active_resource: Option<&Rc<ResourceMarkers>>, focus: bool, last_selected_relative_top: f64);
    fn set_aria_label(&self, label: &str);
    fn set_marker_selection(&self, selection: Option<&[Rc<Marker>]>, focus: Option<&[Rc<Marker>]>);
    fn toggle_visibility(&self, hide: bool);
    fn update(&self, resource_markers: &[Rc<ResourceMarkers>]);
    fn update_marker(&self, marker: &Rc<Marker>);
}

// ---------------------------------------------------------------------------------------------------------------------
// Auxiliares do contêiner (a tabela e a árvore)
// ---------------------------------------------------------------------------------------------------------------------

/// `container.style.height = `${height}px``.
pub(super) fn layout_container(ui: &Ui, container: NodeId, height: f64) {
    let mut tree = ui.tree_mut();
    let bounds = tree.bounds(container);
    tree.set_fill(container, true, false);
    tree.set_bounds(container, Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y0 + height));
}

/// `!container.classList.contains('hidden')`.
pub(super) fn is_container_visible(ui: &Ui, container: NodeId) -> bool {
    !ui.tree().has_class(container, "hidden")
}

/// `container.classList.toggle('hidden', hide)`; o `.hidden` de `.markers-panel .markers-panel-container` é `display: none`.
pub(super) fn toggle_container_visibility(ui: &Ui, container: NodeId, hide: bool) {
    let mut tree = ui.tree_mut();
    tree.toggle_class(container, "hidden", hide);
    tree.style_mut(container).visible = !hide;
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkersTree
// ---------------------------------------------------------------------------------------------------------------------

/// `Option<MarkerElement>` (o `MarkerElement | null` da árvore) como o elemento do widget.
fn problems_element_of(element: Option<MarkerElement>) -> Option<ProblemsElement> {
    element.map(ProblemsElement::Element)
}

/// `ITreeEvent<MarkerElement | null>` como o evento do `IProblemsWidget`.
fn to_widget_event(event: &TreeEvent<Option<MarkerElement>>) -> ProblemsWidgetEvent {
    ProblemsWidgetEvent { elements: event.elements.iter().cloned().map(problems_element_of).collect(), browser_event: event.browser_event }
}

/// `MarkersTree`.
pub struct MarkersTree {
    tree: Rc<WorkbenchObjectTree<MarkerElement, FilterData>>,
    ui: Ui,
    container: NodeId,
    visibility_context_key: Rc<dyn IContextKey>,
}

impl MarkersTree {
    /// `new MarkersTree(user, container, delegate, renderers, options, ...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<MarkerElement>>,
        renderers: Vec<Rc<dyn ITreeRenderer<MarkersTreeModel>>>,
        options: ObjectTreeOptions<MarkerElement, FilterData>,
        workbench: IWorkbenchTreeOptions,
        services: &WorkbenchTreeServices,
    ) -> Rc<Self> {
        let tree = WorkbenchObjectTree::<MarkerElement, FilterData>::new(user, ui, container, delegate, renderers, options, workbench, services);
        let visibility_context_key = markers_tree_visibility_context_key().bind_to(&*services.context_key_service);

        Rc::new(Self { tree, ui: ui.clone(), container, visibility_context_key })
    }

    /// A `WorkbenchObjectTree` por baixo (o que a classe herda no original).
    pub fn tree(&self) -> &Rc<WorkbenchObjectTree<MarkerElement, FilterData>> {
        &self.tree
    }

    /// `isVisible()`.
    pub fn is_visible(&self) -> bool {
        is_container_visible(&self.ui, self.container)
    }

    /// `findMarkerNode(marker)`: o elemento da árvore com o mesmo marcador cru, ou a raiz (`null`) se não há.
    fn find_marker_node(&self, marker: &Marker) -> Option<MarkerElement> {
        for resource_node in self.tree.get_node(&None).children() {
            for marker_node in resource_node.children() {
                if let Some(MarkerElement::Marker(element)) = marker_node.element() {
                    if Rc::ptr_eq(&element.marker, &marker.marker) {
                        return Some(MarkerElement::Marker(element));
                    }
                }
            }
        }

        None
    }

    /// `hasSelectedMarkerFor(resource)`.
    fn has_selected_marker_for(&self, resource: &ResourceMarkers) -> bool {
        match self.tree.get_selection().first() {
            Some(Some(MarkerElement::Marker(marker))) => resource.has(marker.resource()),
            _ => false,
        }
    }
}

impl IProblemsWidget for MarkersTree {
    fn context_key_service(&self) -> Rc<dyn IContextKeyService> {
        self.tree.context_key_service()
    }

    fn on_context_menu(&self) -> Event<ProblemsContextMenuEvent> {
        self.tree.on_context_menu().map(
            |event| ProblemsContextMenuEvent {
                element: event.element.clone().flatten().map(ProblemsElement::Element),
                index: None,
                browser_event: event.browser_event,
                is_handled_by_list: event.is_handled_by_list.clone(),
            },
            None,
        )
    }

    fn on_did_change_focus(&self) -> Event<ProblemsWidgetEvent> {
        self.tree.on_did_change_focus().map(to_widget_event, None)
    }

    fn on_did_change_selection(&self) -> Event<ProblemsWidgetEvent> {
        self.tree.on_did_change_selection().map(to_widget_event, None)
    }

    fn on_did_open(&self) -> Event<IOpenEvent<ProblemsElement>> {
        self.tree.on_did_open().map(
            |event| IOpenEvent {
                editor_options: event.editor_options.clone(),
                side_by_side: event.side_by_side,
                element: ProblemsElement::Element(event.element.clone()),
                browser_event: event.browser_event,
            },
            None,
        )
    }

    fn collapse_markers(&self) {
        self.tree.collapse_all();
        self.tree.set_selection(&[], None);
        self.tree.set_focus(&[], None);
        self.tree.dom_focus();
        self.tree.focus_first(None, None);
    }

    fn dom_focus(&self) {
        self.tree.dom_focus();
    }

    fn filter_markers(&self, _resource_markers: &[Rc<ResourceMarkers>], _filter_options: Rc<FilterOptions>) {
        self.tree.refilter();
    }

    fn get_focus(&self) -> Vec<Option<ProblemsElement>> {
        self.tree.get_focus().into_iter().map(problems_element_of).collect()
    }

    fn get_html_element(&self) -> NodeId {
        self.tree.get_html_element()
    }

    fn get_relative_top(&self, location: Option<&ProblemsElement>) -> Option<f64> {
        let location = match location {
            None => None,
            Some(ProblemsElement::Element(element)) => Some(element.clone()),
            // Um item de tabela não está na árvore: o modelo lança o mesmo erro.
            Some(ProblemsElement::TableItem(_)) => je_base::errors::throw(tree_error("MarkersView", "Tree element not found")),
        };
        self.tree.get_relative_top(&location)
    }

    fn get_selection(&self) -> Vec<Option<ProblemsElement>> {
        self.tree.get_selection().into_iter().map(problems_element_of).collect()
    }

    fn get_visible_item_count(&self) -> usize {
        let mut filtered = 0;
        let root = self.tree.get_node(&None);

        for resource_marker_node in root.children() {
            for marker_node in resource_marker_node.children() {
                if resource_marker_node.visible() && marker_node.visible() {
                    filtered += 1;
                }
            }
        }

        filtered
    }

    fn layout(&self, height: f64, width: f64) {
        layout_container(&self.ui, self.container, height);
        self.tree.layout(Some(height), Some(width));
    }

    fn reset(&self, resource_markers: &[Rc<ResourceMarkers>]) {
        let children = resource_markers
            .iter()
            .map(|resource_markers| IObjectTreeElement {
                children: Some(create_resource_markers_iterator(resource_markers)),
                ..IObjectTreeElement::new(MarkerElement::ResourceMarkers(resource_markers.clone()))
            })
            .collect();
        self.tree.set_children(&None, children, ObjectTreeModelSetChildrenOptions::default());
    }

    fn reveal_markers(&self, active_resource: Option<&Rc<ResourceMarkers>>, focus: bool, last_selected_relative_top: f64) {
        if let Some(active_resource) = active_resource {
            let element = MarkerElement::ResourceMarkers(active_resource.clone());
            if self.tree.has_element(&element) {
                let location = Some(element);
                if !self.tree.is_collapsed(&location) && self.has_selected_marker_for(active_resource) {
                    let selection = self.tree.get_selection();
                    self.tree.reveal(&selection[0], Some(last_selected_relative_top));
                    if focus {
                        self.tree.set_focus(&selection, None);
                    }
                } else {
                    self.tree.expand(&location, false);
                    self.tree.reveal(&location, Some(0.0));

                    if focus {
                        self.tree.set_focus(std::slice::from_ref(&location), None);
                        self.tree.set_selection(std::slice::from_ref(&location), None);
                    }
                }
            }
        } else if focus {
            self.tree.set_selection(&[], None);
            self.tree.focus_first(None, None);
        }
    }

    fn set_aria_label(&self, label: &str) {
        self.ui.tree_mut().set_label(self.tree.get_html_element(), label);
    }

    fn set_marker_selection(&self, selection: Option<&[Rc<Marker>]>, focus: Option<&[Rc<Marker>]>) {
        if !self.is_visible() {
            return;
        }

        if let Some(selection) = selection.filter(|selection| !selection.is_empty()) {
            let locations = |markers: &[Rc<Marker>]| markers.iter().map(|marker| self.find_marker_node(marker)).collect::<Vec<_>>();
            self.tree.set_selection(&locations(selection), None);

            match focus.filter(|focus| !focus.is_empty()) {
                Some(focus) => self.tree.set_focus(&locations(focus), None),
                None => self.tree.set_focus(&[self.find_marker_node(&selection[0])], None),
            }

            self.tree.reveal(&self.find_marker_node(&selection[0]), None);
        } else if self.tree.get_selection().is_empty() {
            let marker = match self.tree.first_visible_element() {
                Some(Some(MarkerElement::ResourceMarkers(resource_markers))) => resource_markers.markers().into_iter().next(),
                Some(Some(MarkerElement::Marker(marker))) => Some(marker),
                _ => None,
            };

            if let Some(marker) = marker {
                let location = Some(MarkerElement::Marker(marker));
                self.tree.set_selection(std::slice::from_ref(&location), None);
                self.tree.set_focus(std::slice::from_ref(&location), None);
                self.tree.reveal(&location, None);
            }
        }
    }

    fn toggle_visibility(&self, hide: bool) {
        self.visibility_context_key.set(Dynamic::Bool(!hide));
        toggle_container_visibility(&self.ui, self.container, hide);
    }

    fn update(&self, resource_markers: &[Rc<ResourceMarkers>]) {
        for resource_marker in resource_markers {
            let element = MarkerElement::ResourceMarkers(resource_marker.clone());
            if self.tree.has_element(&element) {
                self.tree.set_children(&Some(element.clone()), create_resource_markers_iterator(resource_marker), ObjectTreeModelSetChildrenOptions::default());
                self.tree.rerender_element(Some(&element));
            }
        }
    }

    fn update_marker(&self, marker: &Rc<Marker>) {
        self.tree.rerender_element(Some(&MarkerElement::Marker(marker.clone())));
    }
}

impl IDisposable for MarkersTree {
    fn dispose(&self) {
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MarkersTree"
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkersListDnDHandler
// ---------------------------------------------------------------------------------------------------------------------

/// O `TextEditorSelection` de uma informação relacionada (que tem os quatro campos de posição).
fn related_information_selection(raw: &IRelatedInformation) -> TextEditorSelection {
    TextEditorSelection {
        start_line_number: raw.start_line_number as f64,
        start_column: raw.start_column as f64,
        end_line_number: Some(raw.end_line_number as f64),
        end_column: Some(raw.end_column as f64),
    }
}

/// O `toResource` do `MarkersListDnDHandler`.
fn resource_of(element: &ProblemsElement) -> Option<URI> {
    match element {
        ProblemsElement::TableItem(item) => Some(with_selection(item.base.resource(), &range_to_text_editor_selection(&item.base.range()))),
        ProblemsElement::Element(MarkerElement::ResourceMarkers(resource_markers)) => Some(resource_markers.resource.clone()),
        ProblemsElement::Element(MarkerElement::Marker(marker)) => Some(with_selection(marker.resource(), &range_to_text_editor_selection(&marker.range()))),
        ProblemsElement::Element(MarkerElement::RelatedInformation(related)) => Some(with_selection(&related.raw.resource, &related_information_selection(&related.raw))),
    }
}

/// O `MarkerTransferData` de um elemento arrastado (`undefined` não existe: todo elemento tem um).
fn marker_transfer_data_of(element: &ProblemsElement) -> MarkerTransferData {
    match element {
        ProblemsElement::TableItem(item) => MarkerTransferData::Marker(item.base.marker.clone()),
        ProblemsElement::Element(MarkerElement::RelatedInformation(related)) => MarkerTransferData::Marker(related.marker.clone()),
        ProblemsElement::Element(MarkerElement::Marker(marker)) => MarkerTransferData::Marker(marker.marker.clone()),
        ProblemsElement::Element(MarkerElement::ResourceMarkers(resource_markers)) => MarkerTransferData::Uri(resource_markers.resource.clone()),
    }
}

/// `onWillDragElements(elements, originalEvent)` do `MarkersListDnDHandler`.
fn on_will_drag_elements(elements: &[ProblemsElement], original_event: &DragEvent) {
    let data: Vec<MarkerTransferData> = elements.iter().map(marker_transfer_data_of).collect();

    if data.is_empty() {
        return;
    }

    fill_in_markers_drag_data(&data, original_event);
}

/// Os elementos da árvore (sem a raiz, que não mapeia para recurso) como a união do `ResourceListDnDHandler`.
fn problems_elements_of(elements: &[Option<MarkerElement>]) -> Vec<ProblemsElement> {
    elements.iter().flatten().cloned().map(ProblemsElement::Element).collect()
}

/// `MarkersListDnDHandler extends ResourceListDnDHandler<MarkerElement | MarkerTableItem>`.
pub struct MarkersListDnDHandler {
    base: ResourceListDnDHandler<ProblemsElement, TreeDragOverResult>,
}

impl MarkersListDnDHandler {
    pub fn new(fill_editors_drag_data: FillEditorsDragData) -> Self {
        Self { base: ResourceListDnDHandler::new(Rc::new(resource_of), fill_editors_drag_data).with_will_drag_elements(Rc::new(on_will_drag_elements)) }
    }
}

impl IListDragAndDrop<Option<MarkerElement>, TreeDragOverResult> for MarkersListDnDHandler {
    fn get_drag_uri(&self, element: &Option<MarkerElement>) -> Option<String> {
        problems_element_of(element.clone()).and_then(|element| self.base.get_drag_uri(&element))
    }

    fn get_drag_label(&self, elements: &[Option<MarkerElement>], original_event: &DragEvent) -> Option<String> {
        self.base.get_drag_label(&problems_elements_of(elements), original_event)
    }

    fn on_drag_start(&self, data: &Rc<dyn IDragAndDropData>, original_event: &DragEvent) {
        let Some(elements) = data.as_any().downcast_ref::<ElementsDragAndDropData<Option<MarkerElement>>>() else {
            return;
        };
        let data: Rc<dyn IDragAndDropData> = Rc::new(elements.derive(problems_elements_of(&elements.elements)));
        self.base.on_drag_start(&data, original_event);
    }

    fn on_drag_over(
        &self,
        data: &Rc<dyn IDragAndDropData>,
        target_element: Option<&Option<MarkerElement>>,
        target_index: Option<usize>,
        target_sector: Option<ListViewTargetSector>,
        original_event: &DragEvent,
    ) -> TreeDragOverResult {
        let target = target_element.and_then(|element| problems_element_of(element.clone()));
        self.base.on_drag_over(data, target.as_ref(), target_index, target_sector, original_event)
    }

    fn drop(
        &self,
        data: &Rc<dyn IDragAndDropData>,
        target_element: Option<&Option<MarkerElement>>,
        target_index: Option<usize>,
        target_sector: Option<ListViewTargetSector>,
        original_event: &DragEvent,
    ) {
        let target = target_element.and_then(|element| problems_element_of(element.clone()));
        self.base.drop(data, target.as_ref(), target_index, target_sector, original_event);
    }
}
