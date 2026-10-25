//! Porte de `src/vs/editor/browser/widget/diffEditor/diffEditorWidget.ts`: o `DiffEditorWidget`, o editor de diff do Monaco (os dois
//! editores lado a lado ou em linha, o modelo, as opções, a navegação entre as diferenças e o estado que o pintor lê).
//!
//! O widget implementa [`DelegatingEditor`] (a base `DelegatingEditor` do original), [`EmbeddedDiffEditorBase`] (o que o
//! `EmbeddedDiffEditorWidget` herda dele) e [`IDiffEditorWidget`] (o recorte que o editor de vários arquivos lê).
//!
//! Divergências:
//! - **DOM vira dado.** `elements` (o `div.monaco-diff-editor.side-by-side` e os filhos `original`, `modified` e
//!   `accessibleDiffViewer`) é o [`DiffEditorElementsState`] ([`DiffEditorWidget::elements_state`]): a altura da raiz, a classe
//!   `side-by-side`, e a posição, a largura e a visibilidade de cada lado, que o pintor lê. O `appendChild`/`remove` da raiz no
//!   contêiner e o `applyStyle(visibility)` não existem como chamadas: o autorun do `applyStyle` escreve direto no estado. O
//!   `domElement` é o [`NodeId`] da árvore de `je-ui` ([`DiffEditorWidgetArgs::dom_node`]) onde o sash entra; `getDomNode` e
//!   `getContainerDomNode` são [`DiffEditorWidget::get_dom_node`] (a raiz é o próprio nó do contêiner).
//! - **Os ouvintes da janela viram métodos.** Os `addDisposableListener(targetWindow, 'resize' | 'mousedown' | 'mouseup' |
//!   'touchstart' | 'touchend' | 'touchcancel')` do redimensionamento suave são [`DiffEditorWidget::on_window_resize`],
//!   [`DiffEditorWidget::on_pointer_down`] e [`DiffEditorWidget::on_pointer_up`], que o dono da janela chama quando o evento
//!   nativo chega (`mousedown`/`touchstart` são o `on_pointer_down`; `mouseup`/`touchend`/`touchcancel` são o `on_pointer_up`).
//!   O `ResizeObserver` do `automaticLayout` é o [`ReferenceDomElement`] opcional de [`DiffEditorWidgetArgs`].
//! - **Serviços.** O construtor do original injeta `IContextKeyService`, `IInstantiationService`, `ICodeEditorService`,
//!   `IAccessibilitySignalService` e `IEditorProgressService`. Os quatro primeiros com identificador de serviço entram por
//!   [`Injectable`]; o `ICodeEditorService` (só `willCreateDiffEditor`, `addDiffEditor` e `removeDiffEditor`) é o
//!   [`IDiffEditorRegistry`] opcional dos argumentos, porque o registro de editores do porte (`ICodeEditorRegistry`) ainda não os
//!   tem. O `target` do `createScoped(this._domElement)` é o [`DiffEditorWidgetArgs::context_key_target`]. O filho de
//!   `IInstantiationService` com o serviço de chaves de contexto escopado é criado como no original.
//! - **`_createInnerEditor`** (`protected`, a criação do `CodeEditorWidget` pelo serviço de instanciação) é a
//!   [`CreateInnerEditor`] dos argumentos.
//! - **As features e o `AccessibleDiffViewer`** (fatia 3) e o `DiffEditorViewZones` são criados como no original, na mesma ordem e com
//!   os mesmos argumentos, pelo [`DiffEditorFeatures`] dos argumentos (ver [`crate::diff_editor::feature_contracts`]). O
//!   `DiffEditorDecorations` (fatia 1) é criado direto.
//! - **`derivedDisposable(...).recomputeInitiallyAndOnChange(store)`** com leitura tipada do resultado: ver
//!   [`crate::diff_editor_utils::derived_disposable_of`]. As referências do construtor a campos atribuídos mais adiante (`this._sash`,
//!   `this._gutter`, `this._overviewRulerPart` dentro do `_layoutInfo`) são `OnceCell`s preenchidas na ordem do original; lidas antes
//!   de existirem valem `undefined`, que é o que o original lê.
//! - **`this` no construtor.** O widget é criado por `Rc::new_cyclic`; as features recebem um `Weak` do widget. A criação das
//!   `contributions` do diff (`_createDiffEditorContributions`) e o `addDiffEditor(this)` precisam do `Rc` do widget e rodam logo
//!   depois do construtor, não antes do `_gutter` e do resto como no original (nenhum dos dois é lido por eles).
//! - **`disposableObservableValue`** do `_diffModelSrc`: o observável de `je_base` só guarda `DisposableRef`, e o widget precisa do
//!   `RefCounted<Rc<DiffEditorViewModel>>` tipado. A troca descarta o valor antigo antes de gravar o novo, e o `dispose` descarta o
//!   atual, como o `DisposableObservableValue`.
//! - **`observableFromEvent.batchEventsGlobally(tx, ...)`** exige uma transação `Rc`. Com `tx` ausente o widget cria a transação
//!   (`TransactionImpl`) e agrupa os eventos dos dois `setModel`; com `tx` recebido emprestado (`&dyn ITransaction`) os dois
//!   `setModel` rodam sem o agrupamento global (os observáveis dos editores internos convergem para o mesmo estado final).
//! - **`IDiffEditorModel | IDiffEditorViewModel`** de `setModel` é o [`EditorModel`]; a variante `Text` não é aceita (o original só
//!   aceita as duas formas do diff). O `as DiffEditorViewModel` é o `downcast` do `Rc<dyn IDiffEditorViewModel>` (o trait ganhou
//!   `Any` como supertrait).
//! - **`IDiffEditorViewState.modelState: unknown`** é um [`Dynamic`] com o formato de `SerializedState`
//!   (`{ collapsedRegions: [{ range: [início, fim] }] }`).
//! - **Os getters do original que devolvem `ICodeEditor`** (`getOriginalEditor`, `getModifiedEditor`) são os campos
//!   `original` e `modified` de [`DiffEditorWidget::editors`]; no trait [`IDiffEditorWidget`] devolvem o recorte `IDiffSideEditor`,
//!   implementado pelo [`DiffSideEditor`] (um tipo próprio, porque no `CodeEditorWidget` os métodos do recorte colidiriam com os do
//!   `ICodeEditor`).
//! - **`updateOptions`** existe nos três traits que o widget implementa (o mesmo método do original); o corpo está no
//!   [`DelegatingEditor`] e os outros dois o chamam.
//! - **`IAccessbilitySignalOptions.source`** (`'diffEditor.goToDiff'`, `'diffEditor.cursorPositionChanged'`) não tem campo no porte
//!   do `playSignal`; só o sinal é tocado.
//! - **`onVisible` e `onHide`** do original chamam o `onVisible`/`onHide` dos dois editores de código, que só chamam
//!   `view.refreshFocusState()` (a `View` não existe no porte; o foco é dado pelo hospedeiro nativo), então não há o que chamar.
//! - **`readHotReloadableExport`** só existe para recarga a quente de módulos do desenvolvimento do VS Code e não tem equivalente.

use std::any::Any;
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceScheduler;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, RefCounted, to_disposable};
use je_base::observable::deps::try_finally;
use je_base::observable::observables::derived::derived_owned;
use je_base::observable::observables::observable_from_event::batch_events_globally;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::{
    AnyObservable, DebugOwner, IObservable, IReader, ITransaction, Observable, ObservableExt, SettableExt, StrictEquals, TransactionImpl, autorun,
    autorun_with_store, recompute_initially_and_on_change, transaction,
};
use je_base::promise::Promise;
use je_base::runtime::set_timeout;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService, IContextKeyServiceTarget, RawContextKey};
use je_platform::observable::platform_observable_utils::bind_context_key;
use je_ui::{NodeId, Ui};
use je_widgets::sash::BoundarySashes;

