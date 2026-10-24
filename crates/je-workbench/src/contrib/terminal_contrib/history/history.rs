//! Porte de `src/vs/workbench/contrib/terminalContrib/history/common/history.ts`: o histórico persistido do
//! terminal (`TerminalPersistedHistory`, com os singletons de comandos e de diretórios) e a leitura dos arquivos de
//! histórico de bash, zsh, fish, pwsh e python.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor (`@IConfigurationService`, `@IStorageService`) são a tupla
//!   `Deps` do [`Injectable`], depois do argumento estático (a chave de armazenamento). `ServicesAccessor` é o
//!   [`ServicesAccessor`] e `accessor.get(IInstantiationService)` é `accessor.get(I_INSTANTIATION_SERVICE)`.
//! - **Genérico `T` do histórico.** O original guarda qualquer valor que o `JSON.stringify` aceite. Aqui `T` é um
//!   [`PersistedHistoryValue`], que sabe se escrever e se ler como [`Value`]: `f64` (o `number` da suíte),
//!   [`DirectoryHistoryEntry`] (`{ remoteAuthority?: string }`) e [`CommandHistoryEntry`] (`{ shellType }`). Uma
//!   entrada gravada com outro formato é ignorada na leitura, e um `shellType` desconhecido vira `None`.
//! - **`entries`.** O `IterableIterator<[string, T]>` é um `Vec<(String, T)>` com o estado do cache naquele
//!   instante (o iterador do original falharia se o cache mudasse durante a iteração).
//! - **Limite do cache.** `LRUCache.limit` é `usize`: um limite negativo vira 0 e um fracionário é truncado; `NaN`
//!   (que `isNumber` rejeita) cai no padrão 100.
//! - **Estado persistido inválido.** Um texto que não é JSON vale como ausente (o `catch` do original). Um JSON
//!   verdadeiro que não é um objeto com `entries` em lista faz o original lançar `TypeError` ao iterar, e aqui
//!   lança o mesmo erro com [`errors::throw`].
//! - **`process.env`.** O ambiente do processo é mutável no Node e os testes o alteram (`env['HOME'] = ...`); em Rust
//!   a edição 2024 exige `unsafe` para mudar o ambiente real e os testes rodam em paralelo. O histórico lê o `env` do
//!   porte de `process.ts` ([`env_var`]), a cópia do ambiente do processo guardada por thread.
//! - **`isFileWindows` de `fetchFileContents`.** O parâmetro nunca é lido no original e não existe aqui.
//! - **`Map` e `Set`.** O `Set<string>` de cada leitor é um [`IndexSet`] (ordem de inserção); o `Map` do cache de
//!   histórico de arquivos guarda `Option<entrada>`, em que `None` é o `null` do original.
//! - **`async`.** As funções `fetch*` leem os serviços do `accessor` na hora da chamada (o `accessor` só vale durante a
//!   invocação) e devolvem a [`Promise`] do corpo `async`, que roda já até o primeiro `await`.
//! - **`connection.remoteAuthority`.** É [`IRemoteAgentService::get_connection`] seguido de `remote_authority()`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use indexmap::IndexSet;
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::js_string::is_js_whitespace;
use je_base::json::{Object, Value};
use je_base::json_formatter::json_stringify;
use je_base::jsonc::json_parse;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::LRUCache;
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::{OperatingSystem, Platform};
use je_base::process::env_var;
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::{URI, UriComponents};
use je_instantiation::{I_INSTANTIATION_SERVICE, Injectable, ServicesAccessor};
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileOperationError, FileOperationResult};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::remote::remote_agent_environment::RemoteAgentEnvironment;
use je_platform::resources::{fs_path, host_platform};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::terminal::terminal::{GeneralShellType, PosixShellType, ShellType, TerminalShellType, WindowsShellType};
use je_storage::StorageValue;

use super::terminal_history::TerminalHistorySettingId;
use crate::services::remote::remote_agent_service::{I_REMOTE_AGENT_SERVICE, IRemoteAgentService};

/// `ITerminalPersistedHistory<T>`: tracks a list of generic entries.
pub trait ITerminalPersistedHistory<T> {
    /// The persisted entries.
    fn entries(&self) -> Vec<(String, T)>;

    /// Adds an entry.
    fn add(&self, key: &str, value: T);

    /// Removes an entry.
    fn remove(&self, key: &str);

    /// Clears all entries.
    fn clear(&self);
}

