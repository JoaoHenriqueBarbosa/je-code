//! Porte de `src/vs/platform/quickinput/browser/quickInput.ts`: a base `QuickInput` e os controladores `QuickPick`,
//! `InputBox` e `QuickWidget` que o `QuickInputController` entrega ao chamador, com os botões, o estado de visibilidade e de
//! foco e os eventos. O contrato que eles implementam (`IQuickInput`, `IQuickPick`, `IInputBox`) é o de
//! [`super::quick_input`] (o `common/quickInput.ts`).
//!
//! O arquivo se chama `quick_input_browser.rs` porque `quick_input.rs` já é o porte do `common/quickInput.ts`.
//!
//! Notas de tradução:
//! - **`QuickInputUI`.** No original é um objeto com os widgets concretos (`ToolBar`, `Button`, `CountBadge`, `ProgressBar`,
//!   `QuickInputBox`, `QuickInputList`...). Aqui é o trait [`IQuickInputUi`], com exatamente as operações que a base e os
//!   controladores executam sobre esses widgets, e dois traits para as partes que o `update()` mais usa: a caixa de texto
//!   ([`IQuickInputBoxUi`]) e a lista ([`IQuickInputListUi`]). O `QuickInputController` (fatia 2) os implementa sobre os widgets
//!   reais. A base só fala com eles: nada de pintura aqui.
//! - **Herança.** `QuickPick`, `InputBox` e `QuickWidget` estendem `QuickInput`: cada um guarda um [`QuickInputBase`] (o estado e o
//!   comportamento comuns) e implementa [`QuickInputUpdate`], que é o `update()` virtual (a base chama o do filho, que chama
//!   [`QuickInputBase::base_update`], o `super.update()`). O `show()` do filho é `show_impl`, que termina em
//!   [`QuickInputBase::show`].
//! - **Identidade de item.** `a === b` entre itens do seletor é [`same_item`] (ver `picker_quick_access`). Os `activeItemsToConfirm`
//!   e `selectedItemsToConfirm` do original (`T[] | null`, comparados por referência com `_activeItems`) são [`ToConfirm`]: nulo,
//!   o mesmo array de agora ou um array já substituído.
//! - **`backButton`.** É um objeto comparado por identidade: [`back_button`] devolve sempre o mesmo `Rc`.
//! - **`titleButtons`.** Só o `.length` é lido, e o original conta o array dos botões da direita como um elemento quando há botões
//!   da esquerda (`[...left, right]`): [`QuickInputBase::title_buttons_len`] repete essa conta.
//! - **`localize`** com `{0}/{1}` é o `je_base::nls::localize` com argumentos em texto.
//! - **`dom.isMouseEvent(event) && event.button === 1`** do `onDidChangeSelection` é o campo `middle_click` de
//!   [`QuickPickSelectionEvent`]; o `keyup` do contêiner (`dom.addDisposableListener(container, KEY_UP)`) é
//!   [`IQuickInputUi::on_container_key_up`]; `renderQuickInputDescription` e o `dom.reset` da mensagem são
//!   [`IQuickInputUi::reset_message`] e [`IQuickInputUi::render_message`].
//!
//! Fica para a fatia 2: o `QuickInputController` (`quickInputController.ts`), a lista (`quickInputList.ts`), a árvore
//! (`quickInputTree`), a `QuickInputBox`, os estilos e as opções (`IQuickInputOptions`, `IQuickInputStyles`), o `QuickTree`, o
//! `renderQuickInputDescription` e o `QuickInputHoverDelegate` (depende do `WorkbenchHoverDelegate`, ainda não portado).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::TimeoutTimer;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event, EventBufferer};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::observable::base::{Observable, SettableExt};
use je_base::observable::observables::observable_value::{ObservableValue, observable_value};
use je_base::platform::Platform;
use je_base::severity::Severity;
use je_base::themables::ThemeIcon;

use super::picker_quick_access::same_item;
use super::quick_input::{
    IInputBox, IKeyMods, IQuickInput, IQuickInputButton, IQuickInputHideEvent, IQuickNavigateConfiguration, IQuickPick, IQuickPickDidAcceptEvent,
    IQuickPickItem, IQuickPickItemButtonEvent, IQuickPickSeparatorButtonEvent, IQuickPickWillAcceptEvent, ItemActivation, MatchOnLabelMode,
    NO_KEY_MODS, QuickInputAnchor, QuickInputAnchorPosition, QuickInputButtonLocation, QuickInputButtonRef, QuickInputHideReason, QuickInputType,
    QuickPickFocus, QuickPickInput, QuickPickOk,
};
use super::quick_input_utils::{OnTrigger, quick_input_button_to_action, quick_input_buttons_to_action_arrays};
use crate::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, RawContextKey};
use crate::contextview::view_element::ViewElement;
use je_base::actions::ActionRef;
use je_base::promise::Promise;
use je_base::promise::MaybePromise;

// ---------------------------------------------------------------------------------------------
// Chaves de contexto e o botão de voltar.
// ---------------------------------------------------------------------------------------------

/// `inQuickInputContextKeyValue`.
pub const IN_QUICK_INPUT_CONTEXT_KEY_VALUE: &str = "inQuickInput";
/// `quickInputAlignmentContextKeyValue`.
pub const QUICK_INPUT_ALIGNMENT_CONTEXT_KEY_VALUE: &str = "quickInputAlignment";
/// `quickInputTypeContextKeyValue`.
pub const QUICK_INPUT_TYPE_CONTEXT_KEY_VALUE: &str = "quickInputType";
/// `endOfQuickInputBoxContextKeyValue`.
pub const END_OF_QUICK_INPUT_BOX_CONTEXT_KEY_VALUE: &str = "cursorAtEndOfQuickInputBox";

/// `InQuickInputContextKey`.
pub fn in_quick_input_context_key() -> RawContextKey {
    RawContextKey::new(
        IN_QUICK_INPUT_CONTEXT_KEY_VALUE,
        false,
        localize("inQuickInput", "Whether keyboard focus is inside the quick input control", &[]),
    )
}

/// `inQuickInputContext`.
pub fn in_quick_input_context() -> ContextKeyExpression {
    ContextKeyExpr::has(IN_QUICK_INPUT_CONTEXT_KEY_VALUE)
}

/// `QuickInputAlignmentContextKey`.
pub fn quick_input_alignment_context_key() -> RawContextKey {
    RawContextKey::new(QUICK_INPUT_ALIGNMENT_CONTEXT_KEY_VALUE, Dynamic::string("top"), localize("quickInputAlignment", "The alignment of the quick input", &[]))
}

/// `QuickInputTypeContextKey`.
pub fn quick_input_type_context_key() -> RawContextKey {
    RawContextKey::new(
        QUICK_INPUT_TYPE_CONTEXT_KEY_VALUE,
        Dynamic::Undefined,
        localize("quickInputType", "The type of the currently visible quick input", &[]),
    )
}

/// `EndOfQuickInputBoxContextKey`.
pub fn end_of_quick_input_box_context_key() -> RawContextKey {
    RawContextKey::new(
        END_OF_QUICK_INPUT_BOX_CONTEXT_KEY_VALUE,
        false,
        localize("cursorAtEndOfQuickInputBox", "Whether the cursor in the quick input is at the end of the input box", &[]),
    )
}

/// `endOfQuickInputBoxContext`.
pub fn end_of_quick_input_box_context() -> ContextKeyExpression {
    ContextKeyExpr::has(END_OF_QUICK_INPUT_BOX_CONTEXT_KEY_VALUE)
}

thread_local! {
    static BACK_BUTTON: QuickInputButtonRef = Rc::new(IQuickInputButton {
        icon_class: Some(ThemeIcon { id: "quick-input-back".to_owned(), color: None }.as_class_name()),
        tooltip: Some(localize("quickInput.back", "Back", &[])),
        ..Default::default()
    });
}

/// `backButton`: o mesmo botão (a mesma identidade) em toda chamada. O `handle: -1` do original não existe no contrato dos botões.
pub fn back_button() -> QuickInputButtonRef {
    BACK_BUTTON.with(Rc::clone)
}

// ---------------------------------------------------------------------------------------------
// O que a base e os controladores pedem aos widgets.
// ---------------------------------------------------------------------------------------------

/// `Visibilities`: o que o `ui.setVisibilities` mostra (campo ausente é `undefined`, ou seja, escondido).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Visibilities {
    pub title: Option<bool>,
    pub description: Option<bool>,
    pub check_all: Option<bool>,
    pub input_box: Option<bool>,
    pub check_box: Option<bool>,
    pub visible_count: Option<bool>,
    pub count: Option<bool>,
    pub message: Option<bool>,
    pub list: Option<bool>,
    pub tree: Option<bool>,
    pub ok: Option<bool>,
    pub custom_button: Option<bool>,
    pub progress_bar: Option<bool>,
}

/// O que `ui.inputBox.stylesForType(severity)` devolve: as cores da mensagem (`undefined` vira `None`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickInputMessageStyles {
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub border: Option<String>,
}

/// O `ui.message.style.{color, backgroundColor, border, marginBottom}` de `showMessageDecoration`: texto vazio é `''`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickInputMessageDecoration {
    pub color: String,
    pub background_color: String,
    pub border: String,
    pub margin_bottom: String,
}

