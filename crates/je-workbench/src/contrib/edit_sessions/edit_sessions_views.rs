//! Porte de `src/vs/workbench/contrib/editSessions/browser/editSessionsViews.ts`: a vista Cloud Changes (a árvore das edit sessions
//! guardadas na nuvem, com as pastas e os arquivos de cada uma), as ações da vista (retomar, guardar, apagar e apagar tudo) e o
//! provedor de dados da árvore.
//!
//! Divergências:
//! - **Injeção.** `@IInstantiationService` e os serviços do `EditSessionDataViewDataProvider` são os campos de
//!   [`EditSessionsViewsServices`]; o `createInstance(TreeView, ...)` é o [`CustomTreeView::new_without_extension`] com os serviços
//!   da árvore em `tree_view_services`, e o `createInstance(EditSessionDataViewDataProvider)` é o construtor do provedor. As ações
//!   leem `ICommandService`, `IDialogService` e `IEditSessionsStorageService` do `accessor`, como no original, antes do primeiro
//!   `await`.
//! - **`TreeViewPane` e `ViewPaneContainer`** (camada de interface, ainda não portada) são o `ctor_descriptor` ausente do
//!   descritor da vista e do contêiner (ver `common::views`). `treeView.showRefreshAction = true` não existe: o `CustomTreeView`
//!   do porte não tem o estado do botão de atualizar (ele é da barra de título do `TreeViewPane`).
//! - **`ITreeViewDescriptor`** é o campo `tree_view` do `IViewDescriptor`. `ITreeItem` e `TreeViewItemHandleArg` (o objeto com
//!   `$treeItemHandle` que as ações recebem) são os de `common::views`, este último como [`Dynamic`].
//! - **`Promise.all(changes.map(async ...))`** de `getEditSessionFolderContents` roda em sequência (o resultado, na ordem das
//!   mudanças, é o mesmo; só o erro de uma mudança posterior deixa de ser visto antes do de uma anterior).
//! - **`registerAction2(class extends Action2 ...)`** são os [`FnAction`] (ver `common`). A promessa que o `run` assíncrono do
//!   original devolve é disparada e descartada, porque o `Action2::run` do porte não devolve valor.
//! - **`API_OPEN_EDITOR_COMMAND_ID`** (`editorCommands.ts`, ainda não portado) é o de `timeline_pane`, com o mesmo valor.
//! - **`fromNow`, `createCommandUri` e `basename`** (de `base/common/path`) são `from_now`, `create_command_uri` e o `basename` do
//!   módulo de caminho nativo; os URIs que vão como argumento do comando saem como `Uri` (`ApiValue`), como os do `ITreeItem`.

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::date::from_now;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::html_content::create_command_uri;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::{LocalizedString as NlsLocalizedString, localize};
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents};
use je_editor::languages::StringOrMarkdown;
use je_platform::action::{CommandAction, Icon, Title};
use je_platform::actions::actions::{Action2MenuItem, Action2Options, MenuId};
use je_platform::actions::menu_ids;
use je_platform::commands::I_COMMAND_SERVICE;
use je_platform::contextkey::contextkey::{ContextKeyMeta, ContextRegExp, RawContextKey};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKey, IContextKeyService};
use je_platform::dialogs::{BaseDialogOptions, Confirmation, DialogDetail, DialogType, DialogTypeOrSeverity, I_DIALOG_SERVICE};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{host_environment, join_path};
use je_platform::workspace::IWorkspaceContextService;

use super::common::{
    ChangeType, EDIT_SESSIONS_DATA_VIEW_ID, EDIT_SESSIONS_SCHEME, EditSession, FnAction, IEditSessionsStorageService, I_EDIT_SESSIONS_STORAGE_SERVICE, SyncResource, action2_options,
    edit_sessions_show_view, edit_sessions_title,
};
use crate::api::common::ext_host_protocol::ICommandDto;
use crate::api::common::ext_host_values::ApiValue;
use crate::browser::parts::views::tree_view::{CustomTreeView, CustomTreeViewServices};
use crate::common::tree_view_contracts::{ITreeView, ITreeViewDataProvider};
use crate::common::views::{
    ITreeItem, ITreeItemLabel, IViewContentDescriptor, IViewDescriptor, TreeCommand, TreeItemCollapsibleState, TreeItemDescription, ViewContainerRc, ViewContentWhen, views_registry,
};
use crate::contrib::timeline::browser::timeline_pane::API_OPEN_EDITOR_COMMAND_ID;