/// O que um valor do histórico precisa saber para ir e voltar do JSON que o armazenamento guarda.
pub trait PersistedHistoryValue: Clone + 'static {
    /// O valor que o `JSON.stringify` do original escreveria.
    fn to_json(&self) -> Value;

    /// O valor lido do JSON, ou `None` se o formato não é o esperado.
    fn from_json(value: &Value) -> Option<Self>;
}

impl PersistedHistoryValue for f64 {
    fn to_json(&self) -> Value {
        Value::Number(*self)
    }

    fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }
}

/// `{ remoteAuthority?: string }`: o valor do histórico de diretórios.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryHistoryEntry {
    pub remote_authority: Option<String>,
}

impl PersistedHistoryValue for DirectoryHistoryEntry {
    fn to_json(&self) -> Value {
        let mut object = Object::new();
        if let Some(remote_authority) = &self.remote_authority {
            object.insert(JsString::from("remoteAuthority"), Value::String(JsString::from(remote_authority.as_str())));
        }
        Value::Object(object)
    }

    fn from_json(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else { return None };
        Some(Self {
            remote_authority: match object.get("remoteAuthority") {
                Some(Value::String(remote_authority)) => Some(remote_authority.to_string_lossy()),
                _ => None,
            },
        })
    }
}

/// `{ shellType: TerminalShellType }`: o valor do histórico de comandos.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandHistoryEntry {
    pub shell_type: TerminalShellType,
}

/// O `ShellType` cujo valor do `const enum` é `text`.
fn shell_type_from_str(text: &str) -> TerminalShellType {
    const POSIX: [PosixShellType; 6] =
        [PosixShellType::Bash, PosixShellType::Fish, PosixShellType::Sh, PosixShellType::Csh, PosixShellType::Ksh, PosixShellType::Zsh];
    const WINDOWS: [WindowsShellType; 3] = [WindowsShellType::CommandPrompt, WindowsShellType::Wsl, WindowsShellType::GitBash];
    const GENERAL: [GeneralShellType; 11] = [
        GeneralShellType::Claude,
        GeneralShellType::Codex,
        GeneralShellType::CommandCode,
        GeneralShellType::Copilot,
        GeneralShellType::Gemini,
        GeneralShellType::PowerShell,
        GeneralShellType::Python,
        GeneralShellType::Julia,
        GeneralShellType::NuShell,
        GeneralShellType::Node,
        GeneralShellType::Xonsh,
    ];
    POSIX
        .iter()
        .find(|shell| shell.as_str() == text)
        .map(|shell| ShellType::Posix(*shell))
        .or_else(|| WINDOWS.iter().find(|shell| shell.as_str() == text).map(|shell| ShellType::Windows(*shell)))
        .or_else(|| GENERAL.iter().find(|shell| shell.as_str() == text).map(|shell| ShellType::General(*shell)))
}

impl PersistedHistoryValue for CommandHistoryEntry {
    fn to_json(&self) -> Value {
        let mut object = Object::new();
        if let Some(shell_type) = self.shell_type {
            object.insert(JsString::from("shellType"), Value::String(JsString::from(shell_type.as_str())));
        }
        Value::Object(object)
    }

    fn from_json(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else { return None };
        Some(Self {
            shell_type: match object.get("shellType") {
                Some(Value::String(shell_type)) => shell_type_from_str(&shell_type.to_string_lossy()),
                _ => None,
            },
        })
    }
}

/// `Constants.DefaultHistoryLimit`.
const DEFAULT_HISTORY_LIMIT: usize = 100;

/// `StorageKeys.Entries`.
const STORAGE_KEY_ENTRIES: &str = "terminal.history.entries";

/// `StorageKeys.Timestamp`.
const STORAGE_KEY_TIMESTAMP: &str = "terminal.history.timestamp";

thread_local! {
    static DIRECTORY_HISTORY: RefCell<Option<Rc<TerminalPersistedHistory<DirectoryHistoryEntry>>>> = const { RefCell::new(None) };
    static COMMAND_HISTORY: RefCell<Option<Rc<TerminalPersistedHistory<CommandHistoryEntry>>>> = const { RefCell::new(None) };
}

/// `getDirectoryHistory(accessor)`: o histórico de diretórios, criado na primeira chamada.
pub fn get_directory_history(accessor: &ServicesAccessor) -> Rc<dyn ITerminalPersistedHistory<DirectoryHistoryEntry>> {
    singleton_history(&DIRECTORY_HISTORY, accessor, "dirs")
}

/// `getCommandHistory(accessor)`: o histórico de comandos, criado na primeira chamada.
pub fn get_command_history(accessor: &ServicesAccessor) -> Rc<dyn ITerminalPersistedHistory<CommandHistoryEntry>> {
    singleton_history(&COMMAND_HISTORY, accessor, "commands")
}