/// O `{ keyCode, ctrlKey, altKey, metaKey, shiftKey }` do `StandardKeyboardEvent` de um `keyup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuickInputKeyUp {
    pub key_code: KeyCode,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
}

/// O `{ items, event }` de `ui.list.onDidChangeSelection`; `middle_click` é `isMouseEvent(event) && event.button === 1`.
#[derive(Clone, Debug)]
pub struct QuickPickSelectionEvent {
    pub items: Vec<IQuickPickItem>,
    pub middle_click: bool,
}

/// `QuickInputBox`, o que os controladores usam dela.
pub trait IQuickInputBoxUi {
    fn value(&self) -> String;
    fn set_value(&self, value: &str);
    fn placeholder(&self) -> String;
    fn set_placeholder(&self, placeholder: &str);
    fn aria_label(&self) -> Option<String>;
    fn set_aria_label(&self, aria_label: &str);
    fn password(&self) -> bool;
    fn set_password(&self, password: bool);
    /// `select(range)`: sem intervalo, seleciona tudo.
    fn select(&self, range: Option<(u32, u32)>);
    /// `getSelection()` como `(start, end)`.
    fn get_selection(&self) -> Option<(u32, u32)>;
    fn has_focus(&self) -> bool;
    fn set_focus(&self);
    fn on_did_change(&self) -> Event<String>;
    fn show_decoration(&self, severity: Severity);
    fn styles_for_type(&self, severity: Severity) -> QuickInputMessageStyles;
    /// `actions = ...`: as ações dos botões de entrada.
    fn set_actions(&self, actions: Vec<ActionRef>);
}

/// `QuickInputList`, o que o `QuickPick` usa dela.
pub trait IQuickInputListUi {
    /// `filter(query)`: devolve se a lista foi refiltrada.
    fn filter(&self, query: &str) -> bool;
    fn focus(&self, focus: QuickPickFocus);
    fn clear_focus(&self);
    fn dom_focus(&self);
    fn scroll_top(&self) -> f64;
    fn set_scroll_top(&self, scroll_top: f64);
    fn set_elements(&self, elements: &[QuickPickInput]);
    fn get_checked_elements(&self) -> Vec<IQuickPickItem>;
    fn set_checked_elements(&self, items: &[IQuickPickItem]);
    fn set_selected_elements(&self, items: &[IQuickPickItem]);
    fn set_focused_elements(&self, items: &[IQuickPickItem]);
    fn set_should_loop(&self, should_loop: bool);
    fn set_match_on_description(&self, value: bool);
    fn set_match_on_detail(&self, value: bool);
    fn set_match_on_label(&self, value: bool);
    fn set_match_on_label_mode(&self, value: MatchOnLabelMode);
    fn set_sort_by_label(&self, value: bool);
    fn aria_label(&self) -> Option<String>;
    fn set_aria_label(&self, aria_label: Option<&str>);
    fn on_did_change_focus(&self) -> Event<Vec<IQuickPickItem>>;
    fn on_did_change_selection(&self) -> Event<QuickPickSelectionEvent>;
    fn on_changed_checked_elements(&self) -> Event<Vec<IQuickPickItem>>;
    fn on_button_triggered(&self) -> Event<IQuickPickItemButtonEvent>;
    fn on_separator_button_triggered(&self) -> Event<IQuickPickSeparatorButtonEvent>;
}

/// `QuickInputUI`: a interface que o `QuickInputController` entrega à base.
pub trait IQuickInputUi {
    fn container(&self) -> Rc<ViewElement>;
    fn input_box(&self) -> Rc<dyn IQuickInputBoxUi>;
    fn list(&self) -> Rc<dyn IQuickInputListUi>;

    /// `ui.title.textContent`.
    fn title_text(&self) -> String;
    fn set_title_text(&self, text: &str);
    /// `ui.description1.textContent` (a `description2` recebe o mesmo texto).
    fn description_text(&self) -> String;
    fn set_description_text(&self, text: &str);

    /// `progressBar.infinite()` e `getContainer().removeAttribute('aria-hidden')`.
    fn show_progress_infinite(&self);
    /// `progressBar.stop()` e `getContainer().setAttribute('aria-hidden', 'true')`.
    fn stop_progress(&self);

    fn set_left_actions(&self, primary: Vec<ActionRef>, secondary: Vec<ActionRef>);
    fn set_right_actions(&self, primary: Vec<ActionRef>, secondary: Vec<ActionRef>);
    fn set_inline_actions(&self, primary: Vec<ActionRef>, secondary: Vec<ActionRef>);
    /// `countContainer.style.right`.
    fn set_count_container_right(&self, right: &str);

    /// `dom.reset(ui.message)`.
    fn reset_message(&self);
    /// `renderQuickInputDescription(message, ui.message, { callback: linkOpenerDelegate, disposables })`.
    fn render_message(&self, message: &str, disposables: &DisposableStore);
    fn set_message_decoration(&self, decoration: &QuickInputMessageDecoration);

    fn set_ok_label(&self, label: &str);
    fn set_custom_button_label(&self, label: &str);
    /// `customButton.element.title`.
    fn set_custom_button_hover(&self, hover: &str);
    fn set_custom_button_secondary(&self, secondary: bool);

    /// `dom.reset(ui.widget, widget?)`.
    fn reset_widget(&self, widget: Option<Rc<ViewElement>>);

    fn on_did_accept(&self) -> Event<()>;
    fn on_did_custom(&self) -> Event<()>;
    fn on_did_trigger_button(&self) -> Event<QuickInputButtonRef>;
    fn on_container_key_up(&self) -> Event<QuickInputKeyUp>;

    fn set_ignore_focus_out(&self, ignore_focus_out: bool);
    fn key_mods(&self) -> IKeyMods;
    fn show(&self, controller: Rc<dyn IQuickInput>);
    fn set_visibilities(&self, visibilities: Visibilities);
    fn set_enabled(&self, enabled: bool);
    fn set_context_key(&self, context_key: Option<&str>);
    fn link_opener_delegate(&self, content: &str);
    fn hide(&self);
}

// ---------------------------------------------------------------------------------------------
// A base `QuickInput`.
// ---------------------------------------------------------------------------------------------

/// O `update()` virtual: a base o chama pelo filho, que termina em [`QuickInputBase::base_update`].
pub trait QuickInputUpdate {
    fn update(&self);
}

/// `QuickInput.noPromptMessage`.
fn no_prompt_message() -> String {
    localize("inputModeEntry", "Press 'Enter' to confirm your input or 'Escape' to cancel", &[])
}

fn is_ios() -> bool {
    Platform::detect().is_ios
}

/// O que `QuickInput` guarda e faz, comum aos três controladores.
pub struct QuickInputBase {
    base: Rc<Disposable>,
    this: Weak<QuickInputBase>,
    ui: Rc<dyn IQuickInputUi>,
    quick_input_type: QuickInputType,
    update_target: RefCell<Option<Weak<dyn QuickInputUpdate>>>,
    owner: RefCell<Option<Weak<dyn IQuickInput>>>,
    visible: Rc<ObservableValue<bool, ()>>,
    title: RefCell<Option<String>>,
    description: RefCell<Option<String>>,
    steps: Cell<Option<u32>>,
    total_steps: Cell<Option<u32>>,
    enabled: Cell<bool>,
    context_key: RefCell<Option<String>>,
    busy: Cell<bool>,
    ignore_focus_out: Cell<bool>,
    left_buttons: RefCell<Vec<QuickInputButtonRef>>,
    right_buttons: RefCell<Vec<QuickInputButtonRef>>,
    inline_buttons: RefCell<Vec<QuickInputButtonRef>>,
    input_buttons: RefCell<Vec<QuickInputButtonRef>>,
    buttons_updated: Cell<bool>,
    no_validation_message: RefCell<Option<String>>,
    validation_message: RefCell<Option<String>>,
    last_validation_message: RefCell<Option<String>>,
    severity: Cell<Severity>,
    last_severity: Cell<Option<Severity>>,
    anchor: Cell<Option<QuickInputAnchor>>,
    anchor_position: Cell<Option<QuickInputAnchorPosition>>,
    on_did_trigger_button: Rc<Emitter<QuickInputButtonRef>>,
    on_did_hide: Rc<Emitter<IQuickInputHideEvent>>,
    on_will_hide: Rc<Emitter<IQuickInputHideEvent>>,
    on_dispose: Rc<Emitter<()>>,
    visible_disposables: Rc<DisposableStore>,
    busy_delay: RefCell<Option<Rc<TimeoutTimer>>>,
}

/// Gera os pares `get`/`set` que só guardam o valor e mandam atualizar.
macro_rules! base_props {
    ($($get:ident, $set:ident, $field:ident: $ty:ty;)*) => {$(
        pub fn $get(&self) -> $ty {
            self.$field.borrow().clone()
        }

        pub fn $set(&self, value: $ty) {
            *self.$field.borrow_mut() = value;
            self.update();
        }
    )*};
}

