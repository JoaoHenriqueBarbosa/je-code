//! Porte de `src/vs/workbench/services/configurationResolver/browser/baseConfigurationResolverService.ts`: o serviço que
//! liga o resolvedor abstrato ao workbench (editor ativo, pastas do workspace, configuração, extensões) e acrescenta a
//! resolução interativa: `${command:...}`, `${input:...}` e as variáveis contribuídas.
//!
//! Notas de tradução:
//! - **Herança.** `BaseConfigurationResolverService extends AbstractVariableResolverService` é a composição: o campo `base`
//!   guarda o [`AbstractVariableResolverService`], e o que o original herda sem mudar (`resolveAsync`,
//!   `resolveWithEnvironment`, `contributeVariable`, `resolvableVariables`) o trait [`IConfigurationResolverService`] repassa
//!   a ele. `this.resolvableVariables.add('command')` e `.add('input')` do construtor não fazem nada, porque os dois já
//!   estão no conjunto inicial (`allVariableKinds`).
//! - **Contexto.** Os closures que o construtor passa ao `super` são o [`BaseResolveContext`], com os mesmos serviços.
//!   `{ getAppRoot, getExecPath }` é o [`ResolverAppContext`]. `isCodeEditor`/`isDiffEditor` são as variantes de
//!   [`ActiveTextEditorControl`].
//! - **Funções `async` com dono.** As rotinas que esperam promessas são funções livres que recebem o `Rc<BaseInner>` (o
//!   `this` do original), porque o futuro precisa ser `'static`.
//! - **`userInputAccessQueue`** é um `Queue<string | IQuickPickItem | undefined>`: [`UserInputResult`] é a união (texto,
//!   escolha ou o retorno de um comando), e as três entradas passam pela mesma fila, uma por vez.
//! - **`variable.arg!`** ausente (`${command}` sem argumento) é `undefined` no original, que vira a chave `"undefined"`;
//!   aqui é o texto `"undefined"`.
//! - **`IResolvedValue.value`** nulo (um comando de entrada que devolve `null`) é o texto `"null"`, que é o que o
//!   `replaceAll` do original escreve.
//! - **`LRUCache.fromJSON`** é o laço sobre `set`: o `fromJSON` do original chama o `set` da subclasse, que aplica o limite.
//!   Um par com chave ou valor que não é texto (armazenamento adulterado) é ignorado.
//! - **`INPUT_OR_COMMAND_VARIABLES_PATTERN`** é o texto da expressão regular (`/\${((input|command):(.*?))}/g`).

use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{Queue, async_fn};
use je_base::dynamic::{Dynamic, Truthy, json_stringify};
use je_base::errors::{self, Error};
use je_base::json::Value;
use je_base::jsonc::json_parse;
use je_base::map::LRUCache;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::types::is_undefined_or_null;
use je_base::uri::URI;
use je_editor::selection::Selection;
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget};
use je_platform::js::to_js_string;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::path::path_service::IPathService;
use je_platform::quickinput::quick_input::{IInputOptions, IPickOptions, IQuickInputService, IQuickItem, IQuickPickItem};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::workspace::{IWorkspaceContextService, WorkspaceFolder};
use je_storage::StorageValue;

use crate::common::editor::{EditorResourceAccessor, EditorResourceAccessorOptions, SchemeFilter, SideBySideEditor};
use crate::services::configuration_resolver::common::configuration_resolver::{
    ConfiguredInput, ContributedVariable, IConfigurationResolverService, ResolvedVariables, VariableError, VariableKind,
};
use crate::services::configuration_resolver::common::configuration_resolver_expression::{ConfigurationResolverExpression, IResolvedValue};
use crate::services::configuration_resolver::common::variable_resolver::{AbstractVariableResolverService, IVariableResolveContext};
use crate::services::editor::editor_service::{ActiveTextEditorControl, IEditorService};
use crate::services::extensions::extensions::{IExtensionDescription, IExtensionService};

const LAST_INPUT_STORAGE_KEY: &str = "configResolveInputLru";
const LAST_INPUT_CACHE_SIZE: usize = 5;

/// `BaseConfigurationResolverService.INPUT_OR_COMMAND_VARIABLES_PATTERN`: o texto da expressão regular, com a flag `g`.
pub const INPUT_OR_COMMAND_VARIABLES_PATTERN: &str = r"\$\{((input|command):(.*?))\}";