use crate::cursor_common::CursorSimpleModel;
use crate::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService, IEditorProgressService, IProgressIndicator, ProgressTotal};
use crate::code_editor_widget::{CodeEditorWidget, ExecuteEditsSource, ICodeEditor, SelectionOrRange};
use crate::config::editor_options::layout_info::EditorLayoutInfo;
use crate::config::element_size_observer::ReferenceDomElement;
use crate::core_2d::dimension::Dimension;
use crate::cursor_events::{CursorChangeReason, ICursorPositionChangedEvent};
use crate::diff::legacy_lines_diff_computer::{CharChange, DiffComputationResult, LineChange};
use crate::diff::range_mapping::{DetailedLineRangeMapping, LineRangeMapping, RangeMapping};
use crate::diff_editor::components::diff_editor_decorations::DiffEditorDecorations;
use crate::diff_editor::components::diff_editor_editors::{CreateInnerEditor, DiffEditorEditors, DiffEditorEditorsArgs};
use crate::diff_editor::components::diff_editor_sash::{DiffEditorSash, ObservableDimension, SashLayout};
use crate::diff_editor::delegating_editor_impl::{DelegatingEditor, DelegatingEditorBase, DiffEditorViewState, EditorModel, EditorViewState};
use crate::diff_editor::embedded_diff_editor_widget::EmbeddedDiffEditorBase;
use crate::diff_editor::feature_contracts::{
    DiffEditorFeatureContext, DiffEditorFeatures, IAccessibleDiffViewer, IDiffEditorGutter, IMovedBlocksLinesFeature, IOverviewRulerFeature,
};
pub use crate::diff_editor::DiffCodeEditorWidgetOptions;
use crate::diff_editor::ENTIRE_DIFF_OVERVIEW_WIDTH;
use crate::diff_editor_options::{DiffEditorOptions, DiffEditorVariant};
use crate::diff_editor_utils::{
    IObservableViewZone, ObservableElementSizeObserver, ViewZoneIdSet, apply_view_zones, derived_disposable_of, derived_optional_disposable_of, translate_position,
};
use crate::diff_editor_view_model::{
    DiffEditorModel, DiffEditorViewModel, DiffMapping, DiffState, IDiffEditorViewModel, SerializedCollapsedRegion, SerializedState,
};
use crate::editor_context_keys;
use crate::line_range::LineRange;
use crate::multi_diff_editor::diff_editor_surface::{IDiffEditorWidget, IDiffSideEditor};
use crate::observable_code_editor::observable_code_editor;
use crate::position::Position;
use crate::range::Range;
use crate::edit_operation::SingleEditOperation;
use crate::stable_editor_scroll::StableEditorScrollState;
use crate::view_model_event_dispatcher::ContentSizeChangedEvent;

/// `EditorType.IDiffEditor`.
pub const EDITOR_TYPE_IDIFF_EDITOR: &str = "vs.editor.IDiffEditor";

/// Uma das `EditorContextKeys` do diff (a função que a devolve).
type KeyGetter = fn() -> RawContextKey;

/// `this._register(bindContextKey(key, service, reader => <booleano>))`.
fn bind_bool_key(store: &DisposableStore, service: &dyn IContextKeyService, key: KeyGetter, compute: impl Fn(&dyn IReader) -> bool + 'static) {
    store.add(bind_context_key(&key(), service, move |reader| Dynamic::Bool(compute(reader))));
}

/// O que o `DiffEditorWidget` chama no `ICodeEditorService` (`willCreateDiffEditor`, `addDiffEditor`, `removeDiffEditor`). O
/// registro de editores do porte ainda não os tem; quem os implementar (o `CodeEditorRegistry`) passa a instância nos
/// [`DiffEditorWidgetArgs`].
pub trait IDiffEditorRegistry {
    /// `willCreateDiffEditor()`.
    fn will_create_diff_editor(&self);

    /// `addDiffEditor(editor)`.
    fn add_diff_editor(&self, editor: Rc<DiffEditorWidget>);

    /// `removeDiffEditor(editor)`, pelo `getId()` do editor (o editor já está sendo descartado).
    fn remove_diff_editor(&self, editor_id: &str);
}

/// Os argumentos estáticos do construtor do `DiffEditorWidget` (ver as divergências do módulo).
pub struct DiffEditorWidgetArgs {
    /// A árvore de UI onde o sash entra.
    pub ui: Ui,
    /// O `domElement`.
    pub dom_node: NodeId,
    /// O alvo do `contextKeyService.createScoped(domElement)`.
    pub context_key_target: Rc<dyn IContextKeyServiceTarget>,
    /// O elemento que o `automaticLayout` observa (o `elements.root`), se houver.
    pub reference_element: Option<Rc<dyn ReferenceDomElement>>,
    /// `options: Readonly<IDiffEditorConstructionOptions>`.
    pub options: Dynamic,
    /// `codeEditorWidgetOptions`.
    pub code_editor_widget_options: DiffCodeEditorWidgetOptions,
    /// O `_createInnerEditor`.
    pub create_inner_editor: CreateInnerEditor,
    /// A criação das features da fatia 3.
    pub features: Rc<dyn DiffEditorFeatures>,
    /// O `ICodeEditorService` (a parte dos editores de diff).
    pub diff_editor_registry: Option<Rc<dyn IDiffEditorRegistry>>,
}

/// A altura da raiz: `100%` (`automaticLayout`) ou em pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RootHeight {
    Full,
    Px(f64),
}

/// A posição horizontal, a largura e a visibilidade (`visibility`) do contêiner de um dos editores.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorElementBox {
    pub left: f64,
    pub width: f64,
    pub visible: bool,
}

/// O que o pintor lê do `elements` do widget (ver as divergências do módulo).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffEditorElementsState {
    /// `root.style.height`.
    pub root_height: RootHeight,
    /// A classe `side-by-side` da raiz.
    pub side_by_side: bool,
    pub original: EditorElementBox,
    pub modified: EditorElementBox,
}

impl Default for DiffEditorElementsState {
    fn default() -> Self {
        let hidden = EditorElementBox { left: 0.0, width: 0.0, visible: true };
        Self { root_height: RootHeight::Full, side_by_side: true, original: hidden, modified: hidden }
    }
}

/// O que o `_layoutInfo` derivado calcula.
struct LayoutInfo {
    modified_editor: EditorLayoutInfo,
    original_editor: EditorLayoutInfo,
}

/// `'previous' | 'next'` de `goToDiff`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffNavigationTarget {
    Previous,
    Next,
}

/// O retorno de `mapToOtherSide`.
pub struct MapToOtherSide {
    pub destination: Rc<CodeEditorWidget>,
    pub destination_selection: Option<Range>,
}

/// O estado do redimensionamento suave (`lastWidth`, `resizeStartWidth`, `windowResizeEventCount`, `resizeWasPointerDriven`,
/// `pointerDown` e o `smoothResizeScheduler`).
struct SmoothResize {
    last_width: Cell<Option<f64>>,
    resize_start_width: Cell<Option<f64>>,
    window_resize_event_count: Cell<u32>,
    resize_was_pointer_driven: Cell<bool>,
    pointer_down: Cell<bool>,
    scheduler: OnceCell<Rc<RunOnceScheduler>>,
}

impl SmoothResize {
    fn new() -> Rc<Self> {
        let this = Rc::new(Self {
            last_width: Cell::new(None),
            resize_start_width: Cell::new(None),
            window_resize_event_count: Cell::new(0),
            resize_was_pointer_driven: Cell::new(false),
            pointer_down: Cell::new(false),
            scheduler: OnceCell::new(),
        });
        let weak = Rc::downgrade(&this);
        let scheduler = RunOnceScheduler::new(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.finish();
                }
            },
            200,
        );
        this.scheduler.set(scheduler).ok();
        this
    }

    fn scheduler(&self) -> &Rc<RunOnceScheduler> {
        self.scheduler.get().expect("o agendador é criado em `new`")
    }

    /// `finishSmoothResize`.
    fn finish(&self) {
        if self.pointer_down.get() {
            self.scheduler().schedule(None);
            return;
        }
        self.resize_start_width.set(None);
        self.window_resize_event_count.set(0);
        self.resize_was_pointer_driven.set(false);
    }

    fn on_window_resize(&self) {
        self.window_resize_event_count.set(self.window_resize_event_count.get() + 1);
        self.scheduler().schedule(None);
    }

    fn on_pointer_down(&self) {
        self.pointer_down.set(true);
    }

    fn on_pointer_up(&self) {
        if !self.pointer_down.get() {
            return;
        }
        self.pointer_down.set(false);
        self.scheduler().cancel();
        self.finish();
    }

    /// A parte do autorun de largura que decide o `smoothResizeStartWidth` (antes do `setWidth`).
    fn smooth_resize_start_width(&self, width: f64) -> Option<f64> {
        let mut smooth_resize_start_width = None;
        if let Some(last_width) = self.last_width.get()
            && width != last_width
        {
            if self.resize_start_width.get().is_none() {
                self.resize_start_width.set(Some(last_width));
            }
            self.resize_was_pointer_driven.set(self.resize_was_pointer_driven.get() || self.pointer_down.get());
            if let Some(resize_start_width) = self.resize_start_width.get()
                && (self.resize_was_pointer_driven.get() || self.window_resize_event_count.get() > 1)
            {
                smooth_resize_start_width = Some(resize_start_width);
            }
            self.scheduler().schedule(None);
        }
        smooth_resize_start_width
    }
}

/// `read(reader)` de um observável atribuído mais adiante no construtor: `None` enquanto ele não existe.
fn read_late<T: Clone + 'static>(slot: &OnceCell<IObservable<Option<T>>>, reader: &dyn IReader) -> Option<T> {
    slot.get().and_then(|observable| observable.read(reader))
}

