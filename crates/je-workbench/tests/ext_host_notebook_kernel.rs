//! Porte de `src/vs/workbench/api/test/browser/extHostNotebookKernel.test.ts` (suíte `NotebookKernel`), caso a caso e na mesma ordem.
//!
//! PORTADOS (7 de 8 `test(`): todos, menos o caso abaixo.
//!
//! FORA, e por quê (não vira `#[test]` enquanto não puder ser fiel):
//! - `create/dispose kernel`: abre com `assert.throws(() => kernel.id = 'dd')` e `assert.throws(() => kernel.notebookType = 'dd')`,
//!   que conferem que atribuir às propriedades somente leitura do `vscode.NotebookController` lança. O [`NotebookController`] do
//!   porte não tem setter de `id` nem de `notebook_type` (a atribuição nem compila), então as duas asserções não têm
//!   correspondente, e o resto do caso (valores do DTO e a remoção ao descartar) não pode ser portado sem elas.
//!
//! Notas de tradução:
//! - `new TestRPCProtocol()` é o [`TestMainContext`] de `support/ext_host_workspace_support.rs`, montado por
//!   `support/ext_host_notebook_support.rs` (ver as notas de lá); o `MainThreadNotebookKernels` do `rpcProtocol.set(...)` é o
//!   [`MockMainThreadNotebookKernels`], registrado antes de o `ExtHostNotebookKernels` ser criado. `ExtHostConsumerFileSystem` e
//!   `ExtHostSearch` não existem no porte, e o `IExtHostInitDataService` do `ExtHostNotebookKernels` também não (ver
//!   `ext_host_notebook_kernels.rs`).
//! - O `$updateKernel` do dublê guarda o DTO inteiro: o `{ ...kernelData.get(handle), ...data }` do original recebe sempre o
//!   objeto completo do controlador. `cellExecuteCreate` e `cellExecuteComplete` são preenchidos e nunca lidos, como no original.
//! - `assert.throws(() => kernel.createNotebookCellExecution(cell))` é o [`Result`] com erro de `create_notebook_cell_execution`.
//! - `rpcProtocol.sync()` é o `sync()` do [`TestMainContext`]. Os casos assíncronos rodam em tempo real (`run_with_real_timers`):
//!   o coletor de atualizações do original espera 10 ms de verdade.
//! - O ouvinte `async` de `onCancellationRequested` é uma função `async_fn` cujo resultado é descartado, como a promessa que o
//!   original ignora. `assert.ok(notebook.apiNotebook === _notebook)` é `NotebookDocument::ptr_eq`.
//! - `Array.from(edit.outputs[0].items[0].valueBytes.buffer)` e `TextEncoder().encode(...)` são `value_bytes.to_vec()` e os bytes
//!   do texto.

mod support;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{Barrier, async_fn};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_notebook_document::{ExtHostNotebookDocument, NotebookDocument};
use je_workbench::api::common::ext_host_notebook_documents::ExtHostNotebookDocuments;
use je_workbench::api::common::ext_host_notebook_kernels::{ExtHostNotebookKernels, NotebookController};
use je_workbench::api::common::ext_host_protocol::{
    ExtHostNotebookDocumentsShape, ExtHostNotebookKernelsShape, ICellExecuteUpdateDto, ICellExecutionCompleteDto, INotebookKernelDto2, MainContext,
    MainThreadNotebookKernelsShape, NotebookCellsChangedEventDto, NotebookRawContentEventDto, SerializableObjectWithBuffers, VariablesResult,
};
use je_workbench::api::common::ext_host_types as types;
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_notebook_support::common::run_with_real_timers;
use support::ext_host_notebook_support::{LeakGuard, NotebookHost, add_notebook_and_editor, ensure_no_disposables_are_leaked_in_test_suite, new_notebook_host};

/// `new class extends mock<MainThreadNotebookKernelsShape>() { ... }`: só o que o original sobrescreve responde, o resto falha com
/// `panic!`.
#[derive(Default)]
struct MockMainThreadNotebookKernels {
    kernel_data: RefCell<HashMap<u32, INotebookKernelDto2>>,
    #[allow(dead_code)]
    cell_execute_create: RefCell<Vec<(URI, i64)>>,
    cell_execute_updates: RefCell<Vec<ICellExecuteUpdateDto>>,
    #[allow(dead_code)]
    cell_execute_complete: RefCell<Vec<ICellExecutionCompleteDto>>,
}

