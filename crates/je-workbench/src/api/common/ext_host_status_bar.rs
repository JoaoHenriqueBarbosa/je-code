//! Porte de `src/vs/workbench/api/common/extHostStatusBar.ts`: o lado da extensão dos itens da barra de status
//! (`vscode.window.createStatusBarItem` e `setStatusBarMessage`).
//!
//! Divergências:
//! - Os setters e getters do `vscode.StatusBarItem` são métodos (`set_text`, `text`, ...). `tooltip2` pode lançar (a
//!   proposta `statusBarItemTooltip` precisa estar habilitada), então `tooltip2` e `set_tooltip2` devolvem [`Result`].
//! - `string | vscode.Command` de `command` é [`StatusBarCommand`], com o `Rc` do comando para a comparação por identidade
//!   que o original faz (`this._command?.fromApi === command`). `tooltip2` (`string | MarkdownString | undefined | (token)
//!   => Promise<...>`) é [`Tooltip2`]; `string | ThemeColor` de `color` é [`StatusBarColor`].
//! - `CommandsConverter` é o trait [`ICommandsConverter`] (`toInternal` devolve `Result`; um erro do conversor lança, como
//!   no setter do original). `setTimeout(..., 0)` e `clearTimeout` são os de [`je_base::runtime`]; o `Timeout` guardado é
//!   um [`TimerId`].
//! - O `_onDispose?: () => void` recebe o `entryId` da própria entrada (o original lê `entry.entryId` do fecho, depois do
//!   construtor). O `ID_GEN` estático é um contador `thread_local`.
//! - `staticItems: ReadonlyMap<string, StatusBarItemDto>` é `&HashMap<String, StatusBarItemDto>`.
//! - `Number.MIN_VALUE` (a prioridade da mensagem de status) é `f64::from_bits(1)`.
//! - `setStatusBarMessage(text, timeoutOrThenable)` recebe o enum [`TimeoutOrThenable`] (um número ou uma promessa); o
//!   `Disposable` devolvido é o `Disposable` de `extHostTypes`.
//! - `ExtHostStatusBar.$provideTooltip` devolve `Option<StringOrMarkdown>` (o `MarkdownString.fromStrict` do original).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, ICancellationToken};
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime::{TimerId, clear_timeout, set_timeout};
use je_base::themables::ThemeColor;
use je_editor::languages::StringOrMarkdown;
use je_extensions::scanner::ExtensionDescription;
use je_platform::accessibility::IAccessibilityInformation;

use super::ext_host_protocol::{ExtHostStatusBarShape, ICommandDto, IMainContext, MainContext, MainThreadStatusBarShape, StatusBarColor, StatusBarItemDto};
use super::ext_host_type_converters::{ICommandsConverter, MarkdownString};
use super::ext_host_types::{self as types, Command, Documentation, StatusBarAlignment as ExtHostStatusBarAlignment, as_status_bar_item_identifier};
use crate::services::extensions::extensions::check_proposed_api_enabled;

thread_local! {
    /// `ExtHostStatusBarEntry.ID_GEN`.
    static ID_GEN: Cell<u64> = const { Cell::new(0) };
}

/// `string | vscode.Command`.
#[derive(Clone, Debug)]
pub enum StatusBarCommand {
    Id(String),
    Command(Rc<Command>),
}