/// O singleton do módulo (`let directoryHistory`, `let commandHistory`) criado por
/// `accessor.get(IInstantiationService).createInstance(TerminalPersistedHistory, key)`.
fn singleton_history<T: PersistedHistoryValue>(
    slot: &'static std::thread::LocalKey<RefCell<Option<Rc<TerminalPersistedHistory<T>>>>>,
    accessor: &ServicesAccessor,
    storage_data_key: &str,
) -> Rc<dyn ITerminalPersistedHistory<T>> {
    let existing = slot.with(|slot| slot.borrow().clone());
    match existing {
        Some(history) => history,
        None => {
            let history = accessor.get(I_INSTANTIATION_SERVICE).create_instance::<TerminalPersistedHistory<T>>(storage_data_key.to_owned());
            slot.with(|slot| *slot.borrow_mut() = Some(history.clone()));
            history
        }
    }
}

/// O limite lido da configuração `terminal.integrated.shellIntegration.history` (`_getHistoryLimit`).
fn history_limit(configuration_service: &dyn IConfigurationService) -> usize {
    match configuration_service.get_value(Some(TerminalHistorySettingId::ShellIntegrationCommandHistory.as_str()), None) {
        Dynamic::Number(limit) if !limit.is_nan() => limit.max(0.0) as usize,
        _ => DEFAULT_HISTORY_LIMIT,
    }
}

/// `TerminalPersistedHistory`.
pub struct TerminalPersistedHistory<T: PersistedHistoryValue> {
    base: Rc<Disposable>,
    /// `this._store`: o repositório que o `onDidChangeValue` do armazenamento recebe.
    store: Rc<DisposableStore>,
    storage_data_key: String,
    configuration_service: Rc<dyn IConfigurationService>,
    storage_service: Rc<dyn StorageService>,
    entries: LRUCache<String, T>,
    timestamp: Cell<f64>,
    is_ready: Cell<bool>,
    is_stale: Cell<bool>,
}

impl<T: PersistedHistoryValue> Injectable for TerminalPersistedHistory<T> {
    type Args = String;
    type Deps = (Rc<dyn IConfigurationService>, Rc<dyn StorageService>);

    fn create(storage_data_key: String, (configuration_service, storage_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());

        // Init cache
        let entries = LRUCache::new(history_limit(&*configuration_service), 1.0);

        let this = Rc::new(Self {
            base,
            store,
            storage_data_key,
            configuration_service,
            storage_service,
            entries,
            timestamp: Cell::new(0.0),
            is_ready: Cell::new(false),
            is_stale: Cell::new(true),
        });

        // Listen for config changes to set history limit
        let weak = Rc::downgrade(&this);
        this.base.register(this.configuration_service.on_did_change_configuration().listen(move |event| {
            let Some(this) = weak.upgrade() else { return };
            if event.affects_configuration(TerminalHistorySettingId::ShellIntegrationCommandHistory.as_str(), None) {
                this.entries.set_limit(history_limit(&*this.configuration_service));
            }
        }));

        // Listen to cache changes from other windows
        let weak = Rc::downgrade(&this);
        let timestamp_key = this.timestamp_storage_key();
        let changes = this.storage_service.on_did_change_value(StorageScope::Application, Some(timestamp_key.as_str()), &this.store);
        this.base.register(changes.listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if !this.is_stale.get() {
                this.is_stale.set(this.stored_timestamp() != this.timestamp.get());
            }
        }));

        this
    }
}

impl<T: PersistedHistoryValue> IDisposable for TerminalPersistedHistory<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl<T: PersistedHistoryValue> ITerminalPersistedHistory<T> for TerminalPersistedHistory<T> {
    fn entries(&self) -> Vec<(String, T)> {
        self.ensure_up_to_date();
        self.entries.entries().collect()
    }

    fn add(&self, key: &str, value: T) {
        self.ensure_up_to_date();
        self.entries.set(key.to_owned(), value);
        self.save_state();
    }

    fn remove(&self, key: &str) {
        self.ensure_up_to_date();
        self.entries.delete(&key.to_owned());
        self.save_state();
    }

    fn clear(&self) {
        self.ensure_up_to_date();
        self.entries.clear();
        self.save_state();
    }
}

impl<T: PersistedHistoryValue> TerminalPersistedHistory<T> {
    fn ensure_up_to_date(&self) {
        // Initial load
        if !self.is_ready.get() {
            self.load_state();
            self.is_ready.set(true);
        }

        // React to stale cache caused by another window
        if self.is_stale.get() {
            // Since state is saved whenever the entries change, it's a safe assumption that no
            // merging of entries needs to happen, just loading the new state.
            self.entries.clear();
            self.load_state();
            self.is_stale.set(false);
        }
    }

