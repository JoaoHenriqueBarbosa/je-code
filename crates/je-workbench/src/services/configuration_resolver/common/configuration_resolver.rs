//! Porte de `src/vs/workbench/services/configurationResolver/common/configurationResolver.ts`: o contrato
//! `IConfigurationResolverService`, os tipos de entrada configurada (`ConfiguredInput`), os tipos de variável
//! (`VariableKind`) e o erro de variável (`VariableError`).
//!
//! Notas de tradução:
//! - **Configuração como valor dinâmico.** `config: T` e `config: unknown` são [`Dynamic`] (o JSON que vem dos arquivos de
//!   configuração); uma string é um `Dynamic::String`. O `T extends ConfigurationResolverExpression<infer R> ? R : T` do
//!   `resolveAsync` não existe no contrato: quem já tem uma expressão analisada chama
//!   `AbstractVariableResolverService::resolve_expression_async`.
//! - **`resolveWithInteractionReplace`** devolve `Dynamic::Undefined` quando uma entrada é cancelada (o `undefined` do
//!   original).
//! - **`resolveWithInteraction`** devolve a lista `(chave, valor)` do `Map<string, string>` do original, na ordem de inserção. O
//!   valor é `None` quando a variável foi resolvida para `undefined` (o `value.value!` do original aceita isso em tempo de
//!   execução).
//! - **`resolvableVariables`** é um `ReadonlySet<string>` vivo; aqui a cópia do conjunto, na ordem de inserção.
//! - **`IStringDictionary<string>`** (`variables`) é `HashMap<String, String>`, e `IProcessEnvironment` é o
//!   [`ProcessEnvironment`] do `je-base`.
//! - **`VariableError`** é um `ErrorNoTelemetry` com a propriedade `variable`: [`VariableError::new`] monta o [`Error`] e
//!   [`VariableError::variable_of`] lê a propriedade.
//! - **`ConfiguredInput`** é a união de três interfaces (`promptString`, `pickString`, `command`) que o original só confere
//!   em tempo de execução, porque o objeto vem do `launch.json` ou do `tasks.json` do usuário: [`ConfiguredInput`] guarda o
//!   objeto dinâmico com acessores para os campos, e o resolvedor valida os atributos ao usá-lo.
//! - **Instanciação atrasada.** O `registerSingleton(..., InstantiationType.Delayed)` do serviço pede um proxy no
//!   identificador; é o [`ConfigurationResolverServiceProxy`].

use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_instantiation::{delayed_proxy, service_id};
use je_platform::configuration::ConfigurationTarget;
use je_platform::workspace::WorkspaceFolder;

/// A resolução de uma variável contribuída: `() => Promise<string | undefined>`.
pub type ContributedVariable = Rc<dyn Fn() -> Promise<Option<String>>>;

/// O `Map<string, string>` de `resolveWithInteraction`: o texto de cada variável (`env:HOME`, `input:nome`) e o valor
/// resolvido, na ordem de inserção.
pub type ResolvedVariables = Vec<(String, Option<String>)>;

/// `IConfigurationResolverService`.
pub trait IConfigurationResolverService {
    /// Variables the resolver is able to resolve.
    fn resolvable_variables(&self) -> Vec<String>;

    fn resolve_with_environment(&self, environment: &ProcessEnvironment, folder: Option<&WorkspaceFolder>, value: &str) -> Promise<String>;

    /// Recursively resolves all variables in the given config and returns a copy of it with substituted values.
    /// Command variables are only substituted if a "commandValueMapping" dictionary is given and if it contains an entry
    /// for the command.
    fn resolve_async(&self, folder: Option<&WorkspaceFolder>, config: &Dynamic) -> Promise<Dynamic>;

    /// Recursively resolves all variables (including commands and user input) in the given config and returns a copy of
    /// it with substituted values. If a "variables" dictionary (with names -> command ids) is given, command variables
    /// are first mapped through it before being resolved.
    ///
    /// `section`: for example, 'tasks' or 'debug'. Used for resolving inputs. `variables`: aliases for commands.
    fn resolve_with_interaction_replace(
        &self,
        folder: Option<&WorkspaceFolder>,
        config: &Dynamic,
        section: Option<&str>,
        variables: Option<&HashMap<String, String>>,
        target: Option<ConfigurationTarget>,
    ) -> Promise<Dynamic>;

