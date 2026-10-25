//! `vscode.workspace`, ligado ao `ExtHostWorkspace` e ao `ExtHostConfiguration` portados (`extHost.api.impl.ts`, o literal
//! `workspace`), mais a classe `vscode.RelativePattern`.
//!
//! Entregue: `name`, `rootPath`, `workspaceFolders`, `workspaceFile`, `isTrusted`, `getWorkspaceFolder`, `asRelativePath`,
//! `getConfiguration` (`get`, `has`, `update`, `inspect` e os valores como propriedades), `findFiles`,
//! `onDidChangeConfiguration`, `onDidChangeWorkspaceFolders`, `onDidGrantWorkspaceTrust`, `fs` e `registerFileSystemProvider` (módulo
//! `file_system`, quando o binder registrou `MainThreadFileSystem`; o registro pede também os clientes de linguagem) e, quando o binder
//! registrou
//! `MainThreadDocuments` e `MainThreadTextEditors`, `textDocuments`, `openTextDocument`, `onDidOpenTextDocument`,
//! `onDidCloseTextDocument`, `onDidChangeTextDocument` e `onDidSaveTextDocument` (módulo `text_editors`).
//!
//! Falta (cada um depende de um serviço do host ainda não portado, e não é preenchido com fachada):
//! - `createFileSystemWatcher` e `onDid/WillCreate/Delete/RenameFiles` existem (módulo `file_system_events`) quando o host tem o
//!   cliente `MainThreadFileSystemEventService` (o watcher pede também os clientes de workspace e configuração);
//! - `applyEdit` existe (módulo `workspace_edit`, com a classe `WorkspaceEdit`) quando o host tem o cliente `MainThreadBulkEdits`;
//! - `notebookDocuments`, `openNotebookDocument`, `onDidOpenNotebookDocument` e `onDidCloseNotebookDocument` existem (módulo `notebooks`)
//!   quando o host tem os clientes `MainThreadNotebook`, `MainThreadNotebookDocuments` e `MainThreadNotebookEditors`;
//! - `onWillSaveTextDocument`, `registerTextDocumentContentProvider`: pedem o `ExtHostDocumentSaveParticipant` e o
//!   `ExtHostDocumentContentProvider` ligados;
//! - `updateWorkspaceFolders`, `findTextInFiles`, `save`, `saveAll`, `requestWorkspaceTrust`, `getCanonicalUri`, `decode`/`encode`,
//!   `registerTaskProvider` e as propostas: o serviço do host os tem, falta só a ponte.
//!
//! `vscode.languages` (diagnósticos, `getLanguages`, `match` e os provedores de linguagem) é o módulo `languages`; `glob_of` daqui é
//! reaproveitado por ele para o `pattern` de um `DocumentFilter`.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões (um único `require('vscode')`), então o host não sabe qual extensão chama: o
//!   `getConfiguration` não passa a extensão ao aviso de escopo e o `findFiles` usa o identificador `je.exthost` no log de traço.
//! - `workspaceFolders` e os eventos entregam cada pasta como objeto `{ uri, name, index }` copiado no instante da chamada: o motor
//!   copia objetos comuns, então a pasta que a extensão guardou não acompanha um renome depois (no original o objeto é vivo).
//! - O `WorkspaceConfiguration` é um objeto com os valores como propriedades (congelados no Rust, o objeto do motor não é) e os
//!   membros `has`, `get`, `update` e `inspect` por cima, como o `mixin` sem sobrescrever do original.
//! - `onDidChangeConfiguration` lê o provedor de configuração no momento da inscrição: sem `$initializeConfiguration` ainda, o
//!   listener não recebe nada (o original só entrega a API depois da inicialização, então o caso não existe lá).

use std::rc::Rc;

use je_base::event::{Event, Listener};
use je_base::lifecycle::{to_disposable, DisposableRef};
use je_extensions::manifest::ExtensionIdentifier;
use je_workbench::api::common::ext_host_configuration::{
    ConfigurationScopeArg, ConfigurationTargetArg, ExtHostConfiguration, ExtHostConfigurationChangeEvent, WorkspaceConfiguration,
};
use je_workbench::api::common::ext_host_types::ConfigurationTarget as ExtHostConfigurationTarget;
use je_workbench::api::common::ext_host_workspace::{
    ExtHostWorkspace, FindFilesExclude, GlobPattern, RelativePathInput, RelativePattern, RelativePatternBase, WorkspaceFolder,
};
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner};

