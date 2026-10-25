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
//!   Code escuta os dois com captura). Sem `preventDefault`, o `keydown` tem a ação padrão do navegador: `Tab` e
//!   `Shift+Tab` movem o foco pela ordem de Tab ([`Tree::tab_order`]: `tabindex` e links de parágrafo) e `Enter`
//!   num link de parágrafo o ativa com um `Click` (o `Space` não ativa link, como no Chromium).
//!
//! - **Roda do mouse.** [`Ui::wheel_input`] despacha um [`EventKind::Wheel`] (que borbulha) ao nó sob o
//!   ponteiro, com os deltas em pixels ou em linhas e o `deltaMode` ([`crate::wheel`]). O
//!   [`crate::StandardWheelEvent`] faz a normalização do `mouseEvent.ts` do VS Code (entalhes da roda).
//!
//! - **Texto digitado.** [`Ui::text_input`] entrega um [`EventKind::TextInput`] ao nó com foco (o `input` de um
//!   `<input>`); a edição do campo é do widget.
//!
//! - **Arrastar e soltar.** O modelo de processamento do HTML (o do Chromium do Electron), com a carga em
//!   [`crate::drag::DataTransfer`] no [`Event::data_transfer`]. Botão principal baixo sobre um nó cujo antepassado
//!   mais próximo com o atributo `draggable` (ou ele mesmo) o tem como `"true"` arma o arrasto; o ponteiro mover
//!   [`Ui::drag_threshold`] pixels (em algum eixo) dispara [`EventKind::DragStart`] nesse nó, e `preventDefault`
//!   cancela o arrasto (o ponteiro volta a produzir `PointerMove` e `Click`). Durante o arrasto não há `PointerMove`,
//!   `PointerUp` nem `Click`: há [`EventKind::Drag`] na origem, [`EventKind::DragEnter`] no novo alvo seguido de
//!   [`EventKind::DragLeave`] no antigo ao trocar de nó sob o ponteiro (nessa ordem, como no navegador, para o
//!   contador de entradas do `DragAndDropObserver` fechar), [`EventKind::DragOver`] no alvo a cada movimento e, ao
//!   soltar, [`EventKind::Drop`] no alvo se o último `dragover` foi aceito (`preventDefault`) e
//!   [`EventKind::DragEnd`] na origem, com o `dropEffect` final (`none` se o alvo recusou ou se o `drop` não foi
//!   cancelado). Sem `preventDefault` no `dragover`, o `dropEffect` fica `none`. `Escape` ([`Ui::key_input`] e
//!   [`Ui::cancel_drag`]) cancela: `DragLeave` no alvo e `DragEnd` com `none`. Todos os sete eventos borbulham.
//! - **Arrastar de fora.** Arquivos do sistema arrastados para a janela entram por [`Ui::external_drag_enter`],
//!   [`Ui::external_drag_over`], [`Ui::external_drag_leave`] e [`Ui::external_drop`]: o mesmo `DragEnter`,
//!   `DragOver`, `DragLeave` e `Drop` no alvo, com os arquivos em [`crate::drag::DataTransfer::files`] e o tipo
//!   `Files`, mas sem origem, sem `DragStart` e sem `DragEnd`.
//!
//! O limiar de arrasto é parâmetro do `Ui` ([`Ui::set_drag_threshold`]), que a janela informa a partir do sistema; o
//! padrão de 4 px é NÃO MEDIDO (valor assumido, no espírito do `SM_CXDRAG` do Windows e do Chromium, que usam
//! poucos pixels).
//!
//! Fica de fora, por enquanto: toque e gesto (`Gesture`, `EventType.Tap`), composição de IME e `PointerEvent` com
//! vários ponteiros.

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableRef, plain_disposable, to_disposable};

