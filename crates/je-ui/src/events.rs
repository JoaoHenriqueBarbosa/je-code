//! Eventos da árvore de UI: o mínimo do modelo de eventos do DOM de que os widgets do VS Code dependem.
//!
//! O que existe, e por quê:
//!
//! - **[`Ui`]**: a árvore ([`Tree`]) mais a tabela de ouvintes e o estado de entrada (nó sob o ponteiro,
//!   captura do ponteiro, foco). É o que um widget guarda no lugar de um `document`: as callbacks de
//!   ponteiro precisam mexer na árvore (o `sash` posiciona nós ao arrastar), então a árvore fica atrás de
//!   um `RefCell` compartilhado e o despacho nunca segura um empréstimo enquanto chama um ouvinte.
//! - **Ouvintes com fase.** [`Ui::listen`] é o `addEventListener(tipo, fn, useCapture)`: ouvinte de
//!   captura roda da raiz até o pai do alvo, depois os do próprio alvo (fase [`Phase::Target`]), depois,
//!   se o evento borbulha, do pai do alvo até a raiz (ouvintes sem captura). [`Event::stop`] faz o
//!   `EventHelper.stop` do VS Code (`preventDefault` mais `stopPropagation`, a segunda opcional). Um
//!   ouvinte removido durante o despacho não é chamado, como no DOM.
//! - **Ponteiro.** [`Ui::pointer_input`] recebe a entrada crua da janela (botão baixo, movimento, botão
//!   alto, com coordenadas da janela e a contagem de cliques que o sistema operacional informa) e
//!   produz o que o DOM produziria: o alvo é o nó mais ao fundo sob o ponteiro ([`Tree::hit_test`]),
//!   `PointerEnter`/`PointerLeave` (que não borbulham) ao cruzar fronteiras de nós, `Click` no ancestral
//!   comum do alvo do botão baixo e do botão alto, e `DoubleClick` quando o botão alto traz
//!   `click_count == 2`.
//! - **Captura do ponteiro.** [`Ui::set_pointer_capture`] redireciona `PointerMove` e `PointerUp` para um
//!   nó, mesmo fora dele, até o botão subir (o `setPointerCapture` do DOM). O `sash` do VS Code escuta
//!   `mousemove` e `mouseup` na janela; aqui a janela é a raiz da árvore, então um ouvinte de borbulha
//!   na raiz ([`Ui::root`]) faz o mesmo.
//! - **Teclado e foco.** [`Ui::key_input`] entrega `KeyDown`/`KeyUp` ao nó com foco (ou à raiz).
//!   [`Ui::focus`] e [`Ui::blur`] despacham `Blur` e `Focus`, que não borbulham (o `trackFocus` do VS
//!   Code escuta os dois com captura).
//!
//! Fica de fora, por enquanto: toque e gesto (`Gesture`, `EventType.Tap`), arrastar e soltar do DOM, roda
//! do mouse, entrada de texto/IME e `PointerEvent` com vários ponteiros.

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableRef, to_disposable};

use crate::tree::{Cursor, NodeId, Tree};

/// O tipo de um evento (o `type` do DOM).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// `mousedown`/`pointerdown`.
    PointerDown,
    /// `mousemove`/`pointermove`.
    PointerMove,
    /// `mouseup`/`pointerup`.
    PointerUp,
    /// `mouseenter`: não borbulha.
    PointerEnter,
    /// `mouseleave`: não borbulha.
    PointerLeave,
    /// `click`.
    Click,
    /// `dblclick`.
    DoubleClick,
    /// `keydown`.
    KeyDown,
    /// `keyup`.
    KeyUp,
    /// `focus`: não borbulha.
    Focus,
    /// `blur`: não borbulha.
    Blur,
}

impl EventKind {
    fn bubbles(self) -> bool {
        !matches!(self, Self::PointerEnter | Self::PointerLeave | Self::Focus | Self::Blur)
    }

