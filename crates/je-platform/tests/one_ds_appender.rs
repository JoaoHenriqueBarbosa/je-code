//! Porte de `src/vs/platform/telemetry/test/browser/1dsAppender.test.ts` (suíte `AIAdapter`), caso a caso e na
//! mesma ordem.
//!
//! Divergências:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso, e o `setup` e o
//!   `teardown` (`adapter.flush()`) são [`setup`] e o `flush` no fim de cada caso.
//! - O `OneDataSystemWebAppender` do original dispara um `fetch` de sonda de saúde na construção; aqui o cliente
//!   HTTP é injetado ([`HealthyHttpClient`], que responde que o endpoint está no ar), então o teste não toca a rede.
//! - O `AppInsightsCoreMock` guarda o `baseData` de cada evento, como o original; `new Date()` é a data do relógio
//!   do `je_base::runtime`.
//! - `Object.create(null)` é um objeto dinâmico vazio, e `assert.strictEqual` sobre números e textos dinâmicos é
//!   `assert_deep_strict_equal`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use je_base::dynamic::{Dynamic, date_to_iso_string};
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::telemetry::one_ds_appender::{
    IAppInsightsCore, IOneDsHttpClient, OneDataSystemWebAppender, OneDsClientSource, TelemetryItem, TelemetryUnloadState, apply_envelope_defaults,
};
use je_platform::telemetry::ITelemetryAppender;

/// `AppInsightsCoreMock`.
struct AppInsightsCoreMock {
    plugin_version_string: RefCell<String>,
    events: RefCell<Vec<Dynamic>>,
}

impl IAppInsightsCore for AppInsightsCoreMock {
    fn plugin_version_string(&self) -> String {
        self.plugin_version_string.borrow().clone()
    }

    fn set_plugin_version_string(&self, value: &str) {
        *self.plugin_version_string.borrow_mut() = value.to_owned();
    }

    fn track(&self, item: TelemetryItem) {
        self.events.borrow_mut().push(item.base_data);
    }

    fn unload(&self, _is_async: bool, _unload_complete: Box<dyn FnOnce(&TelemetryUnloadState)>) {
        // No-op
    }
}

/// O `fetch` da sonda de saúde: o endpoint responde.
struct HealthyHttpClient;

impl IOneDsHttpClient for HealthyHttpClient {
    fn get(&self, _url: &str) -> Promise<()> {
        Promise::resolved(())
    }
}

const PREFIX: &str = "prefix";

/// O `adapter` de um dado padrão sobre o cliente de teste.
fn adapter_for(mock: &Rc<AppInsightsCoreMock>, default_data: Option<Dynamic>) -> OneDataSystemWebAppender {
    let client: Rc<dyn IAppInsightsCore> = mock.clone();
    OneDataSystemWebAppender::new(false, PREFIX, default_data, OneDsClientSource::Factory(Box::new(move || client)), &HealthyHttpClient)
}

/// O `setup`.
fn setup() -> (LeakGuard, Rc<AppInsightsCoreMock>, OneDataSystemWebAppender) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mock = Rc::new(AppInsightsCoreMock { plugin_version_string: RefCell::new("Test Runner".to_owned()), events: RefCell::new(Vec::new()) });
    let adapter = adapter_for(&mock, None);
    (guard, mock, adapter)
}

fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("o valor não é texto: {other:?}"),
    }
}

/// `events[index].properties[name]`.
fn property(mock: &AppInsightsCoreMock, index: usize, name: &str) -> Dynamic {
    mock.events.borrow()[index].get_str("properties").get_str(name)
}

/// `events[index].measurements[name]`.
fn measurement(mock: &AppInsightsCoreMock, index: usize, name: &str) -> Dynamic {
    mock.events.borrow()[index].get_str("measurements").get_str(name)
}

fn event_name(mock: &AppInsightsCoreMock, index: usize) -> String {
    text_of(&mock.events.borrow()[index].get_str("name"))
}

#[test]
fn simple_event() {
    let (_guard, mock, adapter) = setup();
    adapter.log("testEvent", &Dynamic::Undefined);

    assert_eq!(mock.events.borrow().len(), 1);
    assert_eq!(event_name(&mock, 0), format!("{PREFIX}/testEvent"));
    let _ = adapter.flush();
}

#[test]
fn addional_data() {
    let (_guard, mock, _adapter) = setup();
    let adapter = adapter_for(&mock, Some(obj(vec![("first", s("1st")), ("second", n(2.0)), ("third", Dynamic::Bool(true))])));
    adapter.log("testEvent", &Dynamic::Undefined);

    assert_eq!(mock.events.borrow().len(), 1);
    assert_eq!(event_name(&mock, 0), format!("{PREFIX}/testEvent"));
    assert_deep_strict_equal(&property(&mock, 0, "first"), &s("1st"));
    assert_deep_strict_equal(&measurement(&mock, 0, "second"), &n(2.0));
    assert_deep_strict_equal(&measurement(&mock, 0, "third"), &n(1.0));
    let _ = adapter.flush();
}

