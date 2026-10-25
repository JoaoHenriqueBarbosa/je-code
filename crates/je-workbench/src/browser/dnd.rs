//! Porte parcial de `src/vs/workbench/browser/dnd.ts`: a região "Composites DND" (`CompositeDragAndDropData`,
//! `DraggedCompositeIdentifier`, `DraggedViewIdentifier`, `ICompositeDragAndDropObserverCallbacks` e o
//! `CompositeDragAndDropObserver`), com o que ela lê de `base/browser/dom.ts` (`DragAndDropObserver`) e de
//! `platform/dnd/browser/dnd.ts` (`LocalSelectionTransfer`), e o `ResourceListDnDHandler`.
//!
//! Divergências:
//! - **DOM vira estado nativo.** O `HTMLElement` é o [`ViewElement`], e o que o original faz com o DOM (os ouvintes
//!   `addDisposableListener(element, EventType.DRAG_*, ...)` e `element.draggable = true`) entra pelo trait
//!   [`DragAndDropHost`], que o chamador de `register_target` e `register_draggable` passa (quem desenha entrega os eventos de
//!   arrasto da árvore nativa). O `DragEvent` é o [`DragEvent`] (tipo, `timeStamp`, `preventDefault` e o `dataTransfer` só no
//!   `setDragImage` que o observador usa).
//! - **`LocalSelectionTransfer`.** O original compartilha uma instância global entre todos os usuários (editores, itens de
//!   árvore, composites), distinguidos pelo protótipo guardado. Aqui o protótipo é o parâmetro `P` (o [`ViewType`], que escolhe
//!   `DraggedViewIdentifier.prototype` ou `DraggedCompositeIdentifier.prototype`) e cada observador tem a sua transferência, que
//!   guarda o que ele mesmo escreve.
//! - **`CompositeDragAndDropObserver.INSTANCE`** é [`CompositeDragAndDropObserver::instance`], um singleton por thread marcado
//!   com `markAsSingleton`.
//! - **`IDraggedCompositeData.eventData`** é o `Rc<DragEvent>` do evento, para a identidade de `preventDefault` valer nos
//!   eventos que o observador reemite (`onDragStart` e `onDragEnd`).
//! - **`ResourceListDnDHandler`.** Fica no fim do arquivo. Usa o [`DragEvent`](je_widgets::dnd::DragEvent) e o
//!   `IDragAndDropData` de `je-widgets` (os que as listas e as árvores entregam), importados com os apelidos `WidgetDragEvent` e
//!   `WidgetDragAndDropData` porque este arquivo tem os seus próprios `DragEvent` e `IDragAndDropData` (os do observador de
//!   composites). O `@IInstantiationService` com o `invokeFunction(accessor => fillEditorsDragData(accessor, resources,
//!   originalEvent))` é o [`FillEditorsDragData`] injetado pelo chamador: `fillEditorsDragData` lê o serviço de arquivos de
//!   texto, o de editores, o de arquivos, o de rótulos e o registro de contribuições de arrasto, e seu porte é de quem tem
//!   esses serviços. O `onWillDragElements` protegido que as subclasses sobrescrevem é o
//!   [`ResourceListDnDHandler::with_will_drag_elements`]. O resultado de `onDragOver` (`boolean | ITreeDragOverReaction` na
//!   árvore, `boolean | IListDragOverReaction` na lista) é o parâmetro `R`, que só precisa construir o `false`.
//! - Fica de fora o resto de `dnd.ts` (`ResourcesDropHandler`, `fillEditorsDragData`, `extractTreeDropData`,
//!   `toggleDropEffect`, os alvos de entrada de texto e o rastreador de janela arrastada), que
//!   dependem de `DataTransfer`, do serviço de arquivos e das janelas.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

