//! Porte de `src/vs/workbench/contrib/output/test/browser/outputServices.test.ts` (suíte `OutputService channel disposal`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (dentro do corpo assíncrono,
//!   que roda em `run_with_real_timers`); `disposables.add` vira `guard.add_rc`.
//! - **`workbenchInstantiationService({}, disposables)` e os `stub`.** É o [`new_service`], que monta o [`OutputServiceServices`]
//!   com os dublês do porte (`TestConfigurationService`, `TestFileService`, `TestLoggerService`, `TestStorageService`,
//!   `TestLifecycleService`, `TestEnvironmentService`, `TestContextService`, o `ModelService` e o `LanguageService` reais) e os
//!   locais: o `TestViewsService` (`isViewVisible` verdadeiro, `openView` e `getActiveViewWithId` devolvem `null`), o
//!   `IDefaultLogLevelsService` com `onDidChangeDefaultLogLevels = Event.None`, o `ITextModelService` que só registra o provedor
//!   de conteúdo (o `TextModelResolverService` do original) e o `IFileDialogService` e o `IEditorWorkerService`, que o teste não usa. O
//!   `TestContextService(new Workspace('output-test', []))` é o `new_test_workspace` sem pastas. O `IWebWorkerService` (`mock`) não existe
//!   no porte: o `OutputLinkProvider` calcula os links no mesmo thread (ver `output_link_provider.rs`).
//! - **`setup`/`teardown` do registro compartilhado.** O registro de canais é um por thread (`output_channel_registry()`), então
//!   cada caso já começa sem canais; o `setup` ainda os remove (nenhum) e o `teardown` que os restaura não tem o que restaurar.
//! - **`service['_store']['_toDispose']`.** É [`OutputService::registrations_len`], o tamanho do `DisposableStore` do serviço: o
//!   original compara o conteúdo do `Set` antes e depois, o porte compara o tamanho (nenhum registro por canal pode sobrar).
//! - **`WeakRef`, `globalThis.gc` e `this.skip()`.** O `WeakRef` é o `Weak<OutputChannel>`, e o canal solto é liberado na hora
//!   (contagem de referências), sem coletor: os dois casos de coleta não precisam do `--expose-gc` e nunca são pulados. O
//!   `await timeout(0)` que deixa o `WeakRef` ir embora no fim do job é o [`timeout`] do guarda.
//! - **`for (const removeFromRegistry of [false, true]) test(...)`.** Vira um teste por valor, na ordem do original.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit;
use je_editor::editor_worker::{IEditorWorkerService, WordRanges};
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::language_service::LanguageService;
use je_editor::model_service::ModelService;
use je_editor::range::Range;
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::dialogs::{ConfirmResult, FileNameOrResource, IFileDialogService, OpenDialogOptions, PickAndOpenOptions, SaveDialogOptions};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::{LogLevel, LoggerService, NullLogService};
use je_platform::resources::uri_file;
use je_platform::storage::storage::InMemoryStorageService;
use je_platform::workspace::new_test_workspace;
use je_workbench::contrib::output::browser::output_services::{OutputChannel, OutputService, OutputServiceServices};
use je_workbench::contrib::output::common::output_channel_model::{IOutputChannelModel, OutputChannelModelServices};
use je_workbench::services::log::default_log_levels::{DefaultLogLevels, IDefaultLogLevelsService};
use je_workbench::services::output::output::{
    output_channel_registry, IOutputChannel, IOutputChannelDescriptor, IOutputChannelRegistry, IOutputContentSource, IOutputService, OutputChannelRegistry,
    OutputChannelSource,
};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextModelContentProvider, ITextModelService, TextModelReference};
use je_workbench::services::views::views_service::{IOutputViewPane, IViewPane, IViewsService, ViewVisibilityChangeEvent};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService, TestLifecycleService, TestLoggerService, TestTextResourcePropertiesService};

/// `TestViewsService`, só com o que o `OutputService` lê.
struct TestViewsService;

impl IViewsService for TestViewsService {
    fn open_view(&self, _id: &str, _focus: bool) -> Promise<()> {
        Promise::resolved(())
    }

    fn open_output_view(&self, _id: &str, _focus: bool) -> Promise<Option<Rc<dyn IOutputViewPane>>> {
        Promise::resolved(None)
    }

    fn get_view_with_id(&self, _id: &str) -> Option<Rc<dyn IViewPane>> {
        None
    }

    fn get_active_output_view_with_id(&self, _id: &str) -> Option<Rc<dyn IOutputViewPane>> {
        None
    }

    fn on_did_change_view_visibility(&self) -> Event<ViewVisibilityChangeEvent> {
        Event::none()
    }

    fn is_view_visible(&self, _id: &str) -> bool {
        true
    }
}

/// `new class extends mock<IDefaultLogLevelsService>() { override readonly onDidChangeDefaultLogLevels = Event.None; }`: os outros
/// membros falham, como o `TypeError` do `mock`.
struct TestDefaultLogLevelsService;