    /// `PointerEnter` e `PointerLeave` só chegam ao próprio nó, sem fase de captura (o `mouseenter` do DOM
    /// não tem fase de captura entre os antepassados).
    fn direct(self) -> bool {
        matches!(self, Self::PointerEnter | Self::PointerLeave)
    }
}

/// A fase em que o ouvinte está sendo chamado (o `eventPhase`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Capture,
    Target,
    Bubble,
}

/// As teclas modificadoras (`altKey`, `ctrlKey`, `shiftKey`, `metaKey`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub meta: bool,
}

/// O botão do ponteiro (o `button` do `MouseEvent`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerButton {
    #[default]
    Primary,
    Auxiliary,
    Secondary,
}

/// Os dados de um evento de ponteiro. `x` e `y` são coordenadas da janela (`pageX`/`pageY`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerData {
    pub x: f64,
    pub y: f64,
    pub button: PointerButton,
    pub modifiers: Modifiers,
    /// O `detail` do DOM: 1 no primeiro clique, 2 no segundo, e assim por diante.
    pub click_count: u32,
}

/// Os dados de um evento de teclado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyData {
    pub key_code: KeyCode,
    pub modifiers: Modifiers,
}

/// Um evento em despacho.
pub struct Event {
    pub kind: EventKind,
    /// O nó onde o evento nasceu (o `target`).
    pub target: NodeId,
    /// O nó cujo ouvinte está rodando (o `currentTarget`).
    pub current_target: NodeId,
    pub phase: Phase,
    pub pointer: Option<PointerData>,
    pub key: Option<KeyData>,
    default_prevented: bool,
    propagation_stopped: bool,
}

impl Event {
    pub fn new(kind: EventKind, target: NodeId) -> Self {
        Self { kind, target, current_target: target, phase: Phase::Target, pointer: None, key: None, default_prevented: false, propagation_stopped: false }
    }

    pub fn with_pointer(mut self, pointer: PointerData) -> Self {
        self.pointer = Some(pointer);
        self
    }

    pub fn with_key(mut self, key: KeyData) -> Self {
        self.key = Some(key);
        self
    }

    /// `preventDefault()`.
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }

    /// `stopPropagation()`: os ouvintes do nó atual terminam, os dos próximos nós não rodam.
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    /// `EventHelper.stop(event, cancelBubble)`: `preventDefault`, e `stopPropagation` se `cancel_bubble`.
    pub fn stop(&mut self, cancel_bubble: bool) {
        self.prevent_default();
        if cancel_bubble {
            self.stop_propagation();
        }
    }

    pub fn propagation_stopped(&self) -> bool {
        self.propagation_stopped
    }
}

/// O que o ouvinte recebe.
pub type Handler = Rc<dyn Fn(&mut Event)>;

#[derive(Clone)]
struct Listener {
    id: u64,
    kind: EventKind,
    capture: bool,
    handler: Handler,
    /// Falso depois de removido, para o despacho em andamento não chamá-lo.
    active: Rc<Cell<bool>>,
}

/// O tipo da entrada de ponteiro crua.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerInputKind {
    Down,
    Move,
    Up,
}

/// A entrada de ponteiro como a janela a entrega.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerInput {
    pub kind: PointerInputKind,
    pub x: f64,
    pub y: f64,
    pub button: PointerButton,
    pub modifiers: Modifiers,
    pub click_count: u32,
}

impl PointerInput {
    pub fn new(kind: PointerInputKind, x: f64, y: f64) -> Self {
        Self { kind, x, y, button: PointerButton::Primary, modifiers: Modifiers::default(), click_count: 1 }
    }
}

#[derive(Default)]
struct InputState {
    listeners: HashMap<NodeId, Vec<Listener>>,
    /// O caminho (da raiz ao nó) sob o ponteiro na última entrada, para o `PointerEnter`/`PointerLeave`.
    hover_path: Vec<NodeId>,
    captured: Option<NodeId>,
    focused: Option<NodeId>,
    /// O alvo do último botão baixo, para achar o alvo do `Click`.
    pressed: Option<NodeId>,
    /// Os `* { cursor: X !important }` empilhados (o último vence), com o número de cada um.
    cursor_overrides: Vec<(u64, Cursor)>,
    next_override: u64,
}

