//! Porte parcial de `src/vs/workbench/contrib/notebook/common/notebookKernelService.ts`: `INotebookTextModelLike`
//! (de que `notebookCommon.ts` depende, `INotebookTextModel` o estende), `INotebookKernel`, `INotebookKernelChangeEvent`,
//! `INotebookKernelDetectionTask`, `ISourceAction`, `INotebookSourceActionChangeEvent`, `IKernelSourceActionProvider`,
//! `INotebookKernelMatchResult`, `ISelectedNotebooksChangeEvent`, `INotebookKernelService` e
//! `INotebookKernelHistoryService`. Só `INotebookProxyKernelChangeEvent` e `ProxyKernelState` ficam para a fatia dos
//! kernels proxy.
//!
//! Divergências:
//! - `INotebookTextModelLike` é uma interface estrutural (`{ uri: URI; notebookType: string }`); aqui é um trait, e
//!   `INotebookTextModel` o tem como supertrait.
//! - `INotebookKernel` tem propriedades mutáveis; aqui são métodos de leitura (a implementação guarda o estado em células).
//!   `implementsInterrupt?`, `implementsExecutionOrder?` e `hasVariableProvider?` são `Option<bool>`.
//! - `INotebookKernelChangeEvent` (`label?: true`, ...) é um struct de `bool`.
//! - `VariablesResult` e o `'named' | 'indexed'` são os de `ext_host_protocol` (no original `VariablesResult` mora aqui);
//!   `provideVariables` devolve o `AsyncIterableProducer`.
//! - `IKernelSourceActionProvider` é um struct com o fecho `provideKernelSourceActions`; `onDidChangeSourceActions?` é `Option`.
//! - `updateKernelNotebookAffinity(kernel, notebook, preference: number | undefined)` recebe `Option<i32>`.

use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::async_::AsyncIterableProducer;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_instantiation::service_id;
use je_platform::contextkey::contextkey::IContextKeyService;

use super::notebook_common::INotebookKernelSourceAction;
use crate::api::common::ext_host_protocol::{VariablesRequestKindDto, VariablesResult};

/// `INotebookTextModelLike`.
pub trait INotebookTextModelLike {
    fn uri(&self) -> URI;
    fn notebook_type(&self) -> String;
}

/// O literal `{ uri, notebookType }` do original, como [`INotebookTextModelLike`].
#[derive(Clone, Debug)]
pub struct NotebookTextModelLike {
    pub uri: URI,
    pub notebook_type: String,
}

impl INotebookTextModelLike for NotebookTextModelLike {
    fn uri(&self) -> URI {
        self.uri.clone()
    }

    fn notebook_type(&self) -> String {
        self.notebook_type.clone()
    }
}

/// `ISourceAction`: `execution` é a promessa da execução em curso (`Promise<void> | undefined`).
pub trait ISourceAction {
    fn action(&self) -> ActionRef;
    fn on_did_change_state(&self) -> Event<()>;
    fn is_primary(&self) -> bool;
    fn execution(&self) -> Option<Promise<()>>;
    fn run_action(&self) -> Promise<()>;
}

/// `INotebookSourceActionChangeEvent`.
#[derive(Clone, Debug)]
pub struct INotebookSourceActionChangeEvent {
    pub notebook: Option<URI>,
    pub view_type: String,
}

/// `INotebookKernelChangeEvent`: as propriedades que mudaram.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct INotebookKernelChangeEvent {
    pub label: bool,
    pub description: bool,
    pub detail: bool,
    pub supported_languages: bool,
    pub has_execution_order: bool,
    pub has_interrupt_handler: bool,
    pub has_variable_provider: bool,
}

/// `INotebookKernel`.
pub trait INotebookKernel {
    fn id(&self) -> String;
    fn view_type(&self) -> String;
    fn on_did_change(&self) -> Event<INotebookKernelChangeEvent>;
    fn extension(&self) -> ExtensionIdentifier;

    fn local_resource_root(&self) -> URI;
    fn preload_uris(&self) -> Vec<URI>;
    fn preload_provides(&self) -> Vec<String>;

    fn label(&self) -> String;
    fn description(&self) -> Option<String>;
    fn detail(&self) -> Option<String>;
    fn supported_languages(&self) -> Vec<String>;
    fn implements_interrupt(&self) -> Option<bool>;
    fn implements_execution_order(&self) -> Option<bool>;
    fn has_variable_provider(&self) -> Option<bool>;

    fn execute_notebook_cells_request(&self, uri: &URI, cell_handles: Vec<i64>) -> Promise<()>;
    fn cancel_notebook_cell_execution(&self, uri: &URI, cell_handles: Vec<i64>) -> Promise<()>;

    fn provide_variables(
        &self,
        notebook_uri: &URI,
        parent_id: Option<u32>,
        kind: VariablesRequestKindDto,
        start: usize,
        token: CancellationToken,
    ) -> Rc<AsyncIterableProducer<VariablesResult>>;
}

/// `INotebookKernelDetectionTask`.
pub trait INotebookKernelDetectionTask {
    fn notebook_type(&self) -> String;
}

