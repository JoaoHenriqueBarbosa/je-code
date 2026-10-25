//! `vscode.env`: `appName`, `appRoot`, `language`, `machineId`, `sessionId`, `uriScheme`, `shell`, `uiKind`, `remoteName`, `clipboard`
//! (`readText` e `writeText`), `openExternal`, `asExternalUri`, `isTelemetryEnabled`, `onDidChangeTelemetryEnabled`,
//! `createTelemetryLogger`, `logLevel` e `onDidChangeLogLevel` (`extHost.api.impl.ts`: o literal `env`).
//!
//! Cada grupo só existe quando o dono do host o entrega: os valores estáticos vêm do [`HostEnvironment`]
//! ([`IMainThreadBinder::host_environment`](crate::in_process::IMainThreadBinder::host_environment)), `clipboard` do
//! `IClipboardService` do workbench
//! ([`IMainThreadBinder::clipboard_service`](crate::in_process::IMainThreadBinder::clipboard_service)), `openExternal` e `asExternalUri`
//! do `ExtHostWindow` (que precisa do cliente `MainThreadWindow` registrado pelo binder), os membros de telemetria do
//! `ExtHostTelemetry` ([`IMainThreadBinder::telemetry`](crate::in_process::IMainThreadBinder::telemetry)) e `logLevel` do serviço de log
//! do host (presente quando o host tem binder; sozinho ele não cria `env`).
//!
//! Falta: `isAppPortable`, `appHost`, `isNewAppInstall`, `onDidChangeShell` e as propostas.
//!
//! Divergências:
//! - O original lê os valores estáticos do `initData` (`environment.appName`, `appRoot`, `appLanguage`, `appUriScheme`,
//!   `telemetryInfo.machineId` e `sessionId`, `remote.authority`) e o `shell` do `ExtHostTerminalService`; o host em processo não tem
//!   `initData`, então o dono do host entrega todos num [`HostEnvironment`]. `uiKind` entra no mesmo struct.
//! - `env.clipboard` não passa pelo par `ExtHostClipboard`/`MainThreadClipboard` (`extHostClipboard.ts` e `mainThreadClipboard.ts`,
//!   que só repassam `$readText` e `$writeText` ao `IClipboardService`, e ainda não estão portados): as duas funções falam com o
//!   `IClipboardService` do workbench diretamente.
//! - `openExternal` ignora a opção `allowContributedOpeners` e manda `allowTunneling: false` (o host é local, `initData.remote.isRemote`
//!   do original vale `false` e não há autoridade remota).
//! - `asExternalUri` não trata o esquema do aplicativo (`ExtHostUrls.createAppUri` não está portado): ele também passa pelo
//!   `MainThreadWindow`. O resto é o do original: `allowTunneling` vale `!!remote.authority`, e uma rejeição de `http` ou `https`
//!   devolve o URI de entrada.
//! - O objeto `env` é uma instância de classe nativa (`logLevel` e `isTelemetryEnabled` são lidos no instante da leitura) com os
//!   valores estáticos como propriedades próprias: `env.clipboard` devolve as mesmas duas funções, mas `vscode.env.clipboard ===
//!   vscode.env.clipboard` falha.
//! - `createTelemetryLogger` usa a extensão do host e o `sender` chega como cópia (ver [`crate::api::telemetry_logger`]).

use std::rc::Rc;

use je_base::network::{matches_scheme, UriOrString};
use je_base::js_string::JsString;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::remote::remote_hosts::get_remote_name;
use je_workbench::api::common::ext_host_protocol::IOpenUriOptions;
use je_workbench::api::common::ext_host_telemetry::ExtHostTelemetry;
use je_workbench::api::common::ext_host_window::{ExtHostWindow, UriOrString as WindowUriOrString};
use zjsc::api::embed_node::{JsData, MethodCallback, NodeRunner};

