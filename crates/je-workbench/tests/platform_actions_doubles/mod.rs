//! Dublês dos serviços que os testes de `src/vs/platform/actions/test/browser` montam com `mock<...>()` e
//! `NullTelemetryServiceShape`: o `TestTelemetryService` que guarda os eventos, serviços que nunca devem ser chamados
//! (`mock<IMenuService>()`, `mock<ICommandService>()`) e o `IContextMenuService` que não mostra nada.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_platform::actions::{IMenu, IMenuService, MenuActionGroup, MenuActionOptions, MenuCreateOptions, MenuId};
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{ContextMenuRequest, IContextMenuService};
use je_platform::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, NullTelemetryServiceShape, TelemetryLevel};

/// Um evento de `publicLog2`: `{ name, data }`.
#[derive(Clone, Debug)]
pub struct TelemetryEvent {
    pub name: String,
    pub data: Option<Dynamic>,
}

/// O `assert.deepStrictEqual` do original compara os eventos pela estrutura, não pela identidade do objeto `data`.
impl PartialEq for TelemetryEvent {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && match (&self.data, &other.data) {
                (Some(a), Some(b)) => je_base::equals::structural_equals(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl TelemetryEvent {
    /// O `{ name: 'workbenchActionExecuted', data: { id, from } }` que o toolbar e a barra de botões publicam.
    pub fn workbench_action_executed(id: &str, from: &str) -> Self {
        Self { name: "workbenchActionExecuted".to_owned(), data: Some(Dynamic::object([("id", Dynamic::from(id)), ("from", Dynamic::from(from))])) }
    }
}

/// `class TestTelemetryService extends NullTelemetryServiceShape`: `publicLog2` guarda o evento (se tem nome).
#[derive(Default)]
pub struct TestTelemetryService {
    inner: NullTelemetryServiceShape,
    pub events: RefCell<Vec<TelemetryEvent>>,
}

impl ITelemetryService for TestTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        self.inner.telemetry_level()
    }
    fn session_id(&self) -> String {
        self.inner.session_id()
    }
    fn machine_id(&self) -> String {
        self.inner.machine_id()
    }
    fn sqm_id(&self) -> String {
        self.inner.sqm_id()
    }
    fn dev_device_id(&self) -> String {
        self.inner.dev_device_id()
    }
    fn first_session_date(&self) -> String {
        self.inner.first_session_date()
    }
    fn send_error_telemetry(&self) -> bool {
        self.inner.send_error_telemetry()
    }
    fn public_log(&self, event_name: &str, data: Option<ITelemetryData>) {
        self.inner.public_log(event_name, data);
    }
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        if !event_name.is_empty() {
            self.events.borrow_mut().push(TelemetryEvent { name: event_name.to_owned(), data });
        }
    }
    fn public_log_error(&self, error_event_name: &str, data: Option<ITelemetryData>) {
        self.inner.public_log_error(error_event_name, data);
    }
    fn public_log_error2(&self, event_name: &str, data: Option<Dynamic>) {
        self.inner.public_log_error2(event_name, data);
    }
    fn set_experiment_property(&self, name: &str, value: &str) {
        self.inner.set_experiment_property(name, value);
    }
    fn set_common_property(&self, name: &str, value: Option<CommonPropertyValue>) {
        self.inner.set_common_property(name, value);
    }
}

/// `new class extends mock<IMenuService>() { }()`: qualquer chamada é um `TypeError`.
pub struct UnusedMenuService;

impl IMenuService for UnusedMenuService {
    fn create_menu(&self, _id: &MenuId, _context_key_service: Rc<dyn IContextKeyService>, _options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        unreachable!("TypeError: createMenu is not a function")
    }
    fn get_menu_actions(&self, _id: &MenuId, _context_key_service: &dyn IContextKeyService, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        unreachable!("TypeError: getMenuActions is not a function")
    }
    fn get_menu_contexts(&self, _id: &MenuId) -> HashSet<String> {
        unreachable!("TypeError: getMenuContexts is not a function")
    }
    fn reset_hidden_states(&self, _menu_ids: Option<&[MenuId]>) {
        unreachable!("TypeError: resetHiddenStates is not a function")
    }
}

/// `new class extends mock<ICommandService>() { }()`: qualquer execução é um `TypeError`.
pub struct UnusedCommandService;

impl ICommandService for UnusedCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, _command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        unreachable!("TypeError: executeCommand is not a function")
    }
}

/// `override showContextMenu(): void { }`.
pub struct NoopContextMenuService;

impl IContextMenuService for NoopContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_hide_context_menu(&self) -> Event<()> {
        Event::none()
    }
    fn show_context_menu(&self, _request: ContextMenuRequest) {}
}