use crate::drag::{DataStoreMode, DataTransfer, DropEffect};
use crate::tree::{Content, Cursor, NodeId, Tree};
use crate::wheel::WheelData;

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
    /// `contextmenu`: o botão secundário solto (no Windows o Chromium dispara no `mouseup`).
    ContextMenu,
    /// `wheel`.
    Wheel,
    /// `keydown`.
    KeyDown,
    /// `keyup`.
    KeyUp,
    /// `focus`: não borbulha.
    Focus,
    /// `blur`: não borbulha.
    Blur,
    /// `input`/`beforeinput` de um campo de texto: o texto digitado (ou colado pelo IME) em
    /// [`Event::text`]. Vai ao nó com foco, borbulha.
    TextInput,
    /// `dragstart`: na origem, com o repositório de dados aberto para escrita; `preventDefault` cancela o arrasto.
    DragStart,
    /// `drag`: na origem, a cada movimento durante o arrasto.
    Drag,
    /// `dragenter`: no nó que passou a estar sob o ponteiro.
    DragEnter,
    /// `dragover`: no nó sob o ponteiro, a cada movimento; `preventDefault` aceita a soltura.
    DragOver,
    /// `dragleave`: no nó que deixou de estar sob o ponteiro.
    DragLeave,
    /// `drop`: no alvo, quando o último `dragover` foi aceito; `getData` e `files` funcionam aqui.
    Drop,
    /// `dragend`: na origem, no fim do arrasto, com o `dropEffect` final.
    DragEnd,
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
    /// Os dados da roda do mouse, num evento [`EventKind::Wheel`].
    pub wheel: Option<WheelData>,
    /// O texto de um evento [`EventKind::TextInput`].
    pub text: Option<String>,
    /// A carga do arrasto, nos sete eventos de arrastar e soltar (o `dataTransfer`).
    pub data_transfer: Option<DataTransfer>,
    default_prevented: bool,
    propagation_stopped: bool,
}

impl Event {
    pub fn new(kind: EventKind, target: NodeId) -> Self {
        Self { kind, target, current_target: target, phase: Phase::Target, pointer: None, key: None, wheel: None, text: None, data_transfer: None, default_prevented: false, propagation_stopped: false }
    }

    pub fn with_data_transfer(mut self, data_transfer: DataTransfer) -> Self {
        self.data_transfer = Some(data_transfer);
        self
    }

    pub fn with_pointer(mut self, pointer: PointerData) -> Self {
        self.pointer = Some(pointer);
        self
    }

    pub fn with_wheel(mut self, wheel: WheelData) -> Self {
        self.wheel = Some(wheel);
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
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
    /// Ação padrão do elemento (ver [`Ui::listen_default`]): roda depois de todo o despacho.
    default_action: bool,
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
    /// O arrasto armado pelo botão baixo: a origem e onde o ponteiro estava. Vira [`InputState::drag`] ao passar o limiar.
    drag_pending: Option<(NodeId, f64, f64)>,
    drag: Option<DragSession>,
}

/// Um arrasto em andamento (de dentro ou de fora da janela).
struct DragSession {
    /// A origem (`None` no arrasto externo, que não tem `dragstart` nem `dragend`).
    source: Option<NodeId>,
    external: bool,
    transfer: DataTransfer,
    /// O nó que recebeu o último `dragenter`.
    last_target: Option<NodeId>,
    /// O último `dragover` foi aceito (`preventDefault`)?
    accepted: bool,
    /// O efeito que valerá na soltura, decidido no último `dragover`.
    effect: DropEffect,
    /// O último ponteiro conhecido, para os eventos de cancelamento.
    pointer: PointerData,
}

/// O limiar de arrasto padrão, em pixels lógicos. NÃO MEDIDO: valor assumido; a janela deve informar o do sistema
/// por [`Ui::set_drag_threshold`].
pub const DEFAULT_DRAG_THRESHOLD: f64 = 4.0;

/// Um callback agendado para o próximo quadro de animação.
struct FrameCallback {
    id: u64,
    priority: i32,
    /// Falso depois de cancelado, para o quadro em andamento não chamá-lo.
    active: Rc<Cell<bool>>,
    runner: Box<dyn FnOnce()>,
}

struct UiInner {
    tree: RefCell<Tree>,
    input: RefCell<InputState>,
    next_listener: Cell<u64>,
    frame_callbacks: RefCell<Vec<FrameCallback>>,
    next_frame_callback: Cell<u64>,
    drag_threshold: Cell<f64>,
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
        Self(Rc::new(UiInner { tree: RefCell::new(Tree::new()), input: RefCell::default(), next_listener: Cell::new(0), frame_callbacks: RefCell::new(Vec::new()), next_frame_callback: Cell::new(0), drag_threshold: Cell::new(DEFAULT_DRAG_THRESHOLD) }))
    }

