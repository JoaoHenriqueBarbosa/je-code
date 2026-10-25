//! Utilitários do editor de diff (`src/vs/editor/browser/widget/diffEditor/utils.ts`) que os view models, os componentes e o widget
//! usam: `filterWithPrevious`, `applyObservableDecorations`, `ObservableElementSizeObserver`, `IObservableViewZone`,
//! `applyViewZones` e `translatePosition`, mais os que as features da fatia 3 usam: `PlaceholderViewZone`, `ManagedOverlayWidget` e
//! `ViewZoneOverlayWidget`, e os que o `DiffEditorViewZones` usa: `joinCombine` e `animatedObservable`. O `RefCounted` do mesmo arquivo
//! vive em `je_base::lifecycle` ao lado do `RefCountedDisposable`. O restante do arquivo (`deepMerge`...) pertence às features do editor
//! de diff que ainda faltam e entra com elas.
//!
//! Divergências:
//! - **`animatedObservable`** troca o `window.requestAnimationFrame` pelo [`CodeWindow::request_animation_frame`], que não tem
//!   cancelamento: cada pedido leva o número da geração corrente e o `cancelAnimationFrame` é incrementá-la, de modo que o quadro já
//!   pedido não faz nada. O `Date.now()` é [`runtime::date_now`], o relógio que os testes falsificam.
//! - **`joinCombine`** devolve sempre um `Vec` novo (o original devolve o próprio array de entrada quando o outro está vazio).
//! - **`ObservableElementSizeObserver`** recebe o `Option<Rc<dyn ReferenceDomElement>>` do [`ElementSizeObserver`] no lugar do
//!   `HTMLElement | null`. O `observe(dimension)` do original só repassa ao `ElementSizeObserver` (e uma função de repasse não
//!   existe neste projeto), então o observador interno é o campo público
//!   [`ObservableElementSizeObserver::element_size_observer`] e o widget chama `observe` nele.
//! - **`applyStyle` e `CSSStyle`** não são portados como função: escrevem no `style` de um `HTMLElement`, e o estilo é o dado que o
//!   pintor lê. O único uso destas utilidades, o `applyStyle(htmlElement, { height, top })` do `ViewZoneOverlayWidget`, é o autorun
//!   que grava o [`ViewZoneOverlayLayout`] (ver [`ViewZoneOverlayWidget::layout`]); as features gravam o resto do estilo (largura,
//!   posição) nos próprios dados de visão.
//! - **`IObservableViewZone`** é o trait [`IObservableViewZone`] sobre o [`IViewZone`] nativo; `onChange?: IObservable<unknown>` é
//!   `Option<ObservableRef>` (observável apagado) e `setZoneId?` tem corpo vazio por padrão. A zona se identifica pelo endereço do
//!   `Rc` (a chave dos `Map` do original).
//! - **`PlaceholderViewZone.domNode`** (um `div` vazio) é o [`PlaceholderViewZoneNode`], o dado que o pintor recupera de
//!   [`IViewZone::dom_node`]; a zona é só o espaço reservado, e quem desenha é o overlay posicionado sobre ela.
//! - **`ManagedOverlayWidget`** não recebe o `HTMLElement` (o `IOverlayWidget` nativo não tem `getDomNode`): o conteúdo é do dono do
//!   widget, que o pintor localiza pelo id do overlay ([`ManagedOverlayWidget::id`]).
//! - **`derivedDisposable`** de `je_base::observable` devolve um observável apagado (`Option<DisposableRef>`); o widget lê o objeto
//!   criado (a feature) pelo tipo, então [`derived_disposable_with`], [`derived_disposable_of`] e
//!   [`derived_optional_disposable_of`] têm a mesma semântica (descartam o objeto anterior a cada recálculo e quando o derivado
//!   fica sem observadores) mas guardam o valor tipado.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::browser::dom::CodeWindow;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::observable::base::ObservableRef;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value, observable_value_owned};
use je_base::runtime;
use je_base::observable::change_tracker::FnChangeTracker;
use je_base::observable::{
    AnyObservable, DebugNameOptions, DebugOwner, DerivedOptions, IObservable, IReader, Observable, SettableExt, StrictEquals, autorun,
    autorun_handle_changes, autorun_opts, autorun_with_store, derived_opts, transaction,
};

