//! Porte de `src/vs/base/browser/ui/sash/sash.ts`: o [`Sash`], a linha (quase sempre invisível) que o
//! usuário arrasta para redimensionar o que está dos dois lados.
//!
//! Do original ficam a orientação, os estados ([`SashState`]), o tamanho global e individual, o atraso do
//! hover, os eventos `onDidStart`/`onDidChange`/`onDidEnd`/`onDidReset` (duplo clique), as alças dos
//! sashes ortogonais (`orthogonalStartSash`/`orthogonalEndSash`, arrastar dois sashes de uma vez pelo
//! canto), o sash ligado (`linkedSash`, o 2x2 do grid) e os arrendamentos de classe (`addClass`).
//!
//! Como o DOM vira [`je_ui`]:
//!
//! - O elemento `.monaco-sash` é um nó da árvore, com as mesmas classes (`vertical`, `horizontal`,
//!   `disabled`, `minimum`, `maximum`, `hover`, `active`, `mac`, `orthogonal-edge-*`), lidas com
//!   [`Sash::has_class`]. A regra de CSS vira [`Sash::refresh_style`]: `pointer-events: none` quando
//!   desabilitado, o cursor de cada estado e o `:before` (um nó filho) pintado com a cor de hover.
//! - `width: 100%` e `height: 100%` do CSS são [`je_ui::Tree::set_fill`]; `right`/`bottom` das alças são
//!   [`je_ui::Tree::set_right`] e [`je_ui::Tree::set_bottom`].
//! - O `mousemove`/`mouseup` na `window` é um ouvinte de borbulha na raiz da árvore. O `<style>` que
//!   troca o cursor da janela inteira durante o arraste é [`je_ui::Ui::add_cursor_override`].
//! - `setTimeout`, `Delayer` e `Emitter` são os de `je-base` (mesma ordem do event loop).
//!
//! Fica de fora, por enquanto: o toque (`Gesture`, `EventType.Start`/`Tap`: o duplo toque que reseta e o
//! arraste por gesto) e o `iframe` que ganha `pointer-events: none` durante o arraste (as webviews são da
//! Fase 6). O `--vscode-sash-hoverBorder` do tema entra por [`set_global_sash_hover_border`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::Delayer;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::MaybePromise;
use je_base::symbols::Delay;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Cursor, EventKind, NodeId, PointerData, PointerEvents, Role, Ui};

/// Se o sistema é o macOS (o `isMacintosh`).
// TODO(integração): ler do `je_base::platform` quando houver a plataforma global.
pub(crate) fn is_macintosh() -> bool {
    cfg!(target_os = "macos")
}

/// Uma configuração global que avisa quem se inscreveu (o `Emitter` de módulo do original). Não é um
/// `Emitter` porque o rastreador de vazamento do `je-base` veria um disposable criado dentro de um teste.
type Subscriber<T> = (u64, Rc<dyn Fn(T)>);

struct GlobalSetting<T: Copy + 'static> {
    value: Cell<T>,
    subscribers: RefCell<Vec<Subscriber<T>>>,
    next: Cell<u64>,
}

impl<T: Copy + 'static> GlobalSetting<T> {
    fn new(value: T) -> Self {
        Self { value: Cell::new(value), subscribers: RefCell::new(Vec::new()), next: Cell::new(0) }
    }

    fn set(&self, value: T) {
        self.value.set(value);
        let subscribers: Vec<_> = self.subscribers.borrow().iter().map(|(_, f)| f.clone()).collect();
        for subscriber in subscribers {
            subscriber(value);
        }
    }
}

thread_local! {
    static GLOBAL_SIZE: GlobalSetting<f64> = GlobalSetting::new(4.0);
    static GLOBAL_HOVER_DELAY: GlobalSetting<u64> = GlobalSetting::new(300);
    static GLOBAL_HOVER_SIZE: GlobalSetting<f64> = GlobalSetting::new(4.0);
    static GLOBAL_HOVER_BORDER: Cell<Option<Rgba>> = const { Cell::new(None) };
}

fn subscribe<T: Copy + 'static>(setting: &'static std::thread::LocalKey<GlobalSetting<T>>, f: impl Fn(T) + 'static) -> DisposableRef {
    let id = setting.with(|s| {
        let id = s.next.get();
        s.next.set(id + 1);
        s.subscribers.borrow_mut().push((id, Rc::new(f)));
        id
    });
    to_disposable(move || setting.with(|s| s.subscribers.borrow_mut().retain(|(i, _)| *i != id)))
}

/// `setGlobalSashSize`: o tamanho dos sashes que não definem o próprio.
pub fn set_global_sash_size(size: f64) {
    GLOBAL_SIZE.with(|s| s.set(size));
}

