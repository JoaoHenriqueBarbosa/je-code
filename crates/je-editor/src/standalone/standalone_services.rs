//! Porte de `src/vs/editor/standalone/browser/standaloneServices.ts`, nos serviços que o editor standalone traz no lugar dos
//! do workbench: [`StandaloneConfigurationService`], [`StandaloneNotificationService`], [`StandaloneCommandService`] e
//! [`StandaloneKeybindingService`].
//!
//! Os demais serviços do arquivo (progresso, diálogos, ambiente, telemetria, rótulos, menus, confiança de workspace...) entram
//! com o contêiner de serviços do editor standalone (`StandaloneServices`), que reúne o que o arquivo registra por
//! `registerSingleton`.
//!
//! Decisões de tradução:
//! - **`StandaloneConfigurationService`**
//!   - `getValue(arg1?, arg2?)` é a forma do trait [`IConfigurationService`]: seção e `overrides` opcionais.
//!   - `updateValue(key, value, arg3?, arg4?)` ignora `overrides` e `target`, como o original.
//!   - `updateValues` é [`StandaloneConfigurationService::update_values`], com a comparação `===` do original
//!     ([`Dynamic::strict_equals`]).
//! - **`StandaloneNotificationService`.** `console.error`/`console.warn`/`console.log` são `eprintln!`/`eprintln!`/`println!`.
//!   `info`, `warn` e `error` recebem a lista de mensagens do trait [`INotificationService`] e notificam uma a uma. O
//!   `NO_OP` estático é o [`NoOpNotification`] guardado no serviço.
//! - **`StandaloneCommandService`.** `instantiationService.invokeFunction(command.handler, ...args)` é
//!   [`invoke_registered_command`], o miolo que o `CommandService` do workbench também usa.
//! - **`StandaloneKeybindingService`**
//!   - A classe base `AbstractKeybindingService` é o [`KeybindingServiceHost`] com o [`KeybindingServiceState`]; o
//!     `_dispatch` e o `_singleModifierDispatch` (protegidos) são `dispatch_internal` e `single_modifier_dispatch`, funções
//!     de `je_platform::keybinding::abstract_keybinding_service`.
//!   - **Sem `ICodeEditorService` e sem DOM.** O original escuta `keydown`/`keyup` em cada contêiner de editor
//!     (`addContainer`/`removeContainer`) e entrega o evento ao `_dispatch`. A UI nativa não tem nó de DOM: o host entrega o
//!     `KeyboardEvent` do editor que tem o foco direto a `dispatch_internal` (`keydown`) e a `single_modifier_dispatch`
//!     (`keyup`). Por isso o construtor não recebe o `ICodeEditorService`.
//!   - **`_documentHasFocus()`** (`mainWindow.document.hasFocus()`) é o parâmetro `document_has_focus`, a função que o host
//!     da janela dá ao serviço.
//!   - **`IKeybindingRule`** é [`StandaloneKeybindingRule`] (o nome `KeybindingRule` já é o da regra do registro).
//!   - **`this._dynamicKeybindings`** guarda `Rc<KeybindingItem>`: a remoção procura a primeira entrada pela identidade
//!     (`===`), que é o `Rc::ptr_eq`.
//!   - **`_getResolver`** passa ao resolvedor o `(str) => this._log(str)` de [`KeybindingServiceState::logger`].
//!   - **`OS`** é o sistema detectado na partida.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::keybindings::{KeyCodeChord, Keybinding, ResolvedKeybinding, decode_keybinding};
use je_base::lifecycle::{DisposableRef, IDisposable, combined_disposable, none, to_disposable};
use je_base::map::ResourceMap;
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_instantiation::InstantiationService;
use je_platform::commands::{CommandEvent, CommandHandler, ICommandService, commands_registry, invoke_registered_command};
use je_platform::configuration::defaults::DefaultConfiguration;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationKeys, ConfigurationModel, PreviousConfiguration};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{
    ConfigurationChange, ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_platform::contextkey::{ContextKeyExpression, IContextKeyService};
use je_platform::keybinding::abstract_keybinding_service::{KeybindingServiceHost, KeybindingServiceState};
use je_platform::keybinding::keybinding_resolver::KeybindingResolver;
use je_platform::keybinding::keybindings_registry::{KeybindingItem, keybindings_registry};
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_platform::keybinding::{KeyboardEvent, KeybindingsSchemaContribution};
use je_platform::log::LogServiceRef;
use je_platform::notification::{
    INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource,
    NotificationSourceFilter, NotificationsFilter, PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};
use je_platform::telemetry::ITelemetryService;

/// `StandaloneConfigurationService`.
pub struct StandaloneConfigurationService {
    on_did_change_configuration: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
    configuration: Rc<Configuration>,
    log_service: LogServiceRef,
}

impl StandaloneConfigurationService {
    pub fn new(log_service: LogServiceRef) -> Rc<Self> {
        let on_did_change_configuration = Emitter::new();
        let default_configuration = DefaultConfiguration::new(log_service.clone());
        let empty = || ConfigurationModel::create_empty_model(log_service.clone());
        let configuration = Configuration::new(
            default_configuration.reload(),
            empty(),
            empty(),
            empty(),
            empty(),
            empty(),
            ResourceMap::new(),
            empty(),
            ResourceMap::new(),
            log_service.clone(),
        );
        default_configuration.dispose();
        Rc::new(Self { on_did_change_configuration, configuration: Rc::new(configuration), log_service })
    }

    /// `updateValues(values)`: grava na memória os pares cujo valor mudou e dispara um só evento com as chaves
    /// alteradas.
    pub fn update_values(&self, values: Vec<(&str, Dynamic)>) -> Promise<()> {
        let previous = PreviousConfiguration { workspace: None, data: self.configuration.to_data() };

        let mut changed_keys: Vec<JsString> = Vec::new();
        for (key, value) in values {
            if IConfigurationService::get_value(self, Some(key), None).strict_equals(&value) {
                continue;
            }
            self.configuration.update_value(key, value, &ConfigurationUpdateOverrides::default());
            changed_keys.push(JsString::from(key));
        }

        if !changed_keys.is_empty() {
            let event = ConfigurationChangeEvent::new(
                ConfigurationChange { keys: changed_keys, overrides: Vec::new() },
                Some(previous),
                self.configuration.clone(),
                None,
                self.log_service.clone(),
            );
            event.source.set(Some(ConfigurationTarget::Memory));
            self.on_did_change_configuration.fire(Rc::new(event));
        }

        Promise::resolved(())
    }
}

impl IConfigurationService for StandaloneConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.on_did_change_configuration.event()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        None
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        let section = section.map(JsString::from);
        self.configuration.get_value(section.as_ref(), &overrides.cloned().unwrap_or_default(), None)
    }

    fn update_value(&self, key: &str, value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, _target: Option<ConfigurationTarget>) -> Promise<()> {
        self.update_values(vec![(key, value)])
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        Rc::new(self.configuration.inspect(key, overrides, None))
    }

    fn reload_configuration(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn keys(&self) -> ConfigurationKeys {
        self.configuration.keys(None)
    }
}

