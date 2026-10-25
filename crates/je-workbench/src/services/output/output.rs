//! Porte de `src/vs/workbench/services/output/common/output.ts`: as constantes, as chaves de contexto (`CONTEXT_IN_OUTPUT`...),
//! os filtros (`IOutputViewFilters`), `IOutputChannel`, `IOutputChannelDescriptor`, `IOutputService` e o `IOutputChannelRegistry`
//! com a implementação em memória [`OutputChannelRegistry`].
//!
//! Divergências:
//! - **`source?: IOutputContentSource | ReadonlyArray<IOutputContentSource>`** é o [`OutputChannelSource`] (uma fonte ou uma lista),
//!   e `isSingleSourceOutputChannelDescriptor` e `isMultiSourceOutputChannelDescriptor` são as funções de mesmo nome (em
//!   snake_case) mais os acessores [`IOutputChannelDescriptor::single_source`] e [`IOutputChannelDescriptor::multi_source`], que
//!   fazem o papel do estreitamento de tipo (`ISingleSourceOutputChannelDescriptor` e `IMultiSourceOutputChannelDescriptor`).
//!   `onDidUpdateChannelSources` emite o próprio [`IOutputChannelDescriptor`], já com a lista nova de fontes.
//! - **Chaves de contexto.** Cada `RawContextKey` é uma função (a primeira chamada registra a chave, e a função devolve sempre a
//!   mesma), como em `common/contextkeys.rs`.
//! - **`ILogEntry`** mora em `contrib/output/common/output_channel_model.rs` (onde o porte da fatia 1 o definiu) e é reexportado aqui.
//! - **Membros novos do contrato** (`filters`, `getChannelDescriptor`, `getChannelDescriptors`, `registerCompoundLogChannel`,
//!   `saveOutputAs`, `canSetLogLevel`, `getLogLevel`, `setLogLevel` e `getLogEntries`) têm corpo padrão que lança
//!   `TypeError: x is not a function`, o que um objeto parcial responde no original e deixa os dublês existentes valendo.
//!   `registerCompoundLogChannel(channels)` ordena a lista recebida no lugar (como o `descriptors.sort` do original), então recebe
//!   `&mut [IOutputChannelDescriptor]`, e `saveOutputAs(outputPath?, ...channels)` recebe as reticências como um `Vec`.
//! - **`IOutputViewFilters`** troca os pares `get x()`/`set x(v)` por `x()`/`set_x(v)`.
//! O `Registry.as<IOutputChannelRegistry>(Extensions.OutputChannels)` global é a função [`output_channel_registry`] (um registro por
//! thread, como o `commands_registry()` da plataforma).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};
use je_platform::log::LogLevel;

