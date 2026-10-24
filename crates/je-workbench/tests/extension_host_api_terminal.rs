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
    ExtHostContext, ExtHostTerminalIdentifier, ExtHostTerminalServiceShape, ICreateContributedTerminalProfileOptions, IMainContext, IShellLaunchConfigDto, ITerminalDimensionsDto,
    MainContext, MainThreadTerminalServiceShape, TerminalLaunchConfig,
};

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
    registered_profiles: RefCell<Vec<(String, String)>>,
    process_data: RefCell<Vec<(u32, String)>>,
    process_ready: RefCell<Vec<(u32, i32)>>,
    process_exits: RefCell<Vec<(u32, Option<i32>)>>,
    process_support: Cell<Option<bool>>,
}

impl Renderer {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            ext_host: RefCell::new(None),
            next_id: Cell::new(1),
            pending: RefCell::new(Vec::new()),
            calls: RefCell::new(Vec::new()),
            requested: RefCell::new(Vec::new()),
            registered_profiles: RefCell::new(Vec::new()),
            process_data: RefCell::new(Vec::new()),
            process_ready: RefCell::new(Vec::new()),
            process_exits: RefCell::new(Vec::new()),
            process_support: Cell::new(None),
        })
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

/// Liga o dublê da thread principal ao host: registra o cliente e entrega a ele o proxy do ExtHost.
struct Binder {
    renderer: Rc<Renderer>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.renderer.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_TERMINAL_SERVICE));
        let renderer: Rc<dyn MainThreadTerminalServiceShape> = self.renderer.clone();
        context.set(&MainContext::MAIN_THREAD_TERMINAL_SERVICE, renderer);
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

        // `$acceptProcessShutdown`: a thread principal fecha o terminal e o `close` do pty roda.
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