/// O `{ getAppRoot, getExecPath }` que o construtor recebe.
#[derive(Clone)]
pub struct ResolverAppContext {
    pub get_app_root: Rc<dyn Fn() -> Option<String>>,
    pub get_exec_path: Rc<dyn Fn() -> Option<String>>,
}

/// O resultado de uma entrada do usuário que passa pela fila (`string | IQuickPickItem | undefined`, mais o retorno de um
/// comando de entrada).
#[derive(Clone)]
enum UserInputResult {
    Text(Option<String>),
    Pick(Option<IQuickPickItem>),
    Command(Dynamic),
}

impl UserInputResult {
    fn into_text(self) -> Option<String> {
        match self {
            UserInputResult::Text(text) => text,
            _ => errors::throw(Error::bug_indicating(Some("a fila devolveu um resultado que não é texto"))),
        }
    }

    fn into_pick(self) -> Option<IQuickPickItem> {
        match self {
            UserInputResult::Pick(item) => item,
            _ => errors::throw(Error::bug_indicating(Some("a fila devolveu um resultado que não é uma escolha"))),
        }
    }

    fn into_command(self) -> Dynamic {
        match self {
            UserInputResult::Command(result) => result,
            _ => errors::throw(Error::bug_indicating(Some("a fila devolveu um resultado que não é de comando"))),
        }
    }
}

/// O contexto que o construtor monta para o `super`: pastas do workspace, configuração, editor ativo e extensões.
struct BaseResolveContext {
    app_context: ResolverAppContext,
    editor_service: Rc<dyn IEditorService>,
    configuration_service: Rc<dyn IConfigurationService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    label_service: Rc<dyn ILabelService>,
    path_service: Rc<dyn IPathService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl BaseResolveContext {
    /// `EditorResourceAccessor.getOriginalUri(editorService.activeEditor, { supportSideBySide: PRIMARY, filterByScheme: [...] })`.
    fn active_file_resource(&self) -> Option<URI> {
        let options = EditorResourceAccessorOptions {
            support_side_by_side: Some(SideBySideEditor::Primary),
            filter_by_scheme: Some(SchemeFilter::Multiple(vec![
                schemas::FILE.to_string(),
                schemas::VSCODE_USER_DATA.to_string(),
                self.path_service.default_uri_scheme(),
            ])),
        };
        EditorResourceAccessor::get_original_uri(self.editor_service.active_editor().as_ref(), Some(&options))
    }

    /// `getUriLabel(resource, { noPrefix: true })`.
    fn label_of(&self, resource: &URI) -> String {
        self.label_service.get_uri_label(resource, &UriLabelOptions { no_prefix: Some(true), ..UriLabelOptions::default() })
    }

    /// A seleção do editor de código ativo (`isCodeEditor(activeTextEditorControl)`, `getSelection()`).
    fn active_code_editor_selection(&self) -> Option<Selection> {
        match self.editor_service.active_text_editor_control() {
            Some(ActiveTextEditorControl::Code(editor)) => editor.get_selection(),
            _ => None,
        }
    }
}

impl IVariableResolveContext for BaseResolveContext {
    fn get_folder_uri(&self, folder_name: &str) -> Option<URI> {
        let folders = self.workspace_context_service.get_workspace().folders();
        folders.into_iter().rfind(|folder| folder.name == folder_name).map(|folder| folder.uri)
    }

    fn get_workspace_folder_count(&self) -> usize {
        self.workspace_context_service.get_workspace().folders().len()
    }

    fn get_configuration_value(&self, folder_uri: Option<&URI>, section: &str) -> Dynamic {
        let overrides = folder_uri.map_or_else(ConfigurationOverrides::default, |folder_uri| ConfigurationOverrides::with_resource(folder_uri.clone()));
        self.configuration_service.get_value(Some(section), Some(&overrides))
    }

    fn get_app_root(&self) -> Option<String> {
        (self.app_context.get_app_root)()
    }

    fn get_exec_path(&self) -> Option<String> {
        (self.app_context.get_exec_path)()
    }

    fn get_file_path(&self) -> Option<String> {
        let file_resource = self.active_file_resource()?;
        Some(self.label_of(&file_resource))
    }

    fn get_workspace_folder_path_for_file(&self) -> Option<String> {
        let file_resource = self.active_file_resource()?;
        let ws_folder = self.workspace_context_service.get_workspace_folder(&file_resource)?;
        Some(self.label_of(&ws_folder.uri))
    }

