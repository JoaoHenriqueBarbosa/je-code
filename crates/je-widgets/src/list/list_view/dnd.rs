//! A arrastar e soltar da [`ListView`] (a região `// DND` de `listView.ts`): o arrasto de uma linha
//! (`dragstart`, `onDragStart`), o destino (`dragover`, `dragleave`, `drop`, `dragend`), o feedback de soltar nas
//! linhas e na lista, e a rolagem automática perto das bordas.
//!
//! Do TypeScript para o Rust:
//! - O `IListViewDragAndDrop` vem de [`ListView::set_dnd`] (antes de existir qualquer item), no lugar da opção
//!   `dnd` do construtor: a `ListViewOptions` do `je-widgets` é `Clone` e não tem o tipo do elemento. Sem `dnd`,
//!   o padrão é o `DefaultOptions.dnd` (nada é arrastável e nada aceita soltar).
//! - Os eventos do DOM (`dragstart` na linha, `dragover`, `drop`, `dragleave` e `dragend` no `domNode`) entram pelo
//!   [`listen_drag_event`] (ver o `//!` de [`crate::dnd`]); os `handle_*` abaixo são os ouvintes e recebem o
//!   [`DragEvent`] já montado. O `browserEvent.preventDefault()` é o retorno `true`.
//! - `item.row.domNode.draggable = !!uri` é o atributo `draggable` do nó da linha.
//! - O `offsetY` do `getTargetSector` é o [`DragEvent::offset_y`]; o `pageY` da rolagem é `pointer.y`; o
//!   `getTopLeftOffset(domNode).top` é o topo da caixa do `domNode` em coordenadas da janela.
//! - `animate(window, fn)` roda `fn` a cada quadro de animação até ser descartado; `disposableTimeout` é o
//!   `TimeoutTimer`. O feedback de soltar muda classes das linhas, e quem pinta as linhas (a `List`) é avisada por
//!   [`ListView::on_did_change_drag_feedback`].
//! - A imagem de arrasto é destruída quando o arrasto termina (`dragend` na origem ou `drop` no destino), no lugar
//!   do `setTimeout(0)` do original (ver [`crate::drag_image`]).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::TimeoutTimer;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_ui::Ui;

use super::ListView;
use crate::common::{registered_emitter, throw};
use crate::dnd::{DataTransfers, DragEvent, DragEventKind, DragHandler, IDragAndDropData, listen_drag_event};
use crate::drag_image::{DragImageStyle, apply_drag_image, discard_drag_image};
use crate::list::list_dnd::{
    ElementsDragAndDropData, ExternalElementsDragAndDropData, IListDragAndDrop, IListViewDragAndDrop, ListDragOverEffectPosition, ListDragOverEffectType, ListDragOverResult, ListViewTargetSector,
    NativeDragAndDropData, current_drag_and_drop_data, is_vscode_ui_data, set_current_drag_and_drop_data,
};

/// O `DefaultOptions.dnd`: nada é arrastável e nada aceita soltar.
struct NoDragAndDrop;

impl<T: Clone> IListDragAndDrop<T> for NoDragAndDrop {
    fn get_drag_uri(&self, _element: &T) -> Option<String> {
        None
    }

    fn on_drag_over(&self, _data: &Rc<dyn IDragAndDropData>, _target_element: Option<&T>, _target_index: Option<usize>, _target_sector: Option<ListViewTargetSector>, _original_event: &DragEvent) -> ListDragOverResult {
        ListDragOverResult::Bool(false)
    }

    fn drop(&self, _data: &Rc<dyn IDragAndDropData>, _target_element: Option<&T>, _target_index: Option<usize>, _target_sector: Option<ListViewTargetSector>, _original_event: &DragEvent) {}
}

impl<T: Clone> IListViewDragAndDrop<T> for NoDragAndDrop {
    fn get_drag_elements(&self, element: &T, _index: usize) -> Vec<T> {
        vec![element.clone()]
    }
}

/// O estado do arrasto da `ListView`.
pub(super) struct ListViewDragState<T: 'static> {
    dnd: RefCell<Rc<dyn IListViewDragAndDrop<T>>>,
    can_drop: Cell<bool>,
    current_drag_data: RefCell<Option<Rc<dyn IDragAndDropData>>>,
    current_drag_feedback: RefCell<Option<Vec<i64>>>,
    current_drag_feedback_position: Cell<Option<ListDragOverEffectPosition>>,
    current_drag_feedback_disposable: RefCell<Option<DisposableRef>>,
    on_drag_leave_timeout: Rc<TimeoutTimer>,
    drag_over_animation: RefCell<Option<DisposableRef>>,
    drag_over_animation_stop: Rc<TimeoutTimer>,
    drag_over_mouse_y: Cell<f64>,
    image_style: Cell<DragImageStyle>,
    on_did_change_feedback: Rc<Emitter<()>>,
}