/// `setGlobalHoverDelay`: o atraso, em milissegundos, até o sash mostrar o destaque de hover.
pub fn set_global_hover_delay(delay: u64) {
    GLOBAL_HOVER_DELAY.with(|s| s.set(delay));
}

/// O `--vscode-sash-hover-size`: a largura do destaque (o `:before`) do sash.
pub fn set_global_sash_hover_size(size: f64) {
    GLOBAL_HOVER_SIZE.with(|s| s.set(size));
}

/// O `--vscode-sash-hoverBorder`: a cor do destaque de hover e de arraste. Sem cor, o destaque é
/// transparente (a variável de CSS indefinida).
pub fn set_global_sash_hover_border(color: Option<Rgba>) {
    GLOBAL_HOVER_BORDER.with(|c| c.set(color));
}

/// `this._register(new Emitter<T>())`.
pub(crate) fn registered_emitter<T: 'static>(base: &Rc<Disposable>) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

/// `this._register(new DisposableStore())`.
pub(crate) fn registered_store(base: &Rc<Disposable>) -> Rc<DisposableStore> {
    let store = DisposableStore::new();
    base.register(store.clone());
    store
}

/// O provedor de layout de um sash vertical: posição e altura.
pub trait VerticalSashLayoutProvider {
    fn get_vertical_sash_left(&self, sash: &Sash) -> f64;
    fn get_vertical_sash_top(&self, _sash: &Sash) -> Option<f64> {
        None
    }
    fn get_vertical_sash_height(&self, _sash: &Sash) -> Option<f64> {
        None
    }
}

/// O provedor de layout de um sash horizontal: posição e largura.
pub trait HorizontalSashLayoutProvider {
    fn get_horizontal_sash_top(&self, sash: &Sash) -> f64;
    fn get_horizontal_sash_left(&self, _sash: &Sash) -> Option<f64> {
        None
    }
    fn get_horizontal_sash_width(&self, _sash: &Sash) -> Option<f64> {
        None
    }
}

/// O `{ getVerticalSashLeft: () => 0 }` do original: uma função só, a posição.
impl<F: Fn(&Sash) -> f64> VerticalSashLayoutProvider for F {
    fn get_vertical_sash_left(&self, sash: &Sash) -> f64 {
        self(sash)
    }
}

impl<F: Fn(&Sash) -> f64> HorizontalSashLayoutProvider for F {
    fn get_horizontal_sash_top(&self, sash: &Sash) -> f64 {
        self(sash)
    }
}

#[derive(Clone)]
enum LayoutProvider {
    Vertical(Rc<dyn VerticalSashLayoutProvider>),
    Horizontal(Rc<dyn HorizontalSashLayoutProvider>),
}

/// O `ISashEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SashEvent {
    pub start_x: f64,
    pub current_x: f64,
    pub start_y: f64,
    pub current_y: f64,
    pub alt_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrthogonalEdge {
    North,
    South,
    East,
    West,
}

impl OrthogonalEdge {
    fn class_name(self) -> &'static str {
        match self {
            Self::North => "orthogonal-edge-north",
            Self::South => "orthogonal-edge-south",
            Self::East => "orthogonal-edge-east",
            Self::West => "orthogonal-edge-west",
        }
    }
}

/// O `IBoundarySashes`.
#[derive(Clone, Default)]
pub struct BoundarySashes {
    pub top: Option<Rc<Sash>>,
    pub right: Option<Rc<Sash>>,
    pub bottom: Option<Rc<Sash>>,
    pub left: Option<Rc<Sash>>,
}

impl BoundarySashes {
    /// A igualdade por identidade de cada sash (o `===` do original).
    pub fn same_as(&self, other: &Self) -> bool {
        fn same(a: &Option<Rc<Sash>>, b: &Option<Rc<Sash>>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        }
        same(&self.top, &other.top) && same(&self.right, &other.right) && same(&self.bottom, &other.bottom) && same(&self.left, &other.left)
    }
}

/// O `Orientation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Vertical,
    Horizontal,
}

/// O `SashState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SashState {
    /// Sem interação.
    Disabled,
    /// Só dá para arrastar para baixo ou para a direita.
    AtMinimum,
    /// Só dá para arrastar para cima ou para a esquerda.
    AtMaximum,
    /// Arraste livre.
    Enabled,
}

/// O `ISashOptions` (a orientação vem do construtor usado).
#[derive(Clone, Default)]
pub struct SashOptions {
    /// A largura (sash vertical) ou a altura (horizontal). Sem valor, vale o tamanho global.
    pub size: Option<f64>,
    pub orthogonal_start_sash: Option<Rc<Sash>>,
    pub orthogonal_end_sash: Option<Rc<Sash>>,
    pub orthogonal_edge: Option<OrthogonalEdge>,
}

