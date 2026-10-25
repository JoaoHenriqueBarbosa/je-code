//! Porte de `src/vs/editor/test/browser/diff/testDiffProviderFactoryService.ts`: o `IDiffProviderFactoryService` dos testes, cujo
//! provedor calcula o diff na hora (sem o serviço de worker), com o computador de linhas padrão.
//!
//! Divergências:
//! - `linesDiffComputers.getDefault()` (`linesDiffComputers.ts`, o `DefaultLinesDiffComputer` por trás de uma fábrica) é o
//!   [`DefaultLinesDiffComputer`] criado a cada cálculo: o computador não guarda estado entre chamadas.
//! - `Promise.resolve({...})` é a [`Promise`] já resolvida, e `onDidChange: Event<void> = () => toDisposable(() => { })` (um
//!   evento cujo ouvinte nunca é chamado) é [`Event::none`].
//! - Este arquivo só usa os caminhos públicos das crates (`je_editor::...`), para que os testes de outras crates o incluam por
//!   `#[path]` sem depender do restante do `tests/support` do `je-editor`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::promise::Promise;
use je_editor::diff::default_lines_diff_computer::default_lines_diff_computer::DefaultLinesDiffComputer;
use je_editor::diff::document_diff_provider::{DocumentDiff, DocumentDiffProviderOptions, IDocumentDiffProvider};
use je_editor::diff::lines_diff_computer::ILinesDiffComputer;
use je_editor::diff_provider_factory_service::{DocumentDiffFactoryOptions, IDiffProviderFactoryService};
use je_editor::text_model::TextModel;

/// `TestDiffProviderFactoryService`.
pub struct TestDiffProviderFactoryService;

impl IDiffProviderFactoryService for TestDiffProviderFactoryService {
    fn create_diff_provider(&self, _options: DocumentDiffFactoryOptions) -> Rc<dyn IDocumentDiffProvider> {
        Rc::new(SyncDocumentDiffProvider)
    }
}

/// `SyncDocumentDiffProvider`.
struct SyncDocumentDiffProvider;

impl IDocumentDiffProvider for SyncDocumentDiffProvider {
    fn compute_diff(
        &self,
        original: &Rc<TextModel>,
        modified: &Rc<TextModel>,
        options: &DocumentDiffProviderOptions,
        _cancellation_token: &CancellationToken,
    ) -> Promise<DocumentDiff> {
        let result = DefaultLinesDiffComputer::new().compute_diff(&original.get_lines_content(), &modified.get_lines_content(), options);
        Promise::resolved(DocumentDiff {
            changes: result.changes,
            quit_early: result.hit_timeout,
            identical: original.get_value() == modified.get_value(),
            moves: result.moves,
        })
    }

    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }
}
