//! O host de extensões local, em processo: o `IExtensionHost` (`common/extensions.ts`) que sobe o host de extensões do `je-exthost`
//! (o zjsc rodando o código das extensões, com o `vscode` montado em Rust) e o liga ao `RPCProtocol` do workbench por um
//! `IMessagePassingProtocol` em memória.
//!
//! Decisão de arquitetura (`wip/notes/exthost-plan.md`, "Decisão de arquitetura"): é o equivalente em processo do
//! `localProcessExtensionHost.ts` (`api/node`, que cria um processo filho e conversa por socket ou pipe) e do
//! `webWorkerExtensionHost.ts` (que conversa com um `Worker` por `MessagePort`). Aqui os dois lados são Rust no mesmo processo e
//! na mesma thread: o par de pontas é o [`MessageChannel`] em memória (`ipc.mp.ts` sobre `MessagePort`), uma ponta é entregue ao
//! `ExtensionHostManager` (que cria o `RPCProtocol` do workbench sobre ela) e a outra serve o `RPCProtocol` do lado do host, onde
//! moram o `ExtHostExtensionService` (ator `ExtHostExtensionService`) e os serviços `ExtHost*` do `vscode`. Toda chamada entre
//! os dois lados atravessa a serialização do RPC, como atravessaria um processo.
//!
//! Divergências:
//! - **Sem aperto de mãos.** O `webWorkerExtensionHost.ts` espera `Ready`, envia o `IExtensionHostInitData` e espera `Initialized`
//!   (`extensionHostProtocol.ts`). Os dois lados nascem juntos no mesmo código, então `start()` pede o `initData.extensions` ao
//!   [`ILocalExtensionHostDataProvider`] (`_initDataProvider.getInitData()`), sobe o host com esse instantâneo (`autoStart` vem de
//!   `startup`) e devolve a ponta do workbench já com o outro lado pronto. O resto do `IExtensionHostInitData` (ambiente, workspace,
//!   telemetria, caminhos de log) não existe; as extensões que mudam depois chegam pelo `$startExtensionHost` e pelo
//!   `$deltaExtensions` do `ExtensionHostManager`.
//! - **`pid`** é `None` (não há processo) e `getInspectPort()`/`enableInspectPort()` não têm inspetor (`None` e `false`).
//! - **`dispose()`** não envia `Terminate`: desativa as extensões (`deactivate` e descartáveis), descarta o RPC do lado do host e fecha
//!   a porta, que é o que o processo do host faria ao receber a mensagem.
//! - **`onExit`** só dispara se alguém chamar [`LocalExtensionHost::exit`] (o host em processo não tem um processo que morra
//!   sozinho); o gerenciador o trata como a saída de um processo.
//! - **Laço de eventos.** O host não tem laço próprio: quem o move (o laço do aplicativo, ou um teste) chama
//!   [`LocalExtensionHost::pump`] para esvaziar as microtarefas do motor e as do Rust, e `NodeRunner::run_event_loop_for` para os
//!   timers das extensões.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::parts::ipc::IMessagePassingProtocol;
use je_base::parts::ipc::mp::{MessageChannel, Protocol};
use je_base::promise::Promise;
use je_platform::log::LogServiceRef;
use je_workbench::api::common::ext_host_rpc_shapes::register_known_shapes;
use je_workbench::services::extensions::extension_host_proxy::EXT_HOST_EXTENSION_SERVICE_SID;
use je_workbench::services::extensions::extension_host_protocol::IExtensionDescriptionSnapshot;
use je_workbench::services::extensions::extension_running_location::ExtensionRunningLocation;
use je_workbench::services::extensions::extensions::{ExtensionHostExtensions, ExtensionHostStartup, IExtensionHost, IExtensionInspectInfo};
use je_workbench::services::extensions::rpc_protocol::RpcProtocol;
use zjsc::api::embed_node::NodeRunner;

use crate::ext_host_extension_service::{ExtHostExtensionService, ExtHostExtensionServiceActor};
use crate::host::ExtensionHost;
use crate::in_process::IMainThreadBinder;

/// `IWebWorkerExtensionHostDataProvider` (`getInitData`): as extensões que o host recebe ao subir (`initData.extensions`). O serviço de
/// extensões o implementa (no caso do primeiro início, as extensões locais e as que rodam neste host; na reinicialização, o
/// instantâneo do registro).
pub trait ILocalExtensionHostDataProvider {
    fn get_init_data(&self) -> Promise<Rc<ExtensionHostExtensions>>;
}

/// O que existe depois de `start()`: o lado do host do canal.
struct StartedHost {
    host_rpc: Rc<RpcProtocol>,
    ext_host: Rc<ExtensionHost>,
    service: Rc<ExtHostExtensionService>,
    host_protocol: Rc<Protocol>,
    workbench_protocol: Rc<Protocol>,
}

/// `LocalExtensionHost`.
pub struct LocalExtensionHost {
    me: Weak<LocalExtensionHost>,
    runner: NodeRunner,
    running_location: ExtensionRunningLocation,
    startup: ExtensionHostStartup,
    log_service: LogServiceRef,
    data_provider: Rc<dyn ILocalExtensionHostDataProvider>,
    extensions: RefCell<Option<Rc<ExtensionHostExtensions>>>,
    on_exit: Rc<Emitter<(i32, Option<String>)>>,
    started: RefCell<Option<StartedHost>>,
    is_terminating: Cell<bool>,
    binder: RefCell<Option<Rc<dyn IMainThreadBinder>>>,
}

