//! Porte de `src/vs/workbench/services/configurationResolver/common/variableResolver.ts`: o `AbstractVariableResolverService`,
//! que resolve cada variável (`${workspaceFolder}`, `${file}`, `${env:X}`, `${config:X}`...) a partir de um contexto
//! (pastas do workspace, arquivo ativo, seleção, extensões, configuração).
//!
//! Notas de tradução:
//! - **Herança.** A classe abstrata é a struct [`AbstractVariableResolverService`], um identificador barato (`Rc` por
//!   dentro, `Clone`) que o `BaseConfigurationResolverService` guarda em `base`: as funções `async` precisam de um dono
//!   `'static`, e o identificador é esse dono. O que a subclasse sobrescreve (`resolveWithInteraction*`) aqui lança
//!   `not implemented`, como no original; `evaluateSingleVariable`, que o original deixa `protected`, é público para a
//!   subclasse e devolve o [`Dynamic`] (o `config` pode ser número, booleano ou array, não só texto).
//! - **`IVariableResolveContext`** é o trait [`IVariableResolveContext`], com as mesmas funções. O
//!   `getWorkspaceFolderPathForFile?` opcional é um método com o padrão `None`, que leva ao mesmo erro do original (função
//!   ausente e função que devolve `undefined` terminam igual).
//! - **Argumento vazio.** `${env:}` tem `arg` igual a `''`, que o original trata como ausente em todo `if (argument)`; o
//!   argumento aqui já nasce `None` quando vazio.
//! - **Plataforma e caminhos.** `isWindows` é o da plataforma detectada, e `paths.*` é o módulo `path` nativo do
//!   hospedeiro. `process.cwd()` é o `cwd` do ambiente do hospedeiro.
//! - **`commandValueMapping`** é tipado `IStringDictionary<IResolvedValue>` no original, mas o código compara cada valor com
//!   `typeof v === 'string'`: é um dicionário de [`Dynamic`], que é o que existe em tempo de execução.
//! - **Erros.** Os `throw` viram `Err` das funções `async` ([`VariableError`], que é um erro sem telemetria com o tipo da
//!   variável). Só o `columnNumber` ausente lança um `Error` comum, como no original.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::labels::normalize_drive_letter;
use je_base::nls::localize;
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::types::{is_object, is_undefined_or_null};
use je_base::uri::URI;
use je_platform::configuration::ConfigurationTarget;
use je_platform::js::to_js_string;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::resources::{fs_path as uri_fs_path, host_environment};
use je_platform::workspace::WorkspaceFolder;

use super::configuration_resolver::{
    ContributedVariable, IConfigurationResolverService, ResolvedVariables, VariableError, VariableKind, all_variable_kinds,
};
use super::configuration_resolver_expression::{ConfigurationResolverExpression, Replacement};
use crate::services::extensions::extensions::IExtensionDescription;

/// `IVariableResolveContext`.
pub trait IVariableResolveContext {
    fn get_folder_uri(&self, folder_name: &str) -> Option<URI>;
    fn get_workspace_folder_count(&self) -> usize;
    /// `getConfigurationValue(folderUri, section)`: o valor da configuração (`undefined` quando não existe).
    fn get_configuration_value(&self, folder_uri: Option<&URI>, section: &str) -> Dynamic;
    fn get_app_root(&self) -> Option<String>;
    fn get_exec_path(&self) -> Option<String>;
    fn get_file_path(&self) -> Option<String>;
    /// `getWorkspaceFolderPathForFile?()`: sem implementação, a variável que a usa falha como se o arquivo estivesse fora de
    /// qualquer pasta.
    fn get_workspace_folder_path_for_file(&self) -> Option<String> {
        None
    }
    fn get_selected_text(&self) -> Option<String>;
    fn get_line_number(&self) -> Option<String>;
    fn get_column_number(&self) -> Option<String>;
    fn get_extension(&self, id: &str) -> Promise<Option<IExtensionDescription>>;
}

/// `isWindows`.
fn is_windows() -> bool {
    Platform::detect().is_windows
}

fn prepare_env(env_variables: ProcessEnvironment) -> ProcessEnvironment {
    // windows env variables are case insensitive
    if is_windows() {
        return env_variables.into_iter().map(|(key, value)| (key.to_lowercase(), value)).collect();
    }
    env_variables
}

struct AbstractVariableResolverInner {
    context: Rc<dyn IVariableResolveContext>,
    label_service: Option<Rc<dyn ILabelService>>,
    env_variables_promise: Option<Promise<ProcessEnvironment>>,
    user_home_promise: Option<Promise<String>>,
    contributed_variables: RefCell<HashMap<String, ContributedVariable>>,
    resolvable_variables: RefCell<Vec<String>>,
}