/// O sinal de acessibilidade da linha do diff (`diffLineDeleted`, `diffLineInserted` ou `diffLineModified`); sem diff não toca.
fn play_diff_line_signal(signals: &dyn IAccessibilitySignalService, mapping: Option<&LineRangeMapping>) {
    let Some(mapping) = mapping else { return };
    let signal = if mapping.modified.is_empty() {
        AccessibilitySignal::DiffLineDeleted
    } else if mapping.original.is_empty() {
        AccessibilitySignal::DiffLineInserted
    } else {
        AccessibilitySignal::DiffLineModified
    };
    signals.play_signal(signal);
}

/// `IDiffEditorOptions.dimension`.
fn dimension_option(options: &Dynamic) -> Option<Dimension> {
    let dimension = options.optional_get_str("dimension");
    match (dimension.optional_get_str("width"), dimension.optional_get_str("height")) {
        (Dynamic::Number(width), Dynamic::Number(height)) => Some(Dimension { width, height }),
        _ => None,
    }
}

/// `SerializedState` como o `unknown` de `IDiffEditorViewState.modelState`.
fn serialized_state_to_dynamic(state: &SerializedState) -> Dynamic {
    let collapsed_regions = match &state.collapsed_regions {
        Some(regions) => Dynamic::array(regions.iter().map(|region| {
            let [start, end] = region.range;
            Dynamic::object([("range", Dynamic::array([Dynamic::Number(f64::from(start)), Dynamic::Number(f64::from(end))]))])
        })),
        None => Dynamic::Undefined,
    };
    Dynamic::object([("collapsedRegions", collapsed_regions)])
}

/// O inverso de [`serialized_state_to_dynamic`] (`modelState as any`).
fn serialized_state_from_dynamic(state: &Dynamic) -> SerializedState {
    let number = |range: &Dynamic, index: usize| match range {
        Dynamic::Array(items) => match items.get(index) {
            Dynamic::Number(value) => value as i32,
            _ => 0,
        },
        _ => 0,
    };
    let collapsed_regions = match state.get_str("collapsedRegions") {
        Dynamic::Array(regions) => Some(
            regions
                .to_vec()
                .iter()
                .map(|region| {
                    let range = region.get_str("range");
                    SerializedCollapsedRegion { range: [number(&range, 0), number(&range, 1)] }
                })
                .collect(),
        ),
        _ => None,
    };
    SerializedState { collapsed_regions }
}

/// `toLineChanges(state)`.
pub fn to_line_changes(state: &DiffState) -> Vec<LineChange> {
    state
        .mappings
        .iter()
        .map(|x| {
            let m = &x.line_range_mapping;
            let mut inner_changes = m.inner_changes.as_ref();

            let (original_start_line_number, original_end_line_number) = if m.original.is_empty() {
                // Insertion
                inner_changes = None;
                (m.original.start_line_number - 1, 0)
            } else {
                (m.original.start_line_number, m.original.end_line_number_exclusive - 1)
            };

            let (modified_start_line_number, modified_end_line_number) = if m.modified.is_empty() {
                // Deletion
                inner_changes = None;
                (m.modified.start_line_number - 1, 0)
            } else {
                (m.modified.start_line_number, m.modified.end_line_number_exclusive - 1)
            };

            LineChange {
                original_start_line_number,
                original_end_line_number,
                modified_start_line_number,
                modified_end_line_number,
                char_changes: inner_changes.map(|inner_changes| {
                    inner_changes
                        .iter()
                        .map(|m| CharChange {
                            original_start_line_number: m.original_range.start_line_number,
                            original_start_column: m.original_range.start_column,
                            original_end_line_number: m.original_range.end_line_number,
                            original_end_column: m.original_range.end_column,
                            modified_start_line_number: m.modified_range.start_line_number,
                            modified_start_column: m.modified_range.start_column,
                            modified_end_line_number: m.modified_range.end_line_number,
                            modified_end_column: m.modified_range.end_column,
                        })
                        .collect()
                }),
            }
        })
        .collect()
}

/// `DiffEditorWidget`.
pub struct DiffEditorWidget {
    me: Weak<DiffEditorWidget>,
    delegating: DelegatingEditorBase,
    store: Rc<DisposableStore>,

    ui: Ui,
    dom_node: NodeId,
    elements: Rc<RefCell<DiffEditorElementsState>>,

    diff_model_src: Rc<ObservableValue<Option<Rc<RefCounted<Rc<DiffEditorViewModel>>>>, ()>>,
    diff_model: IObservable<Option<Rc<DiffEditorViewModel>>>,
    deferred_diff_model_ref_disposals: Rc<DisposableStore>,
    all_unchanged_regions_shown: IObservable<bool>,
    /// `onDidChangeModel`.
    pub on_did_change_model: Event<()>,

    instantiation_service: Rc<InstantiationService>,
    accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,

    root_size_observer: Rc<ObservableElementSizeObserver>,
    smooth_resize: Rc<SmoothResize>,

    boundary_sashes: Rc<ObservableValue<Option<Rc<BoundarySashes>>, ()>>,
    accessible_diff_viewer: IObservable<Rc<dyn IAccessibleDiffViewer>>,
    options: Rc<DiffEditorOptions>,
    editors: Rc<DiffEditorEditors>,

    /// `onDidUpdateDiff`.
    pub on_did_update_diff: Event<()>,
}

/// Os objetos que o construtor ainda precisa do `Rc` do widget para criar (ver as divergências do módulo).
type DeferredInit = Box<dyn FnOnce(&Rc<DiffEditorWidget>)>;

impl Injectable for DiffEditorWidget {
    type Args = DiffEditorWidgetArgs;
    type Deps = (Rc<dyn IContextKeyService>, Rc<InstantiationService>, Rc<dyn IAccessibilitySignalService>, Rc<dyn IEditorProgressService>);

    fn create(args: Self::Args, (context_key_service, instantiation_service, accessibility_signal_service, editor_progress_service): Self::Deps) -> Rc<Self> {
        Self::new(args, context_key_service, instantiation_service, accessibility_signal_service, editor_progress_service)
    }
}

impl DiffEditorWidget {
    /// `DiffEditorWidget.ENTIRE_DIFF_OVERVIEW_WIDTH`.
    pub const ENTIRE_DIFF_OVERVIEW_WIDTH: f64 = ENTIRE_DIFF_OVERVIEW_WIDTH;

    pub fn new(
        args: DiffEditorWidgetArgs,
        parent_context_key_service: Rc<dyn IContextKeyService>,
        parent_instantiation_service: Rc<InstantiationService>,
        accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
        editor_progress_service: Rc<dyn IEditorProgressService>,
    ) -> Rc<Self> {
        let mut deferred: Vec<DeferredInit> = Vec::new();
        let this = Rc::new_cyclic(|me| {
            Self::build(me, args, parent_context_key_service, parent_instantiation_service, accessibility_signal_service, editor_progress_service, &mut deferred)
        });
        for init in deferred {
            init(&this);
        }
        this
    }

