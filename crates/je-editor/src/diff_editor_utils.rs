//! Utilitários do editor de diff (`src/vs/editor/browser/widget/diffEditor/utils.ts`) que os view models, os componentes e o widget
//! usam: `filterWithPrevious`, `applyObservableDecorations`, `ObservableElementSizeObserver`, `IObservableViewZone`,
//! `applyViewZones` e `translatePosition`. O `RefCounted` do mesmo arquivo vive em `je_base::lifecycle` ao lado do
//! `RefCountedDisposable`. O restante do arquivo (`animatedObservable`, `ManagedOverlayWidget`, `PlaceholderViewZone`,
//! `ViewZoneOverlayWidget`, `deepMerge`...) pertence às features do editor de diff (fatia 3) e entra com elas.
//!
//! Divergências:
//! - **`ObservableElementSizeObserver`** recebe o `Option<Rc<dyn ReferenceDomElement>>` do [`ElementSizeObserver`] no lugar do
//!   `HTMLElement | null`. O `observe(dimension)` do original só repassa ao `ElementSizeObserver` (e uma função de repasse não
//!   existe neste projeto), então o observador interno é o campo público
//!   [`ObservableElementSizeObserver::element_size_observer`] e o widget chama `observe` nele.
//! - **`applyStyle` e `CSSStyle`** não são portados: escrevem no `style` de um `HTMLElement`, e o estilo do editor de diff é o dado
//!   que o pintor lê (ver `DiffEditorElementsState` em `diff_editor::diff_editor_widget`).
//! - **`IObservableViewZone`** é o trait [`IObservableViewZone`] sobre o [`IViewZone`] nativo; `onChange?: IObservable<unknown>` é
//!   `Option<ObservableRef>` (observável apagado) e `setZoneId?` tem corpo vazio por padrão. A zona se identifica pelo endereço do
//!   `Rc` (a chave dos `Map` do original).
//! - **`derivedDisposable`** de `je_base::observable` devolve um observável apagado (`Option<DisposableRef>`); o widget lê o objeto
//!   criado (a feature) pelo tipo, então [`derived_disposable_with`], [`derived_disposable_of`] e
//!   [`derived_optional_disposable_of`] têm a mesma semântica (descartam o objeto anterior a cada recálculo e quando o derivado
//!   fica sem observadores) mas guardam o valor tipado.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::observable::base::ObservableRef;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::change_tracker::FnChangeTracker;
use je_base::observable::{
    AnyObservable, DebugNameOptions, DebugOwner, DerivedOptions, IObservable, IReader, Observable, SettableExt, StrictEquals,
    autorun_handle_changes, autorun_opts, autorun_with_store, derived_opts, transaction,
};

use crate::code_editor_widget::CodeEditorWidget;
use crate::config::element_size_observer::{ElementSizeObserver, ReferenceDomElement};
use crate::core_2d::dimension::Dimension;
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::editor_browser::{IViewZone, IViewZoneChangeAccessor};
use crate::model::ModelDeltaDecoration;
use crate::position::Position;
use crate::position_to_offset::TextLength;
use crate::range::Range;

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
