//! Os serviços do `workbenchInstantiationService(undefined, disposables)` que as entradas de editor de recurso leem
//! (`ILabelService`, `IFileService`, `IFilesConfigurationService`, `ITextResourceConfigurationService` e o
//! `IWorkspaceContextService` com o `TestWorkspace`), com os mesmos dublês do original. O `UriIdentityService` real ainda
//! não foi portado: o [`TestUriIdentityService`] devolve o `ExtUri` sem viés e `asCanonicalUri` identidade, que é o que
//! os serviços usam dele.
#![allow(dead_code)]

use std::rc::Rc;

use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::resources::host_environment;
use je_platform::storage::StorageService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::common::editor::resource_editor_input::ResourceEditorInputServices;
use je_workbench::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use je_workbench::services::label::label_service::LabelService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{
    TestContextService, TestFileService, TestFilesConfigurationService, TestLifecycleService, TestMarkerService, TestStorageService,
    TestTextResourceConfigurationService,
};

use crate::common::LeakGuard;

/// Um `IUriIdentityService` com o `ExtUri` sem viés.
pub struct TestUriIdentityService {
    ext_uri: ExtUri,
}

impl IUriIdentityService for TestUriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        self.ext_uri.clone()
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// `new TestUriIdentityService()`.
pub fn uri_identity_service() -> Rc<dyn IUriIdentityService> {
    Rc::new(TestUriIdentityService { ext_uri: ExtUri::unbiased(host_environment()) })
}

/// O que o `workbenchInstantiationService` entrega e as entradas leem.
pub struct WorkbenchServices {
    pub context_service: Rc<TestContextService>,
    pub label_service: Rc<LabelService>,
    pub file_service: Rc<TestFileService>,
    pub files_configuration_service: Rc<TestFilesConfigurationService>,
    pub text_resource_configuration_service: Rc<TestTextResourceConfigurationService>,
}

impl WorkbenchServices {
    /// `workbenchInstantiationService(undefined, disposables)`, com o `IConfigurationService` dado.
    pub fn new(disposables: &LeakGuard, configuration_service: &Rc<TestConfigurationService>) -> Self {
        let configuration_service: Rc<dyn IConfigurationService> = configuration_service.clone();
        let context_service = TestContextService::new(None, None);
        let uri_identity = uri_identity_service();

        let context_key_service = disposables.add_rc(ContextKeyService::new(configuration_service.clone()));
        let file_service = disposables.add_rc(TestFileService::new());
        let text_resource_configuration_service = TestTextResourceConfigurationService::new(configuration_service.clone());
        let files_configuration_service = disposables.add_rc(TestFilesConfigurationService::create(
            (),
            (
                context_key_service,
                configuration_service,
                context_service.clone(),
                TestEnvironmentService::new(),
                uri_identity.clone(),
                file_service.clone(),
                Rc::new(TestMarkerService),
                text_resource_configuration_service.clone(),
            ),
        ));

        let storage = disposables.add_rc(TestStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage);
        let lifecycle = disposables.add_rc(TestLifecycleService::new());
        let label_service = disposables.add_rc(LabelService::create(
            (),
            (TestEnvironmentService::new(), context_service.clone(), TestPathService::new(None, None), Rc::new(TestRemoteAgentService), storage_service, lifecycle, uri_identity),
        ));

        Self { context_service, label_service, file_service, files_configuration_service, text_resource_configuration_service }
    }

    /// Os parâmetros decorados do construtor do `AbstractResourceEditorInput`.
    pub fn resource_editor_input_services(&self, custom_editor_label_service: Rc<dyn ICustomEditorLabelService>) -> ResourceEditorInputServices {
        ResourceEditorInputServices {
            label_service: self.label_service.clone(),
            file_service: self.file_service.clone(),
            files_configuration_service: self.files_configuration_service.clone(),
            text_resource_configuration_service: self.text_resource_configuration_service.clone(),
            custom_editor_label_service,
        }
    }
}

/// `TestLanguageDetectionService`: a detecção desligada.
pub struct TestLanguageDetectionService;

impl je_workbench::services::language_detection::language_detection_worker_service::ILanguageDetectionService for TestLanguageDetectionService {
    fn is_enabled_for_language(&self, _language_id: &str) -> bool {
        false
    }

    fn detect_language(&self, _resource: &URI, _supported_langs: Option<&[String]>) -> je_base::promise::Promise<Option<String>> {
        je_base::promise::Promise::resolved(None)
    }
}

/// `TestAccessibilityService`.
pub struct TestAccessibilityService;

impl je_platform::accessibility::IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> je_base::event::Event<()> {
        je_base::event::Event::none()
    }

    fn on_did_change_reduced_motion(&self) -> je_base::event::Event<()> {
        je_base::event::Event::none()
    }

    fn on_did_change_reduced_transparency(&self) -> je_base::event::Event<()> {
        je_base::event::Event::none()
    }

    fn always_underline_access_keys(&self) -> je_base::promise::Promise<bool> {
        je_base::promise::Promise::resolved(false)
    }

    fn is_screen_reader_optimized(&self) -> bool {
        false
    }

    fn is_motion_reduced(&self) -> bool {
        false
    }

    fn is_transparency_reduced(&self) -> bool {
        false
    }

    fn get_accessibility_support(&self) -> je_platform::accessibility::AccessibilitySupport {
        je_platform::accessibility::AccessibilitySupport::Unknown
    }

    fn set_accessibility_support(&self, _accessibility_support: je_platform::accessibility::AccessibilitySupport) {}

    fn alert(&self, _message: &str) {}

    fn status(&self, _message: &str) {}
}
