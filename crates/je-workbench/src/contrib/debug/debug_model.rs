//! Porte de `src/vs/workbench/contrib/debug/common/debugModel.ts`: o modelo de depuração (`DebugModel`), as threads, os
//! quadros de pilha, os escopos, as variáveis e as expressões, a região de memória e o armazenamento de breakpoints.
//! Os breakpoints estão em [`breakpoints`](super::breakpoints) (reexportados aqui).
//!
//! Notas de tradução:
//! - **Assíncrono.** Os métodos `async` do original são [`async_fn`](je_base::async_::async_fn) sobre `Rc` clonados (a
//!   mesma ordem de `await`, logo a mesma contagem de microtarefas); os que dependem de `this` assíncrono recebem
//!   `self: &Rc<Self>`.
//! - **Herança de `ExpressionContainer`.** [`Expression`], [`Variable`] e [`Scope`] guardam um `Rc<ExpressionContainer>` em
//!   `base` e delegam. O `this` que o contêiner passa como pai dos filhos é o `owner` (a `Variable`/`Expression` dona),
//!   guardado como `Weak`; sem dono, é o próprio contêiner. `adoptLazyResponse` (o único gancho virtual) é a flag
//!   `adopt_evaluate_name` de [`ExpressionContainer`] (só a `Variable` a liga).
//! - **`Thread` e `getCallStack`.** Os testes do original estendem `Thread` para trocar `getCallStack`; aqui é
//!   [`Thread::set_call_stack`] (grava a lista que `getCallStack` devolve). Para o `refreshTopOfCallstack`, que os testes
//!   alimentam com uma thread falsa, o parâmetro é o trait [`CallStackThread`] (que a `Thread` implementa).
//! - **Ciclos.** `StackFrame.thread` e `Thread.callStack` se referenciam por `Rc` (no original o coletor de lixo resolve);
//!   o ciclo só vaza memória, nenhum `IDisposable`.
//! - **`StackFrame.range`** é `Option<Range>`: os casos do original passam `undefined` e `Range.equalsRange` trata
//!   `undefined` (igual a `undefined`, diferente de qualquer `Range`), que é o `==` de `Option`.
//! - **`DebugStorage`** é portado inteiro (carrega do `IStorageService` do workspace, recarrega nas mudanças externas e
//!   grava). O `observableValue` + `autorun` são as listas com um emissor de mudança que o [`DebugModel`] escuta. As
//!   URIs dos breakpoints são gravadas como o `URI#toJSON` (`$mid` e componentes, sem os caches).
//! - Ficam de fora `VisualizedExpression` (precisa de `getVisualizedChildren`/`editTreeItem` do `IDebugVisualizerService`, que
//!   o contrato portado não tem), `StackFrame.openInEditor` (precisa do `IEditorService.openEditor` com opções e do
//!   `DisassemblyViewInput`, não portados) e o `DisassemblyView` em geral.
//! - `decodeBase64` do porte não falha: o ramo `catch` de `MemoryRegion.read` ("Invalid base64 data") é inalcançável.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::arrays::sort_by_compare;
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, decode_base64, encode_base64};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::event::{Emitter, Event};
use je_base::hash::string_hash;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::{URI, UriComponents};
use je_base::uuid::generate_uuid;
use je_base::JsString;
use je_editor::range::Range;
use je_platform::log::LogService;
use je_platform::resources::ext_uri;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_storage::StorageValue;
use serde_json::{Value, json};

pub use super::breakpoints::{
    BaseBreakpoint, Breakpoint, BreakpointData, DapBreakpoint, DataBreakpoint, DataBreakpointOptions, Enablement, ExceptionBreakpoint,
    ExceptionBreakpointOptions, ExceptionBreakpointsFilter, FunctionBreakpoint, FunctionBreakpointOptions, IBaseBreakpointOptions,
    IBreakpointOptions, IBreakpointSessionData, IDirtyChecker, InstructionBreakpoint, InstructionBreakpointOptions,
    to_breakpoint_session_data,
};
use super::debug::IThread;
use super::debug::{IStackFrame, IDebugSession};
use super::debug_protocol::Source as ProtocolSource;
use super::debug_source::{Source, unknown_source_label};
use super::debug_types::{
    IBreakpointUpdateData, IDebugEvaluatePosition, IDebugSessionFull, IExceptionInfo, IExpression, IExpressionContainer, IMemoryInvalidationEvent,
    IRawModelUpdate, IRawStoppedDetails, MemoryRange, State, StoppedDetails, capability,
};

/// `DEBUG_MEMORY_SCHEME`.
pub const DEBUG_MEMORY_SCHEME: &str = "vscode-debug-memory";

const KEEP_SUBTLE_FRAME_AT_TOP_REASONS: [&str; 3] = ["breakpoint", "step", "function breakpoint"];

thread_local! {
    /// `ExpressionContainer.allValues`.
    static ALL_VALUES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// `ExpressionContainer.allValues.clear()`.
pub fn clear_all_values() {
    ALL_VALUES.with(|values| values.borrow_mut().clear());
}

/// `isFrameDeemphasized(frame)`.
pub fn is_frame_deemphasized(frame: &StackFrame) -> bool {
    let hint = frame.presentation_hint.clone().or_else(|| frame.source.presentation_hint().map(|hint| hint_text(hint).to_owned()));
    matches!(hint.as_deref(), Some("deemphasize") | Some("subtle"))
}

fn hint_text(hint: super::debug_protocol::SourcePresentationHint) -> &'static str {
    match hint {
        super::debug_protocol::SourcePresentationHint::Normal => "normal",
        super::debug_protocol::SourcePresentationHint::Emphasize => "emphasize",
        super::debug_protocol::SourcePresentationHint::Deemphasize => "deemphasize",
    }
}

/// `hasMultipleStackFrames(thread)`: whether the thread has a caller frame, treating an incomplete stack with unknown depth
/// as potentially having one.
pub fn has_multiple_stack_frames(thread: Option<&Thread>) -> bool {
    let Some(thread) = thread else {
        return false;
    };
    let loaded_frame_count = thread.get_call_stack().len() as i64;
    let total_frame_count = thread.stopped_details().and_then(|details| details.total_frames);
    loaded_frame_count > 1 || (match total_frame_count {
        Some(total) => total > 1,
        None => !thread.reached_end_of_call_stack.get(),
    })
}

// ---- expressões

/// `ExpressionContainer`.
pub struct ExpressionContainer {
    session: RefCell<Option<Rc<dyn IDebugSessionFull>>>,
    thread_id: Option<i64>,
    reference: Cell<Option<i64>>,
    id: String,
    pub named_variables: Cell<Option<i64>>,
    pub indexed_variables: Cell<Option<i64>>,
    pub memory_reference: RefCell<Option<String>>,
    start_of_variables: Option<i64>,
    pub presentation_hint: RefCell<Option<Value>>,
    pub value_location_reference: Cell<Option<i64>>,
    pub expr_type: RefCell<Option<String>>,
    pub value_changed: Cell<bool>,
    value: RefCell<String>,
    children: RefCell<Option<Promise<Vec<Rc<dyn IExpression>>>>>,
    owner: RefCell<Option<Weak<dyn IExpressionContainer>>>,
    /// `Variable.adoptLazyResponse`.
    adopt_evaluate_name: Cell<bool>,
    evaluate_name: RefCell<Option<String>>,
}