const EDIT_SESSIONS_COUNT_KEY: &str = "editSessionsCount";

/// `EDIT_SESSIONS_COUNT_CONTEXT_KEY`.
fn edit_sessions_count_context_key() -> RawContextKey {
    thread_local! { static KEY: RawContextKey = RawContextKey::new(EDIT_SESSIONS_COUNT_KEY, 0.0, ContextKeyMeta::None); }
    KEY.with(Clone::clone)
}

const RESUME_ACTION_ID: &str = "workbench.editSessions.actions.resume";
const STORE_ACTION_ID: &str = "workbench.editSessions.actions.store";
const DELETE_ACTION_ID: &str = "workbench.editSessions.actions.delete";
const DELETE_ALL_ACTION_ID: &str = "workbench.editSessions.actions.deleteAll";
const RESUME_LATEST_ACTION_ID: &str = "workbench.editSessions.actions.resumeLatest";
const STORE_CURRENT_ACTION_ID: &str = "workbench.editSessions.actions.storeCurrent";
const REMOTE_SESSION_CONTENT_AUTHORITY: &str = "remote-session-content";

/// `EDIT_SESSIONS_TITLE` como o `ILocalizedString` de `base/common/nls` que o nome da vista e o título do contêiner levam.
pub fn edit_sessions_nls_title() -> NlsLocalizedString {
    let title = edit_sessions_title();
    NlsLocalizedString { original: title.original, value: title.value }
}

/// `str.substring(1)`.
fn substring_from_one(text: &str) -> String {
    text.chars().skip(1).collect()
}

/// `URI.parse(handle.$treeItemHandle).path.substring(1)`.
fn edit_session_id_of(handle: Option<&Dynamic>) -> Result<String, Error> {
    match handle.map(|handle| handle.get_str("$treeItemHandle")) {
        Some(Dynamic::String(item_handle)) => Ok(substring_from_one(&URI::parse(item_handle, false).path().to_string_lossy())),
        _ => Err(Error::named("TypeError", "Cannot read properties of undefined (reading '$treeItemHandle')")),
    }
}

/// `URI.from({ scheme: EDIT_SESSIONS_SCHEME, authority: 'remote-session-content', path })`.
fn remote_session_content_uri(path: String) -> URI {
    URI::from(
        &UriComponents { scheme: Some(JsString::from(EDIT_SESSIONS_SCHEME)), authority: Some(JsString::from(REMOTE_SESSION_CONTENT_AUTHORITY)), path: Some(JsString::from(path)), ..UriComponents::default() },
        false,
    )
}

/// O `when` das ações de item: `view == viewId && viewItem =~ /edit-session/i`.
fn edit_session_item_when(view_id: &str) -> Option<ContextKeyExpression> {
    let regex = ContextRegExp::new("edit-session", "i").unwrap_or_else(|_| errors::throw(Error::named("SyntaxError", "Invalid regular expression: /edit-session/i")));
    ContextKeyExpr::and([ContextKeyExpr::equals("view", view_id), ContextKeyExpr::regex("viewItem", regex)])
}

fn menu_item(id: MenuId, when: Option<ContextKeyExpression>, group: Option<&str>) -> Action2MenuItem {
    Action2MenuItem { id, null_precondition: false, alt: None, when, group: group.map(str::to_string), order: None, is_hidden_by_default: None }
}

fn theme_icon_command(id: &str, title: String, icon: &str) -> CommandAction {
    CommandAction { icon: Some(Icon::Theme(ThemeIcon::from_id(icon))), ..CommandAction::new(id, Title::Plain(title)) }
}