    #[allow(clippy::too_many_lines)]
    fn build(
        me: &Weak<DiffEditorWidget>,
        args: DiffEditorWidgetArgs,
        parent_context_key_service: Rc<dyn IContextKeyService>,
        parent_instantiation_service: Rc<InstantiationService>,
        accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
        editor_progress_service: Rc<dyn IEditorProgressService>,
        deferred: &mut Vec<DeferredInit>,
    ) -> Self {
        let DiffEditorWidgetArgs { ui, dom_node, context_key_target, reference_element, options: construction_options, code_editor_widget_options, create_inner_editor, features, diff_editor_registry } =
            args;
        let owner = DebugOwner::new("DiffEditorWidget");
        let delegating = DelegatingEditorBase::default();
        let store = DisposableStore::new();
        delegating.register(store.clone());

        let elements = Rc::new(RefCell::new(DiffEditorElementsState::default()));

        let diff_model_src: Rc<ObservableValue<Option<Rc<RefCounted<Rc<DiffEditorViewModel>>>>, ()>> = observable_value_owned(owner.clone(), None);
        {
            // `this._register(disposableObservableValue(...))`: descartar o widget descarta a referência atual.
            let diff_model_src = diff_model_src.clone();
            store.add(to_disposable(move || {
                if let Some(current) = diff_model_src.get() {
                    current.dispose();
                }
            }));
        }
        let diff_model: IObservable<Option<Rc<DiffEditorViewModel>>> = {
            let diff_model_src = diff_model_src.clone();
            derived_owned(owner.clone(), move |reader| diff_model_src.read(reader).map(|reference| reference.object().clone()))
        };
        let deferred_diff_model_ref_disposals = DisposableStore::new();
        store.add(deferred_diff_model_ref_disposals.clone());
        let all_unchanged_regions_shown: IObservable<bool> = {
            let diff_model = diff_model.clone();
            derived_owned(owner.clone(), move |reader| {
                let regions = diff_model.read(reader).map(|m| m.unchanged_regions.read(reader)).unwrap_or_default();
                regions.iter().all(|r| r.visible_line_count_top.read(reader) + r.visible_line_count_bottom.read(reader) >= r.line_count)
            })
        };
        let on_did_change_model = Event::<()>::from_observable_light(diff_model.to_any_observable());

        let context_key_service = parent_context_key_service.create_scoped(context_key_target);
        store.add(context_key_service.clone());
        let scoped_context_key_service: Rc<dyn IContextKeyService> = context_key_service.clone();
        let instantiation_service = parent_instantiation_service
            .create_child(ServiceCollection::new().with(I_CONTEXT_KEY_SERVICE, scoped_context_key_service.clone()), Some(&*store));

        let options = instantiation_service.create_instance::<DiffEditorOptions>(construction_options.clone());

        let boundary_sashes: Rc<ObservableValue<Option<Rc<BoundarySashes>>, ()>> = observable_value_owned(owner.clone(), None);
        let accessible_diff_viewer_should_be_visible: Rc<ObservableValue<bool, ()>> = observable_value_owned(owner.clone(), false);
        let accessible_diff_viewer_visible: IObservable<bool> = {
            let (options, should_be_visible) = (options.clone(), accessible_diff_viewer_should_be_visible.clone());
            derived_owned(owner.clone(), move |reader| {
                if options.only_show_accessible_diff_viewer.read(reader) { true } else { should_be_visible.read(reader) }
            })
        };
        let moved_blocks_lines_part: Rc<ObservableValue<Option<Rc<dyn IMovedBlocksLinesFeature>>, ()>> = observable_value_owned(owner.clone(), None);

        if let Some(registry) = &diff_editor_registry {
            registry.will_create_diff_editor();
        }

        scoped_context_key_service.create_key("isInDiffEditor", Dynamic::Bool(true));

        let root_size_observer = ObservableElementSizeObserver::new(reference_element, dimension_option(&construction_options));
        store.add(root_size_observer.clone());
        root_size_observer.set_automatic_layout(construction_options.optional_get_str("automaticLayout").is_truthy());

        // Redimensionamento suave: os ouvintes da janela são `on_window_resize`, `on_pointer_down` e `on_pointer_up`.
        let smooth_resize = SmoothResize::new();
        store.add(smooth_resize.scheduler().clone());
        {
            let (smooth_resize, options, width) = (smooth_resize.clone(), options.clone(), root_size_observer.width.clone());
            store.add(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let width = width.read(reader);
                let smooth_resize_start_width = smooth_resize.smooth_resize_start_width(width);
                options.set_width(width, smooth_resize_start_width);
                smooth_resize.last_width.set(Some(width));
            }));
        }

        scoped_context_key_service.create_key(editor_context_keys::is_embedded_diff_editor().key(), Dynamic::Bool(false));
        {
            let options = options.clone();
            bind_bool_key(&store, &*scoped_context_key_service, editor_context_keys::is_embedded_diff_editor, move |reader| options.is_in_embedded_editor.read(reader));
        }
        {
            let diff_model = diff_model.clone();
            bind_bool_key(&store, &*scoped_context_key_service, editor_context_keys::comparing_moved_code, move |reader| {
                diff_model.read(reader).is_some_and(|m| m.moved_text_to_compare.read(reader).is_some())
            });
        }
        {
            let options = options.clone();
            bind_bool_key(
                &store,
                &*scoped_context_key_service,
                editor_context_keys::diff_editor_render_side_by_side_inline_breakpoint_reached,
                move |reader| options.could_show_inline_view_because_of_size.read(reader),
            );
        }
        {
            let options = options.clone();
            bind_bool_key(&store, &*scoped_context_key_service, editor_context_keys::diff_editor_inline_mode, move |reader| !options.render_side_by_side.read(reader));
        }
        {
            let options = options.clone();
            bind_bool_key(&store, &*scoped_context_key_service, editor_context_keys::diff_editor_temporary_inline_mode, move |reader| {
                options.temporary_inline_mode.read(reader)
            });
        }
        {
            let options = options.clone();
            bind_bool_key(&store, &*scoped_context_key_service, editor_context_keys::diff_editor_automatic_render_side_by_side, move |reader| {
                options.render_side_by_side_in_automatic_mode.read(reader)
            });
        }
        {
            let diff_model = diff_model.clone();
            bind_bool_key(&store, &*scoped_context_key_service, editor_context_keys::has_changes, move |reader| {
                diff_model.read(reader).and_then(|m| m.diff.read(reader)).is_some_and(|diff| !diff.mappings.is_empty())
            });
        }

        let original_widget_options = code_editor_widget_options.run_with_original_editor_scroll_anchor.clone();
        let modified_widget_options = code_editor_widget_options.run_with_modified_editor_scroll_anchor.clone();
        let variant = code_editor_widget_options.variant.unwrap_or(DiffEditorVariant::Default);
        let editors = instantiation_service.create_instance::<DiffEditorEditors>(DiffEditorEditorsArgs {
            options: options.clone(),
            widget_options: code_editor_widget_options,
            create_inner_editor,
        });
        store.add(editors.clone());

        {
            let options = options.clone();
            bind_bool_key(&store, &*scoped_context_key_service, editor_context_keys::diff_editor_original_writable, move |reader| options.original_editable.read(reader));
        }
        {
            let options = options.clone();
            bind_bool_key(&store, &*scoped_context_key_service, editor_context_keys::diff_editor_modified_writable, move |reader| !options.read_only.read(reader));
        }
        let uri_keys: [(KeyGetter, bool); 2] = [(editor_context_keys::diff_editor_original_uri, true), (editor_context_keys::diff_editor_modified_uri, false)];
        for (key, original) in uri_keys {
            let diff_model = diff_model.clone();
            store.add(bind_context_key(&key(), &*scoped_context_key_service, move |reader| {
                let uri = diff_model.read(reader).map(|m| {
                    let text_model = if original { &m.model.original } else { &m.model.modified };
                    text_model.uri().to_string(false)
                });
                Dynamic::String(uri.unwrap_or_default())
            }));
        }

        let feature_context = DiffEditorFeatureContext {
            ui: ui.clone(),
            root: dom_node,
            editors: editors.clone(),
            diff_model: diff_model.clone(),
            options: options.clone(),
            instantiation_service: instantiation_service.clone(),
            diff_editor: me.clone(),
        };

        // As referências do `_layoutInfo` a campos atribuídos mais adiante.
        let sash_slot: Rc<OnceCell<IObservable<Option<Rc<DiffEditorSash>>>>> = Rc::default();
        let gutter_slot: Rc<OnceCell<IObservable<Option<Rc<dyn IDiffEditorGutter>>>>> = Rc::default();
        let overview_ruler_slot: Rc<OnceCell<IObservable<Option<Rc<dyn IOverviewRulerFeature>>>>> = Rc::default();

        let layout_info: IObservable<Rc<LayoutInfo>> = {
            let (elements, size, options, editors, moved_blocks_lines_part) =
                (elements.clone(), root_size_observer.clone(), options.clone(), editors.clone(), moved_blocks_lines_part.clone());
            let (sash_slot, gutter_slot, overview_ruler_slot) = (sash_slot.clone(), gutter_slot.clone(), overview_ruler_slot.clone());
            derived_owned(owner.clone(), move |reader| {
                let full_width = size.width.read(reader);
                let full_height = size.height.read(reader);

                elements.borrow_mut().root_height = if size.automatic_layout() { RootHeight::Full } else { RootHeight::Px(full_height) };

                let sash = read_late(&sash_slot, reader);

                let gutter = read_late(&gutter_slot, reader);
                let gutter_width = gutter.as_ref().map_or(0.0, |gutter| gutter.width().read(reader));

                let overview_ruler_part_width = read_late(&overview_ruler_slot, reader).map_or(0.0, |part| part.width());

                let original_left: f64;
                let original_width: f64;
                let modified_left: f64;
                let modified_width: f64;
                let gutter_left: f64;

                if let Some(sash) = &sash {
                    let sash_left = sash.sash_left.read(reader);
                    let moved_blocks_lines_width = moved_blocks_lines_part.read(reader).map_or(0.0, |part| part.width().read(reader));

                    original_left = 0.0;
                    original_width = sash_left - gutter_width - moved_blocks_lines_width;

                    gutter_left = sash_left - gutter_width;

                    modified_left = sash_left;
                    modified_width = full_width - modified_left - overview_ruler_part_width;
                } else {
                    gutter_left = 0.0;

                    let should_hide_original_line_numbers = options.inline_view_hide_original_line_numbers.read(reader);
                    original_left = gutter_width;
                    original_width = if should_hide_original_line_numbers {
                        0.0
                    } else {
                        5.0_f64.max(editors.original_obs.layout_info_decorations_left.read(reader))
                    };

                    modified_left = gutter_width + original_width;
                    modified_width = full_width - modified_left - overview_ruler_part_width;
                }

                {
                    let mut elements = elements.borrow_mut();
                    elements.original.left = original_left;
                    elements.original.width = original_width;
                }
                editors.original.layout(Some(Dimension { width: original_width, height: full_height }));

                if let Some(gutter) = &gutter {
                    gutter.layout(gutter_left);
                }

                {
                    let mut elements = elements.borrow_mut();
                    elements.modified.left = modified_left;
                    elements.modified.width = modified_width;
                }
                editors.modified.layout(Some(Dimension { width: modified_width, height: full_height }));

                Rc::new(LayoutInfo { modified_editor: editors.modified.get_layout_info(), original_editor: editors.original.get_layout_info() })
            })
        };
        let diff_value: IObservable<Option<Rc<DiffState>>> = diff_model.map(|m, reader| m.and_then(|m| m.diff.read(reader)));
        let on_did_update_diff = Event::<()>::from_observable_light(diff_value.to_any_observable());

        let overview_ruler_part = {
            let (features, context, options, size, layout_info) =
                (features.clone(), feature_context.clone(), options.clone(), root_size_observer.clone(), layout_info.clone());
            derived_optional_disposable_of(Some(owner.clone()), move |reader| {
                if !options.render_overview_ruler.read(reader) {
                    return None;
                }
                Some(features.create_overview_ruler(&context, size.width.clone(), size.height.clone(), layout_info.map(|i, _| i.modified_editor)))
            })
        };
        overview_ruler_slot.set(overview_ruler_part.clone()).ok();
        overview_ruler_part.recompute_initially_and_on_change(&store, None);

        let dimensions = {
            let (size, overview_ruler_slot) = (root_size_observer.clone(), overview_ruler_slot.clone());
            ObservableDimension {
                height: size.height.clone(),
                width: size.width.map(move |w, reader| w - read_late(&overview_ruler_slot, reader).map_or(0.0, |part| part.width())),
            }
        };

        let sash_layout = Rc::new(SashLayout::new(options.clone(), dimensions.clone()));

        let sash = {
            let (elements, options, ui, dimensions, boundary_sashes, sash_layout) =
                (elements.clone(), options.clone(), ui.clone(), dimensions.clone(), boundary_sashes.clone(), sash_layout.clone());
            derived_optional_disposable_of(Some(owner.clone()), move |reader| {
                let show_sash = options.render_side_by_side.read(reader);
                elements.borrow_mut().side_by_side = show_sash;
                if !show_sash {
                    return None;
                }
                let sash_layout_for_reset = sash_layout.clone();
                Some(DiffEditorSash::new(
                    &ui,
                    dom_node,
                    dimensions.clone(),
                    options.enable_split_view_resizing.clone(),
                    boundary_sashes.clone(),
                    sash_layout.sash_left.clone(),
                    Rc::new(move || sash_layout_for_reset.reset_sash()),
                ))
            })
        };
        sash_slot.set(sash.clone()).ok();
        sash.recompute_initially_and_on_change(&store, None);

        let unchanged_ranges_feature = {
            let (features, context) = (features.clone(), feature_context.clone());
            derived_disposable_of(Some(owner.clone()), move |_reader| {
                features.create_hide_unchanged_regions(&context, original_widget_options.clone(), modified_widget_options.clone(), variant)
            })
        };
        unchanged_ranges_feature.recompute_initially_and_on_change(&store, None);

        {
            let (editors, diff_model, options) = (editors.clone(), diff_model.clone(), options.clone());
            derived_disposable_of(Some(owner.clone()), move |_reader| DiffEditorDecorations::new(&editors, diff_model.clone(), options.clone()))
                .recompute_initially_and_on_change(&store, None);
        }

        let orig_view_zone_ids_to_ignore: ViewZoneIdSet = Rc::default();
        let mod_view_zone_ids_to_ignore: ViewZoneIdSet = Rc::default();
        let is_updating_view_zones = Rc::new(Cell::new(false));
        let view_zone_manager = {
            let (features, context, unchanged_ranges_feature, is_updating_view_zones) =
                (features.clone(), feature_context.clone(), unchanged_ranges_feature.clone(), is_updating_view_zones.clone());
            let (orig_ids, mod_ids) = (orig_view_zone_ids_to_ignore.clone(), mod_view_zone_ids_to_ignore.clone());
            derived_disposable_of(Some(owner.clone()), move |_reader| {
                let (is_updating_view_zones, unchanged_ranges_feature) = (is_updating_view_zones.clone(), unchanged_ranges_feature.clone());
                features.create_view_zones(
                    &context,
                    Rc::new(move || is_updating_view_zones.get() || unchanged_ranges_feature.get().is_updating_hidden_areas()),
                    orig_ids.clone(),
                    mod_ids.clone(),
                )
            })
        };
        view_zone_manager.recompute_initially_and_on_change(&store, None);

        let original_view_zones: IObservable<Rc<Vec<Rc<dyn IObservableViewZone>>>> = {
            let (view_zone_manager, unchanged_ranges_feature) = (view_zone_manager.clone(), unchanged_ranges_feature.clone());
            derived_owned(owner.clone(), move |reader| {
                let orig = view_zone_manager.read(reader).view_zones().read(reader);
                let orig2 = unchanged_ranges_feature.read(reader).view_zones().read(reader);
                Rc::new(orig.orig.iter().chain(orig2.orig.iter()).cloned().collect())
            })
        };
        let modified_view_zones: IObservable<Rc<Vec<Rc<dyn IObservableViewZone>>>> = {
            let (view_zone_manager, unchanged_ranges_feature) = (view_zone_manager.clone(), unchanged_ranges_feature.clone());
            derived_owned(owner.clone(), move |reader| {
                let modified = view_zone_manager.read(reader).view_zones().read(reader);
                let modified2 = unchanged_ranges_feature.read(reader).view_zones().read(reader);
                Rc::new(modified.modified.iter().chain(modified2.modified.iter()).cloned().collect())
            })
        };
        {
            let is_updating_view_zones = is_updating_view_zones.clone();
            store.add(apply_view_zones(
                &editors.original,
                original_view_zones,
                Some(Rc::new(move |is_updating_orig_view_zones| is_updating_view_zones.set(is_updating_orig_view_zones))),
                Some(orig_view_zone_ids_to_ignore),
            ));
        }
        {
            let scroll_state: Rc<RefCell<Option<StableEditorScrollState>>> = Rc::default();
            let (is_updating_view_zones, modified) = (is_updating_view_zones.clone(), editors.modified.clone());
            store.add(apply_view_zones(
                &editors.modified,
                modified_view_zones,
                Some(Rc::new(move |is_updating_mod_view_zones| {
                    is_updating_view_zones.set(is_updating_mod_view_zones);
                    if is_updating_mod_view_zones {
                        *scroll_state.borrow_mut() = Some(StableEditorScrollState::capture(&modified));
                    } else {
                        let state = scroll_state.borrow_mut().take();
                        if let Some(state) = state {
                            state.restore(&modified);
                        }
                    }
                })),
                Some(mod_view_zone_ids_to_ignore),
            ));
        }

        let accessible_diff_viewer: IObservable<Rc<dyn IAccessibleDiffViewer>> = {
            let (features, context, options, size, diff_model) =
                (features.clone(), feature_context.clone(), options.clone(), root_size_observer.clone(), diff_model.clone());
            let (visible, should_be_visible) = (accessible_diff_viewer_visible.clone(), accessible_diff_viewer_should_be_visible.clone());
            derived_disposable_of(Some(owner.clone()), move |_reader| {
                let should_be_visible = should_be_visible.clone();
                let diffs = diff_model.map(|m, reader| {
                    m.and_then(|m| m.diff.read(reader))
                        .map(|diff| Rc::new(diff.mappings.iter().map(|mapping| mapping.line_range_mapping.clone()).collect::<Vec<DetailedLineRangeMapping>>()))
                });
                features.create_accessible_diff_viewer(
                    &context,
                    visible.clone(),
                    Rc::new(move |visible: bool, tx: Option<&dyn ITransaction>| should_be_visible.set(visible, tx)),
                    options.only_show_accessible_diff_viewer.map(|v, _| !v),
                    size.width.clone(),
                    size.height.clone(),
                    diffs,
                )
            })
        };
        accessible_diff_viewer.recompute_initially_and_on_change(&store, None);

        {
            // `applyStyle(modified | original, { visibility })`.
            let (elements, accessible_diff_viewer_visible) = (elements.clone(), accessible_diff_viewer_visible.clone());
            store.add(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let visible = !accessible_diff_viewer_visible.read(reader);
                let mut elements = elements.borrow_mut();
                elements.modified.visible = visible;
                elements.original.visible = visible;
            }));
        }

        if let Some(registry) = diff_editor_registry {
            deferred.push(Box::new(move |this: &Rc<DiffEditorWidget>| {
                // `_createDiffEditorContributions` e `this._codeEditorService.addDiffEditor(this)`.
                this.create_diff_editor_contributions();
                registry.add_diff_editor(this.clone());
                let editor_id = DelegatingEditor::get_id(&**this);
                this.store.add(to_disposable(move || registry.remove_diff_editor(&editor_id)));
            }));
        } else {
            deferred.push(Box::new(|this: &Rc<DiffEditorWidget>| this.create_diff_editor_contributions()));
        }

        let gutter = {
            let (features, context, options, sash_layout, boundary_sashes) =
                (features.clone(), feature_context.clone(), options.clone(), sash_layout.clone(), boundary_sashes.clone());
            derived_optional_disposable_of(Some(owner.clone()), move |reader| {
                if options.should_render_gutter_menu.read(reader) {
                    Some(features.create_gutter(&context, sash_layout.clone(), boundary_sashes.clone()))
                } else {
                    None
                }
            })
        };
        gutter_slot.set(gutter).ok();

        store.add(recompute_initially_and_on_change(&layout_info, None));

        {
            let (features, context, layout_info, moved_blocks_lines_part) =
                (features.clone(), feature_context.clone(), layout_info.clone(), moved_blocks_lines_part.clone());
            derived_disposable_of(Some(owner.clone()), move |_reader| {
                // MovedBlocksLinesPart
                features.create_moved_blocks_lines(&context, layout_info.map(|i, _| i.original_editor), layout_info.map(|i, _| i.modified_editor))
            })
            .recompute_initially_and_on_change(
                &store,
                Some(Rc::new(move |value: &Rc<dyn IMovedBlocksLinesFeature>| {
                    // This is to break the layout info <-> moved blocks lines part dependency cycle.
                    moved_blocks_lines_part.set(Some(value.clone()), None);
                })),
            );
        }

        for (editor, is_modified_editor) in [(&editors.modified, true), (&editors.original, false)] {
            let (diff_model, signals) = (diff_model.clone(), accessibility_signal_service.clone());
            store.add(editor.on_did_change_cursor_position().run_and_subscribe(
                move |e| Self::handle_cursor_position_change(&diff_model, &*signals, e, is_modified_editor),
                None,
            ));
        }

        let is_initializing_diff: IObservable<Option<bool>> =
            diff_model.map(|m, reader| m.map(|m| m.diff.read(reader).is_none() && !m.is_diff_up_to_date.read(reader)));
        store.add(autorun_with_store(move |reader, store| {
            // DiffEditorWidgetHelper.ShowProgress
            if is_initializing_diff.read(reader) == Some(true) {
                let runner = editor_progress_service.show(ProgressTotal::Infinite, Some(1000));
                store.add(to_disposable(move || runner.done()));
            }
        }));

        {
            let (features, context) = (features.clone(), feature_context.clone());
            store.add(autorun_with_store(move |_reader, store| {
                store.add(features.create_revert_buttons(&context));
            }));
        }

        {
            let (diff_model, me) = (diff_model.clone(), me.clone());
            store.add(autorun_with_store(move |reader, store| {
                let Some(model) = diff_model.read(reader) else { return };
                for text_model in [&model.model.original, &model.model.modified] {
                    let me = me.clone();
                    store.add(text_model.on_will_dispose().listen(move |_| {
                        errors::on_unexpected_error(&Error::bug_indicating(Some("TextModel got disposed before DiffEditorWidget model got reset")));
                        if let Some(widget) = me.upgrade() {
                            DelegatingEditor::set_model(&*widget, None);
                        }
                    }));
                }
            }));
        }

        {
            let (options, diff_model) = (options.clone(), diff_model.clone());
            store.add(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                options.set_model(diff_model.read(reader));
            }));
        }

        Self {
            me: me.clone(),
            delegating,
            store,
            ui,
            dom_node,
            elements,
            diff_model_src,
            diff_model,
            deferred_diff_model_ref_disposals,
            all_unchanged_regions_shown,
            on_did_change_model,
            instantiation_service,
            accessibility_signal_service,
            root_size_observer,
            smooth_resize,
            boundary_sashes,
            accessible_diff_viewer,
            options,
            editors,
            on_did_update_diff,
        }
    }

    /// `_createDiffEditorContributions()`.
    fn create_diff_editor_contributions(self: &Rc<Self>) {
        let contributions = crate::editor_extensions::EditorExtensionsRegistry::instance().get_diff_editor_contributions();
        let editor: Rc<dyn Any> = self.clone();
        for description in contributions {
            match errors::catch(|| (description.ctor)(&editor)) {
                Ok(contribution) => {
                    let contribution: DisposableRef = contribution;
                    self.store.add(contribution);
                }
                Err(error) => errors::on_unexpected_error(&error),
            }
        }
    }

    /// `_handleCursorPositionChange(e, isModifiedEditor)`.
    fn handle_cursor_position_change(
        diff_model: &IObservable<Option<Rc<DiffEditorViewModel>>>,
        signals: &dyn IAccessibilitySignalService,
        e: Option<&ICursorPositionChangedEvent>,
        is_modified_editor: bool,
    ) {
        let Some(e) = e else { return };
        if e.reason != CursorChangeReason::Explicit {
            return;
        }
        let diff = diff_model.get().and_then(|m| m.diff.get()).and_then(|diff| {
            diff.mappings
                .iter()
                .find(|m| {
                    let mapping = &m.line_range_mapping;
                    if is_modified_editor { mapping.modified.contains(e.position.line_number) } else { mapping.original.contains(e.position.line_number) }
                })
                .cloned()
        });
        play_diff_line_signal(signals, diff.as_ref().map(|diff| &*diff.line_range_mapping));
    }

    /// Os dois editores de código e o que o widget observa deles (o `_editors`).
    pub fn editors(&self) -> &Rc<DiffEditorEditors> {
        &self.editors
    }

    /// O estado dos elementos que o pintor lê (ver as divergências do módulo).
    pub fn elements_state(&self) -> DiffEditorElementsState {
        *self.elements.borrow()
    }

    /// `getDomNode()` e `getContainerDomNode()`.
    pub fn get_dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// A árvore de UI onde o widget vive.
    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// `collapseUnchangedRegions`.
    pub fn collapse_unchanged_regions(&self) -> bool {
        self.options.hide_unchanged_regions.get()
    }

    /// `getViewWidth()`.
    pub fn get_view_width(&self) -> f64 {
        self.root_size_observer.width.get()
    }

    /// `handleInitialized()`.
    pub fn handle_initialized(&self) {
        self.editors.original.handle_initialized();
        self.editors.modified.handle_initialized();
    }

    /// `createViewModel(model)`.
    pub fn create_view_model(&self, model: DiffEditorModel) -> Rc<DiffEditorViewModel> {
        self.instantiation_service.create_instance::<DiffEditorViewModel>((model, self.options.clone()))
    }

    /// `setBoundarySashes(sashes)`.
    pub fn set_boundary_sashes(&self, sashes: Rc<BoundarySashes>) {
        self.boundary_sashes.set(Some(sashes), None);
    }

    /// `ignoreTrimWhitespace`.
    pub fn ignore_trim_whitespace(&self) -> bool {
        self.options.ignore_trim_whitespace.get()
    }

    /// `maxComputationTime`.
    pub fn max_computation_time(&self) -> f64 {
        self.options.max_computation_time_ms.get()
    }

    /// `renderSideBySide`.
    pub fn render_side_by_side(&self) -> bool {
        self.options.render_side_by_side.get()
    }

    /// `renderSideBySideInAutomaticMode`.
    pub fn render_side_by_side_in_automatic_mode(&self) -> IObservable<bool> {
        self.options.render_side_by_side_in_automatic_mode.clone()
    }

    /// `temporaryInlineMode`.
    pub fn temporary_inline_mode(&self) -> IObservable<bool> {
        self.options.temporary_inline_mode.clone()
    }

    /// `resetWidthBasedLayout()`.
    pub fn reset_width_based_layout(&self) {
        self.options.reset_width_based_layout(None);
    }

    /// O `resize` da janela (o ouvinte do `targetWindow`).
    pub fn on_window_resize(&self) {
        self.smooth_resize.on_window_resize();
    }

    /// O `mousedown` e o `touchstart` da janela (fase de captura).
    pub fn on_pointer_down(&self) {
        self.smooth_resize.on_pointer_down();
    }

    /// O `mouseup`, o `touchend` e o `touchcancel` da janela (fase de captura).
    pub fn on_pointer_up(&self) {
        self.smooth_resize.on_pointer_up();
    }

    /// `getLineChanges()` (`@deprecated`: use `getDiffComputationResult().changes2`).
    pub fn get_line_changes(&self) -> Option<Vec<LineChange>> {
        let diff_state = self.diff_model.get()?.diff.get()?;
        Some(to_line_changes(&diff_state))
    }

    /// `getDiffComputationResult()`.
    pub fn get_diff_computation_result(&self) -> Option<DiffComputationResult> {
        let diff_state = self.diff_model.get()?.diff.get()?;
        Some(DiffComputationResult {
            changes: to_line_changes(&diff_state),
            changes2: diff_state.mappings.iter().map(|m| m.line_range_mapping.clone()).collect(),
            identical: diff_state.identical,
            quit_early: diff_state.quit_early,
        })
    }

    /// `executeEdits('diffEditor', edits)` do modificado, entre dois `pushUndoStop`.
    fn execute_edits_in_modified(&self, edits: Vec<SingleEditOperation>) {
        self.editors.modified.push_undo_stop();
        self.editors.modified.execute_edits(ExecuteEditsSource::Name(Some("diffEditor")), edits, None);
        self.editors.modified.push_undo_stop();
    }

    /// `revert(diff)`.
    pub fn revert(&self, diff: &LineRangeMapping) {
        let Some(model) = self.diff_model.get() else { return };
        if !model.is_diff_up_to_date.get() {
            return;
        }
        self.execute_edits_in_modified(vec![SingleEditOperation::new(
            diff.modified.to_exclusive_range(),
            Some(model.model.original.get_value_in_range(&diff.original.to_exclusive_range())),
            false,
        )]);
    }

    /// `revertRangeMappings(diffs)`.
    pub fn revert_range_mappings(&self, diffs: &[RangeMapping]) {
        let Some(model) = self.diff_model.get() else { return };
        if !model.is_diff_up_to_date.get() {
            return;
        }
        let changes = diffs.iter().map(|c| SingleEditOperation::new(c.modified_range, Some(model.model.original.get_value_in_range(&c.original_range)), false)).collect();
        self.execute_edits_in_modified(changes);
    }

    /// `revertFocusedRangeMappings()`.
    pub fn revert_focused_range_mappings(&self) {
        let Some(model) = self.diff_model.get() else { return };
        if !model.is_diff_up_to_date.get() {
            return;
        }

        let Some(diffs) = model.diff.get().map(|diff| diff.mappings.clone()) else { return };
        if diffs.is_empty() {
            return;
        }

        let modified_editor = &self.editors.modified;
        if !modified_editor.has_text_focus() {
            return;
        }

        let cur_line_number = modified_editor.get_position().expect("o editor modificado tem posição").line_number;
        let selected_range = LineRange::from_range(&modified_editor.get_selection().map_or(Range::new(cur_line_number, 0, cur_line_number, 0), |selection| *selection));
        let diffs_to_revert = diffs.iter().filter(|d| d.line_range_mapping.modified.intersect(&selected_range).is_some());

        let edits = diffs_to_revert
            .map(|d| {
                SingleEditOperation::new(
                    d.line_range_mapping.modified.to_exclusive_range(),
                    Some(model.model.original.get_value_in_range(&d.line_range_mapping.original.to_exclusive_range())),
                    false,
                )
            })
            .collect();
        self.execute_edits_in_modified(edits);
    }

    /// `_goTo(diff)`.
    fn go_to(&self, diff: &DiffMapping) {
        self.editors.modified.set_position(Position::new(diff.line_range_mapping.modified.start_line_number, 1), None);
        self.editors.modified.reveal_range_in_center(diff.line_range_mapping.modified.to_exclusive_range(), None);
    }

    /// `goToDiff(target)`.
    pub fn go_to_diff(&self, target: DiffNavigationTarget) {
        let Some(diffs) = self.diff_model.get().and_then(|m| m.diff.get()).map(|diff| diff.mappings.clone()) else { return };
        if diffs.is_empty() {
            return;
        }

        let cur_line_number = self.editors.modified.get_position().expect("o editor modificado tem posição").line_number;
        let diff = match target {
            DiffNavigationTarget::Next => {
                let modified_line_count = self.editors.modified.get_model().expect("o editor modificado tem modelo").get_line_count();
                if modified_line_count == cur_line_number {
                    &diffs[0]
                } else {
                    diffs.iter().find(|d| d.line_range_mapping.modified.start_line_number > cur_line_number).unwrap_or(&diffs[0])
                }
            }
            DiffNavigationTarget::Previous => {
                diffs.iter().rev().find(|d| d.line_range_mapping.modified.start_line_number < cur_line_number).unwrap_or(&diffs[diffs.len() - 1])
            }
        };
        self.go_to(diff);

        play_diff_line_signal(&*self.accessibility_signal_service, Some(&*diff.line_range_mapping));
    }

    /// `revealFirstDiff()`.
    pub fn reveal_first_diff(&self) {
        let Some(diff_model) = self.diff_model.get() else { return };
        // wait for the diff computation to finish
        let me = self.me.clone();
        let _ = self.wait_for_diff().then(move |()| {
            let Some(diffs) = diff_model.diff.get().map(|diff| diff.mappings.clone()) else { return };
            if diffs.is_empty() {
                return;
            }
            if let Some(widget) = me.upgrade() {
                widget.go_to(&diffs[0]);
            }
        });
    }

    /// `accessibleDiffViewerNext()`.
    pub fn accessible_diff_viewer_next(&self) {
        self.accessible_diff_viewer.get().next();
    }

    /// `accessibleDiffViewerPrev()`.
    pub fn accessible_diff_viewer_prev(&self) {
        self.accessible_diff_viewer.get().prev();
    }

    /// `waitForDiff()`.
    pub fn wait_for_diff(&self) -> Promise<()> {
        match self.diff_model.get() {
            None => Promise::resolved(()),
            Some(diff_model) => diff_model.wait_for_diff(),
        }
    }

    /// `mapToOtherSide()`.
    pub fn map_to_other_side(&self) -> MapToOtherSide {
        let is_modified_focus = self.editors.modified.has_widget_focus();
        let (source, destination) =
            if is_modified_focus { (&self.editors.modified, &self.editors.original) } else { (&self.editors.original, &self.editors.modified) };

        let mut destination_selection: Option<Range> = None;

        if let Some(source_selection) = source.get_selection() {
            let mappings = self.diff_model.get().and_then(|m| m.diff.get()).map(|diff| {
                diff.mappings
                    .iter()
                    .map(|m| if is_modified_focus { m.line_range_mapping.flip() } else { m.line_range_mapping.clone() })
                    .collect::<Vec<DetailedLineRangeMapping>>()
            });
            if let Some(mappings) = mappings {
                let new_range1 = translate_position(source_selection.get_start_position(), &mappings);
                let new_range2 = translate_position(source_selection.get_end_position(), &mappings);
                destination_selection = Some(new_range1.plus_range(&new_range2));
            }
        }
        MapToOtherSide { destination: destination.clone(), destination_selection }
    }

    /// `switchSide()`.
    pub fn switch_side(&self) {
        let MapToOtherSide { destination, destination_selection } = self.map_to_other_side();
        destination.focus();
        if let Some(destination_selection) = destination_selection {
            destination.set_selection(SelectionOrRange::Range(destination_selection), None);
        }
    }

    /// `exitCompareMove()`.
    pub fn exit_compare_move(&self) {
        let Some(model) = self.diff_model.get() else { return };
        model.moved_text_to_compare.set(None, None);
    }

    /// `collapseAllUnchangedRegions()`.
    pub fn collapse_all_unchanged_regions(&self) {
        let Some(model) = self.diff_model.get() else { return };
        let unchanged_regions = model.unchanged_regions.get();
        transaction(|tx| {
            for region in unchanged_regions.iter() {
                region.collapse_all(Some(tx));
            }
        });
    }

    /// `showAllUnchangedRegions()`.
    pub fn show_all_unchanged_regions(&self) {
        let Some(model) = self.diff_model.get() else { return };
        let unchanged_regions = model.unchanged_regions.get();
        transaction(|tx| {
            for region in unchanged_regions.iter() {
                region.show_all(Some(tx));
            }
        });
    }

    /// O `set` do `DisposableObservableValue`: o valor antigo é descartado antes de o novo ser gravado.
    fn set_diff_model_src(&self, value: Option<Rc<RefCounted<Rc<DiffEditorViewModel>>>>, tx: Option<&dyn ITransaction>) {
        let old = self.diff_model_src.get();
        if old.strict_equals(&value) {
            return;
        }
        if let Some(old) = old {
            old.dispose();
        }
        self.diff_model_src.set(value, tx);
    }

    /// O corpo de `subtransaction(tx, tx => { ... })` do `setDiffModel`.
    fn apply_diff_model(
        &self,
        view_model: Option<Rc<RefCounted<Rc<DiffEditorViewModel>>>>,
        current_model: &Option<Rc<DiffEditorViewModel>>,
        tx: &dyn ITransaction,
        batch_transaction: Option<Rc<dyn ITransaction>>,
    ) {
        let vm = view_model.as_ref().map(|view_model| view_model.object().clone());
        let model_pointer = |model: &Option<Rc<DiffEditorViewModel>>, original: bool| {
            model.as_ref().map(|m| Rc::as_ptr(if original { &m.model.original } else { &m.model.modified }))
        };
        if model_pointer(current_model, true) != model_pointer(&vm, true) || model_pointer(current_model, false) != model_pointer(&vm, false) {
            self.options.reset_width_based_layout(Some(tx));
        }

        // DiffEditorWidget.setModel
        let set_models = || {
            self.editors.original.set_model(vm.as_ref().map(|vm| vm.model.original.clone()));
            self.editors.modified.set_model(vm.as_ref().map(|vm| vm.model.modified.clone()));
        };
        match batch_transaction {
            Some(batch_transaction) => batch_events_globally(batch_transaction, set_models),
            None => set_models(),
        }

        let prev_value_ref = self.diff_model_src.get().map(|reference| reference.create_new_ref());
        self.set_diff_model_src(view_model.map(|view_model| view_model.create_new_ref()), Some(tx));
        if let Some(prev_value_ref) = prev_value_ref {
            let prev_value_ref: DisposableRef = prev_value_ref;
            self.deferred_diff_model_ref_disposals.add(prev_value_ref.clone());
            let disposals = self.deferred_diff_model_ref_disposals.clone();
            // async, so that this runs after the transaction finished.
            let _ = set_timeout(move || disposals.delete(&prev_value_ref), 0);
        }
    }
}