pub use crate::contrib::output::common::output_channel_model::ILogEntry;

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_a_function(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `OUTPUT_VIEW_ID`.
pub const OUTPUT_VIEW_ID: &str = "workbench.panel.output";

/// `OUTPUT_MIME`.
pub const OUTPUT_MIME: &str = "text/x-code-output";

/// `OUTPUT_MODE_ID`.
pub const OUTPUT_MODE_ID: &str = "Log";

/// `LOG_MIME`.
pub const LOG_MIME: &str = "text/x-code-log-output";

/// `LOG_MODE_ID`.
pub const LOG_MODE_ID: &str = "log";

macro_rules! raw_keys {
    ($($(#[$doc:meta])* $name:ident: $key:literal, $default:expr;)*) => {$(
        $(#[$doc])*
        pub fn $name() -> RawContextKey {
            thread_local! { static KEY: RawContextKey = RawContextKey::new($key, $default, ContextKeyMeta::None); }
            KEY.with(Clone::clone)
        }
    )*};
}

raw_keys! {
    /// `CONTEXT_IN_OUTPUT`.
    context_in_output: "inOutput", false;
    /// `CONTEXT_ACTIVE_FILE_OUTPUT`.
    context_active_file_output: "activeLogOutput", false;
    /// `CONTEXT_ACTIVE_LOG_FILE_OUTPUT`.
    context_active_log_file_output: "activeLogOutput.isLog", false;
    /// `CONTEXT_ACTIVE_OUTPUT_LEVEL_SETTABLE`.
    context_active_output_level_settable: "activeLogOutput.levelSettable", false;
    /// `CONTEXT_ACTIVE_OUTPUT_LEVEL`.
    context_active_output_level: "activeLogOutput.level", "";
    /// `CONTEXT_ACTIVE_OUTPUT_LEVEL_IS_DEFAULT`.
    context_active_output_level_is_default: "activeLogOutput.levelIsDefault", false;
    /// `CONTEXT_OUTPUT_SCROLL_LOCK`.
    context_output_scroll_lock: "outputView.scrollLock", false;
    /// `ACTIVE_OUTPUT_CHANNEL_CONTEXT`.
    active_output_channel_context: "activeOutputChannel", "";
    /// `SHOW_TRACE_FILTER_CONTEXT`.
    show_trace_filter_context: "output.filter.trace", true;
    /// `SHOW_DEBUG_FILTER_CONTEXT`.
    show_debug_filter_context: "output.filter.debug", true;
    /// `SHOW_INFO_FILTER_CONTEXT`.
    show_info_filter_context: "output.filter.info", true;
    /// `SHOW_WARNING_FILTER_CONTEXT`.
    show_warning_filter_context: "output.filter.warning", true;
    /// `SHOW_ERROR_FILTER_CONTEXT`.
    show_error_filter_context: "output.filter.error", true;
    /// `OUTPUT_FILTER_FOCUS_CONTEXT`.
    output_filter_focus_context: "outputFilterFocus", false;
    /// `HIDE_CATEGORY_FILTER_CONTEXT`.
    hide_category_filter_context: "output.filter.categories", "";
}

/// `IOutputViewFilters`.
pub trait IOutputViewFilters {
    fn on_did_change(&self) -> Event<()>;
    fn text(&self) -> String;
    fn set_text(&self, text: &str);
    fn include_patterns(&self) -> Vec<String>;
    fn exclude_patterns(&self) -> Vec<String>;
    fn trace(&self) -> bool;
    fn set_trace(&self, trace: bool);
    fn debug(&self) -> bool;
    fn set_debug(&self, debug: bool);
    fn info(&self) -> bool;
    fn set_info(&self, info: bool);
    fn warning(&self) -> bool;
    fn set_warning(&self, warning: bool);
    fn error(&self) -> bool;
    fn set_error(&self, error: bool);
    fn categories(&self) -> String;
    fn set_categories(&self, categories: &str);
    fn toggle_category(&self, category: &str);
    fn has_category(&self, category: &str) -> bool;
}

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

/// `IOutputContentSource | ReadonlyArray<IOutputContentSource>`.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputChannelSource {
    Single(IOutputContentSource),
    Multi(Vec<IOutputContentSource>),
}

/// `IOutputChannelDescriptor`.
#[derive(Clone, Debug, PartialEq)]
pub struct IOutputChannelDescriptor {
    pub id: String,
    pub label: String,
    pub log: bool,
    pub language_id: Option<String>,
    pub source: Option<OutputChannelSource>,
    pub extension_id: Option<String>,
    pub user: Option<bool>,
}

impl IOutputChannelDescriptor {
    /// `descriptor.source` de um `ISingleSourceOutputChannelDescriptor`.
    pub fn single_source(&self) -> Option<&IOutputContentSource> {
        match &self.source {
            Some(OutputChannelSource::Single(source)) => Some(source),
            _ => None,
        }
    }

    /// `descriptor.source` de um `IMultiSourceOutputChannelDescriptor`.
    pub fn multi_source(&self) -> Option<&[IOutputContentSource]> {
        match &self.source {
            Some(OutputChannelSource::Multi(sources)) => Some(sources),
            _ => None,
        }
    }
}

/// `isSingleSourceOutputChannelDescriptor`.
pub fn is_single_source_output_channel_descriptor(descriptor: &IOutputChannelDescriptor) -> bool {
    descriptor.single_source().is_some()
}

/// `isMultiSourceOutputChannelDescriptor`.
pub fn is_multi_source_output_channel_descriptor(descriptor: &IOutputChannelDescriptor) -> bool {
    descriptor.multi_source().is_some()
}

/// `IOutputChannel`. `update(mode)` e `update(mode, till)` são um método com `till` opcional.
pub trait IOutputChannel: IDisposable {
    fn id(&self) -> String;
    fn label(&self) -> String;
    fn uri(&self) -> URI;
    /// `getLogEntries()`.
    fn get_log_entries(&self) -> Vec<ILogEntry> {
        not_a_function("getLogEntries")
    }
    fn append(&self, output: &str);
    fn clear(&self);
    fn replace(&self, output: &str);
    /// `update(mode)` (só `Append`) ou `update(mode, till)`.
    fn update(&self, mode: OutputChannelUpdateMode, till: Option<u64>);
}

/// `IOutputService`.
pub trait IOutputService {
    /// `filters`: output view filters.
    fn filters(&self) -> Rc<dyn IOutputViewFilters> {
        not_a_function("filters")
    }

    fn get_channel(&self, id: &str) -> Option<Rc<dyn IOutputChannel>>;

    /// `getChannelDescriptor(id)`: given the channel id returns the registered output channel descriptor.
    fn get_channel_descriptor(&self, _id: &str) -> Option<IOutputChannelDescriptor> {
        not_a_function("getChannelDescriptor")
    }

    /// `getChannelDescriptors()`: returns an array of all known output channels descriptors.
    fn get_channel_descriptors(&self) -> Vec<IOutputChannelDescriptor> {
        not_a_function("getChannelDescriptors")
    }

    fn get_active_channel(&self) -> Option<Rc<dyn IOutputChannel>>;
    fn show_channel(&self, id: &str, preserve_focus: bool) -> Promise<()>;
    /// `onActiveOutputChannel`: o id do canal que ficou ativo.
    fn on_active_output_channel(&self) -> Event<String>;

    /// `registerCompoundLogChannel(channels)`: register a compound log channel with the given channels.
    fn register_compound_log_channel(&self, _channels: &mut [IOutputChannelDescriptor]) -> String {
        not_a_function("registerCompoundLogChannel")
    }

    /// `saveOutputAs(outputPath?, ...channels)`: save the logs to a file.
    fn save_output_as(&self, _output_path: Option<URI>, _channels: Vec<IOutputChannelDescriptor>) -> Promise<()> {
        not_a_function("saveOutputAs")
    }

    /// `canSetLogLevel(channel)`: checks if the log level can be set for the given channel.
    fn can_set_log_level(&self, _channel: &IOutputChannelDescriptor) -> bool {
        not_a_function("canSetLogLevel")
    }

    /// `getLogLevel(channel)`: returns the log level for the given channel.
    fn get_log_level(&self, _channel: &IOutputChannelDescriptor) -> Option<LogLevel> {
        not_a_function("getLogLevel")
    }

    /// `setLogLevel(channel, logLevel)`: sets the log level for the given channel.
    fn set_log_level(&self, _channel: &IOutputChannelDescriptor, _log_level: LogLevel) {
        not_a_function("setLogLevel")
    }
}

je_instantiation::service_id!(pub I_OUTPUT_SERVICE: dyn IOutputService = "outputService");

/// `IOutputChannelRegistry`.
pub trait IOutputChannelRegistry {
    fn on_did_register_channel(&self) -> Event<String>;
    fn on_did_remove_channel(&self) -> Event<IOutputChannelDescriptor>;
    /// `onDidUpdateChannelSources`: o descritor de várias fontes, já atualizado.
    fn on_did_update_channel_sources(&self) -> Event<IOutputChannelDescriptor>;
    fn register_channel(&self, descriptor: IOutputChannelDescriptor);
    /// `updateChannelSources(id, sources)`: update the files for the given output channel.
    fn update_channel_sources(&self, id: &str, sources: Vec<IOutputContentSource>);
    fn get_channels(&self) -> Vec<IOutputChannelDescriptor>;
    fn get_channel(&self, id: &str) -> Option<IOutputChannelDescriptor>;
    fn remove_channel(&self, id: &str);
}

/// `OutputChannelRegistry`.
pub struct OutputChannelRegistry {
    channels: RefCell<Vec<IOutputChannelDescriptor>>,
    on_did_register_channel: Rc<Emitter<String>>,
    on_did_remove_channel: Rc<Emitter<IOutputChannelDescriptor>>,
    on_did_update_channel_files: Rc<Emitter<IOutputChannelDescriptor>>,
}

impl OutputChannelRegistry {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            channels: RefCell::new(Vec::new()),
            on_did_register_channel: Emitter::new(),
            on_did_remove_channel: Emitter::new(),
            on_did_update_channel_files: Emitter::new(),
        })
    }
}

impl IOutputChannelRegistry for OutputChannelRegistry {
    fn on_did_register_channel(&self) -> Event<String> {
        self.on_did_register_channel.event()
    }

    fn on_did_remove_channel(&self) -> Event<IOutputChannelDescriptor> {
        self.on_did_remove_channel.event()
    }

    fn on_did_update_channel_sources(&self) -> Event<IOutputChannelDescriptor> {
        self.on_did_update_channel_files.event()
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

    fn update_channel_sources(&self, id: &str, sources: Vec<IOutputContentSource>) {
        let updated = {
            let mut channels = self.channels.borrow_mut();
            match channels.iter_mut().find(|channel| channel.id == id) {
                Some(channel) if is_multi_source_output_channel_descriptor(channel) => {
                    channel.source = Some(OutputChannelSource::Multi(sources));
                    Some(channel.clone())
                }
                _ => None,
            }
        };
        if let Some(channel) = updated {
            self.on_did_update_channel_files.fire(channel);
        }
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
