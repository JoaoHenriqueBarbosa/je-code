//! Teste de ponta a ponta de `vscode.window.createTerminal` e do resto da API de terminais (não é porte do oráculo): uma extensão de
//! exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), cria terminais
//! por nome, por opções e por `Pseudoterminal`, lê `terminals` e `activeTerminal`, assina os eventos de abertura, fechamento e
//! terminal ativo, usa `sendText`, `show`, `hide`, `dispose` e `processId`, e registra um provedor de perfil de terminal.
//!
//! O lado do ExtHost é o `ExtHostTerminalService` REAL (portado de `extHostTerminalService.ts`), ligado pelo host em processo. O lado
//! do workbench é um dublê do `MainThreadTerminalServiceShape` ([`Renderer`]) que faz o papel da thread principal: guarda o que a
//! extensão pediu (`$createTerminal`, `$sendText`, `$show`, `$hide`, `$dispose`, `$registerProfileProvider`, `$sendProcessData`...)
//! e, quando o teste manda, devolve ao ExtHost o que o `MainThreadTerminalService` real devolveria (`$acceptTerminalOpened`,
//! `$acceptActiveTerminalChanged`, `$startExtensionTerminal`, `$acceptProcessInput`, `$acceptTerminalClosed`...). O cliente real
//! (`main_thread_terminal_service.rs`) pede doze serviços do terminal do workbench, sem dublês prontos nesta suíte; o contrato que o
//! dublê imita é o do próprio `MainThreadTerminalServiceShape`.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use harness::{run_extension, Workbench};
use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::{Promise, Resolver};
use je_exthost::in_process::IMainThreadBinder;
use je_platform::terminal::environment_variable::{ISerializableEnvironmentDescriptionMap, ISerializableEnvironmentVariableCollection};
use je_platform::terminal::terminal::{IProcessProperty, IProcessReadyWindowsPty, ShellCwd, TerminalEnvironmentValue, TerminalExitReason as PlatformExitReason};
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostTerminalIdentifier, ExtHostTerminalServiceShape, ExtHostTerminalShellIntegrationShape, ICreateContributedTerminalProfileOptions, IMainContext,
    IShellLaunchConfigDto, ITerminalDimensionsDto, MainContext, MainThreadTerminalServiceShape, MainThreadTerminalShellIntegrationShape, TerminalLaunchConfig,
    TerminalLaunchLocation,
};
use je_workbench::api::common::ext_host_types::TerminalShellExecutionCommandLineConfidence;

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const subscribe = (disposable) => context.subscriptions.push(disposable);
  const command = (id, handler) => subscribe(vscode.commands.registerCommand(id, handler));

  subscribe(vscode.window.onDidOpenTerminal((terminal) => log('open:' + terminal.name)));
  subscribe(vscode.window.onDidCloseTerminal((terminal) => {
    const status = terminal.exitStatus;
    log('close:' + terminal.name + ':' + (status ? status.code + '/' + status.reason : 'none'));
  }));
  subscribe(vscode.window.onDidChangeActiveTerminal((terminal) => log('active:' + (terminal ? terminal.name : 'none'))));

  globalThis.__shell = [];
  const shellLog = (text) => globalThis.__shell.push(text);
  subscribe(vscode.window.onDidChangeTerminalShellIntegration((e) => {
    shellLog('si:' + e.terminal.name + ':' + (e.shellIntegration.cwd ? e.shellIntegration.cwd.fsPath : 'none'));
  }));
  subscribe(vscode.window.onDidStartTerminalShellExecution(async (e) => {
    const line = e.execution.commandLine;
    shellLog('start:' + line.value + ':' + line.confidence + ':' + line.isTrusted + ':' + (e.execution.cwd ? e.execution.cwd.fsPath : 'none'));
    shellLog('match:' + (globalThis.__exec !== undefined && e.execution._id === globalThis.__exec._id));
    let data = '';
    for await (const chunk of e.execution.read()) { data += chunk; }
    shellLog('read:' + data);
  }));
  subscribe(vscode.window.onDidEndTerminalShellExecution((e) => {
    shellLog('end:' + e.execution.commandLine.value + ':' + e.exitCode + ':' + e.terminal.name);
  }));
  command('t.shellState', () => {
    const integration = globalThis.__plain.shellIntegration;
    if (!integration) { return 'none'; }
    return JSON.stringify({ cwd: integration.cwd ? integration.cwd.fsPath : null, env: integration.env });
  });
  command('t.createSecond', () => {
    globalThis.__second = vscode.window.createTerminal('second');
    return globalThis.__second.name;
  });
  command('t.executeForm', (form) => {
    const integration = (form === 'second' ? globalThis.__second : globalThis.__plain).shellIntegration;
    try {
      globalThis.__exec = form === 'args'
        ? integration.executeCommand('echo', ['a b', 'c', "d'e"])
        : integration.executeCommand(form === 'second' ? 'pwd' : 'ls -l');
      return globalThis.__exec.commandLine.value + '|' + globalThis.__exec.commandLine.confidence + '|' + globalThis.__exec.commandLine.isTrusted;
    } catch (error) {
      return error.message;
    }
  });
  command('t.proposedShell', () => {
    const attempt = (call) => { try { call(); return 'sem erro'; } catch (error) { return error.message; } };
    return JSON.stringify({
      command: attempt(() => vscode.window.onDidExecuteTerminalCommand(() => {})),
      quickFix: attempt(() => vscode.window.registerTerminalQuickFixProvider('fix', {})),
    });
  });

  command('t.createByName', () => {
    globalThis.__plain = vscode.window.createTerminal('plain', '/bin/bash', ['-i']);
    return globalThis.__plain.name;
  });
  command('t.createByOptions', () => {
    globalThis.__opts = vscode.window.createTerminal({ name: 'opts', shellPath: '/bin/zsh', shellArgs: ['-l'], cwd: '/proj', env: { A: '1', B: null } });
    return JSON.stringify(globalThis.__opts.creationOptions);
  });
  command('t.list', () => JSON.stringify({
    names: vscode.window.terminals.map((terminal) => terminal.name),
    active: vscode.window.activeTerminal ? vscode.window.activeTerminal.name : 'none',
  }));
  command('t.use', () => {
    globalThis.__plain.sendText('echo hi');
    globalThis.__plain.sendText('raw', false);
    globalThis.__plain.show(true);
    globalThis.__plain.hide();
    return 'ok';
  });
  command('t.processId', async () => String(await globalThis.__plain.processId));
  command('t.dispose', () => {
    globalThis.__plain.dispose();
    try {
      globalThis.__plain.sendText('depois');
      return 'sem erro';
    } catch (error) {
      return error.message;
    }
  });
  command('t.pty', () => {
    class Pty {
      constructor() {
        this.write = new vscode.EventEmitter();
        this.closed = new vscode.EventEmitter();
        this.onDidWrite = this.write.event;
        this.onDidClose = this.closed.event;
        this.input = '';
      }
      open(dimensions) {
        globalThis.__ptyOpened = dimensions ? dimensions.columns + 'x' + dimensions.rows : 'none';
        this.write.fire('hello ' + (dimensions ? dimensions.columns : 'none'));
      }
      close() { globalThis.__ptyClosed = true; }
      handleInput(data) {
        this.input += data;
        globalThis.__input = this.input;
        if (data === 'q') { this.closed.fire(7); }
      }
    }
    globalThis.__pty = vscode.window.createTerminal({ name: 'pty term', pty: new Pty() });
    return globalThis.__pty.name;
  });
  command('t.registerProfile', () => {
    subscribe(vscode.window.registerTerminalProfileProvider('je.profile', {
      provideTerminalProfile(token) {
        return new vscode.TerminalProfile({ name: 'from profile', shellPath: '/bin/fish' });
      },
    }));
    return 'registered';
  });
  command('t.createLocated', () => {
    globalThis.__located = vscode.window.createTerminal({ name: 'located', location: vscode.TerminalLocation.Editor, iconPath: new vscode.ThemeIcon('terminal') });
    return globalThis.__located.name;
  });
  command('t.createEditorColumn', () => {
    vscode.window.createTerminal({ name: 'column', location: { viewColumn: vscode.ViewColumn.Two, preserveFocus: true } });
    return 'ok';
  });
  command('t.createSplit', () => {
    vscode.window.createTerminal({ name: 'child', location: { parentTerminal: globalThis.__plain } });
    return 'ok';
  });
  command('t.proposed', () => {
    const read = (name) => {
      try { vscode.window[name](() => {}); return 'sem erro'; } catch (error) { return error.message; }
    };
    return JSON.stringify({ dimensions: read('onDidChangeTerminalDimensions'), data: read('onDidWriteTerminalData') });
  });
  command('t.registerLinks', () => {
    globalThis.__linked = [];
    subscribe(vscode.window.registerTerminalLinkProvider({
      provideTerminalLinks(context, token) {
        globalThis.__linkContext = context.terminal.name + '|' + context.line;
        return [new vscode.TerminalLink(2, 3, 'dica')];
      },
      handleTerminalLink(link) {
        globalThis.__linked.push(link.startIndex + ':' + link.length + ':' + link.tooltip);
      },
    }));
    return 'registered';
  });
  command('t.badLink', () => {
    try { new vscode.TerminalLink(-1, 3); return 'sem erro'; } catch (error) { return error.message; }
  });
};
"#;

