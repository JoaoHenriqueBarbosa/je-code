//! Porte de `src/vs/workbench/api/browser/mainThreadCommands.ts`: a ponte entre o registro de comandos do workbench e
//! os comandos que as extensões registram no host.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadCommands)` e o `IExtHostContext` do construtor: o registro de
//!   clientes não existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostCommands`.
//! - O handler de um comando registrado devolve [`Dynamic`], que não tem promessa. O handler que o original devolve
//!   (`proxy.$executeContributedCommand(...).then(revive)`, uma `Promise`) devolve a promessa embrulhada num objeto
//!   com carga nativa ([`promise_to_dynamic`]); quem executa o comando a desembrulha com [`promise_from_dynamic`]. O
//!   mesmo vale para o handler de `_generateCommandsDocumentation`.
//! - `CommandsRegistry.getCommands()` é um `HashMap`, sem a ordem de registro do `Map` do original: a lista de
//!   `$getCommands` e a documentação gerada saem ordenadas por id.
//! - `this._generateCommandsDocumentation()` é chamado por um handler que guarda só um [`Weak`](std::rc::Weak) do
//!   objeto (o registro mora no próprio objeto, e um [`Rc`] no fecho seria um ciclo).
//! - O `$getContributedCommandMetadata` devolve uma lista de pares (ver `ExtHostCommandsShape`), então
//!   `result[id] = command.metadata` troca o valor no lugar ou acrescenta no fim. A descrição de um item é sempre um
//!   `ICommandMetadataDto` (o `string` do original nunca é produzido), e `console.log` é `println!`.
//! - O `args` de `$executeCommand` é [`CommandArgs`] (`unknown[] | SerializableObjectWithBuffers<unknown[]>`).

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::Error;
use je_base::lifecycle::{DisposableMap, DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_platform::commands::commands::{commands_registry, CommandDescription, CommandHandler, CommandMetadata, ICommandService};

use crate::api::common::ext_host_protocol::{
    CommandArgs, CommandMetadataArgDto, ExtHostCommandsShape, ExtHostContext, ICommandMetadataDto, IMainContext, MainThreadCommandsShape,
};
use crate::api::common::ext_host_values::revive;
use crate::services::extensions::extensions::IExtensionService;

/// Embrulha uma promessa num [`Dynamic`] (objeto com carga nativa), para um handler de comando devolver o resultado
/// assíncrono pelo retorno `Dynamic` do `ICommandHandler`.
pub fn promise_to_dynamic(promise: Promise<Dynamic>) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(promise)))
}

/// O inverso de [`promise_to_dynamic`]: a promessa embrulhada, se o valor for uma.
pub fn promise_from_dynamic(value: &Dynamic) -> Option<Promise<Dynamic>> {
    match value {
        Dynamic::Object(object) => object.host::<Promise<Dynamic>>().map(|promise| (*promise).clone()),
        _ => None,
    }
}

/// `MainThreadCommands`.
pub struct MainThreadCommands {
    command_registrations: Rc<DisposableMap<String, dyn IDisposable>>,
    generate_commands_documentation_registration: DisposableRef,
    proxy: Rc<dyn ExtHostCommandsShape>,
    command_service: Rc<dyn ICommandService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl MainThreadCommands {
    pub fn new(ext_host_context: &dyn IMainContext, command_service: Rc<dyn ICommandService>, extension_service: Rc<dyn IExtensionService>) -> Rc<Self> {
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_COMMANDS);

        Rc::new_cyclic(|this: &std::rc::Weak<MainThreadCommands>| {
            let this = this.clone();
            let generate: CommandHandler = Rc::new(move |_accessor, _args| match this.upgrade() {
                Some(commands) => promise_to_dynamic(commands.generate_commands_documentation().then(|()| Dynamic::Undefined)),
                None => Dynamic::Undefined,
            });
            let generate_commands_documentation_registration = commands_registry().register_command_handler("_generateCommandsDocumentation", Some(generate));

            Self {
                command_registrations: DisposableMap::new(),
                generate_commands_documentation_registration,
                proxy,
                command_service,
                extension_service,
            }
        })
    }