impl MockMainThreadNotebookKernels {
    /// `const [first] = kernelData.values()`.
    fn first_kernel(&self) -> INotebookKernelDto2 {
        self.kernel_data.borrow().values().next().cloned().expect("há um kernel")
    }
}

impl IDisposable for MockMainThreadNotebookKernels {
    fn dispose(&self) {}
}

impl MainThreadNotebookKernelsShape for MockMainThreadNotebookKernels {
    fn rpc_post_message(&self, _handle: u32, _editor_id: Option<&str>, _message: Dynamic) -> Promise<bool> {
        panic!("$postMessage is not a function")
    }

    fn rpc_add_kernel(&self, handle: u32, data: INotebookKernelDto2) -> Promise<()> {
        self.kernel_data.borrow_mut().insert(handle, data);
        Promise::resolved(())
    }

    fn rpc_update_kernel(&self, handle: u32, data: INotebookKernelDto2) {
        assert!(self.kernel_data.borrow().contains_key(&handle));
        self.kernel_data.borrow_mut().insert(handle, data);
    }

    fn rpc_remove_kernel(&self, handle: u32) {
        self.kernel_data.borrow_mut().remove(&handle);
    }

    fn rpc_update_notebook_priority(&self, _handle: u32, _uri: &URI, _value: Option<types::NotebookControllerAffinity2>) {
        panic!("$updateNotebookPriority is not a function")
    }

    fn rpc_create_execution(&self, _handle: u32, _controller_id: &str, uri: &URI, cell_handle: i64) {
        self.cell_execute_create.borrow_mut().push((uri.clone(), cell_handle));
    }

    fn rpc_update_execution(&self, _handle: u32, data: SerializableObjectWithBuffers<Vec<ICellExecuteUpdateDto>>) {
        self.cell_execute_updates.borrow_mut().extend(data.value);
    }

    fn rpc_complete_execution(&self, _handle: u32, data: SerializableObjectWithBuffers<ICellExecutionCompleteDto>) {
        self.cell_execute_complete.borrow_mut().push(data.value);
    }

    fn rpc_create_notebook_execution(&self, _handle: u32, _controller_id: &str, _uri: &URI) {
        panic!("$createNotebookExecution is not a function")
    }

    fn rpc_begin_notebook_execution(&self, _handle: u32) {
        panic!("$beginNotebookExecution is not a function")
    }

    fn rpc_complete_notebook_execution(&self, _handle: u32) {
        panic!("$completeNotebookExecution is not a function")
    }

    fn rpc_add_kernel_detection_task(&self, _handle: u32, _notebook_type: &str) -> Promise<()> {
        panic!("$addKernelDetectionTask is not a function")
    }

    fn rpc_remove_kernel_detection_task(&self, _handle: u32) {
        panic!("$removeKernelDetectionTask is not a function")
    }

    fn rpc_add_kernel_source_action_provider(&self, _handle: u32, _event_handle: u32, _notebook_type: &str) -> Promise<()> {
        panic!("$addKernelSourceActionProvider is not a function")
    }

    fn rpc_remove_kernel_source_action_provider(&self, _handle: u32, _event_handle: u32) {
        panic!("$removeKernelSourceActionProvider is not a function")
    }

    fn rpc_emit_notebook_kernel_source_actions_change_event(&self, _event_handle: u32) {
        panic!("$emitNotebookKernelSourceActionsChangeEvent is not a function")
    }

    fn rpc_receive_variable(&self, _request_id: &str, _variable: VariablesResult) {
        panic!("$receiveVariable is not a function")
    }

    fn rpc_variables_updated(&self, _notebook_uri: &URI) {
        panic!("$variablesUpdated is not a function")
    }
}

/// O que o `setup` da suíte monta: o host de notebook, o notebook com o editor ativo, o dublê de kernels e o `ExtHostNotebookKernels`.
struct KernelEnv {
    host: NotebookHost,
    notebook_documents: Rc<ExtHostNotebookDocuments>,
    notebook: Rc<ExtHostNotebookDocument>,
    state: Rc<MockMainThreadNotebookKernels>,
    kernels: Rc<ExtHostNotebookKernels>,
    guard: LeakGuard,
}

