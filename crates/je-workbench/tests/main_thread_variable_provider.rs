//! Porte de `src/vs/workbench/api/test/browser/mainThreadVariableProvider.test.ts` (suíte `MainThreadNotebookKernelVariableProvider`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o `DisposableStore` dele.
//!   O `setup` assíncrono do mocha é o [`setup`] no começo do corpo de cada caso, que roda em `run_with_faked_timers`; o `teardown`
//!   vazio do original não tem correspondente.
//! - `mock<ExtHostNotebookKernelsShape>()` com `override async $provideVariables(...)` é o [`VariablesProxy`]: só o
//!   `rpc_provide_variables` existe, e os outros membros falham. O `variables: (VariablesResult | variableGetter)[]` é a lista
//!   de [`VariableEntry`] e `mainThreadKernels.instance.$receiveVariable` lê o `mainThreadKernels` que o `setup` guarda depois de criar
//!   o objeto (o fecho do original o captura por referência).
//! - `SingleProxyRPCProtocol(proxy)` é o `single_proxy_rpc_protocol` do suporte (a forma é `ExtHostNotebookKernelsShape`).
//! - `new Promise(resolve => setTimeout(resolve, 5))` é o `common::timeout`, que usa o relógio virtual.
//! - `for await (const variable of variables)` é o `for_await` de `je_base::async_`; `assert.ok(expectedIds[varIx], 'more variables
//!   than expected')` confere que o item existe e não é zero (o `ok` do original é sobre a verdade do valor).
//! - `assert.ok(kernel, 'Kernel should be registered')` é o `expect`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::ops::ControlFlow;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::{AsyncIterableProducer, async_fn, for_await};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::resources::host_platform;
use je_workbench::api::common::ext_host_protocol::{ExtHostNotebookKernelsShape, MainThreadNotebookKernelsShape, VariablesRequestKindDto, VariablesResult};
use je_workbench::contrib::notebook::common::notebook_common::INotebookKernelSourceAction;
use je_workbench::contrib::notebook::common::notebook_kernel_service::INotebookKernel;
use support::test_main_thread_notebook_kernels::TestMainThreadNotebookKernels;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

/// `VariablesResult | variableGetter`.
#[derive(Clone)]
enum VariableEntry {
    Value(VariablesResult),
    Getter(Rc<dyn Fn() -> Promise<VariablesResult>>),
}

type Variables = Rc<RefCell<Vec<VariableEntry>>>;
type MainThreadKernels = Rc<RefCell<Option<Rc<TestMainThreadNotebookKernels>>>>;

/// O `mock<ExtHostNotebookKernelsShape>()` com o `$provideVariables` sobrescrito.
struct VariablesProxy {
    variables: Variables,
    main_thread_kernels: MainThreadKernels,
}

impl ExtHostNotebookKernelsShape for VariablesProxy {
    fn rpc_accept_notebook_association(&self, _handle: u32, _uri: &URI, _value: bool) {
        unreachable!("TypeError: proxy.$acceptNotebookAssociation is not a function")
    }

    fn rpc_execute_cells(&self, _handle: u32, _uri: &URI, _handles: Vec<i64>) -> Promise<()> {
        unreachable!("TypeError: proxy.$executeCells is not a function")
    }

    fn rpc_cancel_cells(&self, _handle: u32, _uri: &URI, _handles: Vec<i64>) -> Promise<()> {
        unreachable!("TypeError: proxy.$cancelCells is not a function")
    }

    fn rpc_accept_kernel_message_from_renderer(&self, _handle: u32, _editor_id: &str, _message: Dynamic) {
        unreachable!("TypeError: proxy.$acceptKernelMessageFromRenderer is not a function")
    }

    fn rpc_provide_kernel_source_actions(&self, _handle: u32, _token: CancellationToken) -> Promise<Vec<INotebookKernelSourceAction>> {
        unreachable!("TypeError: proxy.$provideKernelSourceActions is not a function")
    }

    fn rpc_provide_variables(
        &self,
        _handle: u32,
        request_id: &str,
        _notebook_uri: &URI,
        _parent_id: Option<u32>,
        _kind: VariablesRequestKindDto,
        _start: usize,
        token: CancellationToken,
    ) -> Promise<()> {
        let variables = self.variables.clone();
        let main_thread_kernels = self.main_thread_kernels.clone();
        let request_id = request_id.to_owned();
        async_fn(async move {
            let entries = variables.borrow().clone();
            for variable in entries {
                if token.is_cancellation_requested() {
                    return Ok(());
                }
                let result = match variable {
                    VariableEntry::Getter(getter) => getter().await?,
                    VariableEntry::Value(value) => value,
                };
                let kernels = main_thread_kernels.borrow().clone().expect("o mainThreadKernels já foi criado");
                kernels.instance().rpc_receive_variable(&request_id, result);
            }
            Ok(())
        })
    }
}

