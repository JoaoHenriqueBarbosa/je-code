//! Porte de `src/vs/editor/browser/observableCodeEditor.ts`: a fachada que dá ao `CodeEditorWidget` os observáveis do
//! estado dele (modelo, valor, seleções, cursor, layout, rolagem, foco, `versionId`, `isReadonly`, composição) e os
//! sinais dos eventos de digitação e de colagem, além das operações que o original põe na classe
//! (`transaction`, `forceUpdate`, `getOption`, `setDecorations`, `createOverlayWidget`, `createContentWidget`,
//! `observePosition`, `observeLineOffsetRange`, `isTargetHovered`, `observeLineHeight...`, `getWidthOfLine`,
//! `observeTopForLineNumber`, `observeBottomForLineNumber`).
//!
//! Notas de tradução:
//! - `ObservableCodeEditor.get(editor)` guarda um registro por editor (`_map`) num `thread_local`, chaveado pelo endereço
//!   do `Rc<CodeEditorWidget>`; a entrada sai (e o objeto é descartado) quando o editor dispara `onDidDispose`. O objeto
//!   guarda o editor, então o endereço não é reaproveitado enquanto a entrada existe.
//! - O estado interno que o original deixa em campos privados (`_updateCounter`, `_currentTransaction`, `_model`,
//!   `_versionId`, `_selections`, `_beginUpdate`, `_endUpdate`, `_forceUpdate`) vive num `Core` compartilhado: os ouvintes
//!   dos eventos do editor e os `getTransaction` dos observáveis de evento o capturam, em vez de capturar `this`. O
//!   `try { _beginUpdate() ... } finally { _endUpdate() }` é o guarda `UpdateScope`.
//! - O tipo da mudança (`TChange`) de `versionId` e `selections` é `()`: a mudança "indefinida" do original (o
//!   `_forceUpdate` e o construtor passam `undefined`) é a única que o sistema de observáveis do porte representa
//!   junto da checagem de igualdade que ela dispara. O evento de conteúdo e o de seleção, que o original anexa à
//!   mudança, não chegam ao `IChangeContext`.
//! - `Selection[] | null` e `readonly Position[] | null` são `Option<Rc<Vec<..>>>` (a igualdade dos arrays do original é a
//!   comparação elemento a elemento, em `equalsFn`); um array novo a cada cálculo, onde o original devolve um array
//!   novo (`observeLineHeightsForLineRange`), é `Rc<Vec<f64>>`, que a igualdade estrita compara por identidade como o `===`.
//! - O valor (`value`) usa a igualdade de [`JsString`], que não implementa `StrictEquals`: o `derivedWithSetter` é montado
//!   direto sobre `Derived`, com a comparação de texto.
//! - Os tipos de `IObservableOverlayWidget` e `IObservableContentWidget` são os traits [`IObservableOverlayWidget`] e
//!   [`IObservableContentWidget`]. Os widgets de `editor_browser` não têm `getDomNode()` (a UI é nativa), e por isso
//!   o `domNode` das duas interfaces também não existe.
//! - `getOption(id)` devolve `IObservable<Rc<V>>`: o valor da opção é clonado para dentro de um `Rc`, e a igualdade é a
//!   do valor (`PartialEq`), já que o `===` do original compara a identidade de um objeto que o porte não tem.
//! - As sobrecargas de `observeLineHeightForPosition`, `observeLineHeightForLine` e `observeLineHeightsForLineRange`
//!   (valor ou observável, com ou sem `null`) são os métodos com [`ValueOrObservable`] e os `..._nullable_...`.
//! - `OffsetRange` tem campos inteiros (`i32`), onde o original usa `number`: os deslocamentos de
//!   `observeLineOffsetRange` são arredondados para o pixel inteiro mais próximo.
//!
//! Divergências (fora do escopo, por nome de membro do original):
//! - `domNode` e `getDomNode`: o `CodeEditorWidget` do porte não tem `View` nem DOM (ver o topo de `code_editor_widget`).
//! - O `ResizeObserver` de `_onDidContainerResize` é o [`ObservableCodeEditor::notify_container_resize`]: quem tem o
//!   contêiner (a `View`, ou o host nativo) avisa quando ele muda de tamanho. O restante do mecanismo (zerar os caches
//!   de largura de linha se uma largura zero foi vista, e invalidar `getWidthOfLine`) é o do original.
//! - Os widgets de conteúdo e de overlay registrados pelo editor são o registro de `code_editor_widget`: sem `View`
//!   real, `afterRender` só roda quando quem desenha chama; o `observePosition` fica, até lá, com o ponto inicial `(0, 0)`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::event::any;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::observable::base::{AnyObservable, IObservable, IReader, ISettableObservable, ITransaction, Observable};
use je_base::observable::change_tracker::IChangeTracker;
use je_base::observable::debug_name::{DebugNameOptions, DebugNameSource, DebugOwner};
use je_base::observable::deps::{EqualityComparer, StrictEquals};
use je_base::observable::observables::derived::{Derived, DerivedOptions, NoTracker, derived, derived_opts};
use je_base::observable::observables::observable_from_event::{FromEventOptions, observable_from_event_opts};
use je_base::observable::observables::observable_signal::{ObservableSignal, observable_signal_owned};
use je_base::observable::observables::observable_signal_from_event::observable_signal_from_event_owned;
use je_base::observable::observables::observable_value::{ObservableValue, ObservableValueOptions, observable_value_opts, observable_value_owned};
use je_base::observable::reactions::autorun::{autorun, autorun_opts};
use je_base::observable::transaction::TransactionImpl;
use je_base::observable::{ObservableExt, SettableExt};

