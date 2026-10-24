//! Porte de `src/vs/workbench/api/common/extHostQuickOpen.ts`: o lado da extensão de `window.showQuickPick`, `showInputBox`,
//! `showWorkspaceFolderPick`, `createQuickPick` e `createInputBox`, sobre o proxy `MainThreadQuickOpen`.
//!
//! Divergências:
//! - O original é uma função de fábrica (`createExtHostQuickOpen`) que fecha sobre o `proxy`; aqui é o [`ExtHostQuickOpen`], e as
//!   classes internas `ExtHostQuickInput`, `ExtHostQuickPick` e `ExtHostInputBox` são [`ExtHostQuickInput`] (o núcleo) mais
//!   [`ExtHostQuickPick`] e [`ExtHostInputBox`], que o guardam.
//! - Os itens e botões da extensão são os structs [`QuickPickItem`] e [`QuickInputButton`] (`vscode.QuickPickItem` e
//!   `vscode.QuickInputButton`), e a identidade dos objetos (`Map<T, number>` do original) é a do `Rc`. `showQuickPick` devolve o
//!   `handle` do item (o índice na lista que a extensão deu) em vez do item: quem chama (a ponte com o zjsc) guarda a lista e
//!   resolve o `handle`, que é o `items[handle]` do original. O `onDidSelectItem` recebe o `handle` pelo mesmo motivo.
//! - `string | QuickPickItem` é o enum [`QuickPickEntry`]. `tooltip` do item é `string | MarkdownString` e viaja como o
//!   `MarkdownString.fromStrict` do original.
//! - `QuickInputButtons.Back` é o botão com `is_back` (o `handle` dele é -1).
//! - `Promise.race([widgetClosedPromise, itemsPromise])` é uma promessa resolvida pelo que assentar primeiro.
//! - O `[key: string]: any` do `TransferQuickInput` é a lista de [`TransferQuickInputParam`] na ordem de inserção; atribuir de novo
//!   a mesma chave troca o valor no lugar, como a atribuição a uma propriedade existente. `undefined` vira `null` no original e
//!   aqui é o `None` da variante.
//! - `checkProposedApiEnabled` e a `IExtensionDescription` não existem: o `vscode` do host é um só para todas as extensões (ver
//!   `je-exthost`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::discriminant;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{is_cancellation_error, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime::{clear_timeout, set_timeout, TimerId};
use je_base::severity::Severity;
use je_base::uri::URI;
use je_platform::quickinput::quick_input::{IPickOptions, IQuickPickSeparator, InputValidationResult, QuickInputButtonLocation, QuickInputButtonToggle, QuickPickTooltip};
use je_editor::languages::StringOrMarkdown;

use super::ext_host_type_converters::MarkdownString;
use super::ext_host_types::Documentation;
use super::ext_host_protocol::{
    ExtHostQuickOpenShape, IInputBoxOptions, IMainContext, MainContext, MainThreadQuickOpenShape, QuickOpenShowResult, TransferQuickInput, TransferQuickInputButton,
    TransferQuickInputKind, TransferQuickInputParam, TransferQuickPickItem, TransferQuickPickItemOrSeparator,
};
use super::ext_host_commands::ExtHostCommands;
use super::ext_host_type_converters::icon_path::{IconPath, IconPathInput};
use super::ext_host_types::InputBoxValidationSeverity;
use super::ext_host_values::ApiValue;
use super::ext_host_workspace::{IExtHostWorkspaceProvider, WorkspaceFolder};

// ---- os valores da API ------------------------------------------------------------------------------------------------

/// `vscode.QuickInputButton`.
pub struct QuickInputButton {
    pub icon_path: IconPathInput,
    pub tooltip: Option<String>,
    pub location: Option<QuickInputButtonLocation>,
    /// `toggle: { checked }`; a extensão e o host leem e escrevem `checked`.
    pub toggle: Option<QuickInputButtonToggle>,
    /// `QuickInputButtons.Back`.
    pub is_back: bool,
}

/// `vscode.QuickPickItem`.
#[derive(Default)]
pub struct QuickPickItem {
    pub label: String,
    /// `kind === QuickPickItemKind.Separator`.
    pub is_separator: bool,
    pub icon_path: Option<IconPathInput>,
    pub description: Option<String>,
    pub detail: Option<String>,
    /// `tooltip` (`string | MarkdownString`).
    pub tooltip: Option<Documentation>,
    pub picked: Option<bool>,
    pub always_show: Option<bool>,
    pub resource_uri: Option<URI>,
    pub buttons: Option<Vec<Rc<QuickInputButton>>>,
}

/// `Item = string | QuickPickItem`.
#[derive(Clone)]
pub enum QuickPickEntry {
    Text(String),
    Item(Rc<QuickPickItem>),
}

/// `vscode.QuickPickOptions`.
#[derive(Default)]
pub struct QuickPickOptions {
    pub title: Option<String>,
    pub place_holder: Option<String>,
    pub prompt: Option<String>,
    pub match_on_description: Option<bool>,
    pub match_on_detail: Option<bool>,
    pub ignore_focus_out: Option<bool>,
    pub can_pick_many: Option<bool>,
    /// `onDidSelectItem`: recebe o `handle` do item.
    pub on_did_select_item: Option<Rc<dyn Fn(u32)>>,
}

