//! Porte de `src/vs/workbench/contrib/opener/browser/opener.contribution.ts`: o `WorkbenchOpenerContribution`, que
//! intercepta o abrir de pastas (revela no explorador as pastas do workspace e, na janela de Sessions, abre as pastas
//! locais fora dele no gerenciador de arquivos do sistema).
//!
//! Recortes:
//! - O `registerWorkbenchContribution2(ID, WorkbenchOpenerContribution, WorkbenchPhase.Eventually)` do fim do módulo
//!   fica com quem monta o workbench.
//!
//! Divergências:
//! - O construtor devolve `Rc<Self>` e registra a si mesmo no `IOpenerService` (`registerOpener(this)`); o registro é
//!   desfeito no `dispose`, que quebra o ciclo entre o serviço e a contribuição.
//! - Um `URI | string` é [`OpenResource`], e o `REVEAL_IN_EXPLORER_COMMAND_ID` recebe a URI como [`Dynamic`].
//! - `error instanceof Error` é sempre verdadeiro: todo erro de Rust é um [`Error`].

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::commands::commands::ICommandService;
use je_platform::files::files::{FileOperationResult, to_file_operation_result};
use je_platform::files::i_file_service::IFileService;
use je_platform::opener::{IOpener, IOpenerService, OpenOptions, OpenResource, extract_selection};
use je_platform::resources::ext_uri;
use je_platform::workspace::IWorkspaceContextService;

use crate::api::common::ext_host_values::ApiValue;
use crate::contrib::files::browser::file_commands::REVEAL_IN_EXPLORER_COMMAND_ID;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// Os serviços que o `open` lê, compartilhados com a promessa que ele devolve.
struct OpenerServices {
    opener_service: Rc<dyn IOpenerService>,
    command_service: Rc<dyn ICommandService>,
    file_service: Rc<dyn IFileService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
}

impl OpenerServices {
    async fn open(self: Rc<Self>, link: OpenResource, options: Option<OpenOptions>) -> Result<bool, Error> {
        if options.as_ref().and_then(|options| options.open_external) == Some(true) {
            return Ok(false);
        }

        let link = match link {
            OpenResource::Uri(uri) => uri,
            OpenResource::Text(text) => URI::parse(text, false),
        };
        let mut uri = extract_selection(&link).uri;
        if uri.scheme().to_string_lossy() == schemas::FILE {
            uri = ext_uri().normalize_path(&uri);
        }
        let can_open_in_os = self.environment_service.is_sessions_window() && uri.scheme().to_string_lossy() == schemas::FILE;
        if (!self.workspace_context_service.is_inside_workspace(&uri) && !can_open_in_os) || !self.file_service.has_provider(&uri) {
            return Ok(false);
        }

        let is_directory = match self.file_service.stat(&uri).await {
            Ok(stat) => stat.is_directory,
            Err(error) => {
                if to_file_operation_result(&error) == FileOperationResult::FileNotFound {
                    return Ok(false);
                }
                return Err(error);
            }
        };
        if !is_directory {
            return Ok(false);
        }

        if self.workspace_context_service.is_inside_workspace(&uri) {
            self.command_service.execute_command(REVEAL_IN_EXPLORER_COMMAND_ID, vec![uri.to_dynamic()]).await?;
            return Ok(true);
        }
        if !can_open_in_os {
            return Ok(false);
        }
        let open_options = OpenOptions {
            open_external: Some(true),
            from_user_gesture: options.as_ref().and_then(|options| options.from_user_gesture),
            ..OpenOptions::default()
        };
        if !self.opener_service.open(&OpenResource::Uri(uri), Some(&open_options)).await? {
            return Err(Error::new(localize("folderLink.failedExternalOpen", "Unable to open the folder in the operating system's file manager.", &[])));
        }
        Ok(true)
    }
}

/// `WorkbenchOpenerContribution`.
pub struct WorkbenchOpenerContribution {
    base: Rc<Disposable>,
    services: Rc<OpenerServices>,
}

impl WorkbenchOpenerContribution {
    pub const ID: &'static str = "workbench.contrib.opener";

    pub fn new(
        opener_service: Rc<dyn IOpenerService>,
        command_service: Rc<dyn ICommandService>,
        file_service: Rc<dyn IFileService>,
        workspace_context_service: Rc<dyn IWorkspaceContextService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            base: Disposable::new(),
            services: Rc::new(OpenerServices {
                opener_service: opener_service.clone(),
                command_service,
                file_service,
                workspace_context_service,
                environment_service,
            }),
        });
        let opener: Rc<dyn IOpener> = this.clone();
        this.base.register(opener_service.register_opener(opener));
        this
    }
}

impl IOpener for WorkbenchOpenerContribution {
    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        async_fn(self.services.clone().open(resource.clone(), options.cloned()))
    }
}

impl IDisposable for WorkbenchOpenerContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}
