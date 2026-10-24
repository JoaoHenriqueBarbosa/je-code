//! Suporte do `extensionsActions.test.ts`: o `setupTest`, `aMultiExtensionManagementServerService`,
//! `aSingleRemoteExtensionManagementServerService`, `createExtensionManagementService` e os construtores dos eventos.
//!
//! Notas de tradução:
//! - `setupTest(disposables)` é o [`ActionsFixture`]: o [`WorkbenchFixture`] (o `TestInstantiationService` do
//!   `extensionsWorkbenchService.test.ts`) com o `TestConfigurationService` no lugar do literal de `stubConfiguration`
//!   (o `setup` deste teste registra `new TestConfigurationService()`), o `ILabelService` do literal
//!   `{ onDidChangeFormatters }` ([`StubLabelService`]) e o `ExtensionsWorkbenchService` que o `setup` cria e registra
//!   (`instantiationService.set(IExtensionsWorkbenchService, ...)`; os casos que o refazem chamam
//!   [`ActionsFixture::recreate_workbench_service`]).
//! - `instantiationService.createInstance(Action, ...args)` é `Action::new(..., &fixture.services())`, com os serviços
//!   registrados naquele momento; `disposables.add(...)` é [`ActionsFixture::add`].
//! - Os emissores `installEvent`, `didInstallEvent`, `uninstallEvent` e `didUninstallEvent` do módulo do original são os do
//!   [`StubExtensionManagementService`] do `setup` (`fixture.management`). `createExtensionManagementService(installed)` é um
//!   [`StubExtensionManagementService`] novo, e o `service.onInstallExtension = emitter.event` que o original faz é
//!   disparar o emissor do próprio serviço (`service.install_event`...).
//! - `aMultiExtensionManagementServerService(local?, remote?, web?)`: `undefined` é [`ServerSlot::Default`] (um serviço
//!   novo), `null` é [`ServerSlot::Absent`] (sem servidor) e o serviço dado é [`ServerSlot::With`]; o `web` ausente é
//!   sem servidor.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use je_base::event::Emitter;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_extensions::management::{GalleryExtension, InstallOperation};
use je_extensions::manifest::ExtensionIdentifierInfo;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::resources::uri_file;
use je_workbench::contrib::extensions::extensions::{ExtensionContainers, IExtensionContainer};
use je_workbench::contrib::extensions::extensions_actions::ExtensionActionServices;
use je_workbench::contrib::extensions::extensions_service::ExtensionsWorkbenchService;
use je_workbench::services::extension_management::extension_install_service::{InstallExtensionEvent, UninstallExtensionEvent};
use je_workbench::services::extension_management::extension_management::{
    DidUninstallExtensionEvent, ExtensionRef, IExtensionManagementServer, IExtensionManagementServerService, InstallExtensionResult, InstallExtensionSource,
};

use super::extension_enablement_support::an_extension_management_server_service;
use super::extensions_workbench_service_support::{StubExtensionManagementService, StubLabelService, WorkbenchFixture, run_future};

/// O `setupTest`.
pub struct ActionsFixture {
    pub fixture: Rc<WorkbenchFixture>,
    /// O `TestConfigurationService` do `setup`.
    pub configuration: Rc<TestConfigurationService>,
    pub label_service: Rc<StubLabelService>,
    workbench_service: RefCell<Rc<ExtensionsWorkbenchService>>,
}

impl ActionsFixture {
    pub fn new() -> Rc<Self> {
        let configuration = TestConfigurationService::new(None);
        let for_fixture = configuration.clone();
        let fixture = WorkbenchFixture::with_configuration(move |_| for_fixture);
        fixture.guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
        let label_service = Rc::new(StubLabelService { did_change_formatters: fixture.guard.add_rc(Emitter::new()) });
        let workbench_service = fixture.create_workbench_service();
        Rc::new(Self { fixture, configuration, label_service, workbench_service: RefCell::new(workbench_service) })
    }

    /// `instantiationService.get(IExtensionsWorkbenchService)`.
    pub fn workbench_service(&self) -> Rc<ExtensionsWorkbenchService> {
        self.workbench_service.borrow().clone()
    }

    /// `instantiationService.set(IExtensionsWorkbenchService, disposables.add(instantiationService.createInstance(ExtensionsWorkbenchService)))`.
    pub fn recreate_workbench_service(&self) -> Rc<ExtensionsWorkbenchService> {
        let workbench_service = self.fixture.create_workbench_service();
        *self.workbench_service.borrow_mut() = workbench_service.clone();
        workbench_service
    }

    /// Os serviços registrados neste momento, para o `createInstance` de uma ação.
    pub fn services(&self) -> ExtensionActionServices {
        self.fixture.extension_action_services(self.label_service.clone(), self.workbench_service())
    }