use crate::code_editor_widget::CodeEditorWidget;
use crate::config::element_size_observer::{ElementSizeObserver, ReferenceDomElement};
use crate::core_2d::dimension::Dimension;
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::editor_browser::{IOverlayWidget, IViewZone, IViewZoneChangeAccessor, OverlayWidgetPosition};
use crate::model::ModelDeltaDecoration;
use crate::position::Position;
use crate::position_to_offset::TextLength;
use crate::range::Range;

/// `joinCombine(arr1, arr2, keySelector, combine)`: junta duas listas ordenadas pela chave, combinando os elementos de mesma chave.
pub fn join_combine<T: Clone>(arr1: &[T], arr2: &[T], key_selector: impl Fn(&T) -> i32, combine: impl Fn(&T, &T) -> T) -> Vec<T> {
    if arr1.is_empty() {
        return arr2.to_vec();
    }
    if arr2.is_empty() {
        return arr1.to_vec();
    }

    let mut result: Vec<T> = Vec::with_capacity(arr1.len() + arr2.len());
    let (mut i, mut j) = (0_usize, 0_usize);
    while i < arr1.len() && j < arr2.len() {
        let (val1, val2) = (&arr1[i], &arr2[j]);
        let (key1, key2) = (key_selector(val1), key_selector(val2));

        if key1 < key2 {
            result.push(val1.clone());
            i += 1;
        } else if key1 > key2 {
            result.push(val2.clone());
            j += 1;
        } else {
            result.push(combine(val1, val2));
            i += 1;
            j += 1;
        }
    }
    result.extend_from_slice(&arr1[i..]);
    result.extend_from_slice(&arr2[j..]);
    result
}

/// `filterWithPrevious(arr, filter)`: filtra os elementos dando a cada um o elemento anterior da lista original (e não o anterior
/// que passou no filtro).
pub fn filter_with_previous<T: Clone>(arr: &[T], mut filter: impl FnMut(&T, Option<&T>) -> bool) -> Vec<T> {
    let mut prev: Option<&T> = None;
    arr.iter()
        .filter(|cur| {
            let result = filter(cur, prev);
            prev = Some(*cur);
            result
        })
        .cloned()
        .collect()
}

/// `applyObservableDecorations(editor, decorations)`: mantém as decorações do editor iguais ao valor do observável e as remove ao
/// descartar. A lista é um `Rc` porque o `===` do original compara a identidade do array.
pub fn apply_observable_decorations(editor: &Rc<CodeEditorWidget>, decorations: IObservable<Rc<Vec<ModelDeltaDecoration>>>) -> DisposableRef {
    let d = DisposableStore::new();
    let decorations_collection = Rc::new(editor.create_decorations_collection(None));
    let debug_name = format!("Apply decorations from {}", decorations.debug_name());
    {
        let decorations_collection = decorations_collection.clone();
        d.add(autorun_opts(DebugNameOptions::named(&debug_name), move |reader| {
            let reader: &dyn IReader = reader;
            let d = decorations.read(reader);
            decorations_collection.set(&d);
        }));
    }
    d.add(to_disposable(move || decorations_collection.clear()));
    d
}

/// `ObservableElementSizeObserver`: a largura e a altura do elemento como observáveis.
pub struct ObservableElementSizeObserver {
    base: Rc<Disposable>,
    /// O `elementSizeObserver` (privado no original; ver as divergências do módulo).
    pub element_size_observer: Rc<ElementSizeObserver>,
    /// `width`.
    pub width: IObservable<f64>,
    /// `height`.
    pub height: IObservable<f64>,
    automatic_layout: Cell<bool>,
}