/// O ponteiro durante o arraste (o `PointerEvent` interno do original).
#[derive(Clone, Copy)]
struct PointerEvent {
    page_x: f64,
    page_y: f64,
}

/// O ponteiro que inicia o arraste, com as marcas `__orthogonalSashEvent` e `__linkedSashEvent` que o
/// original pendura no objeto do evento para não reentrar.
struct StartEvent {
    page_x: f64,
    page_y: f64,
    alt_key: bool,
    target: NodeId,
    orthogonal_sash_event: Cell<bool>,
    linked_sash_event: Cell<bool>,
}

/// O `IPointerEventFactory`: de onde vêm os movimentos e o fim do arraste. `owner` é o que o `dispose` da
/// fábrica do original descarta; a fábrica ortogonal (`OrthogonalPointerEventFactory`) reaproveita os
/// eventos da outra e não descarta nada.
struct PointerStreams {
    on_pointer_move: BaseEvent<PointerEvent>,
    on_pointer_up: BaseEvent<PointerEvent>,
    owner: Option<DisposableRef>,
}

impl PointerStreams {
    /// `MouseEventFactory`: `mousemove` e `mouseup` na janela (a raiz da árvore).
    fn mouse(ui: &Ui) -> Self {
        let store = DisposableStore::new();
        let moved = store.add_rc(Emitter::<PointerEvent>::new());
        let up = store.add_rc(Emitter::<PointerEvent>::new());
        let root = ui.root();
        for (kind, emitter) in [(EventKind::PointerMove, moved.clone()), (EventKind::PointerUp, up.clone())] {
            store.add(ui.listen(root, kind, false, move |e| {
                if let Some(PointerData { x, y, .. }) = e.pointer {
                    emitter.fire(PointerEvent { page_x: x, page_y: y });
                }
            }));
        }
        Self { on_pointer_move: moved.event(), on_pointer_up: up.event(), owner: Some(store) }
    }

    /// `OrthogonalPointerEventFactory`.
    fn share(&self) -> Self {
        Self { on_pointer_move: self.on_pointer_move.clone(), on_pointer_up: self.on_pointer_up.clone(), owner: None }
    }
}

struct ClassLease {
    count: usize,
    remove_on_release: bool,
}

/// Posição e tamanho do nó do sash: o que o `style` do original guarda, campo a campo. `None` na
/// largura ou altura é o `100%` do CSS.
#[derive(Clone, Copy)]
struct Geometry {
    left: f64,
    top: f64,
    width: Option<f64>,
    height: Option<f64>,
}

/// O sash.
pub struct Sash {
    base: Rc<Disposable>,
    weak_self: Weak<Sash>,
    ui: Ui,
    el: NodeId,
    before: NodeId,
    layout_provider: LayoutProvider,
    orientation: Orientation,
    orthogonal_edge: Option<OrthogonalEdge>,
    size: Cell<f64>,
    geometry: Cell<Geometry>,
    hover_delay: Cell<u64>,
    hover_delayer: Rc<Delayer<()>>,
    state: Cell<SashState>,
    class_leases: RefCell<HashMap<String, ClassLease>>,
    on_did_enablement_change: Rc<Emitter<SashState>>,
    on_did_start: Rc<Emitter<SashEvent>>,
    on_did_change: Rc<Emitter<SashEvent>>,
    on_did_reset: Rc<Emitter<()>>,
    on_did_end: Rc<Emitter<()>>,
    /// Os arrastes em andamento, descartados no fim de cada um (ou com o sash).
    sessions: Rc<DisposableStore>,
    orthogonal_start_sash_disposables: Rc<DisposableStore>,
    orthogonal_start_sash: RefCell<Option<Rc<Sash>>>,
    orthogonal_start_drag_handle_disposables: Rc<DisposableStore>,
    orthogonal_start_drag_handle: Cell<Option<NodeId>>,
    orthogonal_end_sash_disposables: Rc<DisposableStore>,
    orthogonal_end_sash: RefCell<Option<Rc<Sash>>>,
    orthogonal_end_drag_handle_disposables: Rc<DisposableStore>,
    orthogonal_end_drag_handle: Cell<Option<NodeId>>,
    /// Fraco para o par do 2x2 não formar um ciclo de `Rc` (o original desfaz na mão, em `trySet2x2`).
    linked_sash: RefCell<Option<Weak<Sash>>>,
    disposed: Cell<bool>,
}

impl Sash {
    /// Cria um sash vertical. `container` é o nó onde o sash entra (o `container` do original).
    pub fn new_vertical(ui: &Ui, container: NodeId, provider: Rc<dyn VerticalSashLayoutProvider>, options: SashOptions) -> Rc<Sash> {
        Self::new(ui, container, LayoutProvider::Vertical(provider), options)
    }

