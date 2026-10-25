//! Porte de `src/vs/platform/commands/common/commands.ts`.
//!
//! Representação:
//! - Os argumentos e o retorno de um comando (`unknown[]`, `R`) são [`Dynamic`], porque o registro
//!   guarda comandos de tipos diferentes e valida as restrições de argumento em tempo de execução.
//! - O `ICommandHandler` é `Rc<dyn Fn(&ServicesAccessor, &[Dynamic]) -> Dynamic>`. A identidade do handler
//!   (`handler === command`, nos testes) é a do `Rc`.
//! - `CommandsRegistry` é o singleton por thread de [`commands_registry`], com o `noop` registrado.
//! - `executeCommand` devolve uma [`Promise`] de `Dynamic` (`Dynamic::Undefined` quando não há retorno).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, mark_as_singleton, to_disposable};
use je_base::linked_list::LinkedList;
use je_base::promise::Promise;
use je_base::types::{TypeConstraint, validate_constraints};
use je_instantiation::{InstantiationService, ServicesAccessor, service_id};

use crate::action::LocalizedString;

/// `ICommandEvent`.
#[derive(Clone, Debug)]
pub struct CommandEvent {
    pub command_id: String,
    pub args: Vec<Dynamic>,
}

/// `ICommandService`.
pub trait ICommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent>;
    fn on_did_execute_command(&self) -> Event<CommandEvent>;
    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic>;
}

service_id!(pub I_COMMAND_SERVICE: dyn ICommandService = "commandService");

/// O miolo comum do `executeCommand` do `CommandService` (workbench) e do `StandaloneCommandService` (editor standalone):
/// procura o comando no registro, dispara `onWillExecuteCommand`, roda o handler dentro de `invokeFunction`, dispara
/// `onDidExecuteCommand` e devolve o resultado como promessa resolvida. Comando ausente ou handler que lança (pânico)
/// voltam como promessa rejeitada.
pub fn invoke_registered_command(
    instantiation_service: &InstantiationService,
    id: &str,
    args: Vec<Dynamic>,
    on_will_execute_command: &Emitter<CommandEvent>,
    on_did_execute_command: &Emitter<CommandEvent>,
) -> Promise<Dynamic> {
    let Some(command) = commands_registry().get_command(id) else {
        return Promise::rejected(Error::new(format!("command '{id}' not found")));
    };
    let result = errors::catch(|| {
        on_will_execute_command.fire(CommandEvent { command_id: id.to_owned(), args: args.clone() });
        let result = instantiation_service.invoke_function(|accessor| (command.handler)(accessor, &args));
        on_did_execute_command.fire(CommandEvent { command_id: id.to_owned(), args: args.clone() });
        result
    });
    match result {
        Ok(result) => Promise::resolved(result),
        Err(error) => Promise::rejected(error),
    }
}

/// `ICommandHandler`.
pub type CommandHandler = Rc<dyn Fn(&ServicesAccessor, &[Dynamic]) -> Dynamic>;

/// `ICommand`.
#[derive(Clone)]
pub struct Command {
    pub id: String,
    pub handler: CommandHandler,
    pub metadata: Option<CommandMetadata>,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Command({})", self.id)
    }
}

impl Command {
    pub fn new(id: impl Into<String>, handler: CommandHandler) -> Self {
        Self { id: id.into(), handler, metadata: None }
    }

    pub fn with_metadata(mut self, metadata: CommandMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// `ILocalizedString | string`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandDescription {
    Plain(String),
    Localized(LocalizedString),
}

impl From<&str> for CommandDescription {
    fn from(value: &str) -> Self {
        CommandDescription::Plain(value.to_string())
    }
}

/// Um item de `ICommandMetadata.args`.
#[derive(Clone)]
pub struct CommandArgument {
    pub name: String,
    pub is_optional: bool,
    pub description: Option<String>,
    pub constraint: Option<TypeConstraint>,
    /// `IJSONSchema`, como valor dinâmico.
    pub schema: Option<Dynamic>,
}

impl CommandArgument {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), is_optional: false, description: None, constraint: None, schema: None }
    }
}

