//! O host de extensões de ponta a ponta: uma extensão CommonJS em disco pede `require('vscode')`, registra comandos no
//! `ExtHostCommands` (que fala com o `MainThreadCommands` real) e os executa, tudo sobre o zjsc.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_exthost::host::{ExtensionHost, WorkbenchServices};
use je_exthost::runtime::{with_runtime_thread, DiskModuleFs, OsConsole};
use je_platform::commands::commands::{CommandEvent, ICommandService};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use zjsc::api::embed_node::{run_node, JsData, NodeRunner};

struct IdleCommandService;

impl ICommandService for IdleCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        Promise::rejected(je_base::errors::Error::new(format!("command '{command_id}' not found")))
    }
}

fn text(runner: &NodeRunner, source: &str) -> String {
    runner.eval(source, "test.js").unwrap().as_text().expect("o programa devolve string")
}

/// Roda `body` com o host montado e uma extensão `extension.js` com o conteúdo `source` em disco.
fn with_host<T: Send + 'static>(source: &'static str, body: impl FnOnce(&NodeRunner, &ExtensionHost) -> T + Send + 'static) -> T {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("extension.js"), source).unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    let main: &'static str = Box::leak(format!("globalThis.loadExtension = () => require('{base}/extension.js'); globalThis.requireVscode = () => require('vscode');").into_boxed_str());
    with_runtime_thread(move || {
        let _keep = dir;
        run_node(Rc::new(OsConsole), Rc::new(DiskModuleFs), "/je-test/main.js", main, |runner| {
            let host = ExtensionHost::new(runner, WorkbenchServices { command_service: Rc::new(IdleCommandService), extension_service: TestExtensionService::new() });
            body(runner, &host)
        })
        .unwrap()
    })
}

const EXTENSION: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  context.subscriptions.push(vscode.commands.registerCommand('ext.hello', (name) => vscode.l10n.t('Olá, {0}!', name)));
  context.subscriptions.push(vscode.commands.registerCommand('ext.double', async (n) => { await null; return n * 2; }));
  context.subscriptions.push(vscode.commands.registerCommand('ext.fail', () => { throw new Error('falhou de propósito'); }));
  context.subscriptions.push(new vscode.Disposable(() => { globalThis.cleaned = true; }));
};
"#;

/// Ativa a extensão e deixa `globalThis.out` para os resultados assíncronos.
fn activate(runner: &NodeRunner, host: &ExtensionHost) {
    runner.eval("globalThis.out = []; globalThis.context = { subscriptions: [] }; loadExtension().activate(context); 0", "activate.js").unwrap();
    host.pump();
}

fn run(runner: &NodeRunner, host: &ExtensionHost, source: &str) -> String {
    runner.eval(source, "run.js").unwrap();
    host.pump();
    text(runner, "out.join('|')")
}

#[test]
fn extension_registers_commands_and_gets_disposables_back() {
    with_host(EXTENSION, |runner, host| {
        activate(runner, host);
        assert_eq!(text(runner, "String(context.subscriptions.length)"), "4");
        assert_eq!(text(runner, "const vs = requireVscode(); context.subscriptions.every((d) => d instanceof vs.Disposable) + ''"), "true");
    });
}

#[test]
fn execute_command_runs_sync_async_and_failing_commands() {
    with_host(EXTENSION, |runner, host| {
        activate(runner, host);
        let out = run(
            runner,
            host,
            "const vs = requireVscode();
             vs.commands.executeCommand('ext.hello', 'mundo').then((r) => out.push('hello:' + r));
             vs.commands.executeCommand('ext.double', 21).then((r) => out.push('double:' + r));
             vs.commands.executeCommand('ext.fail').then(() => out.push('fail:ok'), (e) => out.push('fail:' + e.message));",
        );
        let mut parts: Vec<&str> = out.split('|').collect();
        parts.sort();
        assert_eq!(parts, ["double:42", "fail:falhou de propósito", "hello:Olá, mundo!"]);
    });
}

#[test]
fn get_commands_lists_the_registered_commands() {
    with_host(EXTENSION, |runner, host| {
        activate(runner, host);
        let out = run(runner, host, "requireVscode().commands.getCommands().then((ids) => out.push(ids.filter((id) => id.startsWith('ext.')).sort().join()))");
        assert_eq!(out, "ext.double,ext.fail,ext.hello");
    });
}

#[test]
fn disposing_a_subscription_unregisters_the_command_and_runs_callbacks_once() {
    with_host(EXTENSION, |runner, host| {
        activate(runner, host);
        let out = run(
            runner,
            host,
            "const vs = requireVscode();
             context.subscriptions[0].dispose(); context.subscriptions[0].dispose();
             context.subscriptions[3].dispose(); out.push('cleaned:' + globalThis.cleaned);
             vs.commands.executeCommand('ext.hello', 'x').then(() => out.push('hello:ainda existe'), () => out.push('hello:removido'));",
        );
        assert_eq!(out, "cleaned:true|hello:removido");
    });
}

#[test]
fn registering_the_same_command_twice_throws_in_the_extension() {
    with_host(EXTENSION, |runner, host| {
        activate(runner, host);
        let out = run(runner, host, "try { requireVscode().commands.registerCommand('ext.hello', () => {}); out.push('sem erro'); } catch (e) { out.push(e.message); }");
        assert_eq!(out, "command 'ext.hello' already exists");
    });
}

#[test]
fn l10n_formats_positional_named_and_object_forms() {
    with_host("", |runner, host| {
        runner.eval("globalThis.out = []", "init.js").unwrap();
        let out = run(
            runner,
            host,
            "const t = requireVscode().l10n.t;
             out.push(t('a {0} b {1} c {2}', 'x', 2));
             out.push(t('olá {nome}', { nome: 'você' }));
             out.push(t({ message: '{0}-{0}', args: ['z'], comment: 'ignorado' }));
             out.push(t('sem args {chave}'));",
        );
        assert_eq!(out, "a x b 2 c {2}|olá você|z-z|sem args {chave}");
    });
}