impl IDisposable for DiffEditorWidget {
    fn dispose(&self) {
        self.delegating.dispose();
    }
}

impl DelegatingEditor for DiffEditorWidget {
    fn delegating_base(&self) -> &DelegatingEditorBase {
        &self.delegating
    }

    fn target_editor(&self) -> Rc<CodeEditorWidget> {
        self.editors.modified.clone()
    }

    fn get_editor_type(&self) -> &'static str {
        EDITOR_TYPE_IDIFF_EDITOR
    }

    /// `updateOptions(changedOptions)`: `changedOptions` só traz as opções de nível superior que mudaram.
    fn update_options(&self, new_options: &Dynamic) {
        self.options.update_options(new_options);
    }

    fn on_visible(&self) {
        // TODO: Only compute diffs when diff editor is visible
        // O `onVisible` dos dois editores de código só atualiza o foco da `View` (ver as divergências do módulo).
    }

    fn on_hide(&self) {}

    fn layout(&self, dimension: Option<Dimension>) {
        self.root_size_observer.element_size_observer.observe(dimension);
    }

    fn has_text_focus(&self) -> bool {
        ICodeEditor::has_text_focus(&*self.editors.original) || ICodeEditor::has_text_focus(&*self.editors.modified)
    }

    fn save_view_state(&self) -> Option<EditorViewState> {
        Some(EditorViewState::Diff(DiffEditorViewState {
            original: self.editors.original.save_view_state(),
            modified: self.editors.modified.save_view_state(),
            model_state: self.diff_model.get().map(|m| serialized_state_to_dynamic(&m.serialize_state())),
        }))
    }

    fn restore_view_state(&self, state: Option<&EditorViewState>) {
        let Some(EditorViewState::Diff(diff_editor_state)) = state else { return };
        if let (Some(original), Some(modified)) = (&diff_editor_state.original, &diff_editor_state.modified) {
            self.editors.original.restore_view_state(Some(original));
            self.editors.modified.restore_view_state(Some(modified));
            if let Some(model_state) = &diff_editor_state.model_state
                && model_state.is_truthy()
                && let Some(model) = self.diff_model.get()
            {
                model.restore_serialized_state(&serialized_state_from_dynamic(model_state));
            }
        }
    }

    fn get_model(&self) -> Option<EditorModel> {
        self.diff_model.get().map(|m| EditorModel::Diff(m.model.clone()))
    }

    fn set_model(&self, model: Option<EditorModel>) {
        let view_model = match model {
            None => None,
            Some(EditorModel::DiffViewModel(view_model)) => {
                let view_model: Rc<dyn Any> = view_model;
                let view_model = view_model
                    .downcast::<DiffEditorViewModel>()
                    .unwrap_or_else(|_| errors::throw(Error::bug_indicating(Some("o IDiffEditorViewModel não é um DiffEditorViewModel"))));
                Some(RefCounted::create(view_model).create_new_ref())
            }
            Some(EditorModel::Diff(model)) => Some(RefCounted::create(self.create_view_model(model))),
            Some(EditorModel::Text(_)) => errors::throw(Error::bug_indicating(Some("o DiffEditorWidget não aceita um ITextModel"))),
        };
        IDiffEditorWidget::set_diff_model(self, view_model, None);
    }
}