impl AbstractVariableResolverInner {
    fn fs_path(&self, display_uri: &URI) -> String {
        match &self.label_service {
            Some(label_service) => label_service.get_uri_label(display_uri, &UriLabelOptions { no_prefix: Some(true), ..UriLabelOptions::default() }),
            None => uri_fs_path(display_uri),
        }
    }
}

/// `AbstractVariableResolverService`.
#[derive(Clone)]
pub struct AbstractVariableResolverService {
    inner: Rc<AbstractVariableResolverInner>,
}

impl AbstractVariableResolverService {
    /// `constructor(_context, _labelService?, _userHomePromise?, _envVariablesPromise?)`.
    pub fn new(
        context: Rc<dyn IVariableResolveContext>,
        label_service: Option<Rc<dyn ILabelService>>,
        user_home_promise: Option<Promise<String>>,
        env_variables_promise: Option<Promise<ProcessEnvironment>>,
    ) -> Self {
        Self {
            inner: Rc::new(AbstractVariableResolverInner {
                context,
                label_service,
                env_variables_promise: env_variables_promise.map(|promise| promise.then(prepare_env)),
                user_home_promise,
                contributed_variables: RefCell::new(HashMap::new()),
                resolvable_variables: RefCell::new(all_variable_kinds().into_iter().map(String::from).collect()),
            }),
        }
    }

    /// `resolveAsync` de uma expressão já analisada (`config` é um `ConfigurationResolverExpression`).
    pub fn resolve_expression_async(&self, folder: Option<&WorkspaceFolder>, expr: Rc<ConfigurationResolverExpression>) -> Promise<Dynamic> {
        resolve_unresolved(self.inner.clone(), folder.map(|folder| folder.uri.clone()), expr, None)
    }

    /// A resolução contribuída para a variável (`_contributedVariables.get(inner)`), quando existe.
    pub fn contributed_variable(&self, inner: &str) -> Option<ContributedVariable> {
        self.inner.contributed_variables.borrow().get(inner).cloned()
    }

    /// `evaluateSingleVariable(replacement, folderUri, processEnvironment?, commandValueMapping?)`: o valor da variável, ou
    /// `undefined` quando o resolvedor não sabe resolvê-la.
    pub fn evaluate_single_variable(
        &self,
        replacement: &Replacement,
        folder_uri: Option<&URI>,
        process_environment: Option<&ProcessEnvironment>,
        command_value_mapping: Option<&HashMap<String, Dynamic>>,
    ) -> Promise<Dynamic> {
        async_fn(evaluate(self.inner.clone(), replacement.clone(), folder_uri.cloned(), process_environment.cloned(), command_value_mapping.cloned()))
    }
}

impl IConfigurationResolverService for AbstractVariableResolverService {
    fn resolvable_variables(&self) -> Vec<String> {
        self.inner.resolvable_variables.borrow().clone()
    }

    fn resolve_with_environment(&self, environment: &ProcessEnvironment, folder: Option<&WorkspaceFolder>, value: &str) -> Promise<String> {
        let expr = ConfigurationResolverExpression::parse(&Dynamic::string(value));
        resolve_unresolved(self.inner.clone(), folder.map(|folder| folder.uri.clone()), expr, Some(environment.clone()))
            .then(|resolved| to_js_string(&resolved).to_string_lossy())
    }

    fn resolve_async(&self, folder: Option<&WorkspaceFolder>, config: &Dynamic) -> Promise<Dynamic> {
        self.resolve_expression_async(folder, ConfigurationResolverExpression::parse(config))
    }

    fn resolve_with_interaction_replace(
        &self,
        _folder: Option<&WorkspaceFolder>,
        _config: &Dynamic,
        _section: Option<&str>,
        _variables: Option<&HashMap<String, String>>,
        _target: Option<ConfigurationTarget>,
    ) -> Promise<Dynamic> {
        errors::throw(Error::new("resolveWithInteractionReplace not implemented."))
    }

    fn resolve_with_interaction(
        &self,
        _folder: Option<&WorkspaceFolder>,
        _config: &Dynamic,
        _section: Option<&str>,
        _variables: Option<&HashMap<String, String>>,
        _target: Option<ConfigurationTarget>,
    ) -> Promise<Option<ResolvedVariables>> {
        errors::throw(Error::new("resolveWithInteraction not implemented."))
    }