    fn load_state(&self) {
        self.timestamp.set(self.stored_timestamp());

        // Load global entries plus
        if let Some(serialized) = self.load_persisted_state() {
            for (key, value) in serialized {
                self.entries.set(key, value);
            }
        }
    }

    /// O `ISerializedCache<T>` gravado, como a lista de entradas `{ key, value }`.
    fn load_persisted_state(&self) -> Option<Vec<(String, T)>> {
        let raw = self.storage_service.get(&self.entries_storage_key(), StorageScope::Application, None)?;
        if raw.is_empty() {
            return None;
        }
        // Invalid data
        let serialized = json_parse(JsString::from(raw.as_str()).units()).ok()?;
        let entries = match &serialized {
            Value::Object(object) => match object.get("entries") {
                Some(Value::Array(entries)) => entries,
                _ => errors::throw(Error::type_error("serialized.entries is not iterable")),
            },
            Value::Null | Value::Bool(false) => return None,
            Value::String(text) if text.is_empty() => return None,
            Value::Number(number) if *number == 0.0 || number.is_nan() => return None,
            _ => errors::throw(Error::type_error("serialized.entries is not iterable")),
        };
        Some(
            entries
                .iter()
                .filter_map(|entry| {
                    let Value::Object(entry) = entry else { return None };
                    let Some(Value::String(key)) = entry.get("key") else { return None };
                    Some((key.to_string_lossy(), T::from_json(entry.get("value")?)?))
                })
                .collect(),
        )
    }

    fn save_state(&self) {
        let mut entries: Vec<Value> = Vec::new();
        self.entries.for_each(|value, key, _| {
            let mut entry = Object::new();
            entry.insert(JsString::from("key"), Value::String(JsString::from(key.as_str())));
            entry.insert(JsString::from("value"), value.to_json());
            entries.push(Value::Object(entry));
        });
        let mut serialized = Object::new();
        serialized.insert(JsString::from("entries"), Value::Array(entries));
        self.storage_service.store(
            &self.entries_storage_key(),
            StorageValue::String(json_stringify(&Value::Object(serialized), None)),
            StorageScope::Application,
            StorageTarget::Machine,
        );
        self.timestamp.set(date_now() as f64);
        self.storage_service.store(&self.timestamp_storage_key(), StorageValue::Number(self.timestamp.get()), StorageScope::Application, StorageTarget::Machine);
    }

    /// `this._storageService.getNumber(this._getTimestampStorageKey(), StorageScope.APPLICATION, 0)`.
    fn stored_timestamp(&self) -> f64 {
        self.storage_service.get_number(&self.timestamp_storage_key(), StorageScope::Application, Some(0.0)).unwrap_or(0.0)
    }

    fn timestamp_storage_key(&self) -> String {
        format!("{STORAGE_KEY_TIMESTAMP}.{}", self.storage_data_key)
    }

    fn entries_storage_key(&self) -> String {
        format!("{STORAGE_KEY_ENTRIES}.{}", self.storage_data_key)
    }
}

/// `IShellFileHistoryEntry`: o histórico de um shell lido do arquivo dele.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellFileHistoryEntry {
    pub source_label: String,
    pub source_resource: URI,
    pub commands: Vec<String>,
}

thread_local! {
    /// `shellFileHistory`: o histórico dos arquivos carrega uma vez por shell por janela. `None` é o `null` do original
    /// (o shell não tem arquivo de histórico).
    static SHELL_FILE_HISTORY: RefCell<HashMap<TerminalShellType, Option<ShellFileHistoryEntry>>> = RefCell::new(HashMap::new());

}

/// `getShellFileHistory(accessor, shellType)`.
pub fn get_shell_file_history(accessor: &ServicesAccessor, shell_type: TerminalShellType) -> Promise<Option<ShellFileHistoryEntry>> {
    match SHELL_FILE_HISTORY.with(|cache| cache.borrow().get(&shell_type).cloned()) {
        Some(None) => return Promise::resolved(None),
        Some(cached) => return Promise::resolved(cached),
        None => {}
    }
    let fetch = match shell_type {
        Some(ShellType::Posix(PosixShellType::Bash)) => fetch_bash_history(accessor),
        Some(ShellType::General(GeneralShellType::PowerShell)) => fetch_pwsh_history(accessor),
        Some(ShellType::Posix(PosixShellType::Zsh)) => fetch_zsh_history(accessor),
        Some(ShellType::Posix(PosixShellType::Fish)) => fetch_fish_history(accessor),
        Some(ShellType::General(GeneralShellType::Python)) => fetch_python_history(accessor),
        _ => return Promise::resolved(None),
    };
    fetch.then(move |result| {
        SHELL_FILE_HISTORY.with(|cache| cache.borrow_mut().insert(shell_type, result.clone()));
        result
    })
}

