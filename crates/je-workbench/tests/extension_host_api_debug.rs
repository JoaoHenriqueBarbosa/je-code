//! Teste de ponta a ponta de `vscode.debug` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo host local
//! (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), registra provedores, inicia sessões e mexe nos breakpoints, e o
//! efeito aparece nos dois lados.
//!
//! O lado do ExtHost é o `ExtHostDebugService` REAL (portado de `extHostDebugService.ts`), ligado pelo host em processo. O lado do
//! workbench é um dublê do `MainThreadDebugServiceShape` ([`Panel`]) que anota cada chamada que recebe e guarda o proxy do ExtHost, por
//! onde o teste entrega o que o workbench real mandaria: `$provideDebugConfigurations`, `$resolveDebugConfiguration`,
//! `$provideDebugAdapter`, os `$acceptDebugSession*` e `$acceptBreakpointsDelta`. O `MainThreadDebugService` real pede o
//! `IDebugService` inteiro (sessões, adaptadores, breakpoints do modelo), que este teste não exerce.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;

use harness::{run_extension, run_extension_contributing, settled_ok, Workbench};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::in_process::IMainThreadBinder;
use je_extensions::manifest::{DebuggerContribution, ExtensionContributions};
use je_workbench::api::common::ext_host_debug_protocol::{
    BreakpointDto, ExtHostDebugServiceShape, IBreakpointDto, IBreakpointsDeltaDto, IDebugSessionDto, IDebugSessionFullDto, IFunctionBreakpointDto, ISourceBreakpointDto,
    IStackFrameFocusDto, IStartDebuggingOptions, IThreadFocusDto, MainThreadDebugServiceShape, NameOrConfig, RegisterBreakpointDto, StackFocusDto,
};
use je_workbench::api::common::ext_host_protocol::{ExtHostContext, IMainContext, MainContext};
use je_workbench::contrib::debug::debug::{
    DebugConfigurationProviderTriggerKind, IAdapterDescriptor, IConfig, IDebugAdapterExecutable, IDebugAdapterExecutableOptions,
};
use je_workbench::contrib::debug::debug_protocol::{ProtocolMessage, Request};
use serde_json::{json, Value};