/// `setup(async function () { ... })`: devolve os kernels de teste e a lista de variáveis.
fn setup(store: &Rc<DisposableStore>) -> (Rc<TestMainThreadNotebookKernels>, Variables) {
    let variables: Variables = Rc::default();
    let main_thread_kernels: MainThreadKernels = Rc::default();
    let proxy: Rc<dyn ExtHostNotebookKernelsShape> = Rc::new(VariablesProxy { variables: variables.clone(), main_thread_kernels: main_thread_kernels.clone() });
    let ext_host_context = single_proxy_rpc_protocol(proxy);
    let kernels = store.add_rc(TestMainThreadNotebookKernels::new(&*ext_host_context));
    *main_thread_kernels.borrow_mut() = Some(kernels.clone());
    (kernels, variables)
}

/// `kernel.provideVariables(URI.file('nb.ipynb'), undefined, 'named', 0, token)`.
fn provide_variables(kernel: &Rc<dyn INotebookKernel>, token: CancellationToken) -> Rc<AsyncIterableProducer<VariablesResult>> {
    kernel.provide_variables(&URI::file("nb.ipynb", host_platform()), None, VariablesRequestKindDto::Named, 0, token)
}

/// `verifyVariables(variables, expectedIds)`.
async fn verify_variables(variables: Rc<AsyncIterableProducer<VariablesResult>>, expected_ids: &[u32]) {
    let mut var_ix = 0;

    for_await(&*variables, |variable| {
        assert!(expected_ids.get(var_ix).is_some_and(|expected| *expected != 0), "more variables than expected");
        assert_eq!(variable.id, expected_ids[var_ix]);
        var_ix += 1;
        Ok(ControlFlow::Continue(()))
    })
    .await
    .unwrap();
}

/// `createVariable(id)`.
fn create_variable(id: u32) -> VariablesResult {
    VariablesResult {
        id,
        name: format!("var{id}"),
        value: format!("{id}"),
        r#type: Some("number".to_owned()),
        interfaces: None,
        language: None,
        expression: Some(format!("var{id}")),
        has_named_children: false,
        indexed_children_count: 0,
        extension_id: "extension-id1".to_owned(),
    }
}

/// `kernel` registrado: `assert.ok(kernel, 'Kernel should be registered')`.
fn registered_kernel(main_thread_kernels: &TestMainThreadNotebookKernels) -> Rc<dyn INotebookKernel> {
    main_thread_kernels.get_kernel("test-kernel").expect("Kernel should be registered")
}

#[test]
fn get_variables_from_kernel() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let (main_thread_kernels, variables) = setup(&store);
        main_thread_kernels.add_kernel("test-kernel").await.unwrap();

        let kernel = registered_kernel(&main_thread_kernels);

        variables.borrow_mut().push(VariableEntry::Value(create_variable(1)));
        variables.borrow_mut().push(VariableEntry::Value(create_variable(2)));
        let vars = provide_variables(&kernel, CancellationToken::None);

        verify_variables(vars, &[1, 2]).await;
    });
}

#[test]
fn get_variables_twice() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let (main_thread_kernels, variables) = setup(&store);
        main_thread_kernels.add_kernel("test-kernel").await.unwrap();

        let kernel = registered_kernel(&main_thread_kernels);

        variables.borrow_mut().push(VariableEntry::Value(create_variable(1)));
        variables.borrow_mut().push(VariableEntry::Value(create_variable(2)));
        let vars = provide_variables(&kernel, CancellationToken::None);
        let vars2 = provide_variables(&kernel, CancellationToken::None);

        verify_variables(vars, &[1, 2]).await;
        verify_variables(vars2, &[1, 2]).await;
    });
}

#[test]
fn gets_all_variables_async() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let (main_thread_kernels, variables) = setup(&store);
        main_thread_kernels.add_kernel("test-kernel").await.unwrap();

        let kernel = registered_kernel(&main_thread_kernels);

        variables.borrow_mut().push(VariableEntry::Value(create_variable(1)));
        let result = create_variable(2);
        variables.borrow_mut().push(VariableEntry::Getter(Rc::new(move || {
            let result = result.clone();
            async_fn(async move {
                timeout(5).await?;
                Ok(result)
            })
        })));
        variables.borrow_mut().push(VariableEntry::Value(create_variable(3)));
        let vars = provide_variables(&kernel, CancellationToken::None);

        verify_variables(vars, &[1, 2, 3]).await;
    });
}

#[test]
fn cancel_while_getting_variables() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let (main_thread_kernels, variables) = setup(&store);
        main_thread_kernels.add_kernel("test-kernel").await.unwrap();

        let kernel = registered_kernel(&main_thread_kernels);

        variables.borrow_mut().push(VariableEntry::Value(create_variable(1)));
        let result = create_variable(2);
        variables.borrow_mut().push(VariableEntry::Getter(Rc::new(move || {
            let result = result.clone();
            async_fn(async move {
                timeout(50).await?;
                Ok(result)
            })
        })));
        variables.borrow_mut().push(VariableEntry::Value(create_variable(3)));
        let cancellation = CancellationTokenSource::new(None);
        let vars = provide_variables(&kernel, cancellation.token());
        cancellation.cancel();

        verify_variables(vars, &[1, 2]).await;
    });
}
