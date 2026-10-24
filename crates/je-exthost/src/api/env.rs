//! `vscode.env`: `appName`, `appRoot`, `language`, `machineId`, `sessionId`, `uriScheme`, `shell`, `uiKind`, `clipboard`
//! (`readText` e `writeText`) e `openExternal` (`extHost.api.impl.ts`: o literal `env`).
//!
//! Cada grupo só existe quando o dono do host o entrega: os valores estáticos vêm do [`HostEnvironment`]
//! ([`IMainThreadBinder::host_environment`](crate::in_process::IMainThreadBinder::host_environment)), `clipboard` do
//! `IClipboardService` do workbench
//! ([`IMainThreadBinder::clipboard_service`](crate::in_process::IMainThreadBinder::clipboard_service)) e `openExternal` do `ExtHostWindow`
//! (que precisa do cliente `MainThreadWindow` registrado pelo binder).
//!
//! Falta: `isAppPortable`, `appHost`, `isTelemetryEnabled` e seus eventos, `createTelemetryLogger`, `asExternalUri`, `remoteName`,
//! `logLevel` e `onDidChangeLogLevel`, `isNewAppInstall`, `onDidChangeShell` e as propostas.
//!
//! Divergências:
//! - O original lê os valores estáticos do `initData` (`environment.appName`, `appRoot`, `appLanguage`, `appUriScheme`,
//!   `telemetryInfo.machineId` e `sessionId`) e o `shell` do `ExtHostTerminalService`; o host em processo não tem `initData`, então
//!   o dono do host entrega todos num [`HostEnvironment`]. `uiKind` entra no mesmo struct.
//! - `env.clipboard` não passa pelo par `ExtHostClipboard`/`MainThreadClipboard` (`extHostClipboard.ts` e `mainThreadClipboard.ts`,
//!   que só repassam `$readText` e `$writeText` ao `IClipboardService`, e ainda não estão portados): as duas funções falam com o
//!   `IClipboardService` do workbench diretamente.
//! - `openExternal` ignora a opção `allowContributedOpeners` e manda `allowTunneling: false` (o host é local, `initData.remote.isRemote`
//!   do original vale `false` e não há autoridade remota).
//! - O objeto `env` é um objeto comum (o motor o copia a cada leitura de `vscode.env`): `env.clipboard` devolve as mesmas duas funções,
//!   mas `vscode.env.clipboard === vscode.env.clipboard` falha.

use std::rc::Rc;

use je_editor::services::clipboard_service::IClipboardService;
use je_workbench::api::common::ext_host_protocol::IOpenUriOptions;
use je_workbench::api::common::ext_host_window::{ExtHostWindow, UriOrString};
use zjsc::api::embed_node::{JsData, NodeRunner};

use crate::api::commands::promise_to_js;
use crate::convert::{string_arg, uri_from_js};

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
}

/// Os serviços do host que o namespace usa.
#[derive(Default)]
pub struct EnvServices {
    pub environment: Option<HostEnvironment>,
    pub clipboard: Option<Rc<dyn IClipboardService>>,
    pub window: Option<Rc<ExtHostWindow>>,
}

impl EnvServices {
    /// Se há ao menos um membro para expor.
    pub fn is_any(&self) -> bool {
        self.environment.is_some() || self.clipboard.is_some() || self.window.is_some()
    }
}

/// O namespace `env`.
pub fn namespace(runner: &NodeRunner, services: &EnvServices) -> JsData {
    let mut members: Vec<(String, JsData)> = Vec::new();
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
                Ok(promise_to_js(&open_runner, open_window.open_uri(UriOrString::Uri(uri), options), |opened| JsData::Bool(*opened)))
            }),
        );
        members.push(("openExternal".to_owned(), JsData::Handle(Rc::new(open_external))));
    }
    JsData::Object(members)
}