struct UiInner {
    tree: RefCell<Tree>,
    input: RefCell<InputState>,
    next_listener: Cell<u64>,
}

/// A árvore de UI de uma janela, com os eventos. Clonar é barato (é um `Rc`).
#[derive(Clone)]
pub struct Ui(Rc<UiInner>);

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self(Rc::new(UiInner { tree: RefCell::new(Tree::new()), input: RefCell::default(), next_listener: Cell::new(0) }))
    }

    /// A raiz da árvore: a janela e o `document` ao mesmo tempo. Ouvinte de borbulha aqui é o ouvinte
    /// na `window`.
    pub fn root(&self) -> NodeId {
        self.0.tree.borrow().root()
    }

    /// A árvore, para leitura. Não segure o empréstimo ao chamar código de outro widget.
    pub fn tree(&self) -> Ref<'_, Tree> {
        self.0.tree.borrow()
    }

    /// A árvore, para escrita. Não segure o empréstimo ao chamar código de outro widget.
    pub fn tree_mut(&self) -> RefMut<'_, Tree> {
        self.0.tree.borrow_mut()
    }

    /// Destrói o nó e a subárvore (o `remove` do DOM seguido do descarte), e esquece os ouvintes, o
    /// foco, a captura e o hover que apontavam para ela. Use no lugar de [`Tree::remove`] sempre que o
    /// nó puder ter ouvintes.
    pub fn remove(&self, id: NodeId) {
        let removed = {
            let tree = self.0.tree.borrow();
            if !tree.is_alive(id) {
                return;
            }
            tree.subtree(id)
        };
        {
            let mut input = self.0.input.borrow_mut();
            for n in &removed {
                for listener in input.listeners.remove(n).unwrap_or_default() {
                    listener.active.set(false);
                }
            }
            if input.captured.is_some_and(|c| removed.contains(&c)) {
                input.captured = None;
            }
            if input.focused.is_some_and(|f| removed.contains(&f)) {
                input.focused = None;
            }
            if input.pressed.is_some_and(|p| removed.contains(&p)) {
                input.pressed = None;
            }
            input.hover_path.retain(|n| !removed.contains(n));
        }
        self.0.tree.borrow_mut().remove(id);
    }

    /// `addEventListener(kind, handler, capture)`. Descartar o resultado remove o ouvinte.
    pub fn listen(&self, node: NodeId, kind: EventKind, capture: bool, handler: impl Fn(&mut Event) + 'static) -> DisposableRef {
        let id = self.0.next_listener.get();
        self.0.next_listener.set(id + 1);
        let active = Rc::new(Cell::new(true));
        let listener = Listener { id, kind, capture, handler: Rc::new(handler), active: active.clone() };
        self.0.input.borrow_mut().listeners.entry(node).or_default().push(listener);
        let ui = Rc::downgrade(&self.0);
        to_disposable(move || {
            active.set(false);
            if let Some(ui) = ui.upgrade() {
                let mut input = ui.input.borrow_mut();
                if let Some(list) = input.listeners.get_mut(&node) {
                    list.retain(|l| l.id != id);
                }
            }
        })
    }

    /// Empilha um cursor que vale para a janela inteira (o `* { cursor: X !important }` que o `sash` põe
    /// enquanto arrasta). Devolve o número para [`Ui::update_cursor_override`] e
    /// [`Ui::remove_cursor_override`].
    pub fn add_cursor_override(&self, cursor: Cursor) -> u64 {
        let mut input = self.0.input.borrow_mut();
        let id = input.next_override;
        input.next_override += 1;
        input.cursor_overrides.push((id, cursor));
        id
    }

    pub fn update_cursor_override(&self, id: u64, cursor: Cursor) {
        if let Some(entry) = self.0.input.borrow_mut().cursor_overrides.iter_mut().find(|(i, _)| *i == id) {
            entry.1 = cursor;
        }
    }

    pub fn remove_cursor_override(&self, id: u64) {
        self.0.input.borrow_mut().cursor_overrides.retain(|(i, _)| *i != id);
    }

    /// O cursor a mostrar no ponto: o override mais recente, ou o do nó sob o ponteiro.
    pub fn cursor_at(&self, x: f64, y: f64) -> Cursor {
        if let Some((_, cursor)) = self.0.input.borrow().cursor_overrides.last() {
            return *cursor;
        }
        let tree = self.0.tree.borrow();
        tree.hit_test(x, y).map_or(Cursor::Default, |n| tree.cursor(n))
    }

    /// Despacha o evento ao alvo, com captura, alvo e borbulha. Devolve falso se algum ouvinte chamou
    /// `preventDefault` (o retorno do `dispatchEvent`).
    pub fn dispatch(&self, event: &mut Event) -> bool {
        let path = self.path_to(event.target);
        let (ancestors, target) = match path.split_last() {
            Some((target, ancestors)) => (ancestors.to_vec(), *target),
            None => return true,
        };
        if event.kind.direct() {
            event.phase = Phase::Target;
            self.invoke(target, event, |_| true);
            return !event.default_prevented;
        }
        event.phase = Phase::Capture;
        for node in &ancestors {
            if event.propagation_stopped {
                break;
            }
            self.invoke(*node, event, |l| l.capture);
        }
        if !event.propagation_stopped {
            event.phase = Phase::Target;
            self.invoke(target, event, |_| true);
        }
        if event.kind.bubbles() {
            event.phase = Phase::Bubble;
            for node in ancestors.iter().rev() {
                if event.propagation_stopped {
                    break;
                }
                self.invoke(*node, event, |l| !l.capture);
            }
        }
        !event.default_prevented
    }

    fn invoke(&self, node: NodeId, event: &mut Event, accept: impl Fn(&Listener) -> bool) {
        event.current_target = node;
        let snapshot: Vec<Listener> = self.0.input.borrow().listeners.get(&node).map(|l| l.iter().filter(|l| l.kind == event.kind && accept(l)).cloned().collect()).unwrap_or_default();
        for listener in snapshot {
            if listener.active.get() {
                (listener.handler)(event);
            }
        }
    }

    /// A cadeia da raiz (ou do nó solto mais alto) até `node`.
    fn path_to(&self, node: NodeId) -> Vec<NodeId> {
        let tree = self.0.tree.borrow();
        if !tree.is_alive(node) {
            return Vec::new();
        }
        let mut path = vec![node];
        while let Some(parent) = tree.parent(*path.last().expect("o caminho nunca fica vazio")) {
            path.push(parent);
        }
        path.reverse();
        path
    }

    /// Redireciona `PointerMove` e `PointerUp` para `node` até o botão subir.
    pub fn set_pointer_capture(&self, node: NodeId) {
        self.0.input.borrow_mut().captured = Some(node);
    }

    pub fn release_pointer_capture(&self) {
        self.0.input.borrow_mut().captured = None;
    }

    pub fn pointer_captured_by(&self) -> Option<NodeId> {
        self.0.input.borrow().captured
    }

    /// Processa uma entrada de ponteiro da janela: acha o alvo, sintetiza enter/leave/click/dblclick e
    /// despacha. Devolve o nó que foi o alvo do evento.
    pub fn pointer_input(&self, input: PointerInput) -> NodeId {
        let hit = self.0.tree.borrow().hit_test(input.x, input.y).unwrap_or_else(|| self.root());
        let captured = self.0.input.borrow().captured.filter(|c| self.0.tree.borrow().is_alive(*c));
        let target = match input.kind {
            PointerInputKind::Down => hit,
            PointerInputKind::Move | PointerInputKind::Up => captured.unwrap_or(hit),
        };
        let data = PointerData { x: input.x, y: input.y, button: input.button, modifiers: input.modifiers, click_count: input.click_count };
        match input.kind {
            PointerInputKind::Down => {
                self.update_hover(hit, data);
                self.0.input.borrow_mut().pressed = Some(target);
                self.dispatch(&mut Event::new(EventKind::PointerDown, target).with_pointer(data));
            }
            PointerInputKind::Move => {
                self.update_hover(hit, data);
                self.dispatch(&mut Event::new(EventKind::PointerMove, target).with_pointer(data));
            }
            PointerInputKind::Up => {
                self.dispatch(&mut Event::new(EventKind::PointerUp, target).with_pointer(data));
                let pressed = self.0.input.borrow_mut().pressed.take();
                self.release_pointer_capture();
                if let (Some(pressed), PointerButton::Primary) = (pressed, input.button)
                    && let Some(common) = self.common_ancestor(pressed, target) {
                        self.dispatch(&mut Event::new(EventKind::Click, common).with_pointer(data));
                        if input.click_count == 2 {
                            self.dispatch(&mut Event::new(EventKind::DoubleClick, common).with_pointer(data));
                        }
                    }
            }
        }
        target
    }

    /// O ponteiro saiu da janela: `PointerLeave` em todo o caminho sob o ponteiro.
    pub fn pointer_left_window(&self) {
        let data = PointerData { x: f64::NAN, y: f64::NAN, button: PointerButton::Primary, modifiers: Modifiers::default(), click_count: 0 };
        self.update_hover_path(Vec::new(), data);
    }

    fn update_hover(&self, hit: NodeId, data: PointerData) {
        let path = self.path_to(hit);
        self.update_hover_path(path, data);
    }

    fn update_hover_path(&self, path: Vec<NodeId>, data: PointerData) {
        let old = std::mem::replace(&mut self.0.input.borrow_mut().hover_path, path.clone());
        let common = old.iter().zip(&path).take_while(|(a, b)| a == b).count();
        for node in old[common..].iter().rev() {
            self.dispatch(&mut Event::new(EventKind::PointerLeave, *node).with_pointer(data));
        }
        for node in &path[common..] {
            self.dispatch(&mut Event::new(EventKind::PointerEnter, *node).with_pointer(data));
        }
    }

    fn common_ancestor(&self, a: NodeId, b: NodeId) -> Option<NodeId> {
        let (pa, pb) = (self.path_to(a), self.path_to(b));
        pa.iter().zip(&pb).take_while(|(x, y)| x == y).last().map(|(n, _)| *n)
    }

    /// Entrega uma tecla ao nó com foco (ou à raiz). Devolve falso se `preventDefault` foi chamado.
    pub fn key_input(&self, down: bool, key: KeyData) -> bool {
        let target = self.focused().unwrap_or_else(|| self.root());
        let kind = if down { EventKind::KeyDown } else { EventKind::KeyUp };
        self.dispatch(&mut Event::new(kind, target).with_key(key))
    }

    pub fn focused(&self) -> Option<NodeId> {
        self.0.input.borrow().focused
    }

    /// Dá o foco a `node`: `Blur` no que tinha, depois `Focus` no novo.
    pub fn focus(&self, node: NodeId) {
        let previous = self.focused();
        if previous == Some(node) {
            return;
        }
        if let Some(previous) = previous {
            self.0.input.borrow_mut().focused = None;
            self.dispatch(&mut Event::new(EventKind::Blur, previous));
        }
        self.0.input.borrow_mut().focused = Some(node);
        self.dispatch(&mut Event::new(EventKind::Focus, node));
    }

    pub fn blur(&self) {
        if let Some(previous) = self.focused() {
            self.0.input.borrow_mut().focused = None;
            self.dispatch(&mut Event::new(EventKind::Blur, previous));
        }
    }
}