/// O que o dublê guarda de cada `$createTerminal` que a extensão pediu.
#[derive(Clone, Debug, PartialEq)]
struct Created {
    name: Option<String>,
    shell_path: Option<String>,
    shell_args: String,
    cwd: Option<String>,
    env: Vec<(String, TerminalEnvironmentValue)>,
    is_pty: bool,
}

struct Pending {
    uuid: String,
    created: Created,
    resolver: Resolver<()>,
}

/// O papel da thread principal: o `MainThreadTerminalServiceShape` que anota os pedidos e deixa o teste devolver os eventos.
struct Renderer {
    ext_host: RefCell<Option<Rc<dyn ExtHostTerminalServiceShape>>>,
    next_id: Cell<u32>,
    pending: RefCell<Vec<Pending>>,
    /// Os pedidos da extensão, como texto (`sendText:1:echo hi:true`, `show:1:true`, `hide:1`, `dispose:1`).
    calls: RefCell<Vec<String>>,
    requested: RefCell<Vec<Created>>,
    /// O local e o ícone de cada `$createTerminal` (`location:editor`, `icon`, `parent:1`...).
    launches: RefCell<Vec<String>>,
    registered_profiles: RefCell<Vec<(String, String)>>,
    process_data: RefCell<Vec<(u32, String)>>,
    process_ready: RefCell<Vec<(u32, i32)>>,
    process_exits: RefCell<Vec<(u32, Option<i32>)>>,
    process_support: Cell<Option<bool>>,
    /// O `ExtHostTerminalShellIntegration` do outro lado (o proxy que o `MainThreadTerminalShellIntegration` real pede no construtor).
    shell_ext_host: RefCell<Option<Rc<dyn ExtHostTerminalShellIntegrationShape>>>,
    /// Os `$executeCommand` que a extensão pediu (`execute:1:ls -l`).
    shell_calls: RefCell<Vec<String>>,
}