/// O JavaScript da extensão de exemplo: cada comando devolve um valor simples que o teste confere.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const push = (text) => globalThis.__events.push(text);
  const describe = (bp) => {
    if (bp instanceof vscode.SourceBreakpoint) {
      return 'source:' + bp.location.uri.path + ':' + bp.location.range.start.line + ':' + bp.enabled + ':' + bp.condition + ':' + bp.logMessage;
    }
    if (bp instanceof vscode.FunctionBreakpoint) {
      return 'function:' + bp.functionName + ':' + bp.enabled;
    }
    return 'outro';
  };

  command('t.registerProvider', () => {
    globalThis.__provider = vscode.debug.registerDebugConfigurationProvider('mock', {
      provideDebugConfigurations(folder, token) {
        globalThis.__provided = [folder === undefined, typeof token.isCancellationRequested];
        return [{ type: 'mock', request: 'launch', name: 'Gerada', program: '${file}' }];
      },
      resolveDebugConfiguration(folder, config) {
        config.resolvedBy = 'extensão';
        return config;
      },
    }, vscode.DebugConfigurationProviderTriggerKind.Dynamic);
    return 'registrado';
  });
  command('t.disposeProvider', () => {
    globalThis.__provider.dispose();
    return 'descartado';
  });

  command('t.registerFactory', () => {
    try {
      globalThis.__factory = vscode.debug.registerDebugAdapterDescriptorFactory('mock', {
        createDebugAdapterDescriptor(session, executable) {
          globalThis.__descriptor = [session.id, session.type, session.name, executable === undefined].join(':');
          return new vscode.DebugAdapterExecutable('mock-adapter', ['--stdio'], { cwd: '/w' });
        },
      });
      return 'registrado';
    } catch (error) {
      return 'erro:' + error.message;
    }
  });
  command('t.registerForeignFactory', () => {
    try {
      vscode.debug.registerDebugAdapterDescriptorFactory('outro', { createDebugAdapterDescriptor() { return undefined; } });
      return 'sem erro';
    } catch (error) {
      return 'erro:' + error.message;
    }
  });
  command('t.disposeFactory', () => {
    globalThis.__factory.dispose();
    return 'descartado';
  });

  command('t.listen', () => {
    context.subscriptions.push(vscode.debug.onDidStartDebugSession((s) => push('start:' + s.id + ':' + s.type + ':' + s.name + ':' + s.configuration.request)));
    context.subscriptions.push(vscode.debug.onDidTerminateDebugSession((s) => push('terminate:' + s.id)));
    context.subscriptions.push(vscode.debug.onDidChangeActiveDebugSession((s) => push('active:' + (s ? s.id : 'nenhuma'))));
    context.subscriptions.push(vscode.debug.onDidChangeBreakpoints((e) => push('breakpoints:' + e.added.length + '/' + e.removed.length + '/' + e.changed.length)));
    context.subscriptions.push(vscode.debug.onDidReceiveDebugSessionCustomEvent((e) => push('custom:' + e.session.id + ':' + e.event + ':' + JSON.stringify(e.body))));
    return 'ouvindo';
  });
  command('t.activeSession', () => {
    const session = vscode.debug.activeDebugSession;
    return session ? session.id + ':' + session.name : 'nenhuma';
  });
  command('t.rename', () => {
    vscode.debug.activeDebugSession.name = 'Renomeada';
    return vscode.debug.activeDebugSession.name;
  });
  command('t.customRequest', async () => JSON.stringify(await vscode.debug.activeDebugSession.customRequest('threads', { all: true })));
  command('t.console', () => {
    vscode.debug.activeDebugConsole.append('a');
    vscode.debug.activeDebugConsole.appendLine('b');
    return 'ok';
  });

  const folder = () => ({ uri: vscode.Uri.file('/w'), name: 'w', index: 0 });
  command('t.start', () => vscode.debug.startDebugging(folder(), { type: 'mock', request: 'launch', name: 'Rodar', port: 9229 }, {
    noDebug: true, consoleMode: vscode.DebugConsoleMode.MergeWithParent, suppressDebugView: true,
  }));
  command('t.startByName', () => vscode.debug.startDebugging(undefined, 'Rodar'));
  command('t.startChild', () => vscode.debug.startDebugging(undefined, 'filho', vscode.debug.activeDebugSession));
  command('t.stopAll', async () => {
    await vscode.debug.stopDebugging();
    return 'parou';
  });
  command('t.stopActive', async () => {
    await vscode.debug.stopDebugging(vscode.debug.activeDebugSession);
    return 'parou';
  });

  command('t.addBreakpoints', async () => {
    const source = new vscode.SourceBreakpoint(new vscode.Location(vscode.Uri.file('/w/a.rs'), new vscode.Range(4, 0, 4, 10)), true, 'x > 1', undefined, 'valor {x}');
    const named = new vscode.FunctionBreakpoint('main', false);
    await vscode.debug.addBreakpoints([source, named]);
    globalThis.__bps = [source, named];
    return JSON.stringify(vscode.debug.breakpoints.map(describe));
  });
  command('t.removeFirstBreakpoint', async () => {
    await vscode.debug.removeBreakpoints([globalThis.__bps[0]]);
    return JSON.stringify(vscode.debug.breakpoints.map(describe));
  });
  command('t.breakpoints', () => JSON.stringify(vscode.debug.breakpoints.map(describe)));

  const describeStack = (item) =>
    (item instanceof vscode.DebugStackFrame ? 'frame:' : item instanceof vscode.DebugThread ? 'thread:' : '?:') + item.session.id + ':' + item.threadId + ':' + item.frameId;
  command('t.stack', () => {
    const item = vscode.debug.activeStackItem;
    return item ? describeStack(item) : 'nenhum';
  });
  command('t.listenStack', () => {
    context.subscriptions.push(vscode.debug.onDidChangeActiveStackItem((item) => push('stack:' + (item ? describeStack(item) : 'nenhum'))));
    return 'ouvindo';
  });
  command('t.newStackItems', () => {
    const session = vscode.debug.activeDebugSession;
    return JSON.stringify([describeStack(new vscode.DebugThread(session, 2)), describeStack(new vscode.DebugStackFrame(session, 2, 5))]);
  });
  command('t.sourceUri', () => {
    const parts = (uri) => uri.scheme + '|' + uri.path + '|' + uri.query;
    return JSON.stringify([
      parts(vscode.debug.asDebugSourceUri({ path: '/w/a.rs' })),
      parts(vscode.debug.asDebugSourceUri({ path: 'mem', sourceReference: 7 }, vscode.debug.activeDebugSession)),
      parts(vscode.debug.asDebugSourceUri({ sourceReference: 9 })),
      (() => { try { vscode.debug.asDebugSourceUri({}); return 'sem erro'; } catch (error) { return error.message; } })(),
    ]);
  });
  command('t.dataBreakpoint', () => {
    const breakpoint = new vscode.DataBreakpoint('var x', 'id-1', true, false, 'x > 1');
    let failure;
    try { new vscode.DataBreakpoint('v', ''); } catch (error) { failure = error.message; }
    return JSON.stringify([breakpoint instanceof vscode.Breakpoint, breakpoint.label, breakpoint.dataId, breakpoint.canPersist, breakpoint.enabled, breakpoint.condition, failure]);
  });

  command('t.registerInlineAndTracker', () => {
    const emitter = new vscode.EventEmitter();
    const adapter = {
      onDidSendMessage: emitter.event,
      handleMessage(message) {
        push('adapter<-' + message.command);
        emitter.fire({ seq: 2, type: 'response', request_seq: message.seq, success: true, command: message.command });
      },
      dispose() { push('adapter dispose'); },
    };
    context.subscriptions.push(vscode.debug.registerDebugAdapterDescriptorFactory('mock', {
      createDebugAdapterDescriptor() { return new vscode.DebugAdapterInlineImplementation(adapter); },
    }));
    globalThis.__tracker = vscode.debug.registerDebugAdapterTrackerFactory('mock', {
      createDebugAdapterTracker(session) {
        return {
          onWillStartSession: () => push('tracker start ' + session.id),
          onWillReceiveMessage: (message) => push('tracker recv ' + message.command),
          onDidSendMessage: (message) => push('tracker sent ' + message.type),
          onWillStopSession: () => push('tracker stop'),
        };
      },
    });
    return 'registrado';
  });
  command('t.disposeTracker', () => {
    globalThis.__tracker.dispose();
    return 'descartado';
  });
};
"#;

