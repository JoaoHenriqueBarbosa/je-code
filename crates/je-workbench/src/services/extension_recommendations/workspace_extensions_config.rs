//! Porte de `src/vs/workbench/services/extensionRecommendations/common/workspaceExtensionsConfig.ts`: lê e edita as
//! recomendações de extensão do workspace (`.vscode/extensions.json` de cada pasta e a chave `extensions` do arquivo de
//! workspace).
//!
//! Notas de tradução:
//! - **Injeção de dependência.** Os serviços do construtor entram agrupados em [`WorkspaceExtensionsConfigServiceParts`].
//! - **`IWorkspace | IWorkspaceFolder`** de `pickWorkspaceOrFolders` é [`WorkspaceOrFolder`]. O item do quick pick guarda o
//!   índice do alvo em `extra` (o `workspaceOrFolder` do original) e a escolha volta por ele.
//! - **`Promise.all`** das pastas roda uma pasta por vez dentro de uma promessa só; a leitura de cada `extensions.json` é
//!   independente e o resultado mantém a ordem das pastas, como o original.

use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{self, Value};
use je_base::json_edit::EditSegment;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::get_icon_classes::get_icon_classes;
use je_editor::language_service::ILanguageService;
use je_editor::services_model::IModelService;
use je_platform::files::files::FileKind;
use je_platform::files::i_file_service::IFileService;
use je_platform::quickinput::quick_input::{IPickOptions, IQuickPickItem, IQuickPickSeparator, IQuickInputService, QuickPickInput, QuickPicks};
use je_platform::workspace::{IWorkspaceContextService, Workspace, WorkspaceFolder};

use crate::services::configuration::common::json_editing::{IJsonEditingService, JsonValue};

/// `EXTENSIONS_CONFIG`.
pub const EXTENSIONS_CONFIG: &str = ".vscode/extensions.json";

/// `IExtensionsConfigContent`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IExtensionsConfigContent {
    pub recommendations: Option<Vec<String>>,
    pub unwanted_recommendations: Option<Vec<String>>,
}

/// `IWorkspaceExtensionsConfigService`.
pub trait IWorkspaceExtensionsConfigService {
    fn on_did_change_extensions_configs(&self) -> Event<()>;
    fn get_extensions_configs(&self) -> Promise<Vec<IExtensionsConfigContent>>;
    fn get_recommendations(&self) -> Promise<Vec<String>>;
    fn get_unwanted_recommendations(&self) -> Promise<Vec<String>>;

    fn toggle_recommendation(&self, extension_id: &str) -> Promise<()>;
    fn toggle_unwanted_recommendation(&self, extension_id: &str) -> Promise<()>;
}

/// `IWorkspace | IWorkspaceFolder`.
#[derive(Clone)]
pub enum WorkspaceOrFolder {
    Workspace(Rc<Workspace>),
    Folder(WorkspaceFolder),
}

/// Os serviços que o construtor do original recebe por decorador.
///
/// `quick_input_service` e `json_editing_service` só servem a `toggleRecommendation` e `toggleUnwantedRecommendation`:
/// `None` é o serviço não registrado (o `TestInstantiationService` entrega `undefined`), e usá-lo rejeita a promessa com o
/// `TypeError` que o original lança.
pub struct WorkspaceExtensionsConfigServiceParts {
    pub workspace_context_service: Rc<dyn IWorkspaceContextService>,
    pub file_service: Rc<dyn IFileService>,
    pub quick_input_service: Option<Rc<dyn IQuickInputService>>,
    pub model_service: Rc<dyn IModelService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub json_editing_service: Option<Rc<dyn IJsonEditingService>>,
}

/// `distinct(array)` de strings, na ordem da primeira ocorrência.
fn distinct_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for value in values {
        if !result.contains(&value) {
            result.push(value);
        }
    }
    result
}

/// `(list || []).map(e => e.toLowerCase())`: falha, como o original, se algum elemento não for texto.
fn lower_case_list(value: Option<&Value>) -> Result<Vec<String>, Error> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => Ok(text.to_string().to_lowercase()),
                _ => Err(Error::new("e.toLowerCase is not a function")),
            })
            .collect(),
        Some(_) => Err(Error::new("(intermediate value).map is not a function")),
    }
}

/// `string[] | undefined` do campo de um objeto JSON, sem normalizar (o `recommendations` cru de um arquivo já normalizado).
fn parse_extension_config(content: Option<&Value>) -> Result<IExtensionsConfigContent, Error> {
    let object = match content {
        Some(Value::Object(object)) => Some(object),
        _ => None,
    };
    let recommendations = lower_case_list(object.and_then(|object| object.get("recommendations")))?;
    let unwanted = lower_case_list(object.and_then(|object| object.get("unwantedRecommendations")))?;
    Ok(IExtensionsConfigContent {
        recommendations: Some(distinct_strings(recommendations)),
        unwanted_recommendations: Some(distinct_strings(unwanted)),
    })
}

