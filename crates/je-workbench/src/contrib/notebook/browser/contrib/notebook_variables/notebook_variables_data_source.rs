//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/notebookVariables/notebookVariablesDataSource.ts`: a fonte de dados
//! da árvore de variáveis do notebook (`NotebookVariableDataSource`), que lê as variáveis do kernel selecionado e as agrupa em páginas
//! quando uma lista indexada é grande.
//!
//! Divergências:
//! - A árvore (`IAsyncDataSource<INotebookScope, INotebookVariableElement>`) ainda não tem a visão portada, e `getChildren` aceita
//!   também o `IEmptyScope`: o argumento é o [`NotebookVariablesNode`] (a união `INotebookScope | INotebookVariableElement | IEmptyScope`,
//!   discriminada por `kind`), e `hasChildren` o lê do mesmo jeito. O contrato `IAsyncDataSource` entra com a visão.
//! - `notebook: NotebookTextModel` é o `Rc<dyn INotebookTextModelLike>` (a fonte só lê o `uri` e entrega o notebook ao
//!   `getMatchingKernel`, que também recebe esse contrato).
//! - `VariablesResult` do porte tem `extension_id` obrigatório, e o `extensionId` do elemento é `Some(...)` dele.
//! - `getChildren` é assíncrona: devolve a [`Promise`] da função `async`, e o `for await` é o `for_await` do je-base. O
//!   `cancellationTokenSource` fica compartilhado (`Rc<RefCell<...>>`) com as tarefas, que leem o token no mesmo ponto do original.

use std::cell::RefCell;
use std::ops::ControlFlow;
use std::rc::Rc;

use je_base::async_::{async_fn, for_await};
use je_base::cancellation::CancellationTokenSource;
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::Promise;

use crate::api::common::ext_host_protocol::{VariablesRequestKindDto, VariablesResult};
use crate::contrib::notebook::common::notebook_kernel_service::{INotebookKernel, INotebookKernelService, INotebookTextModelLike, VARIABLE_PAGE_SIZE};

/// `INotebookScope`.
#[derive(Clone)]
pub struct INotebookScope {
    pub notebook: Rc<dyn INotebookTextModelLike>,
}

/// `INotebookVariableElement`.
#[derive(Clone)]
pub struct INotebookVariableElement {
    pub id: String,
    pub ext_host_id: u32,
    pub name: String,
    pub value: String,
    pub r#type: Option<String>,
    pub interfaces: Option<Vec<String>>,
    pub expression: Option<String>,
    pub language: Option<String>,
    pub indexed_children_count: usize,
    pub index_start: Option<usize>,
    pub has_named_children: bool,
    pub notebook: Rc<dyn INotebookTextModelLike>,
    pub extension_id: Option<String>,
}

/// `INotebookScope | INotebookVariableElement | IEmptyScope`, pelo `kind` (`'root' | 'variable' | 'empty'`).
#[derive(Clone)]
pub enum NotebookVariablesNode {
    /// `IEmptyScope`.
    Empty,
    /// `INotebookScope`.
    Root(INotebookScope),
    /// `INotebookVariableElement`.
    Variable(INotebookVariableElement),
}

/// `NotebookVariableDataSource`.
pub struct NotebookVariableDataSource {
    cancellation_token_source: Rc<RefCell<CancellationTokenSource>>,
    notebook_kernel_service: Rc<dyn INotebookKernelService>,
}

impl NotebookVariableDataSource {
    pub fn new(notebook_kernel_service: Rc<dyn INotebookKernelService>) -> Self {
        Self { cancellation_token_source: Rc::new(RefCell::new(CancellationTokenSource::default())), notebook_kernel_service }
    }

    /// `hasChildren(element)`.
    pub fn has_children(&self, element: &NotebookVariablesNode) -> bool {
        match element {
            NotebookVariablesNode::Empty => false,
            NotebookVariablesNode::Root(_) => true,
            NotebookVariablesNode::Variable(variable) => variable.has_named_children || variable.indexed_children_count > 0,
        }
    }

    /// `cancel()`.
    pub fn cancel(&self) {
        let previous = self.cancellation_token_source.replace(CancellationTokenSource::default());
        previous.cancel();
        previous.dispose();
    }

    /// `getChildren(element)`.
    pub fn get_children(&self, element: &NotebookVariablesNode) -> Promise<Vec<INotebookVariableElement>> {
        let ctx = Context { cancellation_token_source: self.cancellation_token_source.clone(), notebook_kernel_service: self.notebook_kernel_service.clone() };
        match element {
            NotebookVariablesNode::Empty => Promise::resolved(Vec::new()),
            NotebookVariablesNode::Root(scope) => {
                let notebook = scope.notebook.clone();
                async_fn(async move { ctx.get_root_variables(notebook).await })
            }
            NotebookVariablesNode::Variable(parent) => {
                let parent = parent.clone();
                async_fn(async move { ctx.get_variables(parent).await })
            }
        }
    }
}

/// O que as tarefas assíncronas da fonte compartilham com ela.
struct Context {
    cancellation_token_source: Rc<RefCell<CancellationTokenSource>>,
    notebook_kernel_service: Rc<dyn INotebookKernelService>,
}