use crate::api::accessor_namespace;
use crate::api::commands::{promise_outcome_to_js, promise_to_js};
use crate::api::disposable::DisposableTable;
use crate::api::enums::log_level_value;
use crate::api::events::event_function;
use crate::api::telemetry_logger::{self, TelemetryLoggers};
use crate::convert::{error_to_js, string_arg, uri_from_js, uri_to_js};

/// Os valores de ambiente que o original lê do `initData`, entregues pelo dono do host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostEnvironment {
    /// `initData.environment.appName`.
    pub app_name: String,
    /// `initData.environment.appRoot.fsPath` (vazio se não há).
    pub app_root: String,
    /// `initData.environment.appLanguage`.
    pub app_language: String,
    /// `initData.environment.appUriScheme`.
    pub app_uri_scheme: String,
    /// `initData.telemetryInfo.machineId`.
    pub machine_id: String,
    /// `initData.telemetryInfo.sessionId`.
    pub session_id: String,
    /// `ExtHostTerminalService.getDefaultShell(false)`.
    pub shell: String,
    /// `initData.uiKind`: `1` é `UIKind.Desktop` e `2` é `UIKind.Web`.
    pub ui_kind: i32,
    /// `initData.remote.authority` (`None` num host local).
    pub remote_authority: Option<String>,
}

/// Os serviços do host que o namespace usa.
#[derive(Default)]
pub struct EnvServices {
    pub environment: Option<HostEnvironment>,
    pub clipboard: Option<Rc<dyn IClipboardService>>,
    pub window: Option<Rc<ExtHostWindow>>,
    pub telemetry: Option<Rc<ExtHostTelemetry>>,
    pub log_service: Option<LogServiceRef>,
}

impl EnvServices {
    /// Se há ao menos um membro para expor (o serviço de log sozinho não basta).
    pub fn is_any(&self) -> bool {
        self.environment.is_some() || self.clipboard.is_some() || self.window.is_some() || self.telemetry.is_some()
    }
}

/// `asExternalUri(uri)`: pelo `MainThreadWindow`; uma rejeição de `http` ou `https` devolve o próprio URI.
fn as_external_uri(runner: &NodeRunner, window: &Rc<ExtHostWindow>, authority: bool, args: &[JsData]) -> Result<JsData, String> {
    let uri = uri_from_js(args.first().unwrap_or(&JsData::Undefined))?;
    let options = IOpenUriOptions { allow_tunneling: Some(authority), ..IOpenUriOptions::default() };
    let fallback = uri.clone();
    Ok(promise_outcome_to_js(runner, window.as_external_uri(&uri, options), move |outcome| match outcome {
        Ok(external) => Ok(uri_to_js(external)),
        Err(_) if matches_scheme(UriOrString::Uri(&fallback), "http") || matches_scheme(UriOrString::Uri(&fallback), "https") => Ok(uri_to_js(&fallback)),
        Err(error) => Err(error_to_js(error)),
    }))
}