/// `clearShellFileHistory()`.
pub fn clear_shell_file_history() {
    SHELL_FILE_HISTORY.with(|cache| cache.borrow_mut().clear());
}

/// Onde fica o arquivo de histórico de um shell: o rótulo, a pasta de base e o caminho dentro dela.
struct HistorySource {
    source_label: String,
    folder_prefix: Option<String>,
    file_path: String,
}

/// `remoteEnvironment?.os === OperatingSystem.Windows || !remoteEnvironment && isWindows`: o arquivo está num Windows.
fn targets_windows(remote_environment: &Option<RemoteAgentEnvironment>) -> bool {
    match remote_environment {
        Some(remote_environment) => remote_environment.os == OperatingSystem::Windows,
        None => Platform::detect().is_windows,
    }
}

/// `remoteEnvironment?.userHome?.fsPath ?? env['HOME']`.
fn user_home(remote_environment: &Option<RemoteAgentEnvironment>) -> Option<String> {
    match remote_environment {
        Some(remote_environment) => Some(fs_path(&remote_environment.user_home)),
        None => env_var("HOME"),
    }
}

/// O corpo comum dos `fetch*History`: lê o ambiente remoto, descobre o arquivo (`locate` devolve `None` quando o shell
/// não tem histórico naquele sistema), lê o conteúdo e o interpreta com `parse`. O arquivo ausente dá `None`.
fn fetch_shell_history(
    accessor: &ServicesAccessor,
    locate: impl FnOnce(&Option<RemoteAgentEnvironment>) -> Option<HistorySource> + 'static,
    parse: fn(&str) -> Vec<String>,
) -> Promise<Option<ShellFileHistoryEntry>> {
    let file_service = accessor.get(I_FILE_SERVICE);
    let remote_agent_service = accessor.get(I_REMOTE_AGENT_SERVICE);
    async_fn(async move {
        let remote_environment = remote_agent_service.get_environment().await?;
        let Some(source) = locate(&remote_environment) else { return Ok(None) };
        let Some(resolved_file) = fetch_file_contents(source.folder_prefix, &source.file_path, &*file_service, &*remote_agent_service).await? else {
            return Ok(None);
        };
        Ok(Some(ShellFileHistoryEntry { source_label: source.source_label, source_resource: resolved_file.resource, commands: parse(&resolved_file.content) }))
    })
}

/// `fetchBashHistory(accessor)`.
pub fn fetch_bash_history(accessor: &ServicesAccessor) -> Promise<Option<ShellFileHistoryEntry>> {
    fetch_shell_history(
        accessor,
        |remote_environment| {
            if targets_windows(remote_environment) {
                return None;
            }
            Some(HistorySource { source_label: "~/.bash_history".to_owned(), folder_prefix: user_home(remote_environment), file_path: ".bash_history".to_owned() })
        },
        parse_bash_history,
    )
}

/// `fetchZshHistory(accessor)`.
pub fn fetch_zsh_history(accessor: &ServicesAccessor) -> Promise<Option<ShellFileHistoryEntry>> {
    fetch_shell_history(
        accessor,
        |remote_environment| {
            if targets_windows(remote_environment) {
                return None;
            }
            Some(HistorySource { source_label: "~/.zsh_history".to_owned(), folder_prefix: user_home(remote_environment), file_path: ".zsh_history".to_owned() })
        },
        parse_zsh_history,
    )
}

/// `fetchPythonHistory(accessor)`.
pub fn fetch_python_history(accessor: &ServicesAccessor) -> Promise<Option<ShellFileHistoryEntry>> {
    fetch_shell_history(
        accessor,
        |remote_environment| {
            Some(HistorySource { source_label: "~/.python_history".to_owned(), folder_prefix: user_home(remote_environment), file_path: ".python_history".to_owned() })
        },
        parse_python_history,
    )
}