/// `string | InputBoxValidationMessage | undefined | null`, o que `validateInput` devolve (depois de esperada a promessa).
#[derive(Clone, Debug, PartialEq)]
pub enum InputBoxValidation {
    None,
    Text(String),
    /// `{ message, severity? }`; `severity` ausente é o caso `default` do `switch`.
    Message { message: String, severity: Option<InputBoxValidationSeverity> },
}

/// `InputBoxOptions`: as opções que viajam mais a função `validateInput`.
#[derive(Default, Clone)]
pub struct InputBoxOptions {
    pub options: IInputBoxOptions,
    pub validate_input: Option<Rc<dyn Fn(&str) -> MaybePromise<InputBoxValidation>>>,
}

// ---- a conversão dos itens --------------------------------------------------------------------------------------------

fn button_to_transfer(button: &QuickInputButton, handle: i64, with_location: bool) -> TransferQuickInputButton {
    // `iconPathDto: IconPath.from(button.iconPath)`
    let mut transfer = TransferQuickInputButton::new(handle, IconPath::from(Some(button.icon_path.clone())));
    transfer.tooltip = button.tooltip.clone();
    if with_location {
        transfer.location = button.location;
    }
    transfer.toggle = button.toggle.as_ref().map(|toggle| Rc::new(QuickInputButtonToggle { checked: Cell::new(toggle.checked.get()) }));
    transfer
}

/// O item da lista como o DTO. `with_buttons` é o caso de `QuickPick#items` (o `showQuickPick` não envia botões).
fn item_to_transfer(handle: u32, item: &QuickPickItem, with_buttons: bool) -> TransferQuickPickItemOrSeparator {
    if item.is_separator {
        return TransferQuickPickItemOrSeparator::Separator(IQuickPickSeparator {
            id: None,
            label: Some(item.label.clone()),
            description: None,
            aria_label: None,
            buttons: None,
            tooltip: None,
        });
    }
    TransferQuickPickItemOrSeparator::Item(TransferQuickPickItem {
        handle,
        label: Some(item.label.clone()),
        icon_path_dto: IconPath::from(item.icon_path.clone()),
        description: item.description.clone(),
        detail: item.detail.clone(),
        picked: item.picked,
        always_show: item.always_show,
        tooltip: MarkdownString::from_strict(item.tooltip.as_ref()).map(|tooltip| match tooltip {
            StringOrMarkdown::String(text) => QuickPickTooltip::Text(text),
            StringOrMarkdown::Markdown(markdown) => QuickPickTooltip::Markdown(markdown),
        }),
        buttons: if with_buttons {
            item.buttons.as_ref().map(|buttons| buttons.iter().enumerate().map(|(index, button)| button_to_transfer(button, index as i64, false)).collect())
        } else {
            None
        },
        resource_uri: item.resource_uri.clone(),
        ..TransferQuickPickItem::default()
    })
}

// ---- ExtHostQuickInput ------------------------------------------------------------------------------------------------

thread_local! {
    /// `ExtHostQuickInput._nextId`.
    static NEXT_SESSION_ID: Cell<u32> = const { Cell::new(1) };
}

/// `ExtHostQuickInput`: o estado e as notificações comuns ao quick pick e à caixa de entrada.
pub struct ExtHostQuickInput {
    this: Weak<ExtHostQuickInput>,
    proxy: Rc<dyn MainThreadQuickOpenShape>,
    id: u32,
    on_did_dispose: RefCell<Option<Box<dyn FnOnce()>>>,
    title: RefCell<Option<String>>,
    step: Cell<Option<u32>>,
    total_steps: Cell<Option<u32>>,
    visible: Cell<bool>,
    expecting_hide: Cell<bool>,
    enabled: Cell<bool>,
    busy: Cell<bool>,
    ignore_focus_out: Cell<bool>,
    value: RefCell<String>,
    value_selection: Cell<Option<(u32, u32)>>,
    placeholder: RefCell<Option<String>>,
    buttons: RefCell<Vec<Rc<QuickInputButton>>>,
    handles_to_buttons: RefCell<HashMap<i64, Rc<QuickInputButton>>>,
    on_did_accept: Rc<Emitter<()>>,
    on_did_change_value: Rc<Emitter<String>>,
    on_did_trigger_button: Rc<Emitter<Rc<QuickInputButton>>>,
    on_did_hide: Rc<Emitter<()>>,
    update_timeout: Cell<Option<TimerId>>,
    pending_update: RefCell<TransferQuickInput>,
    disposed: Cell<bool>,
    extra_disposables: RefCell<Vec<Rc<dyn IDisposable>>>,
}