    /// `disposables.add(x)`.
    pub fn add<T: IDisposable + 'static>(&self, disposable: Rc<T>) -> Rc<T> {
        self.fixture.guard.add_rc(disposable)
    }

    /// `disposables.add(instantiationService.createInstance(ExtensionContainers, containers))`.
    pub fn containers(&self, containers: Vec<Rc<dyn IExtensionContainer>>) {
        self.add(ExtensionContainers::new(containers, &self.workbench_service()));
    }

    /// `createExtensionManagementService(installed)`.
    pub fn create_extension_management_service(&self, installed: Vec<ExtensionRef>) -> Rc<StubExtensionManagementService> {
        StubExtensionManagementService::new(&self.fixture.guard, installed)
    }

    /// `aMultiExtensionManagementServerService(instantiationService, local, remote, web)`.
    pub fn a_multi_extension_management_server_service(
        &self,
        local: ServerSlot,
        remote: ServerSlot,
        web: Option<Rc<StubExtensionManagementService>>,
    ) -> Rc<dyn IExtensionManagementServerService> {
        let server = |id: &str, label: &str, slot: ServerSlot| match slot {
            ServerSlot::Absent => None,
            ServerSlot::Default => Some(Rc::new(IExtensionManagementServer {
                id: id.to_owned(),
                label: label.to_owned(),
                extension_management_service: self.create_extension_management_service(Vec::new()),
            })),
            ServerSlot::With(service) => Some(Rc::new(IExtensionManagementServer { id: id.to_owned(), label: label.to_owned(), extension_management_service: service })),
        };
        let web_server = web.map(|service| Rc::new(IExtensionManagementServer { id: "vscode-web".to_owned(), label: "web".to_owned(), extension_management_service: service }));
        an_extension_management_server_service(server("vscode-local", "local", local), server("vscode-remote", "remote", remote), web_server)
    }

    /// `aSingleRemoteExtensionManagementServerService(instantiationService, remoteExtensionManagementService)`.
    pub fn a_single_remote_extension_management_server_service(&self, remote: Rc<StubExtensionManagementService>) -> Rc<dyn IExtensionManagementServerService> {
        let server = Rc::new(IExtensionManagementServer { id: "vscode-remote".to_owned(), label: "remote".to_owned(), extension_management_service: remote });
        an_extension_management_server_service(None, Some(server), None)
    }

    /// `instantiationService.stub(IExtensionManagementServerService, service)` e
    /// `instantiationService.stub(IWorkbenchExtensionEnablementService, disposables.add(new TestExtensionEnablementService(instantiationService)))`.
    pub fn use_servers(&self, service: Rc<dyn IExtensionManagementServerService>) {
        self.fixture.stub_server_service(service);
        self.fixture.restub_enablement_service();
    }
}

/// O argumento de servidor de `aMultiExtensionManagementServerService`.
pub enum ServerSlot {
    /// `undefined`: um serviço de gerenciamento novo.
    Default,
    /// `null`: sem servidor.
    Absent,
    /// O serviço dado.
    With(Rc<StubExtensionManagementService>),
}

/// Roda o caso: o `setupTest`, o corpo (`async`) e o `teardown`.
pub fn run_actions<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<ActionsFixture>) -> F) {
    let fixture = ActionsFixture::new();
    run_future(body(fixture.clone()));
}

/// O profile que os eventos do original deixam `null!`.
pub fn profile() -> URI {
    uri_file("profile")
}

/// `{ identifier, source: gallery, profileLocation: null! }` (`InstallExtensionEvent`).
pub fn install_event(gallery: &GalleryExtension) -> InstallExtensionEvent {
    InstallExtensionEvent {
        identifier: gallery.identifier.clone(),
        source: InstallExtensionSource::Gallery(gallery.clone()),
        profile_location: profile(),
        application_scoped: None,
        workspace_scoped: None,
    }
}

/// `{ identifier, source: gallery, profileLocation: null! }` com o identificador dado (`installEvent.fire({ identifier: local.identifier, source: gallery, ... })`).
pub fn install_event_for(identifier: &ExtensionIdentifierInfo, gallery: &GalleryExtension) -> InstallExtensionEvent {
    InstallExtensionEvent { identifier: identifier.clone(), ..install_event(gallery) }
}

/// `{ identifier, profileLocation: null! }` (`UninstallExtensionEvent`).
pub fn uninstall_event(identifier: &ExtensionIdentifierInfo) -> UninstallExtensionEvent {
    UninstallExtensionEvent { identifier: identifier.clone(), profile_location: profile(), application_scoped: None, workspace_scoped: None }
}

/// `{ identifier, profileLocation: null! }` (`DidUninstallExtensionEvent`).
pub fn did_uninstall_event(identifier: &ExtensionIdentifierInfo) -> DidUninstallExtensionEvent {
    DidUninstallExtensionEvent { identifier: identifier.clone(), profile_location: None, error: None, workspace_scoped: None, server: None }
}

/// `{ identifier, source, operation, local, profileLocation: null! }` (`InstallExtensionResult`).
pub fn did_install_result(identifier: &ExtensionIdentifierInfo, source: Option<InstallExtensionSource>, operation: InstallOperation, local: ExtensionRef) -> InstallExtensionResult {
    InstallExtensionResult { identifier: identifier.clone(), operation, source, profile_location: Some(profile()), local: Some(local), workspace_scoped: None }
}