impl ExpressionContainer {
    const BASE_CHUNK_SIZE: i64 = 100;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session: Option<Rc<dyn IDebugSessionFull>>,
        thread_id: Option<i64>,
        reference: Option<i64>,
        id: String,
        named_variables: Option<i64>,
        indexed_variables: Option<i64>,
        memory_reference: Option<String>,
        start_of_variables: Option<i64>,
        presentation_hint: Option<Value>,
        value_location_reference: Option<i64>,
    ) -> Rc<Self> {
        Rc::new(Self {
            session: RefCell::new(session),
            thread_id,
            reference: Cell::new(reference),
            id,
            named_variables: Cell::new(named_variables),
            indexed_variables: Cell::new(indexed_variables),
            memory_reference: RefCell::new(memory_reference),
            start_of_variables,
            presentation_hint: RefCell::new(presentation_hint),
            value_location_reference: Cell::new(value_location_reference),
            expr_type: RefCell::new(None),
            value_changed: Cell::new(false),
            value: RefCell::new(String::new()),
            children: RefCell::new(None),
            owner: RefCell::new(None),
            adopt_evaluate_name: Cell::new(false),
            evaluate_name: RefCell::new(None),
        })
    }

    /// Com os padrões do construtor (`namedVariables = 0`, `indexedVariables = 0`, `startOfVariables = 0`).
    pub fn with_defaults(session: Option<Rc<dyn IDebugSessionFull>>, thread_id: Option<i64>, reference: Option<i64>, id: String) -> Rc<Self> {
        Self::new(session, thread_id, reference, id, Some(0), Some(0), None, Some(0), None, None)
    }

    fn set_owner(&self, owner: Weak<dyn IExpressionContainer>) {
        *self.owner.borrow_mut() = Some(owner);
    }

    pub fn reference(&self) -> Option<i64> {
        self.reference.get()
    }

    /// `set reference(value)`: invalida o cache dos filhos.
    pub fn set_reference(&self, value: Option<i64>) {
        self.reference.set(value);
        *self.children.borrow_mut() = None;
    }

    pub fn session(&self) -> Option<Rc<dyn IDebugSessionFull>> {
        self.session.borrow().clone()
    }

    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    /// `set value(value)`.
    pub fn set_value(&self, value: &str) {
        *self.value.borrow_mut() = value.to_owned();
        let id = self.id.clone();
        let default_value = Expression::default_value();
        let changed = ALL_VALUES.with(|values| {
            let mut values = values.borrow_mut();
            let previous = values.get(&id).cloned();
            let changed = previous.as_ref().is_some_and(|p| !p.is_empty()) && previous.as_deref() != Some(default_value.as_str()) && previous.as_deref() != Some(value);
            values.insert(id, value.to_owned());
            changed
        });
        self.value_changed.set(changed);
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }

    /// `hasChildren`: only variables with reference > 0 have children.
    pub fn has_children(&self) -> bool {
        self.reference.get().is_some_and(|r| r > 0) && !self.presentation_hint.borrow().as_ref().is_some_and(|h| value_truthy(h.get("lazy")))
    }

    /// `evaluateLazy()`.
    pub fn evaluate_lazy(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let Some(reference) = this.reference.get() else {
                return Ok(());
            };
            let session = this.session().expect("a sessão do contêiner");
            let response = session.variables(reference, this.thread_id, None, None, None).await?;
            let Some(variables) = response.and_then(|r| r.body).and_then(|b| b.get("variables").cloned()).and_then(|v| v.as_array().cloned()) else {
                return Ok(());
            };
            if variables.len() != 1 {
                return Ok(());
            }
            let dummy = &variables[0];
            this.set_reference(dummy.get("variablesReference").and_then(Value::as_i64));
            *this.value.borrow_mut() = dummy.get("value").and_then(Value::as_str).unwrap_or_default().to_owned();
            this.named_variables.set(dummy.get("namedVariables").and_then(Value::as_i64));
            this.indexed_variables.set(dummy.get("indexedVariables").and_then(Value::as_i64));
            *this.memory_reference.borrow_mut() = dummy.get("memoryReference").and_then(Value::as_str).map(str::to_owned);
            *this.presentation_hint.borrow_mut() = dummy.get("presentationHint").cloned();
            this.value_location_reference.set(dummy.get("valueLocationReference").and_then(Value::as_i64));
            // Also call overridden method to adopt subclass props
            if this.adopt_evaluate_name.get() {
                *this.evaluate_name.borrow_mut() = dummy.get("evaluateName").and_then(Value::as_str).map(str::to_owned);
            }
            Ok(())
        })
    }

    /// `getChildren()`.
    pub fn get_children(self: &Rc<Self>) -> Promise<Vec<Rc<dyn IExpression>>> {
        if self.children.borrow().is_none() {
            let children = self.do_get_children();
            *self.children.borrow_mut() = Some(children);
        }
        self.children.borrow().clone().expect("filhos")
    }

    /// O `this` como pai dos filhos.
    fn parent(self: &Rc<Self>) -> Rc<dyn IExpressionContainer> {
        self.owner.borrow().as_ref().and_then(Weak::upgrade).unwrap_or_else(|| self.clone() as Rc<dyn IExpressionContainer>)
    }

    fn do_get_children(self: &Rc<Self>) -> Promise<Vec<Rc<dyn IExpression>>> {
        let this = self.clone();
        async_fn(async move {
            if !this.has_children() {
                return Ok(Vec::new());
            }

            // The adapter explicitly sents the children count of an expression only if there are lots of children which should be chunked.
            let get_children_in_chunks = this.indexed_variables.get().is_some_and(|n| n != 0);
            if !get_children_in_chunks {
                return Ok(this.fetch_variables(None, None, None).await?);
            }

            // Check if object has named variables, fetch them independent from indexed variables #9670
            let mut children: Vec<Rc<dyn IExpression>> =
                if this.named_variables.get().is_some_and(|n| n != 0) { this.fetch_variables(None, None, Some("named")).await? } else { Vec::new() };

            // Use a dynamic chunk size based on the number of elements #9774
            let indexed = this.indexed_variables.get().unwrap_or(0);
            let mut chunk_size = Self::BASE_CHUNK_SIZE;
            while indexed != 0 && indexed > chunk_size * Self::BASE_CHUNK_SIZE {
                chunk_size *= Self::BASE_CHUNK_SIZE;
            }

            if indexed != 0 && indexed > chunk_size {
                // There are a lot of children, create fake intermediate values that represent chunks #9537
                let number_of_chunks = (indexed as f64 / chunk_size as f64).ceil() as i64;
                for i in 0..number_of_chunks {
                    let start = this.start_of_variables.unwrap_or(0) + i * chunk_size;
                    let count = chunk_size.min(indexed - i * chunk_size);
                    let mut init = VariableInit::new(this.parent(), format!("[{}..{}]", start, start + count - 1));
                    init.session = this.session();
                    init.thread_id = this.thread_id;
                    init.reference = this.reference.get();
                    init.value = Some(String::new());
                    init.indexed_variables = Some(count);
                    init.presentation_hint = Some(json!({ "kind": "virtual" }));
                    init.start_of_variables = start;
                    init.evaluate_name = Some(String::new());
                    children.push(Variable::new(init));
                }
                return Ok(children);
            }

            let variables = this.fetch_variables(this.start_of_variables, this.indexed_variables.get(), Some("indexed")).await?;
            children.extend(variables);
            Ok(children)
        })
    }

    fn fetch_variables(self: &Rc<Self>, start: Option<i64>, count: Option<i64>, filter: Option<&'static str>) -> Promise<Vec<Rc<dyn IExpression>>> {
        let this = self.clone();
        async_fn(async move {
            let session = this.session().expect("a sessão do contêiner");
            let error_variable = |message: String| -> Rc<dyn IExpression> {
                let mut init = VariableInit::new(this.parent(), String::new());
                init.session = this.session();
                init.thread_id = this.thread_id;
                init.reference = Some(0);
                init.value = Some(message);
                init.named_variables = Some(0);
                init.indexed_variables = Some(0);
                init.presentation_hint = Some(json!({ "kind": "virtual" }));
                init.available = false;
                Variable::new(init)
            };
            let response = match session.variables(this.reference.get().unwrap_or(0), this.thread_id, filter, start, count).await {
                Ok(response) => response,
                Err(error) => return Ok(vec![error_variable(error.message)]),
            };
            let Some(variables) = response.and_then(|r| r.body).and_then(|b| b.get("variables").cloned()).and_then(|v| v.as_array().cloned()) else {
                return Ok(Vec::new());
            };

            let mut name_count: HashMap<String, i64> = HashMap::new();
            let mut vars: Vec<Rc<Variable>> = Vec::new();
            for v in variables.iter().filter(|v| !v.is_null()) {
                let name = v.get("name").and_then(Value::as_str);
                let value = v.get("value").and_then(Value::as_str);
                let reference = v.get("variablesReference").and_then(Value::as_i64);
                if let (Some(name), Some(value), Some(reference)) = (name, value, reference) {
                    let count = name_count.get(name).copied().unwrap_or(0);
                    let id_duplication_index = if count > 0 { count.to_string() } else { String::new() };
                    name_count.insert(name.to_owned(), count + 1);
                    let mut init = VariableInit::new(this.parent(), name.to_owned());
                    init.session = this.session();
                    init.thread_id = this.thread_id;
                    init.reference = Some(reference);
                    init.evaluate_name = v.get("evaluateName").and_then(Value::as_str).map(str::to_owned);
                    init.value = Some(value.to_owned());
                    init.named_variables = v.get("namedVariables").and_then(Value::as_i64);
                    init.indexed_variables = v.get("indexedVariables").and_then(Value::as_i64);
                    init.memory_reference = v.get("memoryReference").and_then(Value::as_str).map(str::to_owned);
                    init.presentation_hint = v.get("presentationHint").cloned();
                    init.ty = v.get("type").and_then(Value::as_str).map(str::to_owned);
                    init.variable_menu_context = v.get("__vscodeVariableMenuContext").and_then(Value::as_str).map(str::to_owned);
                    init.id_duplication_index = id_duplication_index;
                    init.declaration_location_reference = v.get("declarationLocationReference").and_then(Value::as_i64);
                    init.value_location_reference = v.get("valueLocationReference").and_then(Value::as_i64);
                    vars.push(Variable::new_variable(init));
                } else {
                    let mut init = VariableInit::new(this.parent(), String::new());
                    init.session = this.session();
                    init.thread_id = this.thread_id;
                    init.reference = Some(0);
                    init.value = Some(localize("invalidVariableAttributes", "Invalid variable attributes", &[]));
                    init.named_variables = Some(0);
                    init.indexed_variables = Some(0);
                    init.presentation_hint = Some(json!({ "kind": "virtual" }));
                    init.available = false;
                    vars.push(Variable::new_variable(init));
                }
            }

            if session.auto_expand_lazy_variables() {
                let lazy: Vec<Promise<()>> = vars
                    .iter()
                    .filter(|v| v.base.presentation_hint.borrow().as_ref().is_some_and(|h| value_truthy(h.get("lazy"))))
                    .map(|v| v.base.evaluate_lazy())
                    .collect();
                Promise::all(lazy).await?;
            }

            Ok(vars.into_iter().map(|v| v as Rc<dyn IExpression>).collect())
        })
    }

    /// `evaluateExpression(expression, session, stackFrame, context, keepLazyVars, location)`.
    pub fn evaluate_expression(
        self: &Rc<Self>,
        expression: &str,
        session: Option<Rc<dyn IDebugSessionFull>>,
        stack_frame: Option<Rc<StackFrame>>,
        context: &str,
        keep_lazy_vars: bool,
        location: Option<IDebugEvaluatePosition>,
    ) -> Promise<bool> {
        let this = self.clone();
        let expression = expression.to_owned();
        let context = context.to_owned();
        async_fn(async move {
            let Some(session) = session.filter(|_| stack_frame.is_some() || context == "repl") else {
                this.set_value(&if context == "repl" {
                    localize("startDebugFirst", "Please start a debug session to evaluate expressions", &[])
                } else {
                    Expression::default_value()
                });
                this.set_reference(Some(0));
                return Ok(false);
            };

            *this.session.borrow_mut() = Some(session.clone());
            let response = session.evaluate(&expression, stack_frame.as_ref().map(|f| f.frame_id), Some(&context), location).await;
            match response {
                Ok(response) => {
                    let body = response.and_then(|r| r.body);
                    if let Some(body) = body.filter(|b| !b.is_null()) {
                        this.set_value(body.get("result").and_then(Value::as_str).unwrap_or_default());
                        this.set_reference(body.get("variablesReference").and_then(Value::as_i64));
                        this.named_variables.set(body.get("namedVariables").and_then(Value::as_i64));
                        this.indexed_variables.set(body.get("indexedVariables").and_then(Value::as_i64));
                        *this.memory_reference.borrow_mut() = body.get("memoryReference").and_then(Value::as_str).map(str::to_owned);
                        if let Some(ty) = body.get("type").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                            *this.expr_type.borrow_mut() = Some(ty.to_owned());
                        }
                        let presentation_hint = body.get("presentationHint").cloned();
                        *this.presentation_hint.borrow_mut() = presentation_hint.clone();
                        this.value_location_reference.set(body.get("valueLocationReference").and_then(Value::as_i64));

                        if !keep_lazy_vars && presentation_hint.as_ref().is_some_and(|h| value_truthy(h.get("lazy"))) {
                            this.evaluate_lazy().await?;
                        }
                        return Ok(true);
                    }
                    Ok(false)
                }
                Err(error) => {
                    this.set_value(&error.message);
                    this.set_reference(Some(0));
                    *this.memory_reference.borrow_mut() = None;
                    Ok(false)
                }
            }
        })
    }

    /// `handleSetResponse(expression, response)`.
    fn handle_set_response(&self, response: Option<super::debug_protocol::Response>) {
        if let Some(body) = response.and_then(|r| r.body).filter(|b| !b.is_null()) {
            self.set_value(body.get("value").and_then(Value::as_str).unwrap_or_default());
            if let Some(ty) = body.get("type").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                *self.expr_type.borrow_mut() = Some(ty.to_owned());
            }
            self.set_reference(body.get("variablesReference").and_then(Value::as_i64));
            self.named_variables.set(body.get("namedVariables").and_then(Value::as_i64));
            self.indexed_variables.set(body.get("indexedVariables").and_then(Value::as_i64));
            *self.memory_reference.borrow_mut() = body.get("memoryReference").and_then(Value::as_str).map(str::to_owned);
            self.value_location_reference.set(body.get("valueLocationReference").and_then(Value::as_i64));
        }
    }
}

/// O `value` truthy do JS para um campo opcional do JSON.
fn value_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(_) => true,
    }
}

impl IExpressionContainer for ExpressionContainer {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn value(&self) -> String {
        self.value.borrow().clone()
    }

    fn expr_type(&self) -> Option<String> {
        self.expr_type.borrow().clone()
    }

    fn value_changed(&self) -> bool {
        self.value_changed.get()
    }

    fn has_children(&self) -> bool {
        ExpressionContainer::has_children(self)
    }

    fn get_session(&self) -> Option<Rc<dyn IDebugSessionFull>> {
        self.session()
    }

    fn evaluate_lazy(&self) -> Promise<()> {
        panic!("evaluateLazy exige o Rc do contêiner; use ExpressionContainer::evaluate_lazy no Rc")
    }

    fn get_children(&self) -> Promise<Vec<Rc<dyn IExpression>>> {
        panic!("getChildren exige o Rc do contêiner; use ExpressionContainer::get_children no Rc")
    }

    fn reference(&self) -> Option<i64> {
        self.reference.get()
    }

    fn memory_reference(&self) -> Option<String> {
        self.memory_reference.borrow().clone()
    }

    fn presentation_hint(&self) -> Option<Value> {
        self.presentation_hint.borrow().clone()
    }

    fn value_location_reference(&self) -> Option<i64> {
        self.value_location_reference.get()
    }
}

/// Os membros de [`IExpressionContainer`] de um tipo que guarda o contêiner em `base: Rc<ExpressionContainer>`.
macro_rules! delegate_expression_container {
    ($holder:ty) => {
        impl IExpressionContainer for $holder {
            fn get_id(&self) -> String {
                self.base.get_id()
            }

            fn value(&self) -> String {
                self.base.value()
            }

            fn expr_type(&self) -> Option<String> {
                self.base.expr_type.borrow().clone()
            }

            fn value_changed(&self) -> bool {
                self.base.value_changed.get()
            }

            fn has_children(&self) -> bool {
                self.base.has_children()
            }

            fn get_session(&self) -> Option<Rc<dyn IDebugSessionFull>> {
                self.base.session()
            }

            fn evaluate_lazy(&self) -> Promise<()> {
                self.base.evaluate_lazy()
            }

            fn get_children(&self) -> Promise<Vec<Rc<dyn IExpression>>> {
                self.base.get_children()
            }

            fn reference(&self) -> Option<i64> {
                self.base.reference()
            }

            fn memory_reference(&self) -> Option<String> {
                self.base.memory_reference.borrow().clone()
            }

            fn presentation_hint(&self) -> Option<Value> {
                self.base.presentation_hint.borrow().clone()
            }

            fn value_location_reference(&self) -> Option<i64> {
                self.base.value_location_reference.get()
            }
        }
    };
}

/// `Expression`.
pub struct Expression {
    pub base: Rc<ExpressionContainer>,
    pub name: RefCell<String>,
    pub available: Cell<bool>,
    on_did_change_value: Rc<Emitter<Rc<Expression>>>,
    me: Weak<Expression>,
}

impl Expression {
    /// `Expression.DEFAULT_VALUE`.
    pub fn default_value() -> String {
        localize("notAvailable", "not available", &[])
    }

    /// O construtor: `id` ausente gera um uuid.
    pub fn new(name: &str, id: Option<String>) -> Rc<Self> {
        let base = ExpressionContainer::with_defaults(None, None, Some(0), id.unwrap_or_else(generate_uuid));
        // name is not set if the expression is just being added
        // in that case do not set default value to prevent flashing #14499
        if !name.is_empty() {
            base.set_value(&Self::default_value());
        }
        let expression = Rc::new_cyclic(|me| Self {
            base: base.clone(),
            name: RefCell::new(name.to_owned()),
            available: Cell::new(false),
            on_did_change_value: Emitter::new(),
            me: me.clone(),
        });
        base.set_owner(Rc::downgrade(&(expression.clone() as Rc<dyn IExpressionContainer>)));
        expression
    }

    pub fn on_did_change_value(&self) -> Event<Rc<Expression>> {
        self.on_did_change_value.event()
    }