    /// Cria um sash horizontal.
    pub fn new_horizontal(ui: &Ui, container: NodeId, provider: Rc<dyn HorizontalSashLayoutProvider>, options: SashOptions) -> Rc<Sash> {
        Self::new(ui, container, LayoutProvider::Horizontal(provider), options)
    }

    fn new(ui: &Ui, container: NodeId, layout_provider: LayoutProvider, options: SashOptions) -> Rc<Sash> {
        let orientation = match layout_provider {
            LayoutProvider::Vertical(_) => Orientation::Vertical,
            LayoutProvider::Horizontal(_) => Orientation::Horizontal,
        };
        let base = Disposable::new();
        let (el, before) = {
            let mut tree = ui.tree_mut();
            let el = tree.create_detached(Role::Separator);
            tree.append_child(container, el);
            tree.add_class(el, "monaco-sash");
            let before = tree.create(el, Role::Generic);
            tree.style_mut(before).pointer_events = PointerEvents::None;
            (el, before)
        };
        let global_size = GLOBAL_SIZE.with(|s| s.value.get());
        let size = options.size.unwrap_or(global_size);
        let hover_delay = GLOBAL_HOVER_DELAY.with(|s| s.value.get());
        let hover_delayer = Rc::new(Delayer::new(Delay::Millis(hover_delay)));
        base.register(hover_delayer.clone());
        let this = Rc::new_cyclic(|weak| Sash {
            weak_self: weak.clone(),
            ui: ui.clone(),
            el,
            before,
            layout_provider,
            orientation,
            orthogonal_edge: options.orthogonal_edge,
            size: Cell::new(size),
            geometry: Cell::new(match orientation {
                Orientation::Vertical => Geometry { left: 0.0, top: 0.0, width: Some(size), height: None },
                Orientation::Horizontal => Geometry { left: 0.0, top: 0.0, width: None, height: Some(size) },
            }),
            hover_delay: Cell::new(hover_delay),
            hover_delayer,
            state: Cell::new(SashState::Enabled),
            class_leases: RefCell::new(HashMap::new()),
            on_did_enablement_change: registered_emitter(&base),
            on_did_start: registered_emitter(&base),
            on_did_change: registered_emitter(&base),
            on_did_reset: registered_emitter(&base),
            on_did_end: registered_emitter(&base),
            sessions: registered_store(&base),
            orthogonal_start_sash_disposables: registered_store(&base),
            orthogonal_start_sash: RefCell::new(None),
            orthogonal_start_drag_handle_disposables: registered_store(&base),
            orthogonal_start_drag_handle: Cell::new(None),
            orthogonal_end_sash_disposables: registered_store(&base),
            orthogonal_end_sash: RefCell::new(None),
            orthogonal_end_drag_handle_disposables: registered_store(&base),
            orthogonal_end_drag_handle: Cell::new(None),
            linked_sash: RefCell::new(None),
            disposed: Cell::new(false),
            base,
        });

        {
            let mut tree = ui.tree_mut();
            if let Some(edge) = options.orthogonal_edge {
                tree.add_class(el, edge.class_name());
            }
            if is_macintosh() {
                tree.add_class(el, "mac");
            }
            tree.add_class(el, if orientation == Orientation::Horizontal { "horizontal" } else { "vertical" });
        }

        let weak = this.weak_self.clone();
        let with_sash = move |f: fn(&Rc<Sash>, &mut je_ui::Event)| {
            let weak = weak.clone();
            move |e: &mut je_ui::Event| {
                if let Some(sash) = weak.upgrade() {
                    f(&sash, e);
                }
            }
        };
        this.base.register(ui.listen(el, EventKind::PointerDown, false, with_sash(|sash, e| {
            e.stop(true);
            if let Some(start) = StartEvent::from(e) {
                sash.on_pointer_start(&start, PointerStreams::mouse(&sash.ui));
            }
        })));
        this.base.register(ui.listen(el, EventKind::DoubleClick, false, with_sash(|sash, e| {
            sash.on_pointer_double_press(e.target);
        })));
        this.base.register(ui.listen(el, EventKind::PointerEnter, false, with_sash(|sash, _| Sash::on_mouse_enter(sash, false))));
        this.base.register(ui.listen(el, EventKind::PointerLeave, false, with_sash(|sash, _| Sash::on_mouse_leave(sash, false))));

        if options.size.is_none() {
            let weak = this.weak_self.clone();
            this.base.register(subscribe(&GLOBAL_SIZE, move |size| {
                if let Some(sash) = weak.upgrade() {
                    sash.size.set(size);
                    sash.layout();
                }
            }));
        }
        let weak = this.weak_self.clone();
        this.base.register(subscribe(&GLOBAL_HOVER_DELAY, move |delay| {
            if let Some(sash) = weak.upgrade() {
                sash.hover_delay.set(delay);
            }
        }));
        let weak = this.weak_self.clone();
        this.base.register(subscribe(&GLOBAL_HOVER_SIZE, move |_| {
            if let Some(sash) = weak.upgrade() {
                sash.layout();
            }
        }));

        this.set_orthogonal_start_sash(options.orthogonal_start_sash);
        this.set_orthogonal_end_sash(options.orthogonal_end_sash);
        this.refresh_style();
        this.layout();
        this
    }