impl<T: Clone + 'static> ListViewDragState<T> {
    pub(super) fn new(base: &Rc<Disposable>) -> Self {
        let on_drag_leave_timeout = TimeoutTimer::new();
        base.register(on_drag_leave_timeout.clone());
        let drag_over_animation_stop = TimeoutTimer::new();
        base.register(drag_over_animation_stop.clone());

        Self {
            dnd: RefCell::new(Rc::new(NoDragAndDrop)),
            can_drop: Cell::new(false),
            current_drag_data: RefCell::new(None),
            current_drag_feedback: RefCell::new(None),
            current_drag_feedback_position: Cell::new(None),
            current_drag_feedback_disposable: RefCell::new(None),
            on_drag_leave_timeout,
            drag_over_animation: RefCell::new(None),
            drag_over_animation_stop,
            drag_over_mouse_y: Cell::new(0.0),
            image_style: Cell::new(DragImageStyle::default()),
            on_did_change_feedback: registered_emitter(base),
        }
    }
}

/// `animate(window, fn)`: roda `step` a cada quadro de animação até o resultado ser descartado.
fn animate(ui: &Ui, step: Rc<dyn Fn()>) -> DisposableRef {
    let stopped = Rc::new(Cell::new(false));
    let pending: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
    schedule_step(ui.clone(), step, stopped.clone(), pending.clone());

    to_disposable(move || {
        stopped.set(true);
        let scheduled = pending.borrow_mut().take();
        if let Some(scheduled) = scheduled {
            scheduled.dispose();
        }
    })
}

fn schedule_step(ui: Ui, step: Rc<dyn Fn()>, stopped: Rc<Cell<bool>>, pending: Rc<RefCell<Option<DisposableRef>>>) {
    let next = (ui.clone(), step, stopped, pending.clone());
    let scheduled = ui.schedule_at_next_animation_frame(0, move || {
        let (ui, step, stopped, pending) = next;
        if stopped.get() {
            return;
        }
        step();
        if !stopped.get() {
            schedule_step(ui, step, stopped, pending);
        }
    });
    *pending.borrow_mut() = Some(scheduled);
}