/// O papel da thread principal: o `MainThreadDebugServiceShape` que anota o que recebe e guarda o proxy do ExtHost.
struct Panel {
    log: RefCell<Vec<String>>,
    ext_host: RefCell<Option<Rc<dyn ExtHostDebugServiceShape>>>,
}

impl Panel {
    fn new() -> Rc<Panel> {
        Rc::new(Panel { log: RefCell::new(Vec::new()), ext_host: RefCell::new(None) })
    }

    fn note(&self, line: String) {
        self.log.borrow_mut().push(line);
    }

    /// As linhas anotadas que começam com `prefix` (o `$registerDebugTypes` do início não interessa à maioria dos casos).
    fn entries(&self, prefix: &str) -> Vec<String> {
        self.log.borrow().iter().filter(|line| line.starts_with(prefix)).cloned().collect()
    }

    fn ext_host(&self) -> Rc<dyn ExtHostDebugServiceShape> {
        self.ext_host.borrow().clone().expect("o binder já guardou o proxy do ExtHost")
    }
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

fn describe_registered(dto: &RegisterBreakpointDto) -> String {
    match dto {
        RegisterBreakpointDto::SourceMulti(multi) => format!(
            "source:{}:{}",
            multi.uri.to_string(false).to_string_lossy(),
            multi.lines.iter().map(|line| format!("{}@{}:{:?}", line.line, line.character, line.condition)).collect::<Vec<_>>().join(",")
        ),
        RegisterBreakpointDto::Function(function) => format!("function:{}:{}", function.function_name, function.base.enabled),
        RegisterBreakpointDto::Data(_) => "data".to_owned(),
    }
}

impl MainThreadDebugServiceShape for Panel {
    fn rpc_register_debug_types(&self, debug_types: Vec<String>) {
        self.note(format!("registerDebugTypes:{}", debug_types.join(",")));
    }

    fn rpc_session_cached(&self, session_id: &str) {
        self.note(format!("sessionCached:{session_id}"));
    }

    fn rpc_accept_da_message(&self, handle: u32, message: ProtocolMessage) {
        self.note(format!("acceptDAMessage:{handle}:{}", message.to_json()));
    }

    fn rpc_accept_da_error(&self, _handle: u32, _name: &str, _message: &str, _stack: Option<&str>) {
        panic!("o teste não exercita $acceptDAError");
    }

    fn rpc_accept_da_exit(&self, _handle: u32, _code: Option<i32>, _signal: Option<&str>) {
        panic!("o teste não exercita $acceptDAExit");
    }