impl ExtHostQuickInput {
    fn new(proxy: Rc<dyn MainThreadQuickOpenShape>, kind: TransferQuickInputKind, on_did_dispose: Box<dyn FnOnce()>) -> Rc<Self> {
        let id = NEXT_SESSION_ID.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });
        let input = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            proxy,
            id,
            on_did_dispose: RefCell::new(Some(on_did_dispose)),
            title: RefCell::new(None),
            step: Cell::new(None),
            total_steps: Cell::new(None),
            visible: Cell::new(false),
            expecting_hide: Cell::new(false),
            enabled: Cell::new(true),
            busy: Cell::new(false),
            ignore_focus_out: Cell::new(true),
            value: RefCell::new(String::new()),
            value_selection: Cell::new(None),
            placeholder: RefCell::new(None),
            buttons: RefCell::new(Vec::new()),
            handles_to_buttons: RefCell::new(HashMap::new()),
            on_did_accept: Emitter::new(),
            on_did_change_value: Emitter::new(),
            on_did_trigger_button: Emitter::new(),
            on_did_hide: Emitter::new(),
            update_timeout: Cell::new(None),
            pending_update: RefCell::new(TransferQuickInput { id, kind: None, params: Vec::new() }),
            disposed: Cell::new(false),
            extra_disposables: RefCell::new(Vec::new()),
        });
        // `this.update({ type: 'quickPick' | 'inputBox' })`
        input.pending_update.borrow_mut().kind = Some(kind);
        input
    }

    /// `_id`.
    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn title(&self) -> Option<String> {
        self.title.borrow().clone()
    }

    pub fn set_title(&self, title: Option<String>) {
        *self.title.borrow_mut() = title.clone();
        self.update(vec![TransferQuickInputParam::Title(title)]);
    }

    pub fn step(&self) -> Option<u32> {
        self.step.get()
    }

    pub fn set_step(&self, step: Option<u32>) {
        self.step.set(step);
        self.update(vec![TransferQuickInputParam::Step(step)]);
    }

    pub fn total_steps(&self) -> Option<u32> {
        self.total_steps.get()
    }

    pub fn set_total_steps(&self, total_steps: Option<u32>) {
        self.total_steps.set(total_steps);
        self.update(vec![TransferQuickInputParam::TotalSteps(total_steps)]);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
        self.update(vec![TransferQuickInputParam::Enabled(enabled)]);
    }

    pub fn busy(&self) -> bool {
        self.busy.get()
    }

    pub fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.update(vec![TransferQuickInputParam::Busy(busy)]);
    }

    pub fn ignore_focus_out(&self) -> bool {
        self.ignore_focus_out.get()
    }

    pub fn set_ignore_focus_out(&self, ignore_focus_out: bool) {
        self.ignore_focus_out.set(ignore_focus_out);
        self.update(vec![TransferQuickInputParam::IgnoreFocusOut(ignore_focus_out)]);
    }

    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    pub fn set_value(&self, value: &str) {
        *self.value.borrow_mut() = value.to_owned();
        self.update(vec![TransferQuickInputParam::Value(value.to_owned())]);
    }

    pub fn value_selection(&self) -> Option<(u32, u32)> {
        self.value_selection.get()
    }

    pub fn set_value_selection(&self, value_selection: Option<(u32, u32)>) {
        self.value_selection.set(value_selection);
        self.update(vec![TransferQuickInputParam::ValueSelection(value_selection)]);
    }

    pub fn placeholder(&self) -> Option<String> {
        self.placeholder.borrow().clone()
    }

    pub fn set_placeholder(&self, placeholder: Option<String>) {
        *self.placeholder.borrow_mut() = placeholder.clone();
        self.update(vec![TransferQuickInputParam::Placeholder(placeholder)]);
    }

    pub fn on_did_change_value(&self) -> Event<String> {
        self.on_did_change_value.event()
    }

    pub fn on_did_accept(&self) -> Event<()> {
        self.on_did_accept.event()
    }

    pub fn buttons(&self) -> Vec<Rc<QuickInputButton>> {
        self.buttons.borrow().clone()
    }

    pub fn set_buttons(&self, buttons: Vec<Rc<QuickInputButton>>) {
        *self.buttons.borrow_mut() = buttons.clone();
        let handle_of = |index: usize, button: &QuickInputButton| if button.is_back { -1 } else { index as i64 };
        {
            let mut handles = self.handles_to_buttons.borrow_mut();
            handles.clear();
            for (index, button) in buttons.iter().enumerate() {
                handles.insert(handle_of(index, button), button.clone());
            }
        }
        let transfer = buttons.iter().enumerate().map(|(index, button)| button_to_transfer(button, handle_of(index, button), true)).collect();
        self.update(vec![TransferQuickInputParam::Buttons(transfer)]);
    }

    pub fn on_did_trigger_button(&self) -> Event<Rc<QuickInputButton>> {
        self.on_did_trigger_button.event()
    }

    pub fn show(&self) {
        self.visible.set(true);
        self.expecting_hide.set(true);
        self.update(vec![TransferQuickInputParam::Visible(true)]);
    }

    pub fn hide(&self) {
        self.visible.set(false);
        self.update(vec![TransferQuickInputParam::Visible(false)]);
    }

    pub fn on_did_hide(&self) -> Event<()> {
        self.on_did_hide.event()
    }

    pub fn fire_did_accept(&self) {
        self.on_did_accept.fire(());
    }

    pub fn fire_did_change_value(&self, value: &str) {
        *self.value.borrow_mut() = value.to_owned();
        self.on_did_change_value.fire(value.to_owned());
    }

    pub fn fire_did_trigger_button(&self, handle: i64, checked: Option<bool>) {
        let button = self.handles_to_buttons.borrow().get(&handle).cloned();
        if let Some(button) = button {
            if let (Some(checked), Some(toggle)) = (checked, &button.toggle) {
                toggle.checked.set(checked);
            }
            self.on_did_trigger_button.fire(button);
        }
    }

    pub fn fire_did_hide(&self) {
        if self.expecting_hide.get() {
            // if visible is true, show() was called between hide() and onDidHide. To emit the right number of onDidHide events,
            // expectingHide takes that value, so the next hide() can emit the event again (#135747).
            self.expecting_hide.set(self.visible.get());
            self.on_did_hide.fire(());
        }
    }

    /// Registra um descartável do filho (os emissores do quick pick) para o `dispose`.
    fn add_disposable(&self, disposable: Rc<dyn IDisposable>) {
        self.extra_disposables.borrow_mut().push(disposable);
    }

    /// `update(properties)`: acumula e envia agora (se `visible` mudou) ou depois do laço corrente (se está visível).
    fn update(&self, properties: Vec<TransferQuickInputParam>) {
        if self.disposed.get() {
            return;
        }
        {
            let mut pending = self.pending_update.borrow_mut();
            for property in properties {
                match pending.params.iter_mut().find(|known| discriminant::<TransferQuickInputParam>(&**known) == discriminant(&property)) {
                    Some(known) => *known = property,
                    None => pending.params.push(property),
                }
            }
        }
        let has_visible = self.pending_update.borrow().params.iter().any(|param| matches!(param, TransferQuickInputParam::Visible(_)));
        if has_visible {
            if let Some(timeout) = self.update_timeout.take() {
                clear_timeout(timeout);
            }
            self.dispatch_update();
        } else if self.visible.get() && self.update_timeout.get().is_none() {
            // Defer the update so that multiple changes to setters don't cause a redraw each
            let this = self.this.clone();
            self.update_timeout.set(Some(set_timeout(
                move || {
                    if let Some(this) = this.upgrade() {
                        this.update_timeout.set(None);
                        this.dispatch_update();
                    }
                },
                0,
            )));
        }
    }

    fn dispatch_update(&self) {
        let pending = self.pending_update.replace(TransferQuickInput { id: self.id, kind: None, params: Vec::new() });
        let _ = self.proxy.rpc_create_or_update(pending);
    }
}