/// `fetchPwshHistory(accessor)`.
pub fn fetch_pwsh_history(accessor: &ServicesAccessor) -> Promise<Option<ShellFileHistoryEntry>> {
    fetch_shell_history(
        accessor,
        |remote_environment| {
            if targets_windows(remote_environment) {
                return Some(HistorySource {
                    source_label: "$APPDATA\\Microsoft\\Windows\\PowerShell\\PSReadLine\\ConsoleHost_history.txt".to_owned(),
                    folder_prefix: env_var("APPDATA"),
                    file_path: "Microsoft\\Windows\\PowerShell\\PSReadLine\\ConsoleHost_history.txt".to_owned(),
                });
            }
            let file_path = ".local/share/powershell/PSReadline/ConsoleHost_history.txt";
            Some(HistorySource { source_label: format!("~/{file_path}"), folder_prefix: user_home(remote_environment), file_path: file_path.to_owned() })
        },
        parse_pwsh_history,
    )
}

/// `fetchFishHistory(accessor)`.
pub fn fetch_fish_history(accessor: &ServicesAccessor) -> Promise<Option<ShellFileHistoryEntry>> {
    fetch_shell_history(
        accessor,
        |remote_environment| {
            if targets_windows(remote_environment) {
                return None;
            }

            // From `fish` docs:
            // > The command history is stored in the file ~/.local/share/fish/fish_history
            //   (or $XDG_DATA_HOME/fish/fish_history if that variable is set) by default.
            //
            // (https://fishshell.com/docs/current/interactive.html#history-search)
            let overriden_data_home = env_var("XDG_DATA_HOME").filter(|data_home| !data_home.is_empty());

            // TODO: Unchecked fish behavior:
            // What if XDG_DATA_HOME was defined but somehow $XDG_DATA_HOME/fish/fish_history
            // was not exist. Does fish fall back to ~/.local/share/fish/fish_history?
            Some(match overriden_data_home {
                Some(data_home) => HistorySource {
                    source_label: "$XDG_DATA_HOME/fish/fish_history".to_owned(),
                    folder_prefix: Some(data_home),
                    file_path: "fish/fish_history".to_owned(),
                },
                None => HistorySource {
                    source_label: "~/.local/share/fish/fish_history".to_owned(),
                    folder_prefix: user_home(remote_environment),
                    file_path: ".local/share/fish/fish_history".to_owned(),
                },
            })
        },
        parse_fish_history,
    )
}

/// `str.trim()` do JavaScript (não é o `trim` do Rust: o U+FEFF conta e o U+0085 não).
fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace_char)
}

/// `str.trimStart()` do JavaScript.
fn js_trim_start(text: &str) -> &str {
    text.trim_start_matches(is_js_whitespace_char)
}

fn is_js_whitespace_char(character: char) -> bool {
    u16::try_from(u32::from(character)).is_ok_and(is_js_whitespace)
}

/// O corpo de `fetchBashHistory` depois da leitura: `.bash_history` does not differentiate wrapped commands from
/// multiple commands, então as aspas abertas continuam o comando na linha seguinte.
fn parse_bash_history(content: &str) -> Vec<String> {
    let mut result: IndexSet<String> = IndexSet::new();
    let mut current_command: Option<String> = None;
    let mut wrap_char: Option<char> = None;
    for current_line in content.split('\n') {
        let command = match current_command.take() {
            None => current_line.to_owned(),
            Some(mut command) => {
                command.push('\n');
                command.push_str(current_line);
                command
            }
        };
        for character in current_line.chars() {
            if let Some(wrap) = wrap_char {
                if character == wrap {
                    wrap_char = None;
                }
            } else if character == '\'' || character == '"' {
                wrap_char = Some(character);
            }
        }
        if wrap_char.is_none() {
            if !command.is_empty() {
                result.insert(js_trim(&command).to_owned());
            }
        } else {
            current_command = Some(command);
        }
    }
    result.into_iter().collect()
}

/// O fim do casamento de `/\:\s\d+\:\d+;/` em `chars[at..]` (a marca de data do histórico estendido do zsh).
fn match_zsh_extended_marker(chars: &[char], at: usize) -> Option<usize> {
    let skip_digits = |from: usize| from + chars[from.min(chars.len())..].iter().take_while(|character| character.is_ascii_digit()).count();
    let mut index = at;
    if chars.get(index) != Some(&':') {
        return None;
    }
    index += 1;
    if !chars.get(index).is_some_and(|character| is_js_whitespace_char(*character)) {
        return None;
    }
    index += 1;
    let digits_end = skip_digits(index);
    if digits_end == index {
        return None;
    }
    index = digits_end;
    if chars.get(index) != Some(&':') {
        return None;
    }
    index += 1;
    let digits_end = skip_digits(index);
    if digits_end == index {
        return None;
    }
    index = digits_end;
    if chars.get(index) != Some(&';') {
        return None;
    }
    Some(index + 1)
}

