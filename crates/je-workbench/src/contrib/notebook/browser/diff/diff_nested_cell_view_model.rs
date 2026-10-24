//! Porte de `src/vs/workbench/contrib/notebook/browser/diff/diffNestedCellViewModel.ts`: o view model de uma das células de
//! um elemento de diff (`DiffNestedCellViewModel`), com os view models das saídas e a altura delas.
//!
//! Divergências:
//! - `Disposable` é o campo `base`; o view model nasce como `Rc<Self>` e os fechos dos eventos guardam um `Weak` dele.
//! - O construtor não recebe o `INotebookService` do original: ele só o repassa ao `CellOutputViewModel`, que no porte não o
//!   usa (ver a divergência dele).
//! - `IGenericCellViewModel` é o trait [`IGenericCellViewModel`] (`id`, `handle`, `uri`, `metadata`, os três
//!   `output*IsFocused/Hovered` com `get`/`set` como o par `x()` / `set_x(...)`, `outputsViewModels`, `getOutputOffset` e
//!   `updateOutputHeight`); o `source` opcional de `updateOutputHeight` não é lido por este view model.
//! - `Uint32Array` do `PrefixSumComputer` é `Vec<u32>`: as alturas são truncadas para inteiro sem sinal.
//! - `_onDidChangeState` só dispara `outputIsHoveredChanged` e `outputIsFocusedChanged`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::prefix_sum_computer::PrefixSumComputer;

use super::notebook_diff_editor_browser::IDiffNestedCellViewModel;
use crate::contrib::notebook::browser::notebook_browser::IGenericCellViewModel;
use crate::contrib::notebook::browser::notebook_view_events::CellViewModelStateChangeEvent;
use crate::contrib::notebook::browser::view_model::cell_output_view_model::CellOutputViewModel;
use crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::notebook_common::{ICell, ICellOutput, NotebookCellMetadata};

/// `DiffNestedCellViewModel`.
pub struct DiffNestedCellViewModel {
    base: Rc<Disposable>,
    id: String,
    pub text_model: Rc<NotebookCellTextModel>,
    on_did_change_state: Rc<Emitter<CellViewModelStateChangeEvent>>,
    hovering_output: Cell<bool>,
    focus_on_output: Cell<bool>,
    focus_input_in_output: Cell<bool>,
    output_view_models: RefCell<Vec<Rc<CellOutputViewModel>>>,
    output_collection: RefCell<Vec<u32>>,
    outputs_top: RefCell<Option<PrefixSumComputer>>,
    on_did_change_output_layout: Rc<Emitter<()>>,
}

impl DiffNestedCellViewModel {
    pub fn new(text_model: Rc<NotebookCellTextModel>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_state = Emitter::<CellViewModelStateChangeEvent>::new();
        let on_did_change_output_layout = Emitter::<()>::new();
        base.register(on_did_change_state.clone());
        base.register(on_did_change_output_layout.clone());

        let id = generate_uuid();
        let output_view_models: Vec<Rc<CellOutputViewModel>> = text_model.outputs().into_iter().map(|output| CellOutputViewModel::new(&id, output)).collect();
        let output_count = output_view_models.len();
        let this = Rc::new(Self {
            base,
            id,
            text_model: text_model.clone(),
            on_did_change_state,
            hovering_output: Cell::new(false),
            focus_on_output: Cell::new(false),
            focus_input_in_output: Cell::new(false),
            output_view_models: RefCell::new(output_view_models),
            output_collection: RefCell::new(vec![0; output_count]),
            outputs_top: RefCell::new(None),
            on_did_change_output_layout,
        });

        let weak = Rc::downgrade(&this);
        this.base.register(text_model.on_did_change_outputs().listen(move |splice| {
            let Some(this) = weak.upgrade() else { return };
            {
                let mut collection = this.output_collection.borrow_mut();
                let start = splice.start.min(collection.len());
                let end = (start + splice.delete_count).min(collection.len());
                collection.splice(start..end, splice.new_outputs.iter().map(|_| 0));
            }
            let new_view_models: Vec<Rc<CellOutputViewModel>> = splice.new_outputs.iter().map(|output| CellOutputViewModel::new(&this.id, output.clone())).collect();
            let removed: Vec<Rc<CellOutputViewModel>> = {
                let mut view_models = this.output_view_models.borrow_mut();
                let start = splice.start.min(view_models.len());
                let end = (start + splice.delete_count).min(view_models.len());
                let removed: Vec<Rc<CellOutputViewModel>> = view_models.splice(start..end, new_view_models).collect();
                removed
            };
            for view_model in &removed {
                view_model.dispose();
            }

            *this.outputs_top.borrow_mut() = None;
            this.on_did_change_output_layout.fire(());
        }));

        this
    }