impl Renderer {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            ext_host: RefCell::new(None),
            next_id: Cell::new(1),
            pending: RefCell::new(Vec::new()),
            calls: RefCell::new(Vec::new()),
            requested: RefCell::new(Vec::new()),
            launches: RefCell::new(Vec::new()),
            registered_profiles: RefCell::new(Vec::new()),
            process_data: RefCell::new(Vec::new()),
            process_ready: RefCell::new(Vec::new()),
            process_exits: RefCell::new(Vec::new()),
            process_support: Cell::new(None),
            shell_ext_host: RefCell::new(None),
            shell_calls: RefCell::new(Vec::new()),
        })
    }

    fn shell(&self) -> Rc<dyn ExtHostTerminalShellIntegrationShape> {
        self.shell_ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    /// O `ExtHostTerminalService` do outro lado (o proxy que o `MainThreadTerminalService` real pede no construtor).
    fn ext_host(&self) -> Rc<dyn ExtHostTerminalServiceShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    /// O `MainThreadTerminalService` cria os terminais pedidos: avisa o ExtHost de que abriram (`$acceptTerminalOpened`), do terminal
    /// ativo (`$acceptActiveTerminalChanged`) e, para os que têm processo, do pid (`$acceptTerminalProcessId`); depois resolve o
    /// `$createTerminal`. Devolve os ids dos terminais abertos, na ordem.
    fn open_pending(&self) -> Vec<u32> {
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        let mut ids = Vec::new();
        for entry in pending {
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            let name = entry.created.name.clone().unwrap_or_default();
            let ext_host = self.ext_host();
            ext_host.rpc_accept_terminal_opened(id, Some(&entry.uuid), &name, IShellLaunchConfigDto::default());
            drop(ext_host.rpc_accept_active_terminal_changed(Some(id)));
            if !entry.created.is_pty {
                drop(ext_host.rpc_accept_terminal_process_id(id, 4242));
            }
            entry.resolver.resolve(());
            ids.push(id);
        }
        ids
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl IDisposable for Renderer {
    fn dispose(&self) {}
}

fn id_text(id: &ExtHostTerminalIdentifier) -> String {
    match id {
        ExtHostTerminalIdentifier::Id(id) => id.to_string(),
        ExtHostTerminalIdentifier::Uuid(uuid) => uuid.clone(),
    }
}

impl MainThreadTerminalServiceShape for Renderer {
    fn rpc_create_terminal(&self, ext_host_terminal_id: &str, config: TerminalLaunchConfig) -> Promise<()> {
        let shell_args = match &config.shell_args {
            Some(OneOrMany::Many(args)) => args.join(","),
            Some(OneOrMany::One(arg)) => arg.clone(),
            None => String::new(),
        };
        let cwd = config.cwd.as_ref().map(|cwd| match cwd {
            ShellCwd::Path(path) => path.clone(),
            ShellCwd::Uri(uri) => uri.to_string(false).to_string_lossy(),
        });
        let created = Created {
            name: config.name.clone(),
            shell_path: config.shell_path.clone(),
            shell_args,
            cwd,
            env: config.env.clone().unwrap_or_default(),
            is_pty: config.is_extension_custom_pty_terminal.unwrap_or(false),
        };
        self.requested.borrow_mut().push(created.clone());
        let location = match &config.location {
            Some(TerminalLaunchLocation::Location(location)) => format!("location:{}", *location as i32),
            Some(TerminalLaunchLocation::ViewColumn { preserve_focus, .. }) => format!("column:preserveFocus={preserve_focus:?}"),
            Some(TerminalLaunchLocation::ParentTerminal(parent)) => format!("parent:{}", id_text(parent)),
            Some(TerminalLaunchLocation::SplitActiveTerminal(split)) => format!("split:{split}"),
            None => "no-location".to_owned(),
        };
        self.launches.borrow_mut().push(format!("{location}:{}", if config.icon.is_some() { "icon" } else { "no-icon" }));
        let (promise, resolver) = Promise::<()>::new();
        self.pending.borrow_mut().push(Pending { uuid: ext_host_terminal_id.to_owned(), created, resolver });
        promise
    }

    fn rpc_dispose(&self, id: &ExtHostTerminalIdentifier) {
        self.calls.borrow_mut().push(format!("dispose:{}", id_text(id)));
    }

    fn rpc_hide(&self, id: &ExtHostTerminalIdentifier) {
        self.calls.borrow_mut().push(format!("hide:{}", id_text(id)));
    }

    fn rpc_send_text(&self, id: &ExtHostTerminalIdentifier, text: &str, should_execute: bool) {
        self.calls.borrow_mut().push(format!("sendText:{}:{text}:{should_execute}", id_text(id)));
    }

    fn rpc_show(&self, id: &ExtHostTerminalIdentifier, preserve_focus: bool) {
        self.calls.borrow_mut().push(format!("show:{}:{preserve_focus}", id_text(id)));
    }

    fn rpc_register_process_support(&self, is_supported: bool) {
        self.process_support.set(Some(is_supported));
    }

    fn rpc_register_profile_provider(&self, id: &str, extension_identifier: &str) {
        self.registered_profiles.borrow_mut().push((id.to_owned(), extension_identifier.to_owned()));
    }

    fn rpc_unregister_profile_provider(&self, _id: &str) {}

    fn rpc_register_completion_provider(&self, _id: &str, _extension_identifier: &str, _trigger_characters: &[String]) {}

    fn rpc_unregister_completion_provider(&self, _id: &str) {}

    fn rpc_register_quick_fix_provider(&self, _id: &str, _extension_identifier: &str) {}

    fn rpc_unregister_quick_fix_provider(&self, _id: &str) {}

    fn rpc_set_environment_variable_collection(
        &self,
        _extension_identifier: &str,
        _persistent: bool,
        _collection: Option<ISerializableEnvironmentVariableCollection>,
        _description_map: ISerializableEnvironmentDescriptionMap,
    ) {
    }

    fn rpc_start_sending_data_events(&self) {}

    fn rpc_stop_sending_data_events(&self) {}

    fn rpc_start_sending_command_events(&self) {}

    fn rpc_stop_sending_command_events(&self) {}

    fn rpc_start_link_provider(&self) {}

    fn rpc_stop_link_provider(&self) {}

    fn rpc_send_process_data(&self, terminal_id: u32, data: &str) {
        self.process_data.borrow_mut().push((terminal_id, data.to_owned()));
    }

    fn rpc_send_process_ready(&self, terminal_id: u32, pid: i32, _cwd: &str, _windows_pty: Option<IProcessReadyWindowsPty>) {
        self.process_ready.borrow_mut().push((terminal_id, pid));
    }

    fn rpc_send_process_property(&self, _terminal_id: u32, _property: IProcessProperty) {}

    fn rpc_send_process_exit(&self, terminal_id: u32, exit_code: Option<i32>) {
        self.process_exits.borrow_mut().push((terminal_id, exit_code));
    }
}

impl MainThreadTerminalShellIntegrationShape for Renderer {
    fn rpc_execute_command(&self, terminal_id: u32, command_line: &str) {
        self.shell_calls.borrow_mut().push(format!("execute:{terminal_id}:{command_line}"));
    }
}

/// Liga o dublê da thread principal ao host: registra o cliente e entrega a ele o proxy do ExtHost.
struct Binder {
    renderer: Rc<Renderer>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.renderer.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_TERMINAL_SERVICE));
        *self.renderer.shell_ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_TERMINAL_SHELL_INTEGRATION));
        let renderer: Rc<dyn MainThreadTerminalServiceShape> = self.renderer.clone();
        context.set(&MainContext::MAIN_THREAD_TERMINAL_SERVICE, renderer);
        let shell: Rc<dyn MainThreadTerminalShellIntegrationShape> = self.renderer.clone();
        context.set(&MainContext::MAIN_THREAD_TERMINAL_SHELL_INTEGRATION, shell);
        Vec::new()
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Renderer>) {
    let renderer = Renderer::new();
    (Rc::new(Binder { renderer: renderer.clone() }), renderer)
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

/// Deixa os temporizadores de 5 ms do buffer de dados do terminal vencerem e esvazia o laço.
fn settle(workbench: &Workbench) {
    std::thread::sleep(Duration::from_millis(50));
    workbench.pump();
}

/// Abre o que a extensão pediu e esvazia o laço (o `$createTerminal` resolve e o ExtHost liga o processo).
fn open_pending(workbench: &Workbench, renderer: &Renderer) -> Vec<u32> {
    let ids = renderer.open_pending();
    workbench.pump();
    ids
}

#[test]
fn create_terminal_by_name_reaches_the_main_thread_and_opens_back() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        assert_eq!(renderer.process_support.get(), Some(true), "o host em processo cria os processos pelo cliente da thread principal");
        assert_eq!(run(workbench, "t.createByName"), Dynamic::string("plain"));
        assert_eq!(
            *renderer.requested.borrow(),
            vec![Created { name: Some("plain".to_owned()), shell_path: Some("/bin/bash".to_owned()), shell_args: "-i".to_owned(), cwd: None, env: Vec::new(), is_pty: false }]
        );
        assert_eq!(run(workbench, "t.list"), Dynamic::string(r#"{"names":["plain"],"active":"none"}"#), "o terminal só existe para a extensão depois do $acceptTerminalOpened");

        assert_eq!(open_pending(workbench, &renderer), vec![1]);
        assert_eq!(run(workbench, "t.list"), Dynamic::string(r#"{"names":["plain"],"active":"plain"}"#));
        assert_eq!(events(workbench), r#"["open:plain","active:plain"]"#);
        assert_eq!(run(workbench, "t.processId"), Dynamic::string("4242"));
    });
}

#[test]
fn create_terminal_by_options_sends_every_option_and_returns_them_as_creation_options() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        assert_eq!(
            run(workbench, "t.createByOptions"),
            Dynamic::string(r#"{"name":"opts","shellPath":"/bin/zsh","shellArgs":["-l"],"cwd":"/proj","env":{"A":"1","B":null}}"#)
        );
        assert_eq!(
            *renderer.requested.borrow(),
            vec![Created {
                name: Some("opts".to_owned()),
                shell_path: Some("/bin/zsh".to_owned()),
                shell_args: "-l".to_owned(),
                cwd: Some("/proj".to_owned()),
                env: vec![("A".to_owned(), TerminalEnvironmentValue::Value("1".to_owned())), ("B".to_owned(), TerminalEnvironmentValue::Null)],
                is_pty: false,
            }]
        );
    });
}

#[test]
fn terminal_methods_reach_the_main_thread_and_dispose_blocks_further_use() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        run(workbench, "t.createByName");
        open_pending(workbench, &renderer);

        assert_eq!(run(workbench, "t.use"), Dynamic::string("ok"));
        assert_eq!(renderer.calls(), vec!["sendText:1:echo hi:true", "sendText:1:raw:false", "show:1:true", "hide:1"]);

        assert_eq!(run(workbench, "t.dispose"), Dynamic::string("Terminal has already been disposed"));
        assert_eq!(renderer.calls().last().map(String::as_str), Some("dispose:1"));

        // O fechamento vem da thread principal, com o código de saída: o evento entrega o terminal já com o `exitStatus`.
        let closed = renderer.ext_host().rpc_accept_terminal_closed(1, Some(0), PlatformExitReason::Process);
        workbench.pump();
        assert!(matches!(closed.result(), Some(Ok(()))), "o fechamento assenta");
        assert_eq!(events(workbench), r#"["open:plain","active:plain","close:plain:0/2"]"#);
        assert_eq!(run(workbench, "t.list"), Dynamic::string(r#"{"names":[],"active":"plain"}"#));
    });
}

#[test]
fn extension_pseudoterminal_opens_writes_receives_input_and_closes() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        assert_eq!(run(workbench, "t.pty"), Dynamic::string("pty term"));
        assert_eq!(renderer.requested.borrow().len(), 1);
        assert!(renderer.requested.borrow()[0].is_pty, "o terminal de extensão pede o pty customizado");

        let ids = open_pending(workbench, &renderer);
        assert_eq!(ids, vec![1]);
        assert_eq!(workbench.probe("String(globalThis.__ptyOpened)"), "undefined", "o pty só abre quando a thread principal manda iniciar");

        // `$startExtensionTerminal`: a thread principal pede o início com as dimensões iniciais.
        let started = renderer.ext_host().rpc_start_extension_terminal(1, Some(ITerminalDimensionsDto { columns: 80, rows: 24 }));
        settle(workbench);
        assert!(matches!(started.result(), Some(Ok(None))), "sem erro de lançamento");
        assert_eq!(workbench.probe("String(globalThis.__ptyOpened)"), "80x24");
        assert_eq!(*renderer.process_ready.borrow(), vec![(1, -1)]);
        assert_eq!(*renderer.process_data.borrow(), vec![(1, "hello 80".to_owned())], "o que o pty escreve chega à thread principal");

        // `$acceptProcessInput`: o que o usuário digita chega ao `handleInput` do pty, com o `this` do objeto da extensão.
        renderer.ext_host().rpc_accept_process_input(1, "ls\r");
        workbench.pump();
        assert_eq!(workbench.probe("String(globalThis.__input)"), "ls\r");

        // `onDidClose` do pty vira a saída do processo.
        renderer.ext_host().rpc_accept_process_input(1, "q");
        workbench.pump();
        assert_eq!(*renderer.process_exits.borrow(), vec![(1, Some(7))]);

        // O `onDidClose` do pty já encerrou o processo: o ExtHost o esquece (`_onProcessExit` apaga a referência), então um
        // `$acceptProcessShutdown` depois da saída não chama o `close` do pty (o `?.shutdown` do original não acha o processo).
        renderer.ext_host().rpc_accept_process_shutdown(1, true);
        workbench.pump();
        assert_eq!(workbench.probe("String(globalThis.__ptyClosed)"), "undefined");
    });
}