use crate::code_editor_widget::{CodeEditorWidget, IPasteEvent, ICodeEditor};
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::layout_info::{EditorLayoutInfo, EditorMinimapLayoutInfo};
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::registry::READ_ONLY;
use crate::config::editor_options::Opt;
use crate::core_2d::point::Point;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_browser::{
    ContentWidgetPosition, ContentWidgetPositionPreference, ContentWidgetRenderedCoordinate, EditorMouseEvent, IContentWidget, IOverlayWidget,
    OverlayWidgetPosition,
};
use crate::line_range::LineRange;
use crate::model::ModelDeltaDecoration;
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::selection::Selection;
use crate::text_model::{TextModel, TextSource};

// #region Igualdade estrita

/// O `===` dos tipos de valor do editor que os observáveis desta fachada carregam: a comparação por valor.
macro_rules! strict_equals_by_value {
    ($($t:ty),* $(,)?) => {$(
        impl StrictEquals for $t {
            fn strict_equals(&self, other: &Self) -> bool {
                self == other
            }
        }
    )*};
}

strict_equals_by_value!(Selection, Point, OffsetRange, EditorLayoutInfo, EditorMinimapLayoutInfo, ContentWidgetPosition, OverlayWidgetPosition);

// #endregion

/// `observableCodeEditor(editor)`: a fachada do editor com os observáveis dele.
pub fn observable_code_editor(editor: &Rc<CodeEditorWidget>) -> Rc<ObservableCodeEditor> {
    ObservableCodeEditor::get(editor)
}

thread_local! {
    /// `ObservableCodeEditor._map`: a fachada de cada editor vivo, pelo endereço do `Rc<CodeEditorWidget>`.
    static REGISTRY: RefCell<HashMap<usize, Rc<ObservableCodeEditor>>> = RefCell::new(HashMap::new());
}

// #region Estado interno

/// `try { this._beginUpdate(); ... } finally { this._endUpdate(); }`: o `end` roda ao sair do escopo.
struct UpdateScope<'a>(&'a Core);

impl Drop for UpdateScope<'_> {
    fn drop(&mut self) {
        self.0.end_update();
    }
}

/// O estado que o original guarda em campos privados e que os ouvintes dos eventos do editor alteram.
struct Core {
    editor: Rc<CodeEditorWidget>,
    /// `_updateCounter`.
    update_counter: Cell<usize>,
    /// `_currentTransaction`.
    current_transaction: RefCell<Option<Rc<TransactionImpl>>>,
    /// `_model`.
    model: ISettableObservable<Option<Rc<TextModel>>>,
    /// `_versionId`.
    version_id: ISettableObservable<Option<i64>>,
    /// `_selections`.
    selections: ISettableObservable<Option<Rc<Vec<Selection>>>>,
}

impl Core {
    /// `_beginUpdate()`.
    fn begin_update(&self) {
        self.update_counter.set(self.update_counter.get() + 1);
        if self.update_counter.get() == 1 {
            // `/** @description Update editor state */`
            *self.current_transaction.borrow_mut() = Some(Rc::new(TransactionImpl::new()));
        }
    }

    /// `_endUpdate()`.
    fn end_update(&self) {
        self.update_counter.set(self.update_counter.get() - 1);
        if self.update_counter.get() == 0 {
            let transaction = self.current_transaction.borrow_mut().take();
            if let Some(transaction) = transaction {
                transaction.finish();
            }
        }
    }

    /// `_beginUpdate()` com o `_endUpdate()` no `Drop`.
    fn scope(&self) -> UpdateScope<'_> {
        self.begin_update();
        UpdateScope(self)
    }

    /// `_currentTransaction`, com o empréstimo do `RefCell` já solto.
    fn current_transaction(&self) -> Option<Rc<TransactionImpl>> {
        self.current_transaction.borrow().clone()
    }

    /// Roda `f` com `this._currentTransaction` no formato que o `set` dos observáveis recebe.
    fn with_transaction<R>(&self, f: impl FnOnce(Option<&dyn ITransaction>) -> R) -> R {
        let transaction = self.current_transaction();
        f(transaction.as_deref().map(|t| t as &dyn ITransaction))
    }

    /// `_forceUpdate()`.
    fn force_update(&self) {
        let _scope = self.scope();
        let model = self.editor.get_model();
        let version_id = model.as_ref().map(|model| model.get_version_id());
        let selections = self.editor.get_selections().map(Rc::new);
        self.with_transaction(|tx| {
            self.model.set(model, tx);
            self.version_id.set(version_id, tx);
            self.selections.set(selections, tx);
        });
    }

    /// O `getTransaction: () => this._currentTransaction` das opções de `observableFromEventOpts`.
    fn from_event_options<T>(self: &Rc<Self>, owner: &DebugOwner, name: &str) -> FromEventOptions<T> {
        let core = Rc::clone(self);
        let get_transaction: Rc<dyn Fn() -> Option<Rc<dyn ITransaction>>> =
            Rc::new(move || core.current_transaction().map(|transaction| transaction as Rc<dyn ITransaction>));
        FromEventOptions { debug: named(owner, name), equals_fn: None, get_transaction: Some(get_transaction) }
    }
}

/// `{ owner: this, debugName }`.
fn named(owner: &DebugOwner, name: &str) -> DebugNameOptions {
    DebugNameOptions { owner: Some(owner.clone()), debug_name: Some(DebugNameSource::from(name)) }
}

/// `{ owner: this, debugName, equalsFn }` de `derivedOpts`.
fn derived_options<T>(owner: &DebugOwner, name: &str, equals_fn: Option<EqualityComparer<T>>) -> DerivedOptions<T> {
    DerivedOptions { owner: Some(owner.clone()), debug_name: Some(DebugNameSource::from(name)), equals_fn, on_last_observer_removed: None }
}