/// Os serviços que o `EditSessionsDataViews` e o `EditSessionDataViewDataProvider` recebem por injeção.
#[derive(Clone)]
pub struct EditSessionsViewsServices {
    pub edit_sessions_storage_service: Rc<dyn IEditSessionsStorageService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub workspace_context_service: Rc<dyn IWorkspaceContextService>,
    pub file_service: Rc<dyn IFileService>,
    /// Os serviços do `TreeView` que o `createInstance(TreeView, ...)` resolve.
    pub tree_view_services: CustomTreeViewServices,
}

/// `EditSessionsDataViews`.
pub struct EditSessionsDataViews {
    base: Rc<Disposable>,
}

impl IDisposable for EditSessionsDataViews {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditSessionsDataViews {
    pub fn new(container: &ViewContainerRc, services: EditSessionsViewsServices) -> Rc<Self> {
        let this = Rc::new(Self { base: Disposable::new() });
        this.register_views(container, services);
        this
    }

    fn register_views(&self, container: &ViewContainerRc, services: EditSessionsViewsServices) {
        let view_id = EDIT_SESSIONS_DATA_VIEW_ID;
        let tree_view = CustomTreeView::new_without_extension(view_id, &edit_sessions_title().value, services.tree_view_services.clone());
        tree_view.set_show_collapse_all_action(true);
        tree_view.set_data_provider(Some(EditSessionDataViewDataProvider::new(&services)));

        let views_registry = views_registry();
        let tree_view_for_descriptor: Rc<dyn ITreeView> = tree_view.clone();
        views_registry.register_views(
            vec![Rc::new(IViewDescriptor {
                can_toggle_visibility: true,
                can_move_view: false,
                tree_view: Some(tree_view_for_descriptor),
                collapsed: false,
                when: ContextKeyExpr::and([edit_sessions_show_view().expr()]),
                order: Some(100.0),
                hide_by_default: true,
                ..IViewDescriptor::new(view_id, edit_sessions_nls_title())
            })],
            container,
        );

        let store_command_units: Vec<u16> = STORE_ACTION_ID.encode_utf16().collect();
        let store_command_uri = create_command_uri(&store_command_units, &[]).to_string(false).to_string_lossy();
        views_registry.register_view_welcome_content(
            view_id,
            IViewContentDescriptor {
                when: Some(ViewContentWhen::Expression(ContextKeyExpr::equals(EDIT_SESSIONS_COUNT_KEY, 0.0))),
                order: Some(1.0),
                ..IViewContentDescriptor::new(&localize(
                    "noStoredChanges",
                    "You have no stored changes in the cloud to display.\n{0}",
                    &[&format!("[{}]({store_command_uri})", localize("storeWorkingChangesTitle", "Store Working Changes", &[]))],
                ))
            },
        );

        self.register_resume_action(view_id, &tree_view);
        self.register_store_action(&tree_view);
        self.register_delete_action(view_id, &tree_view);
        self.register_delete_all_action(view_id, &tree_view);
    }

    fn register_resume_action(&self, view_id: &str, tree_view: &Rc<CustomTreeView>) {
        let tree_view = tree_view.clone();
        self.base.register(FnAction::register(
            Action2Options {
                menu: vec![menu_item(menu_ids::view_item_context(), edit_session_item_when(view_id), Some("inline"))],
                ..action2_options(theme_icon_command(RESUME_ACTION_ID, localize("workbench.editSessions.actions.resume.v2", "Resume Working Changes", &[]), "desktop-download"), false)
            },
            move |accessor, args| {
                let command_service = accessor.get(I_COMMAND_SERVICE);
                let handle = args.first().cloned();
                let tree_view = tree_view.clone();
                let _ = async_fn(async move {
                    let edit_session_id = edit_session_id_of(handle.as_ref())?;
                    command_service.execute_command(RESUME_LATEST_ACTION_ID, vec![Dynamic::string(&edit_session_id), Dynamic::Bool(true)]).await?;
                    tree_view.refresh(None, None).await?;
                    Ok(())
                });
            },
        ));
    }