use je_base::event::Emitter;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, mark_as_singleton};
use je_base::uri::URI;
use je_platform::contextview::view_element::ViewElement;
use je_platform::resources::ext_uri;
use je_widgets::dnd::{DragEvent as WidgetDragEvent, IDragAndDropData as WidgetDragAndDropData};
use je_widgets::list::list_dnd::{ElementsDragAndDropData, IListDragAndDrop, ListDragOverResult, ListViewTargetSector};

// region: base/browser/dom.ts e platform/dnd/browser/dnd.ts

/// Os eventos de arrasto (`EventType.DRAG_START` e companhia).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragEventKind {
    DragStart,
    Drag,
    DragEnter,
    DragOver,
    DragLeave,
    DragEnd,
    Drop,
}

/// O `DataTransfer` no que o observador usa.
pub trait IDataTransfer {
    /// `setDragImage(element, x, y)`.
    fn set_drag_image(&self, element: &Rc<ViewElement>, x: f64, y: f64);
}

/// `DragEvent`.
pub struct DragEvent {
    /// O `type` do evento.
    pub kind: DragEventKind,
    /// `timeStamp`.
    pub time_stamp: f64,
    /// `dataTransfer` (`null` num evento criado sem dados, como `new DragEvent('dragstart')`).
    pub data_transfer: Option<Rc<dyn IDataTransfer>>,
    default_prevented: Cell<bool>,
}

impl DragEvent {
    /// `new DragEvent(type)`: sem `dataTransfer` e com `timeStamp` zero.
    pub fn new(kind: DragEventKind) -> Rc<Self> {
        Self::with_details(kind, 0.0, None)
    }

    /// Um `DragEvent` com o `timeStamp` e o `dataTransfer` dados.
    pub fn with_details(kind: DragEventKind, time_stamp: f64, data_transfer: Option<Rc<dyn IDataTransfer>>) -> Rc<Self> {
        Rc::new(Self { kind, time_stamp, data_transfer, default_prevented: Cell::new(false) })
    }

    /// `preventDefault()`.
    pub fn prevent_default(&self) {
        self.default_prevented.set(true);
    }

    /// `defaultPrevented`.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented.get()
    }
}

/// O tratador de um evento de arrasto.
pub type DragEventHandler = Rc<dyn Fn(&Rc<DragEvent>)>;

/// Tudo que o observador faz com o DOM além do que o [`ViewElement`] guarda.
pub trait DragAndDropHost {
    /// `addDisposableListener(element, type, handler)`: o ouvinte vale até o disposable devolvido ser descartado.
    fn add_listener(&self, element: &Rc<ViewElement>, kind: DragEventKind, handler: DragEventHandler) -> DisposableRef;

    /// `element.draggable = draggable`.
    fn set_draggable(&self, element: &Rc<ViewElement>, draggable: bool);
}

/// `IDragAndDropObserverCallbacks`.
#[derive(Clone, Default)]
pub struct DragAndDropObserverCallbacks {
    pub on_drag_start: Option<DragEventHandler>,
    pub on_drag: Option<DragEventHandler>,
    pub on_drag_enter: Option<DragEventHandler>,
    /// `onDragOver(e, dragDuration)`.
    pub on_drag_over: Option<Rc<dyn Fn(&Rc<DragEvent>, f64)>>,
    pub on_drag_leave: Option<DragEventHandler>,
    pub on_drag_end: Option<DragEventHandler>,
    pub on_drop: Option<DragEventHandler>,
}

/// `DragAndDropObserver`: entrega os eventos de arrasto de um elemento, com o contador que corrige as chamadas repetidas de
/// `dragenter` e `dragleave` quando o elemento tem filhos (https://github.com/microsoft/vscode/issues/14470).
pub struct DragAndDropObserver {
    base: Rc<Disposable>,
}