/// `getEditToRemoveValueFromArray`.
fn get_edit_to_remove_value_from_array(path: &[&str], array: Option<&Vec<String>>, value: &str) -> Option<JsonValue> {
    let index = array?.iter().position(|item| item == value)?;
    let mut segments: Vec<EditSegment> = path.iter().map(|segment| EditSegment::from(*segment)).collect();
    segments.push(EditSegment::Index(index as i64));
    Some(JsonValue { path: segments, value: Dynamic::Undefined })
}

fn path_with_end(path: &[&str]) -> Vec<EditSegment> {
    let mut segments: Vec<EditSegment> = path.iter().map(|segment| EditSegment::from(*segment)).collect();
    segments.push(EditSegment::Index(-1));
    segments
}

fn path_of(path: &[&str]) -> Vec<EditSegment> {
    path.iter().map(|segment| EditSegment::from(*segment)).collect()
}

fn string_array(id: &str) -> Dynamic {
    Dynamic::array([Dynamic::string(id)])
}

/// `WorkspaceExtensionsConfigService`.
pub struct WorkspaceExtensionsConfigService {
    this: Weak<WorkspaceExtensionsConfigService>,
    base: Rc<Disposable>,
    on_did_change_extensions_configs: Rc<Emitter<()>>,
    p: WorkspaceExtensionsConfigServiceParts,
}

