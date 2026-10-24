//! Porte de `src/vs/workbench/test/common/testNotificationTelemetry.ts`: o serviço de telemetria que guarda os
//! eventos que as notificações publicam.
//!
//! Divergências:
//! - `extends NullTelemetryServiceShape` é composição: o resto do contrato é delegado ao [`NullTelemetryServiceShape`].
//! - `events` guarda `{ eventName, data }` como um par `(String, Dynamic)`; o `data ?? {}` do original é o objeto
//!   vazio quando o evento chega sem dados; `shown` e `interactions` devolvem só os dados, na ordem de chegada.

use std::cell::RefCell;

use je_base::dynamic::Dynamic;
use je_platform::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, NullTelemetryServiceShape, TelemetryLevel};

/// `TestNotificationTelemetryService`.
#[derive(Default)]
pub struct TestNotificationTelemetryService {
    null: NullTelemetryServiceShape,
    events: RefCell<Vec<(String, Dynamic)>>,
}

impl TestNotificationTelemetryService {
    pub fn new() -> Self {
        Self::default()
    }

    /// `events`: o nome e os dados de cada evento publicado.
    pub fn events(&self) -> Vec<(String, Dynamic)> {
        self.events.borrow().clone()
    }

    fn data_of(&self, event_name: &str) -> Vec<Dynamic> {
        self.events.borrow().iter().filter(|(name, _)| name == event_name).map(|(_, data)| data.clone()).collect()
    }

    /// `shown`: os dados dos eventos `notificationShown`.
    pub fn shown(&self) -> Vec<Dynamic> {
        self.data_of("notificationShown")
    }

    /// `interactions`: os dados dos eventos `notificationInteraction`.
    pub fn interactions(&self) -> Vec<Dynamic> {
        self.data_of("notificationInteraction")
    }
}

impl ITelemetryService for TestNotificationTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        self.null.telemetry_level()
    }
    fn session_id(&self) -> String {
        self.null.session_id()
    }
    fn machine_id(&self) -> String {
        self.null.machine_id()
    }
    fn sqm_id(&self) -> String {
        self.null.sqm_id()
    }
    fn dev_device_id(&self) -> String {
        self.null.dev_device_id()
    }
    fn first_session_date(&self) -> String {
        self.null.first_session_date()
    }
    fn send_error_telemetry(&self) -> bool {
        self.null.send_error_telemetry()
    }
    fn public_log(&self, event_name: &str, data: Option<ITelemetryData>) {
        self.null.public_log(event_name, data);
    }
    /// `assert.ok(eventName)` e o registro do evento.
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        assert!(!event_name.is_empty(), "o evento de telemetria precisa de um nome");
        self.events.borrow_mut().push((event_name.to_owned(), data.unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new()))));
    }
    fn public_log_error(&self, error_event_name: &str, data: Option<ITelemetryData>) {
        self.null.public_log_error(error_event_name, data);
    }
    fn public_log_error2(&self, event_name: &str, data: Option<Dynamic>) {
        self.null.public_log_error2(event_name, data);
    }
    fn set_experiment_property(&self, name: &str, value: &str) {
        self.null.set_experiment_property(name, value);
    }
    fn set_common_property(&self, name: &str, value: Option<CommonPropertyValue>) {
        self.null.set_common_property(name, value);
    }
}
