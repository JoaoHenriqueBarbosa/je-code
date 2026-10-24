//! Porte de `src/vs/workbench/services/textfile/electron-browser/nativeTextFileService.ts`: o `NativeTextFileService`, o
//! serviço de arquivos de texto do desktop, que acrescenta ao [`AbstractTextFileService`] o prolongamento do desligamento
//! enquanto há arquivos pendentes de salvar e a garantia de que toda leitura leva um objeto `limits`.
//!
//! Divergências:
//! - **Subclasse sem estado próprio.** Como no [`BrowserTextFileService`](super::browser_text_file_service), o
//!   `NativeTextFileService` só registra uma escuta no construtor, então [`NativeTextFileService::new`] monta o
//!   [`AbstractTextFileService`], registra a escuta e devolve esse mesmo objeto. O `environmentService` que o original
//!   redeclara com o tipo `INativeWorkbenchEnvironmentService` é o mesmo serviço que a base já guarda, e o porte não tem
//!   membro com tipo mais estreito, então a redeclaração não existe.
//! - **`read` e `readStream` sobrescritos.** Os dois só chamam `ensureLimits(options)` antes de `super.read` e
//!   `super.readStream`; escrever um tipo que repassasse os outros membros do contrato seria função de repasse. A base
//!   expõe o gancho [`AbstractTextFileService::set_read_options_hook`], que ela aplica às opções antes de ler, e o
//!   construtor instala nele o [`ensure_limits`].
//! - **`ensureLimits`.** `Object.create(null)` (as opções ou os limites ainda ausentes) é o valor padrão da struct; o
//!   `{ ...ensuredOptions, limits }` é a atribuição de `limits` na cópia das opções.
//! - **Injeção.** O construtor recebe a struct [`TextFileServiceDependencies`] (ver o módulo da classe base).
//! - **`registerSingleton(ITextFileService, NativeTextFileService, InstantiationType.Eager)`** entra com a montagem do
//!   workbench, que cria o `TextFileEditorModelManager` e passa o resultado em `files`.
//! - **`event.join(this.onWillShutdown(), { id, label })`** é [`WillShutdownEvent::join`]; `Promises.settled` é
//!   [`promises::settled`].

use std::rc::Rc;

use je_base::async_::{async_fn, promises};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::files::files::FileReadLimits;

use crate::services::lifecycle::lifecycle::WillShutdownEventJoiner;
use crate::services::textfile::abstract_text_file_service::{AbstractTextFileService, TextFileServiceDependencies};
use crate::services::textfile::textfiles::{ITextFileEditorModel, ITextFileEditorModelManager, ITextFileService, ReadTextFileOptions, TextFileEditorModelState};

/// `NativeTextFileService`.
pub struct NativeTextFileService;

impl NativeTextFileService {
    /// `constructor(...)`: `super(...)` seguido de `registerListeners()`.
    pub fn new(dependencies: TextFileServiceDependencies) -> Rc<AbstractTextFileService> {
        let service = AbstractTextFileService::new(dependencies);

        // ensure platform limits are applied (`read` e `readStream`)
        service.set_read_options_hook(Rc::new(|options| Some(Self::ensure_limits(options))));

        // Lifecycle
        let files = service.files();
        service.register(service.lifecycle_service().on_will_shutdown().listen(move |event| {
            event.join(
                Self::on_will_shutdown(files.clone()),
                WillShutdownEventJoiner { id: "join.textFiles".to_string(), label: localize("join.textFiles", "Saving text files", &[]), order: None },
            );
        }));

        service
    }

    fn on_will_shutdown(files: Rc<dyn ITextFileEditorModelManager>) -> Promise<()> {
        async_fn(async move {
            // As long as models are pending to be saved, we prolong the shutdown
            // until that has happened to ensure we are not shutting down in the
            // middle of writing to the file
            // (https://github.com/microsoft/vscode/issues/116600)
            loop {
                let models_pending_to_save: Vec<Rc<dyn ITextFileEditorModel>> =
                    files.models().into_iter().filter(|model| model.has_state(TextFileEditorModelState::PendingSave)).collect();
                if models_pending_to_save.is_empty() {
                    break;
                }

                promises::settled(models_pending_to_save.iter().map(|model| model.join_state(TextFileEditorModelState::PendingSave)).collect()).await?;
            }

            Ok(())
        })
    }

    /// `ensureLimits(options)`.
    pub fn ensure_limits(options: Option<ReadTextFileOptions>) -> ReadTextFileOptions {
        let mut ensured_options = options.unwrap_or_default();

        if ensured_options.limits.is_none() {
            ensured_options.limits = Some(FileReadLimits::default());
        }

        ensured_options
    }
}