impl ObservableElementSizeObserver {
    pub fn new(element: Option<Rc<dyn ReferenceDomElement>>, dimension: Option<Dimension>) -> Rc<Self> {
        let base = Disposable::new();
        let element_size_observer = ElementSizeObserver::new(element, dimension);
        base.register(element_size_observer.clone());

        let owner = DebugOwner::new("ObservableElementSizeObserver");
        let width: Rc<ObservableValue<f64, ()>> = observable_value_owned(owner.clone(), element_size_observer.get_width());
        let height: Rc<ObservableValue<f64, ()>> = observable_value_owned(owner, element_size_observer.get_height());

        {
            // O `Weak` evita o ciclo observador -> emissor -> ouvinte -> observador.
            let (observer, width, height) = (Rc::downgrade(&element_size_observer), width.clone(), height.clone());
            base.register(element_size_observer.on_did_change().listen(move |_| {
                let Some(observer) = observer.upgrade() else { return };
                transaction(|tx| {
                    // Set width/height from elementSizeObserver
                    width.set(observer.get_width(), Some(tx));
                    height.set(observer.get_height(), Some(tx));
                });
            }));
        }

        Rc::new(Self { base, element_size_observer, width, height, automatic_layout: Cell::new(false) })
    }

    /// `automaticLayout`.
    pub fn automatic_layout(&self) -> bool {
        self.automatic_layout.get()
    }

    /// `setAutomaticLayout(automaticLayout)`.
    pub fn set_automatic_layout(&self, automatic_layout: bool) {
        self.automatic_layout.set(automatic_layout);
        if automatic_layout {
            self.element_size_observer.start_observing();
        } else {
            self.element_size_observer.stop_observing();
        }
    }
}

impl IDisposable for ObservableElementSizeObserver {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `IObservableViewZone`: uma zona de visão que pede novo layout quando um observável muda e que sabe o próprio id.
pub trait IObservableViewZone: IViewZone {
    /// `onChange`: causes the view zone to relayout.
    fn on_change(&self) -> Option<ObservableRef> {
        None
    }

    /// `setZoneId(zoneId)`: tells a view zone its id.
    fn set_zone_id(&self, _zone_id: &str) {}
}

/// A chave de identidade de uma zona (o objeto, no `Map` do original).
fn view_zone_key(zone: &Rc<dyn IObservableViewZone>) -> usize {
    Rc::as_ptr(zone).cast::<()>() as usize
}

/// `setIsUpdatingViewZones` de `applyViewZones`.
pub type SetIsUpdatingViewZones = Rc<dyn Fn(bool)>;

/// O `zoneIds?: Set<string>` de `applyViewZones`, compartilhado com quem o lê.
pub type ViewZoneIdSet = Rc<RefCell<HashSet<String>>>;

/// `applyViewZones(editor, viewZones, setIsUpdating?, zoneIds?)`: mantém as zonas do editor iguais à lista do observável.
pub fn apply_view_zones(
    editor: &Rc<CodeEditorWidget>,
    view_zones: IObservable<Rc<Vec<Rc<dyn IObservableViewZone>>>>,
    set_is_updating: Option<SetIsUpdatingViewZones>,
    zone_ids: Option<ViewZoneIdSet>,
) -> DisposableRef {
    let store = DisposableStore::new();
    let last_view_zone_ids: Rc<RefCell<Vec<String>>> = Rc::default();
    let set_updating: SetIsUpdatingViewZones = set_is_updating.unwrap_or_else(|| Rc::new(|_| {}));

    {
        let (editor, last_view_zone_ids, zone_ids, set_updating) = (editor.clone(), last_view_zone_ids.clone(), zone_ids.clone(), set_updating.clone());
        store.add(autorun_with_store(move |reader, store| {
            // applyViewZones
            let cur_view_zones = view_zones.read(reader);

            let view_zone_ids_per_view_zone: Rc<RefCell<HashMap<usize, String>>> = Rc::default();
            let view_zone_id_per_on_change_observable: Rc<RefCell<HashMap<usize, String>>> = Rc::default();

            // Add/remove view zones
            set_updating(true);
            editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| {
                for id in last_view_zone_ids.borrow().iter() {
                    accessor.remove_zone(id);
                    if let Some(zone_ids) = &zone_ids {
                        zone_ids.borrow_mut().remove(id);
                    }
                }
                last_view_zone_ids.borrow_mut().clear();

                for z in cur_view_zones.iter() {
                    let zone: Rc<dyn IViewZone> = z.clone();
                    let id = accessor.add_zone(zone);
                    z.set_zone_id(&id);
                    last_view_zone_ids.borrow_mut().push(id.clone());
                    if let Some(zone_ids) = &zone_ids {
                        zone_ids.borrow_mut().insert(id.clone());
                    }
                    view_zone_ids_per_view_zone.borrow_mut().insert(view_zone_key(z), id);
                }
            });
            set_updating(false);

            // Layout zone on change
            let change_tracker = FnChangeTracker::<RefCell<Vec<String>>>::new(
                |_| RefCell::new(Vec::new()),
                {
                    let view_zone_id_per_on_change_observable = view_zone_id_per_on_change_observable.clone();
                    move |context, change_summary| {
                        if let Some(id) = view_zone_id_per_on_change_observable.borrow().get(&context.changed_observable().id()) {
                            change_summary.borrow_mut().push(id.clone());
                        }
                        true
                    }
                },
            );
            let (editor, set_updating) = (editor.clone(), set_updating.clone());
            store.add(autorun_handle_changes(
                DebugNameOptions::named("layoutZone on change"),
                change_tracker,
                move |reader, change_summary: &RefCell<Vec<String>>| {
                    for vz in cur_view_zones.iter() {
                        if let Some(on_change) = vz.on_change() {
                            if let Some(id) = view_zone_ids_per_view_zone.borrow().get(&view_zone_key(vz)) {
                                view_zone_id_per_on_change_observable.borrow_mut().insert(on_change.id(), id.clone());
                            }
                            reader.read_observable_erased(on_change, &mut || {});
                        }
                    }
                    set_updating(true);
                    editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| {
                        for id in change_summary.borrow().iter() {
                            accessor.layout_zone(id);
                        }
                    });
                    set_updating(false);
                },
            ));
        }));
    }

    {
        let editor = editor.clone();
        store.add(to_disposable(move || {
            set_updating(true);
            editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| {
                for id in last_view_zone_ids.borrow().iter() {
                    accessor.remove_zone(id);
                }
            });
            if let Some(zone_ids) = &zone_ids {
                zone_ids.borrow_mut().clear();
            }
            set_updating(false);
        }));
    }

    store
}