impl DragAndDropObserver {
    pub fn new(host: &Rc<dyn DragAndDropHost>, element: &Rc<ViewElement>, callbacks: DragAndDropObserverCallbacks) -> Rc<Self> {
        let base = Disposable::new();

        // A helper to fix issues with repeated DRAG_ENTER / DRAG_LEAVE calls when the element has child elements where the
        // events are fired repeatedly.
        let counter = Rc::new(Cell::new(0_i32));

        // Allows to measure the duration of the drag operation.
        let drag_start_time = Rc::new(Cell::new(0.0_f64));

        let listen = |kind: DragEventKind, handler: DragEventHandler| {
            base.register(host.add_listener(element, kind, handler));
        };

        if let Some(on_drag_start) = callbacks.on_drag_start {
            listen(DragEventKind::DragStart, on_drag_start);
        }

        if let Some(on_drag) = callbacks.on_drag {
            listen(DragEventKind::Drag, on_drag);
        }

        let (enter_counter, enter_start_time, on_drag_enter) = (counter.clone(), drag_start_time.clone(), callbacks.on_drag_enter);
        listen(
            DragEventKind::DragEnter,
            Rc::new(move |e: &Rc<DragEvent>| {
                enter_counter.set(enter_counter.get() + 1);
                enter_start_time.set(e.time_stamp);

                if let Some(on_drag_enter) = &on_drag_enter {
                    on_drag_enter(e);
                }
            }),
        );

        let (over_start_time, on_drag_over) = (drag_start_time.clone(), callbacks.on_drag_over);
        listen(
            DragEventKind::DragOver,
            Rc::new(move |e: &Rc<DragEvent>| {
                e.prevent_default(); // needed so that the drop event fires (https://stackoverflow.com/questions/21339924/drop-event-not-firing-in-chrome)

                if let Some(on_drag_over) = &on_drag_over {
                    on_drag_over(e, e.time_stamp - over_start_time.get());
                }
            }),
        );

        let (leave_counter, leave_start_time, on_drag_leave) = (counter.clone(), drag_start_time.clone(), callbacks.on_drag_leave);
        listen(
            DragEventKind::DragLeave,
            Rc::new(move |e: &Rc<DragEvent>| {
                leave_counter.set(leave_counter.get() - 1);

                if leave_counter.get() == 0 {
                    leave_start_time.set(0.0);

                    if let Some(on_drag_leave) = &on_drag_leave {
                        on_drag_leave(e);
                    }
                }
            }),
        );

        // `dragend` e `drop` zeram o contador e o tempo antes de chamar o tratador.
        let reset_then = |callback: Option<DragEventHandler>| -> DragEventHandler {
            let (counter, drag_start_time) = (counter.clone(), drag_start_time.clone());
            Rc::new(move |e: &Rc<DragEvent>| {
                counter.set(0);
                drag_start_time.set(0.0);

                if let Some(callback) = &callback {
                    callback(e);
                }
            })
        };
        listen(DragEventKind::DragEnd, reset_then(callbacks.on_drag_end));
        listen(DragEventKind::Drop, reset_then(callbacks.on_drop));

        Rc::new(Self { base })
    }
}

impl IDisposable for DragAndDropObserver {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `LocalSelectionTransfer<T>`: o dado local de um arrasto, com o protótipo (`P`) que o identifica.
pub struct LocalSelectionTransfer<P: Copy + PartialEq, T: Clone> {
    data: RefCell<Option<Vec<T>>>,
    proto: Cell<Option<P>>,
}

impl<P: Copy + PartialEq, T: Clone> Default for LocalSelectionTransfer<P, T> {
    fn default() -> Self {
        Self { data: RefCell::new(None), proto: Cell::new(None) }
    }
}

impl<P: Copy + PartialEq, T: Clone> LocalSelectionTransfer<P, T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `hasData(proto)`.
    pub fn has_data(&self, proto: P) -> bool {
        self.proto.get() == Some(proto)
    }

    /// `clearData(proto)`.
    pub fn clear_data(&self, proto: P) {
        if self.has_data(proto) {
            self.proto.set(None);
            *self.data.borrow_mut() = None;
        }
    }

    /// `getData(proto)`.
    pub fn get_data(&self, proto: P) -> Option<Vec<T>> {
        if self.has_data(proto) { self.data.borrow().clone() } else { None }
    }

