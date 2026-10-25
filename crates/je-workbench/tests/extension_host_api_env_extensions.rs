//! Teste de ponta a ponta de `vscode.env` e `vscode.extensions` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada
//! pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), lê o ambiente, usa a área de transferência e
//! consulta o registro de extensões do host.
//!
//! O lado do workbench é o binder do teste: entrega o [`HostEnvironment`] (os valores que o original lê do `initData`) e um
//! `IClipboardService` que guarda o texto. O registro de extensões é o do `ExtHostExtensionService` real, com a extensão de exemplo
//! como única extensão local.
//!
//! - **`env`**: `appName`, `appRoot`, `language`, `machineId`, `sessionId`, `uriScheme`, `shell`, `uiKind`, `remoteName`,
//!   `clipboard.writeText` e `clipboard.readText`; `openExternal` e `asExternalUri` não existem sem o cliente `MainThreadWindow`.
//! - **`env` (telemetria e log)**: `isTelemetryEnabled`, `onDidChangeTelemetryEnabled`, `createTelemetryLogger` (`logUsage`, `logError`
//!   com texto e com `Error`, `isUsageEnabled`, `isErrorsEnabled`, `onDidChangeEnableStates`, `dispose` com `flush`), `logLevel` e
//!   `onDidChangeLogLevel`.
//! - **`extensions`**: `all`, `getExtension` (com o identificador em outra caixa e um desconhecido), `id`, `packageJSON`,
//!   `extensionPath`, `extensionUri`, `extensionKind`, `isFromDifferentExtensionHost`, `isActive`, `exports`, `activate()` e
//!   `onDidChange` (a função de evento e o descarte da assinatura).
//!
//! Fora deste teste: `openExternal` e `asExternalUri` (pedem um cliente `MainThreadWindow`, ainda não portado) e o disparo de
//! `extensions.onDidChange` (o registro do host não é alcançável pelo harness).

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;

use harness::run_extension;
use je_base::dynamic::Dynamic;
use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_exthost::api::env::HostEnvironment;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::log::{AbstractLoggerService, CreateLoggerFn, LogLevel, LoggerOptions, LoggerRef, NullLogger};
use je_platform::telemetry::TelemetryLevel;
use je_workbench::api::common::ext_host_protocol::{ExtHostTelemetryShape, IMainContext};
use je_workbench::api::common::ext_host_telemetry::{ExtHostTelemetry, ExtHostTelemetryInitData};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__path = context.extensionPath;
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  command('t.env', () => JSON.stringify({
    appName: vscode.env.appName,
    appRoot: vscode.env.appRoot,
    language: vscode.env.language,
    machineId: vscode.env.machineId,
    sessionId: vscode.env.sessionId,
    uriScheme: vscode.env.uriScheme,
    shell: vscode.env.shell,
    uiKind: vscode.env.uiKind,
    remoteName: vscode.env.remoteName,
    openExternal: typeof vscode.env.openExternal,
    asExternalUri: typeof vscode.env.asExternalUri,
  }));
  command('t.members', () => JSON.stringify({
    isTelemetryEnabled: typeof vscode.env.isTelemetryEnabled,
    onDidChangeTelemetryEnabled: typeof vscode.env.onDidChangeTelemetryEnabled,
    createTelemetryLogger: typeof vscode.env.createTelemetryLogger,
    logLevel: typeof vscode.env.logLevel,
    onDidChangeLogLevel: typeof vscode.env.onDidChangeLogLevel,
    extensionsOnDidChange: typeof vscode.extensions.onDidChange,
    subscription: typeof vscode.extensions.onDidChange(() => {}).dispose,
    badSender: (() => { try { vscode.env.createTelemetryLogger({}); return 'no error'; } catch (error) { return error.message; } })(),
  }));
  command('t.telemetry.setup', () => {
    globalThis.__sent = [];
    globalThis.__events = [];
    globalThis.__logger = vscode.env.createTelemetryLogger({
      sendEventData: (name, data) => { globalThis.__sent.push(['event', name, data.k, data['common.extname'], data.extra]); },
      sendErrorData: (error, data) => { globalThis.__sent.push(['error', error.message, data.k]); },
      flush: () => { globalThis.__sent.push(['flush']); },
    }, { additionalCommonProperties: { extra: 'x' } });
    context.subscriptions.push(vscode.env.onDidChangeTelemetryEnabled((enabled) => globalThis.__events.push(['enabled', enabled])));
    globalThis.__logger.onDidChangeEnableStates((logger) => globalThis.__events.push(['states', logger.isUsageEnabled, logger.isErrorsEnabled]));
    globalThis.__logger.logUsage('click', { k: 'v' });
    globalThis.__logger.logError('boom', { k: 'e' });
    globalThis.__logger.logError(new Error('bad'), { k: 'x' });
    return JSON.stringify({
      enabled: vscode.env.isTelemetryEnabled,
      usage: globalThis.__logger.isUsageEnabled,
      errors: globalThis.__logger.isErrorsEnabled,
      sent: globalThis.__sent,
    });
  });
  command('t.telemetry.state', () => {
    globalThis.__logger.logUsage('late');
    return JSON.stringify({
      enabled: vscode.env.isTelemetryEnabled,
      usage: globalThis.__logger.isUsageEnabled,
      errors: globalThis.__logger.isErrorsEnabled,
      events: globalThis.__events,
      count: globalThis.__sent.length,
    });
  });
  command('t.telemetry.dispose', () => {
    globalThis.__logger.logUsage('again', { k: 'a' });
    const before = globalThis.__sent.length;
    globalThis.__logger.dispose();
    globalThis.__logger.logUsage('after dispose');
    return JSON.stringify({ before, count: globalThis.__sent.length, last: globalThis.__sent[globalThis.__sent.length - 1] });
  });

  command('t.clipboard', async () => {
    await vscode.env.clipboard.writeText('copiado ç');
    return await vscode.env.clipboard.readText();
  });
  command('t.extensions', async () => {
    const all = vscode.extensions.all;
    const extension = vscode.extensions.getExtension('JE-TEST.API');
    const unknown = vscode.extensions.getExtension('nao.existe');
    const manifest = extension.packageJSON;
    return JSON.stringify({
      count: all.length,
      ids: all.map((item) => item.id),
      id: extension.id,
      unknown: String(unknown),
      name: manifest.name,
      publisher: manifest.publisher,
      version: manifest.version,
      main: manifest.main,
      isBuiltin: manifest.isBuiltin,
      identifier: manifest.identifier.value,
      pathMatches: extension.extensionPath === globalThis.__path,
      uriScheme: extension.extensionUri.scheme,
      kind: extension.extensionKind,
      foreign: extension.isFromDifferentExtensionHost,
      active: extension.isActive,
      answer: extension.exports.answer,
      greeting: extension.exports.greet('mundo'),
      activated: (await extension.activate()).answer,
    });
  });
  return { answer: 42, greet: (name) => 'olá ' + name };
};
"#;