impl QuickInputBase {
    pub fn new(ui: Rc<dyn IQuickInputUi>, quick_input_type: QuickInputType) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_trigger_button = Emitter::new();
        let on_did_hide = Emitter::new();
        let on_will_hide = Emitter::new();
        let on_dispose = Emitter::new();
        base.register(on_did_trigger_button.clone());
        base.register(on_did_hide.clone());
        base.register(on_will_hide.clone());
        base.register(on_dispose.clone());
        let visible_disposables = DisposableStore::new();
        base.register(visible_disposables.clone());

        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            ui,
            quick_input_type,
            update_target: RefCell::new(None),
            owner: RefCell::new(None),
            visible: observable_value("visible", false),
            title: RefCell::new(None),
            description: RefCell::new(None),
            steps: Cell::new(None),
            total_steps: Cell::new(None),
            enabled: Cell::new(true),
            context_key: RefCell::new(None),
            busy: Cell::new(false),
            ignore_focus_out: Cell::new(false),
            left_buttons: RefCell::new(Vec::new()),
            right_buttons: RefCell::new(Vec::new()),
            inline_buttons: RefCell::new(Vec::new()),
            input_buttons: RefCell::new(Vec::new()),
            buttons_updated: Cell::new(false),
            no_validation_message: RefCell::new(Some(no_prompt_message())),
            validation_message: RefCell::new(None),
            last_validation_message: RefCell::new(None),
            severity: Cell::new(Severity::Ignore),
            last_severity: Cell::new(None),
            anchor: Cell::new(None),
            anchor_position: Cell::new(None),
            on_did_trigger_button,
            on_did_hide,
            on_will_hide,
            on_dispose,
            visible_disposables,
            busy_delay: RefCell::new(None),
        })
    }

    /// Liga a base ao controlador que a guarda: o `update()` do filho e o `this` que `ui.show(this)` recebe.
    pub fn bind(&self, update_target: Weak<dyn QuickInputUpdate>, owner: Weak<dyn IQuickInput>) {
        *self.update_target.borrow_mut() = Some(update_target);
        *self.owner.borrow_mut() = Some(owner);
    }

    pub fn ui(&self) -> &Rc<dyn IQuickInputUi> {
        &self.ui
    }

    /// `register(...)` dos filhos: descartados junto com o controlador.
    pub fn register(&self, disposable: DisposableRef) -> DisposableRef {
        self.base.register(disposable)
    }

    pub fn quick_input_type(&self) -> QuickInputType {
        self.quick_input_type
    }

    /// `protected visibleDisposables`.
    pub fn visible_disposables(&self) -> &Rc<DisposableStore> {
        &self.visible_disposables
    }

    /// `protected _visible`.
    pub fn visible_observable(&self) -> &Rc<ObservableValue<bool, ()>> {
        &self.visible
    }

    /// `protected get visible()`.
    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }

    /// `protected noValidationMessage`.
    pub fn no_validation_message(&self) -> Option<String> {
        self.no_validation_message.borrow().clone()
    }

    pub fn set_no_validation_message(&self, message: Option<String>) {
        *self.no_validation_message.borrow_mut() = message;
    }

    /// O `update()` do controlador (o virtual): o do filho, se já está ligado.
    pub fn update(&self) {
        let target = self.update_target.borrow().as_ref().and_then(Weak::upgrade);
        match target {
            Some(target) => target.update(),
            None => self.base_update(),
        }
    }

    base_props! {
        title, set_title, title: Option<String>;
        description, set_description, description: Option<String>;
        context_key, set_context_key, context_key: Option<String>;
        validation_message, set_validation_message, validation_message: Option<String>;
    }

    pub fn step(&self) -> Option<u32> {
        self.steps.get()
    }

    pub fn set_step(&self, step: Option<u32>) {
        self.steps.set(step);
        self.update();
    }

    pub fn total_steps(&self) -> Option<u32> {
        self.total_steps.get()
    }

    pub fn set_total_steps(&self, total_steps: Option<u32>) {
        self.total_steps.set(total_steps);
        self.update();
    }

    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
        self.update();
    }

    pub fn busy(&self) -> bool {
        self.busy.get()
    }

    pub fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.update();
    }

    pub fn ignore_focus_out(&self) -> bool {
        self.ignore_focus_out.get()
    }

    pub fn set_ignore_focus_out(&self, ignore_focus_out: bool) {
        let should_update = self.ignore_focus_out.get() != ignore_focus_out && !is_ios();
        self.ignore_focus_out.set(ignore_focus_out && !is_ios());
        if should_update {
            self.update();
        }
    }

    pub fn severity(&self) -> Severity {
        self.severity.get()
    }

    pub fn set_severity(&self, severity: Severity) {
        self.severity.set(severity);
        self.update();
    }

    pub fn anchor(&self) -> Option<QuickInputAnchor> {
        self.anchor.get()
    }

    pub fn set_anchor(&self, anchor: Option<QuickInputAnchor>) {
        self.anchor.set(anchor);
    }

    pub fn anchor_position(&self) -> Option<QuickInputAnchorPosition> {
        self.anchor_position.get()
    }

    pub fn set_anchor_position(&self, anchor_position: Option<QuickInputAnchorPosition>) {
        self.anchor_position.set(anchor_position);
    }

    /// `protected get titleButtons()`, só pelo `.length` (ver as notas do módulo).
    pub fn title_buttons_len(&self) -> usize {
        let left = self.left_buttons.borrow().len();
        if left > 0 { left + 1 } else { self.right_buttons.borrow().len() }
    }

    /// `get buttons()`.
    pub fn buttons(&self) -> Vec<QuickInputButtonRef> {
        [&self.left_buttons, &self.right_buttons, &self.inline_buttons, &self.input_buttons].into_iter().flat_map(|buttons| buttons.borrow().clone()).collect()
    }

    /// `set buttons(buttons)`.
    pub fn set_buttons(&self, buttons: Vec<QuickInputButtonRef>) {
        let (mut left, mut right, mut inline, mut input) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let back = back_button();
        for button in buttons {
            if Rc::ptr_eq(&button, &back) {
                left.push(button);
            } else {
                match button.location {
                    Some(QuickInputButtonLocation::Inline) => inline.push(button),
                    Some(QuickInputButtonLocation::Input) => input.push(button),
                    _ => right.push(button),
                }
            }
        }
        *self.left_buttons.borrow_mut() = left;
        *self.right_buttons.borrow_mut() = right;
        *self.inline_buttons.borrow_mut() = inline;
        *self.input_buttons.borrow_mut() = input;
        self.buttons_updated.set(true);
        self.update();
    }

    pub fn on_did_trigger_button(&self) -> Event<QuickInputButtonRef> {
        self.on_did_trigger_button.event()
    }

    pub fn on_did_hide(&self) -> Event<IQuickInputHideEvent> {
        self.on_did_hide.event()
    }

    pub fn on_will_hide(&self) -> Event<IQuickInputHideEvent> {
        self.on_will_hide.event()
    }

    pub fn on_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }

    /// `show()`.
    pub fn show(&self) {
        if self.is_visible() {
            return;
        }
        let weak = self.this.clone();
        self.visible_disposables.add(self.ui.on_did_trigger_button().listen(move |button| {
            let Some(this) = weak.upgrade() else { return };
            if this.buttons().iter().any(|candidate| Rc::ptr_eq(candidate, button)) {
                this.on_did_trigger_button.fire(button.clone());
            }
        }));
        let owner = self.owner.borrow().as_ref().and_then(Weak::upgrade).expect("QuickInputBase::bind must run before show");
        self.ui.show(owner);

        // update properties in the controller that get reset in the ui.show() call
        self.visible.set(true, None);
        // This ensures the message/prompt gets rendered
        *self.last_validation_message.borrow_mut() = None;
        // This ensures the input box has the right severity applied
        self.last_severity.set(None);
        if !self.buttons().is_empty() {
            // if there are buttons, the ui.show() clears them out of the UI so we should rerender them.
            self.buttons_updated.set(true);
        }

        self.update();
    }

    /// `hide()`.
    pub fn hide(&self) {
        if !self.is_visible() {
            return;
        }
        self.ui.hide();
    }

    /// `didHide(reason)`.
    pub fn did_hide(&self, reason: Option<QuickInputHideReason>) {
        self.visible.set(false, None);
        self.visible_disposables.clear();
        self.on_did_hide.fire(IQuickInputHideEvent { reason: reason.unwrap_or(QuickInputHideReason::Other) });
    }

    /// `willHide(reason)`.
    pub fn will_hide(&self, reason: Option<QuickInputHideReason>) {
        self.on_will_hide.fire(IQuickInputHideEvent { reason: reason.unwrap_or(QuickInputHideReason::Other) });
    }

    /// O `update()` da base (o `super.update()` dos filhos).
    pub fn base_update(&self) {
        if !self.is_visible() {
            return;
        }
        let ui = &self.ui;
        let title = self.get_title();
        if !title.is_empty() && ui.title_text() != title {
            ui.set_title_text(&title);
        } else if title.is_empty() && ui.title_text() != "\u{a0}" {
            ui.set_title_text("\u{a0}");
        }
        let description = self.get_description();
        if ui.description_text() != description {
            ui.set_description_text(&description);
        }
        self.update_busy();
        if self.buttons_updated.get() {
            self.buttons_updated.set(false);
            self.update_buttons();
        }
        ui.set_ignore_focus_out(self.ignore_focus_out());
        ui.set_enabled(self.enabled());
        ui.set_context_key(self.context_key().as_deref());

        let validation_message = self.validation_message().filter(|message| !message.is_empty()).or_else(|| self.no_validation_message().filter(|message| !message.is_empty()));
        if *self.last_validation_message.borrow() != validation_message {
            *self.last_validation_message.borrow_mut() = validation_message.clone();
            ui.reset_message();
            if let Some(message) = validation_message {
                ui.render_message(&message, &self.visible_disposables);
            }
        }
        if self.last_severity.get() != Some(self.severity()) {
            self.last_severity.set(Some(self.severity()));
            self.show_message_decoration(self.severity());
        }
    }

    fn update_busy(&self) {
        if self.busy() && self.busy_delay.borrow().is_none() {
            let timer = TimeoutTimer::new();
            let weak = self.this.clone();
            timer.set_if_not_set(
                move || {
                    if let Some(this) = weak.upgrade().filter(|this| this.is_visible()) {
                        this.ui.show_progress_infinite();
                    }
                },
                800,
            );
            *self.busy_delay.borrow_mut() = Some(timer);
        }
        if !self.busy() {
            let timer = self.busy_delay.borrow_mut().take();
            if let Some(timer) = timer {
                self.ui.stop_progress();
                timer.cancel();
            }
        }
    }

    fn trigger_handler(&self) -> OnTrigger {
        let weak = self.this.clone();
        Rc::new(move |button: &QuickInputButtonRef| {
            if let Some(this) = weak.upgrade() {
                this.on_did_trigger_button.fire(button.clone());
            }
            MaybePromise::Value(Dynamic::Undefined)
        })
    }

    fn update_buttons(&self) {
        let ui = &self.ui;
        let on_trigger = self.trigger_handler();
        let left = quick_input_buttons_to_action_arrays(&self.left_buttons.borrow(), "left-button", on_trigger.clone());
        ui.set_left_actions(left.primary, left.secondary);
        let right = quick_input_buttons_to_action_arrays(&self.right_buttons.borrow(), "right-button", on_trigger.clone());
        ui.set_right_actions(right.primary, right.secondary);
        let inline = quick_input_buttons_to_action_arrays(&self.inline_buttons.borrow(), "inline-button", on_trigger);
        ui.set_inline_actions(inline.primary, inline.secondary);
        // Adjust count badge position based on input buttons (each button/toggle is ~22px wide)
        let input_buttons = self.input_buttons.borrow().clone();
        let input_button_offset = input_buttons.len() * 22;
        ui.set_count_container_right(&if input_button_offset > 0 { format!("{}px", 4 + input_button_offset) } else { "4px".to_owned() });
        let actions = input_buttons
            .iter()
            .enumerate()
            .map(|(index, button)| {
                let weak = self.this.clone();
                let triggered = button.clone();
                quick_input_button_to_action(
                    button,
                    &format!("id-{index}"),
                    Rc::new(move || {
                        if let Some(this) = weak.upgrade() {
                            this.on_did_trigger_button.fire(triggered.clone());
                        }
                        MaybePromise::Promise(Promise::resolved(Dynamic::Undefined))
                    }),
                )
            })
            .collect();
        ui.input_box().set_actions(actions);
    }

    fn get_title(&self) -> String {
        let title = self.title().filter(|title| !title.is_empty());
        let step = self.step().filter(|step| *step != 0);
        match (title, step) {
            (Some(title), Some(_)) => format!("{title} ({})", self.get_steps()),
            (Some(title), None) => title,
            (None, Some(_)) => self.get_steps(),
            (None, None) => String::new(),
        }
    }

    fn get_description(&self) -> String {
        self.description().unwrap_or_default()
    }

    fn get_steps(&self) -> String {
        match (self.step().filter(|step| *step != 0), self.total_steps().filter(|total| *total != 0)) {
            (Some(step), Some(total)) => localize("quickInput.steps", "{0}/{1}", &[&step.to_string(), &total.to_string()]),
            (Some(step), None) => step.to_string(),
            _ => String::new(),
        }
    }

    /// `protected showMessageDecoration(severity)`.
    pub fn show_message_decoration(&self, severity: Severity) {
        let input_box = self.ui.input_box();
        input_box.show_decoration(severity);
        let decoration = if severity != Severity::Ignore {
            let styles = input_box.styles_for_type(severity);
            QuickInputMessageDecoration {
                color: styles.foreground.unwrap_or_default(),
                background_color: styles.background.unwrap_or_default(),
                border: styles.border.map(|border| format!("1px solid {border}")).unwrap_or_default(),
                margin_bottom: "-2px".to_owned(),
            }
        } else {
            QuickInputMessageDecoration::default()
        };
        self.ui.set_message_decoration(&decoration);
    }

    /// `dispose()`.
    pub fn dispose(&self) {
        self.hide();
        self.on_dispose.fire(());
        self.base.dispose();
    }
}

