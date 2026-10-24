//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/cellOutputViewModel.ts`: o view model de uma saída de célula
//! de código (`CellOutputViewModel`).
//!
//! Divergências:
//! - `INotebookService` (o terceiro argumento do construtor original) não existe na camada lógica das células, então o construtor
//!   não o recebe: [`CellOutputViewModel::resolve_mime_types`] recebe o serviço como primeiro argumento (quem o chama, como o
//!   `CellOutputElement` e o `getAllOutputsText`, já o tem). O corpo é o do original: `getOutputMimeTypeInfo` e o primeiro tipo
//!   confiável com renderizador disponível (ou o índice 0).
//! - `visible = observableValue('outputVisible', false)` é o observável [`CellOutputViewModel::visible_observable`], e
//!   [`CellOutputViewModel::visible`] é o `.get()`.
//! - `outputHandle = handle++` usa um contador do módulo (`thread_local`, o `let handle = 0` do módulo original).
//! - `cellViewModel: IGenericCellViewModel` é o `id` da célula dona (o único membro que o resto do porte lê).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{ISettableObservable, Observable, SettableExt, observable_value};

use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{ICellOutput, IOrderedMimeType, IOutputItemDto, RENDERER_NOT_AVAILABLE};
use crate::contrib::notebook::common::notebook_service::INotebookService;

thread_local! {
    static HANDLE: Cell<i64> = const { Cell::new(0) };
}

/// `CellOutputViewModel`.
pub struct CellOutputViewModel {
    base: Rc<Disposable>,
    on_did_reset_renderer: Rc<Emitter<()>>,
    always_show: Cell<bool>,
    visible: ISettableObservable<bool>,
    pub output_handle: i64,
    pub cell_view_model_id: String,
    output_raw_data: Rc<dyn ICellOutput>,
    picked_mime_type: RefCell<Option<IOrderedMimeType>>,
}

impl CellOutputViewModel {
    pub fn new(cell_view_model_id: &str, output_raw_data: Rc<dyn ICellOutput>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_reset_renderer = Emitter::<()>::new();
        base.register(on_did_reset_renderer.clone());
        let output_handle = HANDLE.with(|handle| {
            let current = handle.get();
            handle.set(current + 1);
            current
        });
        Rc::new(Self {
            base,
            on_did_reset_renderer,
            always_show: Cell::new(false),
            visible: observable_value::<bool, ()>("outputVisible", false),
            output_handle,
            cell_view_model_id: cell_view_model_id.to_string(),
            output_raw_data,
            picked_mime_type: RefCell::new(None),
        })
    }

    pub fn on_did_reset_renderer(&self) -> Event<()> {
        self.on_did_reset_renderer.event()
    }

    /// `visible.get()`.
    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    /// `visible`: o observável, que quem reage (`autorun`) lê com `read(reader)`.
    pub fn visible_observable(&self) -> ISettableObservable<bool> {
        self.visible.clone()
    }

    /// `setVisible(visible = true, force = false)`.
    pub fn set_visible(&self, visible: bool, force: bool) {
        if !visible && self.always_show.get() {
            // we are forced to show, so no-op
            return;
        }

        if force && visible {
            self.always_show.set(true);
        }

        self.visible.set(visible, None);
    }

    pub fn model(&self) -> Rc<dyn ICellOutput> {
        self.output_raw_data.clone()
    }

    pub fn picked_mime_type(&self) -> Option<IOrderedMimeType> {
        self.picked_mime_type.borrow().clone()
    }

    pub fn set_picked_mime_type(&self, value: Option<IOrderedMimeType>) {
        *self.picked_mime_type.borrow_mut() = value;
    }

    pub fn has_multi_mime_type(&self) -> bool {
        let outputs = self.output_raw_data.outputs();
        if outputs.len() < 2 {
            return false;
        }

        let first_mime_type = outputs[0].mime.clone();
        outputs.iter().any(|output| output.mime != first_mime_type)
    }

    /// `resolveMimeTypes(textModel, kernelProvides)`: os tipos de mídia na ordem de preferência e o índice do escolhido.
    pub fn resolve_mime_types(
        &self,
        notebook_service: &dyn INotebookService,
        text_model: &NotebookTextModel,
        kernel_provides: Option<&[String]>,
    ) -> (Vec<IOrderedMimeType>, usize) {
        let mime_types = notebook_service.get_output_mime_type_info(text_model, kernel_provides, self.output_raw_data.as_ref());
        let index = mime_types.iter().position(|mime_type| mime_type.renderer_id != RENDERER_NOT_AVAILABLE && mime_type.is_trusted);

        (mime_types, index.unwrap_or(0))
    }

    pub fn reset_renderer(&self) {
        // reset the output renderer
        *self.picked_mime_type.borrow_mut() = None;
        self.model().bump_version();
        self.on_did_reset_renderer.fire(());
    }

    /// `toRawJSON()`: `{ outputs }`.
    pub fn to_raw_json(&self) -> Vec<IOutputItemDto> {
        self.output_raw_data.outputs()
    }
}

impl IDisposable for CellOutputViewModel {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CellOutputViewModel"
    }
}