/// A duração da animação de `animatedObservable`.
const ANIMATION_DURATION_MS: f64 = 300.0;

/// `easeOutExpo(t, b, c, d)`.
fn ease_out_expo(t: f64, b: f64, c: f64, d: f64) -> f64 {
    if t == d { b + c } else { c * (-(2.0_f64.powf(-10.0 * t / d)) + 1.0) + b }
}

/// O estado da animação de [`animated_observable`] (as variáveis `targetVal`, `startVal`, `curVal`, `animationStartMs` e
/// `animationFrame` do original).
struct ValueAnimator {
    window: Rc<CodeWindow>,
    result: Rc<ObservableValue<f64, ()>>,
    target_val: Cell<f64>,
    start_val: Cell<f64>,
    cur_val: Cell<f64>,
    animation_start_ms: Cell<f64>,
    /// A geração do quadro pedido; `cancelAnimationFrame` é incrementá-la.
    frame_generation: Cell<u64>,
}

impl ValueAnimator {
    /// `window.cancelAnimationFrame(animationFrame)`.
    fn cancel_frame(&self) {
        self.frame_generation.set(self.frame_generation.get() + 1);
    }

    /// O corpo do autorun `update value`, depois de o observável base ter sido lido.
    fn retarget(self: &Rc<Self>, new_target: f64, animate: bool) {
        self.cancel_frame();

        self.start_val.set(self.cur_val.get());
        self.target_val.set(new_target);
        let now = runtime::date_now() as f64;
        if self.start_val.get() == self.target_val.get() {
            // No change, no animation
            self.animation_start_ms.set(now - ANIMATION_DURATION_MS);
        } else {
            self.animation_start_ms.set(now - if animate { 0.0 } else { ANIMATION_DURATION_MS });
        }

        self.update();
    }