/// O `impl IQuickInput` de um controlador: tudo repassa à base, menos `show`, que é o `show_impl` do filho.
macro_rules! impl_quick_input {
    ($ty:ty) => {
        impl IDisposable for $ty {
            fn dispose(&self) {
                self.core.dispose();
            }
        }

        impl IQuickInput for $ty {
            fn quick_input_type(&self) -> QuickInputType {
                self.core.quick_input_type()
            }
            fn on_did_hide(&self) -> Event<IQuickInputHideEvent> {
                self.core.on_did_hide()
            }
            fn on_will_hide(&self) -> Event<IQuickInputHideEvent> {
                self.core.on_will_hide()
            }
            fn on_dispose(&self) -> Event<()> {
                self.core.on_dispose()
            }
            fn on_did_trigger_button(&self) -> Event<QuickInputButtonRef> {
                self.core.on_did_trigger_button()
            }
            fn title(&self) -> Option<String> {
                self.core.title()
            }
            fn set_title(&self, value: Option<String>) {
                self.core.set_title(value);
            }
            fn description(&self) -> Option<String> {
                self.core.description()
            }
            fn set_description(&self, value: Option<String>) {
                self.core.set_description(value);
            }
            fn step(&self) -> Option<u32> {
                self.core.step()
            }
            fn set_step(&self, value: Option<u32>) {
                self.core.set_step(value);
            }
            fn total_steps(&self) -> Option<u32> {
                self.core.total_steps()
            }
            fn set_total_steps(&self, value: Option<u32>) {
                self.core.set_total_steps(value);
            }
            fn buttons(&self) -> Vec<QuickInputButtonRef> {
                self.core.buttons()
            }
            fn set_buttons(&self, value: Vec<QuickInputButtonRef>) {
                self.core.set_buttons(value);
            }
            fn enabled(&self) -> bool {
                self.core.enabled()
            }
            fn set_enabled(&self, value: bool) {
                self.core.set_enabled(value);
            }
            fn context_key(&self) -> Option<String> {
                self.core.context_key()
            }
            fn set_context_key(&self, value: Option<String>) {
                self.core.set_context_key(value);
            }
            fn busy(&self) -> bool {
                self.core.busy()
            }
            fn set_busy(&self, value: bool) {
                self.core.set_busy(value);
            }
            fn ignore_focus_out(&self) -> bool {
                self.core.ignore_focus_out()
            }
            fn set_ignore_focus_out(&self, value: bool) {
                self.core.set_ignore_focus_out(value);
            }
            fn anchor(&self) -> Option<QuickInputAnchor> {
                self.core.anchor()
            }
            fn set_anchor(&self, value: Option<QuickInputAnchor>) {
                self.core.set_anchor(value);
            }
            fn anchor_position(&self) -> Option<QuickInputAnchorPosition> {
                self.core.anchor_position()
            }
            fn set_anchor_position(&self, value: Option<QuickInputAnchorPosition>) {
                self.core.set_anchor_position(value);
            }
            fn show(&self) {
                self.show_impl();
            }
            fn hide(&self) {
                self.core.hide();
            }
            fn did_hide(&self, reason: Option<QuickInputHideReason>) {
                self.core.did_hide(reason);
            }
            fn will_hide(&self, reason: Option<QuickInputHideReason>) {
                self.core.will_hide(reason);
            }
        }
    };
}

/// Pares `get`/`set` de um filho: guardam o valor e mandam atualizar (`$upd = update`).
macro_rules! props {
    ($($get:ident, $set:ident, $field:ident: $ty:ty, $upd:ident;)*) => {$(
        fn $get(&self) -> $ty {
            self.$field.borrow().clone()
        }

        fn $set(&self, value: $ty) {
            *self.$field.borrow_mut() = value;
            props!(@$upd self);
        }
    )*};
    (@update $self_:ident) => {
        $self_.core.update()
    };
}

/// Amarra o controlador recém-criado à base e o devolve.
fn bind_core<T: QuickInputUpdate + IQuickInput + 'static>(this: Rc<T>, core: &QuickInputBase) -> Rc<T> {
    let update_target: Weak<dyn QuickInputUpdate> = Rc::downgrade(&this) as Weak<dyn QuickInputUpdate>;
    let owner: Weak<dyn IQuickInput> = Rc::downgrade(&this) as Weak<dyn IQuickInput>;
    core.bind(update_target, owner);
    this
}

