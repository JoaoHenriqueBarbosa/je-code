//! Infraestrutura de teste de `src/vs/workbench/services/editor/test/browser/openWithEditorPicker.test.ts`: o `TestQuickPick`
//! (a classe que o teste deriva de `mock<IQuickPick<...>>()`) e o `upcastPartial<IQuickInputService>({ createQuickPick })`.
//!
//! Divergências:
//! - **`mock<T>()`.** Cada dublê implementa só o que o caso usa, e todo outro membro lança `X is not a function`, como
//!   chamar um membro ausente no JavaScript (a mesma convenção de `test_service_mocks.rs`). As propriedades que o original
//!   sobrescreve (`placeholder`, `canAcceptInBackground`, `items` e `selectedItems`) são pares de leitura e escrita sobre
//!   `Cell` e `RefCell`; `onDidTriggerItemButton` é o `Event.None`.
//! - **`quickPick.items.filter((item): item is IQuickPickItem => item.type !== 'separator' && item.id === id)`.** É o
//!   [`QuickPickInput::as_item`] com o filtro do `id`.
//! - **`new TestQuickPick(onShow)`.** O `onShow` recebe o próprio seletor (`Rc`), que o dublê guarda por `Weak` para o
//!   `show()` poder entregá-lo.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::observable::base::IObservable;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_platform::quickinput::quick_input::{
    IInputBox, IInputOptions, IKeyMods, IPickOptions, IQuickInput, IQuickInputHideEvent, IQuickInputService, IQuickNavigateConfiguration, IQuickPick,
    IQuickPickDidAcceptEvent, IQuickPickItem, IQuickPickItemButtonEvent, IQuickPickSeparatorButtonEvent, IQuickPickWillAcceptEvent, IQuickTree, ItemActivation,
    MatchOnLabelMode, QuickInputAlignment, QuickInputAlignmentSetting, QuickInputAnchor, QuickInputAnchorPosition, QuickInputButtonRef, QuickInputHideReason,
    QuickInputRef, QuickInputType, QuickPickCreateOptions, QuickPickFocus, QuickPickInput, QuickPickOk, QuickPicks,
};

/// `X is not a function`.
fn not_a_function(name: &str) -> ! {
    throw(Error::new(format!("{name} is not a function")))
}

/// Gera os pares `nome()` e `set_nome(valor)` que o dublê não implementa.
macro_rules! unavailable_properties {
    ($($get:ident, $set:ident: $ty:ty;)*) => {
        $(
            fn $get(&self) -> $ty {
                not_a_function(stringify!($get))
            }

            fn $set(&self, _value: $ty) {
                not_a_function(stringify!($set))
            }
        )*
    };
}

/// Gera os eventos que o dublê não implementa.
macro_rules! unavailable_events {
    ($(fn $name:ident() -> $ty:ty;)*) => {
        $(
            fn $name(&self) -> $ty {
                not_a_function(stringify!($name))
            }
        )*
    };
}

/// `TestQuickPick`: a quick pick que se entrega ao teste quando é mostrada, para o teste aceitá-la ou escondê-la.
pub struct TestQuickPick {
    this: Weak<TestQuickPick>,
    on_did_accept: Rc<Emitter<IQuickPickDidAcceptEvent>>,
    on_did_hide: Rc<Emitter<IQuickInputHideEvent>>,
    placeholder: RefCell<Option<String>>,
    can_accept_in_background: Cell<bool>,
    items: RefCell<Vec<QuickPickInput>>,
    selected_items: RefCell<Vec<IQuickPickItem>>,
    disposed: Cell<bool>,
    on_show: Box<dyn Fn(&Rc<TestQuickPick>)>,
}

impl TestQuickPick {
    pub fn new(on_show: impl Fn(&Rc<TestQuickPick>) + 'static) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            on_did_accept: Emitter::new(),
            on_did_hide: Emitter::new(),
            placeholder: RefCell::new(None),
            can_accept_in_background: Cell::new(false),
            items: RefCell::new(Vec::new()),
            selected_items: RefCell::new(Vec::new()),
            disposed: Cell::new(false),
            on_show: Box::new(on_show),
        })
    }

    /// `acceptItem(id, inBackground = false)`.
    pub fn accept_item(&self, id: &str, in_background: bool) {
        let selected: Vec<IQuickPickItem> =
            self.items.borrow().iter().filter_map(QuickPickInput::as_item).filter(|item| item.base.id.as_deref() == Some(id)).cloned().collect();
        *self.selected_items.borrow_mut() = selected;
        self.on_did_accept.fire(IQuickPickDidAcceptEvent { in_background });
    }
}