    /// O limiar de arrasto em pixels lógicos (ver [`Ui::set_drag_threshold`]).
    pub fn drag_threshold(&self) -> f64 {
        self.0.drag_threshold.get()
    }

    /// Informa o limiar de arrasto do sistema: o ponteiro precisa mover tantos pixels em algum eixo, com o botão
    /// principal baixo sobre um nó `draggable`, para começar o arrasto. A janela chama isto com o valor do sistema.
    pub fn set_drag_threshold(&self, pixels: f64) {
        self.0.drag_threshold.set(pixels.max(0.0));
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
            if input.drag_pending.is_some_and(|(source, _, _)| removed.contains(&source)) {
                input.drag_pending = None;
            }
            input.hover_path.retain(|n| !removed.contains(n));
        }
        self.0.tree.borrow_mut().remove(id);
    }

    /// `addEventListener(kind, handler, capture)`. Descartar o resultado remove o ouvinte.
    pub fn listen(&self, node: NodeId, kind: EventKind, capture: bool, handler: impl Fn(&mut Event) + 'static) -> DisposableRef {
        self.add_listener(node, kind, capture, false, handler)
    }

    /// A ação padrão de um elemento para o evento (o que o navegador faz com um `keydown` num `<input>`
    /// depois que os ouvintes rodaram): roda no fim do despacho, só se nenhum ouvinte chamou
    /// `preventDefault`, e só a do nó mais próximo do alvo (o próprio alvo ou o antepassado mais baixo
    /// que tem uma). `stopPropagation` não a impede, como no DOM.
    pub fn listen_default(&self, node: NodeId, kind: EventKind, handler: impl Fn(&mut Event) + 'static) -> DisposableRef {
        self.add_listener(node, kind, false, true, handler)
    }

    fn add_listener(&self, node: NodeId, kind: EventKind, capture: bool, default_action: bool, handler: impl Fn(&mut Event) + 'static) -> DisposableRef {
        let id = self.0.next_listener.get();
        self.0.next_listener.set(id + 1);
        let active = Rc::new(Cell::new(true));
        let listener = Listener { id, kind, capture, handler: Rc::new(handler), default_action, active: active.clone() };
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

    /// `scheduleAtNextAnimationFrame(window, runner, priority)`: agenda `runner` para o próximo quadro.
    /// Dentro de um quadro, a prioridade maior roda antes e, na mesma prioridade, a ordem de agendamento
    /// vale. Descartar o resultado cancela. O dono da janela chama [`Ui::run_animation_frame`] a cada
    /// quadro (60 vezes por segundo, no ritmo do monitor).
    pub fn schedule_at_next_animation_frame(&self, priority: i32, runner: impl FnOnce() + 'static) -> DisposableRef {
        let id = self.0.next_frame_callback.get();
        self.0.next_frame_callback.set(id + 1);
        let active = Rc::new(Cell::new(true));
        self.0.frame_callbacks.borrow_mut().push(FrameCallback { id, priority, active: active.clone(), runner: Box::new(runner) });
        let ui = Rc::downgrade(&self.0);
        plain_disposable(move || {
            active.set(false);
            if let Some(ui) = ui.upgrade() {
                ui.frame_callbacks.borrow_mut().retain(|c| c.id != id);
            }
        })
    }

    /// Roda os callbacks agendados até aqui (o que o navegador faz antes de pintar cada quadro). O que um
    /// callback agenda enquanto roda fica para o próximo quadro. Devolve quantos rodaram.
    pub fn run_animation_frame(&self) -> usize {
        let mut batch = std::mem::take(&mut *self.0.frame_callbacks.borrow_mut());
        batch.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
        let count = batch.len();
        for callback in batch {
            if callback.active.get() {
                (callback.runner)();
            }
        }
        count
    }

    /// Há callbacks esperando o próximo quadro?
    pub fn has_pending_animation_frame(&self) -> bool {
        !self.0.frame_callbacks.borrow().is_empty()
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
        if !event.default_prevented {
            self.run_default_action(&path, event);
        }
        !event.default_prevented
    }

    /// A ação padrão do nó mais próximo do alvo que tem uma para o tipo do evento.
    fn run_default_action(&self, path: &[NodeId], event: &mut Event) {
        for node in path.iter().rev() {
            let actions: Vec<Listener> = self.0.input.borrow().listeners.get(node).map(|l| l.iter().filter(|l| l.default_action && l.kind == event.kind).cloned().collect()).unwrap_or_default();
            if actions.is_empty() {
                continue;
            }
            event.current_target = *node;
            event.phase = Phase::Target;
            for action in actions {
                if action.active.get() {
                    (action.handler)(event);
                }
            }
            return;
        }
    }

    fn invoke(&self, node: NodeId, event: &mut Event, accept: impl Fn(&Listener) -> bool) {
        event.current_target = node;
        let snapshot: Vec<Listener> = self.0.input.borrow().listeners.get(&node).map(|l| l.iter().filter(|l| !l.default_action && l.kind == event.kind && accept(l)).cloned().collect()).unwrap_or_default();
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
        if self.0.input.borrow().drag.is_some() {
            // Durante o arrasto o ponteiro só produz eventos de arrastar e soltar (e o hover fica congelado).
            match input.kind {
                PointerInputKind::Down => {}
                PointerInputKind::Move => self.drag_update(data),
                PointerInputKind::Up => {
                    self.drag_update(data);
                    self.end_drag(true);
                    let mut state = self.0.input.borrow_mut();
                    state.pressed = None;
                    state.captured = None;
                }
            }
            return hit;
        }
        match input.kind {
            PointerInputKind::Down => {
                let pending = if input.button == PointerButton::Primary { self.draggable_ancestor(hit).map(|source| (source, input.x, input.y)) } else { None };
                self.0.input.borrow_mut().drag_pending = pending;
                self.update_hover(hit, data);
                self.0.input.borrow_mut().pressed = Some(target);
                self.dispatch(&mut Event::new(EventKind::PointerDown, target).with_pointer(data));
            }
            PointerInputKind::Move => {
                if self.try_begin_drag(data) {
                    return hit;
                }
                self.update_hover(hit, data);
                self.dispatch(&mut Event::new(EventKind::PointerMove, target).with_pointer(data));
            }
            PointerInputKind::Up => {
                self.0.input.borrow_mut().drag_pending = None;
                self.dispatch(&mut Event::new(EventKind::PointerUp, target).with_pointer(data));
                let pressed = self.0.input.borrow_mut().pressed.take();
                self.release_pointer_capture();
                if input.button == PointerButton::Secondary {
                    self.dispatch(&mut Event::new(EventKind::ContextMenu, target).with_pointer(data));
                }
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

    /// Processa uma entrada de roda do mouse: o alvo é o nó mais ao fundo sob o ponteiro (ou a raiz) e o
    /// evento borbulha. Devolve falso se algum ouvinte chamou `preventDefault` (a roda foi consumida).
    pub fn wheel_input(&self, wheel: WheelData) -> bool {
        let target = self.0.tree.borrow().hit_test(wheel.x, wheel.y).unwrap_or_else(|| self.root());
        self.dispatch(&mut Event::new(EventKind::Wheel, target).with_wheel(wheel))
    }

    /// Entrega uma tecla ao nó com foco (ou à raiz). Devolve falso se `preventDefault` foi chamado ou se a tecla teve a
    /// ação padrão do navegador: `Tab` e `Shift+Tab` movem o foco pela ordem de Tab ([`Ui::focus_by_tab`]), e `Enter`
    /// num link de parágrafo o ativa (o `click` com o link por alvo, como o `HTMLAnchorElement` faz).
    pub fn key_input(&self, down: bool, key: KeyData) -> bool {
        if down && key.key_code == KeyCode::Escape && self.cancel_drag() {
            return false;
        }
        let target = self.focused().unwrap_or_else(|| self.root());
        let kind = if down { EventKind::KeyDown } else { EventKind::KeyUp };
        let proceed = self.dispatch(&mut Event::new(kind, target).with_key(key));
        proceed && !(down && self.key_default_action(target, key))
    }

    /// A ação padrão do navegador para o `keydown` que nenhum ouvinte consumiu (ver [`Ui::key_input`]). `Space` não
    /// ativa link no Chromium e não faz nada aqui. Devolve verdadeiro se a tecla foi consumida.
    fn key_default_action(&self, target: NodeId, key: KeyData) -> bool {
        let Modifiers { alt, ctrl, shift, meta } = key.modifiers;
        if key.key_code == KeyCode::Enter && self.0.tree.borrow().link_index(target).is_some() {
            self.dispatch(&mut Event::new(EventKind::Click, target));
            return true;
        }
        key.key_code == KeyCode::Tab && !(alt || ctrl || meta) && self.focus_by_tab(shift)
    }

    /// Move o foco para o próximo (ou, com `reverse`, o anterior) da ordem de Tab ([`Tree::tab_order`]), voltando ao
    /// outro extremo no fim. Sem foco na ordem, começa pelo primeiro (ou pelo último). Devolve falso se não há nó
    /// focável.
    pub fn focus_by_tab(&self, reverse: bool) -> bool {
        let order = self.0.tree.borrow().tab_order();
        let Some(last) = order.len().checked_sub(1) else { return false };
        let current = self.focused().and_then(|focused| order.iter().position(|node| *node == focused));
        let next = match (current, reverse) {
            (Some(index), false) => (index + 1) % order.len(),
            (Some(index), true) => (index + last) % order.len(),
            (None, false) => 0,
            (None, true) => last,
        };
        self.focus(order[next]);
        true
    }

    /// Cria, atualiza e remove os nós de link (`Role::Link`) do parágrafo do nó `id`, um por link do
    /// [`crate::Paragraph`], na ordem em que aparecem no texto (a ordem de Tab). O nó de um link que continua existe
    /// mantém a identidade (e o foco); o de um link que sumiu é removido com os ouvintes e o foco. Chame depois de
    /// trocar o conteúdo do parágrafo.
    pub fn sync_paragraph_links(&self, id: NodeId) {
        let (wanted, existing) = {
            let tree = self.0.tree.borrow();
            if !tree.is_alive(id) {
                return;
            }
            let wanted: Vec<(u32, String)> = match &tree.style(id).content {
                Content::Paragraph(paragraph) => paragraph.link_numbers().into_iter().map(|link| (link, paragraph.link_name(link))).collect(),
                _ => Vec::new(),
            };
            (wanted, tree.paragraph_link_nodes(id))
        };
        for (link, node) in &existing {
            if !wanted.iter().any(|(number, _)| number == link) {
                self.remove(*node);
            }
        }
        let mut tree = self.0.tree.borrow_mut();
        for (link, name) in wanted {
            let node = match existing.iter().find(|(number, _)| *number == link) {
                Some((_, node)) => {
                    tree.set_label(*node, name);
                    *node
                }
                None => tree.add_paragraph_link(id, link, name),
            };
            // Reanexar na ordem do texto: a ordem dos filhos é a ordem de Tab.
            tree.append_child(id, node);
        }
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
            self.set_focused(None);
            self.dispatch(&mut Event::new(EventKind::Blur, previous));
        }
        self.set_focused(Some(node));
        self.dispatch(&mut Event::new(EventKind::Focus, node));
    }

    pub fn blur(&self) {
        if let Some(previous) = self.focused() {
            self.set_focused(None);
            self.dispatch(&mut Event::new(EventKind::Blur, previous));
        }
    }

    /// Guarda o foco no estado de entrada e na árvore (que pinta o contorno de foco do link).
    fn set_focused(&self, node: Option<NodeId>) {
        self.0.input.borrow_mut().focused = node;
        self.0.tree.borrow_mut().set_focused(node);
    }

    /// Entrega texto digitado (teclado, IME, colar do sistema) ao nó com foco (ou à raiz). Devolve falso
    /// se `preventDefault` foi chamado.
    pub fn text_input(&self, text: &str) -> bool {
        let target = self.focused().unwrap_or_else(|| self.root());
        self.dispatch(&mut Event::new(EventKind::TextInput, target).with_text(text))
    }
}

/// Arrastar e soltar (ver o `//!` do módulo).
impl Ui {
    /// Há um arrasto em andamento (de dentro ou de fora da janela)?
    pub fn is_dragging(&self) -> bool {
        self.0.input.borrow().drag.is_some()
    }

    /// A carga do arrasto em andamento, para o shell pintar a imagem de arrasto ([`DataTransfer::drag_image`]).
    pub fn drag_data_transfer(&self) -> Option<DataTransfer> {
        self.0.input.borrow().drag.as_ref().map(|d| d.transfer.clone())
    }

    /// Cancela o arrasto em andamento (o `Escape`): `DragLeave` no alvo e `DragEnd` na origem com `dropEffect` `none`.
    /// Devolve falso se não havia arrasto.
    pub fn cancel_drag(&self) -> bool {
        if !self.is_dragging() {
            return false;
        }
        self.end_drag(false);
        true
    }

    /// Arquivos do sistema entraram na janela: o primeiro chamado dispara `DragEnter` e `DragOver` no nó sob `(x, y)`;
    /// os seguintes, enquanto o arrasto externo continua, só acrescentam arquivos (o winit entrega um evento por
    /// arquivo) e atualizam a posição. Ignorado durante um arrasto interno.
    pub fn external_drag_enter(&self, paths: Vec<PathBuf>, x: f64, y: f64) {
        let data = Self::external_pointer(x, y);
        {
            let mut input = self.0.input.borrow_mut();
            match input.drag.as_mut() {
                Some(session) if session.external => session.transfer.add_files(paths),
                Some(_) => return,
                None => {
                    input.drag = Some(DragSession { source: None, external: true, transfer: DataTransfer::external(paths), last_target: None, accepted: false, effect: DropEffect::None, pointer: data });
                }
            }
        }
        self.drag_update(data);
    }

    /// O arrasto externo moveu: `DragOver` no nó sob `(x, y)` (e `DragEnter`/`DragLeave` se o nó mudou).
    pub fn external_drag_over(&self, x: f64, y: f64) {
        if self.external_dragging() {
            self.drag_update(Self::external_pointer(x, y));
        }
    }

    /// O arrasto externo saiu da janela ou foi cancelado: `DragLeave` no alvo, sem `DragEnd`.
    pub fn external_drag_leave(&self) {
        if self.external_dragging() {
            self.end_drag(false);
        }
    }

    /// Os arquivos foram soltos em `(x, y)`: `Drop` no alvo se o último `dragover` foi aceito, com os arquivos em
    /// [`DataTransfer::files`]; sem `DragEnd`. Devolve verdadeiro se o `drop` foi tratado (`preventDefault`).
    pub fn external_drop(&self, x: f64, y: f64) -> bool {
        if !self.external_dragging() {
            return false;
        }
        self.drag_update(Self::external_pointer(x, y));
        self.end_drag(true)
    }

    fn external_dragging(&self) -> bool {
        self.0.input.borrow().drag.as_ref().is_some_and(|d| d.external)
    }

    fn external_pointer(x: f64, y: f64) -> PointerData {
        PointerData { x, y, button: PointerButton::Primary, modifiers: Modifiers::default(), click_count: 0 }
    }

    /// O nó mais próximo (o próprio `node` ou um antepassado) que declara o atributo `draggable`, se ele vale
    /// `"true"`. `draggable="false"` no mais próximo impede o arrasto, como no navegador.
    fn draggable_ancestor(&self, node: NodeId) -> Option<NodeId> {
        let tree = self.0.tree.borrow();
        let mut current = Some(node).filter(|n| tree.is_alive(*n));
        while let Some(n) = current {
            match tree.attribute(n, "draggable") {
                Some("true") => return Some(n),
                Some(_) => return None,
                None => current = tree.parent(n),
            }
        }
        None
    }

    /// Dispara um evento de arrastar e soltar com o repositório no `mode` dado. Devolve falso se um ouvinte chamou
    /// `preventDefault`.
    fn drag_event(&self, kind: EventKind, target: NodeId, data: PointerData, transfer: &DataTransfer, mode: DataStoreMode) -> bool {
        transfer.set_mode(mode);
        self.dispatch(&mut Event::new(kind, target).with_pointer(data).with_data_transfer(transfer.clone()))
    }

    /// Com o botão baixo sobre um nó `draggable`, passado o limiar, dispara `DragStart`. Devolve verdadeiro se o
    /// arrasto começou (o movimento já foi consumido por ele); falso se não havia arrasto armado, se o limiar não foi
    /// passado, ou se `DragStart` foi cancelado (o movimento segue como `PointerMove`).
    fn try_begin_drag(&self, data: PointerData) -> bool {
        let Some((source, start_x, start_y)) = self.0.input.borrow().drag_pending else { return false };
        if !self.0.tree.borrow().is_alive(source) || self.0.input.borrow().captured.is_some() {
            // A origem sumiu, ou um ouvinte tomou o ponteiro (o `sash`): não é um arrasto de nó.
            self.0.input.borrow_mut().drag_pending = None;
            return false;
        }
        let threshold = self.drag_threshold();
        if (data.x - start_x).abs() < threshold && (data.y - start_y).abs() < threshold {
            return false;
        }
        self.0.input.borrow_mut().drag_pending = None;
        let transfer = DataTransfer::new();
        if !self.drag_event(EventKind::DragStart, source, data, &transfer, DataStoreMode::ReadWrite) {
            transfer.set_mode(DataStoreMode::Protected);
            return false;
        }
        transfer.set_mode(DataStoreMode::Protected);
        {
            let mut input = self.0.input.borrow_mut();
            input.pressed = None;
            input.captured = None;
            input.drag = Some(DragSession { source: Some(source), external: false, transfer, last_target: None, accepted: false, effect: DropEffect::None, pointer: data });
        }
        self.drag_update(data);
        true
    }

    /// O ponteiro moveu durante o arrasto: `Drag` na origem; se o nó sob o ponteiro mudou, `DragEnter` no novo e
    /// depois `DragLeave` no antigo; `DragOver` no nó sob o ponteiro. O `dropEffect` chega aos ouvintes com o valor
    /// derivado de `effectAllowed` e vira `none` se ninguém aceitou (`preventDefault`) ou se `effectAllowed` não o
    /// permite.
    fn drag_update(&self, data: PointerData) {
        let Some((source, transfer, previous)) = self.0.input.borrow().drag.as_ref().map(|d| (d.source, d.transfer.clone(), d.last_target)) else { return };
        let target = self.0.tree.borrow().hit_test(data.x, data.y).unwrap_or_else(|| self.root());
        if let Some(session) = self.0.input.borrow_mut().drag.as_mut() {
            session.pointer = data;
        }
        if let Some(source) = source {
            self.drag_event(EventKind::Drag, source, data, &transfer, DataStoreMode::Protected);
        }
        if previous != Some(target) && self.is_dragging() {
            transfer.reset_drop_effect();
            self.drag_event(EventKind::DragEnter, target, data, &transfer, DataStoreMode::Protected);
            if let Some(previous) = previous {
                transfer.reset_drop_effect();
                self.drag_event(EventKind::DragLeave, previous, data, &transfer, DataStoreMode::Protected);
            }
            if let Some(session) = self.0.input.borrow_mut().drag.as_mut() {
                session.last_target = Some(target);
            }
        }
        if !self.is_dragging() {
            return;
        }
        transfer.reset_drop_effect();
        let accepted = !self.drag_event(EventKind::DragOver, target, data, &transfer, DataStoreMode::Protected);
        let chosen = transfer.drop_effect();
        let effect = if accepted && transfer.effect_allowed().permits(chosen) { chosen } else { DropEffect::None };
        transfer.set_drop_effect(effect);
        if let Some(session) = self.0.input.borrow_mut().drag.as_mut() {
            session.accepted = accepted;
            session.effect = effect;
        }
    }

    /// Encerra o arrasto. Com `drop` e o último `dragover` aceito (com efeito diferente de `none`), `Drop` no alvo
    /// (com o repositório legível); senão `DragLeave` no alvo. Depois, `DragEnd` na origem (o arrasto externo não tem)
    /// com o `dropEffect` final: o efeito aceito se o `drop` foi cancelado (`preventDefault`), `none` no resto.
    /// Devolve verdadeiro se o `drop` foi tratado.
    fn end_drag(&self, drop: bool) -> bool {
        let Some(session) = self.0.input.borrow_mut().drag.take() else { return false };
        let (data, transfer) = (session.pointer, &session.transfer);
        let mut handled = false;
        if let Some(target) = session.last_target {
            if drop && session.accepted && session.effect != DropEffect::None {
                transfer.set_drop_effect(session.effect);
                handled = !self.drag_event(EventKind::Drop, target, data, transfer, DataStoreMode::ReadOnly);
            } else {
                transfer.set_drop_effect(DropEffect::None);
                self.drag_event(EventKind::DragLeave, target, data, transfer, DataStoreMode::Protected);
            }
        }
        if let Some(source) = session.source {
            transfer.set_drop_effect(if handled { session.effect } else { DropEffect::None });
            self.drag_event(EventKind::DragEnd, source, data, transfer, DataStoreMode::Protected);
        }
        transfer.set_mode(DataStoreMode::Protected);
        handled
    }
}