    fn contribute_variable(&self, variable: &str, resolution: ContributedVariable) {
        let mut contributed_variables = self.inner.contributed_variables.borrow_mut();
        if contributed_variables.contains_key(variable) {
            errors::throw(Error::new(format!("Variable {variable} is contributed twice.")));
        }
        let mut resolvable_variables = self.inner.resolvable_variables.borrow_mut();
        if !resolvable_variables.iter().any(|known| known == variable) {
            resolvable_variables.push(variable.to_string());
        }
        contributed_variables.insert(variable.to_string(), resolution);
    }
}

/// O laço comum de `resolveWithEnvironment` e `resolveAsync`: resolve cada variável pendente da expressão (e as que a
/// resolução fizer aparecer) e devolve o objeto.
fn resolve_unresolved(
    this: Rc<AbstractVariableResolverInner>,
    folder_uri: Option<URI>,
    expr: Rc<ConfigurationResolverExpression>,
    environment: Option<ProcessEnvironment>,
) -> Promise<Dynamic> {
    async_fn(async move {
        for replacement in expr.unresolved() {
            let resolved_value = evaluate(this.clone(), replacement.clone(), folder_uri.clone(), environment.clone(), None).await?;
            if !resolved_value.is_undefined() {
                expr.resolve(&replacement, to_js_string(&resolved_value));
            }
        }

        Ok(expr.to_object())
    })
}