    /// `setData(data, proto)`.
    pub fn set_data(&self, data: Vec<T>, proto: P) {
        *self.data.borrow_mut() = Some(data);
        self.proto.set(Some(proto));
    }
}

// endregion

// region: Composites DND

/// `IDragAndDropData` (`base/browser/dnd.ts`).
pub trait IDragAndDropData {
    /// `update(dataTransfer)`.
    fn update(&self, data_transfer: &dyn IDataTransfer);
}

/// `ViewType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewType {
    Composite,
    View,
}

/// O `{ type, id }` que `CompositeDragAndDropData.getData()` devolve e que o provedor do item arrastado informa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositeDragData {
    /// `type`.
    pub view_type: ViewType,
    pub id: String,
}

/// `CompositeDragAndDropData`.
#[derive(Clone, Debug)]
pub struct CompositeDragAndDropData {
    view_type: ViewType,
    id: String,
}

impl CompositeDragAndDropData {
    pub fn new(view_type: ViewType, id: &str) -> Self {
        Self { view_type, id: id.to_owned() }
    }

    /// `getData()`.
    pub fn get_data(&self) -> CompositeDragData {
        CompositeDragData { view_type: self.view_type, id: self.id.clone() }
    }
}

impl IDragAndDropData for CompositeDragAndDropData {
    fn update(&self, _data_transfer: &dyn IDataTransfer) {
        // no-op
    }
}

/// `IDraggedCompositeData`.
#[derive(Clone)]
pub struct DraggedCompositeData {
    pub event_data: Rc<DragEvent>,
    pub drag_and_drop_data: CompositeDragAndDropData,
}

/// `DraggedCompositeIdentifier`.
#[derive(Clone, Debug)]
pub struct DraggedCompositeIdentifier {
    composite_id: String,
}

impl DraggedCompositeIdentifier {
    pub fn new(composite_id: &str) -> Self {
        Self { composite_id: composite_id.to_owned() }
    }

    pub fn id(&self) -> &str {
        &self.composite_id
    }
}

/// `DraggedViewIdentifier`.
#[derive(Clone, Debug)]
pub struct DraggedViewIdentifier {
    view_id: String,
}

impl DraggedViewIdentifier {
    pub fn new(view_id: &str) -> Self {
        Self { view_id: view_id.to_owned() }
    }

    pub fn id(&self) -> &str {
        &self.view_id
    }
}

/// `DraggedCompositeIdentifier | DraggedViewIdentifier`.
#[derive(Clone, Debug)]
pub enum DraggedIdentifier {
    Composite(DraggedCompositeIdentifier),
    View(DraggedViewIdentifier),
}

impl DraggedIdentifier {
    pub fn id(&self) -> &str {
        match self {
            Self::Composite(identifier) => identifier.id(),
            Self::View(identifier) => identifier.id(),
        }
    }
}

/// Um callback do `ICompositeDragAndDropObserverCallbacks`.
pub type CompositeCallback = Rc<dyn Fn(&DraggedCompositeData)>;

/// `ICompositeDragAndDropObserverCallbacks`.
#[derive(Clone, Default)]
pub struct CompositeDragAndDropObserverCallbacks {
    pub on_drag_enter: Option<CompositeCallback>,
    pub on_drag_leave: Option<CompositeCallback>,
    pub on_drop: Option<CompositeCallback>,
    pub on_drag_over: Option<CompositeCallback>,
    pub on_drag_start: Option<CompositeCallback>,
    pub on_drag_end: Option<CompositeCallback>,
}

type Transfer = LocalSelectionTransfer<ViewType, DraggedIdentifier>;

/// `readDragData(type)`.
fn read_drag_data(transfer: &Transfer, view_type: ViewType) -> Option<CompositeDragAndDropData> {
    let data = transfer.get_data(view_type)?;
    let first = data.first()?;
    Some(CompositeDragAndDropData::new(view_type, first.id()))
}