impl WorkspaceExtensionsConfigService {
    /// `constructor(...)`.
    pub fn new(parts: WorkspaceExtensionsConfigServiceParts) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_extensions_configs = Emitter::<()>::new();
        base.register(on_did_change_extensions_configs.clone());
        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base: base.clone(),
            on_did_change_extensions_configs: on_did_change_extensions_configs.clone(),
            p: parts,
        });

        let emitter = on_did_change_extensions_configs.clone();
        this.base.register(this.p.workspace_context_service.on_did_change_workspace_folders().listen(move |_| emitter.fire(())));

        let emitter = on_did_change_extensions_configs.clone();
        let context = this.p.workspace_context_service.clone();
        this.base.register(this.p.file_service.on_did_files_change().listen(move |e| {
            let workspace = context.get_workspace();
            let affects_workspace_file = workspace.configuration().is_some_and(|configuration| e.affects(&configuration, &[]));
            if affects_workspace_file || workspace.folders().iter().any(|folder| e.affects(&folder.to_resource(EXTENSIONS_CONFIG), &[])) {
                emitter.fire(());
            }
        }));
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `resolveWorkspaceExtensionConfig`.
    fn resolve_workspace_extension_config(&self, workspace_configuration_resource: URI) -> Promise<Option<IExtensionsConfigContent>> {
        let file_service = self.p.file_service.clone();
        async_fn(async move {
            let attempt = async {
                let content = file_service.read_file(&workspace_configuration_resource, None, None).await?;
                let mut errors = Vec::new();
                let parsed = json::parse_str(&content.value.to_string(), &mut errors);
                let extensions = match &parsed {
                    Some(Value::Object(object)) => object.get("extensions"),
                    _ => None,
                };
                // `extensionsConfigContent ? parse : undefined`: os valores falsos do JavaScript viram `undefined`.
                match extensions {
                    None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(None),
                    Some(Value::String(text)) if text.is_empty() => Ok(None),
                    Some(Value::Number(number)) if *number == 0.0 || number.is_nan() => Ok(None),
                    Some(value) => parse_extension_config(Some(value)).map(Some),
                }
            };
            Ok(attempt.await.unwrap_or(None))
        })
    }

    /// `resolveWorkspaceFolderExtensionConfig`.
    fn resolve_workspace_folder_extension_config(&self, workspace_folder: WorkspaceFolder) -> Promise<IExtensionsConfigContent> {
        let file_service = self.p.file_service.clone();
        async_fn(async move {
            let attempt = async {
                let content = file_service.read_file(&workspace_folder.to_resource(EXTENSIONS_CONFIG), None, None).await?;
                let mut errors = Vec::new();
                let parsed = json::parse_str(&content.value.to_string(), &mut errors);
                // `parse(...)` indefinido ou nulo lança no acesso a `.recommendations`, e o `catch` devolve `{}`.
                match parsed {
                    None | Some(Value::Null) => Err(Error::new("Cannot read properties of undefined")),
                    Some(value) => parse_extension_config(Some(&value)),
                }
            };
            Ok(attempt.await.unwrap_or_default())
        })
    }

    /// As configurações do arquivo de workspace (se houver) e de cada pasta, na ordem do original.
    fn resolve_all(&self) -> Promise<(Option<IExtensionsConfigContent>, Vec<(WorkspaceFolder, IExtensionsConfigContent)>)> {
        let this = self.rc();
        async_fn(async move {
            let workspace = this.p.workspace_context_service.get_workspace();
            let workspace_content = match workspace.configuration() {
                Some(configuration) => this.resolve_workspace_extension_config(configuration).await?,
                None => None,
            };
            let mut folders = Vec::new();
            for folder in workspace.folders() {
                let content = this.resolve_workspace_folder_extension_config(folder.clone()).await?;
                folders.push((folder, content));
            }
            Ok((workspace_content, folders))
        })
    }

    /// `pickWorkspaceOrFolders`.
    fn pick_workspace_or_folders(&self, workspace_folders: Vec<WorkspaceFolder>, workspace: Option<Rc<Workspace>>, place_holder: String) -> Promise<Vec<WorkspaceOrFolder>> {
        let this = self.rc();
        async_fn(async move {
            let mut workspace_or_folders: Vec<WorkspaceOrFolder> = workspace_folders.iter().cloned().map(WorkspaceOrFolder::Folder).collect();
            if let Some(workspace) = &workspace {
                workspace_or_folders.push(WorkspaceOrFolder::Workspace(workspace.clone()));
            }
            if workspace_or_folders.len() == 1 {
                return Ok(workspace_or_folders);
            }

            let mut folder_picks: Vec<QuickPickInput<IQuickPickItem>> = Vec::new();
            for (index, workspace_folder) in workspace_folders.iter().enumerate() {
                let mut item = IQuickPickItem::default();
                item.base.label = workspace_folder.name.clone().into();
                item.base.description = Some(localize("workspace folder", "Workspace Folder").into());
                item.base.icon_classes = Some(get_icon_classes(
                    this.p.model_service.as_ref(),
                    this.p.language_service.as_ref(),
                    Some(&workspace_folder.uri),
                    Some(FileKind::RootFolder),
                    None,
                ));
                item.extra = Dynamic::object([("workspaceOrFolder", Dynamic::Number(index as f64))]);
                folder_picks.push(QuickPickInput::Item(item));
            }

            if workspace.is_some() {
                folder_picks.push(QuickPickInput::Separator(IQuickPickSeparator::default()));
                let mut item = IQuickPickItem::default();
                item.base.label = localize("workspace", "Workspace").into();
                item.extra = Dynamic::object([("workspaceOrFolder", Dynamic::Number(workspace_folders.len() as f64))]);
                folder_picks.push(QuickPickInput::Item(item));
            }

            let Some(quick_input_service) = this.p.quick_input_service.clone() else {
                return Err(Error::new("Cannot read properties of undefined (reading 'pick')"));
            };
            let options = IPickOptions { place_holder: Some(place_holder), can_pick_many: Some(true), ..IPickOptions::default() };
            let result = quick_input_service.pick_many(QuickPicks::Items(folder_picks), Some(&options), None).await?.unwrap_or_default();
            let mut picked = Vec::new();
            for item in result {
                if let Dynamic::Number(index) = item.extra.get_str("workspaceOrFolder") {
                    if let Some(target) = workspace_or_folders.get(index as usize) {
                        picked.push(target.clone());
                    }
                }
            }
            Ok(picked)
        })
    }

    fn add_or_remove_workspace_folder_recommendation(&self, extension_id: &str, workspace_folder: &WorkspaceFolder, extensions_config_content: &IExtensionsConfigContent, add: bool) -> Promise<()> {
        let mut values: Vec<JsonValue> = Vec::new();
        if add {
            if extensions_config_content.recommendations.is_some() {
                values.push(JsonValue { path: path_with_end(&["recommendations"]), value: Dynamic::string(extension_id) });
            } else {
                values.push(JsonValue { path: path_of(&["recommendations"]), value: string_array(extension_id) });
            }
            if let Some(edit) = get_edit_to_remove_value_from_array(&["unwantedRecommendations"], extensions_config_content.unwanted_recommendations.as_ref(), extension_id) {
                values.push(edit);
            }
        } else if extensions_config_content.recommendations.is_some() {
            if let Some(edit) = get_edit_to_remove_value_from_array(&["recommendations"], extensions_config_content.recommendations.as_ref(), extension_id) {
                values.push(edit);
            }
        }
        self.write_if_any(workspace_folder.to_resource(EXTENSIONS_CONFIG), values)
    }

    fn add_or_remove_workspace_recommendation(&self, extension_id: &str, workspace: &Workspace, extensions_config_content: Option<&IExtensionsConfigContent>, add: bool) -> Promise<()> {
        let mut values: Vec<JsonValue> = Vec::new();
        if let Some(content) = extensions_config_content {
            if add {
                if content.recommendations.is_some() {
                    values.push(JsonValue { path: path_with_end(&["extensions", "recommendations"]), value: Dynamic::string(extension_id) });
                } else {
                    values.push(JsonValue { path: path_of(&["extensions", "recommendations"]), value: string_array(extension_id) });
                }
                if let Some(edit) = get_edit_to_remove_value_from_array(&["extensions", "unwantedRecommendations"], content.unwanted_recommendations.as_ref(), extension_id) {
                    values.push(edit);
                }
            } else if content.recommendations.is_some() {
                if let Some(edit) = get_edit_to_remove_value_from_array(&["extensions", "recommendations"], content.recommendations.as_ref(), extension_id) {
                    values.push(edit);
                }
            }
        } else if add {
            values.push(JsonValue { path: path_of(&["extensions"]), value: Dynamic::object([("recommendations", string_array(extension_id))]) });
        }
        match workspace.configuration() {
            Some(configuration) => self.write_if_any(configuration, values),
            None => Promise::resolved(()),
        }
    }

    fn add_or_remove_workspace_folder_unwanted_recommendation(&self, extension_id: &str, workspace_folder: &WorkspaceFolder, extensions_config_content: &IExtensionsConfigContent, add: bool) -> Promise<()> {
        let mut values: Vec<JsonValue> = Vec::new();
        if add {
            if extensions_config_content.unwanted_recommendations.is_some() {
                values.push(JsonValue { path: path_with_end(&["unwantedRecommendations"]), value: Dynamic::string(extension_id) });
            } else {
                values.push(JsonValue { path: path_of(&["unwantedRecommendations"]), value: string_array(extension_id) });
            }
            if let Some(edit) = get_edit_to_remove_value_from_array(&["recommendations"], extensions_config_content.recommendations.as_ref(), extension_id) {
                values.push(edit);
            }
        } else if extensions_config_content.unwanted_recommendations.is_some() {
            if let Some(edit) = get_edit_to_remove_value_from_array(&["unwantedRecommendations"], extensions_config_content.unwanted_recommendations.as_ref(), extension_id) {
                values.push(edit);
            }
        }
        self.write_if_any(workspace_folder.to_resource(EXTENSIONS_CONFIG), values)
    }

    fn add_or_remove_workspace_unwanted_recommendation(&self, extension_id: &str, workspace: &Workspace, extensions_config_content: Option<&IExtensionsConfigContent>, add: bool) -> Promise<()> {
        let mut values: Vec<JsonValue> = Vec::new();
        if let Some(content) = extensions_config_content {
            if add {
                // O original testa `recommendations` (e não `unwantedRecommendations`) para decidir entre acrescentar e criar.
                if content.recommendations.is_some() {
                    values.push(JsonValue { path: path_with_end(&["extensions", "unwantedRecommendations"]), value: Dynamic::string(extension_id) });
                } else {
                    values.push(JsonValue { path: path_of(&["extensions", "unwantedRecommendations"]), value: string_array(extension_id) });
                }
                if let Some(edit) = get_edit_to_remove_value_from_array(&["extensions", "recommendations"], content.recommendations.as_ref(), extension_id) {
                    values.push(edit);
                }
            } else if content.unwanted_recommendations.is_some() {
                if let Some(edit) = get_edit_to_remove_value_from_array(&["extensions", "unwantedRecommendations"], content.unwanted_recommendations.as_ref(), extension_id) {
                    values.push(edit);
                }
            }
        } else if add {
            values.push(JsonValue { path: path_of(&["extensions"]), value: Dynamic::object([("unwantedRecommendations", string_array(extension_id))]) });
        }
        match workspace.configuration() {
            Some(configuration) => self.write_if_any(configuration, values),
            None => Promise::resolved(()),
        }
    }

    /// `if (values.length) return this.jsonEditingService.write(resource, values, true)`.
    fn write_if_any(&self, resource: URI, values: Vec<JsonValue>) -> Promise<()> {
        if values.is_empty() {
            return Promise::resolved(());
        }
        match &self.p.json_editing_service {
            Some(json_editing_service) => json_editing_service.write(&resource, values, true),
            None => Promise::rejected(Error::new("Cannot read properties of undefined (reading 'write')")),
        }
    }

    /// O corpo comum de `toggleRecommendation` e `toggleUnwantedRecommendation`: a lista que decide o estado é
    /// `selected` (`recommendations` ou `unwantedRecommendations`), `extension_id` já vem no formato que o original compara.
    fn toggle(&self, extension_id: String, unwanted: bool) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let extension_id = if unwanted { extension_id } else { extension_id.to_lowercase() };
            let workspace = this.p.workspace_context_service.get_workspace();
            let (workspace_content, folder_contents) = this.resolve_all().await?;

            let list_of = |content: &IExtensionsConfigContent| if unwanted { content.unwanted_recommendations.clone() } else { content.recommendations.clone() };
            let contains = |list: Option<Vec<String>>| {
                list.is_some_and(|list| list.iter().any(|item| if unwanted { *item == extension_id } else { item.to_lowercase() == extension_id }))
            };

            let is_workspace_marked = workspace_content.as_ref().is_some_and(|content| contains(list_of(content)));
            let marked_folders: Vec<WorkspaceFolder> =
                workspace.folders().into_iter().filter(|folder| folder_contents.iter().any(|(candidate, content)| candidate.uri == folder.uri && contains(list_of(content)))).collect();
            let is_marked = is_workspace_marked || !marked_folders.is_empty();

            let workspace_or_folders = if is_marked {
                this.pick_workspace_or_folders(
                    marked_folders,
                    if is_workspace_marked { Some(workspace.clone()) } else { None },
                    localize("select for remove", "Remove extension recommendation from"),
                )
                .await?
            } else {
                this.pick_workspace_or_folders(
                    workspace.folders(),
                    if workspace.configuration().is_some() { Some(workspace.clone()) } else { None },
                    localize("select for add", "Add extension recommendation to"),
                )
                .await?
            };

            for target in workspace_or_folders {
                match target {
                    WorkspaceOrFolder::Workspace(target_workspace) => {
                        if unwanted {
                            this.add_or_remove_workspace_unwanted_recommendation(&extension_id, &target_workspace, workspace_content.as_ref(), !is_marked).await?;
                        } else {
                            this.add_or_remove_workspace_recommendation(&extension_id, &target_workspace, workspace_content.as_ref(), !is_marked).await?;
                        }
                    }
                    WorkspaceOrFolder::Folder(target_folder) => {
                        let content = folder_contents.iter().find(|(candidate, _)| candidate.uri == target_folder.uri).map(|(_, content)| content.clone()).unwrap_or_default();
                        if unwanted {
                            this.add_or_remove_workspace_folder_unwanted_recommendation(&extension_id, &target_folder, &content, !is_marked).await?;
                        } else {
                            this.add_or_remove_workspace_folder_recommendation(&extension_id, &target_folder, &content, !is_marked).await?;
                        }
                    }
                }
            }
            Ok(())
        })
    }
}