use crate::api::commands::promise_to_js;
use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, EventTables};
use crate::api::accessor_namespace;
use crate::api::text_editors::{self, TextEditorApi};
use crate::api::file_system::{self, FileSystemApi};
use crate::api::file_system_events::{self, FileSystemEventsApi};
use crate::api::notebooks::{self, NotebookApi};
use crate::api::workspace_edit::{self, WorkspaceEditObjects};
use je_workbench::api::common::ext_host_bulk_edits::ExtHostBulkEdits;
use crate::api::HOST_EXTENSION;
use crate::convert::{class_name, dynamic_to_js, field, is_nullish, js_to_dynamic, opt_number, opt_string, string_arg, uri_from_js, uri_to_js};

/// Os serviços do host que o namespace usa.
pub struct WorkspaceServices {
    /// Sem os clientes `MainThreadWorkspace`, `MainThreadConfiguration` e `MainThreadMessageService` não há pastas, configuração nem
    /// `findFiles`; o namespace continua existindo com os membros de documento, como no original, em que `vscode.workspace` sempre
    /// existe.
    pub workspace: Option<(Rc<ExtHostWorkspace>, Rc<ExtHostConfiguration>)>,
    /// Sem os clientes `MainThreadDocuments` e `MainThreadTextEditors` não há `textDocuments`, `openTextDocument` nem os eventos de
    /// documento.
    pub text_editors: Option<TextEditorApi>,
    /// Sem o cliente `MainThreadBulkEdits` não há `applyEdit` (a classe `WorkspaceEdit` existe sempre).
    pub bulk_edits: Option<(Rc<ExtHostBulkEdits>, Rc<WorkspaceEditObjects>)>,
    /// Sem o cliente `MainThreadFileSystemEventService` não há os eventos de arquivo nem o `createFileSystemWatcher`.
    pub file_system_events: Option<FileSystemEventsApi>,
    /// Sem os clientes `MainThreadNotebook*` não há `notebookDocuments`, `openNotebookDocument` nem os eventos de notebook.
    pub notebooks: Option<NotebookApi>,
    /// Sem o cliente `MainThreadFileSystem` não há `fs` nem `registerFileSystemProvider`.
    pub file_system: Option<FileSystemApi>,
}

/// Uma pasta como o objeto `vscode.WorkspaceFolder` (ver as divergências do módulo).
pub(crate) fn folder_to_js(folder: &WorkspaceFolder) -> JsData {
    JsData::Object(vec![
        ("uri".to_owned(), uri_to_js(folder.uri())),
        ("name".to_owned(), JsData::Str(folder.name())),
        ("index".to_owned(), JsData::Num(folder.index() as f64)),
    ])
}

fn folders_to_js(folders: &[Rc<WorkspaceFolder>]) -> JsData {
    JsData::Array(folders.iter().map(|folder| folder_to_js(folder)).collect())
}

/// O escopo de `getConfiguration`: `Uri`, `TextDocument`, `WorkspaceFolder`, `{ uri?, languageId }` ou `null`.
fn scope_of(value: Option<&JsData>) -> Result<Option<ConfigurationScopeArg>, String> {
    match value {
        None | Some(JsData::Undefined) => Ok(None),
        Some(JsData::Null) => Ok(Some(ConfigurationScopeArg::Null)),
        Some(object @ JsData::Object(_)) => {
            if class_name(object) == Some("Uri") {
                return Ok(Some(ConfigurationScopeArg::Uri(uri_from_js(object)?)));
            }
            let language_id = opt_string(field(object, "languageId"));
            match (field(object, "uri"), language_id) {
                (Some(uri), Some(language_id)) => Ok(Some(ConfigurationScopeArg::ResourceLanguage { uri: uri_from_js(uri)?, language_id })),
                (Some(uri), None) => {
                    let uri = uri_from_js(uri)?;
                    match opt_number(field(object, "index")) {
                        Some(index) => {
                            let name = opt_string(field(object, "name")).unwrap_or_default();
                            Ok(Some(ConfigurationScopeArg::WorkspaceFolder(WorkspaceFolder::new(uri, name, index as usize))))
                        }
                        None => Ok(Some(ConfigurationScopeArg::ResourceLanguage { uri, language_id: String::new() })),
                    }
                }
                (None, Some(language_id)) => Ok(Some(ConfigurationScopeArg::Language { language_id })),
                (None, None) => Err("getConfiguration: o escopo precisa ser Uri, TextDocument, WorkspaceFolder ou { languageId }".to_owned()),
            }
        }
        Some(_) => Err("getConfiguration: o escopo precisa ser Uri, TextDocument, WorkspaceFolder ou { languageId }".to_owned()),
    }
}

