//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalInstanceService.ts`: o [`TerminalInstanceService`], que cria as
//! instâncias de terminal, converte um perfil em configuração de lançamento e entrega o backend registrado para uma autoridade
//! remota (esperando o registro quando ele ainda não chegou).
//!
//! Divergências:
//! - **`registerSingleton(ITerminalInstanceService, TerminalInstanceService, InstantiationType.Delayed)`.** O registro do
//!   singleton fica com o frontend que monta a coleção de serviços (o `Delayed` pede o proxy de instanciação atrasada, que o
//!   `ITerminalInstanceService` ainda não declara no identificador).
//! - **`createInstance(profile | shellLaunchConfig, target)`.** A sobrecarga dupla é o trait ([`ITerminalInstanceService`]), que recebe
//!   a configuração já convertida; o tipo da união é o [`ShellLaunchConfigOrProfile`] de `convert_profile_to_shell_launch_config`.
//!   A criação do xterm (`TerminalInstance.getXtermConstructor`, que carrega o módulo do xterm.js) é o construtor que o frontend
//!   entrega por [`TerminalInstanceService::set_xterm_ctor_factory`]: o porte da instância recebe o construtor como argumento. Sem
//!   fábrica registrada, criar uma instância lança.
//! - **`hasKey(shellLaunchConfigOrProfile, { profileName: true })`.** É a variante [`ShellLaunchConfigOrProfile::Profile`].
//! - **Perfil sem `path`.** O original devolve o próprio objeto do perfil como configuração de lançamento; o porte devolve a
//!   configuração com os campos que os dois tipos compartilham (`args`, `env`, `icon` e `color`), porque o perfil não é um
//!   `IShellLaunchConfig`.
//! - **Configuração recebida e `cwd`.** O original muda o objeto recebido (`shellLaunchConfigOrProfile.cwd = cwd`) e o devolve; o
//!   porte recebe a configuração por valor e devolve a alterada.
//! - **`Registry.as<ITerminalBackendRegistry>(TerminalExtensions.Backend)`.** É o registro de `terminal_backend_registry`.
//! - **`getRegisteredBackends(): IterableIterator`.** É um `Vec`.
//! - **`async getBackend`.** Devolve a [`Promise`] da tarefa local.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::{Promise, Resolver};
use je_instantiation::{Injectable, InstantiationService, service_impl};
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::terminal::terminal::{IShellLaunchConfig, ShellCwd, TerminalLocation};

use super::terminal::{ITerminalInstance, ITerminalInstanceService, ShellLaunchConfigOrProfile};
use super::terminal_instance::{TerminalInstance, TerminalInstanceArgs};
use super::terminal_process_manager::run_async;
use super::xterm::xterm_terminal::XtermCtor;
use crate::contrib::terminal::common::terminal_backend::ITerminalBackend;
use crate::contrib::terminal::common::terminal_backend_registry;
use crate::contrib::terminal::common::terminal_context_key;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// O que o frontend entrega para criar o construtor do emulador de cada instância (ver "Divergências").
pub type XtermCtorFactory = Rc<dyn Fn() -> XtermCtor>;

/// `TerminalInstanceService`.
pub struct TerminalInstanceService {
    base: Rc<Disposable>,
    instantiation_service: Rc<InstantiationService>,
    terminal_shell_type_context_key: Rc<dyn IContextKey>,
    backend_registration: HashMap<Option<String>, (Promise<()>, Resolver<()>)>,
    xterm_ctor_factory: RefCell<Option<XtermCtorFactory>>,

    on_did_create_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_register_backend: Rc<Emitter<Rc<dyn ITerminalBackend>>>,
}

impl Injectable for TerminalInstanceService {
    type Args = ();
    type Deps = (Rc<InstantiationService>, Rc<dyn IContextKeyService>, Rc<dyn IWorkbenchEnvironmentService>);

