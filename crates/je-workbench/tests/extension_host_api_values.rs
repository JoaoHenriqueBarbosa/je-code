//! Teste de ponta a ponta das classes de valor do `vscode` (não é porte do oráculo): `Uri`, `EventEmitter`,
//! `CancellationTokenSource` e as enumerações. Uma extensão de exemplo em JavaScript, ativada pelo host local
//! (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), usa as classes de verdade e devolve o que mediu pelos comandos,
//! que o workbench executa pelo `CommandService` (o resultado atravessa o RPC).
//!
//! Sem clientes `MainThread*` ligados: o `vscode` do host tem só `commands`, `l10n` e as classes, que é o que este teste exerce.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::IMainContext;

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  command('t.uri', () => {
    const file = vscode.Uri.file('/work/app/src/main.rs');
    const parsed = vscode.Uri.parse('https://example.com:8080/a%20b?q=1#frag');
    const joined = vscode.Uri.joinPath(file, '..', 'lib.rs');
    const changed = file.with({ scheme: 'untitled', fragment: 'x' });
    const cleared = parsed.with({ query: null });
    const built = vscode.Uri.from({ scheme: 'vscode-test', authority: 'host', path: '/p' });
    let constructed = false;
    try { new vscode.Uri('x', 'y', 'z'); } catch (error) { constructed = String(error.message).includes('Uri'); }
    return {
      fsPath: file.fsPath, scheme: file.scheme, text: file.toString(), isInstance: file instanceof vscode.Uri,
      parsedAuthority: parsed.authority, parsedPath: parsed.path, parsedQuery: parsed.query, parsedFragment: parsed.fragment,
      parsedText: parsed.toString(), joined: joined.path, changedScheme: changed.scheme, changedFragment: changed.fragment,
      changedPath: changed.path, clearedQuery: cleared.query, built: built.toString(), constructorThrows: constructed,
      jsonPath: JSON.parse(JSON.stringify(file)).path,
    };
  });
  command('t.events', () => {
    const emitter = new vscode.EventEmitter();
    const seen = [];
    const first = emitter.event((value) => seen.push('a:' + value));
    const second = emitter.event((value) => seen.push('b:' + value));
    emitter.fire(1);
    first.dispose();
    emitter.fire(2);
    second.dispose();
    emitter.fire(3);
    emitter.dispose();
    return seen;
  });
  command('t.cancel', () => {
    const source = new vscode.CancellationTokenSource();
    const token = source.token;
    globalThis.__cancelLog = [];
    token.onCancellationRequested(() => globalThis.__cancelLog.push('cancelled'));
    const before = token.isCancellationRequested;
    source.cancel();
    return { before, after: token.isCancellationRequested, again: source.token.isCancellationRequested };
  });
  command('t.enums', () => ({
    left: vscode.StatusBarAlignment.Left, rightName: vscode.StatusBarAlignment[2],
    global: vscode.ConfigurationTarget.Global, workspaceFolder: vscode.ConfigurationTarget.WorkspaceFolder, web: vscode.UIKind.Web,
  }));
};
"#;

/// Sem clientes do workbench: só as classes do `vscode`.
struct NoClients;

impl IMainThreadBinder for NoClients {
    fn bind(&self, _context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        Vec::new()
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, ()) {
    (Rc::new(NoClients), ())
}

fn text(value: &Dynamic, key: &str) -> String {
    match value.get_str(key) {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto em '{key}', veio {other:?}"),
    }
}

#[test]
fn uri_builds_parses_joins_and_changes_like_the_original() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, ()| {
        let uri = workbench.execute("t.uri", Vec::new());
        assert_eq!(text(&uri, "fsPath"), "/work/app/src/main.rs");
        assert_eq!(text(&uri, "scheme"), "file");
        assert_eq!(text(&uri, "text"), "file:///work/app/src/main.rs");
        assert_eq!(uri.get_str("isInstance"), Dynamic::Bool(true));
        assert_eq!(text(&uri, "parsedAuthority"), "example.com:8080");
        assert_eq!(text(&uri, "parsedPath"), "/a b", "o caminho vem decodificado");
        assert_eq!(text(&uri, "parsedQuery"), "q=1");
        assert_eq!(text(&uri, "parsedFragment"), "frag");
        assert_eq!(text(&uri, "parsedText"), "https://example.com:8080/a%20b?q=1#frag", "o toString recodifica");
        assert_eq!(text(&uri, "joined"), "/work/app/src/lib.rs");
        assert_eq!(text(&uri, "changedScheme"), "untitled");
        assert_eq!(text(&uri, "changedFragment"), "x");
        assert_eq!(text(&uri, "changedPath"), "/work/app/src/main.rs");
        assert_eq!(text(&uri, "clearedQuery"), "", "with({{ query: null }}) limpa o componente");
        assert_eq!(text(&uri, "built"), "vscode-test://host/p");
        assert_eq!(uri.get_str("constructorThrows"), Dynamic::Bool(true), "Uri não tem construtor público");
        assert_eq!(text(&uri, "jsonPath"), "/work/app/src/main.rs", "JSON.stringify usa o toJSON, que mantém os componentes");
    });
}

#[test]
fn event_emitter_delivers_to_listeners_until_each_subscription_is_disposed() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, ()| {
        let seen = workbench.execute("t.events", Vec::new());
        let Dynamic::Array(seen) = seen else { panic!("o comando deveria devolver uma lista") };
        let seen: Vec<String> = seen.to_vec().iter().map(|item| match item {
            Dynamic::String(text) => text.to_string_lossy(),
            other => panic!("esperava texto, veio {other:?}"),
        }).collect();
        assert_eq!(seen, vec!["a:1", "b:1", "b:2"]);
    });
}

#[test]
fn cancellation_token_source_cancels_the_live_token() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, ()| {
        let result = workbench.execute("t.cancel", Vec::new());
        assert_eq!(result.get_str("before"), Dynamic::Bool(false));
        assert_eq!(result.get_str("after"), Dynamic::Bool(true), "o token acompanha o cancel() da fonte");
        assert_eq!(result.get_str("again"), Dynamic::Bool(true), "o token da fonte é sempre o mesmo");
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__cancelLog)"), r#"["cancelled"]"#);
    });
}

#[test]
fn enumerations_have_the_original_values_in_both_directions() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, ()| {
        let enums = workbench.execute("t.enums", Vec::new());
        assert_eq!(enums.get_str("left"), Dynamic::Number(1.0));
        assert_eq!(text(&enums, "rightName"), "Right");
        assert_eq!(enums.get_str("global"), Dynamic::Number(1.0));
        assert_eq!(enums.get_str("workspaceFolder"), Dynamic::Number(3.0));
        assert_eq!(enums.get_str("web"), Dynamic::Number(2.0));
    });
}