impl IDisposable for ExtHostQuickInput {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.fire_did_hide();
        self.on_did_trigger_button.dispose();
        self.on_did_hide.dispose();
        self.on_did_accept.dispose();
        self.on_did_change_value.dispose();
        for disposable in self.extra_disposables.borrow_mut().drain(..) {
            disposable.dispose();
        }
        if let Some(timeout) = self.update_timeout.take() {
            clear_timeout(timeout);
        }
        if let Some(on_did_dispose) = self.on_did_dispose.borrow_mut().take() {
            on_did_dispose();
        }
        let _ = self.proxy.rpc_dispose(self.id);
    }
}

// ---- ExtHostQuickPick -------------------------------------------------------------------------------------------------

/// `ExtHostQuickPick<T>`.
pub struct ExtHostQuickPick {
    input: Rc<ExtHostQuickInput>,
    items: RefCell<Vec<Rc<QuickPickItem>>>,
    handles_to_items: RefCell<HashMap<u32, Rc<QuickPickItem>>>,
    can_select_many: Cell<bool>,
    match_on_description: Cell<bool>,
    match_on_detail: Cell<bool>,
    sort_by_label: Cell<bool>,
    keep_scroll_position: Cell<bool>,
    active_items: RefCell<Vec<Rc<QuickPickItem>>>,
    prompt: RefCell<Option<String>>,
    on_did_change_active: Rc<Emitter<Vec<Rc<QuickPickItem>>>>,
    selected_items: RefCell<Vec<Rc<QuickPickItem>>>,
    on_did_change_selection: Rc<Emitter<Vec<Rc<QuickPickItem>>>>,
    on_did_trigger_item_button: Rc<Emitter<(Rc<QuickInputButton>, Rc<QuickPickItem>)>>,
}

impl ExtHostQuickPick {
    fn new(proxy: Rc<dyn MainThreadQuickOpenShape>, on_dispose: Box<dyn FnOnce()>) -> Rc<Self> {
        let input = ExtHostQuickInput::new(proxy, TransferQuickInputKind::QuickPick, on_dispose);
        let pick = Rc::new(Self {
            input,
            items: RefCell::new(Vec::new()),
            handles_to_items: RefCell::new(HashMap::new()),
            can_select_many: Cell::new(false),
            match_on_description: Cell::new(true),
            match_on_detail: Cell::new(true),
            sort_by_label: Cell::new(true),
            keep_scroll_position: Cell::new(false),
            active_items: RefCell::new(Vec::new()),
            prompt: RefCell::new(None),
            on_did_change_active: Emitter::new(),
            selected_items: RefCell::new(Vec::new()),
            on_did_change_selection: Emitter::new(),
            on_did_trigger_item_button: Emitter::new(),
        });
        pick.input.add_disposable(pick.on_did_change_active.clone());
        pick.input.add_disposable(pick.on_did_change_selection.clone());
        pick.input.add_disposable(pick.on_did_trigger_item_button.clone());
        pick
    }

