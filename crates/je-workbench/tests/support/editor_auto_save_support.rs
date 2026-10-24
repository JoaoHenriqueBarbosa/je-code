//! Infraestrutura de teste de `editorAutoSave.test.ts` e `textFileEditorTracker.test.ts`: o `createEditorAutoSave` e o
//! `createTracker` do original montam o mesmo cenário (um segundo `IConfigurationService` com `files.*` do usuário, um
//! `TestFilesConfigurationService` sobre ele, a parte de editores, o `EditorService` real e o `TestServiceAccessor`), e o
//! rastreador usa um `TestHostService` com foco.
//!
//! Divergências:
//! - **`workbenchInstantiationService`, `createEditorPart` e `EditorService`.** O [`EditorServiceAccessor`] os monta (as
//!   divergências estão no cabeçalho de `support/editor_service_support.rs`); [`create_accessor`] é o trecho comum do
//!   `createEditorAutoSave` e do `createTracker`: o `TestConfigurationService` com `files` entra na coleção e na parte antes de
//!   o `EditorService` ser criado (`instantiationService.stub(IConfigurationService, configurationService)`).
//! - **`instantiationService.stub(IFilesConfigurationService, ...)`.** O `TestFilesConfigurationService` do original substitui o
//!   serviço na coleção, e os modelos de arquivo o leem de lá. O acessor do arquivo de texto cria os modelos com o serviço
//!   dele (o padrão), então o `TestFilesConfigurationService` de [`create_accessor`] é o que a contribuição sob teste recebe:
//!   nenhum dos casos lê o `files.autoSave` pelos modelos.
//! - **`TestHostService`.** O [`FocusTestHostService`] é o `TestHostService` de `workbenchTestServices.ts` (`hasFocus`,
//!   `onDidChangeFocus`, `setFocus`); o dublê de `je_workbench::test` não tem foco.

use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::services::host::host::{IHostService, ReloadOptions, OpenWindowOptions, WindowOpenable};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService, TestFilesConfigurationService, TestMarkerService, TestTextResourceConfigurationService};

use super::editor_service_support::EditorServiceAccessor;

/// `TestHostService`: a janela tem foco até o teste dizer o contrário.
pub struct FocusTestHostService {
    has_focus: Cell<bool>,
    on_did_change_focus: Rc<Emitter<bool>>,
}

impl FocusTestHostService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { has_focus: Cell::new(true), on_did_change_focus: Emitter::new() })
    }

    /// `setFocus(focus)`.
    pub fn set_focus(&self, focus: bool) {
        self.has_focus.set(focus);
        self.on_did_change_focus.fire(focus);
    }
}

impl IDisposable for FocusTestHostService {
    fn dispose(&self) {
        self.on_did_change_focus.dispose();
    }
}

impl IHostService for FocusTestHostService {
    fn open_window(&self, _to_open: Vec<WindowOpenable>, _options: Option<OpenWindowOptions>) -> Promise<()> {
        Promise::resolved(())
    }

    fn reload(&self, _options: Option<ReloadOptions>) -> Promise<()> {
        Promise::resolved(())
    }

    fn restart(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn has_focus(&self) -> bool {
        self.has_focus.get()
    }

    fn on_did_change_focus(&self) -> Event<bool> {
        self.on_did_change_focus.event()
    }
}

/// O que o `createEditorAutoSave` e o `createTracker` devolvem: o acessor e o `IFilesConfigurationService` de teste.
pub struct AutoSaveAccessor {
    pub accessor: EditorServiceAccessor,
    pub files_configuration_service: Rc<TestFilesConfigurationService>,
}

/// O trecho comum do `createEditorAutoSave(autoSaveConfig)` e do `createTracker(autoSaveEnabled)`: `files_configuration` é o
/// valor de `configurationService.setUserConfiguration('files', ...)`.
pub fn create_accessor(disposables: &Rc<DisposableStore>, files_configuration: Dynamic) -> AutoSaveAccessor {
    let test_configuration_service = TestConfigurationService::new(Some(Dynamic::object([("files", files_configuration)])));
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;

    let accessor = EditorServiceAccessor::with_configuration_service(disposables, {
        let configuration_service = configuration_service.clone();
        move |_| configuration_service
    });

    let context_key_service = disposables.add_rc(MockContextKeyService::new());
    let uri_identity_file_service = disposables.add_rc(TestFileService::new());
    let uri_identity_file_service_contract: Rc<dyn IFileService> = uri_identity_file_service;
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (uri_identity_file_service_contract,)));
    let file_service = disposables.add_rc(TestFileService::new());
    let file_service_contract: Rc<dyn IFileService> = file_service;

    let files_configuration_service = disposables.add_rc(TestFilesConfigurationService::create(
        (),
        (
            context_key_service,
            configuration_service.clone(),
            TestContextService::new(None, None),
            TestEnvironmentService::new(),
            uri_identity_service,
            file_service_contract,
            Rc::new(TestMarkerService),
            TestTextResourceConfigurationService::new(configuration_service),
        ),
    ));

    disposables.add_rc(accessor.files());

    AutoSaveAccessor { accessor, files_configuration_service }
}