/// O `StatusHandle` de `{ close: () => { } }`.
struct NoOpStatus;

impl StatusHandle for NoOpStatus {
    fn close(&self) {}
}

/// O texto de uma mensagem de notificação, como o `console` do original a imprimiria.
fn message_text(message: &NotificationMessage) -> String {
    match message {
        NotificationMessage::Text(text) => text.clone(),
        NotificationMessage::Error(error) => error.to_string(),
        other => format!("{other:?}"),
    }
}

/// `StandaloneNotificationService`.
pub struct StandaloneNotificationService {
    /// `NO_OP`.
    no_op: Rc<NoOpNotification>,
}

impl StandaloneNotificationService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { no_op: Rc::new(NoOpNotification::new()) })
    }

    /// `info`/`warn`/`error`: `this.notify({ severity, message })` para cada mensagem.
    fn notify_all(&self, severity: Severity, messages: Vec<NotificationMessage>) {
        for message in messages {
            self.notify(Notification::new(severity, message));
        }
    }
}

impl INotificationService for StandaloneNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        Event::none()
    }

    fn set_filter(&self, _filter: NotificationFilterSetting) {}

    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        NotificationsFilter::Off
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        Vec::new()
    }

    fn remove_filter(&self, _source_id: &str) {}

    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        let message = message_text(&notification.message);
        match notification.severity {
            Severity::Error | Severity::Warning => eprintln!("{message}"),
            _ => println!("{message}"),
        }

        self.no_op.clone()
    }

    fn info(&self, message: Vec<NotificationMessage>) {
        self.notify_all(Severity::Info, message);
    }

    fn warn(&self, message: Vec<NotificationMessage>) {
        self.notify_all(Severity::Warning, message);
    }

    fn error(&self, message: Vec<NotificationMessage>) {
        self.notify_all(Severity::Error, message);
    }

    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        self.no_op.clone()
    }

    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        Rc::new(NoOpStatus)
    }
}