    fn rpc_register_debug_configuration_provider(
        &self,
        r#type: &str,
        trigger_kind: DebugConfigurationProviderTriggerKind,
        has_provide_method: bool,
        has_resolve_method: bool,
        has_resolve2_method: bool,
        handle: u32,
    ) -> Promise<()> {
        self.note(format!(
            "registerConfigProvider:{type}:trigger={trigger_kind:?}:provide={has_provide_method}:resolve={has_resolve_method}:resolve2={has_resolve2_method}:handle={handle}"
        ));
        Promise::resolved(())
    }

    fn rpc_register_debug_adapter_descriptor_factory(&self, r#type: &str, handle: u32) -> Promise<()> {
        self.note(format!("registerDescriptorFactory:{type}:handle={handle}"));
        Promise::resolved(())
    }

    fn rpc_unregister_debug_configuration_provider(&self, handle: u32) {
        self.note(format!("unregisterConfigProvider:{handle}"));
    }

    fn rpc_unregister_debug_adapter_descriptor_factory(&self, handle: u32) {
        self.note(format!("unregisterDescriptorFactory:{handle}"));
    }

    fn rpc_start_debugging(&self, folder: Option<URI>, name_or_config: NameOrConfig, options: IStartDebuggingOptions) -> Promise<bool> {
        let target = match name_or_config {
            NameOrConfig::Name(name) => format!("name={name}"),
            NameOrConfig::Config(config) => format!("config={config}"),
        };
        self.note(format!(
            "startDebugging:folder={}:{target}:noDebug={:?}:suppressView={:?}:repl={:?}:parent={:?}",
            folder.map(|folder| folder.to_string(false).to_string_lossy()).unwrap_or_default(),
            options.no_debug,
            options.suppress_debug_view,
            options.repl,
            options.parent_session_id,
        ));
        Promise::resolved(true)
    }

    fn rpc_stop_debugging(&self, session_id: Option<&str>) -> Promise<()> {
        self.note(format!("stopDebugging:{session_id:?}"));
        Promise::resolved(())
    }

    fn rpc_set_debug_session_name(&self, id: &str, name: &str) {
        self.note(format!("setSessionName:{id}:{name}"));
    }

    fn rpc_custom_debug_adapter_request(&self, id: &str, command: &str, args: Option<Value>) -> Promise<Option<Value>> {
        self.note(format!("customRequest:{id}:{command}:{}", args.clone().unwrap_or(Value::Null)));
        Promise::resolved(Some(json!({ "command": command, "args": args })))
    }

    fn rpc_get_debug_protocol_breakpoint(&self, _id: &str, _breakpoint_id: &str) -> Promise<Option<Value>> {
        panic!("o teste não exercita $getDebugProtocolBreakpoint");
    }

    fn rpc_append_debug_console(&self, value: &str) {
        self.note(format!("appendDebugConsole:{value:?}"));
    }

    fn rpc_register_breakpoints(&self, breakpoints: Vec<RegisterBreakpointDto>) -> Promise<()> {
        self.note(format!("registerBreakpoints:{}", breakpoints.iter().map(describe_registered).collect::<Vec<_>>().join(";")));
        Promise::resolved(())
    }

    fn rpc_unregister_breakpoints(&self, breakpoint_ids: Vec<String>, function_breakpoint_ids: Vec<String>, data_breakpoint_ids: Vec<String>) -> Promise<()> {
        self.note(format!("unregisterBreakpoints:{}:{}:{}", breakpoint_ids.len(), function_breakpoint_ids.len(), data_breakpoint_ids.len()));
        Promise::resolved(())
    }

    fn rpc_register_debug_visualizer(&self, _extension_id: &str, _id: &str) {
        panic!("o teste não exercita $registerDebugVisualizer");
    }

    fn rpc_unregister_debug_visualizer(&self, _extension_id: &str, _id: &str) {
        panic!("o teste não exercita $unregisterDebugVisualizer");
    }

    fn rpc_register_debug_visualizer_tree(&self, _tree_id: &str, _can_edit: bool) {
        panic!("o teste não exercita $registerDebugVisualizerTree");
    }

    fn rpc_unregister_debug_visualizer_tree(&self, _tree_id: &str) {
        panic!("o teste não exercita $unregisterDebugVisualizerTree");
    }
}

/// Liga o dublê da thread principal ao host.
struct Binder {
    panel: Rc<Panel>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.panel.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_DEBUG_SERVICE));
        let panel: Rc<dyn MainThreadDebugServiceShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_DEBUG_SERVICE, panel);
        let clients: Vec<Rc<dyn IDisposable>> = vec![self.panel.clone()];
        clients
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Panel>) {
    let panel = Panel::new();
    (Rc::new(Binder { panel: panel.clone() }), panel)
}

