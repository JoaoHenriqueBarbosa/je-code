//! Porte parcial de `src/vs/workbench/contrib/markers/browser/markersView.ts`: por enquanto só o contrato `IProblemsWidget`,
//! que a tabela (`markersTable.ts`, [`super::markers_table`]) implementa e que a árvore (`MarkersTree`) implementará. O
//! `MarkersView` como `ViewPane`, a árvore, a fonte de dados e as ações da barra são a próxima fatia e entram neste arquivo.
//!
//! Notas de tradução:
//! - **Uniões `MarkerElement | MarkerTableItem`.** São o enum [`ProblemsElement`]. As uniões de eventos
//!   (`Event<ITreeEvent<MarkerElement | null>> | Event<ITableEvent<MarkerTableItem>>`) são um único tipo por evento
//!   ([`ProblemsWidgetEvent`], [`ProblemsContextMenuEvent`]), porque um trait não tem tipos de retorno em união; o `null` da
//!   árvore (a raiz) e os elementos da tabela são `Option<ProblemsElement>`.
//! - **`ITreeContextMenuEvent`.** Ver `table_types`: o menu de contexto da tabela é o do ponteiro, com o elemento, o índice
//!   (da tabela) e o evento de origem.
//! - **Estado mutável.** Os métodos recebem `&self`: quem implementa guarda o estado em `Cell` e `RefCell`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_ui::NodeId;
use je_widgets::list::list_types::BrowserEvent;

use crate::browser::resource_navigator::IOpenEvent;

use super::markers_filter_options::FilterOptions;
use super::markers_model::{Marker, MarkerElement, MarkerTableItem, ResourceMarkers};

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