    fn register_store_action(&self, tree_view: &Rc<CustomTreeView>) {
        let tree_view = tree_view.clone();
        self.base.register(FnAction::register(
            action2_options(theme_icon_command(STORE_ACTION_ID, localize("workbench.editSessions.actions.store.v2", "Store Working Changes", &[]), "cloud-upload"), false),
            move |accessor, _args| {
                let command_service = accessor.get(I_COMMAND_SERVICE);
                let tree_view = tree_view.clone();
                let _ = async_fn(async move {
                    command_service.execute_command(STORE_CURRENT_ACTION_ID, Vec::new()).await?;
                    tree_view.refresh(None, None).await?;
                    Ok(())
                });
            },
        ));
    }

    fn register_delete_action(&self, view_id: &str, tree_view: &Rc<CustomTreeView>) {
        let tree_view = tree_view.clone();
        self.base.register(FnAction::register(
            Action2Options {
                menu: vec![menu_item(menu_ids::view_item_context(), edit_session_item_when(view_id), Some("inline"))],
                ..action2_options(theme_icon_command(DELETE_ACTION_ID, localize("workbench.editSessions.actions.delete.v2", "Delete Working Changes", &[]), "trash"), false)
            },
            move |accessor, args| {
                let handle = args.first().cloned();
                let dialog_service = accessor.get(I_DIALOG_SERVICE);
                let edit_sessions_storage_service = accessor.get(I_EDIT_SESSIONS_STORAGE_SERVICE);
                let tree_view = tree_view.clone();
                let _ = async_fn(async move {
                    let edit_session_id = edit_session_id_of(handle.as_ref())?;
                    let result = dialog_service
                        .confirm(Confirmation {
                            base: BaseDialogOptions {
                                dialog_type: Some(DialogTypeOrSeverity::Type(DialogType::Warning)),
                                title: Some(edit_sessions_title().value),
                                detail: Some(DialogDetail::Text(localize("confirm delete detail.v2", " You cannot undo this action.", &[]))),
                                ..BaseDialogOptions::new(localize(
                                    "confirm delete.v2",
                                    "Are you sure you want to permanently delete your working changes with ref {0}?",
                                    &[&edit_session_id],
                                ))
                            },
                            primary_button: None,
                            cancel_button: None,
                        })
                        .await?;
                    if result.confirmed {
                        edit_sessions_storage_service.delete(SyncResource::EditSessions, Some(edit_session_id)).await?;
                        tree_view.refresh(None, None).await?;
                    }
                    Ok(())
                });
            },
        ));
    }

    fn register_delete_all_action(&self, view_id: &str, tree_view: &Rc<CustomTreeView>) {
        let tree_view = tree_view.clone();
        let when = ContextKeyExpr::and([ContextKeyExpr::equals("view", view_id), ContextKeyExpr::greater(EDIT_SESSIONS_COUNT_KEY, 0.0)]);
        self.base.register(FnAction::register(
            Action2Options {
                menu: vec![menu_item(menu_ids::view_title(), when, None)],
                ..action2_options(theme_icon_command(DELETE_ALL_ACTION_ID, localize("workbench.editSessions.actions.deleteAll", "Delete All Working Changes from Cloud", &[]), "trash"), false)
            },
            move |accessor, _args| {
                let dialog_service = accessor.get(I_DIALOG_SERVICE);
                let edit_sessions_storage_service = accessor.get(I_EDIT_SESSIONS_STORAGE_SERVICE);
                let tree_view = tree_view.clone();
                let _ = async_fn(async move {
                    let result = dialog_service
                        .confirm(Confirmation {
                            base: BaseDialogOptions {
                                dialog_type: Some(DialogTypeOrSeverity::Type(DialogType::Warning)),
                                title: Some(edit_sessions_title().value),
                                detail: Some(DialogDetail::Text(localize("confirm delete all detail", " You cannot undo this action.", &[]))),
                                ..BaseDialogOptions::new(localize("confirm delete all", "Are you sure you want to permanently delete all stored changes from the cloud?", &[]))
                            },
                            primary_button: None,
                            cancel_button: None,
                        })
                        .await?;
                    if result.confirmed {
                        edit_sessions_storage_service.delete(SyncResource::EditSessions, None).await?;
                        tree_view.refresh(None, None).await?;
                    }
                    Ok(())
                });
            },
        ));
    }
}

/// `EditSessionDataViewDataProvider`.
#[derive(Clone)]
struct EditSessionDataViewDataProvider {
    edit_sessions_storage_service: Rc<dyn IEditSessionsStorageService>,
    edit_sessions_count: Rc<dyn IContextKey>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    file_service: Rc<dyn IFileService>,
}

impl EditSessionDataViewDataProvider {
    fn new(services: &EditSessionsViewsServices) -> Rc<Self> {
        Rc::new(Self {
            edit_sessions_storage_service: services.edit_sessions_storage_service.clone(),
            edit_sessions_count: edit_sessions_count_context_key().bind_to(services.context_key_service.as_ref()),
            workspace_context_service: services.workspace_context_service.clone(),
            file_service: services.file_service.clone(),
        })
    }