/// O `contributes` de uma extensão que define o depurador `mock` (só a contribuição com `label` "define" o tipo).
fn mock_debugger_contribution() -> Option<ExtensionContributions> {
    Some(ExtensionContributions {
        debuggers: Some(vec![DebuggerContribution { label: Some("Mock".to_owned()), r#type: Some("mock".to_owned()), ..DebuggerContribution::default() }]),
        ..ExtensionContributions::default()
    })
}

fn run(workbench: &Workbench, command: &str) -> String {
    match workbench.execute(command, Vec::new()) {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("o comando {command} devolve texto, veio {other:?}"),
    }
}

fn global(workbench: &Workbench, name: &str) -> String {
    workbench.pump();
    workbench.probe(&format!("JSON.stringify(globalThis.{name})"))
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// O resultado de uma chamada do workbench ao ExtHost, depois de esvaziar o laço de eventos.
fn settle<T: Clone + 'static>(workbench: &Workbench, promise: &Promise<T>, what: &str) -> T {
    workbench.pump();
    settled_ok(promise, what)
}

/// `{ id, type: 'mock', name, configuration: { type: 'mock', request: 'launch', name } }`.
fn session_dto(id: &str, name: &str) -> IDebugSessionDto {
    IDebugSessionDto::Full(IDebugSessionFullDto {
        id: id.to_owned(),
        r#type: "mock".to_owned(),
        name: name.to_owned(),
        parent: None,
        folder_uri: None,
        configuration: json!({ "type": "mock", "request": "launch", "name": name }),
    })
}

fn session_id(id: &str) -> IDebugSessionDto {
    IDebugSessionDto::Id(id.to_owned())
}

fn breakpoint_base(id: &str, condition: Option<&str>) -> IBreakpointDto {
    IBreakpointDto { id: Some(id.to_owned()), enabled: true, condition: condition.map(str::to_owned), ..IBreakpointDto::default() }
}

#[test]
fn configuration_provider_registers_answers_the_workbench_and_unregisters() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.registerProvider"), "registrado");
        assert_eq!(
            panel.entries("registerConfigProvider"),
            strings(&["registerConfigProvider:mock:trigger=Dynamic:provide=true:resolve=true:resolve2=false:handle=0"]),
            "os membros que o provedor tem viram as flags do registro, e o triggerKind vem do terceiro argumento"
        );

        let ext_host = panel.ext_host();
        let provided = ext_host.rpc_provide_debug_configurations(0, None, CancellationToken::None);
        assert_eq!(settle(workbench, &provided, "$provideDebugConfigurations"), vec![json!({ "type": "mock", "request": "launch", "name": "Gerada", "program": "${file}" })]);
        assert_eq!(global(workbench, "__provided"), r#"[true,"boolean"]"#, "sem pasta o provedor recebe undefined e o token vivo");

        let config: IConfig = json!({ "type": "mock", "request": "launch", "name": "Rodar" });
        let resolved = ext_host.rpc_resolve_debug_configuration(0, None, config, CancellationToken::None);
        assert_eq!(
            settle(workbench, &resolved, "$resolveDebugConfiguration"),
            Some(json!({ "type": "mock", "request": "launch", "name": "Rodar", "resolvedBy": "extensão" })),
            "a configuração que o provedor muta e devolve volta ao workbench"
        );

        let missing = ext_host.rpc_resolve_debug_configuration_with_substituted_variables(0, None, json!({}), CancellationToken::None);
        workbench.pump();
        assert!(missing.result().is_some_and(|result| result.is_err()), "o provedor não tem resolveDebugConfigurationWithSubstitutedVariables");

        assert_eq!(run(workbench, "t.disposeProvider"), "descartado");
        assert_eq!(panel.entries("unregisterConfigProvider"), strings(&["unregisterConfigProvider:0"]));
        let after = ext_host.rpc_provide_debug_configurations(0, None, CancellationToken::None);
        workbench.pump();
        assert!(after.result().is_some_and(|result| result.is_err()), "descartado o provedor, o handle deixa de existir");
    });
}

#[test]
fn adapter_descriptor_factory_answers_the_workbench_for_the_debugger_the_extension_defines() {
    run_extension_contributing(EXTENSION_SOURCE, mock_debugger_contribution(), setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.registerFactory"), "registrado");
        assert_eq!(panel.entries("registerDescriptorFactory"), strings(&["registerDescriptorFactory:mock:handle=0"]));
        assert!(
            panel.entries("registerDebugTypes").contains(&"registerDebugTypes:mock".to_owned()),
            "o depurador que a extensão contribui é registrado no workbench"
        );

        assert_eq!(
            run(workbench, "t.registerFactory"),
            "erro:a DebugAdapterDescriptorFactory can only be registered once per a type.",
            "uma segunda fábrica para o mesmo tipo é recusada"
        );
        assert_eq!(
            run(workbench, "t.registerForeignFactory"),
            "erro:a DebugAdapterDescriptorFactory can only be registered from the extension that defines the 'outro' debugger.",
            "só a extensão que define o depurador registra a fábrica dele"
        );

        let descriptor = panel.ext_host().rpc_provide_debug_adapter(0, session_dto("s1", "Primeira"));
        assert_eq!(
            settle(workbench, &descriptor, "$provideDebugAdapter"),
            IAdapterDescriptor::Executable(IDebugAdapterExecutable {
                command: "mock-adapter".to_owned(),
                args: vec!["--stdio".to_owned()],
                options: Some(IDebugAdapterExecutableOptions { cwd: Some("/w".to_owned()), env: None }),
            }),
            "o DebugAdapterExecutable que a fábrica devolve chega ao workbench como descritor"
        );
        assert_eq!(global(workbench, "__descriptor"), r#""s1:mock:Primeira:true""#, "a fábrica recebe a sessão e o executável do package.json (nenhum aqui)");
        assert_eq!(panel.entries("sessionCached"), strings(&["sessionCached:s1"]));

        assert_eq!(run(workbench, "t.disposeFactory"), "descartado");
        assert_eq!(panel.entries("unregisterDescriptorFactory"), strings(&["unregisterDescriptorFactory:0"]));
    });
}

#[test]
fn factory_registration_is_refused_when_no_extension_defines_the_debugger() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(
            run(workbench, "t.registerFactory"),
            "erro:a DebugAdapterDescriptorFactory can only be registered from the extension that defines the 'mock' debugger."
        );
        assert!(panel.entries("registerDescriptorFactory").is_empty(), "o registro recusado não chega ao workbench");
    });
}