impl IDisposable for TestQuickPick {
    fn dispose(&self) {
        if !self.disposed.get() {
            self.disposed.set(true);
            IQuickInput::hide(self);
            self.on_did_accept.dispose();
            self.on_did_hide.dispose();
        }
    }
}

impl IQuickInput for TestQuickPick {
    fn on_did_hide(&self) -> Event<IQuickInputHideEvent> {
        self.on_did_hide.event()
    }

    /// `override show()`.
    fn show(&self) {
        let this = self.this.upgrade().expect("o seletor está vivo enquanto é mostrado");
        (self.on_show)(&this);
    }

    /// `override hide()`.
    fn hide(&self) {
        self.on_did_hide.fire(IQuickInputHideEvent { reason: QuickInputHideReason::Gesture });
    }

    unavailable_events! {
        fn quick_input_type() -> QuickInputType;
        fn on_will_hide() -> Event<IQuickInputHideEvent>;
        fn on_dispose() -> Event<()>;
        fn on_did_trigger_button() -> Event<QuickInputButtonRef>;
    }

    unavailable_properties! {
        title, set_title: Option<String>;
        description, set_description: Option<String>;
        step, set_step: Option<u32>;
        total_steps, set_total_steps: Option<u32>;
        buttons, set_buttons: Vec<QuickInputButtonRef>;
        enabled, set_enabled: bool;
        context_key, set_context_key: Option<String>;
        busy, set_busy: bool;
        ignore_focus_out, set_ignore_focus_out: bool;
        anchor, set_anchor: Option<QuickInputAnchor>;
        anchor_position, set_anchor_position: Option<QuickInputAnchorPosition>;
    }

    fn did_hide(&self, _reason: Option<QuickInputHideReason>) {
        not_a_function("didHide")
    }

    fn will_hide(&self, _reason: Option<QuickInputHideReason>) {
        not_a_function("willHide")
    }
}

impl IQuickPick for TestQuickPick {
    /// `override readonly onDidAccept`.
    fn on_did_accept(&self) -> Event<IQuickPickDidAcceptEvent> {
        self.on_did_accept.event()
    }

    /// `override readonly onDidTriggerItemButton: Event<...> = Event.None`.
    fn on_did_trigger_item_button(&self) -> Event<IQuickPickItemButtonEvent> {
        Event::none()
    }

    /// `override placeholder`.
    fn placeholder(&self) -> Option<String> {
        self.placeholder.borrow().clone()
    }

    fn set_placeholder(&self, value: Option<String>) {
        *self.placeholder.borrow_mut() = value;
    }

    /// `override canAcceptInBackground = false`.
    fn can_accept_in_background(&self) -> bool {
        self.can_accept_in_background.get()
    }

    fn set_can_accept_in_background(&self, value: bool) {
        self.can_accept_in_background.set(value);
    }

    /// `override items = []`.
    fn items(&self) -> Vec<QuickPickInput> {
        self.items.borrow().clone()
    }

    fn set_items(&self, value: Vec<QuickPickInput>) {
        *self.items.borrow_mut() = value;
    }

    /// `override selectedItems = []`.
    fn selected_items(&self) -> Vec<IQuickPickItem> {
        self.selected_items.borrow().clone()
    }

    fn set_selected_items(&self, value: Vec<IQuickPickItem>) {
        *self.selected_items.borrow_mut() = value;
    }

    unavailable_events! {
        fn on_did_change_value() -> Event<String>;
        fn on_will_accept() -> Event<IQuickPickWillAcceptEvent>;
        fn on_did_custom() -> Event<()>;
        fn on_did_trigger_separator_button() -> Event<IQuickPickSeparatorButtonEvent>;
        fn on_did_change_active() -> Event<Vec<IQuickPickItem>>;
        fn on_did_change_selection() -> Event<Vec<IQuickPickItem>>;
        fn key_mods() -> IKeyMods;
    }