/// `equalsIfDefinedC(arrayEqualsC(equals))`: `null` só é igual a `null`, e os arrays comparam elemento a elemento.
fn optional_array_equals<T: 'static>(equals: fn(&T, &T) -> bool) -> EqualityComparer<Option<Rc<Vec<T>>>> {
    Rc::new(move |a: &Option<Rc<Vec<T>>>, b: &Option<Rc<Vec<T>>>| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| equals(x, y)),
        _ => false,
    })
}

/// `derivedWithSetter(owner, computeFn, setter)` com a igualdade do valor escolhida por quem chama (o
/// `derivedWithSetter` do porte exige `StrictEquals`, que [`JsString`] não tem).
fn derived_with_setter_and_equals<T: Clone + 'static>(
    owner: &DebugOwner,
    name: &str,
    equals: EqualityComparer<T>,
    compute_fn: impl Fn(&dyn IReader) -> T + 'static,
    setter: impl Fn(T, Option<&dyn ITransaction>) + 'static,
) -> ISettableObservable<T> {
    let tracker: Rc<dyn IChangeTracker<()>> = Rc::new(NoTracker);
    let result: Rc<Derived<T, (), ()>> = Derived::new_tracked(
        named(owner, name).into(),
        move |reader, _| compute_fn(reader),
        tracker,
        None,
        equals,
        Some(Rc::new(move |value, tx, _| setter(value, tx))),
    );
    result
}

// #endregion

// #region Widgets observáveis

/// `IObservableOverlayWidget`.
pub trait IObservableOverlayWidget {
    /// `position`.
    fn position(&self) -> IObservable<Option<OverlayWidgetPosition>>;

    /// `minContentWidthInPx`.
    fn min_content_width_in_px(&self) -> IObservable<f64>;

    /// `allowEditorOverflow`.
    fn allow_editor_overflow(&self) -> bool;
}

/// `IObservableContentWidget`.
pub trait IObservableContentWidget {
    /// `position`.
    fn position(&self) -> IObservable<Option<ContentWidgetPosition>>;

    /// `allowEditorOverflow`.
    fn allow_editor_overflow(&self) -> bool;
}

/// O `IOverlayWidget` que `createOverlayWidget` monta sobre o widget observável.
struct ObservableOverlayWidget {
    id: String,
    widget: Rc<dyn IObservableOverlayWidget>,
}

impl IOverlayWidget for ObservableOverlayWidget {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_position(&self) -> Option<OverlayWidgetPosition> {
        self.widget.position().get()
    }

    fn allow_editor_overflow(&self) -> bool {
        self.widget.allow_editor_overflow()
    }

    fn get_min_content_width_in_px(&self) -> Option<f64> {
        Some(self.widget.min_content_width_in_px().get())
    }
}

/// O `IContentWidget` que `createContentWidget` monta sobre o widget observável.
struct ObservableContentWidget {
    id: String,
    widget: Rc<dyn IObservableContentWidget>,
}

impl IContentWidget for ObservableContentWidget {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_position(&self) -> Option<ContentWidgetPosition> {
        self.widget.position().get()
    }

    fn allow_editor_overflow(&self) -> bool {
        self.widget.allow_editor_overflow()
    }
}

/// O widget de conteúdo de `observePosition`: sem representação visual, só mede onde a posição caiu.
struct PositionWidget {
    id: String,
    position: IObservable<Option<Position>>,
    /// `pos`.
    pos: RefCell<Option<Position>>,
    /// `result`.
    result: ISettableObservable<Option<Point>>,
    /// `this._model`.
    model: ISettableObservable<Option<Rc<TextModel>>>,
    scroll_top: IObservable<f64>,
    editor: Weak<CodeEditorWidget>,
}

impl IContentWidget for PositionWidget {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_position(&self) -> Option<ContentWidgetPosition> {
        self.pos.borrow().as_ref()?;
        Some(ContentWidgetPosition {
            position: self.position.get(),
            secondary_position: None,
            preference: vec![ContentWidgetPositionPreference::Exact],
            position_affinity: None,
        })
    }

    fn use_display_none(&self) -> bool {
        true
    }

    fn after_render(&self, _position: Option<ContentWidgetPositionPreference>, coordinate: Option<ContentWidgetRenderedCoordinate>) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let pos = *self.pos.borrow();
        let model = self.model.get();
        if let (Some(model), Some(pos)) = (&model, pos) {
            let line_count = CursorSimpleModel::get_line_count(&**model);
            if pos.line_number > line_count {
                // the position is after the last line
                let top = editor.get_bottom_for_line_number(line_count, None) - self.scroll_top.get();
                self.result.set(Some(Point::new(0.0, top)), None);
                return;
            }
        }
        self.result.set(coordinate.map(|coordinate| Point::new(coordinate.left, coordinate.top)), None);
    }
}

// #endregion

/// `T | IObservable<T>`: o argumento que as sobrecargas de `observeLineHeight...` aceitam nas duas formas.
#[derive(Clone)]
pub enum ValueOrObservable<T: Clone + 'static> {
    Value(T),
    Observable(IObservable<T>),
}

impl<T: Clone + 'static> ValueOrObservable<T> {
    /// `value instanceof X ? value : observable.read(reader)`.
    fn read(&self, reader: &dyn IReader) -> T {
        match self {
            ValueOrObservable::Value(value) => value.clone(),
            ValueOrObservable::Observable(observable) => observable.read(reader),
        }
    }
}