impl StatusBarCommand {
    /// O `===` do original: texto por valor, comando por identidade.
    fn same_as(&self, other: &StatusBarCommand) -> bool {
        match (self, other) {
            (Self::Id(a), Self::Id(b)) => a == b,
            (Self::Command(a), Self::Command(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `(token: vscode.CancellationToken) => Promise<string | vscode.MarkdownString | undefined>`.
pub type TooltipProvider = Rc<dyn Fn(CancellationToken) -> Promise<Option<Documentation>>>;

/// `string | vscode.MarkdownString | undefined | ((token) => Promise<...>)` de `tooltip2` (o `undefined` é `None` no
/// `Option` que o envolve).
#[derive(Clone)]
pub enum Tooltip2 {
    Value(Documentation),
    Provider(TooltipProvider),
}

struct CommandRegistration {
    from_api: StatusBarCommand,
    internal: Option<ICommandDto>,
}

/// `ALLOWED_BACKGROUND_COLORS`: a cor de fundo permitida e o primeiro plano que a acompanha.
fn allowed_background_foreground(background_id: &str) -> Option<ThemeColor> {
    match background_id {
        "statusBarItem.errorBackground" => Some(ThemeColor::new("statusBarItem.errorForeground")),
        "statusBarItem.warningBackground" => Some(ThemeColor::new("statusBarItem.warningForeground")),
        _ => None,
    }
}

/// `validatePriority`: `NaN` e o que não é número viram `undefined`, e o infinito vira o número mais próximo (o RPC usa
/// JSON, que não o carrega; ver https://github.com/microsoft/vscode/issues/133317).
fn validate_priority(priority: Option<f64>) -> Option<f64> {
    let priority = priority.filter(|priority| !priority.is_nan())?;
    Some(if priority == f64::INFINITY {
        f64::MAX
    } else if priority == f64::NEG_INFINITY {
        -f64::MAX
    } else {
        priority
    })
}

pub struct ExtHostStatusBarEntry {
    this: Weak<ExtHostStatusBarEntry>,
    proxy: Rc<dyn MainThreadStatusBarShape>,
    commands: Rc<dyn ICommandsConverter>,

    entry_id: String,
    extension: Option<ExtensionDescription>,
    id: Option<String>,
    alignment: Cell<ExtHostStatusBarAlignment>,
    priority: Cell<Option<f64>>,

    disposed: Cell<bool>,
    visible: Cell<Option<bool>>,

    text: RefCell<String>,
    tooltip: RefCell<Option<Documentation>>,
    tooltip2: RefCell<Option<Tooltip2>>,
    name: RefCell<Option<String>>,
    color: RefCell<Option<StatusBarColor>>,
    background_color: RefCell<Option<ThemeColor>>,
    latest_command_registration: RefCell<Option<Rc<DisposableStore>>>,
    latest_command_registration_was_published: Cell<bool>,
    stale_command_registrations: Rc<DisposableStore>,
    command: RefCell<Option<CommandRegistration>>,

    timeout_handle: Cell<Option<TimerId>>,
    accessibility_information: RefCell<Option<IAccessibilityInformation>>,
    on_dispose: Option<Box<dyn Fn(&str)>>,
}

impl ExtHostStatusBarEntry {
    #[allow(clippy::too_many_arguments)] // os oito parâmetros do construtor do original
    pub fn new(
        proxy: Rc<dyn MainThreadStatusBarShape>,
        commands: Rc<dyn ICommandsConverter>,
        static_items: &HashMap<String, StatusBarItemDto>,
        extension: Option<ExtensionDescription>,
        id: Option<&str>,
        alignment: Option<ExtHostStatusBarAlignment>,
        priority: Option<f64>,
        on_dispose: Option<Box<dyn Fn(&str)>>,
    ) -> Rc<Self> {
        let mut alignment = alignment.unwrap_or(ExtHostStatusBarAlignment::Left);
        let mut priority = priority;
        let mut static_item = None;
        let entry_id = match (id.filter(|id| !id.is_empty()), &extension) {
            (Some(id), Some(extension)) => {
                let entry_id = as_status_bar_item_identifier(&extension.identifier.value, id);
                // if new item already exists mark it as visible and copy properties
                // this can only happen when an item was contributed by an extension
                if let Some(item) = static_items.get(&entry_id) {
                    alignment = if item.align_left { ExtHostStatusBarAlignment::Left } else { ExtHostStatusBarAlignment::Right };
                    priority = item.priority;
                    static_item = Some(item.clone());
                }
                entry_id
            }
            _ => ID_GEN.with(|id_gen| {
                let next = id_gen.get();
                id_gen.set(next + 1);
                next.to_string()
            }),
        };
        let entry = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            proxy,
            commands,
            entry_id,
            extension,
            id: id.map(str::to_owned),
            alignment: Cell::new(alignment),
            priority: Cell::new(validate_priority(priority)),
            disposed: Cell::new(false),
            visible: Cell::new(None),
            text: RefCell::new(String::new()),
            tooltip: RefCell::new(None),
            tooltip2: RefCell::new(None),
            name: RefCell::new(None),
            color: RefCell::new(None),
            background_color: RefCell::new(None),
            latest_command_registration: RefCell::new(None),
            latest_command_registration_was_published: Cell::new(false),
            stale_command_registrations: DisposableStore::new(),
            command: RefCell::new(None),
            timeout_handle: Cell::new(None),
            accessibility_information: RefCell::new(None),
            on_dispose,
        });
        if let Some(item) = static_item {
            entry.visible.set(Some(true));
            entry.set_name(Some(item.name.as_str()));
            entry.set_text(&item.text);
            entry.set_tooltip(item.tooltip.as_deref().map(|tooltip| Documentation::Text(tooltip.to_owned())));
            entry.set_command(item.command.clone().map(StatusBarCommand::Id));
            entry.set_accessibility_information(item.accessibility_information.clone());
        }
        entry
    }

    pub fn id(&self) -> String {
        self.id.clone().unwrap_or_else(|| self.extension.as_ref().expect("a entrada sem id tem extensão").identifier.value.clone())
    }

    pub fn entry_id(&self) -> &str {
        &self.entry_id
    }

    pub fn alignment(&self) -> ExtHostStatusBarAlignment {
        self.alignment.get()
    }

    pub fn priority(&self) -> Option<f64> {
        self.priority.get()
    }

    pub fn text(&self) -> String {
        self.text.borrow().clone()
    }

    pub fn name(&self) -> Option<String> {
        self.name.borrow().clone()
    }

    pub fn tooltip(&self) -> Option<Documentation> {
        self.tooltip.borrow().clone()
    }

    /// `tooltip2` (lê): exige a proposta `statusBarItemTooltip` da extensão, se houver.
    pub fn tooltip2(&self) -> Result<Option<Tooltip2>, Error> {
        if let Some(extension) = &self.extension {
            check_proposed_api_enabled(extension, "statusBarItemTooltip")?;
        }
        Ok(self.tooltip2.borrow().clone())
    }

    pub fn color(&self) -> Option<StatusBarColor> {
        self.color.borrow().clone()
    }

    pub fn background_color(&self) -> Option<ThemeColor> {
        self.background_color.borrow().clone()
    }

    pub fn command(&self) -> Option<StatusBarCommand> {
        self.command.borrow().as_ref().map(|command| command.from_api.clone())
    }

    pub fn accessibility_information(&self) -> Option<IAccessibilityInformation> {
        self.accessibility_information.borrow().clone()
    }

    pub fn set_text(&self, text: &str) {
        *self.text.borrow_mut() = text.to_owned();
        self.update();
    }

    pub fn set_name(&self, name: Option<&str>) {
        *self.name.borrow_mut() = name.map(str::to_owned);
        self.update();
    }

    pub fn set_tooltip(&self, tooltip: Option<Documentation>) {
        *self.tooltip.borrow_mut() = tooltip;
        self.update();
    }

    /// `tooltip2` (escreve): exige a proposta `statusBarItemTooltip` da extensão, se houver.
    pub fn set_tooltip2(&self, tooltip: Option<Tooltip2>) -> Result<(), Error> {
        if let Some(extension) = &self.extension {
            check_proposed_api_enabled(extension, "statusBarItemTooltip")?;
        }

        *self.tooltip2.borrow_mut() = tooltip;
        self.update();
        Ok(())
    }

    pub fn set_color(&self, color: Option<StatusBarColor>) {
        *self.color.borrow_mut() = color;
        self.update();
    }

    pub fn set_background_color(&self, color: Option<ThemeColor>) {
        let color = color.filter(|color| allowed_background_foreground(&color.id).is_some());

        *self.background_color.borrow_mut() = color;
        self.update();
    }

    pub fn set_command(&self, command: Option<StatusBarCommand>) {
        if let (Some(current), Some(command)) = (&*self.command.borrow(), &command)
            && current.from_api.same_as(command)
        {
            return;
        }
        if self.command.borrow().is_none() && command.is_none() {
            return;
        }

        let latest = self.latest_command_registration.borrow().clone();
        if let Some(latest) = latest {
            if self.latest_command_registration_was_published.get() {
                // Hidden entries can still have renderer clicks in flight.
                self.stale_command_registrations.add_rc(latest);
            } else {
                latest.dispose();
            }
        }
        let registration = DisposableStore::new();
        *self.latest_command_registration.borrow_mut() = Some(registration.clone());
        self.latest_command_registration_was_published.set(false);
        let converted = match &command {
            Some(StatusBarCommand::Id(id)) => {
                let internal = self.to_internal(&Command::new("", id.clone()), &registration);
                Some(CommandRegistration { from_api: StatusBarCommand::Id(id.clone()), internal })
            }
            Some(StatusBarCommand::Command(api_command)) => {
                let internal = self.to_internal(api_command, &registration);
                Some(CommandRegistration { from_api: StatusBarCommand::Command(api_command.clone()), internal })
            }
            None => None,
        };
        *self.command.borrow_mut() = converted;
        self.update();
    }

    fn to_internal(&self, command: &Command, registration: &DisposableStore) -> Option<ICommandDto> {
        self.commands.to_internal(Some(command), registration).unwrap_or_else(|error| errors::throw(error))
    }

    pub fn set_accessibility_information(&self, accessibility_information: Option<IAccessibilityInformation>) {
        *self.accessibility_information.borrow_mut() = accessibility_information;
        self.update();
    }

    pub fn show(&self) {
        self.visible.set(Some(true));
        self.update();
    }

    pub fn hide(&self) {
        if let Some(handle) = self.timeout_handle.get() {
            clear_timeout(handle);
        }
        self.visible.set(Some(false));
        self.proxy.rpc_dispose_entry(&self.entry_id);
    }

    fn update(&self) {
        if self.disposed.get() || self.visible.get() != Some(true) {
            return;
        }

        if let Some(handle) = self.timeout_handle.get() {
            clear_timeout(handle);
        }

        // Defer the update so that multiple changes to setters dont cause a redraw each
        let this = self.this.clone();
        self.timeout_handle.set(Some(set_timeout(
            move || {
                if let Some(this) = this.upgrade() {
                    this.flush_update();
                }
            },
            0,
        )));
    }

    fn flush_update(&self) {
        self.timeout_handle.set(None);

        // If the id is not set, derive it from the extension identifier,
        // otherwise make sure to prefix it with the extension identifier
        // to get a more unique value across extensions.
        let id = match (&self.extension, &self.id) {
            (Some(extension), Some(id)) if !id.is_empty() => format!("{}.{id}", extension.identifier.value),
            (Some(extension), _) => extension.identifier.value.clone(),
            (None, id) => id.clone().expect("a entrada sem extensão tem id"),
        };

        // If the name is not set, derive it from the extension descriptor
        let name = match self.name.borrow().clone().filter(|name| !name.is_empty()) {
            Some(name) => name,
            None => {
                let extension = self.extension.as_ref().expect("a entrada sem nome tem extensão");
                let label = extension.manifest.display_name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| extension.manifest.name.clone());
                localize("extensionLabel", "{0} (Extension)", &[label.as_str()])
            }
        };

        // If a background color is set, the foreground is determined
        let mut color = self.color.borrow().clone();
        let background_color = self.background_color.borrow().clone();
        if let Some(background_color) = &background_color {
            color = allowed_background_foreground(&background_color.id).map(StatusBarColor::ThemeColor);
        }

        let tooltip2 = self.tooltip2.borrow().clone();
        let tooltip = self.tooltip.borrow().clone();
        let (tooltip, has_tooltip_provider): (Option<StringOrMarkdown>, bool) = match tooltip2 {
            Some(Tooltip2::Provider(_)) => (MarkdownString::from_strict(tooltip.as_ref()), true),
            Some(Tooltip2::Value(value)) => (MarkdownString::from_strict(Some(&value)), false),
            None => (MarkdownString::from_strict(tooltip.as_ref()), false),
        };

        // Set to status bar
        let command = self.command.borrow().as_ref().and_then(|command| command.internal.clone());
        self.proxy.rpc_set_entry(
            &self.entry_id,
            &id,
            self.extension.as_ref().map(|extension| extension.identifier.value.as_str()),
            &name,
            &self.text.borrow(),
            tooltip,
            has_tooltip_provider,
            command,
            color,
            background_color,
            self.alignment.get() == ExtHostStatusBarAlignment::Left,
            self.priority.get(),
            self.accessibility_information.borrow().clone(),
        );

        self.latest_command_registration_was_published.set(true);

        // clean-up state commands _after_ updating the UI
        self.stale_command_registrations.clear();
    }
}

impl IDisposable for ExtHostStatusBarEntry {
    fn dispose(&self) {
        self.hide();
        self.stale_command_registrations.dispose();
        let latest = self.latest_command_registration.borrow().clone();
        if let Some(latest) = latest {
            latest.dispose();
        }
        if let Some(on_dispose) = &self.on_dispose {
            on_dispose(&self.entry_id);
        }
        self.disposed.set(true);
    }
}

struct StatusBarMessage {
    item: Rc<ExtHostStatusBarEntry>,
    messages: Rc<RefCell<Vec<Rc<String>>>>,
}

impl StatusBarMessage {
    fn new(status_bar: &ExtHostStatusBar) -> Self {
        let item = status_bar.create_status_bar_entry(None, Some("status.extensionMessage"), Some(ExtHostStatusBarAlignment::Left), Some(f64::from_bits(1)));
        item.set_name(Some(localize("status.extensionMessage", "Extension Status", &[]).as_str()));
        Self { item, messages: Rc::new(RefCell::new(Vec::new())) }
    }

    fn dispose(&self) {
        self.messages.borrow_mut().clear();
        self.item.dispose();
    }

    fn set_message(&self, message: &str) -> types::Disposable {
        let data = Rc::new(message.to_owned()); // use object to not confuse equal strings
        self.messages.borrow_mut().insert(0, data.clone());
        Self::update(&self.item, &self.messages);

        let item = self.item.clone();
        let messages = self.messages.clone();
        types::Disposable::new(move || {
            let index = messages.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, &data));
            if let Some(index) = index {
                messages.borrow_mut().remove(index);
                Self::update(&item, &messages);
            }
        })
    }