/// O alvo de `update`: `ConfigurationTarget`, booleano, `null` ou `undefined`.
fn target_of(value: Option<&JsData>) -> Result<Option<ConfigurationTargetArg>, String> {
    match value {
        None | Some(JsData::Undefined) | Some(JsData::Null) => Ok(None),
        Some(JsData::Bool(flag)) => Ok(Some(ConfigurationTargetArg::Bool(*flag))),
        Some(JsData::Num(number)) => ExtHostConfigurationTarget::from_value(*number as i32)
            .map(|target| Some(ConfigurationTargetArg::Target(target)))
            .ok_or_else(|| format!("update: ConfigurationTarget inválido ({number})")),
        Some(_) => Err("update: o alvo precisa ser ConfigurationTarget ou booleano".to_owned()),
    }
}

/// Um `vscode.GlobPattern` (`string | RelativePattern`) do motor.
pub(crate) fn glob_of(value: &JsData) -> Result<GlobPattern, String> {
    match value {
        JsData::Str(pattern) => Ok(GlobPattern::Pattern(pattern.clone())),
        JsData::Object(_) => {
            let pattern = string_arg(&[field(value, "pattern").cloned().unwrap_or(JsData::Undefined)], 0, "RelativePattern.pattern")?;
            let base = field(value, "baseUri").ok_or("o padrão relativo precisa de baseUri")?;
            Ok(GlobPattern::Relative(RelativePattern::new(uri_from_js(base)?, &pattern)))
        }
        _ => Err("o padrão precisa ser texto ou RelativePattern".to_owned()),
    }
}

/// O `WorkspaceConfiguration` como objeto: os valores, e por cima `has`, `get`, `update` e `inspect`.
fn configuration_to_js(runner: &NodeRunner, config: WorkspaceConfiguration) -> JsData {
    let config = Rc::new(config);
    let JsData::Object(mut fields) = dynamic_to_js(&config.to_dynamic()) else { unreachable!("to_dynamic devolve um objeto") };
    fields.retain(|(name, _)| !matches!(name.as_str(), "has" | "get" | "update" | "inspect"));
    let member = |name: &str, length: u32, body: Rc<dyn Fn(&[JsData]) -> Result<JsData, String>>| (name.to_owned(), JsData::Handle(Rc::new(runner.make_function(name, length, body))));
    let has = config.clone();
    let get = config.clone();
    let update = config.clone();
    let inspect = config;
    let update_runner = runner.clone();
    fields.push(member("has", 1, Rc::new(move |args| Ok(JsData::Bool(has.has(&string_arg(args, 0, "has: a chave")?))))));
    fields.push(member(
        "get",
        2,
        Rc::new(move |args| {
            let key = string_arg(args, 0, "get: a chave")?;
            Ok(dynamic_to_js(&get.get(&key, args.get(1).map(js_to_dynamic))))
        }),
    ));
    fields.push(member(
        "update",
        4,
        Rc::new(move |args| {
            let key = string_arg(args, 0, "update: a chave")?;
            let value = args.get(1).map_or(je_base::dynamic::Dynamic::Undefined, js_to_dynamic);
            let target = target_of(args.get(2))?;
            let in_language = match args.get(3) {
                Some(JsData::Bool(flag)) => Some(*flag),
                _ => None,
            };
            Ok(promise_to_js(&update_runner, update.update(&key, value, target, in_language), |()| JsData::Undefined))
        }),
    ));
    fields.push(member(
        "inspect",
        1,
        Rc::new(move |args| {
            let key = string_arg(args, 0, "inspect: a chave")?;
            Ok(inspect.inspect(&key).map_or(JsData::Undefined, |inspect| dynamic_to_js(&inspect.to_dynamic())))
        }),
    ));
    JsData::Object(fields)
}

/// O evento `onDidChangeConfiguration` do provedor, lido na inscrição (ver as divergências do módulo).
fn configuration_change_event(configuration: &Rc<ExtHostConfiguration>) -> Event<ExtHostConfigurationChangeEvent> {
    let configuration = configuration.clone();
    Event::new(move |listener: Listener<ExtHostConfigurationChangeEvent>| -> DisposableRef {
        match configuration.initialized_provider() {
            Some(provider) => provider.on_did_change_configuration().listen_rc(listener),
            None => to_disposable(|| {}),
        }
    })
}