/// Lê a altura de linha de uma posição: o `this.getOption(EditorOption.lineHeight).read(reader)` seguido de
/// `this.editor.getLineHeightForPosition(pos)` que as sobrecargas de `observeLineHeight...` repetem.
#[derive(Clone)]
struct LineHeightReader {
    editor: Rc<CodeEditorWidget>,
    line_height_option: IObservable<Rc<f64>>,
}

impl LineHeightReader {
    fn height(&self, position: Position, reader: &dyn IReader) -> f64 {
        self.line_height_option.read(reader);
        self.editor.get_line_height_for_position(position)
    }
}

/// `ObservableCodeEditor`.
pub struct ObservableCodeEditor {
    pub editor: Rc<CodeEditorWidget>,
    base: Rc<Disposable>,
    owner: DebugOwner,
    core: Rc<Core>,

    pub model: IObservable<Option<Rc<TextModel>>>,
    pub is_readonly: IObservable<bool>,
    pub version_id: IObservable<Option<i64>>,
    pub selections: IObservable<Option<Rc<Vec<Selection>>>>,
    pub positions: IObservable<Option<Rc<Vec<Position>>>>,
    pub is_focused: IObservable<bool>,
    pub is_text_focused: IObservable<bool>,
    pub in_composition: IObservable<bool>,
    pub value: ISettableObservable<JsString>,
    pub value_is_empty: IObservable<bool>,
    pub cursor_selection: IObservable<Option<Selection>>,
    pub cursor_position: IObservable<Option<Position>>,
    pub cursor_line_number: IObservable<Option<i32>>,
    pub on_did_type: Rc<ObservableSignal<JsString>>,
    pub on_did_paste: Rc<ObservableSignal<IPasteEvent>>,
    pub scroll_top: IObservable<f64>,
    pub scroll_left: IObservable<f64>,
    pub layout_info: IObservable<EditorLayoutInfo>,
    pub layout_info_content_left: IObservable<f64>,
    pub layout_info_decorations_left: IObservable<f64>,
    pub layout_info_width: IObservable<f64>,
    pub layout_info_height: IObservable<f64>,
    pub layout_info_minimap: IObservable<EditorMinimapLayoutInfo>,
    pub layout_info_vertical_scrollbar_width: IObservable<f64>,
    pub content_width: IObservable<f64>,
    pub content_height: IObservable<f64>,
    pub opened_peek_widgets: Rc<ObservableValue<i32, ()>>,

    on_did_change_view_zones: IObservable<()>,
    on_did_hidden_areas_changed: IObservable<()>,
    on_did_line_height_changed: IObservable<()>,
    /// `_onDidContainerResize`: o sinal que [`ObservableCodeEditor::notify_container_resize`] dispara.
    on_did_container_resize: Rc<ObservableSignal<()>>,
    /// `_sawZeroLineWidth`.
    saw_zero_line_width: Cell<bool>,
    /// `_widgetCounter`.
    widget_counter: Cell<u32>,
}

impl ObservableCodeEditor {
    /// `ObservableCodeEditor.get(editor)`. Make sure that editor is not disposed yet!
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Rc<ObservableCodeEditor> {
        let key = Rc::as_ptr(editor).cast::<()>() as usize;
        if let Some(existing) = REGISTRY.with(|map| map.borrow().get(&key).cloned()) {
            return existing;
        }

        let result = ObservableCodeEditor::new(editor);
        REGISTRY.with(|map| map.borrow_mut().insert(key, Rc::clone(&result)));
        let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let listener_for_dispose = Rc::clone(&listener);
        let subscription = editor.on_did_dispose().listen(move |_| {
            let item = REGISTRY.with(|map| map.borrow_mut().remove(&key));
            if let Some(item) = item {
                item.dispose();
                let subscription = listener_for_dispose.borrow_mut().take();
                if let Some(subscription) = subscription {
                    subscription.dispose();
                }
            }
        });
        *listener.borrow_mut() = Some(subscription);
        result
    }