    fn get_selected_text(&self) -> Option<String> {
        let active_control = match self.editor_service.active_text_editor_control()? {
            ActiveTextEditorControl::Code(editor) => editor,
            ActiveTextEditorControl::Diff { original, modified } => {
                if original.has_widget_focus() {
                    original
                } else {
                    modified
                }
            }
        };

        let active_selection = active_control.get_selection()?;
        active_control.get_value_in_range(&active_selection).map(|text| text.to_string_lossy())
    }

    fn get_line_number(&self) -> Option<String> {
        self.active_code_editor_selection().map(|selection| selection.position_line_number.to_string())
    }

    fn get_column_number(&self) -> Option<String> {
        self.active_code_editor_selection().map(|selection| selection.position_column.to_string())
    }

    fn get_extension(&self, id: &str) -> Promise<Option<IExtensionDescription>> {
        self.extension_service.get_extension(id)
    }
}

/// O estado da subclasse que as funções `async` compartilham.
struct BaseInner {
    configuration_service: Rc<dyn IConfigurationService>,
    command_service: Rc<dyn ICommandService>,
    quick_input_service: Rc<dyn IQuickInputService>,
    storage_service: Rc<dyn StorageService>,
    user_input_access_queue: Queue<UserInputResult>,
}

/// `BaseConfigurationResolverService`.
pub struct BaseConfigurationResolverService {
    base: AbstractVariableResolverService,
    inner: Rc<BaseInner>,
}

impl BaseConfigurationResolverService {
    /// `constructor(context, envVariablesPromise, editorService, configurationService, commandService,
    /// workspaceContextService, quickInputService, labelService, pathService, extensionService, storageService)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: ResolverAppContext,
        env_variables_promise: Promise<ProcessEnvironment>,
        editor_service: Rc<dyn IEditorService>,
        configuration_service: Rc<dyn IConfigurationService>,
        command_service: Rc<dyn ICommandService>,
        workspace_context_service: Rc<dyn IWorkspaceContextService>,
        quick_input_service: Rc<dyn IQuickInputService>,
        label_service: Rc<dyn ILabelService>,
        path_service: Rc<dyn IPathService>,
        extension_service: Rc<dyn IExtensionService>,
        storage_service: Rc<dyn StorageService>,
    ) -> Self {
        let user_home = path_service.user_home().then(|home| home.path().to_string_lossy());
        let resolve_context = BaseResolveContext {
            app_context: context,
            editor_service,
            configuration_service: configuration_service.clone(),
            workspace_context_service,
            label_service: label_service.clone(),
            path_service,
            extension_service,
        };

        Self {
            base: AbstractVariableResolverService::new(Rc::new(resolve_context), Some(label_service), Some(user_home), Some(env_variables_promise)),
            inner: Rc::new(BaseInner { configuration_service, command_service, quick_input_service, storage_service, user_input_access_queue: Queue::new() }),
        }
    }

    /// `resolveWithInteraction` de uma expressão já analisada: o laço que `resolveWithInteractionReplace` também usa (o
    /// original passa a expressão `parsed` adiante, e o `parse` a devolve como veio).
    fn resolve_expression_with_interaction(
        &self,
        folder: Option<&WorkspaceFolder>,
        expr: Rc<ConfigurationResolverExpression>,
        section: Option<&str>,
        variable_to_command_map: Option<&HashMap<String, String>>,
        target: Option<ConfigurationTarget>,
    ) -> Promise<Option<ResolvedVariables>> {
        let this = self.inner.clone();
        let base = self.base.clone();
        let folder = folder.cloned();
        let section = section.map(str::to_string);
        let variable_to_command_map = variable_to_command_map.cloned();

        async_fn(async move {
            // Get values for input variables from UI
            for variable in expr.unresolved() {
                let result: Option<IResolvedValue>;
                let variable_name = variable.name.to_string_lossy();
                let variable_arg = variable.arg.as_ref().map_or_else(|| "undefined".to_string(), JsString::to_string_lossy);

                // Command
                if variable_name == "command" {
                    let command_id = variable_to_command_map
                        .as_ref()
                        .and_then(|map| map.get(&variable_arg))
                        .filter(|command_id| !command_id.is_empty())
                        .cloned()
                        .unwrap_or_else(|| variable_arg.clone());
                    let value = this.command_service.execute_command(&command_id, vec![expr.to_object()]).await?;
                    result = if is_undefined_or_null(&value) {
                        None
                    } else if let Dynamic::String(text) = value {
                        Some(IResolvedValue::from(text))
                    } else {
                        return Err(VariableError::new(
                            VariableKind::Command,
                            localize(
                                "commandVariable.noStringType",
                                "Cannot substitute command variable '{0}' because command did not return a result of type string.",
                                &[command_id.as_str()],
                            ),
                        ));
                    };
                }
                // Input
                else if variable_name == "input" {
                    let input_infos = resolve_inputs(&this, folder.as_ref(), section.as_deref(), target);
                    result = show_user_input(this.clone(), section.clone().unwrap_or_default(), variable_arg, input_infos, variable_to_command_map.clone()).await?;
                }
                // Contributed variable
                else if let Some(contributed_variable) = base.contributed_variable(&variable.inner.to_string_lossy()) {
                    result = Some(IResolvedValue { value: contributed_variable().await?.map(JsString::from), input: None });
                } else {
                    // Fallback to parent evaluation
                    let folder_uri = folder.as_ref().map(|folder| folder.uri.clone());
                    let resolved_value = base.evaluate_single_variable(&variable, folder_uri.as_ref(), None, None).await?;
                    if resolved_value.is_undefined() {
                        // Not something we can handle
                        continue;
                    }
                    result = Some(IResolvedValue::from(&resolved_value));
                }

                let Some(result) = result else {
                    // Skip the entire flow if any input variable was canceled
                    return Ok(None);
                };

                expr.resolve(&variable, result);
            }

            Ok(Some(expr.resolved().into_iter().map(|(key, value)| (key.inner.to_string_lossy(), value.value.map(|text| text.to_string_lossy()))).collect()))
        })
    }
}