    pub fn evaluate(
        self: &Rc<Self>,
        session: Option<Rc<dyn IDebugSessionFull>>,
        stack_frame: Option<Rc<StackFrame>>,
        context: &str,
        keep_lazy_vars: bool,
        location: Option<IDebugEvaluatePosition>,
    ) -> Promise<()> {
        let this = self.clone();
        let context = context.to_owned();
        async_fn(async move {
            let had_default_value = this.base.value() == Self::default_value();
            let name = this.name.borrow().clone();
            let available = this.base.evaluate_expression(&name, session, stack_frame, &context, keep_lazy_vars, location).await?;
            this.available.set(available);
            if had_default_value || this.base.value_changed.get() {
                this.on_did_change_value.fire(this.clone());
            }
            Ok(())
        })
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        format!("{}\n{}", self.name.borrow(), self.base.value())
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> Value {
        json!({ "sessionId": self.base.session().map(|s| s.get_id()), "variable": self.to_debug_protocol_object() })
    }

    /// `toDebugProtocolObject()`.
    pub fn to_debug_protocol_object(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("name".into(), json!(self.name.borrow().clone()));
        object.insert("variablesReference".into(), json!(self.base.reference().unwrap_or(0)));
        if let Some(memory_reference) = self.base.memory_reference.borrow().as_ref() {
            object.insert("memoryReference".into(), json!(memory_reference));
        }
        object.insert("value".into(), json!(self.base.value()));
        if let Some(ty) = self.base.expr_type.borrow().as_ref() {
            object.insert("type".into(), json!(ty));
        }
        object.insert("evaluateName".into(), json!(self.name.borrow().clone()));
        Value::Object(object)
    }

    pub fn set_expression(self: &Rc<Self>, value: &str, stack_frame: Rc<StackFrame>) -> Promise<()> {
        let this = self.clone();
        let value = value.to_owned();
        async_fn(async move {
            let Some(session) = this.base.session() else {
                return Ok(());
            };
            let name = this.name.borrow().clone();
            let response = session.set_expression(stack_frame.frame_id, &name, &value).await?;
            this.base.handle_set_response(response);
            Ok(())
        })
    }
}

delegate_expression_container!(Expression);

impl IExpression for Expression {
    fn name(&self) -> String {
        self.name.borrow().clone()
    }
}

/// Os parâmetros do construtor de [`Variable`] (o original tem 18 parâmetros posicionais).
pub struct VariableInit {
    pub session: Option<Rc<dyn IDebugSessionFull>>,
    pub thread_id: Option<i64>,
    pub parent: Rc<dyn IExpressionContainer>,
    pub reference: Option<i64>,
    pub name: String,
    pub evaluate_name: Option<String>,
    pub value: Option<String>,
    pub named_variables: Option<i64>,
    pub indexed_variables: Option<i64>,
    pub memory_reference: Option<String>,
    pub presentation_hint: Option<Value>,
    pub ty: Option<String>,
    pub variable_menu_context: Option<String>,
    pub available: bool,
    pub start_of_variables: i64,
    pub id_duplication_index: String,
    pub declaration_location_reference: Option<i64>,
    pub value_location_reference: Option<i64>,
}

impl VariableInit {
    pub fn new(parent: Rc<dyn IExpressionContainer>, name: String) -> Self {
        Self {
            session: None,
            thread_id: None,
            parent,
            reference: None,
            name,
            evaluate_name: None,
            value: None,
            named_variables: None,
            indexed_variables: None,
            memory_reference: None,
            presentation_hint: None,
            ty: None,
            variable_menu_context: None,
            available: true,
            start_of_variables: 0,
            id_duplication_index: String::new(),
            declaration_location_reference: None,
            value_location_reference: None,
        }
    }
}

/// `Variable`.
pub struct Variable {
    pub base: Rc<ExpressionContainer>,
    /// Used to show the error message coming from the adapter when setting the value #7807
    pub error_message: RefCell<Option<String>>,
    pub parent: Rc<dyn IExpressionContainer>,
    pub name: RefCell<Option<String>>,
    pub variable_menu_context: Option<String>,
    pub available: bool,
    pub declaration_location_reference: Option<i64>,
}

impl Variable {
    /// Cria a variável como `Rc<dyn IExpression>` (a forma que os filhos têm).
    pub fn new(init: VariableInit) -> Rc<dyn IExpression> {
        Self::new_variable(init)
    }

    /// Cria a variável concreta.
    pub fn new_variable(init: VariableInit) -> Rc<Variable> {
        let id = format!("variable:{}:{}:{}", init.parent.get_id(), init.name, init.id_duplication_index);
        let base = ExpressionContainer::new(
            init.session,
            init.thread_id,
            init.reference,
            id,
            init.named_variables,
            init.indexed_variables,
            init.memory_reference,
            Some(init.start_of_variables),
            init.presentation_hint,
            init.value_location_reference,
        );
        base.adopt_evaluate_name.set(true);
        *base.evaluate_name.borrow_mut() = init.evaluate_name;
        base.set_value(&init.value.unwrap_or_default());
        *base.expr_type.borrow_mut() = init.ty;
        let variable = Rc::new(Self {
            base: base.clone(),
            error_message: RefCell::new(None),
            parent: init.parent,
            name: RefCell::new(Some(init.name)),
            variable_menu_context: init.variable_menu_context,
            available: init.available,
            declaration_location_reference: init.declaration_location_reference,
        });
        base.set_owner(Rc::downgrade(&(variable.clone() as Rc<dyn IExpressionContainer>)));
        variable
    }

    pub fn evaluate_name(&self) -> Option<String> {
        self.base.evaluate_name.borrow().clone()
    }

    pub fn get_thread_id(&self) -> Option<i64> {
        self.base.thread_id
    }

    pub fn set_variable(self: &Rc<Self>, value: &str, stack_frame: Rc<StackFrame>) -> Promise<()> {
        let this = self.clone();
        let value = value.to_owned();
        async_fn(async move {
            let Some(session) = this.base.session() else {
                return Ok(());
            };

            // Send out a setExpression for debug extensions that do not support set variables https://github.com/microsoft/vscode/issues/124679#issuecomment-869844437
            let capabilities = session.capabilities();
            if capability(&capabilities, "supportsSetExpression") && !capability(&capabilities, "supportsSetVariable") && this.evaluate_name().is_some_and(|n| !n.is_empty()) {
                return this.set_expression(&value, stack_frame).await;
            }

            let name = this.name.borrow().clone().unwrap_or_default();
            match session.set_variable(this.parent.reference().unwrap_or(0), &name, &value).await {
                Ok(response) => this.base.handle_set_response(response),
                Err(error) => *this.error_message.borrow_mut() = Some(error.message),
            }
            Ok(())
        })
    }

    pub fn set_expression(self: &Rc<Self>, value: &str, stack_frame: Rc<StackFrame>) -> Promise<()> {
        let this = self.clone();
        let value = value.to_owned();
        async_fn(async move {
            let (Some(session), Some(evaluate_name)) = (this.base.session(), this.evaluate_name().filter(|n| !n.is_empty())) else {
                return Ok(());
            };
            let response = session.set_expression(stack_frame.frame_id, &evaluate_name, &value).await?;
            this.base.handle_set_response(response);
            Ok(())
        })
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        match self.name.borrow().as_ref().filter(|n| !n.is_empty()) {
            Some(name) => format!("{}: {}", name, self.base.value()),
            None => self.base.value(),
        }
    }

    /// `toDebugProtocolObject()`.
    pub fn to_debug_protocol_object(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("name".into(), json!(self.name.borrow().clone()));
        object.insert("variablesReference".into(), json!(self.base.reference().unwrap_or(0)));
        if let Some(memory_reference) = self.base.memory_reference.borrow().as_ref() {
            object.insert("memoryReference".into(), json!(memory_reference));
        }
        object.insert("value".into(), json!(self.base.value()));
        if let Some(ty) = self.base.expr_type.borrow().as_ref() {
            object.insert("type".into(), json!(ty));
        }
        if let Some(evaluate_name) = self.evaluate_name() {
            object.insert("evaluateName".into(), json!(evaluate_name));
        }
        Value::Object(object)
    }

    /// `toJSON()`: o contêiner é a expressão (`{ expression }`) ou o objeto do protocolo do pai.
    pub fn to_json(&self, parent_json: Value) -> Value {
        json!({ "sessionId": self.base.session().map(|s| s.get_id()), "container": parent_json, "variable": self.to_debug_protocol_object() })
    }
}

delegate_expression_container!(Variable);

impl IExpression for Variable {
    fn name(&self) -> String {
        self.name.borrow().clone().unwrap_or_default()
    }

    fn is_variable(&self) -> bool {
        true
    }

    // `display` é o `toString()`.
}

/// `Scope`.
pub struct Scope {
    pub base: Rc<ExpressionContainer>,
    pub stack_frame: Rc<StackFrame>,
    pub name: String,
    pub expensive: bool,
    pub range: Option<Range>,
}

impl Scope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stack_frame: Rc<StackFrame>,
        id: i64,
        name: String,
        reference: i64,
        expensive: bool,
        named_variables: Option<i64>,
        indexed_variables: Option<i64>,
        range: Option<Range>,
    ) -> Rc<Self> {
        let base = ExpressionContainer::new(
            Some(stack_frame.thread.session.clone()),
            Some(stack_frame.thread.thread_id),
            Some(reference),
            format!("scope:{name}:{id}"),
            named_variables,
            indexed_variables,
            None,
            Some(0),
            None,
            None,
        );
        let scope = Rc::new(Self { base: base.clone(), stack_frame, name, expensive, range });
        base.set_owner(Rc::downgrade(&(scope.clone() as Rc<dyn IExpressionContainer>)));
        scope
    }

    /// `ErrorScope(stackFrame, index, message)`.
    pub fn new_error(stack_frame: Rc<StackFrame>, index: i64, message: String) -> Rc<Self> {
        Self::new(stack_frame, index, message, 0, false, None, None, None)
    }

    pub fn children_have_been_loaded(&self) -> bool {
        self.base.children.borrow().is_some()
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        self.name.clone()
    }

    /// `toDebugProtocolObject()`.
    pub fn to_debug_protocol_object(&self) -> Value {
        json!({ "name": self.name, "variablesReference": self.base.reference().unwrap_or(0), "expensive": self.expensive })
    }
}

delegate_expression_container!(Scope);

// ---- quadros de pilha e threads

/// `StackFrame`.
pub struct StackFrame {
    pub thread: Rc<Thread>,
    pub frame_id: i64,
    pub source: Rc<Source>,
    pub name: String,
    pub presentation_hint: Option<String>,
    pub range: Option<Range>,
    index: i64,
    pub can_restart: bool,
    pub instruction_pointer_reference: Option<String>,
    scopes: RefCell<Option<Promise<Vec<Rc<Scope>>>>>,
}

impl StackFrame {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread: Rc<Thread>,
        frame_id: i64,
        source: Rc<Source>,
        name: &str,
        presentation_hint: Option<&str>,
        range: Option<Range>,
        index: i64,
        can_restart: bool,
        instruction_pointer_reference: Option<String>,
    ) -> Rc<Self> {
        Rc::new(Self {
            thread,
            frame_id,
            source,
            name: name.to_owned(),
            presentation_hint: presentation_hint.map(str::to_owned),
            range,
            index,
            can_restart,
            instruction_pointer_reference,
            scopes: RefCell::new(None),
        })
    }

    pub fn get_id(&self) -> String {
        format!("stackframe:{}:{}:{}", self.thread.get_id(), self.index, self.source.name())
    }

    pub fn get_scopes(self: &Rc<Self>) -> Promise<Vec<Rc<Scope>>> {
        if self.scopes.borrow().is_none() {
            let this = self.clone();
            let promise = async_fn(async move {
                match this.thread.session.scopes(this.frame_id, this.thread.thread_id).await {
                    Ok(response) => {
                        let Some(scopes) = response.and_then(|r| r.body).and_then(|b| b.get("scopes").cloned()).and_then(|s| s.as_array().cloned()) else {
                            return Ok(Vec::new());
                        };
                        let mut used_ids = std::collections::HashSet::new();
                        let mut result = Vec::new();
                        for rs in scopes {
                            // form the id based on the name and location so that it's the same across multiple pauses to retain expansion state
                            let name = rs.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
                            let number = |key: &str| rs.get(key).and_then(Value::as_i64);
                            let show = |value: Option<i64>| value.map_or_else(|| "undefined".to_owned(), |v| v.to_string());
                            let key: Vec<u16> = format!("{}:{}:{}", name, show(number("line")), show(number("column"))).encode_utf16().collect();
                            let mut id = 0;
                            loop {
                                id = string_hash(&key, id);
                                if !used_ids.contains(&id) {
                                    break;
                                }
                            }
                            used_ids.insert(id);
                            let range = match (number("line"), number("column"), number("endLine"), number("endColumn")) {
                                (Some(l), Some(c), Some(el), Some(ec)) if l != 0 && c != 0 && el != 0 && ec != 0 => Some(Range::new(l as i32, c as i32, el as i32, ec as i32)),
                                _ => None,
                            };
                            result.push(Scope::new(
                                this.clone(),
                                id as i64,
                                name,
                                number("variablesReference").unwrap_or(0),
                                rs.get("expensive").and_then(Value::as_bool).unwrap_or(false),
                                number("namedVariables"),
                                number("indexedVariables"),
                                range,
                            ));
                        }
                        Ok(result)
                    }
                    Err(error) => Ok(vec![Scope::new_error(this.clone(), 0, error.message)]),
                }
            });
            *self.scopes.borrow_mut() = Some(promise);
        }
        self.scopes.borrow().clone().expect("escopos")
    }

    pub fn get_most_specific_scopes(self: &Rc<Self>, range: Range) -> Promise<Vec<Rc<Scope>>> {
        let scopes = self.get_scopes();
        async_fn(async move {
            let scopes = scopes.await?;
            let non_expensive: Vec<Rc<Scope>> = scopes.into_iter().filter(|s| !s.expensive).collect();
            let have_range_info = non_expensive.iter().any(|s| s.range.is_some());
            if !have_range_info {
                return Ok(non_expensive);
            }

            let mut containing: Vec<Rc<Scope>> =
                non_expensive.iter().filter(|scope| scope.range.as_ref().is_some_and(|r| r.contains_range(&range))).cloned().collect();
            containing.sort_by_key(|scope| {
                let r = scope.range.expect("com intervalo");
                r.end_line_number - r.start_line_number
            });
            Ok(if containing.is_empty() { non_expensive } else { containing })
        })
    }

    pub fn restart(&self) -> Promise<()> {
        self.thread.session.restart_frame(self.frame_id, self.thread.thread_id)
    }

    pub fn forget_scopes(&self) {
        *self.scopes.borrow_mut() = None;
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        let line_number_to_string = self.range.map(|r| format!(":{}", r.start_line_number)).unwrap_or_default();
        let source_to_string = format!(
            "{}{}",
            if self.source.in_memory() { self.source.name() } else { self.source.uri.fs_path(je_platform::resources::host_platform()).to_string_lossy() },
            line_number_to_string
        );

        if source_to_string == unknown_source_label() { self.name.clone() } else { format!("{} ({})", self.name, source_to_string) }
    }

    pub fn equals(&self, other: &StackFrame) -> bool {
        self.name == other.name
            && Rc::ptr_eq(&other.thread, &self.thread)
            && self.frame_id == other.frame_id
            && Rc::ptr_eq(&other.source, &self.source)
            && self.range == other.range
    }
}

