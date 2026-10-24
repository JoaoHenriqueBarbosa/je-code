//! Porte de `src/vs/workbench/contrib/webview/browser/webviewFindWidget.ts`: o widget de busca de um webview, que
//! repassa a entrada ao [`WebviewFindDelegate`] e reage ao resultado da busca.
//!
//! Recorte declarado (sem DOM, a árvore de widgets é nativa):
//! - `SimpleFindWidget` (a base) ainda não tem porte próprio, então o trecho dela de que `WebviewFindWidget` depende
//!   vive aqui, com o mesmo corpo do original: o tratamento de `onInput` do `FindInput` (sessão de IME, `fromCompositionEnd`),
//!   `updateButtons`, `focusFindBox`, `show` e `hide`.
//! - O `FindInput` é o modelo [`FindInputModel`]: o valor do `<input>`, a bandeira `imeSessionInProgress` e os eventos
//!   `compositionstart`, `compositionend` e `input` (que o `FindInput` converte em `onInput`).
//! - O foco do documento é o modelo [`FocusModel`]: o elemento ativo (`document.activeElement`) e o histórico de chamadas a
//!   `focus()` do campo e do botão "próxima ocorrência" (o `sinon.spy` do teste).
//! - Os botões `prevBtn` e `nextBtn` (`SimpleButton`) guardam só o estado `aria-disabled`.
//! - Os `setTimeout(..., 0)` e `setTimeout(..., 200)` de `show` e `hide` são entregues por [`WebviewFindWidget::run_timers`].
//! - Ficam fora do recorte, por dependerem de DOM, de hover, de sash, de histórico com atraso (`Delayer`), de acessibilidade e
//!   de contagem de resultados (`showResultCount` é falso neste widget): rótulos, ARIA do campo, sash de redimensionamento,
//!   `_delayedUpdateHistory` e `updateResultCount`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};

/// `WebviewFindDelegate`.
pub trait WebviewFindDelegate {
    fn has_find_result(&self) -> Event<bool>;
    fn on_did_stop_find(&self) -> Event<()>;
    fn check_ime_completion_state(&self) -> bool;
    fn find(&self, value: &str, previous: bool);
    fn update_find(&self, value: &str);
    fn stop_find(&self, keep_selection: Option<bool>);
    fn focus(&self);
}

/// Os elementos focáveis do widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// O `<input>` do campo de busca.
    Input,
    /// O botão "próxima ocorrência" (`.codicon-find-next-match`).
    NextButton,
}

/// `document.activeElement` e os espiões de `focus()`.
#[derive(Default)]
pub struct FocusModel {
    active: Cell<Option<FocusTarget>>,
    calls: RefCell<Vec<FocusTarget>>,
}

impl FocusModel {
    /// `element.focus()`: registra a chamada (o espião) e move o elemento ativo.
    fn focus(&self, target: FocusTarget) {
        self.calls.borrow_mut().push(target);
        self.active.set(Some(target));
    }
}

/// O `FindInput`: valor do campo e sessão de IME.
#[derive(Default)]
struct FindInputModel {
    value: RefCell<String>,
    ime_session_in_progress: Cell<bool>,
}

/// O estado de `aria-disabled` de um `SimpleButton`.
struct ButtonModel {
    enabled: Cell<bool>,
}

impl ButtonModel {
    fn new() -> Self {
        Self { enabled: Cell::new(true) }
    }
}

pub struct WebviewFindWidget {
    base: Rc<Disposable>,
    delegate: Rc<dyn WebviewFindDelegate>,
    find_input: FindInputModel,
    focus_model: FocusModel,
    prev_btn: ButtonModel,
    next_btn: ButtonModel,
    is_visible: Cell<bool>,
    found_match: Cell<bool>,
    aria_hidden: Cell<bool>,
    pending_hide: Cell<Option<bool>>,
    pending_show: Cell<bool>,
    find_widget_focused: Cell<bool>,
}