    /// Similar to resolveWithInteractionReplace, except without the replace. Returns a map of variables and their
    /// resolution. Keys in the map will be of the format input:variableName or command:variableName.
    fn resolve_with_interaction(
        &self,
        folder: Option<&WorkspaceFolder>,
        config: &Dynamic,
        section: Option<&str>,
        variables: Option<&HashMap<String, String>>,
        target: Option<ConfigurationTarget>,
    ) -> Promise<Option<ResolvedVariables>>;

    /// Contributes a variable that can be resolved later. Consumers that use resolveAny, resolveWithInteraction, and
    /// resolveWithInteractionReplace will have contributed variables resolved.
    fn contribute_variable(&self, variable: &str, resolution: ContributedVariable);
}

delayed_proxy!(pub ConfigurationResolverServiceProxy for dyn IConfigurationResolverService {
    methods {
        fn resolvable_variables(&self) -> Vec<String>;
        fn resolve_with_environment(&self, environment: &ProcessEnvironment, folder: Option<&WorkspaceFolder>, value: &str) -> Promise<String>;
        fn resolve_async(&self, folder: Option<&WorkspaceFolder>, config: &Dynamic) -> Promise<Dynamic>;
        fn resolve_with_interaction_replace(&self, folder: Option<&WorkspaceFolder>, config: &Dynamic, section: Option<&str>, variables: Option<&HashMap<String, String>>, target: Option<ConfigurationTarget>) -> Promise<Dynamic>;
        fn resolve_with_interaction(&self, folder: Option<&WorkspaceFolder>, config: &Dynamic, section: Option<&str>, variables: Option<&HashMap<String, String>>, target: Option<ConfigurationTarget>) -> Promise<Option<ResolvedVariables>>;
        fn contribute_variable(&self, variable: &str, resolution: ContributedVariable);
    }
    events {
    }
});

service_id!(pub I_CONFIGURATION_RESOLVER_SERVICE: dyn IConfigurationResolverService = "configurationResolverService", proxy ConfigurationResolverServiceProxy);

/// `ConfiguredInput`: `PromptStringInputInfo | PickStringInputInfo | CommandInputInfo`, o objeto da seção `inputs`.
///
/// - `promptString`: `{ id, type, description, default?, password? }`.
/// - `pickString`: `{ id, type, description, options: (string | { value, label? })[], default? }`.
/// - `command`: `{ id, type, command, args? }`.
#[derive(Clone, Debug)]
pub struct ConfiguredInput {
    raw: Dynamic,
}

impl ConfiguredInput {
    pub fn new(raw: Dynamic) -> Self {
        Self { raw }
    }

    /// O objeto como veio da configuração.
    pub fn raw(&self) -> &Dynamic {
        &self.raw
    }

    /// `input.<name>`: `undefined` quando o campo falta.
    pub fn field(&self, name: &str) -> Dynamic {
        self.raw.get_str(name)
    }

    /// `input.id`.
    pub fn id(&self) -> Dynamic {
        self.field("id")
    }

    /// `input.type`.
    pub fn type_(&self) -> Dynamic {
        self.field("type")
    }
}

/// `VariableKind`: o nome de cada tipo de variável (`${env:X}` é [`VariableKind::Env`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VariableKind {
    Unknown,

    Env,
    Config,
    Command,
    Input,
    ExtensionInstallFolder,
    TaskVar,

    WorkspaceFolder,
    Cwd,
    WorkspaceFolderBasename,
    UserHome,
    LineNumber,
    ColumnNumber,
    SelectedText,
    File,
    FileWorkspaceFolder,
    FileWorkspaceFolderBasename,
    RelativeFile,
    RelativeFileDirname,
    FileDirname,
    FileExtname,
    FileBasename,
    FileBasenameNoExtension,
    FileDirnameBasename,
    ExecPath,
    ExecInstallFolder,
    PathSeparator,
    PathSeparatorAlias,
}

