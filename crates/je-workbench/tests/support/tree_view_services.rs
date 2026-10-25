//! Os duplos de serviço que o `CustomTreeView` do porte recebe nos testes da `TreeView` (`mainThreadTreeViews.test.ts` e
//! `treeview.test.ts`): o que o `workbenchInstantiationService` entrega ao original.
#![allow(dead_code)]

use std::rc::Rc;

use je_base::data_transfer::VSDataTransfer;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, none};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::tree_views_dnd::TreeViewsDnDService;
use je_extensions::scanner::ExtensionDescription;
use je_platform::label::label::{
    FormatterChangeEvent, ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, UriLabelOptions, WorkspaceLabelOptions, WorkspaceLabelTarget,
};
use je_platform::log::NullLogService;
use je_platform::progress::progress::{IProgress, IProgressService, Progress, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_workbench::browser::parts::views::tree_view::CustomTreeViewServices;
use je_workbench::services::activity::activity::{ActivityChangeTarget, IActivity, IActivityService};
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};

/// `TestExtensionService`: as extensões já estão registradas.
pub struct TestExtensionService;

impl IExtensionService for TestExtensionService {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        Promise::resolved(())
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        true
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        Promise::resolved(None)
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        Event::none()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        Vec::new()
    }
}

/// O `TestProgressService` do `workbenchInstantiationService`: roda a tarefa na hora.
pub struct TestProgressService;

impl IProgressService for TestProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let progress: Rc<dyn IProgress<ProgressStep>> = Progress::<ProgressStep>::none();
        task(progress)
    }
}

/// O `TestActivityService` de `workbenchTestServices.ts`: o selo não aparece em lugar nenhum.
pub struct TestActivityService;

impl IActivityService for TestActivityService {
    fn on_did_change_activity(&self) -> Event<ActivityChangeTarget> {
        Event::none()
    }

    fn show_view_container_activity(&self, _view_container_id: &str, _badge: IActivity) -> DisposableRef {
        none()
    }

    fn get_view_container_activities(&self, _view_container_id: &str) -> Vec<IActivity> {
        Vec::new()
    }

    fn show_view_activity(&self, _view_id: &str, _badge: IActivity) -> DisposableRef {
        none()
    }

    fn show_accounts_activity(&self, _activity: IActivity) -> DisposableRef {
        none()
    }

    fn show_global_activity(&self, _activity: IActivity) -> DisposableRef {
        none()
    }

    fn get_activity(&self, _id: &str) -> Vec<IActivity> {
        Vec::new()
    }
}

/// `mock<ILabelService>()`: nenhum membro é usado pelos casos.
pub struct MockLabelService;

impl ILabelService for MockLabelService {
    fn get_uri_label(&self, _resource: &URI, _options: &UriLabelOptions) -> String {
        panic!("getUriLabel is not a function");
    }

    fn get_uri_basename_label(&self, _resource: &URI) -> String {
        panic!("getUriBasenameLabel is not a function");
    }

    fn get_workspace_label(&self, _workspace: &WorkspaceLabelTarget, _options: Option<&WorkspaceLabelOptions>) -> String {
        panic!("getWorkspaceLabel is not a function");
    }

    fn get_host_label(&self, _scheme: &str, _authority: Option<&str>) -> String {
        panic!("getHostLabel is not a function");
    }

    fn get_host_tooltip(&self, _scheme: &str, _authority: Option<&str>) -> Option<String> {
        panic!("getHostTooltip is not a function");
    }

    fn get_separator(&self, _scheme: &str, _authority: Option<&str>) -> LabelSeparator {
        panic!("getSeparator is not a function");
    }

    fn get_uri_home(&self, _resource: &URI) -> Option<URI> {
        panic!("getUriHome is not a function");
    }

    fn register_formatter(&self, _formatter: LabelFormatter) -> DisposableRef {
        panic!("registerFormatter is not a function");
    }

    fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent> {
        Event::none()
    }

    fn register_cached_formatter(&self, _formatter: ResourceLabelFormatter) -> DisposableRef {
        panic!("registerCachedFormatter is not a function");
    }
}

/// Os serviços do `CustomTreeView` com os duplos acima.
pub fn custom_tree_view_services(extension_service: Rc<TestExtensionService>, drag_service: Rc<TreeViewsDnDService<Rc<VSDataTransfer>>>) -> CustomTreeViewServices {
    CustomTreeViewServices {
        log_service: NullLogService::shared(),
        telemetry_service: Rc::new(NullTelemetryServiceShape),
        extension_service,
        progress_service: Rc::new(TestProgressService),
        activity_service: Rc::new(TestActivityService),
        label_service: Rc::new(MockLabelService),
        tree_views_dnd_service: drag_service,
    }
}