    fn update(item: &ExtHostStatusBarEntry, messages: &RefCell<Vec<Rc<String>>>) {
        let first = messages.borrow().first().cloned();
        match first {
            Some(message) => {
                item.set_text(&message);
                item.show();
            }
            None => item.hide(),
        }
    }
}

/// `number | Thenable<any>`.
pub enum TimeoutOrThenable {
    Timeout(u64),
    Thenable(Promise<()>),
}

pub struct ExtHostStatusBar {
    this: Weak<ExtHostStatusBar>,
    proxy: Rc<dyn MainThreadStatusBarShape>,
    commands: Rc<dyn ICommandsConverter>,
    status_message: RefCell<Option<StatusBarMessage>>,
    entries: RefCell<HashMap<String, Rc<ExtHostStatusBarEntry>>>,
    existing_items: RefCell<HashMap<String, StatusBarItemDto>>,
}

impl ExtHostStatusBar {
    pub fn new(main_context: &dyn IMainContext, commands: Rc<dyn ICommandsConverter>) -> Rc<Self> {
        let status_bar = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_STATUS_BAR),
            commands,
            status_message: RefCell::new(None),
            entries: RefCell::new(HashMap::new()),
            existing_items: RefCell::new(HashMap::new()),
        });
        let status_message = StatusBarMessage::new(&status_bar);
        *status_bar.status_message.borrow_mut() = Some(status_message);
        status_bar
    }

    /// Descarta a mensagem de status (o original não a descarta; o porte precisa para não deixar a entrada ligada).
    pub fn dispose_status_message(&self) {
        if let Some(status_message) = self.status_message.borrow_mut().take() {
            status_message.dispose();
        }
    }

    pub fn create_status_bar_entry(
        &self,
        extension: Option<ExtensionDescription>,
        id: Option<&str>,
        alignment: Option<ExtHostStatusBarAlignment>,
        priority: Option<f64>,
    ) -> Rc<ExtHostStatusBarEntry> {
        let this = self.this.clone();
        let entry = ExtHostStatusBarEntry::new(
            self.proxy.clone(),
            self.commands.clone(),
            &self.existing_items.borrow(),
            extension,
            id,
            alignment,
            priority,
            Some(Box::new(move |entry_id| {
                if let Some(this) = this.upgrade() {
                    this.entries.borrow_mut().remove(entry_id);
                }
            })),
        );
        self.entries.borrow_mut().insert(entry.entry_id().to_owned(), entry.clone());

        entry
    }

    pub fn set_status_bar_message(&self, text: &str, timeout_or_thenable: Option<TimeoutOrThenable>) -> types::Disposable {
        let d = Rc::new(self.status_message.borrow().as_ref().expect("a mensagem de status existe").set_message(text));
        let handle: Rc<Cell<Option<TimerId>>> = Rc::new(Cell::new(None));

        match timeout_or_thenable {
            Some(TimeoutOrThenable::Timeout(timeout)) => {
                let d = d.clone();
                handle.set(Some(set_timeout(move || d.dispose(), timeout)));
            }
            Some(TimeoutOrThenable::Thenable(thenable)) => {
                let on_settled = d.clone();
                thenable.then_result(move |_| {
                    on_settled.dispose();
                    Ok(())
                });
            }
            None => {}
        }

        types::Disposable::new(move || {
            d.dispose();
            if let Some(handle) = handle.get() {
                clear_timeout(handle);
            }
        })
    }
}

impl ExtHostStatusBarShape for ExtHostStatusBar {
    fn rpc_accept_static_entries(&self, added: Vec<StatusBarItemDto>) {
        for item in added {
            self.existing_items.borrow_mut().insert(item.entry_id.clone(), item);
        }
    }

    fn rpc_provide_tooltip(&self, entry_id: &str, cancellation: CancellationToken) -> Promise<Option<StringOrMarkdown>> {
        let entry = self.entries.borrow().get(entry_id).cloned();
        async_fn(async move {
            let Some(entry) = entry else {
                return Ok(None);
            };

            let tooltip = match entry.tooltip2()? {
                Some(Tooltip2::Provider(provider)) => provider(cancellation.clone()).await?,
                Some(Tooltip2::Value(value)) => Some(value),
                None => None,
            };
            Ok(if !cancellation.is_cancellation_requested() { MarkdownString::from_strict(tooltip.as_ref()) } else { None })
        })
    }
}
