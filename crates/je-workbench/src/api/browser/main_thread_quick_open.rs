//! Porte de `src/vs/workbench/api/browser/mainThreadQuickOpen.ts`: a ponte entre o `IQuickInputService` do workbench
//! e as APIs `window.showQuickPick`, `window.showInputBox`, `createQuickPick` e `createInputBox` das extensões.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadQuickOpen)` e o `IExtHostContext` do construtor: o registro de
//!   clientes não existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostQuickOpen`.
//! - O item do quick pick é o [`IQuickPickItem`] sem genérico (ver `quick_input.rs`): o `TransferQuickPickItem` vira um
//!   [`IQuickPickItem`] com o `handle` em `extra`, e o `handle` de um evento sai de lá. O botão acionado é achado pela
//!   identidade do `Rc` num registro de `(botão, handle)`, no lugar do `button as TransferQuickInputButton`.
//! - `expandItemProps` e `expandIconPath` mutam o `TransferQuickPickItem` no lugar; aqui mutam a cópia que o RPC
//!   entregou, antes de convertê-la. O getter preguiçoso de `iconClasses` (`Lazy` + `Object.defineProperty`) é calculado na
//!   hora, com [`get_icon_classes`]: a função é pura.
//! - `$createOrUpdate` percorre as chaves de `params` na ordem de inserção ([`TransferQuickInputParam`]); as que o
//!   original grava com `(input as any)[param] = value` são os setters do controlador, e uma chave que não se aplica ao
//!   tipo do controlador (`canSelectMany` numa caixa de entrada) ou que o host não define ([`TransferQuickInputParam::Other`])
//!   é ignorada, em vez de pendurar uma propriedade solta.
//! - O `finally` de `$show` roda também quando o `pick` lança de forma síncrona: o corpo captura o pânico (o `throw` do
//!   porte) e só depois solta o pedido pendente.
//! - O `_items` privado (lido pelo teste com `Reflect.get`) tem o acessor [`MainThreadQuickOpen::pending_items`].

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{Promise, Resolver};
use je_base::themables::ThemeIcon;
use je_editor::get_icon_classes::get_icon_classes;
use je_editor::language_service::ILanguageService;
use je_editor::services_model::IModelService;
use je_platform::files::files::FileKind;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::quickinput::quick_input::{
    IInputBox, IInputOptions, IPickOptions, IQuickInput, IQuickInputButton, IQuickInputService, IQuickItem, IQuickPick, IQuickPickItem, QuickInputButtonRef,
    QuickInputIconPath, QuickPickInput, QuickPicks,
};
use je_platform::resources::ext_uri;

use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostQuickOpenShape, IInputBoxOptions, IMainContext, IconPathDto, MainThreadQuickOpenShape, QuickOpenShowResult, TransferQuickInput,
    TransferQuickInputButton, TransferQuickInputKind, TransferQuickInputParam, TransferQuickPickItem, TransferQuickPickItemOrSeparator,
};
use crate::services::editor::custom_editor_label_service::ICustomEditorLabelService;

/// Os botões que o RPC entregou, com o `handle` de cada um (a identidade do `Rc` é o `===` do original).
type ButtonHandles = Rc<RefCell<Vec<(QuickInputButtonRef, i64)>>>;

/// O controlador de uma sessão: o `input as IQuickPick` do original é o ramo.
#[derive(Clone)]
enum SessionInput {
    QuickPick(Rc<dyn IQuickPick>),
    InputBox(Rc<dyn IInputBox>),
}

impl SessionInput {
    fn base(&self) -> &dyn IQuickInput {
        match self {
            Self::QuickPick(input) => &**input,
            Self::InputBox(input) => &**input,
        }
    }
}

/// `QuickInputSession`.
struct QuickInputSession {
    input: SessionInput,
    handles_to_items: RefCell<HashMap<u32, IQuickPickItem>>,
    button_handles: ButtonHandles,
    store: Rc<DisposableStore>,
}