impl WebviewFindWidget {
    pub fn new(delegate: Rc<dyn WebviewFindDelegate>) -> Rc<Self> {
        let this = Rc::new(Self {
            base: Disposable::new(),
            delegate: delegate.clone(),
            find_input: FindInputModel::default(),
            focus_model: FocusModel::default(),
            prev_btn: ButtonModel::new(),
            next_btn: ButtonModel::new(),
            is_visible: Cell::new(false),
            found_match: Cell::new(false),
            aria_hidden: Cell::new(true),
            pending_hide: Cell::new(None),
            pending_show: Cell::new(false),
            find_widget_focused: Cell::new(false),
        });

        let weak = Rc::downgrade(&this);
        this.base.register(delegate.has_find_result().listen(move |has_result| {
            if let Some(this) = weak.upgrade() {
                this.update_buttons(*has_result);
                this.focus_find_box();
            }
        }));

        let weak = Rc::downgrade(&this);
        this.base.register(delegate.on_did_stop_find().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_buttons(false);
            }
        }));

        this
    }

    // ---- SimpleFindWidget ----

    /// `inputValue`.
    fn input_value(&self) -> String {
        self.find_input.value.borrow().clone()
    }

    /// O `onInput` do `FindInput`, tratado por `SimpleFindWidget` (sem `showResultCount` nem histórico atrasado).
    fn on_find_input(&self, from_composition_end: bool) {
        let check_ime = self.delegate.check_ime_completion_state();
        if !check_ime || !self.find_input.ime_session_in_progress.get() {
            self.found_match.set(self.on_input_changed());
            self.update_buttons(self.found_match.get());
            if !check_ime || !from_composition_end {
                self.focus_find_box();
            }
        }
    }

    /// `updateButtons`.
    fn update_buttons(&self, found_match: bool) {
        let has_input = !self.find_input.value.borrow().is_empty();
        let enabled = self.is_visible.get() && has_input && found_match;
        self.prev_btn.enabled.set(enabled);
        self.next_btn.enabled.set(enabled);
    }

    /// `focusFindBox`: volta o foco ao campo, o que exige focar antes o botão seguinte.
    fn focus_find_box(&self) {
        if self.find_input.ime_session_in_progress.get() {
            return;
        }
        self.focus_model.focus(FocusTarget::NextButton);
        self.focus_model.focus(FocusTarget::Input);
        self.on_focus_moved();
    }

    /// Os `trackFocus` de `_innerDomNode`: o foco dentro do widget liga a chave de contexto, fora dele a desliga.
    fn on_focus_moved(&self) {
        if self.focus_model.active.get().is_some() {
            self.on_focus_tracker_focus();
        } else {
            self.on_focus_tracker_blur();
        }
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible.get()
    }

    /// `show`.
    pub fn show(&self) {
        self.is_visible.set(true);
        self.pending_show.set(true);
    }

    /// `hide`, com o `stopFind(true)` e o `focus()` do delegate que `WebviewFindWidget` acrescenta.
    pub fn hide(&self, animated: bool) {
        if self.is_visible.get() {
            self.aria_hidden.set(true);
            self.pending_hide.set(Some(animated));
        }
        self.delegate.stop_find(Some(true));
        self.delegate.focus();
    }

    /// Entrega os `setTimeout` pendentes de `show` e `hide` (o `await timeout(0)` do teste).
    pub fn run_timers(&self) {
        if self.pending_show.replace(false) {
            self.aria_hidden.set(false);
        }
        if self.pending_hide.replace(None).is_some() {
            self.is_visible.set(false);
            self.update_buttons(self.found_match.get());
        }
    }

    // ---- WebviewFindWidget ----

    /// `find`.
    pub fn find(&self, previous: bool) {
        let val = self.input_value();
        if !val.is_empty() {
            self.delegate.find(&val, previous);
        }
    }

    /// `_onInputChanged`.
    fn on_input_changed(&self) -> bool {
        let val = self.input_value();
        if !val.is_empty() {
            self.delegate.update_find(&val);
        } else {
            self.delegate.stop_find(Some(false));
        }
        false
    }

    /// `_onFocusTrackerFocus`.
    fn on_focus_tracker_focus(&self) {
        self.find_widget_focused.set(true);
    }

    /// `_onFocusTrackerBlur`.
    fn on_focus_tracker_blur(&self) {
        self.find_widget_focused.set(false);
    }

    /// `findFirst`.
    pub fn find_first(&self) {}

    /// `_getResultCount`.
    pub fn get_result_count(&self) -> Option<(usize, usize)> {
        None
    }

    // ---- superfície do modelo de DOM ----

    /// `input.focus()`.
    pub fn focus_input(&self) {
        self.focus_model.focus(FocusTarget::Input);
        self.on_focus_moved();
    }

    /// `document.activeElement === element`.
    pub fn has_focus(&self, target: FocusTarget) -> bool {
        self.focus_model.active.get() == Some(target)
    }

    /// `spy.callCount`.
    pub fn focus_call_count(&self, target: FocusTarget) -> usize {
        self.focus_model.calls.borrow().iter().filter(|t| **t == target).count()
    }

    /// `spy.resetHistory()` (de todos os espiões) e o ponto de partida de `sinon.spy(...)`.
    pub fn reset_focus_history(&self) {
        self.focus_model.calls.borrow_mut().clear();
    }

    /// `spy.calledAfter(other)`: a última chamada de `target` veio depois da última de `other`.
    pub fn focus_called_after(&self, target: FocusTarget, other: FocusTarget) -> bool {
        let calls = self.focus_model.calls.borrow();
        let last = |t: FocusTarget| calls.iter().rposition(|c| *c == t);
        match (last(target), last(other)) {
            (Some(a), Some(b)) => a > b,
            _ => false,
        }
    }

    /// `input.value = value` (não dispara evento).
    pub fn set_input_value(&self, value: &str) {
        *self.find_input.value.borrow_mut() = value.to_string();
    }

    /// `input.dispatchEvent(new CompositionEvent('compositionstart'))`.
    pub fn dispatch_composition_start(&self) {
        self.find_input.ime_session_in_progress.set(true);
    }

    /// `input.dispatchEvent(new CompositionEvent('compositionend', ...))`.
    pub fn dispatch_composition_end(&self) {
        self.find_input.ime_session_in_progress.set(false);
        self.on_find_input(true);
    }

    /// `input.dispatchEvent(new InputEvent('input', ...))`.
    pub fn dispatch_input(&self) {
        self.on_find_input(false);
    }

    /// `nextButton.getAttribute('aria-disabled')`.
    pub fn next_button_aria_disabled(&self) -> &'static str {
        if self.next_btn.enabled.get() { "false" } else { "true" }
    }

    /// `prevBtn` habilitado.
    pub fn prev_button_enabled(&self) -> bool {
        self.prev_btn.enabled.get()
    }

    /// O valor da chave de contexto `webviewFindWidgetFocused`.
    pub fn find_widget_focused(&self) -> bool {
        self.find_widget_focused.get()
    }

    pub fn is_aria_hidden(&self) -> bool {
        self.aria_hidden.get()
    }
}

impl IDisposable for WebviewFindWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}