    /// O núcleo comum (`title`, `value`, `show`, `hide`, `dispose`...).
    pub fn input(&self) -> &Rc<ExtHostQuickInput> {
        &self.input
    }

    pub fn items(&self) -> Vec<Rc<QuickPickItem>> {
        self.items.borrow().clone()
    }

    pub fn set_items(&self, items: Vec<Rc<QuickPickItem>>) {
        *self.items.borrow_mut() = items.clone();
        {
            let mut handles = self.handles_to_items.borrow_mut();
            handles.clear();
            for (index, item) in items.iter().enumerate() {
                handles.insert(index as u32, item.clone());
            }
        }
        let pick_items = items.iter().enumerate().map(|(handle, item)| item_to_transfer(handle as u32, item, true)).collect();
        self.input.update(vec![TransferQuickInputParam::Items(Some(pick_items))]);
    }

    pub fn can_select_many(&self) -> bool {
        self.can_select_many.get()
    }

    pub fn set_can_select_many(&self, value: bool) {
        self.can_select_many.set(value);
        self.input.update(vec![TransferQuickInputParam::CanSelectMany(value)]);
    }

    pub fn match_on_description(&self) -> bool {
        self.match_on_description.get()
    }

    pub fn set_match_on_description(&self, value: bool) {
        self.match_on_description.set(value);
        self.input.update(vec![TransferQuickInputParam::MatchOnDescription(value)]);
    }

    pub fn match_on_detail(&self) -> bool {
        self.match_on_detail.get()
    }

    pub fn set_match_on_detail(&self, value: bool) {
        self.match_on_detail.set(value);
        self.input.update(vec![TransferQuickInputParam::MatchOnDetail(value)]);
    }

    pub fn sort_by_label(&self) -> bool {
        self.sort_by_label.get()
    }

    pub fn set_sort_by_label(&self, value: bool) {
        self.sort_by_label.set(value);
        self.input.update(vec![TransferQuickInputParam::SortByLabel(value)]);
    }

    pub fn keep_scroll_position(&self) -> bool {
        self.keep_scroll_position.get()
    }

    pub fn set_keep_scroll_position(&self, value: bool) {
        self.keep_scroll_position.set(value);
        self.input.update(vec![TransferQuickInputParam::KeepScrollPosition(value)]);
    }

    pub fn prompt(&self) -> Option<String> {
        self.prompt.borrow().clone()
    }

    pub fn set_prompt(&self, prompt: Option<String>) {
        *self.prompt.borrow_mut() = prompt.clone();
        self.input.update(vec![TransferQuickInputParam::Prompt(prompt)]);
    }

    /// O `handle` do item, se ele está na lista (`_itemsToHandles.get(item)`, por identidade).
    fn handle_of(&self, item: &Rc<QuickPickItem>) -> Option<u32> {
        self.items.borrow().iter().position(|known| Rc::ptr_eq(known, item)).map(|index| index as u32)
    }

    pub fn active_items(&self) -> Vec<Rc<QuickPickItem>> {
        self.active_items.borrow().clone()
    }

    pub fn set_active_items(&self, active_items: Vec<Rc<QuickPickItem>>) {
        let kept: Vec<Rc<QuickPickItem>> = active_items.into_iter().filter(|item| self.handle_of(item).is_some()).collect();
        let handles = kept.iter().filter_map(|item| self.handle_of(item)).collect();
        *self.active_items.borrow_mut() = kept;
        self.input.update(vec![TransferQuickInputParam::ActiveItems(handles)]);
    }

    pub fn on_did_change_active(&self) -> Event<Vec<Rc<QuickPickItem>>> {
        self.on_did_change_active.event()
    }

    pub fn selected_items(&self) -> Vec<Rc<QuickPickItem>> {
        self.selected_items.borrow().clone()
    }

    pub fn set_selected_items(&self, selected_items: Vec<Rc<QuickPickItem>>) {
        let kept: Vec<Rc<QuickPickItem>> = selected_items.into_iter().filter(|item| self.handle_of(item).is_some()).collect();
        let handles = kept.iter().filter_map(|item| self.handle_of(item)).collect();
        *self.selected_items.borrow_mut() = kept;
        self.input.update(vec![TransferQuickInputParam::SelectedItems(handles)]);
    }

    pub fn on_did_change_selection(&self) -> Event<Vec<Rc<QuickPickItem>>> {
        self.on_did_change_selection.event()
    }