impl IWorkspaceExtensionsConfigService for WorkspaceExtensionsConfigService {
    fn on_did_change_extensions_configs(&self) -> Event<()> {
        self.on_did_change_extensions_configs.event()
    }

    fn get_extensions_configs(&self) -> Promise<Vec<IExtensionsConfigContent>> {
        let this = self.rc();
        async_fn(async move {
            let (workspace_content, folder_contents) = this.resolve_all().await?;
            let mut result: Vec<IExtensionsConfigContent> = Vec::new();
            if let Some(content) = workspace_content {
                result.push(content);
            }
            result.extend(folder_contents.into_iter().map(|(_, content)| content));
            Ok(result)
        })
    }

    fn get_recommendations(&self) -> Promise<Vec<String>> {
        self.get_extensions_configs().then(|configs| {
            distinct_strings(configs.into_iter().flat_map(|config| config.recommendations.unwrap_or_default().into_iter().map(|id| id.to_lowercase())))
        })
    }

    fn get_unwanted_recommendations(&self) -> Promise<Vec<String>> {
        self.get_extensions_configs().then(|configs| {
            distinct_strings(configs.into_iter().flat_map(|config| config.unwanted_recommendations.unwrap_or_default().into_iter().map(|id| id.to_lowercase())))
        })
    }

    fn toggle_recommendation(&self, extension_id: &str) -> Promise<()> {
        self.toggle(extension_id.to_owned(), false)
    }

    fn toggle_unwanted_recommendation(&self, extension_id: &str) -> Promise<()> {
        self.toggle(extension_id.to_owned(), true)
    }
}

impl IDisposable for WorkspaceExtensionsConfigService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