/// `IKernelSourceActionProvider`.
#[derive(Clone)]
pub struct IKernelSourceActionProvider {
    pub view_type: String,
    pub on_did_change_source_actions: Option<Event<()>>,
    pub provide_kernel_source_actions: Rc<dyn Fn() -> Promise<Vec<INotebookKernelSourceAction>>>,
}

/// `ISelectedNotebooksChangeEvent`.
#[derive(Clone, Debug)]
pub struct ISelectedNotebooksChangeEvent {
    pub notebook: URI,
    pub old_kernel: Option<String>,
    pub new_kernel: Option<String>,
}

/// `INotebookKernelMatchResult`.
#[derive(Clone, Default)]
pub struct INotebookKernelMatchResult {
    pub selected: Option<Rc<dyn INotebookKernel>>,
    pub suggestions: Vec<Rc<dyn INotebookKernel>>,
    pub all: Vec<Rc<dyn INotebookKernel>>,
    pub hidden: Vec<Rc<dyn INotebookKernel>>,
}

/// `INotebookKernelService`.
pub trait INotebookKernelService {
    fn on_did_add_kernel(&self) -> Event<Rc<dyn INotebookKernel>>;
    fn on_did_remove_kernel(&self) -> Event<Rc<dyn INotebookKernel>>;
    fn on_did_change_selected_notebooks(&self) -> Event<ISelectedNotebooksChangeEvent>;
    fn on_did_change_notebook_affinity(&self) -> Event<()>;
    fn on_did_notebook_variables_update(&self) -> Event<URI>;
    fn register_kernel(&self, kernel: Rc<dyn INotebookKernel>) -> DisposableRef;

    fn get_matching_kernel(&self, notebook: &dyn INotebookTextModelLike) -> INotebookKernelMatchResult;

    /// Returns the selected or only available kernel.
    fn get_selected_or_suggested_kernel(&self, notebook: &dyn INotebookTextModelLike) -> Option<Rc<dyn INotebookKernel>>;

    /// Bind a notebook document to a kernel. A notebook is only bound to one kernel but a kernel can be bound to many
    /// notebooks (depending on its configuration). `None` desfaz a ligação (a implementação aceita `undefined`).
    fn select_kernel_for_notebook(&self, kernel: Option<&Rc<dyn INotebookKernel>>, notebook: &dyn INotebookTextModelLike);

    /// Set the kernel that a notebook should use when it starts up.
    fn preselect_kernel_for_notebook(&self, kernel: &Rc<dyn INotebookKernel>, notebook: &dyn INotebookTextModelLike);

    /// `updateKernelNotebookAffinity(kernel, notebook, preference)`: higher values win, `None` removes the preference.
    fn update_kernel_notebook_affinity(&self, kernel: &Rc<dyn INotebookKernel>, notebook: &URI, preference: Option<i32>);

    //#region Kernel detection tasks
    fn on_did_change_kernel_detection_tasks(&self) -> Event<String>;
    fn register_notebook_kernel_detection_task(&self, task: Rc<dyn INotebookKernelDetectionTask>) -> DisposableRef;
    fn get_kernel_detection_tasks(&self, notebook: &dyn INotebookTextModelLike) -> Vec<Rc<dyn INotebookKernelDetectionTask>>;
    //#endregion

    //#region Kernel source actions
    fn on_did_change_source_actions(&self) -> Event<INotebookSourceActionChangeEvent>;
    fn get_source_actions(&self, notebook: &dyn INotebookTextModelLike, context_key_service: Option<Rc<dyn IContextKeyService>>) -> Vec<Rc<dyn ISourceAction>>;
    fn get_running_source_actions(&self, notebook: &dyn INotebookTextModelLike) -> Vec<Rc<dyn ISourceAction>>;
    fn register_kernel_source_action_provider(&self, view_type: &str, provider: Rc<IKernelSourceActionProvider>) -> DisposableRef;
    fn get_kernel_source_actions2(&self, notebook: &dyn INotebookTextModelLike) -> Promise<Vec<INotebookKernelSourceAction>>;
    //#endregion

    fn notify_variables_change(&self, notebook_uri: &URI);
}

service_id!(pub I_NOTEBOOK_KERNEL_SERVICE: dyn INotebookKernelService = "INotebookKernelService");

/// `INotebookKernelHistoryService`: `getKernels(notebook)` devolve `{ selected, all }`, o mesmo formato do
/// [`INotebookKernelMatchResult`] nos campos que ele lê.
pub trait INotebookKernelHistoryService {
    fn get_kernels(&self, notebook: &dyn INotebookTextModelLike) -> INotebookKernelHistory;
    fn add_most_recent_kernel(&self, kernel: &Rc<dyn INotebookKernel>);
}

service_id!(pub I_NOTEBOOK_KERNEL_HISTORY_SERVICE: dyn INotebookKernelHistoryService = "INotebookKernelHistoryService");

/// `{ selected: INotebookKernel | undefined; all: INotebookKernel[] }`.
#[derive(Clone, Default)]
pub struct INotebookKernelHistory {
    pub selected: Option<Rc<dyn INotebookKernel>>,
    pub all: Vec<Rc<dyn INotebookKernel>>,
}
