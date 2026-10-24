//! Porte (parcial) de `src/vs/workbench/services/output/common/output.ts`: os contratos que a ponte de saída das extensões
//! (`MainThreadOutputService`) usa: `OutputChannelUpdateMode`, `IOutputChannel`, `IOutputChannelDescriptor`,
//! `IOutputService` e o `IOutputChannelRegistry` com a implementação em memória [`OutputChannelRegistry`].
//!
//! Divergências (o que não entrou, porque nenhum consumidor portado o lê): as chaves de contexto (`CONTEXT_IN_OUTPUT`...), os
//! filtros (`IOutputViewFilters`), `ILogEntry`/`getLogEntries`, `registerCompoundLogChannel`, `saveOutputAs`, `canSetLogLevel`,
//! `getLogLevel`/`setLogLevel`, `updateChannelSources` e os descritores de várias fontes (`source` é uma lista de
//! [`IOutputContentSource`], como no original, mas só o caso de uma fonte é construído por `$register`).
//! O `Registry.as<IOutputChannelRegistry>(Extensions.OutputChannels)` global é a função [`output_channel_registry`] (um registro por
//! thread, como o `commands_registry()` da plataforma).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;

/// `OUTPUT_VIEW_ID`.
pub const OUTPUT_VIEW_ID: &str = "workbench.panel.output";

/// `OutputChannelUpdateMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputChannelUpdateMode {
    Append = 1,
    Replace = 2,
    Clear = 3,
}

impl OutputChannelUpdateMode {
    /// O membro de um valor numérico.
    pub fn from_value(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::Append),
            2 => Some(Self::Replace),
            3 => Some(Self::Clear),
            _ => None,
        }
    }
}

/// `IOutputContentSource`.
#[derive(Clone, Debug, PartialEq)]
pub struct IOutputContentSource {
    pub name: Option<String>,
    pub resource: URI,
}

/// `IOutputChannelDescriptor`.
#[derive(Clone, Debug, PartialEq)]
pub struct IOutputChannelDescriptor {
    pub id: String,
    pub label: String,
    pub log: bool,
    pub language_id: Option<String>,
    pub source: Option<Vec<IOutputContentSource>>,
    pub extension_id: Option<String>,
    pub user: Option<bool>,
}

/// `IOutputChannel`. `update(mode)` e `update(mode, till)` são um método com `till` opcional.
pub trait IOutputChannel: IDisposable {
    fn id(&self) -> String;
    fn label(&self) -> String;
    fn uri(&self) -> URI;
    fn append(&self, output: &str);
    fn clear(&self);
    fn replace(&self, output: &str);
    /// `update(mode)` (só `Append`) ou `update(mode, till)`.
    fn update(&self, mode: OutputChannelUpdateMode, till: Option<u64>);
}

/// `IOutputService`, só o que os consumidores portados leem.
pub trait IOutputService {
    fn get_channel(&self, id: &str) -> Option<Rc<dyn IOutputChannel>>;
    fn get_active_channel(&self) -> Option<Rc<dyn IOutputChannel>>;
    fn show_channel(&self, id: &str, preserve_focus: bool) -> Promise<()>;
    /// `onActiveOutputChannel`: o id do canal que ficou ativo.
    fn on_active_output_channel(&self) -> Event<String>;
}

je_instantiation::service_id!(pub I_OUTPUT_SERVICE: dyn IOutputService = "outputService");

/// `IOutputChannelRegistry`.
pub trait IOutputChannelRegistry {
    fn on_did_register_channel(&self) -> Event<String>;
    fn on_did_remove_channel(&self) -> Event<IOutputChannelDescriptor>;
    fn register_channel(&self, descriptor: IOutputChannelDescriptor);
    fn get_channels(&self) -> Vec<IOutputChannelDescriptor>;
    fn get_channel(&self, id: &str) -> Option<IOutputChannelDescriptor>;
    fn remove_channel(&self, id: &str);
}

/// `OutputChannelRegistry`.
pub struct OutputChannelRegistry {
    channels: RefCell<Vec<IOutputChannelDescriptor>>,
    on_did_register_channel: Rc<Emitter<String>>,
    on_did_remove_channel: Rc<Emitter<IOutputChannelDescriptor>>,
}

impl OutputChannelRegistry {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { channels: RefCell::new(Vec::new()), on_did_register_channel: Emitter::new(), on_did_remove_channel: Emitter::new() })
    }
}

impl IOutputChannelRegistry for OutputChannelRegistry {
    fn on_did_register_channel(&self) -> Event<String> {
        self.on_did_register_channel.event()
    }

    fn on_did_remove_channel(&self) -> Event<IOutputChannelDescriptor> {
        self.on_did_remove_channel.event()
    }

    fn register_channel(&self, descriptor: IOutputChannelDescriptor) {
        let id = descriptor.id.clone();
        {
            let mut channels = self.channels.borrow_mut();
            if channels.iter().any(|channel| channel.id == id) {
                return;
            }
            channels.push(descriptor);
        }
        self.on_did_register_channel.fire(id);
    }

    fn get_channels(&self) -> Vec<IOutputChannelDescriptor> {
        self.channels.borrow().clone()
    }

    fn get_channel(&self, id: &str) -> Option<IOutputChannelDescriptor> {
        self.channels.borrow().iter().find(|channel| channel.id == id).cloned()
    }

    fn remove_channel(&self, id: &str) {
        let removed = {
            let mut channels = self.channels.borrow_mut();
            channels.iter().position(|channel| channel.id == id).map(|index| channels.remove(index))
        };
        if let Some(removed) = removed {
            self.on_did_remove_channel.fire(removed);
        }
    }
}

thread_local! {
    static REGISTRY: Rc<OutputChannelRegistry> = OutputChannelRegistry::new();
}

/// `Registry.as<IOutputChannelRegistry>(Extensions.OutputChannels)`.
pub fn output_channel_registry() -> Rc<OutputChannelRegistry> {
    REGISTRY.with(Rc::clone)
}