    /// `update()`.
    fn update(self: &Rc<Self>) {
        let passed_ms = runtime::date_now() as f64 - self.animation_start_ms.get();
        let (start, target) = (self.start_val.get(), self.target_val.get());
        self.cur_val.set(ease_out_expo(passed_ms, start, target - start, ANIMATION_DURATION_MS).floor());

        if passed_ms < ANIMATION_DURATION_MS {
            let (this, generation) = (Rc::downgrade(self), self.frame_generation.get());
            self.window.request_animation_frame(move || {
                if let Some(this) = this.upgrade() {
                    if this.frame_generation.get() == generation {
                        this.update();
                    }
                }
            });
        } else {
            self.cur_val.set(target);
        }

        self.result.set(self.cur_val.get(), None);
    }
}

/// `animatedObservable(targetWindow, base, store)`: segue `base` animando a mudança em 300 ms (`easeOutExpo`) quando a mudança vem
/// marcada com `true`; sem a marca, o valor salta direto.
pub fn animated_observable(target_window: &Rc<CodeWindow>, base: &Rc<ObservableValue<f64, bool>>, store: &DisposableStore) -> IObservable<f64> {
    let target_val = base.get();
    let result: Rc<ObservableValue<f64, ()>> = observable_value("animatedValue", target_val);
    let animator = Rc::new(ValueAnimator {
        window: target_window.clone(),
        result: result.clone(),
        target_val: Cell::new(target_val),
        start_val: Cell::new(target_val),
        cur_val: Cell::new(target_val),
        animation_start_ms: Cell::new(-1.0),
        frame_generation: Cell::new(0),
    });

    let change_tracker = FnChangeTracker::<Cell<bool>>::new(
        |_| Cell::new(false),
        {
            let base = base.clone();
            move |ctx, summary| {
                if ctx.did_change(&*base) {
                    summary.set(summary.get() || ctx.change::<bool>().copied().unwrap_or(false));
                }
                true
            }
        },
    );
    {
        let (animator, base) = (animator.clone(), base.clone());
        store.add(autorun_handle_changes(DebugNameOptions::named("update value"), change_tracker, move |reader, summary: &Cell<bool>| {
            // update value
            let target = base.read(reader);
            animator.retarget(target, summary.get());
        }));
    }
    {
        let animator = animator.clone();
        store.add(to_disposable(move || animator.cancel_frame()));
    }

    result.to_observable()
}

/// O `domNode` de uma [`PlaceholderViewZone`] (um `div` vazio): o dado que o pintor recupera de [`IViewZone::dom_node`] por `downcast`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaceholderViewZoneNode;

/// `PlaceholderViewZone`: uma zona de visão que só reserva espaço (`heightInPx`) depois da linha do observável, mesmo em áreas
/// escondidas, e que conta ao dono onde (`actualTop`) e com que altura (`actualHeight`) o editor a colocou.
pub struct PlaceholderViewZone {
    dom_node: Rc<PlaceholderViewZoneNode>,
    after_line_number: IObservable<i32>,
    height_in_px: f64,
    actual_top: Rc<ObservableValue<Option<f64>, ()>>,
    actual_height: Rc<ObservableValue<Option<f64>, ()>>,
}

impl PlaceholderViewZone {
    pub fn new(after_line_number: IObservable<i32>, height_in_px: f64) -> Rc<Self> {
        let owner = DebugOwner::new("PlaceholderViewZone");
        Rc::new(Self {
            dom_node: Rc::new(PlaceholderViewZoneNode),
            after_line_number,
            height_in_px,
            actual_top: observable_value_owned(owner.clone(), None),
            actual_height: observable_value_owned(owner, None),
        })
    }

    /// `actualTop`: o topo da zona como o editor a colocou (`onDomNodeTop`).
    pub fn actual_top(&self) -> IObservable<Option<f64>> {
        self.actual_top.to_observable()
    }

    /// `actualHeight`: a altura da zona como o editor a calculou (`onComputedHeight`).
    pub fn actual_height(&self) -> IObservable<Option<f64>> {
        self.actual_height.to_observable()
    }
}

impl IViewZone for PlaceholderViewZone {
    fn after_line_number(&self) -> i32 {
        self.after_line_number.get()
    }

    fn show_in_hidden_areas(&self) -> bool {
        true
    }

    fn height_in_px(&self) -> Option<f64> {
        Some(self.height_in_px)
    }

    fn dom_node(&self) -> Rc<dyn Any> {
        self.dom_node.clone()
    }