/// O namespace `env`.
pub fn namespace(runner: &NodeRunner, services: &EnvServices, table: &Rc<DisposableTable>) -> JsData {
    let mut members: Vec<(String, JsData)> = Vec::new();
    let mut methods: Vec<(String, u32, MethodCallback)> = Vec::new();
    let mut getters: Vec<(String, MethodCallback)> = Vec::new();
    let mut authority = false;
    if let Some(environment) = &services.environment {
        let text = |name: &str, value: &str| (name.to_owned(), JsData::Str(value.to_owned()));
        members.extend([
            text("machineId", &environment.machine_id),
            text("sessionId", &environment.session_id),
            text("language", &environment.app_language),
            text("appName", &environment.app_name),
            text("appRoot", &environment.app_root),
            text("uriScheme", &environment.app_uri_scheme),
            text("shell", &environment.shell),
            ("uiKind".to_owned(), JsData::Num(f64::from(environment.ui_kind))),
        ]);
        let remote_authority = environment.remote_authority.as_deref().map(JsString::from);
        authority = remote_authority.as_ref().is_some_and(|value| !value.is_empty());
        members.push((
            "remoteName".to_owned(),
            get_remote_name(remote_authority.as_ref()).map_or(JsData::Undefined, |name| JsData::Str(name.to_string_lossy())),
        ));
    }
    if let Some(clipboard) = &services.clipboard {
        let (read_runner, read_clipboard) = (runner.clone(), clipboard.clone());
        let read_text = runner.make_function(
            "readText",
            0,
            Rc::new(move |_| Ok(promise_to_js(&read_runner, read_clipboard.read_text(), |text| JsData::Str(text.clone())))),
        );
        let (write_runner, write_clipboard) = (runner.clone(), clipboard.clone());
        let write_text = runner.make_function(
            "writeText",
            1,
            Rc::new(move |args| {
                let value = string_arg(args, 0, "writeText: o texto")?;
                Ok(promise_to_js(&write_runner, write_clipboard.write_text(&value), |()| JsData::Undefined))
            }),
        );
        members.push((
            "clipboard".to_owned(),
            JsData::Object(vec![("readText".to_owned(), JsData::Handle(Rc::new(read_text))), ("writeText".to_owned(), JsData::Handle(Rc::new(write_text)))]),
        ));
    }
    if let Some(window) = &services.window {
        let (open_runner, open_window) = (runner.clone(), window.clone());
        let open_external = runner.make_function(
            "openExternal",
            2,
            Rc::new(move |args| {
                let uri = uri_from_js(args.first().unwrap_or(&JsData::Undefined))?;
                let options = IOpenUriOptions { allow_tunneling: Some(false), ..IOpenUriOptions::default() };
                Ok(promise_to_js(&open_runner, open_window.open_uri(WindowUriOrString::Uri(uri), options), |opened| JsData::Bool(*opened)))
            }),
        );
        members.push(("openExternal".to_owned(), JsData::Handle(Rc::new(open_external))));
        let (external_runner, external_window) = (runner.clone(), window.clone());
        methods.push(("asExternalUri".to_owned(), 1, Rc::new(move |_, args| as_external_uri(&external_runner, &external_window, authority, args))));
    }
    if let Some(telemetry) = &services.telemetry {
        let enabled = telemetry.clone();
        getters.push(("isTelemetryEnabled".to_owned(), Rc::new(move |_, _| Ok(JsData::Bool(enabled.get_telemetry_configuration())))));
        members.push((
            "onDidChangeTelemetryEnabled".to_owned(),
            event_function(runner, table, "onDidChangeTelemetryEnabled", telemetry.on_did_change_telemetry_enabled(), |enabled| JsData::Bool(*enabled)),
        ));
        let loggers = Rc::new(TelemetryLoggers::default());
        telemetry_logger::define_class(runner, &loggers, table);
        let (logger_runner, logger_telemetry) = (runner.clone(), telemetry.clone());
        methods.push((
            "createTelemetryLogger".to_owned(),
            2,
            Rc::new(move |_, args| telemetry_logger::create_logger(&logger_runner, &logger_telemetry, &loggers, args)),
        ));
    }
    if let Some(log_service) = &services.log_service {
        let level = log_service.clone();
        getters.push(("logLevel".to_owned(), Rc::new(move |_, _| Ok(JsData::Num(f64::from(log_level_value(level.get_level())))))));
        members.push((
            "onDidChangeLogLevel".to_owned(),
            event_function(runner, table, "onDidChangeLogLevel", log_service.on_did_change_log_level(), |level| JsData::Num(f64::from(log_level_value(*level)))),
        ));
    }
    match accessor_namespace(runner, "env", methods, getters) {
        JsData::Object(mut fields) => {
            fields.extend(members);
            JsData::Object(fields)
        }
        other => other,
    }
}