#[test]
fn extension_pseudoterminal_close_runs_when_the_main_thread_shuts_the_terminal_down() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        run(workbench, "t.pty");
        open_pending(workbench, &renderer);
        let started = renderer.ext_host().rpc_start_extension_terminal(1, None);
        settle(workbench);
        assert!(matches!(started.result(), Some(Ok(None))), "sem erro de lançamento");
        assert_eq!(workbench.probe("String(globalThis.__ptyOpened)"), "none", "sem dimensões iniciais o pty abre com undefined");

        // `$acceptProcessShutdown` com o processo vivo: o `close` do pty roda.
        renderer.ext_host().rpc_accept_process_shutdown(1, true);
        workbench.pump();
        assert_eq!(workbench.probe("String(globalThis.__ptyClosed)"), "true");
    });
}

#[test]
fn terminal_profile_provider_registers_and_creates_the_profile_terminal() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        assert_eq!(run(workbench, "t.registerProfile"), Dynamic::string("registered"));
        assert_eq!(*renderer.registered_profiles.borrow(), vec![("je.profile".to_owned(), "je.exthost".to_owned())]);

        let created = renderer.ext_host().rpc_create_contributed_profile_terminal("je.profile", ICreateContributedTerminalProfileOptions::default());
        workbench.pump();
        assert!(matches!(created.result(), Some(Ok(()))), "o provedor devolveu o perfil");
        assert_eq!(
            *renderer.requested.borrow(),
            vec![Created { name: Some("from profile".to_owned()), shell_path: Some("/bin/fish".to_owned()), shell_args: String::new(), cwd: None, env: Vec::new(), is_pty: false }]
        );

        assert_eq!(open_pending(workbench, &renderer), vec![1]);
        assert_eq!(events(workbench), r#"["open:from profile","active:from profile"]"#);
    });
}