    /// `_generateCommandsDocumentation`.
    fn generate_commands_documentation(&self) -> Promise<()> {
        let proxy = self.proxy.clone();
        async_fn(async move {
            let mut result = proxy.rpc_get_contributed_command_metadata().await?;

            // add local commands
            let commands = commands_registry().get_commands();
            let mut ids: Vec<&String> = commands.keys().collect();
            ids.sort();
            for id in ids {
                if let Some(metadata) = &commands[id].metadata {
                    let dto = metadata_to_dto(metadata);
                    match result.iter_mut().find(|(existing, _)| existing == id) {
                        Some(entry) => entry.1 = dto,
                        None => result.push((id.clone(), dto)),
                    }
                }
            }

            // print all as markdown
            let all: Vec<String> = result.iter().map(|(id, metadata)| format!("`{id}` - {}", generate_markdown(metadata))).collect();
            println!("{}", all.join("\n"));
            Ok(())
        })
    }
}

impl IDisposable for MainThreadCommands {
    fn dispose(&self) {
        self.command_registrations.dispose();
        self.generate_commands_documentation_registration.dispose();
    }
}

impl MainThreadCommandsShape for MainThreadCommands {
    fn rpc_register_command(&self, id: &str) {
        let proxy = self.proxy.clone();
        let command_id = id.to_owned();
        let handler: CommandHandler = Rc::new(move |_accessor, args| {
            promise_to_dynamic(proxy.rpc_execute_contributed_command(&command_id, args.to_vec()).then(|result| revive(&result)))
        });
        self.command_registrations.set(id.to_owned(), commands_registry().register_command_handler(id, Some(handler)), false);
    }

    fn rpc_unregister_command(&self, id: &str) {
        self.command_registrations.delete_and_dispose(&id.to_owned());
    }

    fn rpc_fire_command_activation_event(&self, id: &str) {
        let activation_event = format!("onCommand:{id}");
        if !self.extension_service.activation_event_is_done(&activation_event) {
            // this is NOT awaited because we only use it as drive-by-activation
            // for commands that are already known inside the extension host
            let _ = self.extension_service.activate_by_event(&activation_event, None);
        }
    }

    fn rpc_execute_command(&self, id: &str, args: CommandArgs, retry: bool) -> Promise<Dynamic> {
        let id = id.to_owned();
        let extension_service = self.extension_service.clone();
        let command_service = self.command_service.clone();
        async_fn(async move {
            let mut args = match args {
                CommandArgs::Plain(args) => args,
                CommandArgs::WithBuffers(args) => args.value,
            };
            for arg in &mut args {
                *arg = revive(arg);
            }
            if retry && !args.is_empty() && commands_registry().get_command(&id).is_none() {
                extension_service.activate_by_event(&format!("onCommand:{id}"), None).await?;
                return Err(Error::new("$executeCommand:retry"));
            }
            command_service.execute_command(&id, args).await
        })
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        let mut ids: Vec<String> = commands_registry().get_commands().into_keys().collect();
        ids.sort();
        Promise::resolved(ids)
    }
}

/// O `ICommandMetadata` como o DTO que o `result` de `$getContributedCommandMetadata` guarda.
fn metadata_to_dto(metadata: &CommandMetadata) -> ICommandMetadataDto {
    ICommandMetadataDto {
        description: metadata.description.clone(),
        args: metadata.args.as_ref().map(|args| {
            args.iter()
                .map(|arg| CommandMetadataArgDto { name: arg.name.clone(), is_optional: Some(arg.is_optional), description: arg.description.clone() })
                .collect()
        }),
        returns: metadata.returns.clone(),
    }
}

// --- command doc

fn generate_markdown(description: &ICommandMetadataDto) -> String {
    let description_string = match &description.description {
        CommandDescription::Plain(text) => text.clone(),
        // Our docs website is in English, so keep the original here.
        CommandDescription::Localized(localized) => localized.original.clone(),
    };
    let mut parts = vec![description_string];
    parts.push("\n\n".to_owned());
    if let Some(args) = &description.args {
        for arg in args {
            parts.push(format!("* _{}_ - {}\n", arg.name, arg.description.as_deref().unwrap_or("")));
        }
    }
    if let Some(returns) = description.returns.as_deref().filter(|returns| !returns.is_empty()) {
        parts.push(format!("* _(returns)_ - {returns}"));
    }
    parts.push("\n\n".to_owned());
    parts.join("")
}
