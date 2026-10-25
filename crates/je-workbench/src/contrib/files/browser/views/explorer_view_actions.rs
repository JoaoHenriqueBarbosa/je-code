//! Porte das quatro `registerAction2` do fim de `src/vs/workbench/contrib/files/browser/views/explorerView.ts` (linhas 1099 a
//! 1207): `CanCreateContext` e as ações de título da vista do explorador (`createFileFromExplorer`, `createFolderFromExplorer`,
//! `refreshFilesExplorer` e `collapseExplorerFolders`), todas no menu `ViewTitle`, grupo `navigation`, com
//! `when: view == VIEW_ID`.
//!
//! Divergências:
//! - **Registro explícito.** Os `registerAction2` rodam no carregamento do módulo do original; aqui são
//!   [`register_explorer_view_actions`], que devolve o descarte dos quatro registros (como `register_multi_diff_editor_menus`).
//! - **Quatro classes anônimas, uma struct.** As quatro ações só diferem no `super({...})` e no corpo de `run`; o
//!   [`ExplorerViewTitleAction`] guarda o `ICommandAction`, a `order` do menu, o `f1` e o corpo, e implementa [`Action2`] uma
//!   vez (o menu é sempre `ViewTitle`, `navigation`, `when: view == VIEW_ID`).
//! - **`Codicon`.** Não está portado: `Codicon.newFile`, `newFolder`, `refresh` e `collapseAll` são o `ThemeIcon` com o id do
//!   codicon (`new-file`, `new-folder`, `refresh`, `collapse-all`).
//! - **`view as ExplorerView`.** É o `downcast_ref` do `&dyn Any` que o [`IViewPane`](crate::services::views::views_service::IViewPane)
//!   dá (ele tem `Any` como supertrait). Uma vista que não é o [`ExplorerView`] lança `TypeError: collapseAll is not a function`,
//!   que é o que o original faz ao chamar `collapseAll` num objeto sem o método.
//! - **`async run`.** O `run` de `refreshFilesExplorer` é `async` no original; o [`Action2::run`] do porte não devolve promessa,
//!   então o corpo roda como [`async_fn`] já iniciado e a rejeição (de `openView` ou de `refresh`) vai para o monitor de
//!   rejeições sem tratador, como a promessa que o `commandService` do original deixaria de lado.
//! - **`ExplorerFindProviderActive.negate()`.** É [`RawContextKey::to_negated`](je_platform::contextkey::contextkey::RawContextKey):
//!   o `negate()` de uma expressão `Defined` é a expressão `Not` da mesma chave.

use std::any::Any;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_instantiation::ServicesAccessor;
use je_platform::action::{CommandAction, CommandActionTitle, Icon, Title, localize2};
use je_platform::actions::actions::{Action2, Action2MenuItem, Action2Options, register_action2};
use je_platform::actions::menu_ids;
use je_platform::commands::{CommandDescription, CommandMetadata, I_COMMAND_SERVICE};
use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression};

use super::explorer_view::{ExplorerView, NEW_FILE_COMMAND_ID, NEW_FOLDER_COMMAND_ID};
use crate::contrib::files::browser_files::I_EXPLORER_SERVICE;
use crate::contrib::files::common_files::{
    VIEW_ID, explorer_find_provider_active, explorer_folder_context, explorer_resource_parent_read_only_context, explorer_resource_writable_context,
};
use crate::services::views::views_service::I_VIEWS_SERVICE;

/// `CanCreateContext`: a pasta pode receber um arquivo ou pasta novo se não é somente leitura; um arquivo, se o pai não é.
pub fn can_create_context() -> ContextKeyExpression {
    let and_of = |items: [ContextKeyExpression; 2]| ContextKeyExpr::and(items).expect("a conjunção tem termos");
    ContextKeyExpr::or([
        // Folder: can create unless readonly
        and_of([explorer_folder_context().expr(), explorer_resource_writable_context()]),
        // File: can create unless parent is readonly
        and_of([explorer_folder_context().to_negated(), explorer_resource_parent_read_only_context().to_negated()]),
    ])
    .expect("a disjunção tem termos")
}

/// `ContextKeyExpr.equals('view', VIEW_ID)`.
fn is_explorer_view() -> ContextKeyExpression {
    ContextKeyExpr::equals("view", VIEW_ID)
}

fn theme_icon(id: &str) -> Option<Icon> {
    Some(Icon::Theme(ThemeIcon::from_id(id)))
}

/// `nls.localize2(key, message)` como o `title` de um `ICommandAction`.
fn localized_title(key: &str, message: &str) -> Title {
    let localized = localize2(key, message);
    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None })
}