/// `MainThreadQuickOpen`.
pub struct MainThreadQuickOpen {
    proxy: Rc<dyn ExtHostQuickOpenShape>,
    quick_input_service: Rc<dyn IQuickInputService>,
    label_service: Rc<dyn ILabelService>,
    custom_editor_label_service: Rc<dyn ICustomEditorLabelService>,
    model_service: Rc<dyn IModelService>,
    language_service: Rc<dyn ILanguageService>,
    items: Rc<RefCell<BTreeMap<u32, Resolver<Vec<TransferQuickPickItemOrSeparator>>>>>,
    sessions: RefCell<HashMap<u32, Rc<QuickInputSession>>>,
}

impl MainThreadQuickOpen {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        quick_input_service: Rc<dyn IQuickInputService>,
        label_service: Rc<dyn ILabelService>,
        custom_editor_label_service: Rc<dyn ICustomEditorLabelService>,
        model_service: Rc<dyn IModelService>,
        language_service: Rc<dyn ILanguageService>,
    ) -> Rc<Self> {
        Rc::new(Self {
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_QUICK_OPEN),
            quick_input_service,
            label_service,
            custom_editor_label_service,
            model_service,
            language_service,
            items: Rc::new(RefCell::new(BTreeMap::new())),
            sessions: RefCell::new(HashMap::new()),
        })
    }

    /// As instâncias de `$show` que ainda esperam os itens (as chaves de `_items`, em ordem crescente).
    pub fn pending_items(&self) -> Vec<u32> {
        self.items.borrow().keys().copied().collect()
    }

    fn create_session(&self, session_id: u32, kind: Option<TransferQuickInputKind>) -> Rc<QuickInputSession> {
        let button_handles: ButtonHandles = Rc::new(RefCell::new(Vec::new()));
        let store = DisposableStore::new();
        let input = match kind {
            Some(TransferQuickInputKind::QuickPick) => SessionInput::QuickPick(self.quick_input_service.create_quick_pick(None)),
            _ => SessionInput::InputBox(self.quick_input_service.create_input_box()),
        };
        match &input {
            SessionInput::QuickPick(input) => store.add(Rc::new(input.clone())),
            SessionInput::InputBox(input) => store.add(Rc::new(input.clone())),
        };

        let proxy = self.proxy.clone();
        match &input {
            SessionInput::QuickPick(quick_pick) => {
                store.add(quick_pick.on_did_accept().listen({
                    let proxy = proxy.clone();
                    move |_| proxy.rpc_on_did_accept(session_id)
                }));
                store.add(quick_pick.on_did_change_value().listen({
                    let proxy = proxy.clone();
                    move |value| proxy.rpc_on_did_change_value(session_id, value)
                }));
            }
            SessionInput::InputBox(input_box) => {
                store.add(input_box.on_did_accept().listen({
                    let proxy = proxy.clone();
                    move |_| proxy.rpc_on_did_accept(session_id)
                }));
                store.add(input_box.on_did_change_value().listen({
                    let proxy = proxy.clone();
                    move |value| proxy.rpc_on_did_change_value(session_id, value)
                }));
            }
        }
        let base = input.base();
        store.add(base.on_did_trigger_button().listen({
            let proxy = proxy.clone();
            let button_handles = button_handles.clone();
            move |button| proxy.rpc_on_did_trigger_button(session_id, handle_of(&button_handles, button), button.toggle.as_ref().map(|toggle| toggle.checked.get()))
        }));
        store.add(base.on_did_hide().listen({
            let proxy = proxy.clone();
            move |_| proxy.rpc_on_did_hide(session_id)
        }));

        if let SessionInput::QuickPick(quick_pick) = &input {
            // Add extra events specific for quick pick
            store.add(quick_pick.on_did_change_active().listen({
                let proxy = proxy.clone();
                move |items| proxy.rpc_on_did_change_active(session_id, items.iter().filter_map(item_handle).collect())
            }));
            store.add(quick_pick.on_did_change_selection().listen({
                let proxy = proxy.clone();
                move |items| proxy.rpc_on_did_change_selection(session_id, items.iter().filter_map(item_handle).collect())
            }));
            store.add(quick_pick.on_did_trigger_item_button().listen({
                let proxy = proxy.clone();
                let button_handles = button_handles.clone();
                move |event| {
                    proxy.rpc_on_did_trigger_item_button(
                        session_id,
                        item_handle(&event.item),
                        handle_of(&button_handles, &event.button),
                        event.button.toggle.as_ref().map(|toggle| toggle.checked.get()),
                    )
                }
            }));
        }

        let session = Rc::new(QuickInputSession { input, handles_to_items: RefCell::new(HashMap::new()), button_handles, store });
        self.sessions.borrow_mut().insert(session_id, session.clone());
        session
    }

    /// Aplica uma chave de `params` ao controlador (o corpo do `switch (param)` de `$createOrUpdate`).
    fn apply_param(&self, session: &QuickInputSession, param: TransferQuickInputParam) {
        let input = session.input.base();
        let quick_pick = match &session.input {
            SessionInput::QuickPick(quick_pick) => Some(quick_pick),
            SessionInput::InputBox(_) => None,
        };
        let input_box = match &session.input {
            SessionInput::InputBox(input_box) => Some(input_box),
            SessionInput::QuickPick(_) => None,
        };

        match param {
            TransferQuickInputParam::Visible(visible) => {
                if visible {
                    input.show();
                } else {
                    input.hide();
                }
            }

            TransferQuickInputParam::Items(items) => {
                session.handles_to_items.borrow_mut().clear();
                let mut converted = Vec::new();
                for item in items.unwrap_or_default() {
                    match item {
                        TransferQuickPickItemOrSeparator::Separator(separator) => converted.push(QuickPickInput::Separator(separator)),
                        TransferQuickPickItemOrSeparator::Item(mut item) => {
                            self.expand_item_props(&mut item);
                            for button in item.buttons.iter_mut().flatten() {
                                expand_icon_path(&Some(button.icon_path_dto.clone()), &mut button.icon_path, &mut button.icon_class);
                            }
                            let pick_item = to_pick_item(&item, &session.button_handles);
                            session.handles_to_items.borrow_mut().insert(item.handle, pick_item.clone());
                            converted.push(QuickPickInput::Item(pick_item));
                        }
                    }
                }
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_items(converted);
                }
            }

            TransferQuickInputParam::ActiveItems(handles) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_active_items(items_for_handles(session, &handles));
                }
            }

            TransferQuickInputParam::SelectedItems(handles) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_selected_items(items_for_handles(session, &handles));
                }
            }

            TransferQuickInputParam::Buttons(transfer_buttons) => {
                let mut buttons = Vec::new();
                for mut button in transfer_buttons {
                    if button.handle == -1 {
                        buttons.push(self.quick_input_service.back_button());
                    } else {
                        expand_icon_path(&Some(button.icon_path_dto.clone()), &mut button.icon_path, &mut button.icon_class);
                        buttons.push(to_button(&button, &session.button_handles));
                    }
                }
                input.set_buttons(buttons);
            }

            TransferQuickInputParam::Title(title) => input.set_title(title),
            TransferQuickInputParam::Step(step) => input.set_step(step),
            TransferQuickInputParam::TotalSteps(total_steps) => input.set_total_steps(total_steps),
            TransferQuickInputParam::Enabled(enabled) => input.set_enabled(enabled),
            TransferQuickInputParam::Busy(busy) => input.set_busy(busy),
            TransferQuickInputParam::IgnoreFocusOut(ignore_focus_out) => input.set_ignore_focus_out(ignore_focus_out),
            TransferQuickInputParam::Value(value) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_value(value);
                } else if let Some(input_box) = input_box {
                    input_box.set_value(value);
                }
            }
            TransferQuickInputParam::ValueSelection(selection) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_value_selection(selection);
                } else if let Some(input_box) = input_box {
                    input_box.set_value_selection(selection);
                }
            }
            TransferQuickInputParam::Placeholder(placeholder) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_placeholder(placeholder);
                } else if let Some(input_box) = input_box {
                    input_box.set_placeholder(placeholder);
                }
            }
            TransferQuickInputParam::Prompt(prompt) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_prompt(prompt);
                } else if let Some(input_box) = input_box {
                    input_box.set_prompt(prompt);
                }
            }
            TransferQuickInputParam::ValidationMessage(message) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_validation_message(message);
                } else if let Some(input_box) = input_box {
                    input_box.set_validation_message(message);
                }
            }
            TransferQuickInputParam::Severity(severity) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_severity(severity);
                } else if let Some(input_box) = input_box {
                    input_box.set_severity(severity);
                }
            }
            TransferQuickInputParam::Password(password) => {
                if let Some(input_box) = input_box {
                    input_box.set_password(password);
                }
            }
            TransferQuickInputParam::CanSelectMany(value) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_can_select_many(value);
                }
            }
            TransferQuickInputParam::MatchOnDescription(value) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_match_on_description(value);
                }
            }
            TransferQuickInputParam::MatchOnDetail(value) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_match_on_detail(value);
                }
            }
            TransferQuickInputParam::SortByLabel(value) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_sort_by_label(value);
                }
            }
            TransferQuickInputParam::KeepScrollPosition(value) => {
                if let Some(quick_pick) = quick_pick {
                    quick_pick.set_keep_scroll_position(value);
                }
            }
            TransferQuickInputParam::Other(..) => {}
        }
    }

    /// Derives icon, label and description for Quick Pick items that represent a resource URI.
    fn expand_item_props(&self, item: &mut TransferQuickPickItem) {
        let Some(resource_uri) = item.resource_uri.clone() else {
            expand_icon_path(&item.icon_path_dto.clone(), &mut item.icon_path, &mut item.icon_class);
            return;
        };

        // Derive missing label and description from resourceUri.
        if item.label.is_none() {
            item.label = Some(self.custom_editor_label_service.get_name(&resource_uri).unwrap_or_default());
        }
        let relative = UriLabelOptions { relative: Some(true), no_prefix: None, separator: None, append_workspace_suffix: None };
        if item.label.as_deref().is_some_and(|label| !label.is_empty()) {
            if item.description.is_none() {
                item.description = Some(self.label_service.get_uri_label(&resource_uri, &relative));
            }
        } else {
            item.label = Some(ext_uri().basename_or_authority(&resource_uri));
            if item.description.is_none() {
                item.description = Some(self.label_service.get_uri_label(&ext_uri().dirname(&resource_uri), &relative));
            }
        }

        // Derive icon props from resourceUri if icon is set to ThemeIcon.File or ThemeIcon.Folder.
        if let Some(IconPathDto::ThemeIcon(icon)) = &item.icon_path_dto
            && (ThemeIcon::is_file(Some(icon)) || ThemeIcon::is_folder(Some(icon)))
        {
            let file_kind = if ThemeIcon::is_folder(Some(icon)) || ext_uri().has_trailing_path_separator(&resource_uri, None) { FileKind::Folder } else { FileKind::File };
            item.icon_classes = Some(get_icon_classes(&*self.model_service, &*self.language_service, Some(&resource_uri), Some(file_kind), None));
        } else {
            expand_icon_path(&item.icon_path_dto.clone(), &mut item.icon_path, &mut item.icon_class);
        }
    }
}