    /// `coalesce(handles.map(handle => this._handlesToItems.get(handle)))`.
    fn items_of(&self, handles: &[u32]) -> Vec<Rc<QuickPickItem>> {
        let known = self.handles_to_items.borrow();
        handles.iter().filter_map(|handle| known.get(handle).cloned()).collect()
    }

    pub fn fire_did_change_active(&self, handles: &[u32]) {
        let items = self.items_of(handles);
        *self.active_items.borrow_mut() = items.clone();
        self.on_did_change_active.fire(items);
    }

    pub fn fire_did_change_selection(&self, handles: &[u32]) {
        let items = self.items_of(handles);
        *self.selected_items.borrow_mut() = items.clone();
        self.on_did_change_selection.fire(items);
    }

    pub fn on_did_trigger_item_button(&self) -> Event<(Rc<QuickInputButton>, Rc<QuickPickItem>)> {
        self.on_did_trigger_item_button.event()
    }

    pub fn fire_did_trigger_item_button(&self, item_handle: u32, button_handle: i64, checked: Option<bool>) {
        let Some(item) = self.handles_to_items.borrow().get(&item_handle).cloned() else { return };
        let Some(buttons) = &item.buttons else { return };
        if buttons.is_empty() {
            return;
        }
        let Some(button) = usize::try_from(button_handle).ok().and_then(|index| buttons.get(index)).cloned() else { return };
        if let (Some(checked), Some(toggle)) = (checked, &button.toggle) {
            toggle.checked.set(checked);
        }
        self.on_did_trigger_item_button.fire((button, item));
    }
}

// ---- ExtHostInputBox --------------------------------------------------------------------------------------------------

/// `ExtHostInputBox`.
pub struct ExtHostInputBox {
    input: Rc<ExtHostQuickInput>,
    password: Cell<bool>,
    prompt: RefCell<Option<String>>,
    validation_message: RefCell<Option<InputBoxValidation>>,
}

impl ExtHostInputBox {
    fn new(proxy: Rc<dyn MainThreadQuickOpenShape>, on_dispose: Box<dyn FnOnce()>) -> Rc<Self> {
        Rc::new(Self {
            input: ExtHostQuickInput::new(proxy, TransferQuickInputKind::InputBox, on_dispose),
            password: Cell::new(false),
            prompt: RefCell::new(None),
            validation_message: RefCell::new(None),
        })
    }

    /// O núcleo comum.
    pub fn input(&self) -> &Rc<ExtHostQuickInput> {
        &self.input
    }

    pub fn password(&self) -> bool {
        self.password.get()
    }

    pub fn set_password(&self, password: bool) {
        self.password.set(password);
        self.input.update(vec![TransferQuickInputParam::Password(password)]);
    }

    pub fn prompt(&self) -> Option<String> {
        self.prompt.borrow().clone()
    }

    pub fn set_prompt(&self, prompt: Option<String>) {
        *self.prompt.borrow_mut() = prompt.clone();
        self.input.update(vec![TransferQuickInputParam::Prompt(prompt)]);
    }

    pub fn validation_message(&self) -> Option<InputBoxValidation> {
        self.validation_message.borrow().clone()
    }

    /// `set validationMessage`: texto, mensagem com gravidade, ou nada.
    pub fn set_validation_message(&self, validation_message: Option<InputBoxValidation>) {
        *self.validation_message.borrow_mut() = validation_message.clone();
        let params = match validation_message {
            None | Some(InputBoxValidation::None) => vec![TransferQuickInputParam::ValidationMessage(None), TransferQuickInputParam::Severity(Severity::Ignore)],
            Some(InputBoxValidation::Text(text)) if text.is_empty() => vec![TransferQuickInputParam::ValidationMessage(None), TransferQuickInputParam::Severity(Severity::Ignore)],
            Some(InputBoxValidation::Text(text)) => vec![TransferQuickInputParam::ValidationMessage(Some(text)), TransferQuickInputParam::Severity(Severity::Error)],
            Some(InputBoxValidation::Message { message, severity }) => vec![
                TransferQuickInputParam::ValidationMessage(Some(message)),
                // `validationMessage.severity ?? Severity.Error`
                TransferQuickInputParam::Severity(severity.map_or(Severity::Error, validation_severity_to_severity)),
            ],
        };
        self.input.update(params);
    }
}

fn validation_severity_to_severity(severity: InputBoxValidationSeverity) -> Severity {
    match severity {
        InputBoxValidationSeverity::Info => Severity::Info,
        InputBoxValidationSeverity::Warning => Severity::Warning,
        InputBoxValidationSeverity::Error => Severity::Error,
    }
}

// ---- ExtHostQuickOpen -------------------------------------------------------------------------------------------------

/// Uma sessão registrada (`ExtHostQuickInput` ou uma das duas classes que o estendem).
#[derive(Clone)]
enum Session {
    QuickPick(Rc<ExtHostQuickPick>),
    InputBox(Rc<ExtHostInputBox>),
}

impl Session {
    fn input(&self) -> &Rc<ExtHostQuickInput> {
        match self {
            Session::QuickPick(pick) => pick.input(),
            Session::InputBox(input_box) => input_box.input(),
        }
    }
}

