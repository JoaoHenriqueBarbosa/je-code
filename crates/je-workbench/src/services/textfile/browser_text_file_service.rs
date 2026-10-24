//! Porte de `src/vs/workbench/services/textfile/browser/browserTextFileService.ts`: o `BrowserTextFileService`, o
//! serviço de arquivos de texto do navegador, que acrescenta ao [`AbstractTextFileService`] o veto do desligamento
//! enquanto há arquivos pendentes de salvar (no navegador não há operação longa no desligamento).
//!
//! Divergências:
//! - **Subclasse sem estado próprio.** O `BrowserTextFileService` só registra uma escuta no construtor e não traz membro
//!   novo, então [`BrowserTextFileService::new`] monta o [`AbstractTextFileService`], registra a escuta e devolve esse
//!   mesmo objeto (que implementa o `ITextFileService`); escrever um tipo que só repassasse cada método do contrato seria
//!   função de repasse. O tipo `BrowserTextFileService` carrega apenas o construtor e a regra do veto.
//! - **Injeção.** O construtor recebe a struct [`TextFileServiceDependencies`] (ver o módulo da classe base).
//! - **`registerSingleton(ITextFileService, BrowserTextFileService, InstantiationType.Eager)`** entra com a montagem do
//!   workbench, que cria o `TextFileEditorModelManager` (cujos modelos precisam do próprio `ITextFileService`) e passa o
//!   resultado em `files`.
//! - **`event.veto(this.onBeforeShutdown(), 'veto.textFiles')`** é [`BeforeShutdownEvent::veto`] com o valor imediato.

use std::rc::Rc;

use je_base::promise::MaybePromise;

use crate::services::textfile::abstract_text_file_service::{AbstractTextFileService, TextFileServiceDependencies};
use crate::services::textfile::textfiles::{ITextFileService, TextFileEditorModelState};

/// `BrowserTextFileService`.
pub struct BrowserTextFileService;

impl BrowserTextFileService {
    /// `constructor(...)`: `super(...)` seguido de `registerListeners()`.
    pub fn new(dependencies: TextFileServiceDependencies) -> Rc<AbstractTextFileService> {
        let lifecycle_service = dependencies.lifecycle_service.clone();
        let service = AbstractTextFileService::new(dependencies);

        // Lifecycle: files are pending to be saved: veto (as there is no support for long running operations on shutdown)
        let files = service.files();
        service.register(lifecycle_service.on_before_shutdown().listen(move |event| {
            let pending_save = files.models().iter().any(|model| model.has_state(TextFileEditorModelState::PendingSave));
            event.veto(MaybePromise::Value(pending_save), "veto.textFiles");
        }));

        service
    }
}
