//! Porte de `src/vs/workbench/services/commands/common/commandService.ts`: o `CommandService`, que executa um comando registrado
//! depois de esperar o evento de ativação `onCommand:<id>` (e, se o comando ainda não existe, a ativação `*` correndo contra o
//! registro do comando e contra 30 s).
//!
//! Divergências:
//! - `executeCommand<T>(id, ...args)` é `execute_command(id, args)` do [`ICommandService`] do `je-platform`, que devolve
//!   `Promise<Dynamic>`.
//! - `instantiationService.invokeFunction(command.handler, ...args)` é `InstantiationService::invoke_function` chamando o
//!   `handler(accessor, args)`. Um comando que lança (pânico) rejeita a chamada, como o `catch` do original.
//! - `registerSingleton(ICommandService, CommandService, InstantiationType.Delayed)` não é portado: o `I_COMMAND_SERVICE` do
//!   `je-platform` ainda não tem o proxy da instanciação atrasada que o registro exige.
//! - `raceCancellablePromises` recebe promessas comuns (a de `activateByEvent`) onde o original passa `Promise` e o `cancel`
//!   inexistente é `cancel?.()`; aqui a promessa comum vira `CancelablePromise` sem `cancel` (`From<Promise<T>>`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{not_cancellable_promise, race_cancellable_promises, timeout};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::{CancelablePromise, Promise};
use je_instantiation::InstantiationService;
use je_platform::commands::commands::{commands_registry, CommandEvent, ICommandService};
use je_platform::log::{LogArg, LogServiceRef};

use crate::services::extensions::extensions::IExtensionService;

/// `CommandService`.
pub struct CommandService {
    me: Weak<CommandService>,
    base: Rc<Disposable>,
    instantiation_service: Rc<InstantiationService>,
    extension_service: Rc<dyn IExtensionService>,
    log_service: LogServiceRef,
    extension_host_is_ready: Rc<std::cell::Cell<bool>>,
    star_activation: Rc<RefCell<Option<CancelablePromise<()>>>>,
    on_will_execute_command: Rc<Emitter<CommandEvent>>,
    on_did_execute_command: Rc<Emitter<CommandEvent>>,
}

impl CommandService {
    pub fn new(instantiation_service: Rc<InstantiationService>, extension_service: Rc<dyn IExtensionService>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_will_execute_command = Emitter::new();
        let on_did_execute_command = Emitter::new();
        base.register(on_will_execute_command.clone());
        base.register(on_did_execute_command.clone());

        let extension_host_is_ready = Rc::new(std::cell::Cell::new(false));
        let ready = extension_host_is_ready.clone();
        let _ = extension_service.when_installed_extensions_registered().then(move |value| ready.set(value));

        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            instantiation_service,
            extension_service,
            log_service,
            extension_host_is_ready,
            star_activation: Rc::default(),
            on_will_execute_command,
            on_did_execute_command,
        })
    }

    fn activate_star(&self) -> Promise<()> {
        let mut star_activation = self.star_activation.borrow_mut();
        if star_activation.is_none() {
            // wait for * activation, limited to at most 30s.
            *star_activation = Some(race_cancellable_promises(vec![
                CancelablePromise::from(self.extension_service.activate_by_event("*", None)),
                timeout(30000),
            ]));
        }

        // This is wrapped with notCancellablePromise so it doesn't get cancelled
        // early because it is shared between consumers.
        not_cancellable_promise(star_activation.as_ref().expect("definido acima"))
    }

    fn try_execute_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        let Some(command) = commands_registry().get_command(id) else {
            return Promise::rejected(Error::new(format!("command '{id}' not found")));
        };
        let result = errors::catch(|| {
            self.on_will_execute_command.fire(CommandEvent { command_id: id.to_owned(), args: args.clone() });
            let result = self.instantiation_service.invoke_function(|accessor| (command.handler)(accessor, &args));
            self.on_did_execute_command.fire(CommandEvent { command_id: id.to_owned(), args: args.clone() });
            result
        });
        match result {
            Ok(result) => Promise::resolved(result),
            Err(error) => Promise::rejected(error),
        }
    }
}

impl ICommandService for CommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        self.on_will_execute_command.event()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        self.on_did_execute_command.event()
    }

    fn execute_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.log_service.trace_args("CommandService#executeCommand", &[LogArg::Value(Dynamic::string(id))]);

        let activation_event = format!("onCommand:{id}");
        let command_is_registered = commands_registry().get_command(id).is_some();

        if command_is_registered {
            // if the activation event has already resolved (i.e. subsequent call),
            // we will execute the registered command immediately
            if self.extension_service.activation_event_is_done(&activation_event) {
                return self.try_execute_command(id, args);
            }

            // if the extension host didn't start yet, we will execute the registered
            // command immediately and send an activation event, but not wait for it
            if !self.extension_host_is_ready.get() {
                let _ = self.extension_service.activate_by_event(&activation_event, None); // intentionally not awaited
                return self.try_execute_command(id, args);
            }

            // we will wait for a simple activation event (e.g. in case an extension wants to overwrite it)
            let this = self.this();
            let id = id.to_owned();
            return self.extension_service.activate_by_event(&activation_event, None).then_promise(move |()| this.try_execute_command(&id, args));
        }

        // finally, if the command is not registered we will send a simple activation event
        // as well as a * activation event raced against registration and against 30s
        let target = id.to_owned();
        let registered = commands_registry().on_did_register_command().filter(move |registered| *registered == target, None).to_promise(None);
        let registered_cancel = registered.clone();
        let registered_or_star = race_cancellable_promises(vec![
            CancelablePromise::new(self.activate_star(), || {}),
            CancelablePromise::new(registered.then(|_| ()), move || registered_cancel.cancel()),
        ]);
        let activation = self.extension_service.activate_by_event(&activation_event, None);
        let this = self.this();
        let id = id.to_owned();
        Promise::all(vec![activation, registered_or_star.then(|_| ())]).then_promise(move |_| this.try_execute_command(&id, args))
    }
}

impl CommandService {
    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }
}

impl IDisposable for CommandService {
    fn dispose(&self) {
        self.base.dispose();
        if let Some(star_activation) = self.star_activation.borrow().as_ref() {
            star_activation.cancel();
        }
    }
}