    async fn get_all_edit_sessions(&self) -> Result<Vec<ITreeItem>, Error> {
        let all_edit_sessions = self.edit_sessions_storage_service.list(SyncResource::EditSessions).await?;
        self.edit_sessions_count.set(Dynamic::Number(all_edit_sessions.len() as f64));
        let mut edit_sessions = Vec::new();

        for session in all_edit_sessions {
            let resource = remote_session_content_uri(format!("/{}", session.r#ref));
            let Some(session_data) = self.edit_sessions_storage_service.read(SyncResource::EditSessions, Some(session.r#ref.clone())).await? else {
                continue;
            };
            let content = EditSession::parse(&session_data.content)?;
            let label = content.folders.iter().map(|folder| folder.name.as_str()).collect::<Vec<_>>().join(", ");
            let machine_name = match content.machine.as_deref().filter(|machine_id| !machine_id.is_empty()) {
                Some(machine_id) => self.edit_sessions_storage_service.get_machine_by_id(machine_id).await?,
                None => None,
            };
            let created = from_now(Some(session.created), true, false, false);
            let description = match machine_name {
                None => created,
                Some(machine_name) => format!("{created}\u{a0}\u{a0}\u{2022}\u{a0}\u{a0}{machine_name}"),
            };

            edit_sessions.push(ITreeItem {
                handle: resource.to_string(false).to_string_lossy(),
                collapsible_state: TreeItemCollapsibleState::Collapsed,
                label: Some(plain_label(label)),
                description: Some(TreeItemDescription::Text(description)),
                theme_icon: Some(ThemeIcon::from_id("repo")),
                context_value: Some("edit-session".to_string()),
                ..ITreeItem::default()
            });
        }

        Ok(edit_sessions)
    }

    async fn get_edit_session(&self, r#ref: &str) -> Result<Vec<ITreeItem>, Error> {
        let Some(data) = self.edit_sessions_storage_service.read(SyncResource::EditSessions, Some(r#ref.to_string())).await? else {
            return Ok(Vec::new());
        };
        let content = EditSession::parse(&data.content)?;

        if content.folders.len() == 1 {
            return self.get_edit_session_folder_contents(r#ref, &content.folders[0].name).await;
        }

        Ok(content
            .folders
            .iter()
            .map(|folder| ITreeItem {
                handle: remote_session_content_uri(format!("/{}/{}", data.r#ref, folder.name)).to_string(false).to_string_lossy(),
                collapsible_state: TreeItemCollapsibleState::Collapsed,
                label: Some(plain_label(folder.name.clone())),
                theme_icon: Some(ThemeIcon::from_id("folder")),
                ..ITreeItem::default()
            })
            .collect())
    }

    async fn get_edit_session_folder_contents(&self, r#ref: &str, folder_name: &str) -> Result<Vec<ITreeItem>, Error> {
        let Some(data) = self.edit_sessions_storage_service.read(SyncResource::EditSessions, Some(r#ref.to_string())).await? else {
            return Ok(Vec::new());
        };
        let content = EditSession::parse(&data.content)?;

        let current_workspace_folder = self.workspace_context_service.get_workspace().folders().into_iter().find(|folder| folder.name == folder_name);
        let Some(edit_session_folder) = content.folders.iter().find(|folder| folder.name == folder_name) else {
            return Ok(Vec::new());
        };

        let mut items = Vec::new();
        for change in &edit_session_folder.working_changes {
            let relative_file_path = change.relative_file_path();
            let cloud_change_uri = remote_session_content_uri(format!("/{}/{folder_name}/{relative_file_path}", data.r#ref));

            if let Some(current_workspace_folder) = &current_workspace_folder {
                // find the corresponding file in the workspace
                let local_copy = join_path(&current_workspace_folder.uri, &[relative_file_path]);
                if change.change_type() == ChangeType::Addition && self.file_service.exists(&local_copy).await? {
                    let title = format!(
                        "{} ({} \u{2194} {})",
                        PathModule::native(host_environment().as_ref()).basename(relative_file_path, None),
                        localize("local copy", "Local Copy", &[]),
                        localize("cloud changes", "Cloud Changes", &[])
                    );
                    items.push(file_item(
                        &cloud_change_uri,
                        relative_file_path,
                        "vscode.diff",
                        localize("compare changes", "Compare Changes", &[]),
                        vec![local_copy.to_dynamic(), cloud_change_uri.to_dynamic(), Dynamic::string(&title), Dynamic::Undefined],
                    ));
                    continue;
                }
            }

            items.push(file_item(
                &cloud_change_uri,
                relative_file_path,
                API_OPEN_EDITOR_COMMAND_ID,
                localize("open file", "Open File", &[]),
                vec![cloud_change_uri.to_dynamic(), Dynamic::Undefined, Dynamic::Undefined],
            ));
        }
        Ok(items)
    }
}

/// `{ label }`.
fn plain_label(label: String) -> ITreeItemLabel {
    ITreeItemLabel { label: StringOrMarkdown::String(label), highlights: None }
}

/// A verdade do JavaScript (`ref && ...`) de um pedaço do caminho: presente e não vazio.
fn non_empty(part: Option<&str>) -> Option<&str> {
    part.filter(|part| !part.is_empty())
}

/// O item de arquivo da árvore, com o comando que o abre.
fn file_item(cloud_change_uri: &URI, relative_file_path: &str, command_id: &str, title: String, arguments: Vec<Dynamic>) -> ITreeItem {
    ITreeItem {
        handle: cloud_change_uri.to_string(false).to_string_lossy(),
        resource_uri: Some(cloud_change_uri.clone()),
        collapsible_state: TreeItemCollapsibleState::None,
        label: Some(plain_label(relative_file_path.to_string())),
        theme_icon: Some(ThemeIcon::from_id("file")),
        command: Some(TreeCommand { command: ICommandDto { ident: None, id: command_id.to_string(), title, tooltip: None, arguments: Some(arguments) }, original_id: None }),
        ..ITreeItem::default()
    }
}

impl ITreeViewDataProvider for EditSessionDataViewDataProvider {
    fn get_children(&self, element: Option<&ITreeItem>) -> Promise<Option<Vec<ITreeItem>>> {
        let this = self.clone();
        let handle = element.map(|element| element.handle.clone());
        async_fn(async move {
            let Some(handle) = handle else {
                return Ok(Some(this.get_all_edit_sessions().await?));
            };

            let path = substring_from_one(&URI::parse(handle.as_str(), false).path().to_string_lossy());
            let mut parts = path.split('/');
            let (r#ref, folder_name, file_path) = (parts.next(), parts.next(), parts.next());

            match (non_empty(r#ref), non_empty(folder_name), non_empty(file_path)) {
                (Some(r#ref), None, _) => Ok(Some(this.get_edit_session(r#ref).await?)),
                (Some(r#ref), Some(folder_name), None) => Ok(Some(this.get_edit_session_folder_contents(r#ref, folder_name).await?)),
                _ => Ok(Some(Vec::new())),
            }
        })
    }
}