    unavailable_properties! {
        value, set_value: String;
        filter_value, set_filter_value: Rc<dyn Fn(&str) -> String>;
        aria_label, set_aria_label: Option<String>;
        prompt, set_prompt: Option<String>;
        ok, set_ok: QuickPickOk;
        ok_label, set_ok_label: Option<String>;
        custom_button, set_custom_button: bool;
        custom_label, set_custom_label: Option<String>;
        custom_hover, set_custom_hover: Option<String>;
        custom_button_secondary, set_custom_button_secondary: Option<bool>;
        can_select_many, set_can_select_many: bool;
        match_on_description, set_match_on_description: bool;
        match_on_detail, set_match_on_detail: bool;
        match_on_label, set_match_on_label: bool;
        match_on_label_mode, set_match_on_label_mode: MatchOnLabelMode;
        sort_by_label, set_sort_by_label: bool;
        keep_scroll_position, set_keep_scroll_position: bool;
        quick_navigate, set_quick_navigate: Option<IQuickNavigateConfiguration>;
        active_items, set_active_items: Vec<IQuickPickItem>;
        item_activation, set_item_activation: ItemActivation;
        value_selection, set_value_selection: Option<(u32, u32)>;
        validation_message, set_validation_message: Option<String>;
        severity, set_severity: Severity;
        hide_input, set_hide_input: bool;
        hide_count_badge, set_hide_count_badge: bool;
        hide_check_all, set_hide_check_all: bool;
    }

    fn input_has_focus(&self) -> bool {
        not_a_function("inputHasFocus")
    }

    fn focus_on_input(&self) {
        not_a_function("focusOnInput")
    }

    fn focus(&self, _focus: QuickPickFocus) {
        not_a_function("focus")
    }

    fn accept(&self, _in_background: Option<bool>) {
        not_a_function("accept")
    }
}

type CreateQuickPick = Box<dyn Fn(Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick>>;

/// `upcastPartial<IQuickInputService>({ createQuickPick })`.
pub struct PartialQuickInputService {
    create_quick_pick: CreateQuickPick,
}

impl PartialQuickInputService {
    pub fn new(create_quick_pick: impl Fn(Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick> + 'static) -> Rc<Self> {
        Rc::new(Self { create_quick_pick: Box::new(create_quick_pick) })
    }
}

impl IQuickInputService for PartialQuickInputService {
    fn create_quick_pick(&self, options: Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick> {
        (self.create_quick_pick)(options)
    }

    fn back_button(&self) -> QuickInputButtonRef {
        not_a_function("backButton")
    }

    fn on_show(&self) -> Event<()> {
        not_a_function("onShow")
    }

    fn on_hide(&self) -> Event<()> {
        not_a_function("onHide")
    }

    fn alignment(&self) -> IObservable<QuickInputAlignment> {
        not_a_function("alignment")
    }

    fn with_quick_input_anchor(
        &self,
        _anchor: Option<QuickInputAnchor>,
        _anchor_position: Option<QuickInputAnchorPosition>,
        _operation: Box<dyn FnOnce() -> Promise<Dynamic>>,
    ) -> Promise<Dynamic> {
        not_a_function("withQuickInputAnchor")
    }

    fn pick(&self, _picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>> {
        not_a_function("pick")
    }

    fn pick_many(&self, _picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>> {
        not_a_function("pick")
    }

    fn input(&self, _options: Option<&IInputOptions>, _token: Option<&CancellationToken>) -> Promise<Option<String>> {
        not_a_function("input")
    }

    fn create_input_box(&self) -> Rc<dyn IInputBox> {
        not_a_function("createInputBox")
    }

    fn create_quick_tree(&self) -> Rc<dyn IQuickTree> {
        not_a_function("createQuickTree")
    }

    fn focus(&self) {
        not_a_function("focus")
    }

    fn toggle(&self) {
        not_a_function("toggle")
    }

    fn navigate(&self, _next: bool, _quick_navigate: Option<&IQuickNavigateConfiguration>) {
        not_a_function("navigate")
    }

    fn back(&self) -> Promise<()> {
        not_a_function("back")
    }

    fn accept(&self, _key_mods: Option<IKeyMods>) -> Promise<()> {
        not_a_function("accept")
    }

    fn cancel(&self, _reason: Option<QuickInputHideReason>) -> Promise<()> {
        not_a_function("cancel")
    }

    fn toggle_hover(&self) {
        not_a_function("toggleHover")
    }

    fn current_quick_input(&self) -> Option<QuickInputRef> {
        not_a_function("currentQuickInput")
    }

    fn set_alignment(&self, _alignment: QuickInputAlignmentSetting) {
        not_a_function("setAlignment")
    }
}