impl IDefaultLogLevelsService for TestDefaultLogLevelsService {
    fn default_log_levels(&self) -> DefaultLogLevels {
        unreachable!("mock<IDefaultLogLevelsService>: defaultLogLevels não é definido")
    }

    fn on_did_change_default_log_levels(&self) -> Event<DefaultLogLevels> {
        Event::none()
    }

    fn get_default_log_level(&self, _extension_id: Option<&str>) -> LogLevel {
        unreachable!("mock<IDefaultLogLevelsService>: getDefaultLogLevel não é definido")
    }

    fn set_default_log_level(&self, _log_level: LogLevel, _extension_id: Option<&str>) -> Promise<()> {
        unreachable!("mock<IDefaultLogLevelsService>: setDefaultLogLevel não é definido")
    }
}

/// O `ITextModelService` do `workbenchInstantiationService`, só com o registro de provedores de conteúdo que o `OutputService` usa.
struct TestTextModelService;

impl ITextModelService for TestTextModelService {
    fn create_model_reference(&self, _resource: &URI) -> Promise<TextModelReference> {
        unreachable!("ITextModelService.createModelReference não é usado por este teste")
    }

    fn register_text_model_content_provider(&self, _scheme: &str, _provider: Rc<dyn ITextModelContentProvider>) -> DisposableRef {
        to_disposable(|| {})
    }
}

/// O `IFileDialogService` do `workbenchInstantiationService`: nenhum caso abre diálogo.
struct UnusedFileDialogService;

impl IFileDialogService for UnusedFileDialogService {
    fn default_file_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        unreachable!("IFileDialogService.defaultFilePath não é usado por este teste")
    }

    fn default_folder_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        unreachable!("IFileDialogService.defaultFolderPath não é usado por este teste")
    }

    fn default_workspace_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        unreachable!("IFileDialogService.defaultWorkspacePath não é usado por este teste")
    }

    fn pick_file_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        unreachable!("IFileDialogService.pickFileFolderAndOpen não é usado por este teste")
    }

    fn pick_file_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        unreachable!("IFileDialogService.pickFileAndOpen não é usado por este teste")
    }

    fn pick_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        unreachable!("IFileDialogService.pickFolderAndOpen não é usado por este teste")
    }

    fn pick_workspace_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        unreachable!("IFileDialogService.pickWorkspaceAndOpen não é usado por este teste")
    }

    fn pick_file_to_save(&self, _default_uri: &URI, _available_file_systems: Option<Vec<String>>) -> Promise<Option<URI>> {
        unreachable!("IFileDialogService.pickFileToSave não é usado por este teste")
    }

    fn preferred_home(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        unreachable!("IFileDialogService.preferredHome não é usado por este teste")
    }

    fn show_save_dialog(&self, _options: SaveDialogOptions) -> Promise<Option<URI>> {
        unreachable!("IFileDialogService.showSaveDialog não é usado por este teste")
    }

    fn show_save_confirm(&self, _file_names_or_resources: &[FileNameOrResource]) -> Promise<ConfirmResult> {
        unreachable!("IFileDialogService.showSaveConfirm não é usado por este teste")
    }

    fn show_open_dialog(&self, _options: OpenDialogOptions) -> Promise<Option<Vec<URI>>> {
        unreachable!("IFileDialogService.showOpenDialog não é usado por este teste")
    }
}

/// O `IEditorWorkerService` do `workbenchInstantiationService`: nenhum caso carrega um modelo de canal.
struct UnusedEditorWorkerService;

impl IEditorWorkerService for UnusedEditorWorkerService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        unreachable!("IEditorWorkerService.canComputeWordRanges não é usado por este teste")
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        unreachable!("IEditorWorkerService.computeWordRanges não é usado por este teste")
    }

    fn compute_more_minimal_edits(&self, _resource: &URI, _edits: Option<Vec<TextEdit>>) -> Promise<Option<Vec<TextEdit>>> {
        unreachable!("IEditorWorkerService.computeMoreMinimalEdits não é usado por este teste")
    }
}

/// O `setup` da suíte: o `OutputService` sobre os serviços do `workbenchInstantiationService`, e o registro compartilhado limpo.
struct Fixture {
    guard: LeakGuard,
    service: Rc<OutputService>,
    registry: Rc<OutputChannelRegistry>,
    next_id: Cell<u32>,
}