impl EmbeddedDiffEditorBase for DiffEditorWidget {
    fn update_options(&self, new_options: &Dynamic) {
        DelegatingEditor::update_options(self, new_options);
    }
}

impl IDiffEditorWidget for DiffEditorWidget {
    fn get_original_editor(&self) -> Rc<dyn IDiffSideEditor> {
        Rc::new(DiffSideEditor(self.editors.original.clone()))
    }

    fn get_modified_editor(&self) -> Rc<dyn IDiffSideEditor> {
        Rc::new(DiffSideEditor(self.editors.modified.clone()))
    }

    fn on_did_content_size_change(&self) -> Event<ContentSizeChangedEvent> {
        self.editors.on_did_content_size_change()
    }

    /// `setDiffModel(viewModel, tx)`.
    fn set_diff_model(&self, view_model: Option<Rc<RefCounted<Rc<DiffEditorViewModel>>>>, tx: Option<&dyn ITransaction>) {
        let current_model = self.diff_model.get();

        if view_model.is_none() && current_model.is_some() {
            // Transitioning from a model to no-model
            self.accessible_diff_viewer.get().close();
        }

        let new_object = view_model.as_ref().map(|view_model| view_model.object().clone());
        if current_model.strict_equals(&new_object) {
            return;
        }

        // `subtransaction(tx, ...)`: sem `tx` o widget cria a transação (ver as divergências do módulo).
        let own_transaction = if tx.is_none() { Some(Rc::new(TransactionImpl::new())) } else { None };
        let (tx_ref, batch_transaction): (&dyn ITransaction, Option<Rc<dyn ITransaction>>) = match (tx, &own_transaction) {
            (Some(tx), _) => (tx, None),
            (None, Some(own)) => (&**own, Some(own.clone() as Rc<dyn ITransaction>)),
            (None, None) => errors::throw(Error::bug_indicating(None)),
        };
        try_finally(
            || self.apply_diff_model(view_model, &current_model, tx_ref, batch_transaction),
            || {
                if let Some(own) = &own_transaction {
                    own.finish();
                }
            },
        );
    }