impl IConfigurationResolverService for BaseConfigurationResolverService {
    fn resolvable_variables(&self) -> Vec<String> {
        self.base.resolvable_variables()
    }

    fn resolve_with_environment(&self, environment: &ProcessEnvironment, folder: Option<&WorkspaceFolder>, value: &str) -> Promise<String> {
        self.base.resolve_with_environment(environment, folder, value)
    }

    fn resolve_async(&self, folder: Option<&WorkspaceFolder>, config: &Dynamic) -> Promise<Dynamic> {
        self.base.resolve_async(folder, config)
    }

    fn resolve_with_interaction_replace(
        &self,
        folder: Option<&WorkspaceFolder>,
        config: &Dynamic,
        section: Option<&str>,
        variables: Option<&HashMap<String, String>>,
        target: Option<ConfigurationTarget>,
    ) -> Promise<Dynamic> {
        let parsed = ConfigurationResolverExpression::parse(config);
        let resolved = self.resolve_expression_with_interaction(folder, parsed.clone(), section, variables, target);

        async_fn(async move {
            // Skip if input variable was canceled
            match resolved.await? {
                None => Ok(Dynamic::Undefined),
                Some(_) => Ok(parsed.to_object()),
            }
        })
    }

    fn resolve_with_interaction(
        &self,
        folder: Option<&WorkspaceFolder>,
        config: &Dynamic,
        section: Option<&str>,
        variables: Option<&HashMap<String, String>>,
        target: Option<ConfigurationTarget>,
    ) -> Promise<Option<ResolvedVariables>> {
        self.resolve_expression_with_interaction(folder, ConfigurationResolverExpression::parse(config), section, variables, target)
    }

    fn contribute_variable(&self, variable: &str, resolution: ContributedVariable) {
        self.base.contribute_variable(variable, resolution);
    }
}

/// `inputs` de um valor de configuração (`value?.inputs`).
fn inputs_of(value: &Dynamic) -> Dynamic {
    if value.is_nullish() { Dynamic::Undefined } else { value.get_str("inputs") }
}

/// `resolveInputs(folder, section, target)`.
fn resolve_inputs(this: &BaseInner, folder: Option<&WorkspaceFolder>, section: Option<&str>, target: Option<ConfigurationTarget>) -> Dynamic {
    let Some(section) = section.filter(|section| !section.is_empty()) else {
        return Dynamic::Undefined;
    };

    // Look at workspace configuration
    let overrides = folder.map_or_else(ConfigurationOverrides::default, |folder| ConfigurationOverrides::with_resource(folder.uri.clone()));
    let result = this.configuration_service.inspect(section, &overrides);

    let layer = match target {
        Some(ConfigurationTarget::Memory) => result.memory_value(),
        Some(ConfigurationTarget::Default) => result.default_value(),
        Some(ConfigurationTarget::User) => result.user_value(),
        Some(ConfigurationTarget::UserLocal) => result.user_local_value(),
        Some(ConfigurationTarget::UserRemote) => result.user_remote_value(),
        Some(ConfigurationTarget::Application) => result.application_value(),
        Some(ConfigurationTarget::Workspace) => result.workspace_value(),
        Some(ConfigurationTarget::WorkspaceFolder) | None => result.workspace_folder_value(),
    };
    let inputs = inputs_of(&layer);

    if inputs.is_nullish() {
        return inputs_of(&this.configuration_service.get_value(Some(section), Some(&overrides)));
    }
    inputs
}