/// `StandaloneCommandService`.
pub struct StandaloneCommandService {
    instantiation_service: Rc<InstantiationService>,
    on_will_execute_command: Rc<Emitter<CommandEvent>>,
    on_did_execute_command: Rc<Emitter<CommandEvent>>,
}

impl StandaloneCommandService {
    pub fn new(instantiation_service: Rc<InstantiationService>) -> Rc<Self> {
        Rc::new(Self { instantiation_service, on_will_execute_command: Emitter::new(), on_did_execute_command: Emitter::new() })
    }
}

impl ICommandService for StandaloneCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        self.on_will_execute_command.event()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        self.on_did_execute_command.event()
    }

    fn execute_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        invoke_registered_command(&self.instantiation_service, id, args, &self.on_will_execute_command, &self.on_did_execute_command)
    }
}

/// `IKeybindingRule`: o atalho na forma binária (`KeyMod | KeyCode`) com o comando e o `when`.
#[derive(Clone)]
pub struct StandaloneKeybindingRule {
    pub keybinding: u32,
    pub command: Option<String>,
    pub command_args: Dynamic,
    pub when: Option<ContextKeyExpression>,
}

/// `StandaloneKeybindingService`.
pub struct StandaloneKeybindingService {
    state: KeybindingServiceState,
    os: OperatingSystem,
    cached_resolver: RefCell<Option<Rc<KeybindingResolver>>>,
    dynamic_keybindings: RefCell<Vec<Rc<KeybindingItem>>>,
}

impl StandaloneKeybindingService {
    pub fn new(
        context_key_service: Rc<dyn IContextKeyService>,
        command_service: Rc<dyn ICommandService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        notification_service: Rc<dyn INotificationService>,
        log_service: LogServiceRef,
        document_has_focus: Rc<dyn Fn() -> bool>,
    ) -> Rc<Self> {
        Rc::new(Self {
            state: KeybindingServiceState::new(context_key_service, command_service, telemetry_service, notification_service, log_service, document_has_focus),
            os: Platform::detect().os,
            cached_resolver: RefCell::new(None),
            dynamic_keybindings: RefCell::new(Vec::new()),
        })
    }

    /// `addDynamicKeybinding(command, keybinding, handler, when)`: registra o comando e o atalho dele.
    pub fn add_dynamic_keybinding(self: &Rc<Self>, command: &str, keybinding: u32, handler: CommandHandler, when: Option<ContextKeyExpression>) -> DisposableRef {
        combined_disposable(vec![
            commands_registry().register_command_handler(command, Some(handler)),
            self.add_dynamic_keybindings(vec![StandaloneKeybindingRule { keybinding, command: Some(command.to_owned()), command_args: Dynamic::Undefined, when }]),
        ])
    }

