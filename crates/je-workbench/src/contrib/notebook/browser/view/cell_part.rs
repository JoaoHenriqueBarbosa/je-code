//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellPart.ts`: `CellContentPart`, `CellOverlayPart` e
//! `CellPartsCollection`, o contrato das partes que o renderizador monta dentro de cada célula.
//!
//! Divergências:
//! - O Rust não tem herança: o estado que as duas classes abstratas compartilham (`currentCell`, `cellDisposables` e a vida do
//!   `Disposable`) é o [`CellPartState`], que quem implementa expõe em [`CellPart::state`] e delega no `dispose`. Os métodos que as
//!   duas classes repetem (`prepareRenderCell`, `didRenderCell`, `unrenderCell`, `updateInternalLayoutNow`, `updateState`,
//!   `updateForExecutionState`) moram uma vez só no trait base [`CellPart`]; [`CellContentPart`] acrescenta `prepareLayout` e o
//!   `renderCell` que isola o erro de `didRenderCell`, e [`CellOverlayPart`] o `renderCell` que não isola.
//! - `ICellViewModel` é o [`CellViewModel`] do porte (a célula de código ou de markup), que é o que as partes concretas leem.
//! - `DOM.modify(targetWindow, callback)` (`scheduleAtNextAnimationFrame` com a prioridade `-10000`, "must be late") ainda não tem
//!   função própria no `dom.rs` do `je-base`: é [`CodeWindow::schedule_at_next_animation_frame`] com [`MODIFY_PRIORITY`].
//! - `safeInvokeNoArg` é `errors::catch` (o pânico é a exceção) mais `onUnexpectedError`.
//! - `CellPartsCollection` guarda as partes em `Rc<dyn ...>` (o original as guarda por referência de objeto), porque os fechos agendados
//!   para o quadro seguinte precisam delas.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::browser::dom::CodeWindow;
use je_base::errors;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};

use crate::contrib::notebook::browser::notebook_view_events::CellViewModelStateChangeEvent;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_execution_state_service::ICellExecutionStateChangedEvent;

/// A prioridade de `DOM.modify`: tarefas de escrita no DOM rodam por último no quadro.
pub const MODIFY_PRIORITY: i32 = -10000;

/// `safeInvokeNoArg`: o erro lançado vai para `onUnexpectedError` e a chamada devolve `None`.
fn safe_invoke_no_arg<T>(func: impl FnOnce() -> T) -> Option<T> {
    match errors::catch(func) {
        Ok(value) => Some(value),
        Err(error) => {
            errors::on_unexpected_error(&error);
            None
        }
    }
}

/// O estado que `CellContentPart` e `CellOverlayPart` declaram: `currentCell` e `cellDisposables`, descartados junto com a parte.
pub struct CellPartState {
    base: Rc<Disposable>,
    current_cell: RefCell<Option<CellViewModel>>,
    cell_disposables: Rc<DisposableStore>,
}

impl CellPartState {
    pub fn new() -> Self {
        let base = Disposable::new();
        let cell_disposables = DisposableStore::new();
        base.register(cell_disposables.clone());
        Self { base, current_cell: RefCell::new(None), cell_disposables }
    }

    /// `this._register(o)`.
    pub fn register(&self, o: DisposableRef) -> DisposableRef {
        self.base.register(o)
    }

    /// `protected currentCell`.
    pub fn current_cell(&self) -> Option<CellViewModel> {
        self.current_cell.borrow().clone()
    }

    /// `protected readonly cellDisposables`.
    pub fn cell_disposables(&self) -> &Rc<DisposableStore> {
        &self.cell_disposables
    }

    fn set_current_cell(&self, cell: Option<CellViewModel>) {
        *self.current_cell.borrow_mut() = cell;
    }
}

impl Default for CellPartState {
    fn default() -> Self {
        Self::new()
    }
}

impl IDisposable for CellPartState {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O que `CellContentPart` e `CellOverlayPart` têm em comum.
pub trait CellPart: IDisposable {
    fn state(&self) -> &CellPartState;

    /// Prepare model for cell part rendering. No DOM operations recommended within this operation.
    fn prepare_render_cell(&self, _element: &CellViewModel) {}

    fn did_render_cell(&self, _element: &CellViewModel) {}

    /// Dispose any disposables generated from `didRenderCell`.
    fn unrender_cell(&self, _element: &CellViewModel) {
        self.state().set_current_cell(None);
        self.state().cell_disposables().clear();
    }

    /// Update internal DOM (top positions) per cell layout info change. A cell part doesn't need to schedule the next frame, the list
    /// view will ensure that layout call is invoked in the right frame.
    fn update_internal_layout_now(&self, _element: &CellViewModel) {}

    /// Update per cell state change.
    fn update_state(&self, _element: &CellViewModel, _e: &CellViewModelStateChangeEvent) {}

    /// Update per execution state change.
    fn update_for_execution_state(&self, _element: &CellViewModel, _e: &Rc<dyn ICellExecutionStateChangedEvent>) {}
}

/// `CellContentPart`: a non-floating element that is rendered inside a cell. The rendering of the content part is synchronous to
/// avoid flickering.
pub trait CellContentPart: CellPart {
    /// Update the DOM for the cell `element`.
    fn render_cell(&self, element: &CellViewModel) {
        self.state().set_current_cell(Some(element.clone()));
        safe_invoke_no_arg(|| self.did_render_cell(element));
    }