impl IDisposable for MainThreadQuickOpen {
    fn dispose(&self) {
        let sessions: Vec<Rc<QuickInputSession>> = self.sessions.borrow().values().cloned().collect();
        for session in sessions {
            session.store.dispose();
        }
    }
}

impl MainThreadQuickOpenShape for MainThreadQuickOpen {
    fn rpc_show(&self, instance: u32, options: IPickOptions, token: CancellationToken) -> Promise<Option<QuickOpenShowResult>> {
        let (contents, resolver) = Promise::<Vec<TransferQuickPickItemOrSeparator>>::new();
        self.items.borrow_mut().insert(instance, resolver);

        let mut options = options;
        let proxy = self.proxy.clone();
        options.on_did_focus = Some(Rc::new(move |element: &IQuickPickItem| {
            if let Some(handle) = item_handle(element) {
                proxy.rpc_on_item_selected(handle);
            }
        }));

        let quick_input_service = self.quick_input_service.clone();
        let pending = self.items.clone();
        let button_handles: ButtonHandles = Rc::new(RefCell::new(Vec::new()));
        async_fn(async move {
            let picks = QuickPicks::Promised(contents.then(move |items| {
                items
                    .iter()
                    .map(|item| match item {
                        TransferQuickPickItemOrSeparator::Item(item) => QuickPickInput::Item(to_pick_item(item, &button_handles)),
                        TransferQuickPickItemOrSeparator::Separator(separator) => QuickPickInput::Separator(separator.clone()),
                    })
                    .collect()
            }));

            // O `try`: um `pick` que lança de forma síncrona vira a rejeição, e o `finally` abaixo roda do mesmo jeito.
            let outcome = errors::catch(|| {
                if options.can_pick_many == Some(true) {
                    quick_input_service
                        .pick_many(picks, Some(&options), Some(&token))
                        .then(|items| items.map(|items| QuickOpenShowResult::Many(items.iter().filter_map(item_handle).collect())))
                } else {
                    quick_input_service.pick(picks, Some(&options), Some(&token)).then(|item| item.as_ref().and_then(item_handle).map(QuickOpenShowResult::One))
                }
            });
            let result = match outcome {
                Ok(promise) => promise.await,
                Err(error) => Err(error),
            };

            // Closing before items arrive must also release the pending contents.
            pending.borrow_mut().remove(&instance);
            result
        })
    }