/// `this.readDragData('composite') || this.readDragData('view')`.
fn read_any_drag_data(transfer: &Transfer) -> Option<CompositeDragAndDropData> {
    read_drag_data(transfer, ViewType::Composite).or_else(|| read_drag_data(transfer, ViewType::View))
}

/// `writeDragData(id, type)`.
fn write_drag_data(transfer: &Transfer, id: &str, view_type: ViewType) {
    let identifier = match view_type {
        ViewType::View => DraggedIdentifier::View(DraggedViewIdentifier::new(id)),
        ViewType::Composite => DraggedIdentifier::Composite(DraggedCompositeIdentifier::new(id)),
    };
    transfer.set_data(vec![identifier], view_type);
}

/// O tratador que lê o dado do arrasto e o entrega ao `callback`, se houver os dois (`preventDefault` antes, nos alvos).
fn deliver_handler(transfer: &Rc<Transfer>, callback: Option<CompositeCallback>, prevent_default: bool) -> DragEventHandler {
    let transfer = transfer.clone();
    Rc::new(move |e: &Rc<DragEvent>| {
        if prevent_default {
            e.prevent_default();
        }

        if let Some(callback) = &callback {
            if let Some(data) = read_any_drag_data(&transfer) {
                callback(&DraggedCompositeData { event_data: e.clone(), drag_and_drop_data: data });
            }
        }
    })
}

/// `CompositeDragAndDropObserver`.
pub struct CompositeDragAndDropObserver {
    base: Rc<Disposable>,
    transfer_data: Rc<Transfer>,
    on_drag_start: Rc<Emitter<DraggedCompositeData>>,
    on_drag_end: Rc<Emitter<DraggedCompositeData>>,
}

thread_local! {
    static INSTANCE: RefCell<Option<Rc<CompositeDragAndDropObserver>>> = const { RefCell::new(None) };
}

impl CompositeDragAndDropObserver {
    /// `CompositeDragAndDropObserver.INSTANCE`.
    pub fn instance() -> Rc<Self> {
        if let Some(instance) = INSTANCE.with(|instance| instance.borrow().clone()) {
            return instance;
        }

        let instance = Self::new();
        mark_as_singleton(instance.base.clone());
        INSTANCE.with(|slot| *slot.borrow_mut() = Some(instance.clone()));
        instance
    }

    fn new() -> Rc<Self> {
        let base = Disposable::new();
        let transfer_data: Rc<Transfer> = Rc::new(LocalSelectionTransfer::new());

        let on_drag_start: Rc<Emitter<DraggedCompositeData>> = Emitter::new();
        base.register(on_drag_start.clone());
        let on_drag_end: Rc<Emitter<DraggedCompositeData>> = Emitter::new();
        base.register(on_drag_end.clone());

        let transfer = transfer_data.clone();
        base.register(on_drag_end.event().listen(move |e| {
            let data = e.drag_and_drop_data.get_data();
            let read = read_drag_data(&transfer, data.view_type);
            if read.is_some_and(|read| read.get_data().id == data.id) {
                transfer.clear_data(data.view_type);
            }
        }));

        Rc::new(Self { base, transfer_data, on_drag_start, on_drag_end })
    }