    /// O construtor (privado no original).
    fn new(editor: &Rc<CodeEditorWidget>) -> Rc<ObservableCodeEditor> {
        let base = Disposable::new();
        let owner = DebugOwner::new("ObservableCodeEditor");

        let model_value = observable_value_owned::<Option<Rc<TextModel>>, ()>(owner.clone(), editor.get_model());
        let model_settable: ISettableObservable<Option<Rc<TextModel>>> = model_value;
        let version_id_value = observable_value_opts::<Option<i64>, ()>(
            ObservableValueOptions { debug: named(&owner, "versionId"), equals_fn: None, lazy: true },
            editor.get_model().map(|model| model.get_version_id()),
        );
        let selections_value = observable_value_opts::<Option<Rc<Vec<Selection>>>, ()>(
            ObservableValueOptions {
                debug: named(&owner, "selections"),
                equals_fn: Some(optional_array_equals(|a: &Selection, b: &Selection| a == b)),
                lazy: true,
            },
            editor.get_selections().map(Rc::new),
        );
        let core = Rc::new(Core {
            editor: Rc::clone(editor),
            update_counter: Cell::new(0),
            current_transaction: RefCell::new(None),
            model: model_settable,
            version_id: version_id_value,
            selections: selections_value,
        });

        let model = core.model.to_observable();
        let version_id = core.version_id.to_observable();
        let selections = core.selections.to_observable();

        let is_readonly = {
            let editor = Rc::clone(editor);
            observable_from_event_opts(core.from_event_options(&owner, "isReadonly"), editor.on_did_change_configuration(), move |_| {
                *editor.get_option(READ_ONLY)
            })
        };

        let positions = {
            let selections = Rc::clone(&selections);
            let equals: EqualityComparer<Option<Rc<Vec<Position>>>> = optional_array_equals(|a: &Position, b: &Position| a == b);
            derived_opts(derived_options(&owner, "positions", Some(equals)), move |reader| {
                selections
                    .read(reader)
                    .map(|selections| Rc::new(selections.iter().map(|s| Position::new(s.start_line_number, s.start_column)).collect::<Vec<_>>()))
            })
        };

        let is_focused = {
            let editor = Rc::clone(editor);
            let events = any(vec![editor.on_did_focus_editor_widget(), editor.on_did_blur_editor_widget()]);
            observable_from_event_opts(core.from_event_options(&owner, "isFocused"), events, move |_| editor.has_widget_focus())
        };
        let is_text_focused = {
            let editor = Rc::clone(editor);
            let events = any(vec![editor.on_did_focus_editor_text(), editor.on_did_blur_editor_text()]);
            observable_from_event_opts(core.from_event_options(&owner, "isTextFocused"), events, move |_| editor.has_text_focus())
        };
        let in_composition = {
            let editor = Rc::clone(editor);
            let events = any(vec![editor.on_did_composition_start(), editor.on_did_composition_end()]);
            observable_from_event_opts(core.from_event_options(&owner, "inComposition"), events, move |_| editor.in_composition())
        };

        let value = {
            let (version_id, model) = (Rc::clone(&version_id), Rc::clone(&model));
            let model_for_setter = Rc::clone(&model);
            derived_with_setter_and_equals(
                &owner,
                "value",
                Rc::new(|a: &JsString, b: &JsString| a == b),
                move |reader| {
                    version_id.read(reader);
                    model.read(reader).map(|model| model.get_value()).unwrap_or_default()
                },
                move |value, _tx| {
                    if let Some(model) = model_for_setter.get() {
                        if value != model.get_value() {
                            model.set_value(TextSource::Text(value));
                        }
                    }
                },
            )
        };
        let value_is_empty = {
            let (version_id, editor) = (Rc::clone(&version_id), Rc::clone(editor));
            derived_opts(derived_options(&owner, "valueIsEmpty", None), move |reader| {
                version_id.read(reader);
                editor.get_model().is_some_and(|model| model.get_value_length() == 0)
            })
        };
        let cursor_selection = {
            let selections = Rc::clone(&selections);
            derived_opts(derived_options(&owner, "cursorSelection", None), move |reader| {
                selections.read(reader).and_then(|selections| selections.first().copied())
            })
        };
        let cursor_position = {
            let selections = Rc::clone(&selections);
            derived_opts(derived_options(&owner, "cursorPosition", None), move |reader| {
                selections.read(reader).and_then(|selections| selections.first().map(Selection::get_position))
            })
        };
        let cursor_line_number = {
            let cursor_position = Rc::clone(&cursor_position);
            derived_opts(derived_options(&owner, "cursorLineNumber", None), move |reader| {
                cursor_position.read(reader).map(|position| position.line_number)
            })
        };
        let on_did_type = observable_signal_owned::<JsString>(owner.clone());
        let on_did_paste = observable_signal_owned::<IPasteEvent>(owner.clone());

        let scroll_top = {
            let editor = Rc::clone(editor);
            observable_from_event_opts(core.from_event_options(&owner, "scrollTop"), editor.on_did_scroll_change(), move |_| editor.get_scroll_top())
        };
        let scroll_left = {
            let editor = Rc::clone(editor);
            observable_from_event_opts(core.from_event_options(&owner, "scrollLeft"), editor.on_did_scroll_change(), move |_| editor.get_scroll_left())
        };
        let layout_info = {
            let editor = Rc::clone(editor);
            observable_from_event_opts(core.from_event_options(&owner, "layoutInfo"), editor.on_did_layout_change(), move |_| editor.get_layout_info())
        };
        let layout_info_content_left = layout_info.map(|l, _| l.content_left);
        let layout_info_decorations_left = layout_info.map(|l, _| l.decorations_left);
        let layout_info_width = layout_info.map(|l, _| l.width);
        let layout_info_height = layout_info.map(|l, _| l.height);
        let layout_info_minimap = layout_info.map(|l, _| l.minimap);
        let layout_info_vertical_scrollbar_width = layout_info.map(|l, _| l.vertical_scrollbar_width);
        let content_width = {
            let editor = Rc::clone(editor);
            observable_from_event_opts(
                core.from_event_options(&owner, "contentWidth"),
                editor.on_did_content_size_change(),
                move |_| editor.get_content_width(),
            )
        };
        let content_height = {
            let editor = Rc::clone(editor);
            observable_from_event_opts(
                core.from_event_options(&owner, "contentHeight"),
                editor.on_did_content_size_change(),
                move |_| editor.get_content_height(),
            )
        };
        let on_did_change_view_zones = observable_signal_from_event_owned(owner.clone(), editor.on_did_change_view_zones());
        let on_did_hidden_areas_changed = observable_signal_from_event_owned(owner.clone(), editor.on_did_change_hidden_areas());
        let on_did_line_height_changed = observable_signal_from_event_owned(owner.clone(), editor.on_did_change_line_height());

        let opened_peek_widgets = observable_value_owned::<i32, ()>(owner.clone(), 0);

        base.register(editor.on_begin_update().listen({
            let core = Rc::clone(&core);
            move |_| core.begin_update()
        }));
        base.register(editor.on_end_update().listen({
            let core = Rc::clone(&core);
            move |_| core.end_update()
        }));

        base.register(editor.on_did_change_model().listen({
            let core = Rc::clone(&core);
            move |_| {
                let _scope = core.scope();
                let model = core.editor.get_model();
                core.with_transaction(|tx| core.model.set(model, tx));
                core.force_update();
            }
        }));

        base.register(editor.on_did_type().listen({
            let (core, on_did_type) = (Rc::clone(&core), Rc::clone(&on_did_type));
            move |e| {
                let _scope = core.scope();
                core.force_update();
                core.with_transaction(|tx| on_did_type.trigger(tx, e.clone()));
            }
        }));

        base.register(editor.on_did_paste().listen({
            let (core, on_did_paste) = (Rc::clone(&core), Rc::clone(&on_did_paste));
            move |e| {
                let _scope = core.scope();
                core.force_update();
                core.with_transaction(|tx| on_did_paste.trigger(tx, e.clone()));
            }
        }));

        base.register(editor.on_did_change_model_content().listen({
            let core = Rc::clone(&core);
            move |_| {
                let _scope = core.scope();
                let version_id = core.editor.get_model().map(|model| model.get_version_id());
                core.with_transaction(|tx| core.version_id.set(version_id, tx));
                core.force_update();
            }
        }));

        base.register(editor.on_did_change_cursor_selection().listen({
            let core = Rc::clone(&core);
            move |_| {
                let _scope = core.scope();
                let selections = core.editor.get_selections().map(Rc::new);
                core.with_transaction(|tx| core.selections.set(selections, tx));
                core.force_update();
            }
        }));

        Rc::new(ObservableCodeEditor {
            editor: Rc::clone(editor),
            base,
            owner: owner.clone(),
            core,
            model,
            is_readonly,
            version_id,
            selections,
            positions,
            is_focused,
            is_text_focused,
            in_composition,
            value,
            value_is_empty,
            cursor_selection,
            cursor_position,
            cursor_line_number,
            on_did_type,
            on_did_paste,
            scroll_top,
            scroll_left,
            layout_info,
            layout_info_content_left,
            layout_info_decorations_left,
            layout_info_width,
            layout_info_height,
            layout_info_minimap,
            layout_info_vertical_scrollbar_width,
            content_width,
            content_height,
            opened_peek_widgets,
            on_did_change_view_zones,
            on_did_hidden_areas_changed,
            on_did_line_height_changed,
            on_did_container_resize: observable_signal_owned::<()>(owner),
            saw_zero_line_width: Cell::new(false),
            widget_counter: Cell::new(0),
        })
    }