/// O objeto do evento de configuração: `{ affectsConfiguration(section, scope?) }`.
fn configuration_change_to_js(runner: &NodeRunner, event: &ExtHostConfigurationChangeEvent) -> JsData {
    let event = event.clone();
    let function = runner.make_function(
        "affectsConfiguration",
        2,
        Rc::new(move |args| {
            let section = string_arg(args, 0, "affectsConfiguration: a seção")?;
            let scope = scope_of(args.get(1))?;
            Ok(JsData::Bool(event.affects_configuration(&section, scope.as_ref())))
        }),
    );
    JsData::Object(vec![("affectsConfiguration".to_owned(), JsData::Handle(Rc::new(function)))])
}

/// O namespace `workspace`.
pub fn namespace(runner: &NodeRunner, services: &WorkspaceServices, table: &Rc<DisposableTable>, events: &Rc<EventTables>) -> JsData {
    let mut methods: Vec<(String, u32, MethodCallback)> = Vec::new();
    let mut getters: Vec<(String, MethodCallback)> = Vec::new();
    if let Some((workspace, configuration)) = &services.workspace {

        {
            let workspace = workspace.clone();
            methods.push((
                "getWorkspaceFolder".to_owned(),
                1,
                Rc::new(move |_, args| {
                    let uri = uri_from_js(args.first().unwrap_or(&JsData::Undefined))?;
                    Ok(workspace.get_workspace_folder(&uri, None).map_or(JsData::Undefined, |folder| folder_to_js(&folder)))
                }),
            ));
        }
        {
            let workspace = workspace.clone();
            methods.push((
                "asRelativePath".to_owned(),
                2,
                Rc::new(move |_, args| {
                    let input = match args.first() {
                        Some(JsData::Str(path)) => RelativePathInput::Path(path.clone()),
                        Some(uri @ JsData::Object(_)) => RelativePathInput::Uri(uri_from_js(uri)?),
                        _ => RelativePathInput::Undefined,
                    };
                    let include_workspace = match args.get(1) {
                        Some(JsData::Bool(flag)) => Some(*flag),
                        _ => None,
                    };
                    Ok(JsData::Str(workspace.get_relative_path(input, include_workspace)))
                }),
            ));
        }
        {
            let configuration = configuration.clone();
            let config_runner = runner.clone();
            methods.push((
                "getConfiguration".to_owned(),
                2,
                Rc::new(move |_, args| {
                    let section = opt_string(args.first());
                    let scope = if args.len() == 1 { None } else { scope_of(args.get(1))? };
                    let provider = configuration.initialized_provider().ok_or("getConfiguration: a configuração ainda não foi inicializada")?;
                    Ok(configuration_to_js(&config_runner, provider.get_configuration(section.as_deref(), scope.as_ref(), None)))
                }),
            ));
        }
        {
            let (workspace, find_events, find_runner) = (workspace.clone(), events.clone(), runner.clone());
            methods.push((
                "findFiles".to_owned(),
                4,
                Rc::new(move |_, args| {
                    let include = if is_nullish(args.first()) { None } else { Some(glob_of(args.first().expect("não é nulo"))?) };
                    let exclude = match args.get(1) {
                        None | Some(JsData::Undefined) => FindFilesExclude::Undefined,
                        Some(JsData::Null) => FindFilesExclude::Null,
                        Some(pattern) => FindFilesExclude::Pattern(glob_of(pattern)?),
                    };
                    let max_results = opt_number(args.get(2)).map(|count| count as i64);
                    let token = find_events.token_arg(args.get(3));
                    let found = workspace.find_files(include, exclude, max_results, &ExtensionIdentifier::new(HOST_EXTENSION), token);
                    Ok(promise_to_js(&find_runner, found, |uris| JsData::Array(uris.iter().map(uri_to_js).collect())))
                }),
            ));
        }

        {
            let workspace = workspace.clone();
            getters.push(("name".to_owned(), Rc::new(move |_, _| Ok(workspace.name().map_or(JsData::Undefined, JsData::Str)))));
        }
        {
            let workspace = workspace.clone();
            getters.push(("rootPath".to_owned(), Rc::new(move |_, _| Ok(workspace.get_path().map_or(JsData::Undefined, JsData::Str)))));
        }
        {
            let workspace = workspace.clone();
            getters.push((
                "workspaceFolders".to_owned(),
                Rc::new(move |_, _| Ok(workspace.get_workspace_folders().map_or(JsData::Undefined, |folders| folders_to_js(&folders)))),
            ));
        }
        {
            let workspace = workspace.clone();
            getters.push(("workspaceFile".to_owned(), Rc::new(move |_, _| Ok(workspace.workspace_file().map_or(JsData::Undefined, |uri| uri_to_js(&uri))))));
        }
        {
            let workspace = workspace.clone();
            getters.push(("isTrusted".to_owned(), Rc::new(move |_, _| Ok(JsData::Bool(workspace.trusted())))));
        }
        {
            let (workspace, runner, table) = (workspace.clone(), runner.clone(), table.clone());
            getters.push((
                "onDidChangeWorkspaceFolders".to_owned(),
                Rc::new(move |_, _| {
                    Ok(event_function(&runner, &table, "onDidChangeWorkspaceFolders", workspace.on_did_change_workspace(), |event| {
                        JsData::Object(vec![("added".to_owned(), folders_to_js(&event.added)), ("removed".to_owned(), folders_to_js(&event.removed))])
                    }))
                }),
            ));
        }
        {
            let (workspace, runner, table) = (workspace.clone(), runner.clone(), table.clone());
            getters.push((
                "onDidGrantWorkspaceTrust".to_owned(),
                Rc::new(move |_, _| Ok(event_function(&runner, &table, "onDidGrantWorkspaceTrust", workspace.on_did_grant_workspace_trust(), |()| JsData::Undefined))),
            ));
        }
        {
            let (configuration, event_runner, table) = (configuration.clone(), runner.clone(), table.clone());
            getters.push((
                "onDidChangeConfiguration".to_owned(),
                Rc::new(move |_, _| {
                    let converter = event_runner.clone();
                    Ok(event_function(&event_runner, &table, "onDidChangeConfiguration", configuration_change_event(&configuration), move |event| {
                        configuration_change_to_js(&converter, event)
                    }))
                }),
            ));
        }
    }
    if let Some(text_editors) = &services.text_editors {
        let (document_methods, document_getters) = text_editors::workspace_members(runner, text_editors, table);
        methods.extend(document_methods);
        getters.extend(document_getters);
    }
    if let Some((bulk_edits, objects)) = &services.bulk_edits {
        methods.extend(workspace_edit::workspace_members(runner, bulk_edits, objects));
    }
    if let Some(api) = &services.file_system_events {
        let (file_methods, file_getters) = file_system_events::workspace_members(runner, api, table);
        methods.extend(file_methods);
        getters.extend(file_getters);
    }
    if let Some(api) = &services.notebooks {
        let (notebook_methods, notebook_getters) = notebooks::workspace_members(runner, api, table);
        methods.extend(notebook_methods);
        getters.extend(notebook_getters);
    }
    // `registerFileSystemProvider` recebe o provedor da extensão: uma função por referência, própria do namespace.
    let mut references: Vec<(String, JsData)> = Vec::new();
    if let Some(api) = &services.file_system {
        let members = file_system::workspace_members(runner, api);
        getters.extend(members.getters);
        references.extend(members.references);
    }
    match accessor_namespace(runner, "workspace", methods, getters) {
        JsData::Object(mut fields) => {
            fields.extend(references);
            JsData::Object(fields)
        }
        other => other,
    }
}