    /// `addDynamicKeybindings(rules)`.
    pub fn add_dynamic_keybindings(self: &Rc<Self>, rules: Vec<StandaloneKeybindingRule>) -> DisposableRef {
        let entries: Vec<Rc<KeybindingItem>> = rules
            .into_iter()
            .map(|rule| {
                let keybinding = match decode_keybinding(rule.keybinding, self.os) {
                    Ok(keybinding) => keybinding,
                    Err(error) => errors::throw(error),
                };
                Rc::new(KeybindingItem {
                    keybinding,
                    command: rule.command,
                    command_args: rule.command_args,
                    when: rule.when,
                    weight1: 1000,
                    weight2: 0,
                    extension_id: None,
                    is_builtin_extension: false,
                })
            })
            .collect();
        self.dynamic_keybindings.borrow_mut().extend(entries.iter().cloned());

        self.update_resolver();

        let this = Rc::downgrade(self);
        to_disposable(move || {
            let Some(this) = this.upgrade() else { return };
            let Some(first) = entries.first() else { return };
            // Search the first entry and remove them all since they will be contiguous
            let index = this.dynamic_keybindings.borrow().iter().position(|item| Rc::ptr_eq(item, first));
            if let Some(index) = index {
                let mut dynamic = this.dynamic_keybindings.borrow_mut();
                dynamic.drain(index..index + entries.len()).for_each(drop);
                drop(dynamic);
                this.update_resolver();
            }
        })
    }

    fn update_resolver(&self) {
        *self.cached_resolver.borrow_mut() = None;
        self.state.on_did_update_keybindings_emitter().fire(());
    }

    /// `_toNormalizedKeybindingItems`.
    fn to_normalized_keybinding_items<'a>(&self, items: impl IntoIterator<Item = &'a KeybindingItem>, is_default: bool) -> Vec<Rc<ResolvedKeybindingItem>> {
        let mut result = Vec::new();
        for item in items {
            let when = item.when.clone();
            let command = item.command.as_deref();

            match &item.keybinding {
                None => {
                    // This might be a removal keybinding item in user settings => accept it
                    result.push(ResolvedKeybindingItem::new(None, command, item.command_args.clone(), when, is_default, None, false, false));
                }
                Some(keybinding) => {
                    for resolved_keybinding in USLayoutResolvedKeybinding::resolve_keybinding(keybinding, self.os) {
                        result.push(ResolvedKeybindingItem::new(Some(resolved_keybinding), command, item.command_args.clone(), when.clone(), is_default, None, false, false));
                    }
                }
            }
        }

        result
    }
}

impl IDisposable for StandaloneKeybindingService {
    fn dispose(&self) {
        self.state.dispose();
    }
}

impl KeybindingServiceHost for StandaloneKeybindingService {
    fn state(&self) -> &KeybindingServiceState {
        &self.state
    }

    fn get_resolver(&self) -> Rc<KeybindingResolver> {
        if let Some(resolver) = self.cached_resolver.borrow().as_ref() {
            return resolver.clone();
        }
        let defaults = self.to_normalized_keybinding_items(&keybindings_registry().get_default_keybindings(), true);
        let overrides = self.to_normalized_keybinding_items(self.dynamic_keybindings.borrow().iter().map(|item| &**item), false);
        let resolver = Rc::new(KeybindingResolver::new(defaults, overrides, self.state.logger()));
        *self.cached_resolver.borrow_mut() = Some(resolver.clone());
        resolver
    }

    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        USLayoutResolvedKeybinding::resolve_keybinding(keybinding, self.os)
    }

    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        let chord = KeyCodeChord::new(
            keyboard_event.ctrl_key,
            keyboard_event.shift_key,
            keyboard_event.alt_key,
            keyboard_event.meta_key,
            keyboard_event.key_code,
        );
        Rc::new(USLayoutResolvedKeybinding::from_chords(vec![chord], self.os))
    }

    fn resolve_user_binding(&self, _user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }

    fn register_schema_contribution(&self, _contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef {
        none()
    }

    fn dump_debug_info(&self) -> String {
        String::new()
    }

    fn dump_debug_info_json(&self) -> String {
        String::new()
    }

    /// not yet supported
    fn enable_keybinding_hold_mode(&self, _command_id: &str) -> Option<Promise<()>> {
        None
    }
}