/// `ExtHostQuickOpenImpl`.
pub struct ExtHostQuickOpen {
    this: Weak<ExtHostQuickOpen>,
    proxy: Rc<dyn MainThreadQuickOpenShape>,
    workspace: Rc<dyn IExtHostWorkspaceProvider>,
    commands: Rc<ExtHostCommands>,
    on_did_select_item: RefCell<Option<Rc<dyn Fn(u32)>>>,
    validate_input: RefCell<Option<Rc<dyn Fn(&str) -> MaybePromise<InputBoxValidation>>>>,
    sessions: RefCell<HashMap<u32, Session>>,
    instances: Cell<u32>,
}

impl ExtHostQuickOpen {
    /// `createExtHostQuickOpen(mainContext, workspace, commands)`.
    pub fn new(main_context: &dyn IMainContext, workspace: Rc<dyn IExtHostWorkspaceProvider>, commands: Rc<ExtHostCommands>) -> Rc<ExtHostQuickOpen> {
        Rc::new_cyclic(|this| ExtHostQuickOpen {
            this: this.clone(),
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_QUICK_OPEN),
            workspace,
            commands,
            on_did_select_item: RefCell::new(None),
            validate_input: RefCell::new(None),
            sessions: RefCell::new(HashMap::new()),
            instances: Cell::new(0),
        })
    }

    /// `showQuickPick`: o `handle` (ou os `handles`) do que o usuário escolheu, ou `None` se fechou sem escolher (ver as divergências do
    /// módulo). `items` é a lista, ou a promessa dela.
    pub fn show_quick_pick(&self, items: Promise<Vec<QuickPickEntry>>, options: Option<QuickPickOptions>, token: CancellationToken) -> Promise<Option<QuickOpenShowResult>> {
        // clear state from last invocation
        *self.on_did_select_item.borrow_mut() = None;

        let instance = self.instances.get() + 1;
        self.instances.set(instance);
        let options = options.unwrap_or_default();

        let widget = self.proxy.rpc_show(
            instance,
            IPickOptions {
                title: options.title.clone(),
                place_holder: options.place_holder.clone(),
                prompt: options.prompt.clone(),
                match_on_description: options.match_on_description,
                match_on_detail: options.match_on_detail,
                ignore_focus_lost: options.ignore_focus_out,
                can_pick_many: options.can_pick_many,
                ..IPickOptions::default()
            },
            token,
        );

        // `Promise.race([widgetClosedPromise, itemsPromise])`: `None` é o marcador de widget fechado
        let (race, resolver) = Promise::<Option<Rc<Vec<QuickPickEntry>>>>::new();
        let closed = resolver.clone();
        let _ = widget.then_result(move |outcome| {
            match outcome {
                Ok(_) => closed.resolve(None),
                Err(error) => closed.reject(error),
            }
            Ok(())
        });
        let listed = resolver;
        let _ = items.then_result(move |outcome| {
            match outcome {
                Ok(items) => listed.resolve(Some(Rc::new(items))),
                Err(error) => listed.reject(error),
            }
            Ok(())
        });

        let this = self.this.clone();
        let proxy = self.proxy.clone();
        let flow_widget = widget.clone();
        let flow = async_fn(async move {
            let Some(items) = race.await? else { return Ok(None) };

            let mut pick_items: Vec<TransferQuickPickItemOrSeparator> = Vec::new();
            for (handle, entry) in items.iter().enumerate() {
                match entry {
                    QuickPickEntry::Text(label) => {
                        pick_items.push(TransferQuickPickItemOrSeparator::Item(TransferQuickPickItem { handle: handle as u32, label: Some(label.clone()), ..TransferQuickPickItem::default() }));
                    }
                    QuickPickEntry::Item(item) => pick_items.push(item_to_transfer(handle as u32, item, false)),
                }
            }

            // handle selection changes
            if let (Some(on_did_select_item), Some(this)) = (options.on_did_select_item.clone(), this.upgrade()) {
                *this.on_did_select_item.borrow_mut() = Some(on_did_select_item);
            }

            // show items
            let _ = proxy.rpc_set_items(instance, pick_items);

            flow_widget.await
        });

        let proxy = self.proxy.clone();
        flow.then_result(move |outcome| match outcome {
            Ok(result) => Ok(result),
            Err(error) if is_cancellation_error(&error) => Ok(None),
            Err(error) => {
                let _ = proxy.rpc_set_error(instance, error.clone());
                Err(error)
            }
        })
    }

    /// `showInput`.
    pub fn show_input(&self, options: Option<InputBoxOptions>, token: CancellationToken) -> Promise<Option<String>> {
        let options = options.unwrap_or_default();

        // global validate fn used in callback below
        *self.validate_input.borrow_mut() = options.validate_input.clone();

        self.proxy.rpc_input(Some(options.options), options.validate_input.is_some(), token).then_result(|outcome| match outcome {
            Err(error) if is_cancellation_error(&error) => Ok(None),
            other => other,
        })
    }

    /// `showWorkspaceFolderPick`. `options` é o `WorkspaceFolderPickOptions` (um objeto que só o comando interno lê).
    pub fn show_workspace_folder_pick(&self, options: Dynamic) -> Promise<Option<Rc<WorkspaceFolder>>> {
        let (commands, workspace) = (self.commands.clone(), self.workspace.clone());
        async_fn(async move {
            let selected_folder = commands.execute_command("_workbench.pickWorkspaceFolder", vec![Dynamic::array([options])]).await?;
            if selected_folder.is_nullish() {
                return Ok(None);
            }
            let Some(workspace_folders) = workspace.get_workspace_folders2().await? else { return Ok(None) };
            let wanted = URI::from_dynamic(&selected_folder.get_str("uri"))?.to_string(false);
            Ok(workspace_folders.into_iter().find(|folder| folder.uri().to_string(false) == wanted))
        })
    }

    /// `createQuickPick`.
    pub fn create_quick_pick(&self) -> Rc<ExtHostQuickPick> {
        let session_id = Rc::new(Cell::new(0u32));
        let (this, id) = (self.this.clone(), session_id.clone());
        let pick = ExtHostQuickPick::new(
            self.proxy.clone(),
            Box::new(move || {
                if let Some(this) = this.upgrade() {
                    this.sessions.borrow_mut().remove(&id.get());
                }
            }),
        );
        session_id.set(pick.input().id());
        self.sessions.borrow_mut().insert(pick.input().id(), Session::QuickPick(pick.clone()));
        pick
    }

    /// `createInputBox`.
    pub fn create_input_box(&self) -> Rc<ExtHostInputBox> {
        let session_id = Rc::new(Cell::new(0u32));
        let (this, id) = (self.this.clone(), session_id.clone());
        let input_box = ExtHostInputBox::new(
            self.proxy.clone(),
            Box::new(move || {
                if let Some(this) = this.upgrade() {
                    this.sessions.borrow_mut().remove(&id.get());
                }
            }),
        );
        session_id.set(input_box.input().id());
        self.sessions.borrow_mut().insert(input_box.input().id(), Session::InputBox(input_box.clone()));
        input_box
    }

    fn session(&self, session_id: u32) -> Option<Session> {
        self.sessions.borrow().get(&session_id).cloned()
    }
}

