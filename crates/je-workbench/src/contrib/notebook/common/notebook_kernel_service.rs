//! Porte parcial de `src/vs/workbench/contrib/notebook/common/notebookKernelService.ts`: o contrato `INotebookTextModelLike`
//! de que `notebookCommon.ts` depende (`INotebookTextModel` o estende) e o que o `MainThreadNotebookKernels` usa do serviço de
//! kernels: `INotebookKernel`, `INotebookKernelChangeEvent`, `INotebookKernelDetectionTask`, `IKernelSourceActionProvider`,
//! `INotebookKernelMatchResult`, `ISelectedNotebooksChangeEvent` e o trait `INotebookKernelService`. O resto do arquivo
//! (`ISourceAction`, `INotebookProxyKernelChangeEvent`, `ProxyKernelState`, o histórico de kernels, os membros do serviço que
//! escolhem, pré-selecionam e listam kernels, as ações de origem e as tarefas de detecção como consulta) fica para a fatia do
//! serviço de kernels.
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

use je_base::async_::AsyncIterableProducer;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;

use super::notebook_common::INotebookKernelSourceAction;
use crate::api::common::ext_host_protocol::{VariablesRequestKindDto, VariablesResult};

/// `INotebookTextModelLike`.
pub trait INotebookTextModelLike {
    fn uri(&self) -> URI;
    fn notebook_type(&self) -> String;
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

/// `INotebookKernelService`, só o que o `MainThreadNotebookKernels` chama.
pub trait INotebookKernelService {
    fn on_did_change_selected_notebooks(&self) -> Event<ISelectedNotebooksChangeEvent>;
    fn register_kernel(&self, kernel: Rc<dyn INotebookKernel>) -> DisposableRef;
    fn get_matching_kernel(&self, notebook: &dyn INotebookTextModelLike) -> INotebookKernelMatchResult;
    /// `updateKernelNotebookAffinity(kernel, notebook, preference)`.
    fn update_kernel_notebook_affinity(&self, kernel: &Rc<dyn INotebookKernel>, notebook: &URI, preference: Option<i32>);
    fn register_notebook_kernel_detection_task(&self, task: Rc<dyn INotebookKernelDetectionTask>) -> DisposableRef;
    fn register_kernel_source_action_provider(&self, view_type: &str, provider: Rc<IKernelSourceActionProvider>) -> DisposableRef;
    fn notify_variables_change(&self, notebook_uri: &URI);
}