impl IStackFrame for StackFrame {
    fn thread(&self) -> Rc<dyn IThread> {
        self.thread.clone()
    }

    fn frame_id(&self) -> i64 {
        self.frame_id
    }
}

/// Os membros de `Thread` que o `refreshTopOfCallstack` do modelo usa (a thread real e a falsa dos testes).
pub trait CallStackThread {
    /// `session.capabilities`.
    fn session_capabilities(&self) -> Value;
    fn get_id(&self) -> String;
    fn fetch_call_stack(&self, levels: Option<i64>) -> Promise<()>;
    fn get_stale_call_stack(&self) -> Vec<Rc<StackFrame>>;
    fn get_call_stack(&self) -> Vec<Rc<StackFrame>>;
    fn reached_end_of_call_stack(&self) -> bool;
}

/// `Thread`.
pub struct Thread {
    pub session: Rc<dyn IDebugSessionFull>,
    pub name: RefCell<String>,
    pub thread_id: i64,
    call_stack: RefCell<Vec<Rc<StackFrame>>>,
    stale_call_stack: RefCell<Vec<Rc<StackFrame>>>,
    call_stack_cancellation_tokens: RefCell<Vec<CancellationTokenSource>>,
    stopped_details: RefCell<Option<StoppedDetails>>,
    pub stopped: Cell<bool>,
    pub reached_end_of_call_stack: Cell<bool>,
    pub last_stepping_granularity: RefCell<Option<String>>,
    me: Weak<Thread>,
}

impl Thread {
    pub fn new(session: Rc<dyn IDebugSessionFull>, name: &str, thread_id: i64) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            session,
            name: RefCell::new(name.to_owned()),
            thread_id,
            call_stack: RefCell::new(Vec::new()),
            stale_call_stack: RefCell::new(Vec::new()),
            call_stack_cancellation_tokens: RefCell::new(Vec::new()),
            stopped_details: RefCell::new(None),
            stopped: Cell::new(false),
            reached_end_of_call_stack: Cell::new(false),
            last_stepping_granularity: RefCell::new(None),
            me: me.clone(),
        })
    }

    fn this(&self) -> Rc<Thread> {
        self.me.upgrade().expect("a thread está viva")
    }

    pub fn get_id(&self) -> String {
        format!("thread:{}:{}", self.session.get_id(), self.thread_id)
    }

    /// `stoppedDetails` (um instantâneo; [`Thread::stopped_details_ref`] é o objeto compartilhado).
    pub fn stopped_details(&self) -> Option<IRawStoppedDetails> {
        self.stopped_details.borrow().as_ref().map(|d| d.borrow().clone())
    }

    pub fn stopped_details_ref(&self) -> Option<StoppedDetails> {
        self.stopped_details.borrow().clone()
    }

    /// `thread.stoppedDetails = details` (objeto novo).
    pub fn set_stopped_details(&self, details: Option<IRawStoppedDetails>) {
        *self.stopped_details.borrow_mut() = details.map(|d| Rc::new(RefCell::new(d)));
    }

    /// `thread.stoppedDetails = details` (objeto compartilhado com a sessão).
    pub fn set_stopped_details_ref(&self, details: Option<StoppedDetails>) {
        *self.stopped_details.borrow_mut() = details;
    }

    /// Altera o `stoppedDetails` em uso (`thread.stoppedDetails.totalFrames = 1`).
    pub fn update_stopped_details(&self, update: impl FnOnce(&mut IRawStoppedDetails)) {
        if let Some(details) = self.stopped_details.borrow().as_ref() {
            update(&mut details.borrow_mut());
        }
    }

    pub fn clear_call_stack(&self) {
        if !self.call_stack.borrow().is_empty() {
            let stale = self.call_stack.borrow().clone();
            *self.stale_call_stack.borrow_mut() = stale;
        }
        self.call_stack.borrow_mut().clear();
        let tokens = std::mem::take(&mut *self.call_stack_cancellation_tokens.borrow_mut());
        for token in tokens {
            token.dispose_with(true);
        }
    }

    pub fn get_call_stack(&self) -> Vec<Rc<StackFrame>> {
        self.call_stack.borrow().clone()
    }

    /// Substitui a pilha que `getCallStack()` devolve (o `getCallStack` sobrescrito dos testes do original).
    pub fn set_call_stack(&self, call_stack: Vec<Rc<StackFrame>>) {
        *self.call_stack.borrow_mut() = call_stack;
    }

    pub fn get_stale_call_stack(&self) -> Vec<Rc<StackFrame>> {
        self.stale_call_stack.borrow().clone()
    }

    pub fn get_top_stack_frame(&self) -> Option<Rc<StackFrame>> {
        let call_stack = self.get_call_stack();
        let details = self.stopped_details();
        let stop_reason = details.as_ref().and_then(|d| d.reason.clone());
        let stepping = self.last_stepping_granularity.borrow().clone();
        // Allow stack frame without source and with instructionReferencePointer as top stack frame when using disassembly view.
        call_stack
            .into_iter()
            .find(|sf| {
                ((stop_reason.as_deref() == Some("instruction breakpoint") || (stop_reason.as_deref() == Some("step") && stepping.as_deref() == Some("instruction")))
                    && sf.instruction_pointer_reference.as_ref().is_some_and(|r| !r.is_empty()))
                    || (sf.source.available.get()
                        && (stop_reason.as_deref().is_some_and(|r| KEEP_SUBTLE_FRAME_AT_TOP_REASONS.contains(&r)) || !is_frame_deemphasized(sf)))
            })
    }

    pub fn state_label(&self) -> String {
        match self.stopped_details() {
            Some(details) => details.description.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| match details.reason.as_deref().filter(|r| !r.is_empty()) {
                Some(reason) => localize("pausedOn", "Paused on {0}", &[reason]),
                None => localize("paused", "Paused", &[]),
            }),
            None => localize("running", "Running", &[]),
        }
    }

    /// Queries the debug adapter for the callstack and returns a promise which completes once the call stack has been
    /// retrieved. If the thread is not stopped, it returns a promise to an empty array. Only fetches the first stack frame
    /// for performance reasons. Calling this method consecutive times gets the remainder of the call stack.
    pub fn fetch_call_stack(self: &Rc<Self>, levels: Option<i64>) -> Promise<()> {
        let this = self.clone();
        let levels = levels.unwrap_or(20);
        async_fn(async move {
            if this.stopped.get() {
                let start = this.call_stack.borrow().len();
                let call_stack = this.get_call_stack_impl(start as i64, levels).await?;
                this.reached_end_of_call_stack.set((call_stack.len() as i64) < levels);
                if start < this.call_stack.borrow().len() {
                    // Set the stack frames for exact position we requested. To make sure no concurrent requests create duplicate stack frames #30660
                    let len = this.call_stack.borrow().len();
                    this.call_stack.borrow_mut().drain(start..len);
                }
                this.call_stack.borrow_mut().extend(call_stack);
                let total = this.stopped_details().and_then(|d| d.total_frames);
                if total.is_some_and(|total| total == this.call_stack.borrow().len() as i64) {
                    this.reached_end_of_call_stack.set(true);
                }
            }
            Ok(())
        })
    }

    fn get_call_stack_impl(self: &Rc<Self>, start_frame: i64, levels: i64) -> Promise<Vec<Rc<StackFrame>>> {
        let this = self.clone();
        async_fn(async move {
            let token_source = CancellationTokenSource::new(None);
            let token = token_source.token();
            this.call_stack_cancellation_tokens.borrow_mut().push(token_source);
            let response = match this.session.stack_trace(this.thread_id, start_frame, levels, token.clone()).await {
                Ok(response) => response,
                Err(error) => {
                    this.update_stopped_details(|details| details.frames_error_message = Some(error.message));
                    return Ok(Vec::new());
                }
            };
            let Some(body) = response.and_then(|r| r.body).filter(|b| !b.is_null()).filter(|_| !token.is_cancellation_requested()) else {
                return Ok(Vec::new());
            };

            let total_frames = body.get("totalFrames").and_then(Value::as_i64);
            this.update_stopped_details(|details| details.total_frames = total_frames);

            let frames = body.get("stackFrames").and_then(Value::as_array).cloned().unwrap_or_default();
            Ok(frames
                .iter()
                .enumerate()
                .map(|(index, rsf)| {
                    let number = |key: &str| rsf.get(key).and_then(Value::as_i64);
                    let source = this.session.get_source(rsf.get("source").filter(|s| s.is_object()).map(super::breakpoints::protocol_source_from_value));
                    let line = number("line").unwrap_or(0);
                    let column = number("column").unwrap_or(0);
                    let end_line = number("endLine").filter(|n| *n != 0).unwrap_or(line);
                    let end_column = number("endColumn").filter(|n| *n != 0).unwrap_or(column);
                    StackFrame::new(
                        this.clone(),
                        number("id").unwrap_or(0),
                        source,
                        rsf.get("name").and_then(Value::as_str).unwrap_or_default(),
                        rsf.get("presentationHint").and_then(Value::as_str),
                        Some(Range::new(line as i32, column as i32, end_line as i32, end_column as i32)),
                        start_frame + index as i64,
                        rsf.get("canRestart").and_then(Value::as_bool).unwrap_or(true),
                        rsf.get("instructionPointerReference").and_then(Value::as_str).map(str::to_owned),
                    )
                })
                .collect())
        })
    }

    /// Returns exception info promise if the exception was thrown, otherwise undefined.
    pub fn exception_info(&self) -> Promise<Option<IExceptionInfo>> {
        if let Some(details) = self.stopped_details()
            && details.reason.as_deref() == Some("exception")
        {
            if capability(&self.session.capabilities(), "supportsExceptionInfoRequest") {
                return self.session.exception_info(self.thread_id);
            }
            return Promise::resolved(Some(IExceptionInfo { description: details.text, ..IExceptionInfo::default() }));
        }
        Promise::resolved(None)
    }

    pub fn next(&self, granularity: Option<String>) -> Promise<()> {
        self.session.next(self.thread_id, granularity)
    }

    pub fn step_in(&self, granularity: Option<String>) -> Promise<()> {
        self.session.step_in(self.thread_id, None, granularity)
    }

    pub fn step_out(&self, granularity: Option<String>) -> Promise<()> {
        self.session.step_out(self.thread_id, granularity)
    }

    pub fn step_back(&self, granularity: Option<String>) -> Promise<()> {
        self.session.step_back(self.thread_id, granularity)
    }

    pub fn continue_(&self) -> Promise<()> {
        self.session.continue_(self.thread_id)
    }

    pub fn pause(&self) -> Promise<()> {
        self.session.pause(self.thread_id)
    }

    pub fn terminate(&self) -> Promise<()> {
        self.session.terminate_threads(Some(vec![self.thread_id]))
    }

    pub fn reverse_continue(&self) -> Promise<()> {
        self.session.reverse_continue(self.thread_id)
    }
}

impl IThread for Thread {
    fn thread_id(&self) -> i64 {
        self.thread_id
    }

    fn session(&self) -> Rc<dyn IDebugSession> {
        self.session.clone()
    }
}

impl CallStackThread for Thread {
    fn session_capabilities(&self) -> Value {
        self.session.capabilities()
    }

    fn get_id(&self) -> String {
        Thread::get_id(self)
    }

    fn fetch_call_stack(&self, levels: Option<i64>) -> Promise<()> {
        Thread::fetch_call_stack(&self.this(), levels)
    }

    fn get_stale_call_stack(&self) -> Vec<Rc<StackFrame>> {
        Thread::get_stale_call_stack(self)
    }

    fn get_call_stack(&self) -> Vec<Rc<StackFrame>> {
        Thread::get_call_stack(self)
    }

    fn reached_end_of_call_stack(&self) -> bool {
        self.reached_end_of_call_stack.get()
    }
}

/// `getUriForDebugMemory(sessionId, memoryReference, range, displayName)`.
pub fn get_uri_for_debug_memory(session_id: &str, memory_reference: &str, range: Option<(i64, i64)>, display_name: Option<&str>) -> URI {
    let encode = |text: &str| {
        let units: Vec<u16> = text.encode_utf16().collect();
        String::from_utf16_lossy(&je_base::uri::encode_uri_component(&units))
    };
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(DEBUG_MEMORY_SCHEME)),
            authority: Some(JsString::from(session_id)),
            path: Some(JsString::from(format!("/{}/{}.bin", encode(memory_reference), encode(display_name.unwrap_or("memory"))))),
            query: range.map(|(from, to)| JsString::from(format!("?range={from}:{to}"))),
            ..UriComponents::default()
        },
        false,
    )
}