impl VariableKind {
    /// Os valores do enum, na ordem de declaração do original.
    pub const ALL: [VariableKind; 28] = [
        VariableKind::Unknown,
        VariableKind::Env,
        VariableKind::Config,
        VariableKind::Command,
        VariableKind::Input,
        VariableKind::ExtensionInstallFolder,
        VariableKind::TaskVar,
        VariableKind::WorkspaceFolder,
        VariableKind::Cwd,
        VariableKind::WorkspaceFolderBasename,
        VariableKind::UserHome,
        VariableKind::LineNumber,
        VariableKind::ColumnNumber,
        VariableKind::SelectedText,
        VariableKind::File,
        VariableKind::FileWorkspaceFolder,
        VariableKind::FileWorkspaceFolderBasename,
        VariableKind::RelativeFile,
        VariableKind::RelativeFileDirname,
        VariableKind::FileDirname,
        VariableKind::FileExtname,
        VariableKind::FileBasename,
        VariableKind::FileBasenameNoExtension,
        VariableKind::FileDirnameBasename,
        VariableKind::ExecPath,
        VariableKind::ExecInstallFolder,
        VariableKind::PathSeparator,
        VariableKind::PathSeparatorAlias,
    ];

    /// O valor de texto do enum (`'workspaceFolder'`, `'/'`...).
    pub fn as_str(self) -> &'static str {
        match self {
            VariableKind::Unknown => "unknown",
            VariableKind::Env => "env",
            VariableKind::Config => "config",
            VariableKind::Command => "command",
            VariableKind::Input => "input",
            VariableKind::ExtensionInstallFolder => "extensionInstallFolder",
            VariableKind::TaskVar => "taskVar",
            VariableKind::WorkspaceFolder => "workspaceFolder",
            VariableKind::Cwd => "cwd",
            VariableKind::WorkspaceFolderBasename => "workspaceFolderBasename",
            VariableKind::UserHome => "userHome",
            VariableKind::LineNumber => "lineNumber",
            VariableKind::ColumnNumber => "columnNumber",
            VariableKind::SelectedText => "selectedText",
            VariableKind::File => "file",
            VariableKind::FileWorkspaceFolder => "fileWorkspaceFolder",
            VariableKind::FileWorkspaceFolderBasename => "fileWorkspaceFolderBasename",
            VariableKind::RelativeFile => "relativeFile",
            VariableKind::RelativeFileDirname => "relativeFileDirname",
            VariableKind::FileDirname => "fileDirname",
            VariableKind::FileExtname => "fileExtname",
            VariableKind::FileBasename => "fileBasename",
            VariableKind::FileBasenameNoExtension => "fileBasenameNoExtension",
            VariableKind::FileDirnameBasename => "fileDirnameBasename",
            VariableKind::ExecPath => "execPath",
            VariableKind::ExecInstallFolder => "execInstallFolder",
            VariableKind::PathSeparator => "pathSeparator",
            VariableKind::PathSeparatorAlias => "/",
        }
    }

    /// O enum que tem o texto dado, se tem.
    pub fn from_text(text: &str) -> Option<VariableKind> {
        VariableKind::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// `allVariableKinds`: os valores de texto de todos os tipos de variável.
pub fn all_variable_kinds() -> Vec<&'static str> {
    VariableKind::ALL.iter().map(|kind| kind.as_str()).collect()
}

/// `VariableError`: um `ErrorNoTelemetry` que sabe de que tipo de variável veio.
pub struct VariableError;

impl VariableError {
    /// `new VariableError(variable, message)`.
    pub fn new(variable: VariableKind, message: impl Into<String>) -> Error {
        Error::no_telemetry(message).with_prop("variable", variable.as_str())
    }

    /// `error.variable`, quando o erro é um `VariableError`.
    pub fn variable_of(error: &Error) -> Option<VariableKind> {
        error.prop("variable").and_then(VariableKind::from_text)
    }
}
