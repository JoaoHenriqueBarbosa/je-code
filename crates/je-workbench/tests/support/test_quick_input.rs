//! Porte lógico de `QuickPick` (`src/vs/platform/quickinput/browser/quickInput.ts`) como bancada de teste: um `IQuickPick`
//! concreto com estado e eventos, sem pintura, e o `IQuickInputService` que o cria (`createQuickPick`).
//!
//! Notas de tradução:
//! - **Só o contrato.** O `QuickPick` do original liga o estado ao `QuickInputList` (filtro, ordenação, foco, checkboxes e
//!   rolagem), que é pintura. A bancada guarda as propriedades e dispara os mesmos eventos: `value` dispara
//!   `onDidChangeValue` quando muda, `items` com `itemActivation` padrão (`First`) deixa o primeiro item ativo e dispara
//!   `onDidChangeActive`, `activeItems` e `selectedItems` disparam os eventos de mudança, `accept` dispara `onWillAccept` (com
//!   `veto`) e `onDidAccept` com `inBackground`, `hide` dispara `onWillHide` e `onDidHide` com o motivo (`Other` sem motivo
//!   explícito do original, que é `QuickInputHideReason.Other`) e `dispose` dispara `onDispose` uma vez.
//! - **`keyMods`.** Começa em `NO_KEY_MODS` e muda por [`TestQuickPick::set_key_mods`].
//! - **Serviço.** [`TestQuickInputService`] só implementa `createQuickPick`; os demais métodos falham, como um objeto parcial
//!   (`upcastPartial`) sem o método.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::observable::base::IObservable;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_platform::quickinput::quick_access::IQuickAccessController;
use je_platform::quickinput::quick_input::{
    IInputBox, IInputOptions, IKeyMods, IPickOptions, IQuickInput, IQuickInputHideEvent, IQuickInputService, IQuickNavigateConfiguration, IQuickPick,
    IQuickPickDidAcceptEvent, IQuickPickItem, IQuickPickItemButtonEvent, IQuickPickSeparatorButtonEvent, IQuickPickWillAcceptEvent, IQuickTree, ItemActivation,
    MatchOnLabelMode, QuickInputAlignment, QuickInputAlignmentSetting, QuickInputAnchor, QuickInputAnchorPosition, QuickInputButtonRef, QuickInputHideReason,
    QuickInputRef, QuickInputType, QuickPickCreateOptions, QuickPickFocus, QuickPickInput, QuickPickOk, QuickPicks, NO_KEY_MODS,
};

/// Declara o par `nome()` e `set_nome(valor)` sobre um campo `RefCell`.
macro_rules! cell_properties {
    ($($get:ident, $set:ident, $field:ident: $ty:ty;)*) => {
        $(
            fn $get(&self) -> $ty {
                self.$field.borrow().clone()
            }

            fn $set(&self, value: $ty) {
                *self.$field.borrow_mut() = value;
            }
        )*
    };
}

/// `QuickPick`.
pub struct TestQuickPick {
    this: std::rc::Weak<TestQuickPick>,
    disposed: Cell<bool>,
    key_mods: Cell<IKeyMods>,
    /// `QuickInput.visible`.
    visible: Cell<bool>,
    /// O serviço que mostra este seletor (o controlador do `QuickInputController`), quando ele o criou.
    service: RefCell<std::rc::Weak<TestQuickInputService>>,

    on_did_hide: Rc<Emitter<IQuickInputHideEvent>>,
    on_will_hide: Rc<Emitter<IQuickInputHideEvent>>,
    on_dispose: Rc<Emitter<()>>,
    on_did_trigger_button: Rc<Emitter<QuickInputButtonRef>>,
    on_did_change_value: Rc<Emitter<String>>,
    on_will_accept: Rc<Emitter<IQuickPickWillAcceptEvent>>,
    on_did_accept: Rc<Emitter<IQuickPickDidAcceptEvent>>,
    on_did_custom: Rc<Emitter<()>>,
    on_did_trigger_item_button: Rc<Emitter<IQuickPickItemButtonEvent>>,
    on_did_trigger_separator_button: Rc<Emitter<IQuickPickSeparatorButtonEvent>>,
    on_did_change_active: Rc<Emitter<Vec<IQuickPickItem>>>,
    on_did_change_selection: Rc<Emitter<Vec<IQuickPickItem>>>,