/// `MemoryRegion`.
pub struct MemoryRegion {
    store: Rc<DisposableStore>,
    invalidate_emitter: Rc<Emitter<IMemoryInvalidationEvent>>,
    memory_reference: String,
    session: Rc<dyn IDebugSessionFull>,
    pub writable: bool,
}

impl MemoryRegion {
    pub fn new(memory_reference: &str, session: Rc<dyn IDebugSessionFull>) -> Rc<Self> {
        let store = DisposableStore::new();
        let invalidate_emitter = Emitter::new();
        store.add(invalidate_emitter.clone());
        let writable = capability(&session.capabilities(), "supportsWriteMemoryRequest");
        let reference = memory_reference.to_owned();
        let emitter = invalidate_emitter.clone();
        store.add(session.on_did_invalidate_memory().listen(move |e| {
            let body = e.body.clone().unwrap_or(Value::Null);
            if body.get("memoryReference").and_then(Value::as_str) == Some(reference.as_str()) {
                let offset = body.get("offset").and_then(Value::as_i64).unwrap_or(0);
                let count = body.get("count").and_then(Value::as_i64).unwrap_or(0);
                // `this.invalidate(e.body.offset, e.body.count - e.body.offset)`
                emitter.fire(IMemoryInvalidationEvent { from_offset: offset, to_offset: count - offset });
            }
        }));
        Rc::new(Self { store, invalidate_emitter, memory_reference: memory_reference.to_owned(), session, writable })
    }

    pub fn on_did_invalidate(&self) -> Event<IMemoryInvalidationEvent> {
        self.invalidate_emitter.event()
    }

    pub fn read(self: &Rc<Self>, from_offset: i64, to_offset: i64) -> Promise<Vec<MemoryRange>> {
        let this = self.clone();
        async_fn(async move {
            let length = to_offset - from_offset;
            let offset = from_offset;
            let result = this.session.read_memory(&this.memory_reference, offset, length).await?;

            let body = result.and_then(|r| r.body);
            let data = body.as_ref().and_then(|b| b.get("data").and_then(Value::as_str).map(str::to_owned)).filter(|d| !d.is_empty());
            let Some(data) = data else {
                return Ok(vec![MemoryRange::Unreadable { offset, length }]);
            };
            let unreadable = body.as_ref().and_then(|b| b.get("unreadableBytes").and_then(Value::as_i64)).unwrap_or(0);
            let mut data = decode_base64(&data);

            let data_length = length - unreadable;
            if (data.byte_length() as i64) < data_length {
                let pad = VSBuffer::alloc((data_length - data.byte_length() as i64) as usize);
                data = VSBuffer::concat(&[data, pad], Some(data_length as usize));
            } else if data.byte_length() as i64 > data_length {
                data = data.slice(Some(0), Some(data_length));
            }

            if unreadable == 0 {
                return Ok(vec![MemoryRange::Valid { offset, length, data }]);
            }

            Ok(vec![MemoryRange::Valid { offset, length: data_length, data }, MemoryRange::Unreadable { offset: offset + data_length, length: unreadable }])
        })
    }

    pub fn write(self: &Rc<Self>, offset: i64, data: VSBuffer) -> Promise<i64> {
        let this = self.clone();
        async_fn(async move {
            let encoded = encode_base64(&data, true, false);
            let result = this.session.write_memory(&this.memory_reference, offset, &encoded, Some(true)).await?;
            let written = result.and_then(|r| r.body).and_then(|b| b.get("bytesWritten").and_then(Value::as_i64)).unwrap_or(data.byte_length() as i64);
            this.invalidate_emitter.fire(IMemoryInvalidationEvent { from_offset: offset, to_offset: offset + written });
            Ok(written)
        })
    }
}

impl IDisposable for MemoryRegion {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// `ThreadAndSessionIds`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadAndSessionIds {
    pub session_id: String,
    pub thread_id: i64,
}

impl ThreadAndSessionIds {
    pub fn get_id(&self) -> String {
        format!("{}:{}", self.session_id, self.thread_id)
    }
}

// ---- armazenamento e modelo

const DEBUG_BREAKPOINTS_KEY: &str = "debug.breakpoint";
const DEBUG_FUNCTION_BREAKPOINTS_KEY: &str = "debug.functionbreakpoint";
const DEBUG_DATA_BREAKPOINTS_KEY: &str = "debug.databreakpoint";
const DEBUG_EXCEPTION_BREAKPOINTS_KEY: &str = "debug.exceptionbreakpoint";
const DEBUG_WATCH_EXPRESSIONS_KEY: &str = "debug.watchexpressions";
const DEBUG_CHOSEN_ENVIRONMENTS_KEY: &str = "debug.chosenenvironment";
const DEBUG_UX_STATE_KEY: &str = "debug.uxstate";

/// `IChosenEnvironment`.
#[derive(Clone, Debug, PartialEq)]
pub struct IChosenEnvironment {
    pub r#type: String,
    pub dynamic_label: Option<String>,
}

/// `DebugStorage`: as listas de breakpoints e de expressões de observação, carregadas do armazenamento do workspace e
/// recarregadas quando ele muda por fora. Os `ISettableObservable` do original são as listas com um emissor de mudança,
/// que o `autorun` do [`DebugModel`] escuta.
pub struct DebugStorage {
    store: Rc<DisposableStore>,
    storage_service: Rc<dyn StorageService>,
    text_file_service: Rc<dyn IDirtyChecker>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    log_service: Rc<dyn LogService>,
    pub breakpoints: RefCell<Vec<Rc<Breakpoint>>>,
    pub function_breakpoints: RefCell<Vec<Rc<FunctionBreakpoint>>>,
    pub exception_breakpoints: RefCell<Vec<Rc<ExceptionBreakpoint>>>,
    pub data_breakpoints: RefCell<Vec<Rc<DataBreakpoint>>>,
    pub watch_expressions: RefCell<Vec<Rc<Expression>>>,
    on_did_change_breakpoints: Rc<Emitter<()>>,
    on_did_change_watch_expressions: Rc<Emitter<()>>,
}