    fn update_options(&self, new_options: &Dynamic) {
        DelegatingEditor::update_options(self, new_options);
    }

    fn layout(&self, width: f64, height: f64) {
        DelegatingEditor::layout(self, Some(Dimension { width, height }));
    }

    fn get_content_height(&self) -> f64 {
        self.editors.get_content_height()
    }

    fn all_unchanged_regions_shown(&self) -> IObservable<bool> {
        self.all_unchanged_regions_shown.clone()
    }

    fn focus(&self) {
        self.target_editor().focus();
    }
}

/// O `ICodeEditor` de um dos lados do diff, no recorte que o editor de vários arquivos lê (ver `diff_editor_surface`). É um tipo
/// próprio, e não o `IDiffSideEditor` implementado no `CodeEditorWidget`, porque vários métodos do recorte têm o nome dos do
/// `ICodeEditor` (`get_layout_info`, `get_content_height`...) e os dois traits no mesmo escopo ficariam ambíguos.
pub struct DiffSideEditor(pub Rc<CodeEditorWidget>);

impl IDiffSideEditor for DiffSideEditor {
    fn on_did_layout_change(&self) -> Event<EditorLayoutInfo> {
        self.0.on_did_layout_change()
    }

    fn get_layout_info(&self) -> EditorLayoutInfo {
        self.0.get_layout_info()
    }

    fn get_content_width(&self) -> f64 {
        self.0.get_content_width()
    }

    fn get_content_height(&self) -> f64 {
        self.0.get_content_height()
    }

    fn get_top_for_line_number(&self, line_number: i32) -> f64 {
        self.0.get_top_for_line_number(line_number, None)
    }

    fn get_line_count(&self) -> i32 {
        self.0.get_model().expect("o editor do diff tem modelo").get_line_count()
    }

    fn set_scroll_left(&self, left: f64) {
        self.0.set_scroll_left(left, None);
    }

    fn set_scroll_top(&self, top: f64) {
        self.0.set_scroll_top(top, None);
    }

    fn is_focused(&self) -> IObservable<bool> {
        observable_code_editor(&self.0).is_focused.clone()
    }
}