    /// O nó do sash (o `.monaco-sash`).
    pub fn element(&self) -> NodeId {
        self.el
    }

    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// O estado: define se o usuário interage com o sash e qual cursor aparece.
    pub fn state(&self) -> SashState {
        self.state.get()
    }

    pub fn set_state(&self, state: SashState) {
        if self.state.get() == state {
            return;
        }
        {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(self.el, "disabled", state == SashState::Disabled);
            tree.toggle_class(self.el, "minimum", state == SashState::AtMinimum);
            tree.toggle_class(self.el, "maximum", state == SashState::AtMaximum);
        }
        self.state.set(state);
        self.refresh_style();
        self.on_did_enablement_change.fire(state);
    }

    /// Dispara quando o usuário começa a arrastar.
    pub fn on_did_start(&self) -> BaseEvent<SashEvent> {
        self.on_did_start.event()
    }

    /// Dispara a cada movimento do arraste.
    pub fn on_did_change(&self) -> BaseEvent<SashEvent> {
        self.on_did_change.event()
    }

    /// Dispara no duplo clique.
    pub fn on_did_reset(&self) -> BaseEvent<()> {
        self.on_did_reset.event()
    }

    /// Dispara quando o usuário solta o sash.
    pub fn on_did_end(&self) -> BaseEvent<()> {
        self.on_did_end.event()
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.ui.tree().has_class(self.el, class)
    }

    /// `addClass`: a classe fica enquanto o disposable devolvido não for descartado. Dois arrendamentos
    /// da mesma classe contam juntos, e uma classe que o sash já tinha antes nunca é removida.
    pub fn add_class(&self, class: &str) -> DisposableRef {
        {
            let mut leases = self.class_leases.borrow_mut();
            if let Some(lease) = leases.get_mut(class) {
                lease.count += 1;
            } else {
                let remove_on_release = !self.has_class(class);
                leases.insert(class.to_owned(), ClassLease { count: 1, remove_on_release });
                self.ui.tree_mut().add_class(self.el, class);
            }
        }
        let weak = self.weak_self.clone();
        let class = class.to_owned();
        to_disposable(move || {
            let Some(sash) = weak.upgrade() else { return };
            let mut leases = sash.class_leases.borrow_mut();
            match leases.get_mut(&class) {
                Some(lease) if lease.count == 1 => {
                    let lease = leases.remove(&class).expect("o arrendamento acabou de ser lido");
                    if lease.remove_on_release && !sash.disposed.get() {
                        sash.ui.tree_mut().remove_class(sash.el, &class);
                    }
                }
                Some(lease) => lease.count -= 1,
                None => {}
            }
        })
    }