impl DebugStorage {
    pub fn new(
        storage_service: Rc<dyn StorageService>,
        text_file_service: Rc<dyn IDirtyChecker>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        log_service: Rc<dyn LogService>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let on_did_change_breakpoints = Emitter::new();
        let on_did_change_watch_expressions = Emitter::new();
        store.add(on_did_change_breakpoints.clone());
        store.add(on_did_change_watch_expressions.clone());
        let this = Rc::new(Self {
            store: store.clone(),
            storage_service: storage_service.clone(),
            text_file_service,
            uri_identity_service,
            log_service,
            breakpoints: RefCell::new(Vec::new()),
            function_breakpoints: RefCell::new(Vec::new()),
            exception_breakpoints: RefCell::new(Vec::new()),
            data_breakpoints: RefCell::new(Vec::new()),
            watch_expressions: RefCell::new(Vec::new()),
            on_did_change_breakpoints,
            on_did_change_watch_expressions,
        });
        *this.breakpoints.borrow_mut() = this.load_breakpoints();
        *this.function_breakpoints.borrow_mut() = this.load_function_breakpoints();
        *this.exception_breakpoints.borrow_mut() = this.load_exception_breakpoints();
        *this.data_breakpoints.borrow_mut() = this.load_data_breakpoints();
        *this.watch_expressions.borrow_mut() = this.load_watch_expressions();

        let weak = Rc::downgrade(&this);
        let events = storage_service.on_did_change_value(StorageScope::Workspace, None, &store);
        store.add(events.listen(move |e| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            if e.external {
                match e.key.as_str() {
                    DEBUG_BREAKPOINTS_KEY => {
                        *this.breakpoints.borrow_mut() = this.load_breakpoints();
                        this.on_did_change_breakpoints.fire(());
                    }
                    DEBUG_FUNCTION_BREAKPOINTS_KEY => {
                        *this.function_breakpoints.borrow_mut() = this.load_function_breakpoints();
                        this.on_did_change_breakpoints.fire(());
                    }
                    DEBUG_EXCEPTION_BREAKPOINTS_KEY => {
                        *this.exception_breakpoints.borrow_mut() = this.load_exception_breakpoints();
                        this.on_did_change_breakpoints.fire(());
                    }
                    DEBUG_DATA_BREAKPOINTS_KEY => {
                        *this.data_breakpoints.borrow_mut() = this.load_data_breakpoints();
                        this.on_did_change_breakpoints.fire(());
                    }
                    DEBUG_WATCH_EXPRESSIONS_KEY => {
                        *this.watch_expressions.borrow_mut() = this.load_watch_expressions();
                        this.on_did_change_watch_expressions.fire(());
                    }
                    _ => {}
                }
            }
        }));
        this
    }

    /// `loadDebugUxState()`.
    pub fn load_debug_ux_state(&self) -> String {
        self.storage_service.get(DEBUG_UX_STATE_KEY, StorageScope::Workspace, Some("default")).unwrap_or_else(|| "default".to_owned())
    }

    /// `storeDebugUxState(value)`.
    pub fn store_debug_ux_state(&self, value: &str) {
        self.storage_service.store(DEBUG_UX_STATE_KEY, StorageValue::String(value.to_owned()), StorageScope::Workspace, StorageTarget::Machine);
    }

    /// `JSON.parse(storage.get(key, WORKSPACE, '[]'))`, com o log do erro e a lista vazia em caso de falha.
    fn load_list<T>(&self, key: &str, what: &str, build: impl Fn(&Value) -> Option<T>) -> Vec<T> {
        let text = self.storage_service.get(key, StorageScope::Workspace, Some("[]")).unwrap_or_else(|| "[]".to_owned());
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Array(items)) => {
                let result: Vec<T> = items.iter().filter_map(&build).collect();
                if result.len() != items.len() {
                    self.log_service.error(&format!("Failed to load {what} from storage: invalid entry"));
                }
                result
            }
            _ => {
                self.log_service.error(&format!("Failed to load {what} from storage"));
                Vec::new()
            }
        }
    }

    fn load_breakpoints(&self) -> Vec<Rc<Breakpoint>> {
        self.load_list(DEBUG_BREAKPOINTS_KEY, "breakpoints", |value| {
            Breakpoint::from_json(value, self.text_file_service.clone(), self.uri_identity_service.clone(), self.log_service.clone())
        })
    }

    fn load_function_breakpoints(&self) -> Vec<Rc<FunctionBreakpoint>> {
        self.load_list(DEBUG_FUNCTION_BREAKPOINTS_KEY, "function breakpoints", |value| Some(FunctionBreakpoint::from_json(value)))
    }

    fn load_exception_breakpoints(&self) -> Vec<Rc<ExceptionBreakpoint>> {
        self.load_list(DEBUG_EXCEPTION_BREAKPOINTS_KEY, "exception breakpoints", |value| {
            Some(ExceptionBreakpoint::new(ExceptionBreakpointOptions::from_json(value), value.get("id").and_then(Value::as_str).map(str::to_owned)))
        })
    }

    fn load_data_breakpoints(&self) -> Vec<Rc<DataBreakpoint>> {
        self.load_list(DEBUG_DATA_BREAKPOINTS_KEY, "data breakpoints", |value| Some(DataBreakpoint::from_json(value)))
    }

    fn load_watch_expressions(&self) -> Vec<Rc<Expression>> {
        self.load_list(DEBUG_WATCH_EXPRESSIONS_KEY, "watch expressions", |value| {
            Some(Expression::new(value.get("name").and_then(Value::as_str).unwrap_or_default(), value.get("id").and_then(Value::as_str).map(str::to_owned)))
        })
    }

    /// `loadChosenEnvironments()`: back compat from when this was a string map.
    pub fn load_chosen_environments(&self) -> Vec<(String, IChosenEnvironment)> {
        let text = self.storage_service.get(DEBUG_CHOSEN_ENVIRONMENTS_KEY, StorageScope::Workspace, Some("{}")).unwrap_or_else(|| "{}".to_owned());
        let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&text) else {
            return Vec::new();
        };
        object
            .iter()
            .map(|(key, value)| {
                let environment = match value {
                    Value::String(r#type) => IChosenEnvironment { r#type: r#type.clone(), dynamic_label: None },
                    value => IChosenEnvironment {
                        r#type: value.get("type").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        dynamic_label: value.get("dynamicLabel").and_then(Value::as_str).map(str::to_owned),
                    },
                };
                (key.clone(), environment)
            })
            .collect()
    }

    /// `storeChosenEnvironments(environments)`.
    pub fn store_chosen_environments(&self, environments: &[(String, IChosenEnvironment)]) {
        let object: serde_json::Map<String, Value> = environments
            .iter()
            .map(|(key, environment)| {
                let mut entry = serde_json::Map::new();
                entry.insert("type".into(), json!(environment.r#type));
                if let Some(label) = &environment.dynamic_label {
                    entry.insert("dynamicLabel".into(), json!(label));
                }
                (key.clone(), Value::Object(entry))
            })
            .collect();
        self.storage_service.store(DEBUG_CHOSEN_ENVIRONMENTS_KEY, StorageValue::String(Value::Object(object).to_string()), StorageScope::Workspace, StorageTarget::Machine);
    }

    /// `storeWatchExpressions(watchExpressions)`.
    pub fn store_watch_expressions(&self, watch_expressions: &[Rc<Expression>]) {
        if !watch_expressions.is_empty() {
            let value: Vec<Value> = watch_expressions.iter().map(|we| json!({ "name": we.name.borrow().clone(), "id": we.base.get_id() })).collect();
            self.storage_service.store(DEBUG_WATCH_EXPRESSIONS_KEY, StorageValue::String(Value::Array(value).to_string()), StorageScope::Workspace, StorageTarget::Machine);
        } else {
            self.storage_service.remove(DEBUG_WATCH_EXPRESSIONS_KEY, StorageScope::Workspace);
        }
    }

    /// Grava a lista (`JSON.stringify(list)`) ou remove a chave quando a lista está vazia.
    fn store_list(&self, key: &str, list: Vec<Value>) {
        if !list.is_empty() {
            self.storage_service.store(key, StorageValue::String(Value::Array(list).to_string()), StorageScope::Workspace, StorageTarget::Machine);
        } else {
            self.storage_service.remove(key, StorageScope::Workspace);
        }
    }

    /// `storeBreakpoints(debugModel)`.
    pub fn store_breakpoints(&self, debug_model: &DebugModel) {
        self.store_list(DEBUG_BREAKPOINTS_KEY, debug_model.get_breakpoints(None).iter().map(|bp| bp.to_json()).collect());
        self.store_list(DEBUG_FUNCTION_BREAKPOINTS_KEY, debug_model.get_function_breakpoints().iter().map(|bp| bp.to_json()).collect());
        self.store_list(DEBUG_DATA_BREAKPOINTS_KEY, debug_model.get_data_breakpoints().iter().filter(|dbp| dbp.can_persist).map(|bp| bp.to_json()).collect());
        self.store_list(DEBUG_EXCEPTION_BREAKPOINTS_KEY, debug_model.get_exception_breakpoints().iter().map(|bp| bp.to_json()).collect());
    }
}

impl IDisposable for DebugStorage {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// `IBreakpointsChangeEvent` do modelo (`added`/`removed`/`changed` com os breakpoints concretos).
#[derive(Clone, Default)]
pub struct BreakpointsChangeEvent {
    pub added: Option<Vec<BreakpointElement>>,
    pub removed: Option<Vec<BreakpointElement>>,
    pub changed: Option<Vec<BreakpointElement>>,
    pub session_only: bool,
}

/// `IBreakpoint | IFunctionBreakpoint | IDataBreakpoint | IInstructionBreakpoint | IExceptionBreakpoint`.
#[derive(Clone)]
pub enum BreakpointElement {
    Breakpoint(Rc<Breakpoint>),
    Function(Rc<FunctionBreakpoint>),
    Exception(Rc<ExceptionBreakpoint>),
    Data(Rc<DataBreakpoint>),
    Instruction(Rc<InstructionBreakpoint>),
}

impl BreakpointElement {
    pub fn get_id(&self) -> String {
        match self {
            Self::Breakpoint(bp) => bp.get_id(),
            Self::Function(bp) => bp.get_id(),
            Self::Exception(bp) => bp.get_id(),
            Self::Data(bp) => bp.get_id(),
            Self::Instruction(bp) => bp.get_id(),
        }
    }

    fn base(&self) -> &BaseBreakpoint {
        match self {
            Self::Breakpoint(bp) => &bp.base,
            Self::Function(bp) => &bp.base,
            Self::Exception(bp) => &bp.base,
            Self::Data(bp) => &bp.base,
            Self::Instruction(bp) => &bp.base,
        }
    }

    pub fn enabled(&self) -> bool {
        self.base().enabled()
    }
}

/// O filtro de `getBreakpoints(filter)`.
#[derive(Clone, Debug, Default)]
pub struct BreakpointFilter {
    pub uri: Option<URI>,
    pub original_uri: Option<URI>,
    pub line_number: Option<i64>,
    pub column: Option<i64>,
    pub enabled_only: bool,
    pub triggered_only: bool,
}

/// `IBreakpointModeInternal`.
#[derive(Clone, Debug, PartialEq)]
pub struct BreakpointMode {
    pub mode: String,
    pub label: String,
    pub description: Option<String>,
    pub applies_to: Vec<String>,
    pub first_from_debug_type: String,
}

/// O que `refreshTopOfCallstack` devolve.
pub struct RefreshResult {
    pub top_call_stack: Promise<()>,
    pub whole_call_stack: Promise<()>,
}

struct SchedulerEntry {
    scheduler: Rc<je_base::async_::RunOnceScheduler>,
    complete_deferred: Rc<DeferredPromise<()>>,
}

/// `DebugModel`.
pub struct DebugModel {
    store: Rc<DisposableStore>,
    debug_storage: Rc<DebugStorage>,
    text_file_service: Rc<dyn IDirtyChecker>,
    uri_identity_service: Option<Rc<dyn IUriIdentityService>>,
    log_service: Rc<dyn LogService>,
    sessions: RefCell<Vec<Rc<dyn IDebugSessionFull>>>,
    schedulers: RefCell<HashMap<String, SchedulerEntry>>,
    breakpoints_activated: Cell<bool>,
    on_did_change_breakpoints: Rc<Emitter<Option<BreakpointsChangeEvent>>>,
    on_did_change_call_stack: Rc<Emitter<()>>,
    on_did_change_call_stack_fire: Rc<je_base::async_::RunOnceScheduler>,
    on_did_change_watch_expressions: Rc<Emitter<Option<Rc<Expression>>>>,
    on_did_change_watch_expression_value: Rc<Emitter<Option<Rc<Expression>>>>,
    breakpoint_modes: RefCell<Vec<(String, BreakpointMode)>>,
    breakpoints: RefCell<Vec<Rc<Breakpoint>>>,
    function_breakpoints: RefCell<Vec<Rc<FunctionBreakpoint>>>,
    exception_breakpoints: RefCell<Vec<Rc<ExceptionBreakpoint>>>,
    data_breakpoints: RefCell<Vec<Rc<DataBreakpoint>>>,
    watch_expressions: RefCell<Vec<Rc<Expression>>>,
    instruction_breakpoints: RefCell<Vec<Rc<InstructionBreakpoint>>>,
    watch_subscriptions: RefCell<Vec<(String, DisposableRef)>>,
}

impl DebugModel {
    pub fn new(
        debug_storage: Rc<DebugStorage>,
        text_file_service: Rc<dyn IDirtyChecker>,
        uri_identity_service: Option<Rc<dyn IUriIdentityService>>,
        log_service: Rc<dyn LogService>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let on_did_change_call_stack = Emitter::new();
        store.add(on_did_change_call_stack.clone());
        let fire = on_did_change_call_stack.clone();
        let on_did_change_call_stack_fire = je_base::async_::RunOnceScheduler::new(move || fire.fire(()), 100);
        let on_did_change_breakpoints = Emitter::new();
        store.add(on_did_change_breakpoints.clone());
        let on_did_change_watch_expressions = Emitter::new();
        store.add(on_did_change_watch_expressions.clone());
        let on_did_change_watch_expression_value = Emitter::new();
        store.add(on_did_change_watch_expression_value.clone());

        let model = Rc::new(Self {
            store,
            debug_storage: debug_storage.clone(),
            text_file_service,
            uri_identity_service,
            log_service,
            sessions: RefCell::new(Vec::new()),
            schedulers: RefCell::new(HashMap::new()),
            breakpoints_activated: Cell::new(true),
            on_did_change_breakpoints,
            on_did_change_call_stack,
            on_did_change_call_stack_fire,
            on_did_change_watch_expressions,
            on_did_change_watch_expression_value,
            breakpoint_modes: RefCell::new(Vec::new()),
            breakpoints: RefCell::new(Vec::new()),
            function_breakpoints: RefCell::new(Vec::new()),
            exception_breakpoints: RefCell::new(Vec::new()),
            data_breakpoints: RefCell::new(Vec::new()),
            watch_expressions: RefCell::new(Vec::new()),
            instruction_breakpoints: RefCell::new(Vec::new()),
            watch_subscriptions: RefCell::new(Vec::new()),
        });

        // `autorun` dos breakpoints: lê as listas do armazenamento e avisa.
        let weak = Rc::downgrade(&model);
        let read_breakpoints = move || {
            if let Some(model) = weak.upgrade() {
                model.read_breakpoints_from_storage();
                model.on_did_change_breakpoints.fire(None);
            }
        };
        read_breakpoints();
        model.store.add(debug_storage.on_did_change_breakpoints.event().listen(move |_| read_breakpoints()));

        // `autorun` das expressões de observação e o `trackSetChanges` que repassa `onDidChangeValue`.
        let weak = Rc::downgrade(&model);
        let read_watch = move || {
            if let Some(model) = weak.upgrade() {
                *model.watch_expressions.borrow_mut() = model.debug_storage.watch_expressions.borrow().clone();
                model.on_did_change_watch_expressions.fire(None);
            }
        };
        read_watch();
        let weak = Rc::downgrade(&model);
        model.track_watch_expressions();
        model.store.add(debug_storage.on_did_change_watch_expressions.event().listen(move |_| {
            read_watch();
            if let Some(model) = weak.upgrade() {
                model.track_watch_expressions();
            }
        }));
        model
    }

    fn read_breakpoints_from_storage(&self) {
        *self.breakpoints.borrow_mut() = self.debug_storage.breakpoints.borrow().clone();
        *self.function_breakpoints.borrow_mut() = self.debug_storage.function_breakpoints.borrow().clone();
        *self.exception_breakpoints.borrow_mut() = self.debug_storage.exception_breakpoints.borrow().clone();
        *self.data_breakpoints.borrow_mut() = self.debug_storage.data_breakpoints.borrow().clone();
    }

    /// `trackSetChanges(() => new Set(watchExpressions), onDidChangeWatchExpressions, we => we.onDidChangeValue(...))`.
    fn track_watch_expressions(self: &Rc<Self>) {
        let current: Vec<Rc<Expression>> = self.watch_expressions.borrow().clone();
        let removed: Vec<DisposableRef> = {
            let mut subscriptions = self.watch_subscriptions.borrow_mut();
            let (kept, removed): (Vec<_>, Vec<_>) = std::mem::take(&mut *subscriptions).into_iter().partition(|(id, _)| current.iter().any(|we| we.base.get_id() == *id));
            *subscriptions = kept;
            removed.into_iter().map(|(_, d)| d).collect()
        };
        for subscription in removed {
            subscription.dispose();
        }
        for we in current {
            let id = we.base.get_id();
            if self.watch_subscriptions.borrow().iter().any(|(existing, _)| *existing == id) {
                continue;
            }
            let emitter = self.on_did_change_watch_expression_value.clone();
            let subscription = we.on_did_change_value().listen(move |e| emitter.fire(Some(e.clone())));
            self.watch_subscriptions.borrow_mut().push((id, subscription));
        }
    }

    pub fn get_id(&self) -> String {
        "root".to_owned()
    }

    pub fn get_session(&self, session_id: Option<&str>, include_inactive: bool) -> Option<Rc<dyn IDebugSessionFull>> {
        let session_id = session_id.filter(|id| !id.is_empty())?;
        self.get_sessions(include_inactive).into_iter().find(|s| s.get_id() == session_id)
    }

    pub fn get_sessions(&self, include_inactive: bool) -> Vec<Rc<dyn IDebugSessionFull>> {
        // By default do not return inactive sessions.
        // However we are still holding onto inactive sessions due to repl and debug service session revival (eh scenario)
        self.sessions.borrow().iter().filter(|s| include_inactive || s.state() != State::Inactive).cloned().collect()
    }

    fn should_dispose_session(&self, session: &Rc<dyn IDebugSessionFull>, new_session: &Rc<dyn IDebugSessionFull>) -> bool {
        if session.state() != State::Inactive {
            return false;
        }
        if config_name(&session.configuration()) == config_name(&new_session.configuration()) {
            return true;
        }
        if new_session.parent_session().is_some() {
            return false;
        }
        let mut root_session: Rc<dyn IDebugSession> = session.clone();
        while let Some(parent) = root_session.parent_session() {
            root_session = parent;
        }
        // O `rootSession.state` é do contrato completo: acha-se a sessão de mesmo id na lista.
        let root_id = root_session.get_id();
        let root_inactive = self.sessions.borrow().iter().find(|s| s.get_id() == root_id).is_none_or(|s| s.state() == State::Inactive);
        root_inactive && config_name(&root_session.configuration()) == config_name(&new_session.configuration())
    }

    pub fn add_session(&self, session: Rc<dyn IDebugSessionFull>) {
        let existing: Vec<Rc<dyn IDebugSessionFull>> = self.sessions.borrow().clone();
        let mut kept = Vec::new();
        for s in existing {
            if s.get_id() == session.get_id() {
                // Make sure to de-dupe if a session is re-initialized. In case of EH debugging we are adding a session again after an attach.
                continue;
            }
            if self.should_dispose_session(&s, &session) {
                // Make sure to remove all inactive sessions that are using the same configuration as the new session
                s.dispose();
                continue;
            }
            kept.push(s);
        }
        *self.sessions.borrow_mut() = kept;

        let mut i = 1;
        while self.sessions.borrow().iter().any(|s| s.get_label() == session.get_label()) {
            i += 1;
            session.set_name(&format!("{} {}", config_name(&session.configuration()), i));
        }

        let mut index: Option<usize> = None;
        if let Some(parent) = session.parent_session() {
            // Make sure that child sessions are placed after the parent session
            let parent_id = parent.get_id();
            index = self.sessions.borrow().iter().rposition(|s| s.parent_session().is_some_and(|p| p.get_id() == parent_id) || s.get_id() == parent_id);
        }
        match index {
            Some(index) => self.sessions.borrow_mut().insert(index + 1, session),
            None => self.sessions.borrow_mut().push(session),
        }
        self.on_did_change_call_stack.fire(());
    }

    pub fn on_did_change_breakpoints(&self) -> Event<Option<BreakpointsChangeEvent>> {
        self.on_did_change_breakpoints.event()
    }

    pub fn on_did_change_call_stack(&self) -> Event<()> {
        self.on_did_change_call_stack.event()
    }

    pub fn on_did_change_watch_expressions(&self) -> Event<Option<Rc<Expression>>> {
        self.on_did_change_watch_expressions.event()
    }

    pub fn on_did_change_watch_expression_value(&self) -> Event<Option<Rc<Expression>>> {
        self.on_did_change_watch_expression_value.event()
    }

    pub fn raw_update(&self, data: IRawModelUpdate) {
        let session = self.sessions.borrow().iter().find(|p| p.get_id() == data.session_id).cloned();
        if let Some(session) = session {
            session.raw_update(data);
            self.on_did_change_call_stack.fire(());
        }
    }

    pub fn clear_threads(&self, id: &str, remove_threads: bool, reference: Option<i64>) {
        let session = self.sessions.borrow().iter().find(|p| p.get_id() == id).cloned();
        if let Some(session) = session {
            let threads: Vec<Rc<Thread>> = match reference {
                None => session.get_all_threads(),
                Some(reference) => session.get_thread(reference).into_iter().collect(),
            };
            for thread in threads {
                let thread_id = thread.get_id();
                let entry = self.schedulers.borrow_mut().remove(&thread_id);
                if let Some(entry) = entry {
                    entry.scheduler.cancel();
                    entry.complete_deferred.complete(());
                }
            }

            session.clear_threads(remove_threads, reference);
            if !self.on_did_change_call_stack_fire.is_scheduled() {
                self.on_did_change_call_stack_fire.schedule(None);
            }
        }
    }

    /// Update the call stack and notify the call stack view that changes have occurred.
    pub fn fetch_callstack(self: &Rc<Self>, thread: Rc<Thread>, levels: Option<i64>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if thread.reached_end_of_call_stack.get() {
                return Ok(());
            }

            let total_frames = thread.stopped_details().and_then(|d| d.total_frames);
            let remaining_frames = total_frames.map(|total| total - thread.get_call_stack().len() as i64);

            let mut levels = levels;
            if levels.is_none_or(|l| l == 0) || remaining_frames.is_some_and(|remaining| remaining != 0 && levels.is_some_and(|l| l > remaining)) {
                levels = remaining_frames;
            }

            if let Some(levels) = levels.filter(|l| *l > 0) {
                thread.fetch_call_stack(Some(levels)).await?;
                this.on_did_change_call_stack.fire(());
            }
            Ok(())
        })
    }

    pub fn refresh_top_of_callstack(self: &Rc<Self>, thread: Rc<dyn CallStackThread>, fetch_full_stack: bool) -> RefreshResult {
        if capability(&thread.session_capabilities(), "supportsDelayedStackTraceLoading") {
            // For improved performance load the first stack frame and then load the rest async.
            let (whole_call_stack, resolver) = Promise::<()>::new();
            let this = self.clone();
            let fetch_thread = thread.clone();
            let top_call_stack = thread.fetch_call_stack(Some(1)).then(move |_| {
                let thread = fetch_thread;
                if !fetch_full_stack {
                    resolver.resolve(());
                    this.on_did_change_call_stack.fire(());
                    return;
                }

                if !this.schedulers.borrow().contains_key(&thread.get_id()) {
                    let deferred = Rc::new(DeferredPromise::<()>::new());
                    let runner_this = this.clone();
                    let runner_thread = thread.clone();
                    let runner_deferred = deferred.clone();
                    let scheduler = je_base::async_::RunOnceScheduler::new(
                        move || {
                            let model = runner_this.clone();
                            let thread = runner_thread.clone();
                            let deferred = runner_deferred.clone();
                            let was_call_stack_complete = thread.reached_end_of_call_stack();
                            let after = thread.clone();
                            let changed_model = model.clone();
                            let finally_model = model.clone();
                            let finally_thread = thread.clone();
                            thread
                                .fetch_call_stack(Some(19))
                                .then(move |_| {
                                    let stale = after.get_stale_call_stack();
                                    let current = after.get_call_stack();
                                    // A thread's call stack can become complete without adding frames, so fire the change event to notify consumers.
                                    let mut call_stack_changed = stale.len() != current.len() || was_call_stack_complete != after.reached_end_of_call_stack();
                                    let mut i = 1;
                                    while i < stale.len() && !call_stack_changed {
                                        call_stack_changed = !stale[i].equals(&current[i]);
                                        i += 1;
                                    }

                                    if call_stack_changed {
                                        changed_model.on_did_change_call_stack.fire(());
                                    }
                                })
                                .finally(move || {
                                    deferred.complete(());
                                    finally_model.schedulers.borrow_mut().remove(&finally_thread.get_id());
                                });
                        },
                        420,
                    );
                    this.schedulers.borrow_mut().insert(thread.get_id(), SchedulerEntry { scheduler, complete_deferred: deferred });
                }

                let (scheduler, deferred) = {
                    let schedulers = this.schedulers.borrow();
                    let entry = schedulers.get(&thread.get_id()).expect("agendador");
                    (entry.scheduler.clone(), entry.complete_deferred.clone())
                };
                scheduler.schedule(None);
                let ok = resolver.clone();
                let err = resolver.clone();
                deferred.promise.then_result(move |result| {
                    match result {
                        Ok(()) => ok.resolve(()),
                        Err(error) => err.reject(error),
                    }
                    Ok(())
                });
                this.on_did_change_call_stack.fire(());
            });

            return RefreshResult { top_call_stack, whole_call_stack };
        }

        let whole_call_stack = thread.fetch_call_stack(None);
        RefreshResult { whole_call_stack: whole_call_stack.clone(), top_call_stack: whole_call_stack }
    }

    pub fn get_breakpoints(&self, filter: Option<&BreakpointFilter>) -> Vec<Rc<Breakpoint>> {
        let Some(filter) = filter else {
            return self.breakpoints.borrow().clone();
        };
        let uri_str = filter.uri.as_ref().map(|u| u.to_string(false).to_string_lossy());
        let original_uri_str = filter.original_uri.as_ref().map(|u| u.to_string(false).to_string_lossy());
        self.breakpoints
            .borrow()
            .iter()
            .filter(|bp| {
                if let Some(uri_str) = uri_str.as_ref().filter(|s| !s.is_empty())
                    && bp.uri().to_string(false).to_string_lossy() != *uri_str
                {
                    return false;
                }
                if let Some(original) = original_uri_str.as_ref().filter(|s| !s.is_empty())
                    && bp.original_uri().to_string(false).to_string_lossy() != *original
                {
                    return false;
                }
                if filter.line_number.is_some_and(|l| l != 0 && bp.line_number() != l) {
                    return false;
                }
                if filter.column.is_some_and(|c| c != 0 && bp.column() != Some(c)) {
                    return false;
                }
                if filter.enabled_only && (!self.breakpoints_activated.get() || !bp.enabled()) {
                    return false;
                }
                if filter.triggered_only && bp.triggered_by.borrow().is_none() {
                    return false;
                }
                true
            })
            .cloned()
            .collect()
    }

    pub fn get_function_breakpoints(&self) -> Vec<Rc<FunctionBreakpoint>> {
        self.function_breakpoints.borrow().clone()
    }

    pub fn get_data_breakpoints(&self) -> Vec<Rc<DataBreakpoint>> {
        self.data_breakpoints.borrow().clone()
    }

    pub fn get_exception_breakpoints(&self) -> Vec<Rc<ExceptionBreakpoint>> {
        self.exception_breakpoints.borrow().clone()
    }

    pub fn get_exception_breakpoints_for_session(&self, session_id: Option<&str>) -> Vec<Rc<ExceptionBreakpoint>> {
        self.exception_breakpoints.borrow().iter().filter(|ebp| ebp.is_supported_session(session_id)).cloned().collect()
    }

    pub fn get_instruction_breakpoints(&self) -> Vec<Rc<InstructionBreakpoint>> {
        self.instruction_breakpoints.borrow().clone()
    }

    pub fn set_exception_breakpoints_for_session(&self, session_id: &str, filters: &[ExceptionBreakpointsFilter]) {
        let mut did_change_breakpoints = false;
        for d in filters {
            let existing = self.exception_breakpoints.borrow().iter().rev().find(|exbp| exbp.matches(d)).cloned();
            let ebp = match existing {
                Some(ebp) => ebp,
                None => {
                    did_change_breakpoints = true;
                    let ebp = ExceptionBreakpoint::new(
                        ExceptionBreakpointOptions {
                            base: IBaseBreakpointOptions { enabled: Some(d.default.unwrap_or(false)), ..IBaseBreakpointOptions::default() },
                            filter: d.filter.clone(),
                            label: d.label.clone(),
                            supports_condition: d.supports_condition.unwrap_or(false),
                            description: d.description.clone(),
                            condition_description: d.condition_description.clone(),
                            fallback: None,
                        },
                        None,
                    );
                    self.exception_breakpoints.borrow_mut().push(ebp.clone());
                    ebp
                }
            };

            ebp.set_supported_session(session_id, true);
        }

        if did_change_breakpoints {
            self.on_did_change_breakpoints.fire(None);
        }
    }

    pub fn remove_exception_breakpoints_for_session(&self, session_id: &str) {
        for ebp in self.exception_breakpoints.borrow().iter() {
            ebp.set_supported_session(session_id, false);
        }
    }

    // Set last focused session as fallback session.
    // This is done to keep track of the exception breakpoints to show when no session is active.
    pub fn set_exception_breakpoint_fallback_session(&self, session_id: &str) {
        for ebp in self.exception_breakpoints.borrow().iter() {
            ebp.set_fallback(ebp.is_supported_session(Some(session_id)));
        }
    }

    pub fn set_exception_breakpoint_condition(&self, exception_breakpoint: &Rc<ExceptionBreakpoint>, condition: Option<String>) {
        *exception_breakpoint.base.condition.borrow_mut() = condition;
        self.on_did_change_breakpoints.fire(None);
    }

    pub fn are_breakpoints_activated(&self) -> bool {
        self.breakpoints_activated.get()
    }

    pub fn set_breakpoints_activated(&self, activated: bool) {
        self.breakpoints_activated.set(activated);
        self.on_did_change_breakpoints.fire(None);
    }

    pub fn add_breakpoints(&self, uri: &URI, raw_data: Vec<BreakpointData>, fire_event: bool) -> Vec<Rc<Breakpoint>> {
        let uri_identity_service = self.uri_identity_service.clone().expect("o serviço de identidade de URI");
        let new_breakpoints: Vec<Rc<Breakpoint>> = raw_data
            .into_iter()
            .map(|raw_bp| {
                Breakpoint::new(
                    IBreakpointOptions {
                        uri: uri.clone(),
                        line_number: raw_bp.line_number,
                        column: raw_bp.column,
                        base: IBaseBreakpointOptions {
                            enabled: Some(raw_bp.enabled.unwrap_or(true)),
                            condition: raw_bp.condition,
                            hit_condition: raw_bp.hit_condition,
                            log_message: raw_bp.log_message,
                            mode: raw_bp.mode,
                            mode_label: raw_bp.mode_label,
                        },
                        triggered_by: raw_bp.triggered_by,
                        adapter_data: None,
                    },
                    self.text_file_service.clone(),
                    uri_identity_service.clone(),
                    self.log_service.clone(),
                    raw_bp.id,
                )
            })
            .collect();
        self.breakpoints.borrow_mut().extend(new_breakpoints.iter().cloned());
        self.breakpoints_activated.set(true);
        self.sort_and_de_dup();

        if fire_event {
            self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent {
                added: Some(new_breakpoints.iter().cloned().map(BreakpointElement::Breakpoint).collect()),
                ..BreakpointsChangeEvent::default()
            }));
        }

        new_breakpoints
    }

    pub fn remove_breakpoints(&self, to_remove: &[Rc<Breakpoint>]) {
        self.breakpoints.borrow_mut().retain(|bp| !to_remove.iter().any(|r| r.get_id() == bp.get_id()));
        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent {
            removed: Some(to_remove.iter().cloned().map(BreakpointElement::Breakpoint).collect()),
            ..BreakpointsChangeEvent::default()
        }));
    }

    pub fn update_breakpoints(&self, data: &HashMap<String, IBreakpointUpdateData>) {
        let mut updated = Vec::new();
        for bp in self.breakpoints.borrow().iter() {
            if let Some(bp_data) = data.get(&bp.get_id()) {
                bp.update(bp_data);
                updated.push(BreakpointElement::Breakpoint(bp.clone()));
            }
        }
        self.sort_and_de_dup();
        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent { changed: Some(updated), ..BreakpointsChangeEvent::default() }));
    }

    pub fn set_breakpoint_session_data(&self, session_id: &str, capabilites: &Value, data: Option<&HashMap<String, DapBreakpoint>>) {
        let apply = |id: String, set: &dyn Fn(Option<IBreakpointSessionData>)| match data {
            None => set(None),
            Some(data) => {
                if let Some(bp_data) = data.get(&id) {
                    set(Some(to_breakpoint_session_data(bp_data.clone(), capabilites)));
                }
            }
        };
        for bp in self.breakpoints.borrow().iter() {
            apply(bp.get_id(), &|d| bp.set_session_data(session_id, d));
        }
        for fbp in self.function_breakpoints.borrow().iter() {
            apply(fbp.get_id(), &|d| fbp.base.set_session_data(session_id, d));
        }
        for dbp in self.data_breakpoints.borrow().iter() {
            apply(dbp.get_id(), &|d| dbp.base.set_session_data(session_id, d));
        }
        for ebp in self.exception_breakpoints.borrow().iter() {
            apply(ebp.get_id(), &|d| ebp.base.set_session_data(session_id, d));
        }
        for ibp in self.instruction_breakpoints.borrow().iter() {
            apply(ibp.get_id(), &|d| ibp.base.set_session_data(session_id, d));
        }

        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent { session_only: true, ..BreakpointsChangeEvent::default() }));
    }

    pub fn get_debug_protocol_breakpoint(&self, breakpoint_id: &str, session_id: &str) -> Option<DapBreakpoint> {
        let bp = self.breakpoints.borrow().iter().find(|bp| bp.get_id() == breakpoint_id).cloned()?;
        bp.base.get_debug_protocol_breakpoint(session_id)
    }

    pub fn get_breakpoint_modes(&self, for_breakpoint_type: &str) -> Vec<BreakpointMode> {
        self.breakpoint_modes.borrow().iter().map(|(_, m)| m).filter(|mode| mode.applies_to.iter().any(|t| t == for_breakpoint_type)).cloned().collect()
    }

    pub fn register_breakpoint_modes(&self, debug_type: &str, modes: &[BreakpointMode]) {
        for mode in modes {
            let key = format!("{}/{}", mode.mode, mode.label);
            let mut registered = self.breakpoint_modes.borrow_mut();
            if let Some((_, rec)) = registered.iter_mut().find(|(k, _)| *k == key) {
                for target in &mode.applies_to {
                    if !rec.applies_to.contains(target) {
                        rec.applies_to.push(target.clone());
                    }
                }
            } else {
                let duplicate = registered.iter_mut().find(|(_, r)| r.label == mode.label);
                let has_duplicate = duplicate.is_some();
                if let Some((_, duplicate)) = duplicate {
                    duplicate.label = format!("{} ({})", duplicate.label, duplicate.first_from_debug_type);
                }

                registered.push((
                    key,
                    BreakpointMode {
                        mode: mode.mode.clone(),
                        label: if has_duplicate { format!("{} ({})", mode.label, debug_type) } else { mode.label.clone() },
                        first_from_debug_type: debug_type.to_owned(),
                        description: mode.description.clone(),
                        applies_to: mode.applies_to.clone(),
                    },
                ));
            }
        }
    }

    fn sort_and_de_dup(&self) {
        let mut breakpoints = self.breakpoints.borrow().clone();
        sort_by_compare(&mut breakpoints, |first, second| {
            if first.uri().to_string(false) != second.uri().to_string(false) {
                return je_base::collator::locale_compare(&ext_uri().basename_or_authority(&first.uri()), &ext_uri().basename_or_authority(&second.uri())) as i32 as f64;
            }
            if first.line_number() == second.line_number() {
                if let (Some(a), Some(b)) = (first.column().filter(|c| *c != 0), second.column().filter(|c| *c != 0)) {
                    return (a - b) as f64;
                }
                return 1.0;
            }

            (first.line_number() - second.line_number()) as f64
        });
        *self.breakpoints.borrow_mut() = je_base::arrays::distinct(&breakpoints, |bp| format!("{}:{}:{:?}", bp.uri().to_string(false).to_string_lossy(), bp.line_number(), bp.column()));
    }

    pub fn set_enablement(&self, element: &BreakpointElement, enable: bool) {
        let mut changed = Vec::new();
        if element.enabled() != enable && !matches!(element, BreakpointElement::Exception(_)) {
            changed.push(element.clone());
        }

        element.base().set_enabled(enable);
        if enable {
            self.breakpoints_activated.set(true);
        }

        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent { changed: Some(changed), ..BreakpointsChangeEvent::default() }));
    }

    pub fn enable_or_disable_all_breakpoints(&self, enable: bool) {
        let mut changed = Vec::new();
        let mut visit = |element: BreakpointElement| {
            if element.enabled() != enable {
                changed.push(element.clone());
            }
            element.base().set_enabled(enable);
        };
        for bp in self.breakpoints.borrow().iter() {
            visit(BreakpointElement::Breakpoint(bp.clone()));
        }
        for fbp in self.function_breakpoints.borrow().iter() {
            visit(BreakpointElement::Function(fbp.clone()));
        }
        for dbp in self.data_breakpoints.borrow().iter() {
            visit(BreakpointElement::Data(dbp.clone()));
        }
        for ibp in self.instruction_breakpoints.borrow().iter() {
            visit(BreakpointElement::Instruction(ibp.clone()));
        }

        if enable {
            self.breakpoints_activated.set(true);
        }

        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent { changed: Some(changed), ..BreakpointsChangeEvent::default() }));
    }

    pub fn add_function_breakpoint(&self, opts: FunctionBreakpointOptions, id: Option<String>) -> Rc<FunctionBreakpoint> {
        let new_function_breakpoint = FunctionBreakpoint::new(opts, id);
        self.function_breakpoints.borrow_mut().push(new_function_breakpoint.clone());
        self.on_did_change_breakpoints
            .fire(Some(BreakpointsChangeEvent { added: Some(vec![BreakpointElement::Function(new_function_breakpoint.clone())]), ..BreakpointsChangeEvent::default() }));

        new_function_breakpoint
    }

    pub fn update_function_breakpoint(&self, id: &str, name: Option<&str>, hit_condition: Option<&str>, condition: Option<&str>) {
        let function_breakpoint = self.function_breakpoints.borrow().iter().find(|fbp| fbp.get_id() == id).cloned();
        if let Some(function_breakpoint) = function_breakpoint {
            if let Some(name) = name {
                *function_breakpoint.name.borrow_mut() = name.to_owned();
            }
            if let Some(condition) = condition {
                *function_breakpoint.base.condition.borrow_mut() = Some(condition.to_owned());
            }
            if let Some(hit_condition) = hit_condition {
                *function_breakpoint.base.hit_condition.borrow_mut() = Some(hit_condition.to_owned());
            }
            self.on_did_change_breakpoints
                .fire(Some(BreakpointsChangeEvent { changed: Some(vec![BreakpointElement::Function(function_breakpoint)]), ..BreakpointsChangeEvent::default() }));
        }
    }

    pub fn remove_function_breakpoints(&self, id: Option<&str>) {
        let removed: Vec<Rc<FunctionBreakpoint>> = if let Some(id) = id.filter(|id| !id.is_empty()) {
            let removed = self.function_breakpoints.borrow().iter().filter(|fbp| fbp.get_id() == id).cloned().collect();
            self.function_breakpoints.borrow_mut().retain(|fbp| fbp.get_id() != id);
            removed
        } else {
            std::mem::take(&mut *self.function_breakpoints.borrow_mut())
        };
        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent {
            removed: Some(removed.into_iter().map(BreakpointElement::Function).collect()),
            ..BreakpointsChangeEvent::default()
        }));
    }

    pub fn add_data_breakpoint(&self, opts: DataBreakpointOptions, id: Option<String>) {
        let new_data_breakpoint = DataBreakpoint::new(opts, id);
        self.data_breakpoints.borrow_mut().push(new_data_breakpoint.clone());
        self.on_did_change_breakpoints
            .fire(Some(BreakpointsChangeEvent { added: Some(vec![BreakpointElement::Data(new_data_breakpoint)]), ..BreakpointsChangeEvent::default() }));
    }

    pub fn update_data_breakpoint(&self, id: &str, hit_condition: Option<&str>, condition: Option<&str>) {
        let data_breakpoint = self.data_breakpoints.borrow().iter().find(|fbp| fbp.get_id() == id).cloned();
        if let Some(data_breakpoint) = data_breakpoint {
            if let Some(condition) = condition {
                *data_breakpoint.base.condition.borrow_mut() = Some(condition.to_owned());
            }
            if let Some(hit_condition) = hit_condition {
                *data_breakpoint.base.hit_condition.borrow_mut() = Some(hit_condition.to_owned());
            }
            self.on_did_change_breakpoints
                .fire(Some(BreakpointsChangeEvent { changed: Some(vec![BreakpointElement::Data(data_breakpoint)]), ..BreakpointsChangeEvent::default() }));
        }
    }

    pub fn remove_data_breakpoints(&self, id: Option<&str>) {
        let removed: Vec<Rc<DataBreakpoint>> = if let Some(id) = id.filter(|id| !id.is_empty()) {
            let removed = self.data_breakpoints.borrow().iter().filter(|fbp| fbp.get_id() == id).cloned().collect();
            self.data_breakpoints.borrow_mut().retain(|fbp| fbp.get_id() != id);
            removed
        } else {
            std::mem::take(&mut *self.data_breakpoints.borrow_mut())
        };
        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent {
            removed: Some(removed.into_iter().map(BreakpointElement::Data).collect()),
            ..BreakpointsChangeEvent::default()
        }));
    }

    pub fn add_instruction_breakpoint(&self, opts: InstructionBreakpointOptions) {
        let new_instruction_breakpoint = InstructionBreakpoint::new(opts, None);
        self.instruction_breakpoints.borrow_mut().push(new_instruction_breakpoint.clone());
        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent {
            added: Some(vec![BreakpointElement::Instruction(new_instruction_breakpoint)]),
            session_only: true,
            ..BreakpointsChangeEvent::default()
        }));
    }

    pub fn remove_instruction_breakpoints(&self, instruction_reference: Option<&str>, offset: Option<i64>, address: Option<u128>) {
        let removed: Vec<Rc<InstructionBreakpoint>>;
        if let Some(address) = address {
            // Prefer matching by resolved memory address: `instructionReference` is allowed by the Debug Adapter Protocol to
            // change between disassemble requests (e.g. after symbol reloads), so matching on reference+offset alone would
            // fail to locate the breakpoint that the user is trying to toggle off. The `address` on an
            // `InstructionBreakpoint` is the stable resolved memory address and uniquely identifies it.
            let (taken, kept): (Vec<_>, Vec<_>) = self.instruction_breakpoints.borrow().iter().cloned().partition(|ibp| ibp.address == address);
            *self.instruction_breakpoints.borrow_mut() = kept;
            removed = taken;
        } else if let Some(reference) = instruction_reference.filter(|r| !r.is_empty()) {
            let (taken, kept): (Vec<_>, Vec<_>) =
                self.instruction_breakpoints.borrow().iter().cloned().partition(|ibp| ibp.instruction_reference == reference && offset.is_none_or(|o| ibp.offset == o));
            *self.instruction_breakpoints.borrow_mut() = kept;
            removed = taken;
        } else {
            removed = std::mem::take(&mut *self.instruction_breakpoints.borrow_mut());
        }
        self.on_did_change_breakpoints.fire(Some(BreakpointsChangeEvent {
            removed: Some(removed.into_iter().map(BreakpointElement::Instruction).collect()),
            ..BreakpointsChangeEvent::default()
        }));
    }

    pub fn get_watch_expressions(&self) -> Vec<Rc<Expression>> {
        self.watch_expressions.borrow().clone()
    }

    pub fn add_watch_expression(self: &Rc<Self>, name: Option<&str>) -> Rc<Expression> {
        let we = Expression::new(name.unwrap_or(""), None);
        self.watch_expressions.borrow_mut().push(we.clone());
        self.on_did_change_watch_expressions.fire(Some(we.clone()));
        self.track_watch_expressions();

        we
    }

    pub fn rename_watch_expression(&self, id: &str, new_name: &str) {
        let filtered: Vec<Rc<Expression>> = self.watch_expressions.borrow().iter().filter(|we| we.base.get_id() == id).cloned().collect();
        if filtered.len() == 1 {
            *filtered[0].name.borrow_mut() = new_name.to_owned();
            self.on_did_change_watch_expressions.fire(Some(filtered[0].clone()));
        }
    }

    pub fn remove_watch_expressions(self: &Rc<Self>, id: Option<&str>) {
        match id.filter(|id| !id.is_empty()) {
            Some(id) => self.watch_expressions.borrow_mut().retain(|we| we.base.get_id() != id),
            None => self.watch_expressions.borrow_mut().clear(),
        }
        self.on_did_change_watch_expressions.fire(None);
        self.track_watch_expressions();
    }

    pub fn move_watch_expression(self: &Rc<Self>, id: &str, position: usize) {
        let we = self.watch_expressions.borrow().iter().find(|we| we.base.get_id() == id).cloned();
        if let Some(we) = we {
            self.watch_expressions.borrow_mut().retain(|e| e.base.get_id() != id);
            let mut list = self.watch_expressions.borrow().clone();
            let position = position.min(list.len());
            list.insert(position, we);
            *self.watch_expressions.borrow_mut() = list;
            self.on_did_change_watch_expressions.fire(None);
        }
    }

    pub fn source_is_not_available(&self, uri: &URI) {
        for s in self.sessions.borrow().iter() {
            if let Some(source) = s.get_source_for_uri(uri) {
                source.available.set(false);
            }
        }
        self.on_did_change_call_stack.fire(());
    }
}

impl IDisposable for DebugModel {
    fn dispose(&self) {
        self.on_did_change_call_stack_fire.cancel();
        for (_, subscription) in self.watch_subscriptions.borrow_mut().drain(..) {
            subscription.dispose();
        }
        self.store.dispose();
    }
}

/// `configuration.name` de uma `IConfig`.
fn config_name(config: &Value) -> Option<String> {
    config.get("name").and_then(Value::as_str).map(str::to_owned)
}

/// `new Source(undefined, ...)`/`ProtocolSource` ao alcance dos testes: o protocolo cru do quadro de pilha.
pub fn raw_source(source: &ProtocolSource) -> Value {
    super::breakpoints::protocol_source_to_value(source)
}