    fn rpc_set_items(&self, instance: u32, items: Vec<TransferQuickPickItemOrSeparator>) -> Promise<()> {
        let resolver = self.items.borrow().get(&instance).cloned();
        if let Some(resolver) = resolver {
            let expanded = items
                .into_iter()
                .map(|item| match item {
                    TransferQuickPickItemOrSeparator::Item(mut item) => {
                        self.expand_item_props(&mut item);
                        TransferQuickPickItemOrSeparator::Item(item)
                    }
                    separator @ TransferQuickPickItemOrSeparator::Separator(_) => separator,
                })
                .collect();
            resolver.resolve(expanded);
            self.items.borrow_mut().remove(&instance);
        }
        Promise::resolved(())
    }

    fn rpc_set_error(&self, instance: u32, error: Error) -> Promise<()> {
        let resolver = self.items.borrow().get(&instance).cloned();
        if let Some(resolver) = resolver {
            resolver.reject(error);
            self.items.borrow_mut().remove(&instance);
        }
        Promise::resolved(())
    }

    // ---- input

    fn rpc_input(&self, options: Option<IInputBoxOptions>, validate_input: bool, token: CancellationToken) -> Promise<Option<String>> {
        let mut input_options = IInputOptions::default();

        if let Some(options) = options {
            input_options.title = options.title;
            input_options.password = options.password;
            input_options.place_holder = options.place_holder;
            input_options.value_selection = options.value_selection;
            input_options.prompt = options.prompt;
            input_options.value = options.value;
            input_options.ignore_focus_lost = options.ignore_focus_out;
        }

        if validate_input {
            let proxy = self.proxy.clone();
            input_options.validate_input = Some(Rc::new(move |value: &str| proxy.rpc_validate_input(value)));
        }

        self.quick_input_service.input(Some(&input_options), Some(&token))
    }