/// Um `IClipboardService` que guarda o último texto gravado.
#[derive(Default)]
struct RecordingClipboard {
    text: RefCell<String>,
}

impl IClipboardService for RecordingClipboard {
    fn write_text(&self, text: &str) -> Promise<()> {
        *self.text.borrow_mut() = text.to_owned();
        Promise::resolved(())
    }

    fn read_text(&self) -> Promise<String> {
        Promise::resolved(self.text.borrow().clone())
    }

    fn read_find_text(&self) -> Promise<JsString> {
        Promise::resolved(JsString::new())
    }

    fn write_find_text(&self, _text: &JsString) -> Promise<()> {
        Promise::resolved(())
    }

    fn write_resources(&self, _resources: Vec<URI>) -> Promise<()> {
        Promise::resolved(())
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        Promise::resolved(Vec::new())
    }

    fn has_resources(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
}

/// O binder que entrega o ambiente, a área de transferência e a telemetria (nenhum cliente `MainThread*`).
struct Binder {
    clipboard: Rc<RecordingClipboard>,
    telemetry: Rc<ExtHostTelemetry>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, _context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        Vec::new()
    }

    fn host_environment(&self) -> Option<HostEnvironment> {
        Some(HostEnvironment {
            app_name: "je-code-teste".to_owned(),
            app_root: "/opt/je-code-teste".to_owned(),
            app_language: "pt-br".to_owned(),
            app_uri_scheme: "je-code-teste".to_owned(),
            machine_id: "machine-0001".to_owned(),
            session_id: "session-0001".to_owned(),
            shell: "/bin/sh".to_owned(),
            ui_kind: 1,
            remote_authority: Some("ssh-remote+caixa".to_owned()),
        })
    }

    fn clipboard_service(&self) -> Option<Rc<dyn IClipboardService>> {
        Some(self.clipboard.clone())
    }

    fn telemetry(&self) -> Option<Rc<ExtHostTelemetry>> {
        Some(self.telemetry.clone())
    }
}