    // #region Transações

    /// Batches the transactions started by observableFromEvent.
    ///
    /// If the callback causes the editor to fire an event that updates an observable value backed by
    /// observableFromEvent (such as scrollTop etc.), then all such updates will be part of the same transaction.
    pub fn transaction<T>(&self, cb: impl FnOnce(&dyn ITransaction) -> T) -> T {
        let _scope = self.core.scope();
        let transaction = self.core.current_transaction().expect("a transação existe enquanto o escopo de atualização está aberto");
        cb(&*transaction)
    }

    /// `forceUpdate()` sem callback.
    pub fn force_update(&self) {
        self.force_update_with(|_| ());
    }

    /// `forceUpdate(cb)`: atualiza o estado e roda `cb` na mesma transação.
    pub fn force_update_with<T>(&self, cb: impl FnOnce(&dyn ITransaction) -> T) -> T {
        let _scope = self.core.scope();
        self.core.force_update();
        let transaction = self.core.current_transaction().expect("a transação existe enquanto o escopo de atualização está aberto");
        cb(&*transaction)
    }

    // #endregion

    /// `getOption(id)`: o valor calculado da opção, atualizado quando a configuração muda aquela opção.
    pub fn get_option<V: Clone + PartialEq + 'static>(&self, option: Opt<V>) -> IObservable<Rc<V>> {
        let id = option.id();
        let changed = self.editor.on_did_change_configuration().filter(move |e| e.has_changed(id), None);
        let editor = Rc::clone(&self.editor);
        observable_from_event_opts(
            FromEventOptions {
                debug: DebugNameOptions { owner: Some(self.owner.clone()), debug_name: None },
                equals_fn: Some(Rc::new(|a: &Rc<V>, b: &Rc<V>| **a == **b) as EqualityComparer<Rc<V>>),
                get_transaction: None,
            },
            changed,
            move |_| Rc::new(editor.get_option(option).clone()),
        )
    }

    /// `setDecorations(decorations)`: aplica as decorações observadas enquanto o resultado não for descartado.
    pub fn set_decorations(&self, decorations: &IObservable<Rc<Vec<ModelDeltaDecoration>>>) -> DisposableRef {
        let d = DisposableStore::new();
        let collection = Rc::new(self.editor.create_decorations_collection(None));
        let name = format!("Apply decorations from {}", decorations.debug_name());
        d.add(autorun_opts(
            DebugNameOptions { owner: Some(self.owner.clone()), debug_name: Some(DebugNameSource::from(name.as_str())) },
            {
                let (decorations, collection) = (Rc::clone(decorations), Rc::clone(&collection));
                move |reader| {
                    let decorations = decorations.read(reader);
                    collection.set(&decorations);
                }
            },
        ));
        d.add(to_disposable(move || collection.clear()));
        d
    }

    /// `this._widgetCounter++`.
    fn next_widget_id(&self) -> u32 {
        let id = self.widget_counter.get();
        self.widget_counter.set(id + 1);
        id
    }

    /// `createOverlayWidget(widget)`.
    pub fn create_overlay_widget(&self, widget: Rc<dyn IObservableOverlayWidget>) -> DisposableRef {
        let overlay_widget_id = format!("observableOverlayWidget{}", self.next_widget_id());
        let w: Rc<dyn IOverlayWidget> = Rc::new(ObservableOverlayWidget { id: overlay_widget_id, widget: Rc::clone(&widget) });
        self.editor.add_overlay_widget(Rc::clone(&w));
        let d = {
            let (editor, w) = (Rc::clone(&self.editor), Rc::clone(&w));
            autorun(move |reader| {
                widget.position().read(reader);
                widget.min_content_width_in_px().read(reader);
                editor.layout_overlay_widget(&w);
            })
        };
        let editor = Rc::clone(&self.editor);
        to_disposable(move || {
            d.dispose();
            editor.remove_overlay_widget(&w);
        })
    }

    /// `createContentWidget(widget)`.
    pub fn create_content_widget(&self, widget: Rc<dyn IObservableContentWidget>) -> DisposableRef {
        let content_widget_id = format!("observableContentWidget{}", self.next_widget_id());
        let w: Rc<dyn IContentWidget> = Rc::new(ObservableContentWidget { id: content_widget_id, widget: Rc::clone(&widget) });
        self.editor.add_content_widget(Rc::clone(&w));
        let d = {
            let (editor, w) = (Rc::clone(&self.editor), Rc::clone(&w));
            autorun(move |reader| {
                widget.position().read(reader);
                editor.layout_content_widget(&w);
            })
        };
        let editor = Rc::clone(&self.editor);
        to_disposable(move || {
            d.dispose();
            editor.remove_content_widget(&w);
        })
    }

    /// `observeLineOffsetRange(lineRange, store)`.
    pub fn observe_line_offset_range(&self, line_range: &IObservable<LineRange>, store: &DisposableStore) -> IObservable<OffsetRange> {
        let start = self.observe_position(&line_range.map(|r, _| Some(Position::new(r.start_line_number, 1))), store);
        let end = self.observe_position(&line_range.map(|r, _| Some(Position::new(r.end_line_number_exclusive + 1, 1))), store);

        let (line_range, model, scroll_top, editor) =
            (Rc::clone(line_range), Rc::clone(&self.model), Rc::clone(&self.scroll_top), Rc::clone(&self.editor));
        derived(move |reader| {
            start.read(reader);
            end.read(reader);
            let range = line_range.read(reader);
            let line_count = model.read(reader).map(|model| CursorSimpleModel::get_line_count(&*model));
            let s = match line_count {
                Some(line_count) if range.start_line_number > line_count => editor.get_bottom_for_line_number(line_count, None),
                _ => editor.get_top_for_line_number(range.start_line_number, None),
            } - scroll_top.read(reader);
            let e = if range.is_empty() {
                s
            } else {
                editor.get_bottom_for_line_number(range.end_line_number_exclusive - 1, None) - scroll_top.read(reader)
            };
            OffsetRange::new(s.round() as i32, e.round() as i32)
        })
    }

    /// Uses an approximation if the exact position cannot be determined.
    pub fn get_left_of_position(&self, position: Position, reader: Option<&dyn IReader>) -> f64 {
        self.layout_info.read_opt(reader);
        self.value.read_opt(reader);

        let offset = self.editor.get_offset_for_column(position.line_number, position.column);
        if offset == -1.0 {
            // approximation
            let typical_halfwidth_character_width = self.editor.get_option(FONT_INFO).typical_halfwidth_character_width;
            return f64::from(position.column) * typical_halfwidth_character_width;
        }
        offset
    }

    /// `observePosition(position, store)`: o canto superior esquerdo da posição, medido por um widget de conteúdo sem
    /// representação visual que fica no editor até o `store` ser descartado.
    pub fn observe_position(&self, position: &IObservable<Option<Position>>, store: &DisposableStore) -> IObservable<Option<Point>> {
        let result = observable_value_opts::<Option<Point>, ()>(
            ObservableValueOptions {
                debug: DebugNameOptions { owner: Some(self.owner.clone()), debug_name: Some(DebugNameSource::from("topLeftOfPosition")) },
                equals_fn: None,
                lazy: false,
            },
            Some(Point::new(0.0, 0.0)),
        );
        let content_widget_id = format!("observablePositionWidget{}", self.next_widget_id());
        let widget = Rc::new(PositionWidget {
            id: content_widget_id,
            position: Rc::clone(position),
            pos: RefCell::new(position.get()),
            result: Rc::clone(&result),
            model: Rc::clone(&self.core.model),
            scroll_top: Rc::clone(&self.scroll_top),
            editor: Rc::downgrade(&self.editor),
        });
        let w: Rc<dyn IContentWidget> = widget.clone();
        self.editor.add_content_widget(Rc::clone(&w));
        store.add(autorun({
            let (widget, editor, w) = (Rc::clone(&widget), Rc::clone(&self.editor), Rc::clone(&w));
            move |reader| {
                *widget.pos.borrow_mut() = widget.position.read(reader);
                editor.layout_content_widget(&w);
            }
        }));
        let editor = Rc::clone(&self.editor);
        store.add(to_disposable(move || {
            editor.remove_content_widget(&w);
        }));
        result.to_observable()
    }

    /// `isTargetHovered(predicate, store)`.
    pub fn is_target_hovered(&self, predicate: impl Fn(&EditorMouseEvent) -> bool + 'static, store: &DisposableStore) -> IObservable<bool> {
        let is_hovered = observable_value_opts::<bool, ()>(
            ObservableValueOptions { debug: DebugNameOptions::named("isInjectedTextHovered"), equals_fn: None, lazy: false },
            false,
        );
        store.add(self.editor.on_mouse_move().listen({
            let is_hovered = Rc::clone(&is_hovered);
            move |e| is_hovered.set(predicate(e), None)
        }));

        store.add(self.editor.on_mouse_leave().listen({
            let is_hovered = Rc::clone(&is_hovered);
            move |_| is_hovered.set(false, None)
        }));
        is_hovered.to_observable()
    }

    // #region Alturas de linha

    /// O leitor de altura de linha que as sobrecargas de `observeLineHeight...` compartilham.
    fn line_height_reader(&self) -> LineHeightReader {
        LineHeightReader { editor: Rc::clone(&self.editor), line_height_option: self.get_option(LINE_HEIGHT) }
    }

    /// `observeLineHeightForPosition(position)`, com a posição fixa ou observável (sem `null`).
    pub fn observe_line_height_for_position(&self, position: ValueOrObservable<Position>) -> IObservable<f64> {
        let heights = self.line_height_reader();
        derived(move |reader| heights.height(position.read(reader), reader))
    }

    /// `observeLineHeightForPosition(position)` com um observável que pode ser `null`: o resultado também.
    pub fn observe_line_height_for_nullable_position(&self, position: &IObservable<Option<Position>>) -> IObservable<Option<f64>> {
        let (heights, position) = (self.line_height_reader(), Rc::clone(position));
        derived(move |reader| {
            let position = position.read(reader)?;
            Some(heights.height(position, reader))
        })
    }

    /// `observeLineHeightForLine(lineNumber)`, com a linha fixa ou observável (sem `null`).
    pub fn observe_line_height_for_line(&self, line_number: ValueOrObservable<i32>) -> IObservable<f64> {
        match line_number {
            ValueOrObservable::Value(line_number) => self.observe_line_height_for_position(ValueOrObservable::Value(Position::new(line_number, 1))),
            ValueOrObservable::Observable(line_number) => {
                let heights = self.line_height_reader();
                derived(move |reader| heights.height(Position::new(line_number.read(reader), 1), reader))
            }
        }
    }

    /// `observeLineHeightForLine(lineNumber)` com um observável que pode ser `null`: o resultado também.
    pub fn observe_line_height_for_nullable_line(&self, line_number: &IObservable<Option<i32>>) -> IObservable<Option<f64>> {
        let (heights, line_number) = (self.line_height_reader(), Rc::clone(line_number));
        derived(move |reader| {
            let line = line_number.read(reader)?;
            Some(heights.height(Position::new(line, 1), reader))
        })
    }

    /// `observeLineHeightsForLineRange(lineNumber)`: a altura de cada linha do intervalo.
    pub fn observe_line_heights_for_line_range(&self, line_range: ValueOrObservable<LineRange>) -> IObservable<Rc<Vec<f64>>> {
        let heights = self.line_height_reader();
        derived(move |reader| {
            let range = line_range.read(reader);
            Rc::new((range.start_line_number..range.end_line_number_exclusive).map(|line| heights.height(Position::new(line, 1), reader)).collect::<Vec<_>>())
        })
    }

    // #endregion

    // #region Largura e posição vertical das linhas

    /// `_onDidContainerResize` (o `ResizeObserver` do original): quem tem o contêiner avisa que ele mudou de tamanho. Se
    /// uma largura de linha zero foi vista antes, o editor estava oculto e agora ficou visível, então os caches de
    /// largura de linha são zerados.
    pub fn notify_container_resize(&self) {
        // If we previously saw a 0 width, the editor was likely hidden.
        // Now that it resized (became visible), flush the cached widths.
        if self.saw_zero_line_width.replace(false) {
            self.editor.reset_line_width_caches();
        }
        self.core.with_transaction(|tx| self.on_did_container_resize.trigger(tx, ()));
    }

    /// Get the width of a line in pixels.
    /// Reading the returned value depends on layoutInfo, value, scrollTop, and container resize events.
    /// The container resize dependency ensures correct values when the editor becomes visible after being hidden.
    pub fn get_width_of_line(&self, line_number: i32, reader: Option<&dyn IReader>) -> f64 {
        self.layout_info.read_opt(reader);
        self.value.read_opt(reader);
        self.scroll_top.read_opt(reader);
        let width = self.editor.get_width_of_line(line_number);
        self.on_did_container_resize.read_opt(reader);
        if width == 0.0 {
            self.saw_zero_line_width.set(true);
        }
        width
    }

    /// As dependências compartilhadas de `observeTopForLineNumber` e `observeBottomForLineNumber`.
    fn observe_line_vertical_offset(&self, offset: impl Fn(&CodeEditorWidget) -> f64 + 'static) -> IObservable<f64> {
        let (layout_info, view_zones, hidden_areas, line_height, version_id, editor) = (
            Rc::clone(&self.layout_info),
            Rc::clone(&self.on_did_change_view_zones),
            Rc::clone(&self.on_did_hidden_areas_changed),
            Rc::clone(&self.on_did_line_height_changed),
            Rc::clone(&self.version_id),
            Rc::clone(&self.editor),
        );
        derived(move |reader| {
            layout_info.read(reader);
            view_zones.read(reader);
            hidden_areas.read(reader);
            line_height.read(reader);
            version_id.read(reader);
            offset(&editor)
        })
    }

    /// Get the vertical position (top offset) for the line's bottom w.r.t. to the first line.
    pub fn observe_top_for_line_number(&self, line_number: i32) -> IObservable<f64> {
        self.observe_line_vertical_offset(move |editor| editor.get_top_for_line_number(line_number, None))
    }

    /// Get the vertical position (top offset) for the line's bottom w.r.t. to the first line.
    pub fn observe_bottom_for_line_number(&self, line_number: i32) -> IObservable<f64> {
        self.observe_line_vertical_offset(move |editor| editor.get_bottom_for_line_number(line_number, None))
    }

    // #endregion
}

impl IDisposable for ObservableCodeEditor {
    fn dispose(&self) {
        self.base.dispose();
    }
}