    /// Os tratadores que `registerTarget` e `registerDraggable` repassam ao `DragAndDropObserver`. Nos alvos o `dragenter` e o
    /// `dragover` chamam `preventDefault`.
    fn forwarding_callbacks(&self, callbacks: &CompositeDragAndDropObserverCallbacks, prevent_default: bool) -> DragAndDropObserverCallbacks {
        let on_drag_over = deliver_handler(&self.transfer_data, callbacks.on_drag_over.clone(), prevent_default);

        let transfer = self.transfer_data.clone();
        let on_drag_end = self.on_drag_end.clone();
        let on_drop = callbacks.on_drop.clone();

        DragAndDropObserverCallbacks {
            on_drag_enter: Some(deliver_handler(&self.transfer_data, callbacks.on_drag_enter.clone(), prevent_default)),
            on_drag_leave: Some(deliver_handler(&self.transfer_data, callbacks.on_drag_leave.clone(), false)),
            on_drag_over: Some(Rc::new(move |e: &Rc<DragEvent>, _drag_duration: f64| on_drag_over(e))),
            on_drop: Some(Rc::new(move |e: &Rc<DragEvent>| {
                let Some(on_drop) = &on_drop else {
                    return;
                };
                let Some(data) = read_any_drag_data(&transfer) else {
                    return;
                };

                let dragged = DraggedCompositeData { event_data: e.clone(), drag_and_drop_data: data };
                on_drop(&dragged);

                // Fire drag event in case drop handler destroys the dragged element
                on_drag_end.fire(dragged);
            })),
            ..DragAndDropObserverCallbacks::default()
        }
    }

    /// As inscrições de `onDragStart` e `onDragEnd` do observador, que valem até o `store` ser descartado.
    fn subscribe_lifecycle(&self, store: &DisposableStore, callbacks: &CompositeDragAndDropObserverCallbacks) {
        if let Some(on_drag_start) = callbacks.on_drag_start.clone() {
            self.on_drag_start.event().listen_in(move |e| on_drag_start(e), store);
        }

        if let Some(on_drag_end) = callbacks.on_drag_end.clone() {
            self.on_drag_end.event().listen_in(move |e| on_drag_end(e), store);
        }
    }

    /// `registerTarget(element, callbacks)`.
    pub fn register_target(&self, host: &Rc<dyn DragAndDropHost>, element: &Rc<ViewElement>, callbacks: CompositeDragAndDropObserverCallbacks) -> DisposableRef {
        let store = DisposableStore::new();

        store.add_rc(DragAndDropObserver::new(host, element, self.forwarding_callbacks(&callbacks, true)));
        self.subscribe_lifecycle(&store, &callbacks);

        store
    }

    /// `registerDraggable(element, draggedItemProvider, callbacks)`.
    pub fn register_draggable(
        &self,
        host: &Rc<dyn DragAndDropHost>,
        element: &Rc<ViewElement>,
        dragged_item_provider: Rc<dyn Fn() -> CompositeDragData>,
        callbacks: CompositeDragAndDropObserverCallbacks,
    ) -> DisposableRef {
        host.set_draggable(element, true);

        let store = DisposableStore::new();

        let start_provider = dragged_item_provider.clone();
        let start_transfer = self.transfer_data.clone();
        let start_emitter = self.on_drag_start.clone();
        let start_element = element.clone();
        let on_drag_start: DragEventHandler = Rc::new(move |e: &Rc<DragEvent>| {
            let CompositeDragData { view_type, id } = start_provider();
            write_drag_data(&start_transfer, &id, view_type);

            if let Some(data_transfer) = &e.data_transfer {
                data_transfer.set_drag_image(&start_element, 0.0, 0.0);
            }

            if let Some(data) = read_drag_data(&start_transfer, view_type) {
                start_emitter.fire(DraggedCompositeData { event_data: e.clone(), drag_and_drop_data: data });
            }
        });

        let end_transfer = self.transfer_data.clone();
        let end_emitter = self.on_drag_end.clone();
        let on_drag_end: DragEventHandler = Rc::new(move |e: &Rc<DragEvent>| {
            let CompositeDragData { view_type, .. } = dragged_item_provider();
            let Some(data) = read_drag_data(&end_transfer, view_type) else {
                return;
            };

            end_emitter.fire(DraggedCompositeData { event_data: e.clone(), drag_and_drop_data: data });
        });

        let observer_callbacks =
            DragAndDropObserverCallbacks { on_drag_start: Some(on_drag_start), on_drag_end: Some(on_drag_end), ..self.forwarding_callbacks(&callbacks, false) };
        store.add_rc(DragAndDropObserver::new(host, element, observer_callbacks));
        self.subscribe_lifecycle(&store, &callbacks);

        store
    }
}