fn new_fixture() -> Fixture {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = output_channel_registry();
    // Earlier suites can leave source-less channels with asynchronous model initialization.
    for channel in registry.get_channels() {
        registry.remove_channel(&channel.id);
    }

    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let undo_redo_service = UndoRedoService::for_tests();
    let model_service = guard.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service),
        undo_redo_service.clone(),
    ));
    let logger_service = guard.add_rc(TestLoggerService::new(None));
    let logger_service: Rc<dyn LoggerService> = logger_service;
    let services = OutputServiceServices {
        channel_model: OutputChannelModelServices {
            file_service: { let concrete = guard.add_rc(TestFileService::new()); concrete },
            model_service,
            log_service: NullLogService::shared(),
            logger_service,
            editor_worker_service: Rc::new(UnusedEditorWorkerService),
            undo_redo_service,
        },
        storage_service: { let storage = guard.add_rc(InMemoryStorageService::new()); Rc::new(storage) },
        text_model_service: Rc::new(TestTextModelService),
        lifecycle_service: { let concrete = guard.add_rc(TestLifecycleService::new()); concrete },
        views_service: Rc::new(TestViewsService),
        context_key_service: { let concrete = guard.add_rc(MockContextKeyService::new()); concrete },
        default_log_levels_service: Rc::new(TestDefaultLogLevelsService),
        file_dialog_service: Rc::new(UnusedFileDialogService),
        environment_service: TestEnvironmentService::new(),
        language_service: { let concrete = guard.add_rc(LanguageService::new(false)); concrete },
        context_service: TestContextService::new(Some(new_test_workspace("output-test", Vec::new(), None, None)), None),
        language_features_service: Rc::new(LanguageFeaturesService::new()),
    };
    let service = guard.add_rc(OutputService::new(services));
    Fixture { guard, service, registry, next_id: Cell::new(0) }
}

impl Fixture {
    fn register_channel(&self) -> Rc<OutputChannel> {
        let id = format!("output-disposal-test-{}", self.next_id.get());
        self.next_id.set(self.next_id.get() + 1);
        self.registry.register_channel(IOutputChannelDescriptor {
            id: id.clone(),
            label: id.clone(),
            log: false,
            language_id: None,
            source: Some(OutputChannelSource::Single(IOutputContentSource { name: None, resource: uri_file(&format!("/logs/{id}.log")) })),
            extension_id: None,
            user: None,
        });
        self.service.get_channel(&id).expect("o canal foi registrado")
    }

    fn dispose_channel(&self) -> Weak<OutputChannel> {
        let channel = self.register_channel();
        let reference = Rc::downgrade(&channel);
        channel.dispose();
        assert!(self.service.get_channel(&channel.id()).is_none());
        reference
    }
}

fn does_not_retain_disposal_listeners(remove_from_registry: bool) {
    let fixture = new_fixture();
    fixture.register_channel();
    let initial_registrations = fixture.service.registrations_len();
    for _ in 0..3 {
        let channel = fixture.register_channel();
        if remove_from_registry {
            fixture.registry.remove_channel(&channel.id());
        } else {
            channel.dispose();
        }
    }
    assert_eq!(fixture.service.registrations_len(), initial_registrations);
}

#[test]
fn does_not_retain_disposal_listeners_after_channel_disposal() {
    does_not_retain_disposal_listeners(false);
}

#[test]
fn does_not_retain_disposal_listeners_after_registry_removal() {
    does_not_retain_disposal_listeners(true);
}

#[test]
fn disposed_channels_become_collectible_while_the_service_remains_alive() {
    run_with_real_timers(async move {
        let fixture = new_fixture();
        fixture.register_channel(); // Keep another channel open, as in the workbench.
        let reference = fixture.dispose_channel();
        // O `WeakRef` espera o fim do job atual.
        timeout(0).await.unwrap();
        assert!(reference.upgrade().is_none(), "Disposed output channel is still strongly retained");
    });
}

#[test]
fn disposing_the_active_channel_preserves_another_open_channel() {
    run_with_real_timers(async move {
        let fixture = new_fixture();
        let remaining = fixture.register_channel();
        let closing = fixture.register_channel();
        fixture.service.show_channel(&closing.id(), false).await.unwrap();
        closing.dispose();
        assert_eq!(
            (
                fixture.service.get_active_channel().map(|channel| channel.id()),
                fixture.service.get_channel(&remaining.id()).map(|channel| channel.id()),
                fixture.registry.get_channel(&closing.id()),
            ),
            (Some(remaining.id()), Some(remaining.id()), None)
        );
    });
}

#[test]
fn registry_removal_releases_its_channel_while_the_service_remains_alive() {
    run_with_real_timers(async move {
        let fixture = new_fixture();
        fixture.register_channel();
        let reference = {
            let channel = fixture.register_channel();
            let reference = Rc::downgrade(&channel);
            fixture.registry.remove_channel(&channel.id());
            assert!(fixture.service.get_channel(&channel.id()).is_none());
            reference
        };
        timeout(0).await.unwrap();
        assert!(reference.upgrade().is_none(), "Removed output channel is still strongly retained");
    });
}

#[test]
fn service_shutdown_still_disposes_open_channels_once() {
    let fixture = new_fixture();
    let channel = fixture.register_channel();
    let disposed = Rc::new(Cell::new(0));
    let counter = disposed.clone();
    fixture.guard.add(channel.model.on_dispose().listen(move |_| counter.set(counter.get() + 1)));
    fixture.service.dispose();
    fixture.service.dispose();
    assert_eq!((disposed.get(), fixture.registry.get_channel(&channel.id())), (1, None));
}