    title: RefCell<Option<String>>,
    description: RefCell<Option<String>>,
    step: RefCell<Option<u32>>,
    total_steps: RefCell<Option<u32>>,
    buttons: RefCell<Vec<QuickInputButtonRef>>,
    enabled: RefCell<bool>,
    context_key: RefCell<Option<String>>,
    busy: RefCell<bool>,
    ignore_focus_out: RefCell<bool>,
    anchor: RefCell<Option<QuickInputAnchor>>,
    anchor_position: RefCell<Option<QuickInputAnchorPosition>>,

    value: RefCell<String>,
    filter_value: RefCell<Rc<dyn Fn(&str) -> String>>,
    aria_label: RefCell<Option<String>>,
    placeholder: RefCell<Option<String>>,
    prompt: RefCell<Option<String>>,
    can_accept_in_background: RefCell<bool>,
    ok: RefCell<QuickPickOk>,
    ok_label: RefCell<Option<String>>,
    custom_button: RefCell<bool>,
    custom_label: RefCell<Option<String>>,
    custom_hover: RefCell<Option<String>>,
    custom_button_secondary: RefCell<Option<bool>>,
    items: RefCell<Vec<QuickPickInput>>,
    can_select_many: RefCell<bool>,
    match_on_description: RefCell<bool>,
    match_on_detail: RefCell<bool>,
    match_on_label: RefCell<bool>,
    match_on_label_mode: RefCell<MatchOnLabelMode>,
    sort_by_label: RefCell<bool>,
    keep_scroll_position: RefCell<bool>,
    quick_navigate: RefCell<Option<IQuickNavigateConfiguration>>,
    active_items: RefCell<Vec<IQuickPickItem>>,
    item_activation: RefCell<ItemActivation>,
    selected_items: RefCell<Vec<IQuickPickItem>>,
    value_selection: RefCell<Option<(u32, u32)>>,
    validation_message: RefCell<Option<String>>,
    severity: RefCell<Severity>,
    hide_input: RefCell<bool>,
    hide_count_badge: RefCell<bool>,
    hide_check_all: RefCell<bool>,
}

impl TestQuickPick {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            disposed: Cell::new(false),
            key_mods: Cell::new(NO_KEY_MODS),
            visible: Cell::new(false),
            service: RefCell::new(std::rc::Weak::new()),
            on_did_hide: Emitter::new(),
            on_will_hide: Emitter::new(),
            on_dispose: Emitter::new(),
            on_did_trigger_button: Emitter::new(),
            on_did_change_value: Emitter::new(),
            on_will_accept: Emitter::new(),
            on_did_accept: Emitter::new(),
            on_did_custom: Emitter::new(),
            on_did_trigger_item_button: Emitter::new(),
            on_did_trigger_separator_button: Emitter::new(),
            on_did_change_active: Emitter::new(),
            on_did_change_selection: Emitter::new(),
            title: RefCell::new(None),
            description: RefCell::new(None),
            step: RefCell::new(None),
            total_steps: RefCell::new(None),
            buttons: RefCell::new(Vec::new()),
            enabled: RefCell::new(true),
            context_key: RefCell::new(None),
            busy: RefCell::new(false),
            ignore_focus_out: RefCell::new(false),
            anchor: RefCell::new(None),
            anchor_position: RefCell::new(None),
            value: RefCell::new(String::new()),
            filter_value: RefCell::new(Rc::new(|value: &str| value.to_string())),
            aria_label: RefCell::new(None),
            placeholder: RefCell::new(None),
            prompt: RefCell::new(None),
            can_accept_in_background: RefCell::new(false),
            ok: RefCell::new(QuickPickOk::Default),
            ok_label: RefCell::new(None),
            custom_button: RefCell::new(false),
            custom_label: RefCell::new(None),
            custom_hover: RefCell::new(None),
            custom_button_secondary: RefCell::new(None),
            items: RefCell::new(Vec::new()),
            can_select_many: RefCell::new(false),
            match_on_description: RefCell::new(false),
            match_on_detail: RefCell::new(false),
            match_on_label: RefCell::new(true),
            match_on_label_mode: RefCell::new(MatchOnLabelMode::Fuzzy),
            sort_by_label: RefCell::new(true),
            keep_scroll_position: RefCell::new(false),
            quick_navigate: RefCell::new(None),
            active_items: RefCell::new(Vec::new()),
            item_activation: RefCell::new(ItemActivation::First),
            selected_items: RefCell::new(Vec::new()),
            value_selection: RefCell::new(None),
            validation_message: RefCell::new(None),
            severity: RefCell::new(Severity::Ignore),
            hide_input: RefCell::new(false),
            hide_count_badge: RefCell::new(false),
            hide_check_all: RefCell::new(false),
        })
    }

    /// O estado das teclas modificadoras que o aceite lê.
    pub fn set_key_mods(&self, key_mods: IKeyMods) {
        self.key_mods.set(key_mods);
    }

    fn self_rc(&self) -> Rc<TestQuickPick> {
        self.this.upgrade().expect("o seletor está vivo enquanto é usado")
    }
}