fn same_items(a: &[IQuickPickItem], b: &[IQuickPickItem]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_item(x, y))
}

/// `activeItemsToConfirm` / `selectedItemsToConfirm`: `null`, o mesmo array que `_activeItems` guarda agora, ou um array que
/// já foi trocado (a comparação `!==` do original só é falsa no segundo caso).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToConfirm {
    Null,
    Same,
    Stale,
}

// ---------------------------------------------------------------------------------------------
// QuickPick.
// ---------------------------------------------------------------------------------------------

/// `QuickPick`.
pub struct QuickPick {
    core: Rc<QuickInputBase>,
    this: Weak<QuickPick>,
    value: RefCell<String>,
    aria_label: RefCell<Option<String>>,
    placeholder: RefCell<Option<String>>,
    filter_value: RefCell<Rc<dyn Fn(&str) -> String>>,
    on_did_change_value: Rc<Emitter<String>>,
    on_will_accept: Rc<Emitter<IQuickPickWillAcceptEvent>>,
    on_did_accept: Rc<Emitter<IQuickPickDidAcceptEvent>>,
    on_did_custom: Rc<Emitter<()>>,
    items: RefCell<Vec<QuickPickInput>>,
    items_updated: Cell<bool>,
    can_select_many: Cell<bool>,
    can_accept_in_background: Cell<bool>,
    match_on_description: Cell<bool>,
    match_on_detail: Cell<bool>,
    match_on_label: Cell<bool>,
    match_on_label_mode: Cell<MatchOnLabelMode>,
    sort_by_label: Cell<bool>,
    keep_scroll_position: Cell<bool>,
    item_activation: Cell<ItemActivation>,
    active_items: RefCell<Vec<IQuickPickItem>>,
    active_items_updated: Cell<bool>,
    active_items_to_confirm: Cell<ToConfirm>,
    on_did_change_active: Rc<Emitter<Vec<IQuickPickItem>>>,
    selected_items: RefCell<Vec<IQuickPickItem>>,
    selected_items_updated: Cell<bool>,
    selected_items_to_confirm: Cell<ToConfirm>,
    on_did_change_selection: Rc<Emitter<Vec<IQuickPickItem>>>,
    on_did_trigger_item_button: Rc<Emitter<IQuickPickItemButtonEvent>>,
    on_did_trigger_separator_button: Rc<Emitter<IQuickPickSeparatorButtonEvent>>,
    value_selection: Cell<Option<(u32, u32)>>,
    value_selection_updated: Cell<bool>,
    ok: Cell<QuickPickOk>,
    ok_label: RefCell<Option<String>>,
    custom_button: Cell<bool>,
    custom_button_label: RefCell<Option<String>>,
    custom_button_hover: RefCell<Option<String>>,
    custom_button_secondary: Cell<bool>,
    quick_navigate: RefCell<Option<IQuickNavigateConfiguration>>,
    hide_input: Cell<bool>,
    hide_count_badge: Cell<bool>,
    hide_check_all: Cell<bool>,
    focus_event_bufferer: Rc<EventBufferer>,
}

impl QuickPick {
    /// `QuickPick.DEFAULT_ARIA_LABEL`.
    fn default_aria_label() -> String {
        localize("quickInputBox.ariaLabel", "Type to narrow down results.", &[])
    }

    pub fn new(ui: Rc<dyn IQuickInputUi>) -> Rc<Self> {
        let core = QuickInputBase::new(ui, QuickInputType::QuickPick);
        core.set_no_validation_message(None);
        let emitters = (Emitter::new(), Emitter::new(), Emitter::new(), Emitter::new());
        let (on_did_change_active, on_did_change_selection, on_did_trigger_item_button, on_did_trigger_separator_button) =
            (Emitter::new(), Emitter::new(), Emitter::new(), Emitter::new());
        for disposable in [
            emitters.0.clone() as DisposableRef,
            emitters.1.clone(),
            emitters.2.clone(),
            emitters.3.clone(),
            on_did_change_active.clone(),
            on_did_change_selection.clone(),
            on_did_trigger_item_button.clone(),
            on_did_trigger_separator_button.clone(),
        ] {
            core.register(disposable);
        }
        let this = Rc::new_cyclic(|this| Self {
            core: core.clone(),
            this: this.clone(),
            value: RefCell::new(String::new()),
            aria_label: RefCell::new(None),
            placeholder: RefCell::new(None),
            filter_value: RefCell::new(Rc::new(|value: &str| value.to_owned())),
            on_did_change_value: emitters.0,
            on_will_accept: emitters.1,
            on_did_accept: emitters.2,
            on_did_custom: emitters.3,
            items: RefCell::new(Vec::new()),
            items_updated: Cell::new(false),
            can_select_many: Cell::new(false),
            can_accept_in_background: Cell::new(false),
            match_on_description: Cell::new(false),
            match_on_detail: Cell::new(false),
            match_on_label: Cell::new(true),
            match_on_label_mode: Cell::new(MatchOnLabelMode::Fuzzy),
            sort_by_label: Cell::new(true),
            keep_scroll_position: Cell::new(false),
            item_activation: Cell::new(ItemActivation::First),
            active_items: RefCell::new(Vec::new()),
            active_items_updated: Cell::new(false),
            // `activeItemsToConfirm = []` é outro array que `_activeItems`.
            active_items_to_confirm: Cell::new(ToConfirm::Stale),
            on_did_change_active,
            selected_items: RefCell::new(Vec::new()),
            selected_items_updated: Cell::new(false),
            selected_items_to_confirm: Cell::new(ToConfirm::Stale),
            on_did_change_selection,
            on_did_trigger_item_button,
            on_did_trigger_separator_button,
            value_selection: Cell::new(None),
            value_selection_updated: Cell::new(true),
            ok: Cell::new(QuickPickOk::Default),
            ok_label: RefCell::new(None),
            custom_button: Cell::new(false),
            custom_button_label: RefCell::new(None),
            custom_button_hover: RefCell::new(None),
            custom_button_secondary: Cell::new(false),
            quick_navigate: RefCell::new(None),
            hide_input: Cell::new(false),
            hide_count_badge: Cell::new(false),
            hide_check_all: Cell::new(false),
            focus_event_bufferer: EventBufferer::new(),
        });
        bind_core(this, &core)
    }

    /// `_activeItems = items`: troca o array, então o "mesmo array" de `activeItemsToConfirm` fica velho.
    fn replace_active_items(&self, items: Vec<IQuickPickItem>) {
        *self.active_items.borrow_mut() = items;
        if self.active_items_to_confirm.get() == ToConfirm::Same {
            self.active_items_to_confirm.set(ToConfirm::Stale);
        }
    }

    /// `_selectedItems = items`.
    fn replace_selected_items(&self, items: Vec<IQuickPickItem>) {
        *self.selected_items.borrow_mut() = items;
        if self.selected_items_to_confirm.get() == ToConfirm::Same {
            self.selected_items_to_confirm.set(ToConfirm::Stale);
        }
    }

    fn do_set_value(&self, value: &str, skip_update: bool) {
        if *self.value.borrow() == value {
            return;
        }
        *self.value.borrow_mut() = value.to_owned();
        if !skip_update {
            self.core.update();
        }
        if self.core.is_visible() {
            let filter = self.filter_value.borrow().clone();
            let did_filter = self.core.ui().list().filter(&filter(value));
            if did_filter {
                self.try_select_first();
            }
        }
        self.on_did_change_value.fire(value.to_owned());
    }

    fn try_select_first(&self) {
        if !self.can_select_many.get() {
            self.core.ui().list().focus(QuickPickFocus::First);
        }
    }

    fn scroll_top(&self) -> f64 {
        self.core.ui().list().scroll_top()
    }

    fn handle_accept(&self, in_background: bool) {
        // Figure out veto via `onWillAccept` event
        let event = IQuickPickWillAcceptEvent::new();
        self.on_will_accept.fire(event.clone());

        // Continue with `onDidAccept` if no veto
        if !event.is_vetoed() {
            self.on_did_accept.fire(IQuickPickDidAcceptEvent { in_background });
        }
    }

    fn register_quick_navigation(&self) -> DisposableRef {
        let weak = self.this.clone();
        self.core.ui().on_container_key_up().listen(move |event| {
            let Some(this) = weak.upgrade() else { return };
            if this.can_select_many.get() {
                return;
            }
            let Some(quick_navigate) = this.quick_navigate.borrow().clone() else { return };

            // Select element when keys are pressed that signal it
            let was_trigger_key_pressed = quick_navigate.keybindings.iter().any(|keybinding| {
                let chords = keybinding.0.get_chords();
                if chords.len() > 1 {
                    return false;
                }
                let Some(chord) = chords.first() else { return false };

                if chord.shift_key && event.key_code == KeyCode::Shift {
                    if event.ctrl_key || event.alt_key || event.meta_key {
                        return false; // this is an optimistic check for the shift key being used to navigate back in quick input
                    }
                    return true;
                }
                if chord.alt_key && event.key_code == KeyCode::Alt {
                    return true;
                }
                if chord.ctrl_key && event.key_code == KeyCode::Ctrl {
                    return true;
                }
                chord.meta_key && event.key_code == KeyCode::Meta
            });

            if was_trigger_key_pressed {
                let first = this.active_items.borrow().first().cloned();
                if let Some(first) = first {
                    this.replace_selected_items(vec![first]);
                    this.on_did_change_selection.fire(this.selected_items.borrow().clone());
                    this.handle_accept(false);
                }
                // Unset quick navigate after press. It is only valid once and should not result in any behaviour change
                // afterwards if the picker remains open because there was no active item
                *this.quick_navigate.borrow_mut() = None;
            }
        })
    }