/// `content.split(/\:\s\d+\:\d+;/)`.
fn split_zsh_extended(chars: &[char]) -> Vec<&[char]> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < chars.len() {
        match match_zsh_extended_marker(chars, index) {
            Some(end) => {
                pieces.push(&chars[start..index]);
                start = end;
                index = end;
            }
            None => index += 1,
        }
    }
    pieces.push(&chars[start..]);
    pieces
}

/// `content.split(/(?<!\\)\n/)`.
fn split_zsh_simple(chars: &[char]) -> Vec<&[char]> {
    let mut pieces = Vec::new();
    let mut start = 0;
    for (index, character) in chars.iter().enumerate() {
        if *character == '\n' && (index == 0 || chars[index - 1] != '\\') {
            pieces.push(&chars[start..index]);
            start = index + 1;
        }
    }
    pieces.push(&chars[start..]);
    pieces
}

/// O corpo de `fetchZshHistory` depois da leitura.
fn parse_zsh_history(content: &str) -> Vec<String> {
    let chars: Vec<char> = content.chars().collect();
    let is_extended_history = match_zsh_extended_marker(&chars, 0).is_some();
    let file_lines = if is_extended_history { split_zsh_extended(&chars) } else { split_zsh_simple(&chars) };
    let mut result: IndexSet<String> = IndexSet::new();
    for file_line in file_lines {
        let line: String = file_line.iter().collect();
        let line = line.replace("\\\n", "\n");
        let sanitized = js_trim(&line);
        if !sanitized.is_empty() {
            result.insert(sanitized.to_owned());
        }
    }
    result.into_iter().collect()
}

/// O corpo de `fetchPythonHistory` depois da leitura: the Python history file is a simple text file with one command
/// per line.
fn parse_python_history(content: &str) -> Vec<String> {
    let mut result: IndexSet<String> = IndexSet::new();
    for line in content.split('\n') {
        if !js_trim(line).is_empty() {
            result.insert(js_trim(line).to_owned());
        }
    }
    result.into_iter().collect()
}

/// O corpo de `fetchPwshHistory` depois da leitura.
fn parse_pwsh_history(content: &str) -> Vec<String> {
    let mut result: IndexSet<String> = IndexSet::new();
    let mut current_command: Option<String> = None;
    let mut wrap_char: Option<char> = None;
    for current_line in content.split('\n') {
        let command = match current_command.take() {
            None => current_line.to_owned(),
            Some(mut command) => {
                command.push('\n');
                command.push_str(current_line);
                command
            }
        };
        if !current_line.ends_with('`') {
            let sanitized = js_trim(&command);
            if !sanitized.is_empty() {
                result.insert(sanitized.to_owned());
            }
            continue;
        }
        // If the line ends with `, the line may be wrapped. Need to also test the case where ` is
        // the last character in the line
        for character in current_line.chars() {
            if let Some(wrap) = wrap_char {
                if character == wrap {
                    wrap_char = None;
                }
            } else if character == '`' {
                wrap_char = Some(character);
            }
        }
        // Having an even number of backticks means the line is terminated
        // TODO: This doesn't cover more complicated cases where ` is within quotes
        if wrap_char.is_none() {
            let sanitized = js_trim(&command);
            if !sanitized.is_empty() {
                result.insert(sanitized.to_owned());
            }
        } else {
            // Remove trailing backtick
            current_command = Some(command.strip_suffix('`').unwrap_or(&command).to_owned());
            wrap_char = None;
        }
    }
    result.into_iter().collect()
}

/// O corpo de `fetchFishHistory` depois da leitura.
///
/// These apply to `fish` v3.5.1:
/// - It looks like YAML but it's not. It's, quoting, *"a broken psuedo-YAML"*.
///   See these discussions for more details:
///   - https://github.com/fish-shell/fish-shell/pull/6493
///   - https://github.com/fish-shell/fish-shell/issues/3341
/// - Every record should exactly start with `- cmd:` (the whitespace between `-` and `cmd` cannot be replaced with tab)
/// - Both `- cmd: echo 1` and `- cmd:echo 1` are valid entries.
/// - Backslashes are esacped as `\\`.
/// - Multiline commands are joined with a `\n` sequence, hence they're read as single line commands.
/// - Property `when` is optional.
/// - History navigation respects the records order and ignore the actual `when` property values (chronological order).
/// - If `cmd` value is multiline , it just takes the first line. Also YAML operators like `>-` or `|-` are not supported.
fn parse_fish_history(content: &str) -> Vec<String> {
    let mut result: IndexSet<String> = IndexSet::new();
    for command in content.split('\n').filter(|line| line.starts_with("- cmd:")).map(|line| js_trim_start(&line["- cmd:".len()..])) {
        let sanitized = sanitize_fish_history_cmd(command);
        let sanitized = js_trim(&sanitized);
        if !sanitized.is_empty() {
            result.insert(sanitized.to_owned());
        }
    }
    result.into_iter().collect()
}