    fn on_dom_node_top(&self, top: f64) {
        self.actual_top.set(Some(top), None);
    }

    fn on_computed_height(&self, height: f64) {
        self.actual_height.set(Some(height), None);
    }
}

impl IObservableViewZone for PlaceholderViewZone {
    fn on_change(&self) -> Option<ObservableRef> {
        Some(self.after_line_number.to_any_observable())
    }
}

thread_local! {
    /// `ManagedOverlayWidget._counter`.
    static MANAGED_OVERLAY_WIDGET_COUNTER: Cell<u32> = const { Cell::new(0) };
}

/// O `IOverlayWidget` literal de `ManagedOverlayWidget`: `getId`, `getDomNode` e `getPosition: () => null`.
struct ManagedOverlayWidgetHandle {
    id: String,
}

impl IOverlayWidget for ManagedOverlayWidgetHandle {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_position(&self) -> Option<OverlayWidgetPosition> {
        None
    }
}

/// `ManagedOverlayWidget`: registra um overlay sem posição no editor e o remove ao descartar.
pub struct ManagedOverlayWidget {
    editor: Rc<CodeEditorWidget>,
    overlay_widget: Rc<dyn IOverlayWidget>,
    id: String,
}

impl ManagedOverlayWidget {
    pub fn new(editor: &Rc<CodeEditorWidget>) -> Rc<Self> {
        let counter = MANAGED_OVERLAY_WIDGET_COUNTER.with(|counter| {
            let current = counter.get();
            counter.set(current + 1);
            current
        });
        let id = format!("managedOverlayWidget-{counter}");
        let overlay_widget: Rc<dyn IOverlayWidget> = Rc::new(ManagedOverlayWidgetHandle { id: id.clone() });
        editor.add_overlay_widget(overlay_widget.clone());
        Rc::new(Self { editor: editor.clone(), overlay_widget, id })
    }

    /// O id do overlay no editor (`_overlayWidgetId`), com que o pintor acha o conteúdo do widget.
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl IDisposable for ManagedOverlayWidget {
    fn dispose(&self) {
        self.editor.remove_overlay_widget(&self.overlay_widget);
    }
}

/// O estilo (`height` e `top`, em pixels) que o `applyStyle` de [`ViewZoneOverlayWidget`] grava; `None` é o valor ainda indefinido.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewZoneOverlayLayout {
    pub top: Option<f64>,
    pub height: Option<f64>,
}

/// `ViewZoneOverlayWidget`: a base dos widgets que se desenham sobre uma [`PlaceholderViewZone`], seguindo o topo e a altura dela. As
/// subclasses do original são structs que a contêm (composição) e registram o que precisam descartar em [`Self::register`].
pub struct ViewZoneOverlayWidget {
    base: Rc<Disposable>,
    managed: Rc<ManagedOverlayWidget>,
    layout: Rc<Cell<ViewZoneOverlayLayout>>,
}

impl ViewZoneOverlayWidget {
    pub fn new(editor: &Rc<CodeEditorWidget>, view_zone: &PlaceholderViewZone) -> Self {
        let base = Disposable::new();
        let managed = ManagedOverlayWidget::new(editor);
        base.register(managed.clone());

        let layout = Rc::new(Cell::new(ViewZoneOverlayLayout::default()));
        {
            let (layout, actual_top, actual_height) = (layout.clone(), view_zone.actual_top(), view_zone.actual_height());
            base.register(autorun(move |reader| {
                // applyStyle
                let reader: &dyn IReader = reader;
                layout.set(ViewZoneOverlayLayout { top: actual_top.read(reader), height: actual_height.read(reader) });
            }));
        }
        Self { base, managed, layout }
    }

    /// `this._register(disposable)`.
    pub fn register(&self, disposable: DisposableRef) -> DisposableRef {
        self.base.register(disposable)
    }

    /// O estilo que o `applyStyle` gravou.
    pub fn layout(&self) -> ViewZoneOverlayLayout {
        self.layout.get()
    }

    /// O id do overlay no editor.
    pub fn overlay_widget_id(&self) -> &str {
        self.managed.id()
    }
}