    fn show_impl(&self) {
        if !self.core.is_visible() {
            let ui = self.core.ui().clone();
            let list = ui.list();
            let input_box = ui.input_box();
            let store = self.core.visible_disposables();

            let weak = self.this.clone();
            store.add(input_box.on_did_change().listen(move |value| {
                if let Some(this) = weak.upgrade() {
                    // skip update since this originates from the UI
                    this.do_set_value(value, true);
                }
            }));

            let weak = self.this.clone();
            store.add(ui.on_did_accept().listen(move |()| {
                let Some(this) = weak.upgrade() else { return };
                if this.can_select_many.get() {
                    // if there are no checked elements, it means that an onDidChangeSelection never fired to overwrite
                    // `_selectedItems`. In that case, we should emit one with an empty array to ensure that
                    // `.selectedItems` is up to date.
                    if this.core.ui().list().get_checked_elements().is_empty() {
                        this.replace_selected_items(Vec::new());
                        this.on_did_change_selection.fire(this.selected_items.borrow().clone());
                    }
                } else {
                    let first = this.active_items.borrow().first().cloned();
                    if let Some(first) = first {
                        // For single-select, we set `selectedItems` to the item that was accepted.
                        this.replace_selected_items(vec![first]);
                        this.on_did_change_selection.fire(this.selected_items.borrow().clone());
                    }
                }
                this.handle_accept(false);
            }));

            let weak = self.this.clone();
            store.add(ui.on_did_custom().listen(move |()| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_custom.fire(());
                }
            }));

            let weak = self.this.clone();
            // Only fire the last event
            store.add(self.focus_event_bufferer.wrap_event_merge(&list.on_did_change_focus(), |_, event| event.clone()).listen(move |focused_items| {
                let Some(this) = weak.upgrade() else { return };
                if this.active_items_updated.get() {
                    return; // Expect another event.
                }
                if this.active_items_to_confirm.get() != ToConfirm::Same && same_items(focused_items, &this.active_items.borrow()) {
                    return;
                }
                this.replace_active_items(focused_items.clone());
                this.on_did_change_active.fire(focused_items.clone());
            }));

            let weak = self.this.clone();
            store.add(list.on_did_change_selection().listen(move |event| {
                let Some(this) = weak.upgrade() else { return };
                let selected_items = &event.items;
                if this.can_select_many.get() && !selected_items.iter().any(|item| item.pickable == Some(false)) {
                    if !selected_items.is_empty() {
                        this.core.ui().list().set_selected_elements(&[]);
                    }
                    return;
                }
                if this.selected_items_to_confirm.get() != ToConfirm::Same && same_items(selected_items, &this.selected_items.borrow()) {
                    return;
                }
                this.replace_selected_items(selected_items.clone());
                this.on_did_change_selection.fire(selected_items.clone());
                if !selected_items.is_empty() {
                    this.handle_accept(event.middle_click);
                }
            }));

            let weak = self.this.clone();
            store.add(list.on_changed_checked_elements().listen(move |checked_items| {
                let Some(this) = weak.upgrade() else { return };
                if !this.can_select_many.get() || !this.core.is_visible() {
                    return;
                }
                if this.selected_items_to_confirm.get() != ToConfirm::Same && same_items(checked_items, &this.selected_items.borrow()) {
                    return;
                }
                this.replace_selected_items(checked_items.clone());
                this.on_did_change_selection.fire(checked_items.clone());
            }));

            let weak = self.this.clone();
            store.add(list.on_button_triggered().listen(move |event| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_trigger_item_button.fire(event.clone());
                }
            }));
            let weak = self.this.clone();
            store.add(list.on_separator_button_triggered().listen(move |event| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_trigger_separator_button.fire(event.clone());
                }
            }));
            store.add(self.register_quick_navigation());
            self.value_selection_updated.set(true);
        }
        self.core.show();
    }
}

impl QuickInputUpdate for QuickPick {
    fn update(&self) {
        if !self.core.is_visible() {
            return;
        }
        let ui = self.core.ui().clone();
        let list = ui.list();
        let input_box = ui.input_box();

        // store the scrollTop before it is reset
        let scroll_top_before = if self.keep_scroll_position.get() { self.scroll_top() } else { 0.0 };
        let has_description = self.core.description().is_some_and(|description| !description.is_empty());
        let title = self.core.title().is_some_and(|title| !title.is_empty());
        let can_select_many = self.can_select_many.get();
        let prompt = self.core.no_validation_message().is_some_and(|prompt| !prompt.is_empty());
        let visibilities = Visibilities {
            title: Some(title || self.core.step().is_some_and(|step| step != 0) || self.core.title_buttons_len() > 0),
            description: Some(has_description),
            check_all: Some(can_select_many && !self.hide_check_all.get()),
            check_box: Some(can_select_many),
            input_box: Some(!self.hide_input.get()),
            progress_bar: Some(!self.hide_input.get() || has_description),
            visible_count: Some(true),
            count: Some(can_select_many && !self.hide_count_badge.get()),
            ok: Some(match self.ok.get() {
                QuickPickOk::Default => can_select_many,
                QuickPickOk::Bool(ok) => ok,
            }),
            list: Some(true),
            message: Some(self.core.validation_message().is_some_and(|message| !message.is_empty()) || prompt),
            custom_button: Some(self.custom_button.get()),
            ..Default::default()
        };
        ui.set_visibilities(visibilities);
        self.core.base_update();
        let value = self.value.borrow().clone();
        if input_box.value() != value {
            input_box.set_value(&value);
        }
        if self.value_selection_updated.get() {
            self.value_selection_updated.set(false);
            input_box.select(self.value_selection.get());
        }
        let placeholder = self.placeholder.borrow().clone().unwrap_or_default();
        if input_box.placeholder() != placeholder {
            input_box.set_placeholder(&placeholder);
        }

        let mut aria_label = self.aria_label.borrow().clone();
        // Only set aria label to the input box placeholder if we actually have an input box.
        if aria_label.as_deref().is_none_or(str::is_empty) && visibilities.input_box == Some(true) {
            aria_label = self.placeholder.borrow().clone().filter(|placeholder| !placeholder.is_empty());
            // If we have a title, include it in the aria label.
            if let Some(title) = self.core.title().filter(|title| !title.is_empty()) {
                aria_label = Some(match aria_label {
                    Some(label) => format!("{label} - {title}"),
                    None => title,
                });
            }
            if aria_label.is_none() {
                aria_label = Some(Self::default_aria_label());
            }
        }
        if list.aria_label() != aria_label {
            list.set_aria_label(aria_label.as_deref());
        }
        if input_box.aria_label() != aria_label {
            input_box.set_aria_label(aria_label.as_deref().unwrap_or("input"));
        }
        list.set_match_on_description(self.match_on_description.get());
        list.set_match_on_detail(self.match_on_detail.get());
        list.set_match_on_label(self.match_on_label.get());
        list.set_match_on_label_mode(self.match_on_label_mode.get());
        list.set_sort_by_label(self.sort_by_label.get());
        if self.items_updated.get() {
            self.items_updated.set(false);
            let items = self.items.borrow().clone();
            self.focus_event_bufferer.buffer_events(|| {
                list.set_elements(&items);
                // We want focus to exist in the list if there are items so that space can be used to toggle
                list.set_should_loop(!can_select_many);
                let filter = self.filter_value.borrow().clone();
                list.filter(&filter(&input_box.value()));
                match self.item_activation.get() {
                    ItemActivation::None => self.item_activation.set(ItemActivation::First), // only valid once, then unset
                    ItemActivation::Second => {
                        list.focus(QuickPickFocus::Second);
                        self.item_activation.set(ItemActivation::First); // only valid once, then unset
                    }
                    ItemActivation::Last => {
                        list.focus(QuickPickFocus::Last);
                        self.item_activation.set(ItemActivation::First); // only valid once, then unset
                    }
                    ItemActivation::First => self.try_select_first(),
                }
            });
        }
        if ui.container().class_list_contains("show-checkboxes") != can_select_many {
            if can_select_many {
                list.clear_focus();
            } else {
                self.try_select_first();
            }
        }
        if self.active_items_updated.get() {
            self.active_items_updated.set(false);
            self.active_items_to_confirm.set(ToConfirm::Same);
            let active_items = self.active_items.borrow().clone();
            list.set_focused_elements(&active_items);
            if self.active_items_to_confirm.get() == ToConfirm::Same {
                self.active_items_to_confirm.set(ToConfirm::Null);
            }
        }
        if self.selected_items_updated.get() {
            self.selected_items_updated.set(false);
            self.selected_items_to_confirm.set(ToConfirm::Same);
            let selected_items = self.selected_items.borrow().clone();
            if can_select_many {
                list.set_checked_elements(&selected_items);
            } else {
                list.set_selected_elements(&selected_items);
            }
            if self.selected_items_to_confirm.get() == ToConfirm::Same {
                self.selected_items_to_confirm.set(ToConfirm::Null);
            }
        }
        ui.set_ok_label(&IQuickPick::ok_label(self).unwrap_or_default());
        ui.set_custom_button_label(&self.custom_button_label.borrow().clone().unwrap_or_default());
        ui.set_custom_button_hover(&self.custom_button_hover.borrow().clone().unwrap_or_default());
        ui.set_custom_button_secondary(self.custom_button_secondary.get());
        if visibilities.input_box != Some(true) {
            // we need to move focus into the tree to detect keybindings properly when the input box is not visible (quick nav)
            list.dom_focus();

            // Focus the first element in the list if multiselect is enabled
            if can_select_many {
                list.focus(QuickPickFocus::First);
            }
        }

        // Set the scroll position to what it was before updating the items
        if self.keep_scroll_position.get() {
            list.set_scroll_top(scroll_top_before);
        }
    }
}