impl IDisposable for TestQuickPick {
    /// `QuickInput.dispose()`: esconde (se visível), dispara `onDispose` e descarta.
    fn dispose(&self) {
        if self.disposed.get() {
            return;
        }
        self.hide();
        self.disposed.set(true);
        self.on_dispose.fire(());
    }
}

impl IQuickInput for TestQuickPick {
    fn quick_input_type(&self) -> QuickInputType {
        QuickInputType::QuickPick
    }

    fn on_did_hide(&self) -> Event<IQuickInputHideEvent> {
        self.on_did_hide.event()
    }

    fn on_will_hide(&self) -> Event<IQuickInputHideEvent> {
        self.on_will_hide.event()
    }

    fn on_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }

    fn on_did_trigger_button(&self) -> Event<QuickInputButtonRef> {
        self.on_did_trigger_button.event()
    }

    cell_properties! {
        title, set_title, title: Option<String>;
        description, set_description, description: Option<String>;
        step, set_step, step: Option<u32>;
        total_steps, set_total_steps, total_steps: Option<u32>;
        buttons, set_buttons, buttons: Vec<QuickInputButtonRef>;
        enabled, set_enabled, enabled: bool;
        context_key, set_context_key, context_key: Option<String>;
        busy, set_busy, busy: bool;
        ignore_focus_out, set_ignore_focus_out, ignore_focus_out: bool;
        anchor, set_anchor, anchor: Option<QuickInputAnchor>;
        anchor_position, set_anchor_position, anchor_position: Option<QuickInputAnchorPosition>;
    }

    /// `QuickInput.show()` e o `QuickInputController.show(controller)`: o seletor passa a ser o atual e o anterior, se havia, é
    /// escondido por `didHide` (para os ouvintes dele não ficarem velhos).
    fn show(&self) {
        if self.visible.get() {
            return;
        }
        self.visible.set(true);

        let service = self.service.borrow().upgrade();
        if let Some(service) = service {
            service.show(self.self_rc());
        }
    }

    /// `QuickInput.hide()`: sem efeito se o seletor não está visível.
    fn hide(&self) {
        if !self.visible.get() {
            return;
        }
        self.will_hide(Some(QuickInputHideReason::Other));
        self.did_hide(Some(QuickInputHideReason::Other));
    }

    fn did_hide(&self, reason: Option<QuickInputHideReason>) {
        self.visible.set(false);
        let service = self.service.borrow().upgrade();
        if let Some(service) = service {
            service.release(self);
        }
        self.on_did_hide.fire(IQuickInputHideEvent { reason: reason.unwrap_or(QuickInputHideReason::Other) });
    }

    fn will_hide(&self, reason: Option<QuickInputHideReason>) {
        self.on_will_hide.fire(IQuickInputHideEvent { reason: reason.unwrap_or(QuickInputHideReason::Other) });
    }
}