impl<T: Clone + 'static> ListView<T> {
    /// O `dnd` das opções: troca o `IListViewDragAndDrop` (chame antes de a lista ter itens; as linhas já
    /// renderizadas só passam a ser arrastáveis quando forem renderizadas de novo).
    pub fn set_dnd(&self, dnd: Rc<dyn IListViewDragAndDrop<T>>) {
        *self.drag.dnd.borrow_mut() = dnd;
    }

    /// As cores da imagem de arrasto (as de `.monaco-drag-image`).
    pub fn set_drag_image_style(&self, style: DragImageStyle) {
        self.drag.image_style.set(style);
    }

    /// Avisa quando o feedback de soltar (as classes `drop-target*` das linhas e da lista) muda.
    pub fn on_did_change_drag_feedback(&self) -> BaseEvent<()> {
        self.drag.on_did_change_feedback.event()
    }

    /// Instala os ouvintes de `dragover`, `drop`, `dragleave` e `dragend` do `domNode`.
    pub(super) fn install_drag_listeners(&self) {
        let handlers: [(DragEventKind, fn(&ListView<T>, &DragEvent) -> bool); 4] = [
            (DragEventKind::DragOver, ListView::<T>::handle_drag_over),
            (DragEventKind::Drop, ListView::<T>::handle_drop),
            (DragEventKind::DragLeave, ListView::<T>::handle_drag_leave),
            (DragEventKind::DragEnd, ListView::<T>::handle_drag_end),
        ];

        for (kind, handle) in handlers {
            let weak = self.weak_self.clone();
            let handler: DragHandler = Rc::new(move |event: &DragEvent| weak.upgrade().is_some_and(|this| handle(&this, event)));
            if let Some(listener) = listen_drag_event(&self.ui, self.dom_node, kind, handler) {
                self.base.register(listener);
            }
        }
    }

    /// O trecho de `insertItemInDOM` depois do `renderElement`: a linha é arrastável quando o `dnd` dá um URI para
    /// o elemento, e o `dragstart` dela abre o arrasto.
    pub(super) fn update_drag_source(&self, index: usize) {
        let (dom_node, element) = {
            let items = self.items.borrow();
            let Some(item) = items.get(index) else { return };
            // Rendering can synchronously recycle the row, for example when the viewport collapses.
            let Some(row) = item.row.as_ref() else { return };
            (row.dom_node, item.element.clone())
        };

        let dnd = self.drag.dnd.borrow().clone();
        let uri = dnd.get_drag_uri(&element).filter(|uri| !uri.is_empty());

        let previous = self.items.borrow_mut()[index].drag_start_disposable.take();
        if let Some(previous) = previous {
            previous.dispose();
        }
        self.ui.tree_mut().set_attribute(dom_node, "draggable", if uri.is_some() { "true" } else { "false" });

        if let Some(uri) = uri {
            let weak = self.weak_self.clone();
            let handler: DragHandler = Rc::new(move |event: &DragEvent| {
                if let Some(this) = weak.upgrade() {
                    this.on_drag_start(&element, &uri, event);
                }
                false
            });
            let listener = listen_drag_event(&self.ui, dom_node, DragEventKind::DragStart, handler);
            self.items.borrow_mut()[index].drag_start_disposable = listener;
        }
    }

    /// O `dispose` do que o arrasto deixou ativo: a rolagem automática e o feedback.
    pub(super) fn dispose_drag(&self) {
        self.teardown_drag_and_drop_scroll_top_animation();
        self.clear_drag_over_feedback();
    }

    // DND

    /// `onDragStart(element, uri, event)`.
    fn on_drag_start(&self, element: &T, uri: &str, event: &DragEvent) {
        let Some(index) = self.get_item_index_from_event_target(event.target) else { return };
        let dnd = self.drag.dnd.borrow().clone();
        let elements = dnd.get_drag_elements(element, index);

        event.data_transfer.set_effect_allowed("copyMove");
        event.data_transfer.set_data(DataTransfers::TEXT, uri);

        let label = dnd.get_drag_label(&elements, event).unwrap_or_else(|| elements.len().to_string());

        // add domId to get list specific styling
        apply_drag_image(&self.ui, &event.data_transfer, self.dom_node, &label, std::slice::from_ref(&self.dom_id), &self.drag.image_style.get());

        self.ui.tree_mut().add_class(self.dom_node, "dragging");
        let data: Rc<dyn IDragAndDropData> = Rc::new(ElementsDragAndDropData::new(elements.clone()));
        *self.drag.current_drag_data.borrow_mut() = Some(data.clone());
        set_current_drag_and_drop_data(Some(Rc::new(ExternalElementsDragAndDropData::new(elements))));

        dnd.on_drag_start(&data, event);
    }

    /// O elemento, o índice e o setor da linha sob o evento (`toDragEvent`).
    fn drag_target(&self, event: &DragEvent) -> (Option<T>, Option<usize>, Option<ListViewTargetSector>) {
        let index = self.get_item_index_from_event_target(event.target);
        let items = self.items.borrow();
        let item = index.and_then(|index| items.get(index));
        (item.map(|item| item.element.clone()), index, item.map(|item| ListViewTargetSector::at(event.offset_y, item.size)))
    }

    /// O ouvinte de `dragover` do `domNode`: sempre consome o evento (`preventDefault()`).
    pub fn handle_drag_over(&self, event: &DragEvent) -> bool {
        self.on_drag_over(event);
        true
    }

    /// `onDragOver(event)`: devolve se a linha aceita soltar.
    fn on_drag_over(&self, event: &DragEvent) -> bool {
        self.drag.on_drag_leave_timeout.cancel();

        if current_drag_and_drop_data().is_some_and(|data| is_vscode_ui_data(&*data)) {
            return false;
        }

        self.setup_drag_and_drop_scroll_top_animation(event);

        // Drag over from outside
        if self.drag.current_drag_data.borrow().is_none() {
            // Drag over from another list, or from the desktop
            let data: Rc<dyn IDragAndDropData> = current_drag_and_drop_data().unwrap_or_else(|| Rc::new(NativeDragAndDropData::new()));
            *self.drag.current_drag_data.borrow_mut() = Some(data);
        }

        let Some(data) = self.drag.current_drag_data.borrow().clone() else { return false };
        let dnd = self.drag.dnd.borrow().clone();
        let (element, index, sector) = self.drag_target(event);
        let result = dnd.on_drag_over(&data, element.as_ref(), index, sector, event);
        let accept = result.accept();
        self.drag.can_drop.set(accept);

        if !accept {
            *self.drag.current_drag_feedback.borrow_mut() = None;
            self.dispose_drag_feedback();
            return false;
        }

        let effect = result.effect();
        event.data_transfer.set_drop_effect(if effect.is_some_and(|effect| effect.kind == ListDragOverEffectType::Copy) { "copy" } else { "move" });

        let mut feedback: Vec<i64> = match result.feedback() {
            Some(feedback) => feedback.clone(),
            None => vec![index.map_or(-1, |index| index as i64)],
        };

        // sanitize feedback list
        let length = self.items.borrow().len() as i64;
        let mut distinct = Vec::with_capacity(feedback.len());
        for candidate in feedback.drain(..) {
            if !distinct.contains(&candidate) && candidate >= -1 && candidate < length {
                distinct.push(candidate);
            }
        }
        distinct.sort_unstable();
        let mut feedback = if distinct.first() == Some(&-1) { vec![-1] } else { distinct };

        let mut drag_over_effect_position = effect.and_then(|effect| effect.position).unwrap_or(ListDragOverEffectPosition::Over);

        if self.drag.current_drag_feedback.borrow().as_ref() == Some(&feedback) && self.drag.current_drag_feedback_position.get() == Some(drag_over_effect_position) {
            return true;
        }

        // O original guarda o mesmo array que adiante muda no lugar (`feedback[0] += 1`).
        let stored_position = drag_over_effect_position;
        self.dispose_drag_feedback();

        if feedback.first() == Some(&-1) {
            // entire list feedback
            {
                let mut tree = self.ui.tree_mut();
                tree.add_class(self.dom_node, drag_over_effect_position.class_name());
                tree.add_class(self.rows_container, drag_over_effect_position.class_name());
            }
            let weak = self.weak_self.clone();
            *self.drag.current_drag_feedback_disposable.borrow_mut() = Some(to_disposable(move || {
                if let Some(this) = weak.upgrade() {
                    let mut tree = this.ui.tree_mut();
                    tree.remove_class(this.dom_node, drag_over_effect_position.class_name());
                    tree.remove_class(this.rows_container, drag_over_effect_position.class_name());
                    drop(tree);
                    this.drag.on_did_change_feedback.fire(());
                }
            }));
        } else {
            if feedback.len() > 1 && drag_over_effect_position != ListDragOverEffectPosition::Over {
                throw("Can't use multiple feedbacks with position different than 'over'");
            }

            // Make sure there is no flicker when moving between two items
            // Always use the before feedback if possible
            if drag_over_effect_position == ListDragOverEffectPosition::After && feedback.first().is_some_and(|first| *first < length - 1) {
                feedback[0] += 1;
                drag_over_effect_position = ListDragOverEffectPosition::Before;
            }

            for &feedback_index in &feedback {
                let row = {
                    let mut items = self.items.borrow_mut();
                    let item = &mut items[feedback_index as usize];
                    item.drop_target = true;
                    item.row.as_ref().map(|row| row.dom_node)
                };
                if let Some(row) = row {
                    self.ui.tree_mut().add_class(row, drag_over_effect_position.class_name());
                }
            }

            let weak = self.weak_self.clone();
            let indexes = feedback.clone();
            *self.drag.current_drag_feedback_disposable.borrow_mut() = Some(to_disposable(move || {
                if let Some(this) = weak.upgrade() {
                    for &feedback_index in &indexes {
                        let row = {
                            let mut items = this.items.borrow_mut();
                            let Some(item) = items.get_mut(feedback_index as usize) else { continue };
                            item.drop_target = false;
                            item.row.as_ref().map(|row| row.dom_node)
                        };
                        if let Some(row) = row {
                            this.ui.tree_mut().remove_class(row, drag_over_effect_position.class_name());
                        }
                    }
                    this.drag.on_did_change_feedback.fire(());
                }
            }));
        }

        *self.drag.current_drag_feedback.borrow_mut() = Some(feedback);
        self.drag.current_drag_feedback_position.set(Some(stored_position));
        self.drag.on_did_change_feedback.fire(());
        true
    }

    /// O ouvinte de `dragleave` do `domNode`.
    pub fn handle_drag_leave(&self, event: &DragEvent) -> bool {
        let weak = self.weak_self.clone();
        self.drag.on_drag_leave_timeout.cancel_and_set(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.clear_drag_over_feedback();
                }
            },
            100,
        );

        let data = self.drag.current_drag_data.borrow().clone();
        if let Some(data) = data {
            let (element, index, _) = self.drag_target(event);
            let dnd = self.drag.dnd.borrow().clone();
            dnd.on_drag_leave(&data, element.as_ref(), index, event);
        }
        false
    }

    /// O ouvinte de `drop` do `domNode`: consome o evento (`preventDefault()`) quando há o que soltar.
    pub fn handle_drop(&self, event: &DragEvent) -> bool {
        if !self.drag.can_drop.get() {
            return false;
        }

        let drag_data = self.drag.current_drag_data.borrow_mut().take();
        self.teardown_drag_and_drop_scroll_top_animation();
        self.clear_drag_over_feedback();
        self.ui.tree_mut().remove_class(self.dom_node, "dragging");
        set_current_drag_and_drop_data(None);
        discard_drag_image(&self.ui, &event.data_transfer);

        let Some(drag_data) = drag_data else { return false };

        drag_data.update(&event.data_transfer);
        let (element, index, sector) = self.drag_target(event);
        let dnd = self.drag.dnd.borrow().clone();
        dnd.drop(&drag_data, element.as_ref(), index, sector, event);
        true
    }

    /// O ouvinte de `dragend` do `domNode`.
    pub fn handle_drag_end(&self, event: &DragEvent) -> bool {
        self.drag.can_drop.set(false);
        self.teardown_drag_and_drop_scroll_top_animation();
        self.clear_drag_over_feedback();
        self.ui.tree_mut().remove_class(self.dom_node, "dragging");
        *self.drag.current_drag_data.borrow_mut() = None;
        set_current_drag_and_drop_data(None);
        discard_drag_image(&self.ui, &event.data_transfer);

        let dnd = self.drag.dnd.borrow().clone();
        dnd.on_drag_end(event);
        false
    }

    /// Descarta o feedback que está aplicado (o `currentDragFeedbackDisposable.dispose()`).
    fn dispose_drag_feedback(&self) {
        let disposable = self.drag.current_drag_feedback_disposable.borrow_mut().take();
        if let Some(disposable) = disposable {
            disposable.dispose();
        }
    }

    /// `clearDragOverFeedback()`.
    fn clear_drag_over_feedback(&self) {
        *self.drag.current_drag_feedback.borrow_mut() = None;
        self.drag.current_drag_feedback_position.set(None);
        self.dispose_drag_feedback();
    }

    // DND scroll top animation

    /// `setupDragAndDropScrollTopAnimation(event)`.
    fn setup_drag_and_drop_scroll_top_animation(&self, event: &DragEvent) {
        if self.drag.drag_over_animation.borrow().is_none() {
            let view_top = self.ui.tree().absolute_bounds(self.dom_node).y0;
            let weak = self.weak_self.clone();
            let animation = animate(
                &self.ui,
                Rc::new(move || {
                    if let Some(this) = weak.upgrade() {
                        this.animate_drag_and_drop_scroll_top(view_top);
                    }
                }),
            );
            *self.drag.drag_over_animation.borrow_mut() = Some(animation);
        }

        let weak: Weak<ListView<T>> = self.weak_self.clone();
        self.drag.drag_over_animation_stop.cancel_and_set(
            move || {
                if let Some(this) = weak.upgrade() {
                    let animation = this.drag.drag_over_animation.borrow_mut().take();
                    if let Some(animation) = animation {
                        animation.dispose();
                    }
                }
            },
            1000,
        );

        self.drag.drag_over_mouse_y.set(event.pointer.y);
    }

    /// `animateDragAndDropScrollTop(viewTop)`.
    fn animate_drag_and_drop_scroll_top(&self, view_top: f64) {
        let diff = self.drag.drag_over_mouse_y.get() - view_top;
        let upper_limit = self.render_height() - 35.0;

        if diff < 35.0 {
            self.set_scroll_top(self.scroll_top() + f64::max(-14.0, (0.3 * (diff - 35.0)).floor()), false);
        } else if diff > upper_limit {
            self.set_scroll_top(self.scroll_top() + f64::min(14.0, (0.3 * (diff - upper_limit)).floor()), false);
        }
    }

    /// `teardownDragAndDropScrollTopAnimation()`.
    fn teardown_drag_and_drop_scroll_top_animation(&self) {
        self.drag.drag_over_animation_stop.cancel();

        let animation = self.drag.drag_over_animation.borrow_mut().take();
        if let Some(animation) = animation {
            animation.dispose();
        }
    }
}