#[test]
fn sessions_events_name_custom_requests_and_console_follow_the_workbench() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.listen"), "ouvindo");
        assert_eq!(run(workbench, "t.activeSession"), "nenhuma");

        let ext_host = panel.ext_host();
        let started = ext_host.rpc_accept_debug_session_started(session_dto("s1", "Primeira"));
        settle(workbench, &started, "$acceptDebugSessionStarted");
        let active = ext_host.rpc_accept_debug_session_active_changed(Some(session_id("s1")));
        settle(workbench, &active, "$acceptDebugSessionActiveChanged");
        assert_eq!(run(workbench, "t.activeSession"), "s1:Primeira");

        assert_eq!(run(workbench, "t.rename"), "Renomeada", "a atribuição de name vale na leitura seguinte");
        assert_eq!(panel.entries("setSessionName"), strings(&["setSessionName:s1:Renomeada"]), "e avisa a thread principal");
        let renamed = ext_host.rpc_accept_debug_session_name_changed(session_id("s1"), "Outra");
        settle(workbench, &renamed, "$acceptDebugSessionNameChanged");
        assert_eq!(run(workbench, "t.activeSession"), "s1:Outra", "o nome que o workbench muda aparece na sessão");

        assert_eq!(run(workbench, "t.customRequest"), r#"{"command":"threads","args":{"all":true}}"#);
        assert_eq!(panel.entries("customRequest"), strings(&[r#"customRequest:s1:threads:{"all":true}"#]));

        assert_eq!(run(workbench, "t.console"), "ok");
        assert_eq!(panel.entries("appendDebugConsole"), strings(&[r#"appendDebugConsole:"a""#, r#"appendDebugConsole:"b\n""#]), "appendLine acrescenta a quebra de linha");

        let custom = ext_host.rpc_accept_debug_session_custom_event(session_id("s1"), json!({ "event": "hot", "body": { "n": 1 } }));
        settle(workbench, &custom, "$acceptDebugSessionCustomEvent");
        let terminated = ext_host.rpc_accept_debug_session_terminated(session_id("s1"));
        settle(workbench, &terminated, "$acceptDebugSessionTerminated");
        let none = ext_host.rpc_accept_debug_session_active_changed(None);
        settle(workbench, &none, "$acceptDebugSessionActiveChanged");
        assert_eq!(run(workbench, "t.activeSession"), "nenhuma");

        assert_eq!(
            global(workbench, "__events"),
            r#"["start:s1:mock:Primeira:launch","active:s1","custom:s1:hot:{\"n\":1}","terminate:s1","active:nenhuma"]"#
        );
    });
}

#[test]
fn start_and_stop_debugging_reach_the_workbench_with_the_options() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(workbench.execute("t.start", Vec::new()), Dynamic::Bool(true));
        assert_eq!(workbench.execute("t.startByName", Vec::new()), Dynamic::Bool(true));
        assert_eq!(
            panel.entries("startDebugging"),
            strings(&[
                r#"startDebugging:folder=file:///w:config={"type":"mock","request":"launch","name":"Rodar","port":9229}:noDebug=Some(true):suppressView=Some(true):repl=Some(MergeWithParent):parent=None"#,
                "startDebugging:folder=:name=Rodar:noDebug=None:suppressView=None:repl=Some(Separate):parent=None",
            ]),
            "a pasta, a configuração e as opções atravessam; sem consoleMode o console é separado"
        );

        let ext_host = panel.ext_host();
        let started = ext_host.rpc_accept_debug_session_started(session_dto("s1", "Pai"));
        settle(workbench, &started, "$acceptDebugSessionStarted");
        let active = ext_host.rpc_accept_debug_session_active_changed(Some(session_id("s1")));
        settle(workbench, &active, "$acceptDebugSessionActiveChanged");

        assert_eq!(workbench.execute("t.startChild", Vec::new()), Dynamic::Bool(true));
        assert!(
            panel.entries("startDebugging").last().is_some_and(|line| line.ends_with(r#":parent=Some("s1")"#)),
            "a sessão como terceiro argumento é a sessão pai"
        );

        assert_eq!(run(workbench, "t.stopAll"), "parou");
        assert_eq!(run(workbench, "t.stopActive"), "parou");
        assert_eq!(panel.entries("stopDebugging"), strings(&["stopDebugging:None", r#"stopDebugging:Some("s1")"#]));
    });
}

#[test]
fn breakpoints_are_registered_listed_removed_and_follow_the_workbench_delta() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.listen"), "ouvindo");
        assert_eq!(run(workbench, "t.breakpoints"), "[]");

        assert_eq!(
            run(workbench, "t.addBreakpoints"),
            r#"["source:/w/a.rs:4:true:x > 1:valor {x}","function:main:false"]"#,
            "os breakpoints criados pela extensão aparecem em debug.breakpoints com as propriedades de cada classe"
        );
        assert_eq!(
            panel.entries("registerBreakpoints"),
            strings(&[r#"registerBreakpoints:source:file:///w/a.rs:4@0:Some("x > 1");function:main:false"#]),
            "os de código-fonte agrupam por arquivo, os de função vão um a um"
        );

        assert_eq!(run(workbench, "t.removeFirstBreakpoint"), r#"["function:main:false"]"#);
        assert_eq!(panel.entries("unregisterBreakpoints"), strings(&["unregisterBreakpoints:1:0:0"]));

        let ext_host = panel.ext_host();
        ext_host.rpc_accept_breakpoints_delta(IBreakpointsDeltaDto {
            added: Some(vec![
                BreakpointDto::Function(IFunctionBreakpointDto { base: breakpoint_base("wb-1", None), function_name: "workbench_fn".to_owned() }),
                BreakpointDto::Source(ISourceBreakpointDto {
                    base: breakpoint_base("wb-2", Some("i == 3")),
                    uri: URI::file("/w/b.rs", je_platform::resources::host_platform()),
                    line: 9,
                    character: 0,
                }),
            ]),
            removed: None,
            changed: None,
        });
        assert_eq!(
            run(workbench, "t.breakpoints"),
            r#"["function:main:false","function:workbench_fn:true","source:/w/b.rs:9:true:i == 3:undefined"]"#,
            "o delta do workbench acrescenta os breakpoints dele"
        );
        ext_host.rpc_accept_breakpoints_delta(IBreakpointsDeltaDto { added: None, removed: Some(vec!["wb-1".to_owned()]), changed: None });
        assert_eq!(run(workbench, "t.breakpoints"), r#"["function:main:false","source:/w/b.rs:9:true:i == 3:undefined"]"#);

        assert_eq!(
            global(workbench, "__events"),
            r#"["breakpoints:2/0/0","breakpoints:0/1/0","breakpoints:2/0/0","breakpoints:0/1/0"]"#,
            "cada mudança dispara onDidChangeBreakpoints com o que entrou, saiu e mudou"
        );
    });
}

#[test]
fn active_stack_item_follows_the_workbench_and_the_stack_classes_are_constructible() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.listenStack"), "ouvindo");
        assert_eq!(run(workbench, "t.stack"), "nenhum");

        let ext_host = panel.ext_host();
        let started = ext_host.rpc_accept_debug_session_started(session_dto("s1", "Primeira"));
        settle(workbench, &started, "$acceptDebugSessionStarted");
        let active = ext_host.rpc_accept_debug_session_active_changed(Some(session_id("s1")));
        settle(workbench, &active, "$acceptDebugSessionActiveChanged");

        let thread = ext_host.rpc_accept_stack_frame_focus(Some(StackFocusDto::Thread(IThreadFocusDto { session_id: "s1".to_owned(), thread_id: 7 })));
        settle(workbench, &thread, "$acceptStackFrameFocus");
        assert_eq!(run(workbench, "t.stack"), "thread:s1:7:undefined");
        let frame = ext_host.rpc_accept_stack_frame_focus(Some(StackFocusDto::StackFrame(IStackFrameFocusDto { session_id: "s1".to_owned(), thread_id: 7, frame_id: 3 })));
        settle(workbench, &frame, "$acceptStackFrameFocus");
        assert_eq!(run(workbench, "t.stack"), "frame:s1:7:3");
        let none = ext_host.rpc_accept_stack_frame_focus(None);
        settle(workbench, &none, "$acceptStackFrameFocus");
        assert_eq!(run(workbench, "t.stack"), "nenhum");

        assert_eq!(global(workbench, "__events"), r#"["stack:thread:s1:7:undefined","stack:frame:s1:7:3","stack:nenhum"]"#);
        assert_eq!(run(workbench, "t.newStackItems"), r#"["thread:s1:2:undefined","frame:s1:2:5"]"#, "DebugThread e DebugStackFrame são construíveis");
    });
}

#[test]
fn as_debug_source_uri_builds_the_file_or_the_debug_uri_and_refuses_an_empty_source() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        let ext_host = panel.ext_host();
        let started = ext_host.rpc_accept_debug_session_started(session_dto("s1", "Primeira"));
        settle(workbench, &started, "$acceptDebugSessionStarted");
        let active = ext_host.rpc_accept_debug_session_active_changed(Some(session_id("s1")));
        settle(workbench, &active, "$acceptDebugSessionActiveChanged");
        assert_eq!(
            run(workbench, "t.sourceUri"),
            r#"["file|/w/a.rs|","debug|mem|session=s1&ref=7","debug||ref=9","cannot create uri from DAP 'source' object; properties 'path' and 'sourceReference' are both missing."]"#
        );
    });
}

#[test]
fn data_breakpoint_is_constructible_and_requires_a_data_id() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _panel| {
        assert_eq!(run(workbench, "t.dataBreakpoint"), r#"[true,"var x","id-1",true,false,"x > 1","Illegal argument: dataId"]"#);
    });
}

#[test]
fn tracker_observes_a_session_whose_adapter_is_an_inline_implementation_held_by_reference() {
    run_extension_contributing(EXTENSION_SOURCE, mock_debugger_contribution(), setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.registerInlineAndTracker"), "registrado");
        let ext_host = panel.ext_host();
        let started = ext_host.rpc_start_da_session(5, session_dto("s1", "Primeira"));
        settle(workbench, &started, "$startDASession");

        ext_host.rpc_send_da_message(5, ProtocolMessage::Request(Request { seq: 1, command: "threads".to_owned(), arguments: None }));
        workbench.pump();
        assert_eq!(
            panel.entries("acceptDAMessage"),
            strings(&[r#"acceptDAMessage:5:{"command":"threads","request_seq":1,"seq":2,"success":true,"type":"response"}"#]),
            "a resposta que o adaptador inline dispara chega ao workbench"
        );

        let stopped = ext_host.rpc_stop_da_session(5);
        settle(workbench, &stopped, "$stopDASession");
        let events: Vec<String> = serde_json::from_str(&global(workbench, "__events")).expect("os eventos são um array de textos");
        assert_eq!(
            events[..5],
            strings(&["tracker start s1", "tracker recv threads", "adapter<-threads", "tracker sent response", "tracker stop"])[..],
            "o tracker vê o início, a mensagem recebida, a enviada e o fim, na ordem do original"
        );
        assert_eq!(run(workbench, "t.disposeTracker"), "descartado");
    });
}