impl IQuickPick for TestQuickPick {
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
        self.key_mods.get()
    }

    cell_properties! {
        filter_value, set_filter_value, filter_value: Rc<dyn Fn(&str) -> String>;
        aria_label, set_aria_label, aria_label: Option<String>;
        placeholder, set_placeholder, placeholder: Option<String>;
        prompt, set_prompt, prompt: Option<String>;
        can_accept_in_background, set_can_accept_in_background, can_accept_in_background: bool;
        ok, set_ok, ok: QuickPickOk;
        ok_label, set_ok_label, ok_label: Option<String>;
        custom_button, set_custom_button, custom_button: bool;
        custom_label, set_custom_label, custom_label: Option<String>;
        custom_hover, set_custom_hover, custom_hover: Option<String>;
        custom_button_secondary, set_custom_button_secondary, custom_button_secondary: Option<bool>;
        can_select_many, set_can_select_many, can_select_many: bool;
        match_on_description, set_match_on_description, match_on_description: bool;
        match_on_detail, set_match_on_detail, match_on_detail: bool;
        match_on_label, set_match_on_label, match_on_label: bool;
        match_on_label_mode, set_match_on_label_mode, match_on_label_mode: MatchOnLabelMode;
        sort_by_label, set_sort_by_label, sort_by_label: bool;
        keep_scroll_position, set_keep_scroll_position, keep_scroll_position: bool;
        quick_navigate, set_quick_navigate, quick_navigate: Option<IQuickNavigateConfiguration>;
        item_activation, set_item_activation, item_activation: ItemActivation;
        value_selection, set_value_selection, value_selection: Option<(u32, u32)>;
        validation_message, set_validation_message, validation_message: Option<String>;
        severity, set_severity, severity: Severity;
        hide_input, set_hide_input, hide_input: bool;
        hide_count_badge, set_hide_count_badge, hide_count_badge: bool;
        hide_check_all, set_hide_check_all, hide_check_all: bool;
    }

    fn value(&self) -> String {
        self.value.borrow().clone()
    }

    fn set_value(&self, value: String) {
        self.assert_not_disposed();
        if *self.value.borrow() == value {
            return;
        }
        *self.value.borrow_mut() = value.clone();
        self.on_did_change_value.fire(value);
    }

    fn items(&self) -> Vec<QuickPickInput> {
        self.items.borrow().clone()
    }

    fn set_items(&self, items: Vec<QuickPickInput>) {
        self.assert_not_disposed();
        *self.items.borrow_mut() = items.clone();
        // O `QuickInputList` recalcula o foco: com `itemActivation` `First` o primeiro item (não separador) fica ativo.
        let item_activation = *self.item_activation.borrow();
        let mut item_list = items.into_iter().filter_map(QuickPickInput::into_item);
        let active: Vec<IQuickPickItem> = match item_activation {
            ItemActivation::None => Vec::new(),
            ItemActivation::First => item_list.next().into_iter().collect(),
            ItemActivation::Second => item_list.nth(1).into_iter().collect(),
            ItemActivation::Last => item_list.last().into_iter().collect(),
        };
        *self.active_items.borrow_mut() = active.clone();
        self.on_did_change_active.fire(active);
    }

    fn active_items(&self) -> Vec<IQuickPickItem> {
        self.active_items.borrow().clone()
    }

    fn set_active_items(&self, active_items: Vec<IQuickPickItem>) {
        self.assert_not_disposed();
        *self.active_items.borrow_mut() = active_items.clone();
        self.on_did_change_active.fire(active_items);
    }

    fn selected_items(&self) -> Vec<IQuickPickItem> {
        self.selected_items.borrow().clone()
    }

    fn set_selected_items(&self, selected_items: Vec<IQuickPickItem>) {
        self.assert_not_disposed();
        *self.selected_items.borrow_mut() = selected_items.clone();
        self.on_did_change_selection.fire(selected_items);
    }

    fn input_has_focus(&self) -> bool {
        false
    }

    fn focus_on_input(&self) {}

    fn focus(&self, _focus: QuickPickFocus) {}

    /// `accept(inBackground)`: sem `canAcceptInBackground` o aceite em segundo plano não acontece; o item ativo vira o
    /// selecionado (sem `canSelectMany`) e `onWillAccept` e `onDidAccept` disparam, este a menos que um ouvinte de
    /// `onWillAccept` use o `veto`.
    fn accept(&self, in_background: Option<bool>) {
        if in_background == Some(true) && !*self.can_accept_in_background.borrow() {
            return; // needs to be enabled
        }
        if !*self.can_select_many.borrow() {
            let active = self.active_items.borrow().clone();
            if !active.is_empty() {
                self.set_selected_items(active.into_iter().take(1).collect());
            }
        }
        let event = IQuickPickWillAcceptEvent::new();
        self.on_will_accept.fire(event.clone());
        if event.is_vetoed() {
            return;
        }
        self.on_did_accept.fire(IQuickPickDidAcceptEvent { in_background: in_background.unwrap_or(false) });
    }
}

impl TestQuickPick {
    fn assert_not_disposed(&self) {
        assert!(!self.disposed.get(), "o seletor foi descartado");
    }
}

/// O `IQuickInputService` com só o que os testes de acesso rápido usam: `createQuickPick`, o seletor atual (`show`), o
/// aceite programático (`accept`) e o `quickAccess`.
pub struct TestQuickInputService {
    this: std::rc::Weak<TestQuickInputService>,
    pub created: RefCell<Vec<Rc<TestQuickPick>>>,
    /// `controller`: o seletor visível, ou nenhum.
    current: RefCell<Option<Rc<TestQuickPick>>>,
    /// `quickAccess`, instalado por [`TestQuickInputService::set_quick_access`].
    quick_access: RefCell<Option<Rc<dyn IQuickAccessController>>>,
}

