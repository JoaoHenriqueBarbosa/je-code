//! Porte parcial de `src/vs/workbench/contrib/files/browser/fileCommands.ts`: o comando `revealInExplorer`
//! (`REVEAL_IN_EXPLORER_COMMAND_ID`), que o teste `fileCommands.test.ts` exercita.
//!
//! Os demais comandos do módulo (`openToSide`, `compareWithSaved`, `copyPath`, `saveAll`, `revert`, `removeRootFolder`,
//! as pastas comprimidas, `newUntitledFile`...) entram com os serviços e as palavras-chave de contexto que eles leem
//! (`IListService.lastFocusedList` com árvores, `IClipboardService`, `ILabelService`, o registro de atalhos).
//!
//! Divergências:
//! - **Efeito colateral do `import`.** O módulo do original registra tudo ao ser carregado; aqui o registro é a função
//!   [`register_file_commands`], que roda uma vez por thread (o registro de comandos é do thread).
//! - **`handler: async (accessor, resource: URI | object)`.** É um [`CommandHandler`] que devolve a promessa do corpo
//!   (`promise_to_dynamic`); os serviços saem do `accessor` na hora, antes do primeiro `await`, como no original.
//! - **`openView<ExplorerView>(id, false)`.** É [`IViewsService::open_explorer_view`], que devolve a [`IExplorerViewPane`] (com
//!   `autoReveal`, `setExpanded` e `focus`); `getViewWithId(OpenEditorsView.ID)` é [`IViewsService::get_view_with_id`].
//! - **`try/finally`.** O corpo da tentativa é um bloco `async` cujo `Result` vem depois da restauração do `autoReveal`.
//! - **`OpenEditorsView.ID`.** É [`OPEN_EDITORS_VIEW_ID`].

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_platform::commands::commands::{CommandHandler, commands_registry};
use je_platform::list::list_service::I_LIST_SERVICE;
use je_platform::workspace::I_WORKSPACE_CONTEXT_SERVICE;

use crate::api::browser::main_thread_commands::promise_to_dynamic;
use crate::contrib::files::browser_files::{ExplorerReveal, I_EXPLORER_SERVICE, get_resource_for_command};
use crate::services::editor::editor_service::I_EDITOR_SERVICE;
use crate::services::views::views_service::I_VIEWS_SERVICE;

/// `REVEAL_IN_EXPLORER_COMMAND_ID` de `fileConstants.ts`.
pub const REVEAL_IN_EXPLORER_COMMAND_ID: &str = "revealInExplorer";

/// `OpenEditorsView.ID`.
pub const OPEN_EDITORS_VIEW_ID: &str = "workbench.explorer.openEditorsView";

/// O handler de `revealInExplorer`.
fn reveal_in_explorer_handler() -> CommandHandler {
    Rc::new(|accessor, args| {
        let view_service = accessor.get(I_VIEWS_SERVICE);
        let context_service = accessor.get(I_WORKSPACE_CONTEXT_SERVICE);
        let explorer_service = accessor.get(I_EXPLORER_SERVICE);
        let editor_service = accessor.get(I_EDITOR_SERVICE);
        let list_service = accessor.get(I_LIST_SERVICE);
        let uri = get_resource_for_command(args.first(), &*editor_service, &*list_service);

        promise_to_dynamic(async_fn(async move {
            match uri.filter(|uri| context_service.is_inside_workspace(uri)) {
                Some(uri) => {
                    let explorer_view = view_service.open_explorer_view(&explorer_service.get_view_id(), false).await?;
                    if let Some(explorer_view) = explorer_view {
                        let old_auto_reveal = explorer_view.auto_reveal();
                        // Disable autoreveal before revealing the explorer to prevent a race betwene auto reveal + selection
                        // Fixes #197268
                        explorer_view.set_auto_reveal(ExplorerReveal::Flag(false));
                        let revealed = async {
                            explorer_view.set_expanded(true);
                            explorer_service.select(&uri, Some(ExplorerReveal::Mode("force".to_string()))).await?;
                            explorer_view.focus();
                            Ok::<(), Error>(())
                        }
                        .await;
                        explorer_view.set_auto_reveal(old_auto_reveal);
                        revealed?;
                    }
                }
                None => {
                    // Do not reveal the open editors view if it's hidden explicitly
                    // See https://github.com/microsoft/vscode/issues/227378
                    if let Some(open_editors_view) = view_service.get_view_with_id(OPEN_EDITORS_VIEW_ID) {
                        open_editors_view.set_expanded(true);
                        open_editors_view.focus();
                    }
                }
            }
            Ok(Dynamic::Undefined)
        }))
    })
}

thread_local! {
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// O que o módulo do original faz ao ser importado: `CommandsRegistry.registerCommand({ id: REVEAL_IN_EXPLORER_COMMAND_ID, ... })`.
pub fn register_file_commands() {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }
    commands_registry().register_command_handler(REVEAL_IN_EXPLORER_COMMAND_ID, Some(reveal_in_explorer_handler()));
}