#[test]
fn property_limits() {
    let (_guard, mock, adapter) = setup();
    let mut really_long_property_name = String::from("abcdefghijklmnopqrstuvwxyz");
    for _ in 0..6 {
        really_long_property_name.push_str("abcdefghijklmnopqrstuvwxyz");
    }
    assert!(really_long_property_name.len() > 150);

    let mut really_long_property_value = String::from("abcdefghijklmnopqrstuvwxyz012345678901234567890123");
    for _ in 0..400 {
        really_long_property_value.push_str("abcdefghijklmnopqrstuvwxyz012345678901234567890123");
    }
    assert!(really_long_property_value.len() > 8192);

    let data = obj(vec![(really_long_property_name.as_str(), s("1234")), ("reallyLongPropertyValue", s(&really_long_property_value))]);
    adapter.log("testEvent", &data);

    assert_eq!(mock.events.borrow().len(), 1);

    let properties = mock.events.borrow()[0].get_str("properties");
    for (prop, value) in properties.own_entries() {
        assert!(prop.len() < 150);
        let Dynamic::String(value) = value else { panic!("a propriedade `{}` não é texto", prop.to_string_lossy()) };
        assert!(value.len() < 8192);
    }
    let _ = adapter.flush();
}

#[test]
fn different_data_types() {
    let (_guard, mock, adapter) = setup();
    let date = runtime::date_now() as f64;
    adapter.log(
        "testEvent",
        &obj(vec![
            ("favoriteDate", Dynamic::date(date)),
            ("likeRed", Dynamic::Bool(false)),
            ("likeBlue", Dynamic::Bool(true)),
            ("favoriteNumber", n(1.0)),
            ("favoriteColor", s("blue")),
            ("favoriteCars", arr(vec![s("bmw"), s("audi"), s("ford")])),
        ]),
    );

    assert_eq!(mock.events.borrow().len(), 1);
    assert_eq!(event_name(&mock, 0), format!("{PREFIX}/testEvent"));
    assert_deep_strict_equal(&property(&mock, 0, "favoriteColor"), &s("blue"));
    assert_deep_strict_equal(&measurement(&mock, 0, "likeRed"), &n(0.0));
    assert_deep_strict_equal(&measurement(&mock, 0, "likeBlue"), &n(1.0));
    assert_deep_strict_equal(&property(&mock, 0, "favoriteDate"), &s(&date_to_iso_string(date).expect("data válida")));
    assert_deep_strict_equal(&property(&mock, 0, "favoriteCars"), &s(r#"["bmw","audi","ford"]"#));
    assert_deep_strict_equal(&measurement(&mock, 0, "favoriteNumber"), &n(1.0));
    let _ = adapter.flush();
}

#[test]
fn nested_data() {
    let (_guard, mock, adapter) = setup();
    adapter.log(
        "testEvent",
        &obj(vec![
            ("window", obj(vec![("title", s("some title")), ("measurements", obj(vec![("width", n(100.0)), ("height", n(200.0))]))])),
            (
                "nestedObj",
                obj(vec![("nestedObj2", obj(vec![("nestedObj3", obj(vec![("testProperty", s("test"))]))])), ("testMeasurement", n(1.0))]),
            ),
        ]),
    );

    assert_eq!(mock.events.borrow().len(), 1);
    assert_eq!(event_name(&mock, 0), format!("{PREFIX}/testEvent"));

    assert_deep_strict_equal(&property(&mock, 0, "window.title"), &s("some title"));
    assert_deep_strict_equal(&measurement(&mock, 0, "window.measurements.width"), &n(100.0));
    assert_deep_strict_equal(&measurement(&mock, 0, "window.measurements.height"), &n(200.0));

    assert_deep_strict_equal(&property(&mock, 0, "nestedObj.nestedObj2.nestedObj3"), &s(r#"{"testProperty":"test"}"#));
    assert_deep_strict_equal(&measurement(&mock, 0, "nestedObj.testMeasurement"), &n(1.0));
    let _ = adapter.flush();
}

#[test]
fn routes_an_event_internally_when_the_machine_or_the_account_is_internal() {
    let (_guard, mock, adapter) = setup();
    // `envelope.ext?.utc?.flags`
    let flags_for = |is_internal_machine: bool, data: Option<Dynamic>| -> Option<f64> {
        adapter.log("testEvent", &data.unwrap_or(Dynamic::Undefined));
        let base_data = mock.events.borrow_mut().pop().expect("o evento foi registrado");
        let mut envelope = TelemetryItem { name: "testEvent".to_owned(), base_data, ext: Dynamic::Undefined };
        apply_envelope_defaults(&mut envelope, Some(is_internal_machine));
        match envelope.ext.get_str("utc") {
            Dynamic::Object(_) => match envelope.ext.get_str("utc").get_str("flags") {
                Dynamic::Number(flags) => Some(flags),
                _ => None,
            },
            _ => None,
        }
    };

    let internal = Some(f64::from(0x0000811ECD_u32));
    assert_eq!(
        (
            flags_for(false, None),
            flags_for(true, None),
            flags_for(false, Some(obj(vec![("common.msftInternal", Dynamic::Bool(true))]))),
            flags_for(false, Some(obj(vec![("common.msftInternal", Dynamic::Bool(false))]))),
        ),
        (None, internal, internal, None)
    );
    let _ = adapter.flush();
}