#[test]
fn terminal_options_location_and_theme_icon_reach_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        assert_eq!(run(workbench, "t.createLocated"), Dynamic::string("located"));
        assert_eq!(run(workbench, "t.createEditorColumn"), Dynamic::string("ok"));
        assert_eq!(*renderer.launches.borrow(), vec!["location:2:icon", "column:preserveFocus=Some(true):no-icon"]);
    });
}

#[test]
fn terminal_options_parent_terminal_splits_the_parent() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        run(workbench, "t.createByName");
        open_pending(workbench, &renderer);
        assert_eq!(run(workbench, "t.createSplit"), Dynamic::string("ok"));
        assert_eq!(renderer.launches.borrow().last().map(String::as_str), Some("parent:1:no-icon"));
    });
}

#[test]
fn proposed_terminal_events_throw_without_the_proposal() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _renderer| {
        let text = format!("{:?}", run(workbench, "t.proposed"));
        assert!(text.contains("terminalDimensions"), "{text}");
        assert!(text.contains("terminalDataWriteEvent"), "{text}");
        assert!(!text.contains("sem erro"), "{text}");
    });
}

#[test]
fn terminal_link_provider_provides_and_handles_links() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        assert_eq!(run(workbench, "t.registerLinks"), Dynamic::string("registered"));
        run(workbench, "t.createByName");
        open_pending(workbench, &renderer);

        let provided = renderer.ext_host().rpc_provide_links(1, "ab cde");
        settle(workbench);
        let Some(Ok(links)) = provided.result() else { panic!("o provedor devolveu os links") };
        assert_eq!(links.len(), 1);
        assert_eq!((links[0].start_index, links[0].length, links[0].label.as_deref()), (2.0, 3.0, Some("dica")));
        assert_eq!(workbench.probe("String(globalThis.__linkContext)"), "plain|ab cde");

        renderer.ext_host().rpc_activate_link(1, links[0].id);
        workbench.pump();
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__linked)"), r#"["2:3:dica"]"#);
    });
}