/// `vscode.RelativePattern`: `new RelativePattern(base, pattern)` com `base` sendo `WorkspaceFolder`, `Uri` ou texto.
pub fn classes(runner: &NodeRunner) -> Vec<(String, JsData)> {
    let constructor = runner.define_class(ClassSpec {
        name: "RelativePattern".to_owned(),
        length: 2,
        construct: Rc::new(|args| {
            let base = match args.first() {
                Some(JsData::Str(path)) => RelativePatternBase::Path(path.clone()),
                Some(uri @ JsData::Object(_)) if class_name(uri) == Some("Uri") => RelativePatternBase::Uri(uri_from_js(uri)?),
                Some(folder @ JsData::Object(_)) => match field(folder, "uri") {
                    Some(uri) => RelativePatternBase::Uri(uri_from_js(uri)?),
                    None => return Err("RelativePattern: a base precisa ser WorkspaceFolder, Uri ou texto".to_owned()),
                },
                _ => return Err("RelativePattern: a base precisa ser WorkspaceFolder, Uri ou texto".to_owned()),
            };
            let pattern = string_arg(args, 1, "RelativePattern: o padrão")?;
            let relative = RelativePattern::new(base, &pattern);
            Ok(JsData::Object(vec![
                ("pattern".to_owned(), JsData::Str(relative.pattern.clone())),
                ("base".to_owned(), JsData::Str(relative.base().to_owned())),
                ("baseUri".to_owned(), uri_to_js(relative.base_uri())),
            ]))
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    vec![("RelativePattern".to_owned(), JsData::Handle(Rc::new(constructor)))]
}