    pub fn outputs(&self) -> Vec<Rc<dyn ICellOutput>> {
        self.text_model.outputs()
    }

    pub fn language(&self) -> String {
        self.text_model.language()
    }

    pub fn on_did_change_state(&self) -> Event<CellViewModelStateChangeEvent> {
        self.on_did_change_state.event()
    }

    pub fn on_did_change_output_layout(&self) -> Event<()> {
        self.on_did_change_output_layout.event()
    }

    fn ensure_outputs_top(&self) {
        if self.outputs_top.borrow().is_none() {
            let values = self.output_collection.borrow().clone();
            *self.outputs_top.borrow_mut() = Some(PrefixSumComputer::new(values));
        }
    }

    pub fn get_output_total_height(&self) -> f64 {
        self.ensure_outputs_top();

        self.outputs_top.borrow().as_ref().map_or(0.0, |outputs_top| f64::from(outputs_top.get_total_sum()))
    }
}

impl IDiffNestedCellViewModel for DiffNestedCellViewModel {}

impl IGenericCellViewModel for DiffNestedCellViewModel {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn handle(&self) -> i64 {
        self.text_model.handle()
    }

    fn uri(&self) -> URI {
        self.text_model.uri()
    }

    fn metadata(&self) -> NotebookCellMetadata {
        self.text_model.metadata()
    }

    fn output_is_hovered(&self) -> bool {
        self.hovering_output.get()
    }

    fn set_output_is_hovered(&self, value: bool) {
        self.hovering_output.set(value);
        self.on_did_change_state.fire(CellViewModelStateChangeEvent { output_is_hovered_changed: true, ..Default::default() });
    }

    fn output_is_focused(&self) -> bool {
        self.focus_on_output.get()
    }

    fn set_output_is_focused(&self, value: bool) {
        self.focus_on_output.set(value);
        self.on_did_change_state.fire(CellViewModelStateChangeEvent { output_is_focused_changed: true, ..Default::default() });
    }

    fn input_in_output_is_focused(&self) -> bool {
        self.focus_input_in_output.get()
    }

    fn set_input_in_output_is_focused(&self, value: bool) {
        self.focus_input_in_output.set(value);
    }

    fn outputs_view_models(&self) -> Vec<Rc<CellOutputViewModel>> {
        self.output_view_models.borrow().clone()
    }

    fn get_output_offset(&self, index: usize) -> f64 {
        self.ensure_outputs_top();

        if index >= self.output_collection.borrow().len() {
            errors::throw(Error::new("Output index out of range!"));
        }

        f64::from(self.outputs_top.borrow().as_ref().expect("outputsTop").get_prefix_sum(index as i64 - 1))
    }

    fn update_output_height(&self, index: usize, height: f64, _source: Option<&str>) {
        if index >= self.output_collection.borrow().len() {
            errors::throw(Error::new("Output index out of range!"));
        }

        self.ensure_outputs_top();
        self.output_collection.borrow_mut()[index] = height as u32;
        let changed = self.outputs_top.borrow_mut().as_mut().expect("outputsTop").set_value(index, height as u32);
        if changed {
            self.on_did_change_output_layout.fire(());
        }
    }
}

impl IDisposable for DiffNestedCellViewModel {
    fn dispose(&self) {
        self.base.dispose();

        for output in self.output_view_models.borrow().iter() {
            output.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "DiffNestedCellViewModel"
    }
}