impl_quick_input!(QuickPick);

impl IQuickPick for QuickPick {
    fn on_did_change_value(&self) -> Event<String> {
        self.on_did_change_value.event()
    }
    fn on_will_accept(&self) -> Event<IQuickPickWillAcceptEvent> {
        self.on_will_accept.event()
    }
    fn on_did_accept(&self) -> Event<IQuickPickDidAcceptEvent> {
        self.on_did_accept.event()
    }
    fn on_did_custom(&self) -> Event<()> {
        self.on_did_custom.event()
    }
    fn on_did_trigger_item_button(&self) -> Event<IQuickPickItemButtonEvent> {
        self.on_did_trigger_item_button.event()
    }
    fn on_did_trigger_separator_button(&self) -> Event<IQuickPickSeparatorButtonEvent> {
        self.on_did_trigger_separator_button.event()
    }
    fn on_did_change_active(&self) -> Event<Vec<IQuickPickItem>> {
        self.on_did_change_active.event()
    }
    fn on_did_change_selection(&self) -> Event<Vec<IQuickPickItem>> {
        self.on_did_change_selection.event()
    }

    fn key_mods(&self) -> IKeyMods {
        if self.quick_navigate.borrow().is_some() {
            // Disable keyMods when quick navigate is enabled because in this model the interaction is purely keyboard driven and
            // Ctrl/Alt are typically pressed and hold during this interaction.
            return NO_KEY_MODS;
        }
        self.core.ui().key_mods()
    }

    props! {
        aria_label, set_aria_label, aria_label: Option<String>, update;
        placeholder, set_placeholder, placeholder: Option<String>, update;
        custom_label, set_custom_label, custom_button_label: Option<String>, update;
        custom_hover, set_custom_hover, custom_button_hover: Option<String>, update;
        quick_navigate, set_quick_navigate, quick_navigate: Option<IQuickNavigateConfiguration>, update;
    }

    fn value(&self) -> String {
        self.value.borrow().clone()
    }

    fn set_value(&self, value: String) {
        self.do_set_value(&value, false);
    }

    fn filter_value(&self) -> Rc<dyn Fn(&str) -> String> {
        self.filter_value.borrow().clone()
    }

    fn set_filter_value(&self, value: Rc<dyn Fn(&str) -> String>) {
        *self.filter_value.borrow_mut() = value;
    }

    fn prompt(&self) -> Option<String> {
        self.core.no_validation_message()
    }

    fn set_prompt(&self, value: Option<String>) {
        self.core.set_no_validation_message(value);
        self.core.update();
    }

    fn can_accept_in_background(&self) -> bool {
        self.can_accept_in_background.get()
    }

    fn set_can_accept_in_background(&self, value: bool) {
        self.can_accept_in_background.set(value);
    }

    fn ok(&self) -> QuickPickOk {
        self.ok.get()
    }

    fn set_ok(&self, value: QuickPickOk) {
        self.ok.set(value);
        self.core.update();
    }

    /// `get okLabel()`: `_okLabel ?? localize('ok', "OK")`.
    fn ok_label(&self) -> Option<String> {
        Some(self.ok_label.borrow().clone().unwrap_or_else(|| localize("ok", "OK", &[])))
    }

    fn set_ok_label(&self, value: Option<String>) {
        *self.ok_label.borrow_mut() = value;
        self.core.update();
    }

    fn custom_button(&self) -> bool {
        self.custom_button.get()
    }

    fn set_custom_button(&self, value: bool) {
        self.custom_button.set(value);
        self.core.update();
    }

    fn custom_button_secondary(&self) -> Option<bool> {
        Some(self.custom_button_secondary.get())
    }

    fn set_custom_button_secondary(&self, value: Option<bool>) {
        self.custom_button_secondary.set(value.unwrap_or(false));
        self.core.update();
    }

    fn items(&self) -> Vec<QuickPickInput> {
        self.items.borrow().clone()
    }

    fn set_items(&self, items: Vec<QuickPickInput>) {
        *self.items.borrow_mut() = items;
        self.items_updated.set(true);
        self.core.update();
    }

    fn can_select_many(&self) -> bool {
        self.can_select_many.get()
    }

    fn set_can_select_many(&self, value: bool) {
        self.can_select_many.set(value);
        self.core.update();
    }

    fn match_on_description(&self) -> bool {
        self.match_on_description.get()
    }

    fn set_match_on_description(&self, value: bool) {
        self.match_on_description.set(value);
        self.core.update();
    }

    fn match_on_detail(&self) -> bool {
        self.match_on_detail.get()
    }

    fn set_match_on_detail(&self, value: bool) {
        self.match_on_detail.set(value);
        self.core.update();
    }

    fn match_on_label(&self) -> bool {
        self.match_on_label.get()
    }

    fn set_match_on_label(&self, value: bool) {
        self.match_on_label.set(value);
        self.core.update();
    }

    fn match_on_label_mode(&self) -> MatchOnLabelMode {
        self.match_on_label_mode.get()
    }

    fn set_match_on_label_mode(&self, value: MatchOnLabelMode) {
        self.match_on_label_mode.set(value);
        self.core.update();
    }

    fn sort_by_label(&self) -> bool {
        self.sort_by_label.get()
    }

    fn set_sort_by_label(&self, value: bool) {
        self.sort_by_label.set(value);
        self.core.update();
    }

    fn keep_scroll_position(&self) -> bool {
        self.keep_scroll_position.get()
    }

    fn set_keep_scroll_position(&self, value: bool) {
        self.keep_scroll_position.set(value);
    }

    fn active_items(&self) -> Vec<IQuickPickItem> {
        self.active_items.borrow().clone()
    }

    fn set_active_items(&self, items: Vec<IQuickPickItem>) {
        self.replace_active_items(items);
        self.active_items_updated.set(true);
        self.core.update();
    }

    fn item_activation(&self) -> ItemActivation {
        self.item_activation.get()
    }

    fn set_item_activation(&self, value: ItemActivation) {
        self.item_activation.set(value);
    }

    fn selected_items(&self) -> Vec<IQuickPickItem> {
        self.selected_items.borrow().clone()
    }

    fn set_selected_items(&self, items: Vec<IQuickPickItem>) {
        self.replace_selected_items(items);
        self.selected_items_updated.set(true);
        self.core.update();
    }

    fn value_selection(&self) -> Option<(u32, u32)> {
        self.core.ui().input_box().get_selection()
    }

    fn set_value_selection(&self, value: Option<(u32, u32)>) {
        self.value_selection.set(value);
        self.value_selection_updated.set(true);
        self.core.update();
    }

    fn validation_message(&self) -> Option<String> {
        self.core.validation_message()
    }

    fn set_validation_message(&self, value: Option<String>) {
        self.core.set_validation_message(value);
    }

    fn severity(&self) -> Severity {
        self.core.severity()
    }

    fn set_severity(&self, value: Severity) {
        self.core.set_severity(value);
    }

    fn hide_input(&self) -> bool {
        self.hide_input.get()
    }

    fn set_hide_input(&self, value: bool) {
        self.hide_input.set(value);
        self.core.update();
    }

    fn hide_count_badge(&self) -> bool {
        self.hide_count_badge.get()
    }

    fn set_hide_count_badge(&self, value: bool) {
        self.hide_count_badge.set(value);
        self.core.update();
    }

    fn hide_check_all(&self) -> bool {
        self.hide_check_all.get()
    }

    fn set_hide_check_all(&self, value: bool) {
        self.hide_check_all.set(value);
        self.core.update();
    }

    fn input_has_focus(&self) -> bool {
        self.core.is_visible() && self.core.ui().input_box().has_focus()
    }

    fn focus_on_input(&self) {
        self.core.ui().input_box().set_focus();
    }

    fn focus(&self, focus: QuickPickFocus) {
        let list = self.core.ui().list();
        list.focus(focus);
        // To allow things like space to check/uncheck items
        if self.can_select_many.get() {
            list.dom_focus();
        }
    }