impl Context {
    /// `getMatchingKernel(notebook).selected`, quando tem provedor de variáveis.
    fn variable_kernel(&self, notebook: &Rc<dyn INotebookTextModelLike>) -> Option<Rc<dyn INotebookKernel>> {
        self.notebook_kernel_service.get_matching_kernel(&**notebook).selected.filter(|kernel| kernel.has_variable_provider() == Some(true))
    }

    /// `getVariables(parent)`.
    async fn get_variables(&self, parent: INotebookVariableElement) -> Result<Vec<INotebookVariableElement>, je_base::errors::Error> {
        let Some(selected_kernel) = self.variable_kernel(&parent.notebook) else {
            return Ok(Vec::new());
        };

        let mut children: Vec<INotebookVariableElement> = Vec::new();
        if parent.has_named_children {
            let token = self.cancellation_token_source.borrow().token();
            let variables = selected_kernel.provide_variables(&parent.notebook.uri(), Some(parent.ext_host_id), VariablesRequestKindDto::Named, 0, token);
            for_await(&*variables, |variable| {
                children.push(create_variable_element(variable, &parent.notebook));
                Ok(ControlFlow::Continue(()))
            })
            .await?;
        }
        if parent.indexed_children_count > 0 {
            let child_nodes = self.get_indexed_children(&parent, &selected_kernel).await?;
            children.extend(child_nodes);
        }

        Ok(children)
    }

    /// `getIndexedChildren(parent, kernel)`.
    async fn get_indexed_children(&self, parent: &INotebookVariableElement, kernel: &Rc<dyn INotebookKernel>) -> Result<Vec<INotebookVariableElement>, je_base::errors::Error> {
        let mut child_nodes: Vec<INotebookVariableElement> = Vec::new();

        if parent.indexed_children_count > VARIABLE_PAGE_SIZE {
            let nested_page_size = (parent.indexed_children_count as f64 / VARIABLE_PAGE_SIZE as f64).max(100.0).floor() as usize;

            let indexed_child_count_limit = 1_000_000;
            let mut start = parent.index_start.unwrap_or(0);
            let last = start + parent.indexed_children_count.min(indexed_child_count_limit);
            while start < last {
                let end = (start + nested_page_size).min(last);

                child_nodes.push(INotebookVariableElement {
                    notebook: parent.notebook.clone(),
                    id: format!("{}{}", parent.id, start),
                    ext_host_id: parent.ext_host_id,
                    name: format!("[{}..{}]", start, end - 1),
                    value: String::new(),
                    r#type: None,
                    interfaces: None,
                    expression: None,
                    language: None,
                    indexed_children_count: end - start,
                    index_start: Some(start),
                    has_named_children: false,
                    extension_id: None,
                });
                start += nested_page_size;
            }

            if parent.indexed_children_count > indexed_child_count_limit {
                child_nodes.push(INotebookVariableElement {
                    notebook: parent.notebook.clone(),
                    id: format!("{}{}", parent.id, last + 1),
                    ext_host_id: parent.ext_host_id,
                    name: localize("notebook.indexedChildrenLimitReached", "Display limit reached", &[]),
                    value: String::new(),
                    r#type: None,
                    interfaces: None,
                    expression: None,
                    language: None,
                    indexed_children_count: 0,
                    index_start: None,
                    has_named_children: false,
                    extension_id: None,
                });
            }
        } else if parent.indexed_children_count > 0 {
            let token = self.cancellation_token_source.borrow().token();
            let variables = kernel.provide_variables(&parent.notebook.uri(), Some(parent.ext_host_id), VariablesRequestKindDto::Indexed, parent.index_start.unwrap_or(0), token);

            for_await(&*variables, |variable| {
                child_nodes.push(create_variable_element(variable, &parent.notebook));
                Ok(if child_nodes.len() >= VARIABLE_PAGE_SIZE { ControlFlow::Break(()) } else { ControlFlow::Continue(()) })
            })
            .await?;
        }
        Ok(child_nodes)
    }

    /// `getRootVariables(notebook)`.
    async fn get_root_variables(&self, notebook: Rc<dyn INotebookTextModelLike>) -> Result<Vec<INotebookVariableElement>, je_base::errors::Error> {
        let Some(selected_kernel) = self.variable_kernel(&notebook) else {
            return Ok(Vec::new());
        };
        let token = self.cancellation_token_source.borrow().token();
        let variables = selected_kernel.provide_variables(&notebook.uri(), None, VariablesRequestKindDto::Named, 0, token);
        let mut var_elements: Vec<INotebookVariableElement> = Vec::new();
        for_await(&*variables, |variable| {
            var_elements.push(create_variable_element(variable, &notebook));
            Ok(ControlFlow::Continue(()))
        })
        .await?;
        Ok(var_elements)
    }
}

/// `createVariableElement(variable, notebook)`.
fn create_variable_element(variable: VariablesResult, notebook: &Rc<dyn INotebookTextModelLike>) -> INotebookVariableElement {
    INotebookVariableElement {
        id: variable.id.to_string(),
        ext_host_id: variable.id,
        name: variable.name,
        value: variable.value,
        r#type: variable.r#type,
        interfaces: variable.interfaces,
        expression: variable.expression,
        language: variable.language,
        indexed_children_count: variable.indexed_children_count,
        index_start: None,
        has_named_children: variable.has_named_children,
        notebook: notebook.clone(),
        extension_id: Some(variable.extension_id),
    }
}