impl LocalExtensionHost {
    /// O host fica no motor `runner` (criado por [`crate::runtime::run_host_program`] ou, com a ponte `__jeRequire`, por
    /// `zjsc::api::embed_node::run_node`) e só sobe em `start()`.
    pub fn new(
        runner: NodeRunner,
        running_location: ExtensionRunningLocation,
        startup: ExtensionHostStartup,
        data_provider: Rc<dyn ILocalExtensionHostDataProvider>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            runner,
            running_location,
            startup,
            log_service,
            data_provider,
            extensions: RefCell::new(None),
            on_exit: Emitter::new(),
            started: RefCell::new(None),
            is_terminating: Cell::new(false),
            binder: RefCell::new(None),
        })
    }

    /// Entrega o `binder` que monta, em processo, os clientes `MainThread*` dos namespaces `workspace` e `window` (ver
    /// [`crate::in_process`]). Precisa vir antes de `start()`; sem ele o `vscode` do host só tem `commands`, `l10n` e as classes.
    pub fn set_main_thread_binder(&self, binder: Rc<dyn IMainThreadBinder>) {
        *self.binder.borrow_mut() = Some(binder);
    }

    /// Esvazia as microtarefas dos dois mundos (as do motor e as das `Promise` do Rust) até estabilizar.
    pub fn pump(&self) {
        // Solta o empréstimo antes: as microtarefas podem chamar `service()` ou `dispose()`.
        let ext_host = self.started.borrow().as_ref().map(|started| started.ext_host.clone());
        match ext_host {
            Some(ext_host) => ext_host.pump(),
            None => je_base::runtime::run_microtasks(),
        }
    }

    /// O `ExtHostExtensionService` do lado do host, depois de `start()`.
    pub fn service(&self) -> Option<Rc<ExtHostExtensionService>> {
        self.started.borrow().as_ref().map(|started| started.service.clone())
    }

    /// Simula a saída do host (o `onExit` do processo): `code` e `signal` chegam ao gerenciador.
    pub fn exit(&self, code: i32, signal: Option<&str>) {
        self.on_exit.fire((code, signal.map(str::to_owned)));
    }

    /// O lado do host do canal, com as extensões do `initData` (`snapshot`).
    fn start_host(&self, snapshot: IExtensionDescriptionSnapshot) -> Result<Rc<dyn IMessagePassingProtocol>, Error> {
        if self.started.borrow().is_some() {
            return Err(Error::new("The local extension host is already started"));
        }
        let channel = MessageChannel::new();
        let workbench_protocol = Protocol::new(channel.port1.clone());
        let host_protocol = Protocol::new(channel.port2.clone());

        let host_rpc = RpcProtocol::new(host_protocol.clone(), None);
        register_known_shapes(&host_rpc);
        let binder = self.binder.borrow().clone();
        let ext_host = Rc::new(ExtensionHost::with_bridge(&self.runner, &*host_rpc, binder.as_deref(), self.log_service.clone()));
        let auto_start = matches!(self.startup, ExtensionHostStartup::EagerAutoStart | ExtensionHostStartup::LazyAutoStart);
        let service = ExtHostExtensionService::new(self.runner.clone(), ext_host.disposables().clone(), &*host_rpc, self.log_service.clone(), auto_start, snapshot);
        host_rpc.set_actor(EXT_HOST_EXTENSION_SERVICE_SID, ExtHostExtensionServiceActor::new(service.clone()));
        ext_host.bind_extension_service(service.clone());
        service.initialize();

        let protocol: Rc<dyn IMessagePassingProtocol> = workbench_protocol.clone();
        *self.started.borrow_mut() = Some(StartedHost { host_rpc, ext_host, service, host_protocol, workbench_protocol });
        Ok(protocol)
    }
}

impl IExtensionHost for LocalExtensionHost {
    fn pid(&self) -> Option<u32> {
        None
    }

    fn running_location(&self) -> ExtensionRunningLocation {
        self.running_location
    }

    fn remote_authority(&self) -> Option<String> {
        None
    }

    fn startup(&self) -> ExtensionHostStartup {
        self.startup
    }

    fn extensions(&self) -> Option<Rc<ExtensionHostExtensions>> {
        self.extensions.borrow().clone()
    }

    fn on_exit(&self) -> Event<(i32, Option<String>)> {
        self.on_exit.event()
    }

    fn start(&self) -> Promise<Rc<dyn IMessagePassingProtocol>> {
        let this = self.me.upgrade().expect("o host está vivo enquanto um método dele roda");
        async_fn(async move {
            // (1) the init data: the extensions this host will run (or already runs)
            let extensions = this.data_provider.get_init_data().await?;
            *this.extensions.borrow_mut() = Some(extensions.clone());
            if this.is_terminating.get() {
                return Err(Error::cancellation());
            }
            // (2) both ends of the channel come up together
            this.start_host(extensions.to_snapshot())
        })
    }

    fn get_inspect_port(&self) -> Option<IExtensionInspectInfo> {
        None
    }

    fn enable_inspect_port(&self) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn dispose(&self) {
        if self.is_terminating.replace(true) {
            return;
        }
        let started = self.started.borrow_mut().take();
        if let Some(started) = started {
            started.service.deactivate_all();
            started.ext_host.dispose_main_thread();
            started.host_rpc.dispose();
            started.host_protocol.disconnect();
            started.workbench_protocol.disconnect();
        }
    }
}