/// `ICommandMetadata`.
#[derive(Clone)]
pub struct CommandMetadata {
    /// Um resumo curto do que o comando faz (API, atalhos sem outra interface, busca da paleta).
    pub description: CommandDescription,
    pub args: Option<Vec<CommandArgument>>,
    pub returns: Option<String>,
}

impl CommandMetadata {
    pub fn new(description: impl Into<CommandDescription>) -> Self {
        Self { description: description.into(), args: None, returns: None }
    }
}

/// `ICommandsMap`.
pub type CommandsMap = HashMap<String, Command>;

/// `ICommandRegistry` e a classe anônima que a implementa.
pub struct CommandsRegistry {
    commands: RefCell<HashMap<String, LinkedList<Command>>>,
    on_did_register_command: Rc<Emitter<String>>,
}

thread_local! {
    static COMMANDS_REGISTRY: Rc<CommandsRegistry> = {
        let registry = Rc::new(CommandsRegistry { commands: RefCell::new(HashMap::new()), on_did_register_command: Emitter::new() });
        // `CommandsRegistry.registerCommand('noop', () => { })`
        let noop: CommandHandler = Rc::new(|_, _| Dynamic::Undefined);
        registry.register_command(Command::new("noop", noop));
        registry
    };
}

/// `CommandsRegistry`: o registro do thread.
pub fn commands_registry() -> Rc<CommandsRegistry> {
    COMMANDS_REGISTRY.with(Rc::clone)
}

impl CommandsRegistry {
    /// `onDidRegisterCommand`.
    pub fn on_did_register_command(&self) -> Event<String> {
        self.on_did_register_command.event()
    }

    /// `registerCommand(id, handler)`. Lança `invalid command` se o handler faltar (`None`).
    pub fn register_command_handler(self: &Rc<Self>, id: &str, handler: Option<CommandHandler>) -> DisposableRef {
        let Some(handler) = handler else {
            errors::throw(Error::new("invalid command"));
        };
        self.register_command(Command::new(id, handler))
    }

    /// `registerCommand(command)`.
    pub fn register_command(self: &Rc<Self>, mut command: Command) -> DisposableRef {
        if command.id.is_empty() {
            errors::throw(Error::new("invalid command"));
        }

        // add argument validation if rich command metadata is provided
        if let Some(args) = command.metadata.as_ref().and_then(|m| m.args.as_ref()) {
            let constraints: Vec<Option<TypeConstraint>> = args.iter().map(|a| a.constraint.clone()).collect();
            let actual_handler = command.handler.clone();
            command.handler = Rc::new(move |accessor, args| {
                validate_constraints(args, &constraints);
                actual_handler(accessor, args)
            });
        }

        // find a place to store the command
        let id = command.id.clone();
        let registered_id = id.clone();
        let removal = {
            let mut commands = self.commands.borrow_mut();
            let list = commands.entry(id.clone()).or_default();
            list.unshift(command)
        };

        let this = Rc::clone(self);
        let ret = to_disposable(move || {
            removal.remove();
            let mut commands = this.commands.borrow_mut();
            if commands.get(&id).is_some_and(LinkedList::is_empty) {
                commands.remove(&id);
            }
        });

        // tell the world about this command
        self.on_did_register_command.fire(registered_id);
        mark_as_singleton(ret)
    }

    /// `registerCommandAlias`.
    pub fn register_command_alias(self: &Rc<Self>, old_id: &str, new_id: &str) -> DisposableRef {
        let new_id = new_id.to_string();
        self.register_command(Command::new(
            old_id,
            Rc::new(move |accessor, args| {
                let command_service = accessor.get(I_COMMAND_SERVICE);
                // O `executeCommand` do original devolve a promessa do alias.
                let _ = command_service.execute_command(&new_id, args.to_vec());
                Dynamic::Undefined
            }),
        ))
    }

    /// `getCommand`.
    pub fn get_command(&self, id: &str) -> Option<Command> {
        let commands = self.commands.borrow();
        let list = commands.get(id)?;
        if list.is_empty() {
            return None;
        }
        list.iter().next()
    }

    /// `getCommands`.
    pub fn get_commands(&self) -> CommandsMap {
        let keys: Vec<String> = self.commands.borrow().keys().cloned().collect();
        keys.into_iter().filter_map(|key| self.get_command(&key).map(|command| (key, command))).collect()
    }
}