#[test]
fn terminal_link_constructor_validates_its_arguments() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _renderer| {
        let message = format!("{:?}", run(workbench, "t.badLink"));
        assert!(message.contains("startIndex"), "{message}");
    });
}

fn run_with(workbench: &Workbench, command: &str, argument: &str) -> Dynamic {
    workbench.execute(command, vec![Dynamic::string(argument)])
}

fn shell_log(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__shell)")
}

/// Abre o terminal `plain` (id 1) e a thread principal anuncia a integração com o shell dele.
fn open_with_shell_integration(workbench: &Workbench, renderer: &Renderer) {
    run(workbench, "t.createByName");
    open_pending(workbench, renderer);
    renderer.shell().rpc_shell_integration_change(1, true);
    workbench.pump();
}

#[test]
fn terminal_shell_integration_appears_and_follows_cwd_and_env() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        run(workbench, "t.createByName");
        open_pending(workbench, &renderer);
        assert_eq!(run(workbench, "t.shellState"), Dynamic::string("none"), "sem $shellIntegrationChange o terminal não tem integração");

        renderer.shell().rpc_shell_integration_change(1, true);
        renderer.shell().rpc_cwd_change(1, Some("/proj"));
        renderer.shell().rpc_shell_env_change(1, &["A".to_owned(), "B".to_owned()], &[Some("1".to_owned()), None], true);
        workbench.pump();
        assert_eq!(shell_log(workbench), r#"["si:plain:none","si:plain:/proj","si:plain:/proj"]"#);
        assert_eq!(run(workbench, "t.shellState"), Dynamic::string(r#"{"cwd":"/proj","env":{"isTrusted":true,"value":{"A":"1"}}}"#));

        // O mesmo diretório não é mudança.
        renderer.shell().rpc_cwd_change(1, Some("/proj"));
        workbench.pump();
        assert_eq!(shell_log(workbench), r#"["si:plain:none","si:plain:/proj","si:plain:/proj"]"#);

        // Depois de fechar o terminal a integração some e os eventos dela são ignorados.
        renderer.shell().rpc_close_terminal(1);
        renderer.shell().rpc_cwd_change(1, Some("/outro"));
        workbench.pump();
        assert_eq!(shell_log(workbench), r#"["si:plain:none","si:plain:/proj","si:plain:/proj"]"#);
    });
}

#[test]
fn shell_execution_started_by_the_shell_streams_data_and_ends_with_the_exit_code() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        open_with_shell_integration(workbench, &renderer);
        renderer.shell().rpc_cwd_change(1, Some("/proj"));

        // `$shellExecutionStart` de um terminal que o usuário digitou: nenhuma execução pendente.
        renderer.shell().rpc_shell_execution_start(1, true, "ls -l", TerminalShellExecutionCommandLineConfidence::High, true, Some("/proj"));
        workbench.pump();
        renderer.shell().rpc_shell_execution_data(1, "a");
        renderer.shell().rpc_shell_execution_data(1, "b");
        workbench.pump();
        renderer.shell().rpc_shell_execution_end(1, "ls -l", TerminalShellExecutionCommandLineConfidence::High, true, Some(3));
        workbench.pump();

        let log = shell_log(workbench);
        for expected in ["start:ls -l:2:true:/proj", "match:false", "read:ab", "end:ls -l:3:plain"] {
            assert!(log.contains(expected), "{expected} em {log}");
        }
    });
}