/// `metadata: { description: nls.localize2(key, message) }`.
fn description_metadata(key: &str, message: &str) -> CommandMetadata {
    CommandMetadata::new(CommandDescription::Localized(localize2(key, message)))
}

/// O corpo de `run` das duas ações de criar: `commandService.executeCommand(commandId)`.
fn execute_command_body(command_id: &'static str) -> Rc<dyn Fn(&ServicesAccessor)> {
    Rc::new(move |accessor| {
        let command_service = accessor.get(I_COMMAND_SERVICE);
        let _ = command_service.execute_command(command_id, Vec::new());
    })
}

/// Uma ação do título da vista do explorador (ver as divergências do módulo).
struct ExplorerViewTitleAction {
    command: CommandAction,
    /// A `order` do item do `MenuId.ViewTitle`.
    order: f64,
    f1: bool,
    run: Rc<dyn Fn(&ServicesAccessor)>,
}

impl Action2 for ExplorerViewTitleAction {
    fn desc(&self) -> Action2Options {
        Action2Options {
            command: self.command.clone(),
            menu: vec![Action2MenuItem {
                id: menu_ids::view_title(),
                null_precondition: false,
                alt: None,
                when: Some(is_explorer_view()),
                group: Some("navigation".to_string()),
                order: Some(self.order),
                is_hidden_by_default: None,
            }],
            keybinding: Vec::new(),
            f1: self.f1,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        (self.run)(accessor);
    }
}

/// `createFileFromExplorer`.
fn create_file_from_explorer() -> ExplorerViewTitleAction {
    ExplorerViewTitleAction {
        command: CommandAction {
            icon: theme_icon("new-file"),
            precondition: Some(can_create_context()),
            ..CommandAction::new("workbench.files.action.createFileFromExplorer", Title::Plain(localize("createNewFile", "New File...", &[])))
        },
        order: 10.0,
        f1: false,
        run: execute_command_body(NEW_FILE_COMMAND_ID),
    }
}

/// `createFolderFromExplorer`.
fn create_folder_from_explorer() -> ExplorerViewTitleAction {
    ExplorerViewTitleAction {
        command: CommandAction {
            icon: theme_icon("new-folder"),
            precondition: Some(can_create_context()),
            ..CommandAction::new("workbench.files.action.createFolderFromExplorer", Title::Plain(localize("createNewFolder", "New Folder...", &[])))
        },
        order: 20.0,
        f1: false,
        run: execute_command_body(NEW_FOLDER_COMMAND_ID),
    }
}

/// `refreshFilesExplorer`.
fn refresh_files_explorer() -> ExplorerViewTitleAction {
    ExplorerViewTitleAction {
        command: CommandAction {
            icon: theme_icon("refresh"),
            metadata: Some(description_metadata("refreshExplorerMetadata", "Forces a refresh of the Explorer.")),
            precondition: Some(explorer_find_provider_active().to_negated()),
            ..CommandAction::new("workbench.files.action.refreshFilesExplorer", localized_title("refreshExplorer", "Refresh Explorer"))
        },
        order: 30.0,
        f1: true,
        run: Rc::new(|accessor| {
            let views_service = accessor.get(I_VIEWS_SERVICE);
            let explorer_service = accessor.get(I_EXPLORER_SERVICE);
            let _ = async_fn(async move {
                views_service.open_view(VIEW_ID, false).await?;
                explorer_service.refresh().await
            });
        }),
    }
}

/// `collapseExplorerFolders`.
fn collapse_explorer_folders() -> ExplorerViewTitleAction {
    ExplorerViewTitleAction {
        command: CommandAction {
            icon: theme_icon("collapse-all"),
            metadata: Some(description_metadata("collapseExplorerFoldersMetadata", "Folds all folders in the Explorer.")),
            ..CommandAction::new("workbench.files.action.collapseExplorerFolders", localized_title("collapseExplorerFolders", "Collapse Folders in Explorer"))
        },
        order: 40.0,
        f1: true,
        run: Rc::new(|accessor| {
            let views_service = accessor.get(I_VIEWS_SERVICE);
            if let Some(view) = views_service.get_view_with_id(VIEW_ID) {
                let view: &dyn Any = &*view;
                match view.downcast_ref::<ExplorerView>() {
                    Some(explorer_view) => explorer_view.collapse_all(),
                    None => errors::throw(Error::type_error("collapseAll is not a function")),
                }
            }
        }),
    }
}

/// Os quatro `registerAction2` do fim de `explorerView.ts`.
pub fn register_explorer_view_actions() -> DisposableRef {
    let registrations = DisposableStore::new();
    for action in [create_file_from_explorer(), create_folder_from_explorer(), refresh_files_explorer(), collapse_explorer_folders()] {
        registrations.add(register_action2(Rc::new(action)));
    }
    registrations
}