    fn create(_: (), (instantiation_service, context_key_service, environment_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_create_instance = Emitter::new();
        base.register(on_did_create_instance.clone());
        let on_did_register_backend = Emitter::new();
        base.register(on_did_register_backend.clone());

        let terminal_shell_type_context_key = terminal_context_key::shell_type_bind_to(&*context_key_service);

        let mut backend_registration = HashMap::new();
        for remote_authority in [None, environment_service.remote_authority()] {
            let (promise, resolver) = Promise::new();
            backend_registration.insert(remote_authority, (promise, resolver));
        }

        Rc::new(Self {
            base,
            instantiation_service,
            terminal_shell_type_context_key,
            backend_registration,
            xterm_ctor_factory: RefCell::new(None),
            on_did_create_instance,
            on_did_register_backend,
        })
    }
}

service_impl!(TerminalInstanceService => dyn ITerminalInstanceService, disposable);

impl IDisposable for TerminalInstanceService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl TerminalInstanceService {
    /// Registra a fábrica do construtor do emulador de cada instância (ver "Divergências").
    pub fn set_xterm_ctor_factory(&self, factory: XtermCtorFactory) {
        *self.xterm_ctor_factory.borrow_mut() = Some(factory);
    }
}

impl ITerminalInstanceService for TerminalInstanceService {
    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_create_instance.event()
    }

    fn on_did_register_backend(&self) -> Event<Rc<dyn ITerminalBackend>> {
        self.on_did_register_backend.event()
    }

    fn convert_profile_to_shell_launch_config(&self, shell_launch_config_or_profile: Option<ShellLaunchConfigOrProfile>, cwd: Option<ShellCwd>) -> IShellLaunchConfig {
        match shell_launch_config_or_profile {
            // Profile was provided
            Some(ShellLaunchConfigOrProfile::Profile(profile)) => {
                if profile.path.is_empty() {
                    return IShellLaunchConfig { args: profile.args, env: profile.env, icon: profile.icon, color: profile.color, ..IShellLaunchConfig::default() };
                }
                IShellLaunchConfig {
                    executable: Some(profile.path),
                    args: profile.args,
                    env: profile.env,
                    icon: profile.icon,
                    color: profile.color,
                    name: if profile.override_name.unwrap_or(false) { Some(profile.profile_name) } else { None },
                    cwd,
                    ..IShellLaunchConfig::default()
                }
            }
            // A shell launch config was provided
            Some(ShellLaunchConfigOrProfile::LaunchConfig(mut shell_launch_config)) => {
                if cwd.is_some() {
                    shell_launch_config.cwd = cwd;
                }
                shell_launch_config
            }
            // Return empty shell launch config
            None => IShellLaunchConfig::default(),
        }
    }

    fn create_instance(&self, launch_config: IShellLaunchConfig, target: TerminalLocation) -> Rc<dyn ITerminalInstance> {
        let shell_launch_config = self.convert_profile_to_shell_launch_config(Some(ShellLaunchConfigOrProfile::LaunchConfig(launch_config)), None);
        let factory = self
            .xterm_ctor_factory
            .borrow()
            .clone()
            .unwrap_or_else(|| errors::throw(Error::new("TerminalInstanceService: nenhuma fábrica do construtor do xterm foi registrada (set_xterm_ctor_factory)")));
        let instance = self.instantiation_service.create_instance::<TerminalInstance>(TerminalInstanceArgs {
            terminal_shell_type_context_key: self.terminal_shell_type_context_key.clone(),
            shell_launch_config,
            xterm_ctor: factory(),
        });
        instance.set_target(Some(target));
        let instance: Rc<dyn ITerminalInstance> = instance;
        self.on_did_create_instance.fire(instance.clone());
        instance
    }

    fn get_backend(&self, remote_authority: Option<&str>) -> Promise<Option<Rc<dyn ITerminalBackend>>> {
        let remote_authority = remote_authority.map(str::to_owned);
        let registration = self.backend_registration.get(&remote_authority).map(|(promise, _)| promise.clone());
        run_async(async move {
            let mut backend = terminal_backend_registry::get_terminal_backend(remote_authority.as_deref());
            if backend.is_none() {
                // Ensure backend is initialized and try again
                if let Some(registration) = registration {
                    registration.await?;
                }
                backend = terminal_backend_registry::get_terminal_backend(remote_authority.as_deref());
            }
            Ok(backend)
        })
    }

    fn get_registered_backends(&self) -> Vec<Rc<dyn ITerminalBackend>> {
        terminal_backend_registry::terminal_backends()
    }

    fn did_register_backend(&self, backend: Rc<dyn ITerminalBackend>) {
        if let Some((_, resolver)) = self.backend_registration.get(&backend.remote_authority()) {
            resolver.resolve(());
        }
        self.on_did_register_backend.fire(backend);
    }
}