impl IDisposable for ViewZoneOverlayWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `lengthBetweenPositions(position1, position2)`.
fn length_between_positions(position1: Position, position2: Position) -> TextLength {
    if position1.line_number == position2.line_number {
        TextLength::new(0, position2.column - position1.column)
    } else {
        TextLength::new(position2.line_number - position1.line_number, position2.column - 1)
    }
}

/// `translatePosition(posInOriginal, mappings)`: a posição do lado original no lado modificado.
pub fn translate_position(pos_in_original: Position, mappings: &[DetailedLineRangeMapping]) -> Range {
    let at = |position: Position| Range::from_positions(position, position);

    let Some(mapping) = mappings.iter().rev().find(|m| m.original.start_line_number <= pos_in_original.line_number) else {
        // No changes before the position
        return at(pos_in_original);
    };

    if mapping.original.end_line_number_exclusive <= pos_in_original.line_number {
        let new_line_number = pos_in_original.line_number - mapping.original.end_line_number_exclusive + mapping.modified.end_line_number_exclusive;
        return at(Position::new(new_line_number, pos_in_original.column));
    }

    let Some(inner_changes) = &mapping.inner_changes else {
        // Only for legacy algorithm
        return at(Position::new(mapping.modified.start_line_number, 1));
    };

    let Some(inner_mapping) = inner_changes.iter().rev().find(|m| m.original_range.get_start_position().is_before_or_equal(&pos_in_original)) else {
        let new_line_number = pos_in_original.line_number - mapping.original.start_line_number + mapping.modified.start_line_number;
        return at(Position::new(new_line_number, pos_in_original.column));
    };

    if inner_mapping.original_range.contains_position(&pos_in_original) {
        inner_mapping.modified_range
    } else {
        let l = length_between_positions(inner_mapping.original_range.get_end_position(), pos_in_original);
        at(l.add_to_position(inner_mapping.modified_range.get_end_position()))
    }
}

/// O corpo comum de `derivedDisposable`: `compute` devolve o valor e o que descartar na próxima computação ou quando o derivado
/// ficar sem observadores.
pub fn derived_disposable_with<V: Clone + StrictEquals + 'static>(
    owner: Option<DebugOwner>,
    compute: impl Fn(&dyn IReader) -> (V, Option<DisposableRef>) + 'static,
) -> IObservable<V> {
    let store: Rc<RefCell<Option<Rc<DisposableStore>>>> = Rc::default();
    let store_for_last_observer = store.clone();
    derived_opts(
        DerivedOptions {
            owner,
            on_last_observer_removed: Some(Rc::new(move || {
                let store = store_for_last_observer.borrow_mut().take();
                if let Some(store) = store {
                    store.dispose();
                }
            })),
            ..DerivedOptions::default()
        },
        move |reader| {
            let current = store.borrow().clone();
            let store = match current {
                Some(store) => {
                    store.clear();
                    store
                }
                None => {
                    let store_new = DisposableStore::new();
                    *store.borrow_mut() = Some(store_new.clone());
                    store_new
                }
            };
            let (value, disposable) = compute(reader);
            if let Some(disposable) = disposable {
                store.add(disposable);
            }
            value
        },
    )
}

/// `derivedDisposable(owner, reader => objeto)`: o valor é o próprio objeto descartável (em geral um `Rc`).
pub fn derived_disposable_of<V: IDisposable + Clone + StrictEquals + 'static>(
    owner: Option<DebugOwner>,
    compute: impl Fn(&dyn IReader) -> V + 'static,
) -> IObservable<V> {
    derived_disposable_with(owner, move |reader| {
        let value = compute(reader);
        let disposable: DisposableRef = Rc::new(value.clone());
        (value, Some(disposable))
    })
}

/// `derivedDisposable(owner, reader => objeto | undefined)`.
pub fn derived_optional_disposable_of<V: IDisposable + Clone + StrictEquals + 'static>(
    owner: Option<DebugOwner>,
    compute: impl Fn(&dyn IReader) -> Option<V> + 'static,
) -> IObservable<Option<V>> {
    derived_disposable_with(owner, move |reader| {
        let value = compute(reader);
        let disposable = value.clone().map(|value| -> DisposableRef { Rc::new(value) });
        (value, disposable)
    })
}