impl BaseInner {
    fn read_input_lru(&self) -> LRUCache<String, String> {
        let contents = self.storage_service.get(LAST_INPUT_STORAGE_KEY, StorageScope::Workspace, None);
        let lru: LRUCache<String, String> = LRUCache::new(LAST_INPUT_CACHE_SIZE, 1.0);
        // try { lru.fromJSON(JSON.parse(contents)) } catch { /* ignored */ }
        if let Some(contents) = contents.filter(|contents| !contents.is_empty())
            && let Ok(Value::Array(entries)) = json_parse(JsString::from(contents).units())
        {
            lru.clear();
            for entry in entries {
                let Value::Array(pair) = entry else { break };
                if let [Value::String(key), Value::String(value)] = pair.as_slice() {
                    lru.set(key.to_string_lossy(), value.to_string_lossy());
                }
            }
        }

        lru
    }

    fn store_input_lru(&self, lru: &LRUCache<String, String>) {
        let entries = Dynamic::array(lru.to_json().into_iter().map(|(key, value)| Dynamic::array([Dynamic::string(&key), Dynamic::string(&value)])));
        self.storage_service.store(
            LAST_INPUT_STORAGE_KEY,
            StorageValue::String(json_stringify(&entries).unwrap_or_default()),
            StorageScope::Workspace,
            StorageTarget::Machine,
        );
    }
}

/// O texto de `value` quando ele não é `undefined` nem `null` (a conversão do JavaScript para texto).
fn optional_text(value: &Dynamic) -> Option<String> {
    if value.is_nullish() { None } else { Some(to_js_string(value).to_string_lossy()) }
}

