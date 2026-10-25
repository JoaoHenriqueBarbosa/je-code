//! Porte de `src/vs/workbench/contrib/notebook/browser/viewParts/notebookViewZones.ts`: as view zones do notebook, o espaço
//! em branco entre células que o `NotebookCellsLayout` reserva e que um contribuinte preenche com um nó de DOM.
//!
//! Divergências:
//! - O nó de DOM (`domNode`, `FastDomNode`, `setTop`, `setHeight`, `setDisplay`) não existe: a camada lógica guarda só o que
//!   o layout lê (`afterModelPosition`, `heightInPx`) e o estado `isInHiddenArea`. O `getViewZoneLayoutInfo` devolve o mesmo
//!   `{ height, top }`.
//! - O `NotebookCellListView` (que guarda o `NotebookCellsLayout`) e o `ICoordinatesConverter` são a própria
//!   [`NotebookCellList`], mantida por `Weak` (o dono da view zone é a lista, como no original).
//! - O `_zones` é um objeto JavaScript cujas chaves são inteiros em texto, iteradas em ordem crescente; o porte usa um
//!   `BTreeMap` pelo número do id.
//! - `safeInvoke1Arg` captura a exceção do callback e a reporta a `onUnexpectedError`; o porte captura o pânico igual.
//! - Os `changeAccessor.addZone = invalidFunc` viram um sinalizador que invalida o acessor depois do callback.
//! - O comando de desenvolvedor `notebook.developer.addViewZones` (`ToggleNotebookViewZoneDeveloperAction`) depende do
//!   `IEditorService` e do editor de notebook ativo, que são a Fase 6.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};

use crate::contrib::notebook::browser::view::notebook_cell_list::NotebookCellList;
use crate::contrib::notebook::browser::view_model::cell_view_model_types::INotebookViewCellsUpdateEvent;

/// `INotebookViewZone` sem o `domNode`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct INotebookViewZone {
    /// Use 0 to insert a view zone before the first cell.
    pub after_model_position: i64,
    pub height_in_px: f64,
}

/// `IZoneWidget` sem o `domNode`.
#[derive(Clone)]
struct IZoneWidget {
    whitespace_id: String,
    is_in_hidden_area: bool,
    zone: INotebookViewZone,
}

/// `INotebookViewZoneChangeAccessor`, invalidado quando o callback de `changeViewZones` termina.
pub struct INotebookViewZoneChangeAccessor {
    view_zones: Rc<NotebookViewZones>,
    zones_have_changed: Cell<bool>,
    valid: Cell<bool>,
}

impl INotebookViewZoneChangeAccessor {
    fn check_valid(&self) {
        if !self.valid.get() {
            errors::throw(Error::new("Invalid notebook view zone change accessor"));
        }
    }

    pub fn add_zone(&self, zone: INotebookViewZone) -> String {
        self.check_valid();
        self.zones_have_changed.set(true);
        self.view_zones.add_zone(zone)
    }

    pub fn remove_zone(&self, id: &str) {
        self.check_valid();
        self.zones_have_changed.set(true);
        self.view_zones.remove_zone(id);
    }

    pub fn layout_zone(&self, id: &str) {
        self.check_valid();
        self.zones_have_changed.set(true);
        self.view_zones.layout_zone(id);
    }
}

/// `NotebookViewZones`.
pub struct NotebookViewZones {
    list: Weak<NotebookCellList>,
    zones: RefCell<BTreeMap<u64, IZoneWidget>>,
}

impl NotebookViewZones {
    pub fn new(list: Weak<NotebookCellList>) -> Rc<Self> {
        Rc::new(Self { list, zones: RefCell::new(BTreeMap::new()) })
    }

    fn list(&self) -> Rc<NotebookCellList> {
        self.list.upgrade().expect("a lista de células foi descartada antes das view zones")
    }

    fn zone_key(id: &str) -> Option<u64> {
        id.parse().ok()
    }

    fn zone_widget(&self, id: &str) -> Option<IZoneWidget> {
        self.zones.borrow().get(&Self::zone_key(id)?).cloned()
    }

    /// Os ids na ordem de iteração do objeto JavaScript (inteiros crescentes).
    fn zone_ids(&self) -> Vec<u64> {
        self.zones.borrow().keys().copied().collect()
    }

    pub fn change_view_zones(self: &Rc<Self>, callback: impl FnOnce(&INotebookViewZoneChangeAccessor)) -> bool {
        let accessor = INotebookViewZoneChangeAccessor { view_zones: self.clone(), zones_have_changed: Cell::new(false), valid: Cell::new(true) };

        if let Err(error) = errors::catch(|| callback(&accessor)) {
            errors::on_unexpected_error(&error);
        }

        // Invalidate changeAccessor
        accessor.valid.set(false);

        accessor.zones_have_changed.get()
    }