/// `sanitizeFishHistoryCmd(cmd)`.
///
/// NOTE
/// This repeatedReplace() call can be eliminated by using look-ahead
/// caluses in the original RegExp pattern:
///
/// >>> ```ts
/// >>> cmds[i].replace(/(?<=^|[^\\])((?:\\\\)*)(\\n)/g, '$1\n')
/// >>> ```
///
/// But since not all browsers support look aheads we opted to a simple
/// pattern and repeatedly calling replace method.
pub fn sanitize_fish_history_cmd(cmd: &str) -> String {
    let mut current = cmd.to_owned();
    loop {
        let next = replace_fish_new_lines(&current);
        if next == current {
            return current;
        }
        current = next;
    }
}

/// `value.replace(/(^|[^\\])((?:\\\\)*)(\\n)/g, '$1$2\n')`, uma passada: no começo do texto ou depois de um caractere que
/// não é barra invertida, uma sequência de barras invertidas de tamanho ímpar seguida de `n` perde a última barra e o
/// `n` e vira uma quebra de linha (as barras invertidas em pares ficam).
fn replace_fish_new_lines(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    // O tamanho da corrida de barras invertidas que começa em `from`.
    let backslash_run = |from: usize| chars[from.min(chars.len())..].iter().take_while(|character| **character == '\\').count();
    // `((?:\\\\)*)(\\n)` a partir de `from`: o fim do casamento e a quantidade de barras invertidas que ficam.
    let match_at = |from: usize| {
        let run = backslash_run(from);
        (run % 2 == 1 && chars.get(from + run) == Some(&'n')).then(|| (from + run + 1, run - 1))
    };
    let mut result = String::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        // `^` só casa no começo do texto, com o primeiro grupo vazio; senão o primeiro grupo é um caractere que não é `\`.
        let matched = match (index == 0).then(|| match_at(0)).flatten() {
            Some((end, kept)) => Some((end, None, kept)),
            None if chars[index] != '\\' => match_at(index + 1).map(|(end, kept)| (end, Some(chars[index]), kept)),
            None => None,
        };
        match matched {
            Some((end, prefix, kept)) => {
                result.extend(prefix);
                result.extend(std::iter::repeat_n('\\', kept));
                result.push('\n');
                index = end;
            }
            None => {
                result.push(chars[index]);
                index += 1;
            }
        }
    }
    result
}

/// O conteúdo lido de um arquivo de histórico e o recurso de onde veio.
struct FileContents {
    resource: URI,
    content: String,
}

/// `fetchFileContents(folderPrefix, filePath, isFileWindows, fileService, remoteAgentService)`: `None` sem pasta de
/// base ou com o arquivo ausente; qualquer outro erro de leitura é propagado.
async fn fetch_file_contents(
    folder_prefix: Option<String>,
    file_path: &str,
    file_service: &dyn IFileService,
    remote_agent_service: &dyn IRemoteAgentService,
) -> Result<Option<FileContents>, Error> {
    let Some(folder_prefix) = folder_prefix.filter(|folder_prefix| !folder_prefix.is_empty()) else { return Ok(None) };
    let remote_authority = remote_agent_service.get_connection().map(|connection| connection.remote_authority()).filter(|authority| !authority.is_empty());
    let is_remote = remote_authority.is_some();
    let joined = PathModule::native(&ProcessEnvironment).join(&[folder_prefix.as_str(), file_path]);
    let resource = URI::from(
        &UriComponents {
            scheme: Some(JsString::from(if is_remote { schemas::VSCODE_REMOTE } else { schemas::FILE })),
            authority: remote_authority.map(|authority| JsString::from(authority.as_str())),
            path: Some(URI::file(JsString::from(joined.as_str()), host_platform()).path().clone()),
            ..UriComponents::default()
        },
        false,
    );
    match file_service.read_file(&resource, None, None).await {
        Ok(content) => Ok(Some(FileContents { resource, content: content.value.to_string() })),
        // Handle file not found only
        Err(error) if FileOperationError::from_error(&error).is_some_and(|error| error.file_operation_result == FileOperationResult::FileNotFound) => Ok(None),
        Err(error) => Err(error),
    }
}