    // ---- QuickInput

    fn rpc_create_or_update(&self, params: TransferQuickInput) -> Promise<()> {
        let session_id = params.id;
        let existing = self.sessions.borrow().get(&session_id).cloned();
        let session = match existing {
            Some(session) => session,
            None => self.create_session(session_id, params.kind),
        };

        for param in params.params {
            self.apply_param(&session, param);
        }
        Promise::resolved(())
    }

    fn rpc_dispose(&self, session_id: u32) -> Promise<()> {
        let session = self.sessions.borrow_mut().remove(&session_id);
        if let Some(session) = session {
            session.store.dispose();
        }
        Promise::resolved(())
    }
}

/// O `handle` que o `TransferQuickPickItem` deixou em `extra`.
fn item_handle(item: &IQuickPickItem) -> Option<u32> {
    match item.extra.get_str("handle") {
        Dynamic::Number(handle) => Some(handle as u32),
        _ => None,
    }
}

/// O `handle` do botão (o `(button as TransferQuickInputButton).handle`), se ele veio do RPC.
fn handle_of(button_handles: &ButtonHandles, button: &QuickInputButtonRef) -> Option<i64> {
    button_handles.borrow().iter().find(|(known, _)| Rc::ptr_eq(known, button)).map(|(_, handle)| *handle)
}

