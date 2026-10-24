//! Porte parcial de `src/vs/workbench/contrib/timeline/browser/timeline.contribution.ts`: o comando `files.openTimeline`
//! (`OpenTimelineAction`).
//!
//! Divergências:
//! - **Efeito colateral do `import`.** O módulo do original registra tudo ao ser carregado; aqui o registro é a função
//!   [`register_timeline_contribution`], que roda uma vez por thread (o registro de comandos é do thread).
//! - **`handler(): ICommandHandler`.** O handler `async (accessor, arg)` é um [`CommandHandler`] que devolve a promessa do
//!   corpo (`promise_to_dynamic`); os serviços saem do `accessor` na hora, antes do primeiro `await`, como no original.
//! - **`getResourceForCommand(arg, ...)`.** É [`get_resource_for_command`], com o `arg` como `Option<&Dynamic>`.
//! - **Fora do recorte.** Os ícones (`registerIcon`), a configuração `timeline.pageSize` e `timeline.pageOnScroll`, o
//!   `TimelinePaneDescriptor` e o registro da view no `VIEW_CONTAINER` do explorador (dependem do `TimelinePane` e do
//!   `explorerViewlet`, ainda não portados) e os itens de menu (`ExplorerContext` e `TimelineTitle`, que dependem do
//!   `ResourceContextKey`, do registro de ícones e do `TimelineHasProviderContext`) entram com essas peças.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_platform::commands::commands::{CommandHandler, commands_registry};
use je_platform::list::list_service::I_LIST_SERVICE;

use crate::api::browser::main_thread_commands::promise_to_dynamic;
use crate::contrib::files::browser_files::get_resource_for_command;
use crate::contrib::timeline::common::timeline::{I_TIMELINE_SERVICE, TIMELINE_PANE_ID};
use crate::services::editor::editor_service::I_EDITOR_SERVICE;
use crate::services::views::views_service::I_VIEWS_SERVICE;

/// `OpenTimelineAction.ID`.
pub const OPEN_TIMELINE_ACTION_ID: &str = "files.openTimeline";

/// `OpenTimelineAction.handler()`.
fn open_timeline_handler() -> CommandHandler {
    Rc::new(|accessor, args| {
        let timeline_service = accessor.get(I_TIMELINE_SERVICE);
        let views_service = accessor.get(I_VIEWS_SERVICE);

        // Fall back to the focused explorer item or active editor when invoked without a resource (e.g. from a keybinding)
        let uri = get_resource_for_command(args.first(), &*accessor.get(I_EDITOR_SERVICE), &*accessor.get(I_LIST_SERVICE));
        promise_to_dynamic(async_fn(async move {
            match uri {
                Some(uri) => timeline_service.set_uri(Some(uri)).await?,
                None => views_service.open_view(TIMELINE_PANE_ID, true).await?,
            }
            Ok(Dynamic::Undefined)
        }))
    })
}

thread_local! {
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// O que o módulo do original faz ao ser importado: `CommandsRegistry.registerCommand(OpenTimelineAction.ID, ...)`.
pub fn register_timeline_contribution() {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }
    commands_registry().register_command_handler(OPEN_TIMELINE_ACTION_ID, Some(open_timeline_handler()));
}