/// O que o teste observa depois que o host sobe.
struct Observed {
    clipboard: Rc<RecordingClipboard>,
    telemetry: Rc<ExtHostTelemetry>,
}

fn setup(_workbench: &harness::Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    let clipboard = Rc::new(RecordingClipboard::default());
    let create_logger: CreateLoggerFn = Rc::new(|_: &URI, _: LogLevel, _: &LoggerOptions| -> LoggerRef { Rc::new(NullLogger::new()) });
    let logger_service = AbstractLoggerService::new(LogLevel::Info, URI::file("/logs", je_platform::resources::host_platform()), Vec::new(), create_logger);
    let telemetry = ExtHostTelemetry::new(false, ExtHostTelemetryInitData::default(), &*logger_service);
    telemetry.rpc_initialize_telemetry_level(TelemetryLevel::Usage, true, None);
    (Rc::new(Binder { clipboard: clipboard.clone(), telemetry: telemetry.clone() }), Observed { clipboard, telemetry })
}

#[test]
fn env_exposes_the_host_environment_and_hides_the_window_members_without_a_window_client() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.env", Vec::new()),
            Dynamic::string(concat!(
                r#"{"appName":"je-code-teste","appRoot":"/opt/je-code-teste","language":"pt-br","machineId":"machine-0001","#,
                r#""sessionId":"session-0001","uriScheme":"je-code-teste","shell":"/bin/sh","uiKind":1,"remoteName":"ssh-remote","#,
                r#""openExternal":"undefined","asExternalUri":"undefined"}"#,
            ))
        );
    });
}

#[test]
fn env_clipboard_writes_and_reads_through_the_clipboard_service() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.clipboard", Vec::new()), Dynamic::string("copiado ç"));
        assert_eq!(*observed.clipboard.text.borrow(), "copiado ç", "o texto chegou ao IClipboardService do workbench");
    });
}

#[test]
fn telemetry_log_level_and_extension_change_members_exist_and_the_sender_is_validated() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.members", Vec::new()),
            Dynamic::string(concat!(
                r#"{"isTelemetryEnabled":"boolean","onDidChangeTelemetryEnabled":"function","createTelemetryLogger":"function","#,
                r#""logLevel":"number","onDidChangeLogLevel":"function","extensionsOnDidChange":"function","subscription":"function","#,
                r#""badSender":"TelemetrySender.sendEventData must be a function"}"#,
            ))
        );
    });
}

#[test]
fn a_telemetry_logger_sends_through_the_sender_follows_the_telemetry_level_and_flushes_on_dispose() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(
            workbench.execute("t.telemetry.setup", Vec::new()),
            Dynamic::string(concat!(
                r#"{"enabled":true,"usage":true,"errors":true,"sent":["#,
                r#"["event","je.exthost/click","v","je.exthost","x"],"#,
                r#"["event","je.exthost/boom","e","je.exthost","x"],"#,
                r#"["error","bad","x"]]}"#,
            ))
        );

        observed.telemetry.rpc_on_did_change_telemetry_level(TelemetryLevel::None);
        workbench.pump();
        assert_eq!(
            workbench.execute("t.telemetry.state", Vec::new()),
            Dynamic::string(r#"{"enabled":false,"usage":false,"errors":false,"events":[["states",false,false],["enabled",false]],"count":3}"#),
            "com a telemetria desligada o logger não envia e os dois eventos chegam, o dos loggers antes do global"
        );

        observed.telemetry.rpc_on_did_change_telemetry_level(TelemetryLevel::Usage);
        workbench.pump();
        assert_eq!(
            workbench.execute("t.telemetry.dispose", Vec::new()),
            Dynamic::string(r#"{"before":4,"count":5,"last":["flush"]}"#),
            "religada, o logger envia; o dispose chama o flush e depois disso nada mais é enviado"
        );
    });
}


#[test]
fn extensions_lists_and_describes_the_extension_registered_in_the_host() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.extensions", Vec::new()),
            Dynamic::string(concat!(
                r#"{"count":1,"ids":["je-test.api"],"id":"je-test.api","unknown":"undefined","name":"api","publisher":"je-test","#,
                r#""version":"1.0.0","main":"./extension.js","isBuiltin":false,"identifier":"je-test.api","pathMatches":true,"#,
                r#""uriScheme":"file","kind":1,"foreign":false,"active":true,"answer":42,"greeting":"olá mundo","activated":42}"#,
            ))
        );
    });
}