impl IDisposable for CompositeDragAndDropObserver {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// endregion

// region: ResourceListDnDHandler

/// `instantiationService.invokeFunction(accessor => fillEditorsDragData(accessor, resources, originalEvent))`: grava os
/// recursos no `dataTransfer` do arrasto (ver as divergências do módulo).
pub type FillEditorsDragData = Rc<dyn Fn(&[URI], &WidgetDragEvent)>;

/// `onWillDragElements(elements, originalEvent)`.
pub type WillDragElements<T> = Rc<dyn Fn(&[T], &WidgetDragEvent)>;

/// `ResourceListDnDHandler<T>`: o arrastar e soltar de uma lista ou árvore cujos elementos mapeiam para um recurso. Só
/// arrasta (o `onDragOver` recusa e o `drop` não faz nada). `R` é o tipo do resultado de `onDragOver` (ver as divergências do
/// módulo).
pub struct ResourceListDnDHandler<T, R = ListDragOverResult> {
    to_resource: Rc<dyn Fn(&T) -> Option<URI>>,
    fill_editors_drag_data: FillEditorsDragData,
    on_will_drag_elements: Option<WillDragElements<T>>,
    result: PhantomData<fn() -> R>,
}

impl<T, R> ResourceListDnDHandler<T, R> {
    /// `new ResourceListDnDHandler(toResource, instantiationService)`.
    pub fn new(to_resource: Rc<dyn Fn(&T) -> Option<URI>>, fill_editors_drag_data: FillEditorsDragData) -> Self {
        Self { to_resource, fill_editors_drag_data, on_will_drag_elements: None, result: PhantomData }
    }

    /// A sobrescrita do `onWillDragElements` protegido, que a subclasse faz.
    pub fn with_will_drag_elements(mut self, on_will_drag_elements: WillDragElements<T>) -> Self {
        self.on_will_drag_elements = Some(on_will_drag_elements);
        self
    }
}

impl<T: Clone + 'static, R: From<bool>> IListDragAndDrop<T, R> for ResourceListDnDHandler<T, R> {
    fn get_drag_uri(&self, element: &T) -> Option<String> {
        (self.to_resource)(element).map(|resource| resource.to_string(false).to_string_lossy())
    }

    fn get_drag_label(&self, elements: &[T], _original_event: &WidgetDragEvent) -> Option<String> {
        let resources: Vec<URI> = elements.iter().filter_map(|element| (self.to_resource)(element)).collect();
        match resources.as_slice() {
            [] => None,
            [resource] => Some(ext_uri().basename(resource, None)),
            _ => Some(resources.len().to_string()),
        }
    }

    fn on_drag_start(&self, data: &Rc<dyn WidgetDragAndDropData>, original_event: &WidgetDragEvent) {
        let Some(data) = data.as_any().downcast_ref::<ElementsDragAndDropData<T>>() else {
            return;
        };
        let elements = &data.elements;
        let resources: Vec<URI> = elements.iter().filter_map(|element| (self.to_resource)(element)).collect();
        if let Some(on_will_drag_elements) = &self.on_will_drag_elements {
            on_will_drag_elements(elements, original_event);
        }
        if !resources.is_empty() {
            // Apply some datatransfer types to allow for dragging the element outside of the application
            (self.fill_editors_drag_data)(&resources, original_event);
        }
    }

    fn on_drag_over(
        &self,
        _data: &Rc<dyn WidgetDragAndDropData>,
        _target_element: Option<&T>,
        _target_index: Option<usize>,
        _target_sector: Option<ListViewTargetSector>,
        _original_event: &WidgetDragEvent,
    ) -> R {
        R::from(false)
    }

    fn drop(&self, _data: &Rc<dyn WidgetDragAndDropData>, _target_element: Option<&T>, _target_index: Option<usize>, _target_sector: Option<ListViewTargetSector>, _original_event: &WidgetDragEvent) {}
}

// endregion
