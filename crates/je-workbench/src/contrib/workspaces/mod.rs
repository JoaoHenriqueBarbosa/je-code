//! Porte de `src/vs/workbench/contrib/workspaces/browser/workspaces.contribution.ts`: o `WorkspacesFinderContribution`, que
//! procura arquivos `.code-workspace` na raiz da pasta aberta e sugere abrir um deles.
//!
//! Divergências:
//! - `extends Disposable` vira um [`Disposable`] por dentro; o construtor devolve `Rc<Self>`. Os serviços são parâmetros do
//!   construtor, na ordem do original.
//! - `findWorkspaces` é `async` e roda como uma cadeia de `then`; o `this.doHandleWorkspaceFiles` só usa os serviços, então é
//!   a função [`do_handle_workspace_files`] sobre os serviços já clonados.
//! - O `run` de uma escolha do prompt é `Rc<dyn Fn()>` e descarta a promessa do `openWindow` (o original a devolve).
//! - `IHostService.openWindow([{ workspaceUri }])` é `open_window(vec![WindowOpenable::Workspace { .. }], None)`.
//! - O registro da contribuição no `WorkbenchContributionsRegistry` (fase `Eventually`), a ação
//!   `workbench.action.openWorkspaceFromEditor` e o `recentRemoteFolderPruner` dependem de módulos que ainda não foram
//!   portados e entram com cada um deles.

pub mod recent_remote_folder_pruner;

use std::rc::Rc;

use je_base::JsString;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_platform::files::i_file_service::IFileService;
use je_platform::notification::notification_message::{NotificationPart, NotificationText};
use je_platform::notification::{
    BasePromptChoice, INotificationService, NeverShowAgainOptions, NeverShowAgainScope, NotificationPriority, NotificationProperties, PromptChoice,
    PromptChoiceKind, PromptOptions, Severity,
};
use je_platform::quickinput::quick_input::{IPickOptions, IQuickInputService, IQuickItem, IQuickPickItem};
use je_platform::resources::join_path;
use je_platform::storage::{StorageScope, StorageService};
use je_platform::virtual_workspace::is_virtual_workspace;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState, has_workspace_file_extension};
use je_base::uri::URI;

use crate::services::host::host::{IHostService, WindowOpenable};

/// O `neverShowAgain` e a prioridade que os dois prompts compartilham.
fn prompt_options(storage_service: &dyn StorageService) -> PromptOptions {
    PromptOptions {
        properties: NotificationProperties {
            never_show_again: Some(NeverShowAgainOptions { id: "workspaces.dontPromptToOpen".to_string(), is_secondary: true, scope: Some(NeverShowAgainScope::Workspace) }),
            // https://github.com/microsoft/vscode/issues/125315
            priority: Some(if !storage_service.is_new(StorageScope::Workspace) { NotificationPriority::Silent } else { NotificationPriority::Optional }),
            ..NotificationProperties::default()
        },
        on_cancel: None,
    }
}

/// Uma escolha comum do prompt.
fn choice(label: String, run: Rc<dyn Fn()>) -> PromptChoiceKind {
    PromptChoiceKind::Plain(PromptChoice { base: BasePromptChoice { telemetry_id: None, label, keep_open: false, run }, is_secondary: false })
}