impl KernelEnv {
    /// `setup(async function () { ... })` com o `ensureNoDisposablesAreLeakedInTestSuite()` já ativo.
    fn new() -> KernelEnv {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let notebook_uri = URI::parse("test:///notebook.file", false);

        let host = new_notebook_host();
        let state = Rc::new(MockMainThreadNotebookKernels::default());
        host.rpc.set(&MainContext::MAIN_THREAD_NOTEBOOK_KERNELS, state.clone() as Rc<dyn MainThreadNotebookKernelsShape>);

        let notebook_documents = ExtHostNotebookDocuments::new(host.notebooks.clone());

        add_notebook_and_editor(&host.notebooks, &notebook_uri);

        let notebook = host.notebooks.notebook_documents()[0].clone();

        guard.add_rc(notebook.clone());
        guard.add_rc(host.documents.clone());

        let kernels = ExtHostNotebookKernels::new(host.rpc.as_ref(), host.notebooks.clone(), &host.commands, NullLogService::shared())
            .expect("new ExtHostNotebookKernels");
        KernelEnv { host, notebook_documents, notebook, state, kernels, guard }
    }

    /// `extHostNotebookKernels.createNotebookController(nullExtensionDescription, 'foo', '*', 'Foo')`.
    fn create_controller(&self) -> NotebookController {
        self.kernels
            .create_notebook_controller(&null_extension_description(), "foo", "*", "Foo", None, None)
            .expect("createNotebookController")
    }

    /// `disposables.add(extHostNotebookKernels.createNotebookController(...))`.
    fn create_disposable_controller(&self) -> NotebookController {
        let kernel = self.create_controller();
        self.guard.add(Rc::new(kernel.clone()));
        kernel
    }
}

/// `new NotebookCellOutput([NotebookCellOutputItem.text(text)])`.
fn text_output(text: &str) -> types::NotebookCellOutput {
    types::NotebookCellOutput::new(vec![types::NotebookCellOutputItem::text(text, None)], None, None)
}

/// O fim dos casos `set outputs on cancel` e `set outputs on interrupt`: houve atualizações, e a de saída substituiu tudo pelo texto.
fn assert_output_replaced_with(cell_execute_updates: &[ICellExecuteUpdateDto], text: &str) {
    assert!(!cell_execute_updates.is_empty());

    let mut found = false;
    for edit in cell_execute_updates {
        if let ICellExecuteUpdateDto::Output(edit) = edit {
            assert_eq!(edit.append, Some(false));
            assert_eq!(edit.outputs.len(), 1);
            assert_eq!(edit.outputs[0].items.len(), 1);
            assert_eq!(edit.outputs[0].items[0].value_bytes.to_vec(), text.as_bytes().to_vec());
            found = true;
        }
    }
    assert!(found);
}

#[test]
fn update_kernel() {
    run_with_real_timers(async move {
        let env = KernelEnv::new();
        let kernel = env.create_disposable_controller();

        env.host.rpc.sync().await.unwrap();

        let first = env.state.first_kernel();
        assert_eq!(first.id, "nullExtensionDescription/foo");
        assert_eq!(first.label, "Foo");

        kernel.set_label(Some("Far"));
        assert_eq!(kernel.label(), "Far");

        env.host.rpc.sync().await.unwrap();
        let first = env.state.first_kernel();
        assert_eq!(first.id, "nullExtensionDescription/foo");
        assert_eq!(first.label, "Far");
    });
}

#[test]
fn execute_simple_create_notebook_cell_execution() {
    let env = KernelEnv::new();
    let kernel = env.create_disposable_controller();

    env.kernels.rpc_accept_notebook_association(0, &env.notebook.uri, true);

    let cell1 = env.notebook.api_notebook().cell_at(0.0).unwrap();
    let task = kernel.create_notebook_cell_execution(&cell1).unwrap();
    task.start(None).unwrap();
    task.end(None, None, None).unwrap();
}

#[test]
fn create_notebook_cell_execution_must_be_selected_associated() {
    let env = KernelEnv::new();
    let kernel = env.create_disposable_controller();
    assert!(kernel.create_notebook_cell_execution(&env.notebook.api_notebook().cell_at(0.0).unwrap()).is_err());

    env.kernels.rpc_accept_notebook_association(0, &env.notebook.uri, true);
    let execution = kernel.create_notebook_cell_execution(&env.notebook.api_notebook().cell_at(0.0).unwrap()).unwrap();
    execution.end(Some(true), None, None).unwrap();
}

#[test]
fn create_notebook_cell_execution_cell_must_be_alive() {
    let env = KernelEnv::new();
    let kernel = env.create_disposable_controller();

    let cell1 = env.notebook.api_notebook().cell_at(0.0).unwrap();

    env.kernels.rpc_accept_notebook_association(0, &env.notebook.uri, true);
    env.notebook_documents.rpc_accept_model_changed(
        &env.notebook.uri,
        SerializableObjectWithBuffers::new(NotebookCellsChangedEventDto {
            version_id: 12,
            raw_events: vec![NotebookRawContentEventDto::ModelChange { changes: vec![(0, env.notebook.api_notebook().cell_count(), Vec::new())] }],
        }),
        true,
        None,
    );

    assert_eq!(cell1.index(), -1.0);

    assert!(kernel.create_notebook_cell_execution(&cell1).is_err());
}