    /// `getViewZoneLayoutInfo(viewZoneId)`: `{ height, top }`.
    pub fn get_view_zone_layout_info(&self, view_zone_id: &str) -> Option<(f64, f64)> {
        let zone_widget = self.zone_widget(view_zone_id)?;
        let top = self.list().get_whitespace_position(&zone_widget.whitespace_id);
        let height = zone_widget.zone.height_in_px;
        Some((height, top))
    }

    pub fn on_cells_changed(&self, e: &INotebookViewCellsUpdateEvent) {
        for (start, deleted, new_cells) in e.splices.iter().rev() {
            let from_index = *start as i64;
            let to_index = (*start + *deleted) as i64;

            // 1, 2, 0
            // delete cell index 1 and 2
            // from index 1, to index 3 (exclusive): [1, 3)
            // if we have whitespace afterModelPosition 3, which is after cell index 2

            for id in self.zone_ids() {
                let Some(widget) = self.zones.borrow().get(&id).cloned() else { continue };
                let cell_before_whitespace_index = widget.zone.after_model_position - 1;

                if cell_before_whitespace_index >= from_index && cell_before_whitespace_index < to_index {
                    // The cell this whitespace was after has been deleted
                    //  => move whitespace to before first deleted cell
                    self.set_after_model_position(id, from_index);
                    self.update_whitespace(id);
                } else if cell_before_whitespace_index >= to_index {
                    // adjust afterModelPosition for all other cells
                    let insert_length = new_cells.len() as i64;
                    let offset = insert_length - *deleted as i64;
                    self.set_after_model_position(id, widget.zone.after_model_position + offset);
                    self.update_whitespace(id);
                }
            }
        }
    }

    fn set_after_model_position(&self, id: u64, after_model_position: i64) {
        if let Some(widget) = self.zones.borrow_mut().get_mut(&id) {
            widget.zone.after_model_position = after_model_position;
        }
    }

    pub fn on_hidden_ranges_change(&self) {
        for id in self.zone_ids() {
            self.update_whitespace(id);
        }
    }

    fn update_whitespace(&self, id: u64) {
        let Some(widget) = self.zones.borrow().get(&id).cloned() else { return };
        let list = self.list();
        let view_position = list.convert_model_index_to_view_index(widget.zone.after_model_position);
        let is_in_hidden_area = self.is_in_hidden_ranges(&widget.zone);
        if let Some(stored) = self.zones.borrow_mut().get_mut(&id) {
            stored.is_in_hidden_area = is_in_hidden_area;
        }
        list.change_one_whitespace(&widget.whitespace_id, view_position.max(0) as usize, if is_in_hidden_area { 0.0 } else { widget.zone.height_in_px });
    }

    pub fn layout(&self) {
        for id in self.zone_ids() {
            self.layout_zone_by_key(id);
        }
    }

    fn add_zone(&self, zone: INotebookViewZone) -> String {
        let list = self.list();
        let view_position = list.convert_model_index_to_view_index(zone.after_model_position);
        let whitespace_id = list.insert_whitespace(view_position.max(0) as usize, zone.height_in_px);
        let is_in_hidden_area = self.is_in_hidden_ranges(&zone);
        let my_zone = IZoneWidget { whitespace_id: whitespace_id.clone(), zone, is_in_hidden_area };

        let key = Self::zone_key(&whitespace_id).expect("o id de whitespace é um inteiro em texto");
        self.zones.borrow_mut().insert(key, my_zone);
        whitespace_id
    }

    fn remove_zone(&self, id: &str) {
        self.list().remove_whitespace(id);
        if let Some(key) = Self::zone_key(id) {
            self.zones.borrow_mut().remove(&key);
        }
    }

    fn layout_zone(&self, id: &str) {
        if let Some(key) = Self::zone_key(id) {
            self.layout_zone_by_key(key);
        }
    }

    /// `_layoutZone(id)`. O posicionamento do nó (`setTop`, `setDisplay`, `setHeight`) é do DOM e não entra; o que sobra é a
    /// atualização do espaço no layout.
    fn layout_zone_by_key(&self, id: u64) {
        if !self.zones.borrow().contains_key(&id) {
            return;
        }

        self.update_whitespace(id);
    }

    fn is_in_hidden_ranges(&self, zone: &INotebookViewZone) -> bool {
        // The view zone is between two cells (zone.afterModelPosition - 1, zone.afterModelPosition)
        let after_index = zone.after_model_position;

        // In notebook, the first cell (markdown cell) in a folding range is always visible, so we need to check the cell after the notebook view zone
        !self.list().model_index_is_visible(after_index)
    }

    pub fn dispose(&self) {
        self.zones.borrow_mut().clear();
    }
}