    /// Perform DOM read operations to prepare for the list/cell layout update.
    fn prepare_layout(&self) {}
}

/// `CellOverlayPart`: renders on top of other components. The rendering of the overlay part might be postponed to the next animation
/// frame to avoid forced reflow.
pub trait CellOverlayPart: CellPart {
    /// Update the DOM for the cell `element`.
    fn render_cell(&self, element: &CellViewModel) {
        self.state().set_current_cell(Some(element.clone()));
        self.did_render_cell(element);
    }
}

/// `CellPartsCollection`.
pub struct CellPartsCollection {
    base: Rc<Disposable>,
    target_window: Rc<CodeWindow>,
    content_parts: Vec<Rc<dyn CellContentPart>>,
    overlay_parts: Vec<Rc<dyn CellOverlayPart>>,
    scheduled_overlay_rendering: Rc<MutableDisposable<dyn IDisposable>>,
    scheduled_overlay_update_state: Rc<MutableDisposable<dyn IDisposable>>,
    scheduled_overlay_update_execution_state: Rc<MutableDisposable<dyn IDisposable>>,
}

impl CellPartsCollection {
    pub fn new(target_window: Rc<CodeWindow>, content_parts: Vec<Rc<dyn CellContentPart>>, overlay_parts: Vec<Rc<dyn CellOverlayPart>>) -> Rc<Self> {
        let base = Disposable::new();
        let scheduled_overlay_rendering = MutableDisposable::<dyn IDisposable>::new();
        let scheduled_overlay_update_state = MutableDisposable::<dyn IDisposable>::new();
        let scheduled_overlay_update_execution_state = MutableDisposable::<dyn IDisposable>::new();
        base.register(scheduled_overlay_rendering.clone());
        base.register(scheduled_overlay_update_state.clone());
        base.register(scheduled_overlay_update_execution_state.clone());
        Rc::new(Self {
            base,
            target_window,
            content_parts,
            overlay_parts,
            scheduled_overlay_rendering,
            scheduled_overlay_update_state,
            scheduled_overlay_update_execution_state,
        })
    }

    pub fn concat_content_part(&self, other: &[Rc<dyn CellContentPart>], target_window: Rc<CodeWindow>) -> Rc<CellPartsCollection> {
        let content_parts = self.content_parts.iter().chain(other).cloned().collect();
        CellPartsCollection::new(target_window, content_parts, self.overlay_parts.clone())
    }

    pub fn concat_overlay_part(&self, other: &[Rc<dyn CellOverlayPart>], target_window: Rc<CodeWindow>) -> Rc<CellPartsCollection> {
        let overlay_parts = self.overlay_parts.iter().chain(other).cloned().collect();
        CellPartsCollection::new(target_window, self.content_parts.clone(), overlay_parts)
    }

    /// `DOM.modify(this.targetWindow, ...)` sobre as partes de sobreposição, guardando o agendamento em `slot`.
    fn modify_overlay_parts(&self, slot: &MutableDisposable<dyn IDisposable>, element: &CellViewModel, run: impl Fn(&dyn CellOverlayPart, &CellViewModel) + 'static) {
        let parts = self.overlay_parts.clone();
        let element = element.clone();
        let scheduled = self.target_window.schedule_at_next_animation_frame(
            move || {
                for part in &parts {
                    safe_invoke_no_arg(|| run(part.as_ref(), &element));
                }
            },
            MODIFY_PRIORITY,
        );
        slot.set(Some(scheduled));
    }

    pub fn schedule_render_cell(&self, element: &CellViewModel) {
        // prepare model
        for part in &self.content_parts {
            safe_invoke_no_arg(|| part.prepare_render_cell(element));
        }

        for part in &self.overlay_parts {
            safe_invoke_no_arg(|| part.prepare_render_cell(element));
        }

        // render content parts
        for part in &self.content_parts {
            safe_invoke_no_arg(|| part.render_cell(element));
        }

        self.modify_overlay_parts(&self.scheduled_overlay_rendering, element, |part, element| part.render_cell(element));
    }

    pub fn unrender_cell(&self, element: &CellViewModel) {
        for part in &self.content_parts {
            safe_invoke_no_arg(|| part.unrender_cell(element));
        }

        self.scheduled_overlay_rendering.set(None);
        self.scheduled_overlay_update_state.set(None);
        self.scheduled_overlay_update_execution_state.set(None);

        for part in &self.overlay_parts {
            safe_invoke_no_arg(|| part.unrender_cell(element));
        }
    }

    pub fn update_internal_layout_now(&self, view_cell: &CellViewModel) {
        for part in &self.content_parts {
            safe_invoke_no_arg(|| part.update_internal_layout_now(view_cell));
        }

        for part in &self.overlay_parts {
            safe_invoke_no_arg(|| part.update_internal_layout_now(view_cell));
        }
    }

    pub fn prepare_layout(&self) {
        for part in &self.content_parts {
            safe_invoke_no_arg(|| part.prepare_layout());
        }
    }

    pub fn update_state(&self, view_cell: &CellViewModel, e: &CellViewModelStateChangeEvent) {
        for part in &self.content_parts {
            safe_invoke_no_arg(|| part.update_state(view_cell, e));
        }

        let e = *e;
        self.modify_overlay_parts(&self.scheduled_overlay_update_state, view_cell, move |part, view_cell| part.update_state(view_cell, &e));
    }

    pub fn update_for_execution_state(&self, view_cell: &CellViewModel, e: &Rc<dyn ICellExecutionStateChangedEvent>) {
        for part in &self.content_parts {
            safe_invoke_no_arg(|| part.update_for_execution_state(view_cell, e));
        }

        let e = e.clone();
        self.modify_overlay_parts(&self.scheduled_overlay_update_execution_state, view_cell, move |part, view_cell| part.update_for_execution_state(view_cell, &e));
    }
}

impl IDisposable for CellPartsCollection {
    fn dispose(&self) {
        self.base.dispose();
    }
}