/// `doHandleWorkspaceFiles`.
fn do_handle_workspace_files(
    folder: &URI,
    workspaces: Vec<String>,
    notification_service: &dyn INotificationService,
    quick_input_service: Rc<dyn IQuickInputService>,
    host_service: Rc<dyn IHostService>,
    storage_service: &dyn StorageService,
) {
    let learn_more_link = NotificationText::link(&localize("workspaceFilesLearnMore", "Learn more", &[]), "https://go.microsoft.com/fwlink/?linkid=2025315", None);

    // Prompt to open one workspace
    if workspaces.len() == 1 {
        let workspace_file = workspaces[0].clone();
        let workspace_uri = join_path(folder, &[&workspace_file]);

        notification_service.prompt(
            Severity::Info,
            NotificationText::format(
                &localize("foundWorkspaceLinked", "This folder contains a workspace file '{0}'. Do you want to open it? {1} about workspace files.", &[]),
                &[NotificationPart::Text(workspace_file), NotificationPart::Rich(learn_more_link)],
            )
            .into(),
            vec![choice(
                localize("openWorkspace", "Open Workspace", &[]),
                Rc::new(move || {
                    let _ = host_service.open_window(vec![WindowOpenable::Workspace { workspace_uri: workspace_uri.clone(), label: None }], None);
                }),
            )],
            Some(prompt_options(storage_service)),
        );
    }
    // Prompt to select a workspace from many
    else if workspaces.len() > 1 {
        let folder = folder.clone();
        notification_service.prompt(
            Severity::Info,
            NotificationText::format(
                &localize("foundWorkspacesLinked", "This folder contains multiple workspace files. Do you want to open one? {0} about workspace files.", &[]),
                &[NotificationPart::Rich(learn_more_link)],
            )
            .into(),
            vec![choice(
                localize("selectWorkspace", "Select Workspace", &[]),
                Rc::new(move || {
                    let items: Vec<IQuickPickItem> =
                        workspaces.iter().map(|workspace| IQuickPickItem { base: IQuickItem { label: JsString::from(workspace.as_str()), ..IQuickItem::default() }, ..IQuickPickItem::default() }).collect();
                    let options = IPickOptions { place_holder: Some(localize("selectToOpen", "Select a workspace to open", &[])), ..IPickOptions::default() };
                    let (folder, host_service) = (folder.clone(), host_service.clone());
                    let _ = quick_input_service.pick_items(items, Some(&options)).then(move |pick| {
                        if let Some(pick) = pick {
                            let workspace_uri = join_path(&folder, &[&pick.base.label.to_string_lossy()]);
                            let _ = host_service.open_window(vec![WindowOpenable::Workspace { workspace_uri, label: None }], None);
                        }
                    });
                }),
            )],
            Some(prompt_options(storage_service)),
        );
    }
}

/// `WorkspacesFinderContribution`.
pub struct WorkspacesFinderContribution {
    base: Rc<Disposable>,
}

impl WorkspacesFinderContribution {
    pub fn new(
        context_service: Rc<dyn IWorkspaceContextService>,
        notification_service: Rc<dyn INotificationService>,
        file_service: Rc<dyn IFileService>,
        quick_input_service: Rc<dyn IQuickInputService>,
        host_service: Rc<dyn IHostService>,
        storage_service: Rc<dyn StorageService>,
    ) -> Rc<Self> {
        let this = Rc::new(Self { base: Disposable::new() });
        Self::find_workspaces(context_service, notification_service, file_service, quick_input_service, host_service, storage_service);
        this
    }

    fn find_workspaces(
        context_service: Rc<dyn IWorkspaceContextService>,
        notification_service: Rc<dyn INotificationService>,
        file_service: Rc<dyn IFileService>,
        quick_input_service: Rc<dyn IQuickInputService>,
        host_service: Rc<dyn IHostService>,
        storage_service: Rc<dyn StorageService>,
    ) {
        let workspace = context_service.get_workspace();
        let Some(folder) = workspace.folders().into_iter().next() else {
            return; // require a single (non virtual) root folder
        };
        if context_service.get_workbench_state() != WorkbenchState::Folder || is_virtual_workspace(workspace.as_ref()) {
            return; // require a single (non virtual) root folder
        }

        let folder_uri = folder.uri.clone();
        let _ = file_service.resolve(&folder.uri, None).then(move |stat| {
            let root_file_names: Option<Vec<String>> = stat.children.map(|children| children.into_iter().map(|child| child.name).collect());
            if let Some(root_file_names) = root_file_names {
                let workspace_files: Vec<String> = root_file_names.into_iter().filter(|name| has_workspace_file_extension(name)).collect();
                if !workspace_files.is_empty() {
                    do_handle_workspace_files(&folder_uri, workspace_files, notification_service.as_ref(), quick_input_service, host_service, storage_service.as_ref());
                }
            }
        });
    }
}

impl IDisposable for WorkspacesFinderContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}