/// `evaluateSingleVariable`.
async fn evaluate(
    this: Rc<AbstractVariableResolverInner>,
    replacement: Replacement,
    folder_uri: Option<URI>,
    process_environment: Option<ProcessEnvironment>,
    command_value_mapping: Option<HashMap<String, Dynamic>>,
) -> Result<Dynamic, Error> {
    // `Environment`: o ambiente do processo e o `userHome`, que só valem quando o chamador não deu um ambiente.
    let (env, user_home): (Option<ProcessEnvironment>, Option<String>) = match process_environment {
        Some(process_environment) => (Some(prepare_env(process_environment)), None),
        None => {
            let env = match &this.env_variables_promise {
                Some(promise) => Some(promise.clone().await?),
                None => None,
            };
            let user_home = match &this.user_home_promise {
                Some(promise) => Some(promise.clone().await?),
                None => None,
            };
            (env, user_home)
        }
    };

    let id = replacement.id.to_string_lossy();
    let variable = replacement.name.to_string_lossy();
    let argument: Option<String> = replacement.arg.as_ref().filter(|arg| !arg.is_empty()).map(JsString::to_string_lossy);

    let context = this.context.clone();
    let host = host_environment();
    let paths = PathModule::native(host.as_ref());
    let windows = is_windows();

    // common error handling for all variables that require an open editor
    let get_file_path = |variable_kind: VariableKind| -> Result<String, Error> {
        match context.get_file_path().filter(|file_path| !file_path.is_empty()) {
            Some(file_path) => Ok(normalize_drive_letter(&file_path, windows)),
            None => Err(VariableError::new(
                variable_kind,
                localize("canNotResolveFile", "Variable {0} can not be resolved. Please open an editor.", &[id.as_str()]),
            )),
        }
    };

    // common error handling for all variables that require an open editor
    let get_folder_path_for_file = |variable_kind: VariableKind| -> Result<String, Error> {
        let file_path = get_file_path(variable_kind)?; // throws error if no editor open
        if let Some(folder_path) = context.get_workspace_folder_path_for_file().filter(|folder_path| !folder_path.is_empty()) {
            return Ok(normalize_drive_letter(&folder_path, windows));
        }
        Err(VariableError::new(
            variable_kind,
            localize("canNotResolveFolderForFile", "Variable {0}: can not find workspace folder of '{1}'.", &[id.as_str(), paths.basename(&file_path, None).as_str()]),
        ))
    };

    // common error handling for all variables that require an open folder and accept a folder name argument
    let get_folder_uri = |variable_kind: VariableKind| -> Result<URI, Error> {
        if let Some(argument) = &argument {
            return match context.get_folder_uri(argument) {
                Some(folder) => Ok(folder),
                None => Err(VariableError::new(
                    variable_kind,
                    localize("canNotFindFolder", "Variable {0} can not be resolved. No such folder '{1}'.", &[variable_kind.as_str(), argument.as_str()]),
                )),
            };
        }

        if let Some(folder_uri) = &folder_uri {
            return Ok(folder_uri.clone());
        }

        if context.get_workspace_folder_count() > 1 {
            return Err(VariableError::new(
                variable_kind,
                localize(
                    "canNotResolveWorkspaceFolderMultiRoot",
                    "Variable {0} can not be resolved in a multi folder workspace. Scope this variable using ':' and a workspace folder name.",
                    &[variable_kind.as_str()],
                ),
            ));
        }
        Err(VariableError::new(
            variable_kind,
            localize("canNotResolveWorkspaceFolder", "Variable {0} can not be resolved. Please open a folder.", &[variable_kind.as_str()]),
        ))
    };

    match variable.as_str() {
        "env" => {
            if let Some(argument) = &argument {
                if let Some(env) = &env {
                    let key = if windows { argument.to_lowercase() } else { argument.clone() };
                    if let Some(value) = env.get(&key) {
                        return Ok(Dynamic::string(value));
                    }
                }
                return Ok(Dynamic::string(""));
            }
            Err(VariableError::new(
                VariableKind::Env,
                localize("missingEnvVarName", "Variable {0} can not be resolved because no environment variable name is given.", &[id.as_str()]),
            ))
        }

        "config" => {
            if let Some(argument) = &argument {
                let config = context.get_configuration_value(folder_uri.as_ref(), argument);
                if is_undefined_or_null(&config) {
                    return Err(VariableError::new(
                        VariableKind::Config,
                        localize("configNotFound", "Variable {0} can not be resolved because setting '{1}' not found.", &[id.as_str(), argument.as_str()]),
                    ));
                }
                if is_object(&config) {
                    return Err(VariableError::new(
                        VariableKind::Config,
                        localize("configNoString", "Variable {0} can not be resolved because '{1}' is a structured value.", &[id.as_str(), argument.as_str()]),
                    ));
                }
                return Ok(config);
            }
            Err(VariableError::new(
                VariableKind::Config,
                localize("missingConfigName", "Variable {0} can not be resolved because no settings name is given.", &[id.as_str()]),
            ))
        }

        "command" => resolve_from_map(VariableKind::Command, &id, argument.as_deref(), command_value_mapping.as_ref(), Some("command")).map(|value| Dynamic::string(&value)),

        "input" => resolve_from_map(VariableKind::Input, &id, argument.as_deref(), command_value_mapping.as_ref(), Some("input")).map(|value| Dynamic::string(&value)),

        "extensionInstallFolder" => {
            if let Some(argument) = &argument {
                let Some(ext) = context.get_extension(argument).await? else {
                    return Err(VariableError::new(
                        VariableKind::ExtensionInstallFolder,
                        localize(
                            "extensionNotInstalled",
                            "Variable {0} can not be resolved because the extension {1} is not installed.",
                            &[id.as_str(), argument.as_str()],
                        ),
                    ));
                };
                return Ok(Dynamic::string(&this.fs_path(&ext.extension_location)));
            }
            Err(VariableError::new(
                VariableKind::ExtensionInstallFolder,
                localize("missingExtensionName", "Variable {0} can not be resolved because no extension name is given.", &[id.as_str()]),
            ))
        }

        "workspaceRoot" | "workspaceFolder" => {
            let uri = get_folder_uri(VariableKind::WorkspaceFolder)?;
            Ok(Dynamic::string(&normalize_drive_letter(&this.fs_path(&uri), windows)))
        }

        "cwd" => {
            if folder_uri.is_none() && argument.is_none() {
                return Ok(Dynamic::string(&host.cwd()));
            }
            let uri = get_folder_uri(VariableKind::Cwd)?;
            Ok(Dynamic::string(&normalize_drive_letter(&this.fs_path(&uri), windows)))
        }

        "workspaceRootFolderName" | "workspaceFolderBasename" => {
            let uri = get_folder_uri(VariableKind::WorkspaceFolderBasename)?;
            Ok(Dynamic::string(&normalize_drive_letter(&paths.basename(&this.fs_path(&uri), None), windows)))
        }

        "userHome" => match user_home.filter(|user_home| !user_home.is_empty()) {
            Some(user_home) => Ok(Dynamic::string(&user_home)),
            None => Err(VariableError::new(
                VariableKind::UserHome,
                localize("canNotResolveUserHome", "Variable {0} can not be resolved. UserHome path is not defined", &[id.as_str()]),
            )),
        },

        "lineNumber" => match context.get_line_number().filter(|line_number| !line_number.is_empty()) {
            Some(line_number) => Ok(Dynamic::string(&line_number)),
            None => Err(VariableError::new(
                VariableKind::LineNumber,
                localize("canNotResolveLineNumber", "Variable {0} can not be resolved. Make sure to have a line selected in the active editor.", &[id.as_str()]),
            )),
        },

        "columnNumber" => match context.get_column_number().filter(|column_number| !column_number.is_empty()) {
            Some(column_number) => Ok(Dynamic::string(&column_number)),
            None => Err(Error::new(localize(
                "canNotResolveColumnNumber",
                "Variable {0} can not be resolved. Make sure to have a column selected in the active editor.",
                &[id.as_str()],
            ))),
        },

        "selectedText" => match context.get_selected_text().filter(|selected_text| !selected_text.is_empty()) {
            Some(selected_text) => Ok(Dynamic::string(&selected_text)),
            None => Err(VariableError::new(
                VariableKind::SelectedText,
                localize("canNotResolveSelectedText", "Variable {0} can not be resolved. Make sure to have some text selected in the active editor.", &[id.as_str()]),
            )),
        },

        "file" => get_file_path(VariableKind::File).map(|file_path| Dynamic::string(&file_path)),

        "fileWorkspaceFolder" => get_folder_path_for_file(VariableKind::FileWorkspaceFolder).map(|folder_path| Dynamic::string(&folder_path)),

        "fileWorkspaceFolderBasename" => {
            let folder_path = get_folder_path_for_file(VariableKind::FileWorkspaceFolderBasename)?;
            Ok(Dynamic::string(&paths.basename(&folder_path, None)))
        }

        "relativeFile" => {
            if folder_uri.is_some() || argument.is_some() {
                let folder = get_folder_uri(VariableKind::RelativeFile)?;
                let file_path = get_file_path(VariableKind::RelativeFile)?;
                return Ok(Dynamic::string(&paths.relative(&this.fs_path(&folder), &file_path)));
            }
            get_file_path(VariableKind::RelativeFile).map(|file_path| Dynamic::string(&file_path))
        }

        "relativeFileDirname" => {
            let dirname = paths.dirname(&get_file_path(VariableKind::RelativeFileDirname)?);
            if folder_uri.is_some() || argument.is_some() {
                let folder = get_folder_uri(VariableKind::RelativeFileDirname)?;
                let relative = paths.relative(&this.fs_path(&folder), &dirname);
                return Ok(Dynamic::string(if relative.is_empty() { "." } else { relative.as_str() }));
            }
            Ok(Dynamic::string(&dirname))
        }

        "fileDirname" => Ok(Dynamic::string(&paths.dirname(&get_file_path(VariableKind::FileDirname)?))),

        "fileExtname" => Ok(Dynamic::string(&paths.extname(&get_file_path(VariableKind::FileExtname)?))),

        "fileBasename" => Ok(Dynamic::string(&paths.basename(&get_file_path(VariableKind::FileBasename)?, None))),

        "fileBasenameNoExtension" => {
            let basename = paths.basename(&get_file_path(VariableKind::FileBasenameNoExtension)?, None);
            let extname = paths.extname(&basename);
            Ok(Dynamic::string(basename.strip_suffix(extname.as_str()).unwrap_or(&basename)))
        }

        "fileDirnameBasename" => Ok(Dynamic::string(&paths.basename(&paths.dirname(&get_file_path(VariableKind::FileDirnameBasename)?), None))),

        "execPath" => match context.get_exec_path().filter(|exec_path| !exec_path.is_empty()) {
            Some(exec_path) => Ok(Dynamic::string(&exec_path)),
            None => Ok(Dynamic::string(&id)),
        },

        "execInstallFolder" => match context.get_app_root().filter(|app_root| !app_root.is_empty()) {
            Some(app_root) => Ok(Dynamic::string(&app_root)),
            None => Ok(Dynamic::string(&id)),
        },

        "pathSeparator" | "/" => Ok(Dynamic::string(paths.sep())),

        _ => Ok(Dynamic::string(
            &resolve_from_map(VariableKind::Unknown, &id, argument.as_deref(), command_value_mapping.as_ref(), None).unwrap_or_else(|_| id.clone()),
        )),
    }
}

fn resolve_from_map(
    variable_kind: VariableKind,
    match_: &str,
    argument: Option<&str>,
    command_value_mapping: Option<&HashMap<String, Dynamic>>,
    prefix: Option<&str>,
) -> Result<String, Error> {
    if let (Some(argument), Some(command_value_mapping)) = (argument, command_value_mapping) {
        let key = match prefix {
            None => argument.to_string(),
            Some(prefix) => format!("{prefix}:{argument}"),
        };
        if let Some(Dynamic::String(value)) = command_value_mapping.get(&key) {
            return Ok(value.to_string_lossy());
        }
        return Err(VariableError::new(
            variable_kind,
            localize("noValueForCommand", "Variable {0} can not be resolved because the command has no value.", &[match_]),
        ));
    }
    Ok(match_.to_string())
}