/// `activeItems`/`selectedItems`: `handles.map(handle => handlesToItems.get(handle)).filter(Boolean)`.
fn items_for_handles(session: &QuickInputSession, handles: &[u32]) -> Vec<IQuickPickItem> {
    let handles_to_items = session.handles_to_items.borrow();
    handles.iter().filter_map(|handle| handles_to_items.get(handle).cloned()).collect()
}

/// Converts IconPath DTO into iconPath/iconClass properties.
fn expand_icon_path(icon: &Option<IconPathDto>, icon_path: &mut Option<QuickInputIconPath>, icon_class: &mut Option<String>) {
    match icon {
        None => {}
        Some(IconPathDto::ThemeIcon(icon)) => {
            // O original anota que, como `IQuickPickItem` e `IQuickInputButton` não aceitam `ThemeIcon`, o id da cor se perde aqui.
            *icon_class = Some(icon.as_class_name());
        }
        Some(IconPathDto::Uri(uri)) => *icon_path = Some(QuickInputIconPath { dark: uri.clone(), light: Some(uri.clone()) }),
        Some(IconPathDto::Themed { light, dark }) => {
            *icon_path = Some(QuickInputIconPath { dark: dark.clone(), light: Some(light.clone()) });
        }
    }
}

/// O botão do RPC como o [`IQuickInputButton`] do widget, registrado com o `handle` dele.
fn to_button(button: &TransferQuickInputButton, button_handles: &ButtonHandles) -> QuickInputButtonRef {
    let converted: QuickInputButtonRef = Rc::new(IQuickInputButton {
        icon_path: button.icon_path.clone(),
        icon_class: button.icon_class.clone(),
        tooltip: button.tooltip.clone(),
        always_visible: button.always_visible,
        location: button.location,
        toggle: button.toggle.clone(),
        label: button.label.clone(),
        secondary: button.secondary,
    });
    button_handles.borrow_mut().push((converted.clone(), button.handle));
    converted
}

/// O `TransferQuickPickItem` (já expandido) como o [`IQuickPickItem`] do widget, com o `handle` em `extra`.
fn to_pick_item(item: &TransferQuickPickItem, button_handles: &ButtonHandles) -> IQuickPickItem {
    IQuickPickItem {
        base: IQuickItem {
            label: JsString::from(item.label.clone().unwrap_or_default().as_str()),
            description: item.description.as_deref().map(JsString::from),
            icon_classes: item.icon_classes.clone(),
            icon_path: item.icon_path.clone(),
            icon_class: item.icon_class.clone(),
            buttons: item.buttons.as_ref().map(|buttons| buttons.iter().map(|button| to_button(button, button_handles)).collect()),
            ..IQuickItem::default()
        },
        extra: Dynamic::object([("handle", Dynamic::Number(f64::from(item.handle)))]),
        detail: item.detail.clone(),
        picked: item.picked,
        always_show: item.always_show,
        ..IQuickPickItem::default()
    }
}