    fn accept(&self, in_background: Option<bool>) {
        if in_background == Some(true) && !self.can_accept_in_background.get() {
            return; // needs to be enabled
        }

        let first = self.active_items.borrow().first().cloned();
        if let (Some(first), false) = (first, self.can_select_many.get()) {
            self.replace_selected_items(vec![first]);
            self.on_did_change_selection.fire(self.selected_items.borrow().clone());
        }
        self.handle_accept(in_background.unwrap_or(false));
    }
}

// ---------------------------------------------------------------------------------------------
// InputBox.
// ---------------------------------------------------------------------------------------------

/// `InputBox` (o `InputBox` do `quickInput.ts`, o campo de texto do quick input).
pub struct InputBox {
    core: Rc<QuickInputBase>,
    this: Weak<InputBox>,
    value: RefCell<String>,
    value_selection: Cell<Option<(u32, u32)>>,
    value_selection_updated: Cell<bool>,
    placeholder: RefCell<Option<String>>,
    aria_label: RefCell<Option<String>>,
    password: Cell<bool>,
    prompt: RefCell<Option<String>>,
    on_did_value_change: Rc<Emitter<String>>,
    on_did_accept: Rc<Emitter<()>>,
}

impl InputBox {
    pub fn new(ui: Rc<dyn IQuickInputUi>) -> Rc<Self> {
        let core = QuickInputBase::new(ui, QuickInputType::InputBox);
        let (on_did_value_change, on_did_accept) = (Emitter::new(), Emitter::new());
        core.register(on_did_value_change.clone());
        core.register(on_did_accept.clone());
        let this = Rc::new_cyclic(|this| Self {
            core: core.clone(),
            this: this.clone(),
            value: RefCell::new(String::new()),
            value_selection: Cell::new(None),
            value_selection_updated: Cell::new(true),
            placeholder: RefCell::new(None),
            aria_label: RefCell::new(None),
            password: Cell::new(false),
            prompt: RefCell::new(None),
            on_did_value_change,
            on_did_accept,
        });
        bind_core(this, &core)
    }

    fn show_impl(&self) {
        if !self.core.is_visible() {
            let ui = self.core.ui().clone();
            let store = self.core.visible_disposables();
            let weak = self.this.clone();
            store.add(ui.input_box().on_did_change().listen(move |value| {
                let Some(this) = weak.upgrade() else { return };
                if *value == *this.value.borrow() {
                    return;
                }
                *this.value.borrow_mut() = value.clone();
                this.on_did_value_change.fire(value.clone());
            }));
            let weak = self.this.clone();
            store.add(ui.on_did_accept().listen(move |()| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_accept.fire(());
                }
            }));
            self.value_selection_updated.set(true);
        }
        self.core.show();
    }
}

impl QuickInputUpdate for InputBox {
    fn update(&self) {
        if !self.core.is_visible() {
            return;
        }
        let ui = self.core.ui().clone();
        let input_box = ui.input_box();

        ui.container().class_list_remove("hidden-input");
        let title = self.core.title().filter(|title| !title.is_empty());
        let step = self.core.step().is_some_and(|step| step != 0);
        let visibilities = Visibilities {
            title: Some(title.is_some() || step || self.core.title_buttons_len() > 0),
            description: Some(self.core.description().is_some_and(|description| !description.is_empty()) || step),
            input_box: Some(true),
            message: Some(true),
            progress_bar: Some(true),
            ..Default::default()
        };

        ui.set_visibilities(visibilities);
        self.core.base_update();
        let value = self.value.borrow().clone();
        if input_box.value() != value {
            input_box.set_value(&value);
        }
        if self.value_selection_updated.get() {
            self.value_selection_updated.set(false);
            input_box.select(self.value_selection.get());
        }
        let placeholder = self.placeholder.borrow().clone().unwrap_or_default();
        if input_box.placeholder() != placeholder {
            input_box.set_placeholder(&placeholder);
        }
        if input_box.password() != self.password.get() {
            input_box.set_password(self.password.get());
        }
        let mut aria_label = self.aria_label.borrow().clone().filter(|label| !label.is_empty());
        // Only set aria label to the input box placeholder if we actually have an input box.
        if aria_label.is_none() {
            let placeholder = self.placeholder.borrow().clone().filter(|placeholder| !placeholder.is_empty());
            aria_label = Some(match (placeholder, title) {
                (Some(placeholder), Some(title)) => format!("{placeholder} - {title}"),
                (Some(placeholder), None) => placeholder,
                (None, Some(title)) => title,
                (None, None) => "input".to_owned(),
            });
        }
        if input_box.aria_label() != aria_label {
            input_box.set_aria_label(aria_label.as_deref().filter(|label| !label.is_empty()).unwrap_or("input"));
        }
    }
}

impl_quick_input!(InputBox);

impl IInputBox for InputBox {
    fn on_did_change_value(&self) -> Event<String> {
        self.on_did_value_change.event()
    }

    fn on_did_accept(&self) -> Event<()> {
        self.on_did_accept.event()
    }

    fn value(&self) -> String {
        self.value.borrow().clone()
    }

    /// `set value(value)`: `value || ''`.
    fn set_value(&self, value: String) {
        *self.value.borrow_mut() = value;
        self.core.update();
    }

    fn value_selection(&self) -> Option<(u32, u32)> {
        self.core.ui().input_box().get_selection()
    }

    fn set_value_selection(&self, value: Option<(u32, u32)>) {
        self.value_selection.set(value);
        self.value_selection_updated.set(true);
        self.core.update();
    }

    props! {
        placeholder, set_placeholder, placeholder: Option<String>, update;
    }

    fn password(&self) -> bool {
        self.password.get()
    }

    fn set_password(&self, value: bool) {
        self.password.set(value);
        self.core.update();
    }

    fn prompt(&self) -> Option<String> {
        self.prompt.borrow().clone()
    }

    fn set_prompt(&self, value: Option<String>) {
        *self.prompt.borrow_mut() = value.clone();
        self.core.set_no_validation_message(Some(match value.filter(|prompt| !prompt.is_empty()) {
            Some(prompt) => localize("inputModeEntryDescription", "{0} (Press 'Enter' to confirm or 'Escape' to cancel)", &[&prompt]),
            None => no_prompt_message(),
        }));
        self.core.update();
    }

    fn validation_message(&self) -> Option<String> {
        self.core.validation_message()
    }

    fn set_validation_message(&self, value: Option<String>) {
        self.core.set_validation_message(value);
    }

    fn severity(&self) -> Severity {
        self.core.severity()
    }

    fn set_severity(&self, value: Severity) {
        self.core.set_severity(value);
    }

    fn accept(&self) {
        self.on_did_accept.fire(());
    }
}

impl InputBox {
    /// `get ariaLabel()`.
    pub fn aria_label(&self) -> Option<String> {
        self.aria_label.borrow().clone()
    }

    /// `set ariaLabel(ariaLabel)`.
    pub fn set_aria_label(&self, aria_label: Option<String>) {
        *self.aria_label.borrow_mut() = aria_label;
        self.core.update();
    }
}

// ---------------------------------------------------------------------------------------------
// QuickWidget.
// ---------------------------------------------------------------------------------------------

/// `QuickWidget`: um quick input cujo corpo é um elemento do chamador.
pub struct QuickWidget {
    core: Rc<QuickInputBase>,
    widget: RefCell<Option<Rc<ViewElement>>>,
    widget_updated: Cell<bool>,
}

impl QuickWidget {
    pub fn new(ui: Rc<dyn IQuickInputUi>) -> Rc<Self> {
        let core = QuickInputBase::new(ui, QuickInputType::QuickWidget);
        let this = Rc::new(Self { core: core.clone(), widget: RefCell::new(None), widget_updated: Cell::new(false) });
        bind_core(this, &core)
    }

    fn show_impl(&self) {
        self.core.show();
    }

    /// `get widget()`.
    pub fn widget(&self) -> Option<Rc<ViewElement>> {
        self.widget.borrow().clone()
    }

    /// `set widget(widget)`.
    pub fn set_widget(&self, widget: Option<Rc<ViewElement>>) {
        let same = match (&*self.widget.borrow(), &widget) {
            (Some(current), Some(new)) => Rc::ptr_eq(current, new),
            (None, None) => true,
            _ => false,
        };
        if !same {
            *self.widget.borrow_mut() = widget;
            self.widget_updated.set(true);
            self.core.update();
        }
    }
}

impl QuickInputUpdate for QuickWidget {
    fn update(&self) {
        if !self.core.is_visible() {
            return;
        }
        let ui = self.core.ui();
        let step = self.core.step().is_some_and(|step| step != 0);
        ui.set_visibilities(Visibilities {
            title: Some(self.core.title().is_some_and(|title| !title.is_empty()) || step || self.core.title_buttons_len() > 0),
            description: Some(self.core.description().is_some_and(|description| !description.is_empty()) || step),
            ..Default::default()
        });
        if self.widget_updated.get() {
            self.widget_updated.set(false);
            ui.reset_widget(self.widget.borrow().clone());
        }
        self.core.base_update();
    }
}

impl_quick_input!(QuickWidget);