impl TestQuickInputService {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            created: RefCell::new(Vec::new()),
            current: RefCell::new(None),
            quick_access: RefCell::new(None),
        })
    }

    /// O `QuickAccessController` que o `quickAccess` devolve (o original o cria na primeira leitura).
    pub fn set_quick_access(&self, quick_access: Option<Rc<dyn IQuickAccessController>>) {
        *self.quick_access.borrow_mut() = quick_access;
    }

    /// `QuickInputController.show(controller)`: o seletor passa a ser o atual e o anterior é escondido por `didHide`.
    fn show(&self, controller: Rc<TestQuickPick>) {
        let old_controller = self.current.borrow_mut().replace(controller);
        if let Some(old_controller) = old_controller {
            old_controller.did_hide(None);
        }
    }

    /// O `this.controller = null` do `hide`: o seletor deixa de ser o atual, se era.
    fn release(&self, picker: &TestQuickPick) {
        let mut current = self.current.borrow_mut();
        if current.as_ref().is_some_and(|current| std::ptr::eq(Rc::as_ptr(current), picker)) {
            *current = None;
        }
    }
}

impl IQuickInputService for TestQuickInputService {
    fn back_button(&self) -> QuickInputButtonRef {
        panic!("backButton não é usado por este teste")
    }

    fn on_show(&self) -> Event<()> {
        panic!("onShow não é usado por este teste")
    }

    fn on_hide(&self) -> Event<()> {
        panic!("onHide não é usado por este teste")
    }

    fn alignment(&self) -> IObservable<QuickInputAlignment> {
        panic!("alignment não é usado por este teste")
    }

    fn with_quick_input_anchor(
        &self,
        _anchor: Option<QuickInputAnchor>,
        _anchor_position: Option<QuickInputAnchorPosition>,
        _operation: Box<dyn FnOnce() -> Promise<Dynamic>>,
    ) -> Promise<Dynamic> {
        panic!("withQuickInputAnchor não é usado por este teste")
    }

    fn pick(&self, _picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>> {
        panic!("pick não é usado por este teste")
    }

    fn pick_many(&self, _picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>> {
        panic!("pick não é usado por este teste")
    }

    fn input(&self, _options: Option<&IInputOptions>, _token: Option<&CancellationToken>) -> Promise<Option<String>> {
        panic!("input não é usado por este teste")
    }

    fn create_quick_pick(&self, _options: Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick> {
        let quick_pick = TestQuickPick::new();
        *quick_pick.service.borrow_mut() = self.this.clone();
        self.created.borrow_mut().push(quick_pick.clone());
        quick_pick
    }

    fn create_input_box(&self) -> Rc<dyn IInputBox> {
        panic!("createInputBox não é usado por este teste")
    }

    fn create_quick_tree(&self) -> Rc<dyn IQuickTree> {
        panic!("createQuickTree não é usado por este teste")
    }

    fn focus(&self) {}

    fn toggle(&self) {}

    fn navigate(&self, _next: bool, _quick_navigate: Option<&IQuickNavigateConfiguration>) {}

    fn back(&self) -> Promise<()> {
        Promise::resolved(())
    }

    /// `QuickInputController.accept(keyMods)`: as teclas modificadoras passam a ser as dadas (ou nenhuma) e o seletor atual
    /// aceita (o `onDidAccept` da interface, que seleciona o item ativo e dispara `onWillAccept` e `onDidAccept`).
    fn accept(&self, key_mods: Option<IKeyMods>) -> Promise<()> {
        let current = self.current.borrow().clone();
        if let Some(current) = current {
            current.set_key_mods(key_mods.unwrap_or(NO_KEY_MODS));
            current.accept(None);
        }
        Promise::resolved(())
    }

    fn cancel(&self, _reason: Option<QuickInputHideReason>) -> Promise<()> {
        Promise::resolved(())
    }

    fn toggle_hover(&self) {}

    fn current_quick_input(&self) -> Option<QuickInputRef> {
        self.current.borrow().as_ref().map(|quick_pick| QuickInputRef::QuickPick(quick_pick.clone()))
    }

    fn set_alignment(&self, _alignment: QuickInputAlignmentSetting) {}

    fn quick_access(&self) -> Rc<dyn IQuickAccessController> {
        self.quick_access.borrow().clone().expect("o QuickAccessController foi instalado")
    }
}