/// `showUserInput(section, variable, inputInfos, variableToCommandMap)`.
async fn show_user_input(
    this: Rc<BaseInner>,
    section: String,
    variable: String,
    input_infos: Dynamic,
    variable_to_command_map: Option<HashMap<String, String>>,
) -> Result<Option<IResolvedValue>, Error> {
    if !input_infos.is_truthy() {
        return Err(VariableError::new(
            VariableKind::Input,
            localize("inputVariable.noInputSection", "Variable '{0}' must be defined in an '{1}' section of the debug or task configuration.", &[variable.as_str(), "inputs"]),
        ));
    }
    let Dynamic::Array(input_infos) = &input_infos else {
        return Err(Error::type_error("inputInfos.filter is not a function"));
    };

    // Find info for the given input variable
    let variable_value = Dynamic::string(&variable);
    let mut matching = Vec::new();
    for item in input_infos.to_vec() {
        if item.get_str("id").strict_equals(&variable_value) {
            matching.push(item);
        }
    }
    let Some(info) = matching.pop() else {
        return Err(VariableError::new(
            VariableKind::Input,
            localize("inputVariable.undefinedVariable", "Undefined input variable '{0}' encountered. Remove or define '{0}' to continue.", &[variable.as_str()]),
        ));
    };

    let info_type = info.get_str("type");
    let missing_attribute = |attr_name: &str| -> Error {
        VariableError::new(
            VariableKind::Input,
            localize(
                "inputVariable.missingAttribute",
                "Input variable '{0}' is of type '{1}' and must include '{2}'.",
                &[variable.as_str(), to_js_string(&info_type).to_string_lossy().as_str(), attr_name],
            ),
        )
    };

    let default_value_map = this.read_input_lru();
    let default_value_key = format!("{section}.{variable}");
    let previous_picked_value = default_value_map.get(&default_value_key);
    let mapped_value = variable_to_command_map.as_ref().and_then(|map| map.get(&format!("input:{variable}"))).cloned();
    let top_value = match mapped_value.or(previous_picked_value) {
        Some(value) => Dynamic::string(&value),
        None => info.get_str("default"),
    };

    let kind = match &info_type {
        Dynamic::String(kind) => kind.to_string_lossy(),
        _ => String::new(),
    };
    match kind.as_str() {
        "promptString" => {
            let Dynamic::String(description) = info.get_str("description") else {
                return Err(missing_attribute("description"));
            };
            let password = info.get_str("password").is_truthy();
            let mut input_options = IInputOptions {
                prompt: Some(description.to_string_lossy()),
                ignore_focus_lost: Some(true),
                value: optional_text(&top_value),
                ..IInputOptions::default()
            };
            if password {
                input_options.password = Some(true);
            }

            let queue_this = this.clone();
            let resolved_input = this
                .user_input_access_queue
                .queue(move || queue_this.quick_input_service.input(Some(&input_options)).then(UserInputResult::Text))
                .await?
                .into_text();
            if let Some(text) = &resolved_input
                && !password
            {
                default_value_map.set(default_value_key, text.clone());
                this.store_input_lru(&default_value_map);
            }
            Ok(resolved_input.map(|value| IResolvedValue { value: Some(JsString::from(value)), input: Some(ConfiguredInput::new(info.clone())) }))
        }

        "pickString" => {
            let Dynamic::String(description) = info.get_str("description") else {
                return Err(missing_attribute("description"));
            };
            let Dynamic::Array(options) = info.get_str("options") else {
                return Err(missing_attribute("options"));
            };
            let options = options.to_vec();
            for pick_option in &options {
                if !matches!(pick_option, Dynamic::String(_)) && !matches!(pick_option.get_str("value"), Dynamic::String(_)) {
                    return Err(missing_attribute("value"));
                }
            }

            let default_value = info.get_str("default");
            let mut picks: Vec<IQuickPickItem> = Vec::new();
            for pick_option in &options {
                let (value, label) = match pick_option {
                    Dynamic::String(text) => (text.clone(), Dynamic::Undefined),
                    _ => (to_js_string(&pick_option.get_str("value")), pick_option.get_str("label")),
                };

                let item_label = if label.is_truthy() { format!("{}: {}", to_js_string(&label), value) } else { value.to_string_lossy() };
                let mut item = IQuickPickItem {
                    base: IQuickItem { label: JsString::from(item_label), ..IQuickItem::default() },
                    extra: Dynamic::object([("value", Dynamic::String(value.clone()))]),
                };

                let value = Dynamic::String(value);
                if value.strict_equals(&default_value) {
                    item.base.description = Some(JsString::from(localize("inputVariable.defaultInputValue", "(Default)", &[])));
                    picks.insert(0, item);
                } else if value.strict_equals(&top_value) {
                    picks.insert(0, item);
                } else {
                    picks.push(item);
                }
            }

            let pick_options = IPickOptions { place_holder: Some(description.to_string_lossy()), match_on_detail: Some(true), ignore_focus_lost: Some(true) };
            let queue_this = this.clone();
            let resolved_input = this
                .user_input_access_queue
                .queue(move || queue_this.quick_input_service.pick(picks, Some(&pick_options)).then(UserInputResult::Pick))
                .await?
                .into_pick();
            match resolved_input {
                Some(resolved_input) => {
                    let value = to_js_string(&resolved_input.extra.get_str("value"));
                    default_value_map.set(default_value_key, value.to_string_lossy());
                    this.store_input_lru(&default_value_map);
                    Ok(Some(IResolvedValue { value: Some(value), input: Some(ConfiguredInput::new(info.clone())) }))
                }
                None => Ok(None),
            }
        }

        "command" => {
            let Dynamic::String(command) = info.get_str("command") else {
                return Err(missing_attribute("command"));
            };
            let command_id = command.to_string_lossy();
            let args = info.get_str("args");
            let queue_this = this.clone();
            let queued_command_id = command_id.clone();
            let result = this
                .user_input_access_queue
                .queue(move || queue_this.command_service.execute_command(&queued_command_id, vec![args]).then(UserInputResult::Command))
                .await?
                .into_command();
            let value = match result {
                Dynamic::String(text) => Some(text),
                Dynamic::Undefined => None,
                Dynamic::Null => Some(JsString::from("null")),
                _ => {
                    return Err(VariableError::new(
                        VariableKind::Input,
                        localize(
                            "inputVariable.command.noStringType",
                            "Cannot substitute input variable '{0}' because command '{1}' did not return a result of type string.",
                            &[variable.as_str(), command_id.as_str()],
                        ),
                    ));
                }
            };
            Ok(Some(IResolvedValue { value, input: Some(ConfiguredInput::new(info.clone())) }))
        }

        _ => Err(VariableError::new(
            VariableKind::Input,
            localize("inputVariable.unknownType", "Input variable '{0}' can only be of type 'promptString', 'pickString', or 'command'.", &[variable.as_str()]),
        )),
    }
}