#[test]
fn interrupt_handler_cancellation() {
    run_with_real_timers(async move {
        let env = KernelEnv::new();
        let interrupt_call_count = Rc::new(Cell::new(0u32));
        let token_cancel_count = Rc::new(Cell::new(0u32));

        let kernel = env.create_disposable_controller();
        kernel.set_interrupt_handler(Some(Rc::new({
            let interrupt_call_count = interrupt_call_count.clone();
            move |_notebook: NotebookDocument| {
                interrupt_call_count.set(interrupt_call_count.get() + 1);
                MaybePromise::Value(())
            }
        })));
        env.kernels.rpc_accept_notebook_association(0, &env.notebook.uri, true);

        let cell1 = env.notebook.api_notebook().cell_at(0.0).unwrap();

        let task = kernel.create_notebook_cell_execution(&cell1).unwrap();
        env.guard.add(task.token().on_cancellation_requested().listen({
            let token_cancel_count = token_cancel_count.clone();
            move |_| token_cancel_count.set(token_cancel_count.get() + 1)
        }));

        env.kernels.rpc_cancel_cells(0, &env.notebook.uri, vec![0]).await.unwrap();
        assert_eq!(interrupt_call_count.get(), 1);
        assert_eq!(token_cancel_count.get(), 0);

        env.kernels.rpc_cancel_cells(0, &env.notebook.uri, vec![0]).await.unwrap();
        assert_eq!(interrupt_call_count.get(), 2);
        assert_eq!(token_cancel_count.get(), 0);

        // should cancelling the cells end the execution task?
        task.end(Some(false), None, None).unwrap();
    });
}

#[test]
fn set_outputs_on_cancel() {
    run_with_real_timers(async move {
        let env = KernelEnv::new();
        let kernel = env.create_disposable_controller();
        env.kernels.rpc_accept_notebook_association(0, &env.notebook.uri, true);

        let cell1 = env.notebook.api_notebook().cell_at(0.0).unwrap();
        let task = kernel.create_notebook_cell_execution(&cell1).unwrap();
        task.start(None).unwrap();

        let b = Barrier::new();

        env.guard.add(task.token().on_cancellation_requested().listen({
            let task = task.clone();
            let b = b.clone();
            move |_| {
                let task = task.clone();
                let b = b.clone();
                let _ = async_fn(async move {
                    task.replace_output(vec![text_output("canceled")], None)?.await?;
                    task.end(Some(true), None, None)?;
                    b.open(); // use barrier to signal that cancellation has happened
                    Ok(())
                });
            }
        }));

        env.state.cell_execute_updates.borrow_mut().clear();
        env.kernels.rpc_cancel_cells(0, &env.notebook.uri, vec![0]).await.unwrap();

        b.wait().await.unwrap();

        assert_output_replaced_with(&env.state.cell_execute_updates.borrow(), "canceled");
    });
}

#[test]
fn set_outputs_on_interrupt() {
    run_with_real_timers(async move {
        let env = KernelEnv::new();
        let kernel = env.create_controller();
        env.kernels.rpc_accept_notebook_association(0, &env.notebook.uri, true);

        let cell1 = env.notebook.api_notebook().cell_at(0.0).unwrap();
        let task = kernel.create_notebook_cell_execution(&cell1).unwrap();
        task.start(None).unwrap();

        kernel.set_interrupt_handler(Some(Rc::new({
            let task = task.clone();
            let notebook = env.notebook.clone();
            move |notebook_arg: NotebookDocument| {
                assert!(NotebookDocument::ptr_eq(&notebook.api_notebook(), &notebook_arg));
                let task = task.clone();
                MaybePromise::Promise(async_fn(async move {
                    task.replace_output(vec![text_output("interrupted")], None)?.await?;
                    task.end(Some(true), None, None)?;
                    Ok(())
                }))
            }
        })));

        env.state.cell_execute_updates.borrow_mut().clear();
        env.kernels.rpc_cancel_cells(0, &env.notebook.uri, vec![0]).await.unwrap();

        assert_output_replaced_with(&env.state.cell_execute_updates.borrow(), "interrupted");
    });
}
