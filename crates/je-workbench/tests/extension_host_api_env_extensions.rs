//! Teste de ponta a ponta de `vscode.env` e `vscode.extensions` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada
//! pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), lê o ambiente, usa a área de transferência e
//! consulta o registro de extensões do host.
//!
//! O lado do workbench é o binder do teste: entrega o [`HostEnvironment`] (os valores que o original lê do `initData`) e um
//! `IClipboardService` que guarda o texto. O registro de extensões é o do `ExtHostExtensionService` real, com a extensão de exemplo
//! como única extensão local.
//!
//! - **`env`**: `appName`, `appRoot`, `language`, `machineId`, `sessionId`, `uriScheme`, `shell`, `uiKind`, `clipboard.writeText` e
//!   `clipboard.readText`; `openExternal` não existe sem o cliente `MainThreadWindow`.
//! - **`extensions`**: `all`, `getExtension` (com o identificador em outra caixa e um desconhecido), `id`, `packageJSON`,
//!   `extensionPath`, `extensionUri`, `extensionKind`, `isFromDifferentExtensionHost`, `isActive`, `exports` e `activate()`.
//!
//! Fora deste teste: `openExternal` (pede um cliente `MainThreadWindow`, ainda não portado) e `extensions.onDidChange`.

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
use je_workbench::api::common::ext_host_protocol::IMainContext;

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
    openExternal: typeof vscode.env.openExternal,
  }));
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

/// O binder que só entrega o ambiente e a área de transferência (nenhum cliente `MainThread*`).
struct Binder {
    clipboard: Rc<RecordingClipboard>,
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
        })
    }

    fn clipboard_service(&self) -> Option<Rc<dyn IClipboardService>> {
        Some(self.clipboard.clone())
    }
}

fn setup(_workbench: &harness::Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<RecordingClipboard>) {
    let clipboard = Rc::new(RecordingClipboard::default());
    (Rc::new(Binder { clipboard: clipboard.clone() }), clipboard)
}

#[test]
fn env_exposes_the_host_environment_and_hides_open_external_without_a_window_client() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _clipboard| {
        assert_eq!(
            workbench.execute("t.env", Vec::new()),
            Dynamic::string(concat!(
                r#"{"appName":"je-code-teste","appRoot":"/opt/je-code-teste","language":"pt-br","machineId":"machine-0001","#,
                r#""sessionId":"session-0001","uriScheme":"je-code-teste","shell":"/bin/sh","uiKind":1,"openExternal":"undefined"}"#,
            ))
        );
    });
}

#[test]
fn env_clipboard_writes_and_reads_through_the_clipboard_service() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, clipboard| {
        assert_eq!(workbench.execute("t.clipboard", Vec::new()), Dynamic::string("copiado ç"));
        assert_eq!(*clipboard.text.borrow(), "copiado ç", "o texto chegou ao IClipboardService do workbench");
    });
}

#[test]
fn extensions_lists_and_describes_the_extension_registered_in_the_host() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _clipboard| {
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