    /// O sash ligado: recebe as mesmas interações e eventos, e se move igual. Útil no 2x2 do grid.
    pub fn linked_sash(&self) -> Option<Rc<Sash>> {
        self.linked_sash.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_linked_sash(&self, sash: Option<&Rc<Sash>>) {
        *self.linked_sash.borrow_mut() = sash.map(Rc::downgrade);
    }

    pub fn orthogonal_start_sash(&self) -> Option<Rc<Sash>> {
        self.orthogonal_start_sash.borrow().clone()
    }

    pub fn orthogonal_end_sash(&self) -> Option<Rc<Sash>> {
        self.orthogonal_end_sash.borrow().clone()
    }

    /// Um sash perpendicular a este que se alinha ao início dele (esquerda de um horizontal, topo de um
    /// vertical): um canto é criado ali.
    pub fn set_orthogonal_start_sash(&self, sash: Option<Rc<Sash>>) {
        self.set_orthogonal_sash(true, sash);
    }

    /// O mesmo para o fim (direita de um horizontal, base de um vertical).
    pub fn set_orthogonal_end_sash(&self, sash: Option<Rc<Sash>>) {
        self.set_orthogonal_sash(false, sash);
    }

    /// Os dois setters do original são o mesmo código com `start`/`end` trocados.
    fn set_orthogonal_sash(&self, start: bool, sash: Option<Rc<Sash>>) {
        let (slot, sash_disposables, handle_disposables, handle_slot) = if start {
            (&self.orthogonal_start_sash, &self.orthogonal_start_sash_disposables, &self.orthogonal_start_drag_handle_disposables, &self.orthogonal_start_drag_handle)
        } else {
            (&self.orthogonal_end_sash, &self.orthogonal_end_sash_disposables, &self.orthogonal_end_drag_handle_disposables, &self.orthogonal_end_drag_handle)
        };
        let unchanged = match (&*slot.borrow(), &sash) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        handle_disposables.clear();
        handle_slot.set(None);
        sash_disposables.clear();

        if let Some(sash) = &sash {
            let weak = self.weak_self.clone();
            let target = Rc::downgrade(sash);
            let on_change = Rc::new(move |state: SashState| {
                let Some(this) = weak.upgrade() else { return };
                let (handle_disposables, handle_slot) = if start {
                    (&this.orthogonal_start_drag_handle_disposables, &this.orthogonal_start_drag_handle)
                } else {
                    (&this.orthogonal_end_drag_handle_disposables, &this.orthogonal_end_drag_handle)
                };
                handle_disposables.clear();
                if state != SashState::Disabled {
                    let handle = {
                        let mut tree = this.ui.tree_mut();
                        let handle = tree.create(this.el, Role::Generic);
                        tree.add_class(handle, "orthogonal-drag-handle");
                        tree.add_class(handle, if start { "start" } else { "end" });
                        tree.style_mut(handle).z_index = 100;
                        tree.style_mut(handle).cursor = Some(this.handle_cursor(start));
                        handle
                    };
                    handle_slot.set(Some(handle));
                    this.layout_handle(start);
                    let ui = this.ui.clone();
                    handle_disposables.add(to_disposable(move || ui.remove(handle)));
                    let (enter, leave) = (target.clone(), target.clone());
                    handle_disposables.add(this.ui.listen(handle, EventKind::PointerEnter, false, move |_| {
                        if let Some(sash) = enter.upgrade() {
                            Sash::on_mouse_enter(&sash, false);
                        }
                    }));
                    handle_disposables.add(this.ui.listen(handle, EventKind::PointerLeave, false, move |_| {
                        if let Some(sash) = leave.upgrade() {
                            Sash::on_mouse_leave(&sash, false);
                        }
                    }));
                }
            });
            let listener = on_change.clone();
            sash_disposables.add(sash.on_did_enablement_change.event().listen(move |state| (*listener)(*state)));
            (*on_change)(sash.state());
        }
        *slot.borrow_mut() = sash;
    }

    /// O cursor da alça do canto: `all-scroll`, ou a diagonal de redimensionar quando o sash horizontal
    /// declara de que borda ele é.
    fn handle_cursor(&self, start: bool) -> Cursor {
        match (self.orientation, self.orthogonal_edge, start) {
            (Orientation::Horizontal, Some(OrthogonalEdge::North), true) | (Orientation::Horizontal, Some(OrthogonalEdge::South), false) => Cursor::NwseResize,
            (Orientation::Horizontal, Some(OrthogonalEdge::North), false) | (Orientation::Horizontal, Some(OrthogonalEdge::South), true) => Cursor::NeswResize,
            _ => Cursor::AllScroll,
        }
    }

    /// A geometria da alça: um quadrado de duas vezes o tamanho do sash, sobre a ponta dele.
    fn layout_handle(&self, start: bool) {
        let handle = if start { self.orthogonal_start_drag_handle.get() } else { self.orthogonal_end_drag_handle.get() };
        let Some(handle) = handle else { return };
        let size = self.size.get();
        let mut tree = self.ui.tree_mut();
        let side = size * 2.0;
        let (left, top) = match (self.orientation, start) {
            (Orientation::Vertical, true) => (Some(size * -0.5), Some(-size)),
            (Orientation::Vertical, false) => (Some(size * -0.5), None),
            (Orientation::Horizontal, true) => (Some(-size), Some(size * -0.5)),
            (Orientation::Horizontal, false) => (None, Some(size * -0.5)),
        };
        tree.set_bounds(handle, Rect::new(left.unwrap_or(0.0), top.unwrap_or(0.0), left.unwrap_or(0.0) + side, top.unwrap_or(0.0) + side));
        // `bottom` (sash vertical) e `right` (horizontal) do fim ancoram na borda de trás do sash.
        tree.set_bottom(handle, (self.orientation == Orientation::Vertical && !start).then_some(-size));
        tree.set_right(handle, (self.orientation == Orientation::Horizontal && !start).then_some(-size));
    }

    /// A regra de CSS do sash: `pointer-events`, cursor e o `:before` do hover e do arraste.
    pub fn refresh_style(&self) {
        let (cursor, hover) = {
            let tree = self.ui.tree();
            let state = self.state.get();
            let cursor = if state == SashState::Disabled {
                Cursor::Default
            } else {
                match (self.orientation, state) {
                    (Orientation::Vertical, SashState::AtMinimum) => Cursor::EResize,
                    (Orientation::Vertical, SashState::AtMaximum) => Cursor::WResize,
                    (Orientation::Vertical, _) if is_macintosh() => Cursor::ColResize,
                    (Orientation::Vertical, _) => Cursor::EwResize,
                    (Orientation::Horizontal, SashState::AtMinimum) => Cursor::SResize,
                    (Orientation::Horizontal, SashState::AtMaximum) => Cursor::NResize,
                    (Orientation::Horizontal, _) if is_macintosh() => Cursor::RowResize,
                    (Orientation::Horizontal, _) => Cursor::NsResize,
                }
            };
            (cursor, tree.has_class(self.el, "hover") || tree.has_class(self.el, "active"))
        };
        let color = if hover { GLOBAL_HOVER_BORDER.with(Cell::get) } else { None };
        let mut tree = self.ui.tree_mut();
        let style = tree.style_mut(self.el);
        style.pointer_events = if self.state.get() == SashState::Disabled { PointerEvents::None } else { PointerEvents::Auto };
        style.cursor = Some(cursor);
        style.z_index = 35;
        tree.style_mut(self.before).background = color;
    }

    fn set_class(&self, class: &str, on: bool) {
        self.ui.tree_mut().toggle_class(self.el, class, on);
        self.refresh_style();
    }

    fn on_pointer_start(self: &Rc<Self>, event: &StartEvent, factory: PointerStreams) {
        let mut is_multisash_resize = false;

        if !event.orthogonal_sash_event.get()
            && let Some(orthogonal) = self.get_orthogonal_sash(event.target) {
                is_multisash_resize = true;
                event.orthogonal_sash_event.set(true);
                orthogonal.on_pointer_start(event, factory.share());
            }

        if let Some(linked) = self.linked_sash()
            && !event.linked_sash_event.get() {
                event.linked_sash_event.set(true);
                linked.on_pointer_start(event, factory.share());
            }

        if self.state.get() == SashState::Disabled {
            return;
        }

        let (start_x, start_y, alt_key) = (event.page_x, event.page_y, event.alt_key);
        let start_event = SashEvent { start_x, current_x: start_x, start_y, current_y: start_y, alt_key };

        self.set_class("active", true);
        self.on_did_start.fire(start_event);

        // Correção de https://github.com/microsoft/vscode/issues/21675: o cursor vale para a janela toda.
        let cursor_id = self.ui.add_cursor_override(self.drag_cursor(is_multisash_resize));

        let disposables = DisposableStore::new();
        self.sessions.add(disposables.clone());

        if !is_multisash_resize {
            let (ui, weak) = (self.ui.clone(), self.weak_self.clone());
            disposables.add(self.on_did_enablement_change.event().listen(move |_| {
                if let Some(sash) = weak.upgrade() {
                    ui.update_cursor_override(cursor_id, sash.drag_cursor(false));
                }
            }));
        }

        let weak = self.weak_self.clone();
        disposables.add(factory.on_pointer_move.listen(move |e| {
            if let Some(sash) = weak.upgrade() {
                sash.on_did_change.fire(SashEvent { start_x, current_x: e.page_x, start_y, current_y: e.page_y, alt_key });
            }
        }));

        let (weak, session) = (self.weak_self.clone(), Rc::downgrade(&disposables));
        disposables.add(factory.on_pointer_up.listen(move |_| {
            let Some(sash) = weak.upgrade() else { return };
            sash.ui.remove_cursor_override(cursor_id);
            sash.set_class("active", false);
            sash.on_did_end.fire(());
            if let Some(session) = session.upgrade() {
                let session: DisposableRef = session;
                sash.sessions.delete(&session);
            }
        }));

        if let Some(owner) = factory.owner {
            disposables.add(owner);
        }
    }

    /// O cursor da janela durante o arraste.
    fn drag_cursor(&self, is_multisash_resize: bool) -> Cursor {
        if is_multisash_resize {
            return Cursor::AllScroll;
        }
        let state = self.state.get();
        match self.orientation {
            Orientation::Horizontal => match state {
                SashState::AtMinimum => Cursor::SResize,
                SashState::AtMaximum => Cursor::NResize,
                _ if is_macintosh() => Cursor::RowResize,
                _ => Cursor::NsResize,
            },
            Orientation::Vertical => match state {
                SashState::AtMinimum => Cursor::EResize,
                SashState::AtMaximum => Cursor::WResize,
                _ if is_macintosh() => Cursor::ColResize,
                _ => Cursor::EwResize,
            },
        }
    }

    fn on_pointer_double_press(&self, target: NodeId) {
        if let Some(orthogonal) = self.get_orthogonal_sash(target) {
            orthogonal.on_did_reset.fire(());
        }
        if let Some(linked) = self.linked_sash() {
            linked.on_did_reset.fire(());
        }
        self.on_did_reset.fire(());
    }

    fn on_mouse_enter(sash: &Rc<Sash>, from_linked_sash: bool) {
        if sash.has_class("active") {
            sash.hover_delayer.cancel();
            sash.set_class("hover", true);
        } else {
            let weak = sash.weak_self.clone();
            sash.hover_delayer
                .trigger(
                    move || {
                        if let Some(sash) = weak.upgrade() {
                            sash.set_class("hover", true);
                        }
                        MaybePromise::Value(())
                    },
                    Some(Delay::Millis(sash.hover_delay.get())),
                )
                .catch(|_| ());
        }
        if !from_linked_sash
            && let Some(linked) = sash.linked_sash() {
                Sash::on_mouse_enter(&linked, true);
            }
    }

    fn on_mouse_leave(sash: &Rc<Sash>, from_linked_sash: bool) {
        sash.hover_delayer.cancel();
        sash.set_class("hover", false);
        if !from_linked_sash
            && let Some(linked) = sash.linked_sash() {
                Sash::on_mouse_leave(&linked, true);
            }
    }

    /// Para qualquer interação com o sash à força. Útil ao esconder o componente pai enquanto o usuário
    /// ainda interage com o sash.
    pub fn clear_sash_hover_state(self: &Rc<Self>) {
        Sash::on_mouse_leave(self, false);
    }

    /// Posiciona e dimensiona o sash pelo provedor de layout.
    pub fn layout(&self) {
        let size = self.size.get();
        let mut geometry = self.geometry.get();
        match &self.layout_provider {
            LayoutProvider::Vertical(provider) => {
                geometry.width = Some(size);
                geometry.left = provider.get_vertical_sash_left(self) - size / 2.0;
                if let Some(top) = provider.get_vertical_sash_top(self) {
                    geometry.top = top;
                }
                if let Some(height) = provider.get_vertical_sash_height(self) {
                    geometry.height = Some(height);
                }
            }
            LayoutProvider::Horizontal(provider) => {
                geometry.height = Some(size);
                geometry.top = provider.get_horizontal_sash_top(self) - size / 2.0;
                if let Some(left) = provider.get_horizontal_sash_left(self) {
                    geometry.left = left;
                }
                if let Some(width) = provider.get_horizontal_sash_width(self) {
                    geometry.width = Some(width);
                }
            }
        }
        self.geometry.set(geometry);
        let hover_size = GLOBAL_HOVER_SIZE.with(|s| s.value.get());
        {
            let mut tree = self.ui.tree_mut();
            let (w, h) = (geometry.width.unwrap_or(0.0), geometry.height.unwrap_or(0.0));
            tree.set_bounds(self.el, Rect::new(geometry.left, geometry.top, geometry.left + w, geometry.top + h));
            tree.set_fill(self.el, geometry.width.is_none(), geometry.height.is_none());
            // O `:before`: `hover-size` de espessura, centrado na espessura do sash, cheio no comprimento.
            let (bounds, fill) = match self.orientation {
                Orientation::Vertical => (Rect::new(size / 2.0 - hover_size / 2.0, 0.0, size / 2.0 + hover_size / 2.0, 0.0), (false, true)),
                Orientation::Horizontal => (Rect::new(0.0, size / 2.0 - hover_size / 2.0, 0.0, size / 2.0 + hover_size / 2.0), (true, false)),
            };
            tree.set_bounds(self.before, bounds);
            tree.set_fill(self.before, fill.0, fill.1);
        }
        self.layout_handle(true);
        self.layout_handle(false);
    }

    fn get_orthogonal_sash(&self, target: NodeId) -> Option<Rc<Sash>> {
        let tree = self.ui.tree();
        if tree.has_class(target, "orthogonal-drag-handle") {
            if tree.has_class(target, "start") { self.orthogonal_start_sash() } else { self.orthogonal_end_sash() }
        } else {
            None
        }
    }
}

impl StartEvent {
    fn from(event: &je_ui::Event) -> Option<Self> {
        let pointer = event.pointer?;
        Some(Self { page_x: pointer.x, page_y: pointer.y, alt_key: pointer.modifiers.alt, target: event.target, orthogonal_sash_event: Cell::new(false), linked_sash_event: Cell::new(false) })
    }
}

impl IDisposable for Sash {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.base.dispose();
        self.ui.remove(self.el);
    }

    fn constructor_name(&self) -> &'static str {
        "Sash"
    }
}