#[test]
fn execute_command_reaches_the_main_thread_and_the_start_event_matches_the_execution() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        open_with_shell_integration(workbench, &renderer);
        renderer.shell().rpc_cwd_change(1, Some("/proj"));
        workbench.pump();

        // `executeCommand(commandLine)`.
        assert_eq!(run_with(workbench, "t.executeForm", "line"), Dynamic::string("ls -l|2|true"));
        assert_eq!(*renderer.shell_calls.borrow(), vec!["execute:1:ls -l"]);

        // O shell começa a execução da mesma linha: a execução pendente é a que o evento entrega.
        renderer.shell().rpc_shell_execution_start(1, true, "ls -l", TerminalShellExecutionCommandLineConfidence::High, true, None);
        workbench.pump();
        renderer.shell().rpc_shell_execution_end(1, "ls -l", TerminalShellExecutionCommandLineConfidence::High, true, Some(0));
        workbench.pump();
        let log = shell_log(workbench);
        for expected in ["start:ls -l:2:true:/proj", "match:true", "end:ls -l:0:plain"] {
            assert!(log.contains(expected), "{expected} em {log}");
        }

        // `executeCommand(executable, args)`: só o argumento com espaço e sem aspas ganha aspas.
        assert_eq!(run_with(workbench, "t.executeForm", "args"), Dynamic::string("echo \"a b\" c d'e|2|true"));
        assert_eq!(renderer.shell_calls.borrow().last().map(String::as_str), Some("execute:1:echo \"a b\" c d'e"));
    });
}

#[test]
fn execute_command_throws_when_the_terminal_does_not_support_it() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        open_with_shell_integration(workbench, &renderer);
        run(workbench, "t.createSecond");
        assert_eq!(open_pending(workbench, &renderer), vec![2]);
        renderer.shell().rpc_shell_integration_change(2, false);
        workbench.pump();

        assert_eq!(run_with(workbench, "t.executeForm", "second"), Dynamic::string("This terminal does not support the executeCommand API."));
        assert!(renderer.shell_calls.borrow().is_empty(), "o pedido nem sai da extensão");
    });
}

#[test]
fn proposed_terminal_shell_members_throw_without_the_proposal() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _renderer| {
        let text = format!("{:?}", run(workbench, "t.proposedShell"));
        assert!(text.contains("terminalExecuteCommandEvent"), "{text}");
        assert!(text.contains("terminalQuickFixProvider"), "{text}");
        assert!(!text.contains("sem erro"), "{text}");
    });
}
