//! Porte de `src/vs/workbench/contrib/debug/test/node/debugger.test.ts` (suíte `Debug - Debugger`), caso a caso e na mesma ordem.
//!
//! NÃO foi portado (sem `#[test]`): `merge platform specific attributes`. O original o pula fora do Electron
//! (`if (!process.versions.electron) this.skip()`, com o comentário "this test fails when run in node.js environments"); o
//! porte não roda no Electron, então o caso nunca executaria. A lógica que ele cobre (`platformAdapterExecutable` com
//! `runtime`/`runtimeArgs`/`program`/`args` por plataforma) está em `debug_adapter.rs`.
//!
//! Os 3 casos portados: `attributes`, `initial config file content`, `getSchemaAttributes logs malformed properties and ignores them`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso; o `setup` cria o `Debugger` e o `teardown`
//!   o solta.
//! - `TestTextResourcePropertiesService.getEOL` (com `files.eol` ausente) é `\r\n` no Windows e `\n` nos demais.
//! - A `TestConfigurationService()` sem configuração devolve `{}` para `getValue()`.
//! - `new class extends NullLogService { override warn }` é um `LogService` que guarda a última mensagem de `warn`.
//! - `IExtensionDescription` é o `ExtensionDescription` com o manifesto cujo `contributes.debuggers` leva a contribuição.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::Platform;
use je_base::uri::URI;
use je_extensions::manifest::{DebuggerContribution, ExtensionContributions, ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogArg, LogLevel, LogMessage, Logger, NullLogService};
use je_platform::resources::uri_file;
use je_workbench::contrib::debug::debug_adapter::ExecutableDebugAdapter;
use je_workbench::contrib::debug::debugger::{Debugger, DebuggerServices};
use serde_json::{Map, Value, json};

const EXTENSION_FOLDER_PATH: &str = "/a/b/c/";

fn debugger_contribution() -> Value {
    json!({
        "type": "mock",
        "label": "Mock Debug",
        "program": "./out/mock/mockDebug.js",
        "args": ["arg1", "arg2"],
        "configurationAttributes": {
            "launch": {
                "required": ["program"],
                "properties": {
                    "program": {
                        "type": "string",
                        "description": "Workspace relative path to a text file.",
                        "default": "readme.md"
                    }
                }
            }
        },
        "variables": null,
        "initialConfigurations": [
            { "name": "Mock-Debug", "type": "mock", "request": "launch", "program": "readme.md" }
        ]
    })
}

fn extension_descriptor0() -> Rc<ExtensionDescription> {
    let contribution: DebuggerContribution = serde_json::from_value(debugger_contribution()).unwrap();
    Rc::new(ExtensionDescription {
        id: "adapter".to_owned(),
        identifier: ExtensionIdentifier::new("adapter"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: uri_file(EXTENSION_FOLDER_PATH).to_string(false).to_string_lossy(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: "myAdapter".to_owned(),
            version: "1.0.0".to_owned(),
            publisher: "vscode".to_owned(),
            contributes: Some(ExtensionContributions { debuggers: Some(vec![contribution]), ..ExtensionContributions::default() }),
            ..ExtensionManifest::default()
        },
    })
}

fn eol() -> String {
    if Platform::detect().is_windows { "\r\n" } else { "\n" }.to_owned()
}

fn services() -> DebuggerServices {
    DebuggerServices { configuration: Some(Rc::new(|| json!({}))), eol: Some(Rc::new(eol)), ..DebuggerServices::default() }
}

/// O `setup`.
fn setup() -> Debugger {
    Debugger::new(&debugger_contribution(), extension_descriptor0(), services())
}

#[test]
fn attributes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let debugger = setup();
    assert_eq!(debugger.debug_type(), "mock");
    assert_eq!(debugger.label(), "Mock Debug");

    let ae = ExecutableDebugAdapter::platform_adapter_executable(&[(*extension_descriptor0()).clone()], "mock");

    let ae = ae.unwrap();
    assert_eq!(ae.command, PathModule::native(&ProcessEnvironment).join(&[EXTENSION_FOLDER_PATH, "./out/mock/mockDebug.js"]));
    assert_eq!(ae.args, vec!["arg1".to_owned(), "arg2".to_owned()]);
}

#[test]
fn initial_config_file_content() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let debugger = setup();

    let _ = URI::parse("file:///somefile", false);
    let expected = [
        "{",
        "\t// Use IntelliSense to learn about possible attributes.",
        "\t// Hover to view descriptions of existing attributes.",
        "\t// For more information, visit: https://go.microsoft.com/fwlink/?linkid=830387",
        "\t\"version\": \"0.2.0\",",
        "\t\"configurations\": [",
        "\t\t{",
        "\t\t\t\"name\": \"Mock-Debug\",",
        "\t\t\t\"type\": \"mock\",",
        "\t\t\t\"request\": \"launch\",",
        "\t\t\t\"program\": \"readme.md\"",
        "\t\t}",
        "\t]",
        "}",
    ]
    .join(&eol());

    let content = debugger.get_initial_configuration_content(None).result().expect("já resolvida").expect("sem erro");
    assert_eq!(content, expected);
}

/// `new class extends NullLogService { override warn(message) { warningMessage = message; } }`.
struct WarnRecorder {
    warning_message: Rc<RefCell<Option<String>>>,
    inner: NullLogService,
}

impl IDisposable for WarnRecorder {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl Logger for WarnRecorder {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.inner.on_did_change_log_level()
    }

    fn get_level(&self) -> LogLevel {
        self.inner.get_level()
    }

    fn set_level(&self, level: LogLevel) {
        self.inner.set_level(level);
    }

    fn trace_args(&self, message: &str, args: &[LogArg]) {
        self.inner.trace_args(message, args);
    }

    fn debug_args(&self, message: &str, args: &[LogArg]) {
        self.inner.debug_args(message, args);
    }

    fn info_args(&self, message: &str, args: &[LogArg]) {
        self.inner.info_args(message, args);
    }

    fn warn_args(&self, message: &LogMessage, _args: &[LogArg]) {
        if let LogMessage::Text(text) = message {
            *self.warning_message.borrow_mut() = Some(text.clone());
        }
    }

    fn error_args(&self, message: &LogMessage, args: &[LogArg]) {
        self.inner.error_args(message, args);
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

#[test]
fn get_schema_attributes_logs_malformed_properties_and_ignores_them() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _debugger = setup();
    let warning_message: Rc<RefCell<Option<String>>> = Rc::default();
    let log_service: Rc<dyn je_platform::log::LogService> = Rc::new(WarnRecorder { warning_message: warning_message.clone(), inner: NullLogService });

    let mut malformed_contribution = debugger_contribution();
    malformed_contribution["configurationAttributes"] = json!({ "launch": { "properties": { "valid": { "type": "string" }, "malformed": "integer" } } });
    let malformed_debugger = Debugger::new(&malformed_contribution, extension_descriptor0(), DebuggerServices { log_service: Some(log_service), ..services() });
    let mut definitions: Map<String, Value> = Map::new();
    definitions.insert("common".to_owned(), json!({ "properties": {} }));

    let attributes = malformed_debugger.get_schema_attributes(&mut definitions);

    let attributes = attributes.expect("há atributos");
    assert_eq!(attributes[0]["properties"]["valid"]["type"], json!("string"));
    assert_eq!(attributes[0]["properties"]["malformed"], json!("integer"));
    let warning = warning_message.borrow().clone().expect("o aviso");
    assert_eq!(warning, "Ignoring malformed debug configuration schema properties for type 'mock': malformed");
}
