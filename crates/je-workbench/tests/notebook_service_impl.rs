//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookServiceImpl.test.ts` (suíte `NotebookProviderInfoStore`), caso a caso e na mesma ordem. O caso está portado.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do caso (o `disposables` é o store dele), e o caso roda no relógio virtual
//!   porque o `workbenchInstantiationService` do original é o [`EditorServiceAccessor`], que espera a parte do editor.
//! - `workbenchInstantiationService(undefined, disposables)` e o `EditorResolverService` real são os do acessor; o `IFileService`
//!   (`hasProvider() { return true; }`) é o `TestFileService` do acessor, que tem provedor para `file` (os recursos do caso são `file:`
//!   e `untitled:`); `IUriIdentityService` e `IStorageService` também são os do acessor.
//! - `mock<IExtensionService>()` com `onDidRegisterExtensions = Event.None` é o `TestExtensionService`, `mock<IAccessibilityService>()` com
//!   `onDidChangeScreenReaderOptimized = Event.None` é o [`AccessibilityServiceMock`], e `mock<INotebookEditorModelResolverService>()`, que
//!   o caso nunca usa, é o [`UnusedInputFactory`] (a criação das entradas é o trait `INotebookEditorInputFactory`, ver o módulo da loja).
//! - `providers[0] === fooInfo` é `Rc::ptr_eq`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::common::editor::{EditorInputRef, UntypedEditorInput};
use je_workbench::contrib::notebook::browser::services::notebook_provider_info_store::{INotebookEditorInputFactory, NotebookProviderInfoStore};
use je_workbench::contrib::notebook::common::notebook_provider::{NotebookEditorDescriptor, NotebookEditorDescriptorSelector, NotebookProviderInfo};
use je_workbench::services::editor::editor_resolver_service::{IEditorResolverService, RegisteredEditorPriority};
use je_workbench::services::extensions::extensions::{IExtensionService, null_extension_description};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use support::editor_service_support::EditorServiceAccessor;

fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `new class extends mock<IAccessibilityService>() { override onDidChangeScreenReaderOptimized = Event.None; }`.
struct AccessibilityServiceMock;

impl IAccessibilityService for AccessibilityServiceMock {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        absent("onDidChangeReducedMotion")
    }
    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        absent("onDidChangeReducedTransparency")
    }
    fn always_underline_access_keys(&self) -> Promise<bool> {
        absent("alwaysUnderlineAccessKeys")
    }
    fn is_screen_reader_optimized(&self) -> bool {
        absent("isScreenReaderOptimized")
    }
    fn is_motion_reduced(&self) -> bool {
        absent("isMotionReduced")
    }
    fn is_transparency_reduced(&self) -> bool {
        absent("isTransparencyReduced")
    }
    fn get_accessibility_support(&self) -> AccessibilitySupport {
        absent("getAccessibilitySupport")
    }
    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {
        absent("setAccessibilitySupport")
    }
    fn alert(&self, _message: &str) {
        absent("alert")
    }
    fn status(&self, _message: &str) {
        absent("status")
    }
}

/// `new class extends mock<INotebookEditorModelResolverService>() { }`: nenhuma criação de entrada acontece no caso.
struct UnusedInputFactory;

impl INotebookEditorInputFactory for UnusedInputFactory {
    fn create_notebook_editor_input(&self, _notebook_uri: &URI, _preferred_resource: Option<&URI>, _view_type: &str) -> EditorInputRef {
        absent("createNotebookEditorInput")
    }
    fn create_diff_editor_input(&self, _modified: &URI, _label: Option<&str>, _description: Option<&str>, _original: &URI, _view_type: &str, _multi_diff: bool) -> EditorInputRef {
        absent("createDiffEditorInput")
    }
    fn create_merge_editor_input(&self, _merge_editor: &UntypedEditorInput) -> EditorInputRef {
        absent("createMergeEditorInput")
    }
    fn create_untitled_notebook_editor_input(&self, _resource: &URI, _view_type: &str) -> Promise<EditorInputRef> {
        absent("resolve")
    }
    fn find_cell_index(&self, _notebook_uri: &URI, _handle: i64) -> Promise<usize> {
        absent("resolve")
    }
}

fn provider_info(id: &str, filename_pattern: &str) -> Rc<NotebookProviderInfo> {
    Rc::new(NotebookProviderInfo::new(NotebookEditorDescriptor {
        extension: Some(null_extension_description().identifier),
        id: id.to_string(),
        display_name: id.to_string(),
        selectors: vec![NotebookEditorDescriptorSelector { filename_pattern: Some(filename_pattern.to_string()), exclude_file_name_pattern: None }],
        priority: RegisteredEditorPriority::Default,
        provider_display_name: id.to_string(),
    }))
}

#[test]
fn cant_open_untitled_notebooks_in_test_119363() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();
    run_with_faked_timers(async move {
        let accessor = EditorServiceAccessor::with_default_backup_service(&disposables);
        accessor.part.when_ready().await.unwrap();

        let storage_service: Rc<dyn StorageService> = Rc::new(disposables.add_rc(InMemoryStorageService::new()));
        let extension_service: Rc<dyn IExtensionService> = TestExtensionService::new();
        let editor_resolver_service: Rc<dyn IEditorResolverService> = accessor.editor_resolver_service.clone();
        let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
        let file_service: Rc<dyn IFileService> = accessor.file_service.clone();
        let uri_identity_service: Rc<dyn IUriIdentityService> = accessor.uri_identity_service.clone();
        let store = NotebookProviderInfoStore::new(
            storage_service,
            extension_service,
            editor_resolver_service,
            configuration_service,
            Rc::new(AccessibilityServiceMock),
            file_service,
            uri_identity_service,
            Rc::new(UnusedInputFactory),
        );
        disposables.add_rc(store.clone());

        let foo_info = provider_info("foo", "*.foo");
        let bar_info = provider_info("bar", "*.bar");

        let _ = store.add(foo_info.clone(), true);
        let _ = store.add(bar_info.clone(), true);

        assert!(store.get("foo").is_some());
        assert!(store.get("bar").is_some());
        assert!(store.get("barfoo").is_none());

        let mut providers = store.get_contributed_notebook(&URI::parse("file:///test/nb.foo", false));
        assert_eq!(providers.len(), 1);
        assert!(Rc::ptr_eq(&providers[0], &foo_info));

        providers = store.get_contributed_notebook(&URI::parse("file:///test/nb.bar", false));
        assert_eq!(providers.len(), 1);
        assert!(Rc::ptr_eq(&providers[0], &bar_info));

        providers = store.get_contributed_notebook(&URI::parse("untitled:///Untitled-1", false));
        assert_eq!(providers.len(), 2);
        assert!(Rc::ptr_eq(&providers[0], &foo_info));
        assert!(Rc::ptr_eq(&providers[1], &bar_info));

        providers = store.get_contributed_notebook(&URI::parse("untitled:///test/nb.bar", false));
        assert_eq!(providers.len(), 1);
        assert!(Rc::ptr_eq(&providers[0], &bar_info));

        store.dispose();
        disposables.clear();
    });
}