impl ExtHostQuickOpenShape for ExtHostQuickOpen {
    fn rpc_on_item_selected(&self, handle: u32) {
        let callback = self.on_did_select_item.borrow().clone();
        if let Some(callback) = callback {
            callback(handle);
        }
    }

    fn rpc_validate_input(&self, input: &str) -> Promise<InputValidationResult> {
        let validate = self.validate_input.borrow().clone();
        let Some(validate) = validate else { return Promise::resolved(InputValidationResult::None) };
        let outcome = validate(input);
        let to_result = |result: InputBoxValidation| match result {
            InputBoxValidation::None => InputValidationResult::None,
            InputBoxValidation::Text(text) => InputValidationResult::Message(text),
            InputBoxValidation::Message { message, severity } => {
                let severity = match severity {
                    Some(severity) => validation_severity_to_severity(severity),
                    None if !message.is_empty() => Severity::Error,
                    None => Severity::Ignore,
                };
                InputValidationResult::Content { content: message, severity }
            }
        };
        match outcome {
            MaybePromise::Value(result) => Promise::resolved(to_result(result)),
            MaybePromise::Promise(promise) => promise.then(to_result),
        }
    }

    fn rpc_on_did_change_active(&self, session_id: u32, handles: Vec<u32>) {
        if let Some(Session::QuickPick(pick)) = self.session(session_id) {
            pick.fire_did_change_active(&handles);
        }
    }

    fn rpc_on_did_change_selection(&self, session_id: u32, handles: Vec<u32>) {
        if let Some(Session::QuickPick(pick)) = self.session(session_id) {
            pick.fire_did_change_selection(&handles);
        }
    }

    fn rpc_on_did_accept(&self, session_id: u32) {
        if let Some(session) = self.session(session_id) {
            session.input().fire_did_accept();
        }
    }

    fn rpc_on_did_change_value(&self, session_id: u32, value: &str) {
        if let Some(session) = self.session(session_id) {
            session.input().fire_did_change_value(value);
        }
    }

    fn rpc_on_did_trigger_button(&self, session_id: u32, handle: Option<i64>, checked: Option<bool>) {
        if let (Some(session), Some(handle)) = (self.session(session_id), handle) {
            session.input().fire_did_trigger_button(handle, checked);
        }
    }

    fn rpc_on_did_trigger_item_button(&self, session_id: u32, item_handle: Option<u32>, button_handle: Option<i64>, checked: Option<bool>) {
        if let (Some(Session::QuickPick(pick)), Some(item_handle), Some(button_handle)) = (self.session(session_id), item_handle, button_handle) {
            pick.fire_did_trigger_item_button(item_handle, button_handle, checked);
        }
    }

    fn rpc_on_did_hide(&self, session_id: u32) {
        if let Some(session) = self.session(session_id) {
            session.input().fire_did_hide();
        }
    }
}
