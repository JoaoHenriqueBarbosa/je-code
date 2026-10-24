//! Porte de `src/vs/editor/test/browser/editorTestServices.ts`.
//!
//! - `TestCodeEditorService` já está portado em `test_code_editor_service.rs` (só no que o `OpenerService` usa), e
//!   é o que `test_code_editor.rs` reutiliza. `TestGlobalStyleSheet` estende `GlobalStyleSheet`, que depende do DOM
//!   (`CSSStyleSheet`) e do `AbstractCodeEditorService`, sem porte: ficam de fora.
//! - [`TestCommandService`] executa os comandos do `CommandsRegistry` (o singleton do thread) dentro do serviço de
//!   instanciação de teste.
//!
//! Divergências: o original recebe o `IInstantiationService` injetado no construtor. Aqui o serviço de instanciação
//! é criado depois do `TestCommandService` (que está na coleção de serviços dele), então ele chega por
//! [`TestCommandService::bind_instantiation_service`], como `Weak` para não formar um ciclo.

use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_platform::commands::{CommandEvent, ICommandService, commands_registry};

use super::test_instantiation_service::TestInstantiationService;

pub struct TestCommandService {
    instantiation_service: RefCell<Weak<TestInstantiationService>>,
    on_will_execute_command: Rc<Emitter<CommandEvent>>,
    on_did_execute_command: Rc<Emitter<CommandEvent>>,
}

impl TestCommandService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            instantiation_service: RefCell::new(Weak::new()),
            on_will_execute_command: Emitter::new(),
            on_did_execute_command: Emitter::new(),
        })
    }

    /// O `IInstantiationService` que o construtor do original recebe.
    pub fn bind_instantiation_service(&self, instantiation_service: Weak<TestInstantiationService>) {
        *self.instantiation_service.borrow_mut() = instantiation_service;
    }
}

impl ICommandService for TestCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        self.on_will_execute_command.event()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        self.on_did_execute_command.event()
    }

    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        let Some(command) = commands_registry().get_command(command_id) else {
            return Promise::rejected(Error::new(format!("command '{command_id}' not found")));
        };
        let Some(instantiation_service) = self.instantiation_service.borrow().upgrade() else {
            return Promise::rejected(Error::new("the instantiation service of the test command service is gone"));
        };

        let result = errors::catch(AssertUnwindSafe(|| {
            self.on_will_execute_command.fire(CommandEvent { command_id: command_id.to_owned(), args: args.clone() });
            let result = instantiation_service.invoke_function(|accessor| (command.handler)(accessor, &args));
            self.on_did_execute_command.fire(CommandEvent { command_id: command_id.to_owned(), args: args.clone() });
            result
        }));
        match result {
            Ok(result) => Promise::resolved(result),
            Err(error) => Promise::rejected(error),
        }
    }
}

impl IDisposable for TestCommandService {
    fn dispose(&self) {
        self.on_will_execute_command.dispose();
        self.on_did_execute_command.dispose();
    }
}
