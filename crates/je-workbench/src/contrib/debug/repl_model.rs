//! Porte de `src/vs/workbench/contrib/debug/common/replModel.ts`: o modelo lógico do REPL (o console de depuração): os
//! elementos de saída, de variável, de objeto cru, de entrada, de resultado e de grupo, e o `ReplModel` que os junta,
//! colapsa linhas idênticas e limita o tamanho.
//!
//! Notas de tradução:
//! - `IReplElement` e as classes que o implementam (testadas com `instanceof`) são o enum [`ReplElement`]; a estrutura
//!   genérica `IExpression` que o `getChildren` devolve é a variante [`ReplElement::Expression`].
//! - `IConfigurationService.getValue('debug')` é o fecho [`DebugConsoleConfigurationProvider`], que devolve só o que o
//!   modelo lê (`console.collapseIdenticalLines` e `console.maximumLines`), relido a cada uso como no original.
//! - `topReplElementCounter` e `ReplGroup.COUNTER` são contadores da thread (como o estado de módulo do original).
//! - `RawObjectReplElement.valueObj` (`any`) é um [`serde_json::Value`]; `isObject` é objeto não nulo e não lista.
//! - `INewReplElementData.sev` usa o [`Severity`] do je-base.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uuid::generate_uuid;
use serde_json::Value;

use super::debug_model::ExpressionContainer;
use super::debug_source::Source;
use super::debug_types::{IDebugSessionFull, IExpression, IExpressionContainer};

thread_local! {
    static TOP_REPL_ELEMENT_COUNTER: Cell<u64> = const { Cell::new(0) };
    static REPL_GROUP_COUNTER: Cell<u64> = const { Cell::new(0) };
}

/// `getUniqueId()`.
fn get_unique_id() -> String {
    TOP_REPL_ELEMENT_COUNTER.with(|counter| {
        let id = counter.get();
        counter.set(id + 1);
        format!("topReplElement:{id}")
    })
}

/// `IReplElementSource`.
#[derive(Clone)]
pub struct IReplElementSource {
    pub source: Rc<Source>,
    pub line_number: i64,
    pub column: i64,
}

/// O que o modelo lê de `IDebugConfiguration.console`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugConsoleConfiguration {
    pub collapse_identical_lines: bool,
    pub maximum_lines: usize,
}

/// `() => configurationService.getValue('debug').console`.
pub type DebugConsoleConfigurationProvider = Rc<dyn Fn() -> DebugConsoleConfiguration>;

/// `INewReplElementData`.
#[derive(Clone)]
pub struct NewReplElementData {
    pub output: String,
    pub expression: Option<Rc<dyn IExpression>>,
    pub sev: Severity,
    pub source: Option<IReplElementSource>,
}

/// `IReplElement` (as classes de `replModel.ts` e a expressão genérica).
#[derive(Clone)]
pub enum ReplElement {
    Output(Rc<ReplOutputElement>),
    Variable(Rc<ReplVariableElement>),
    Raw(Rc<RawObjectReplElement>),
    Input(Rc<ReplEvaluationInput>),
    Result(Rc<ReplEvaluationResult>),
    Group(Rc<ReplGroup>),
    Expression(Rc<dyn IExpression>),
}

impl ReplElement {
    pub fn get_id(&self) -> String {
        match self {
            Self::Output(e) => e.get_id(),
            Self::Variable(e) => e.get_id(),
            Self::Raw(e) => e.get_id(),
            Self::Input(e) => e.get_id(),
            Self::Result(e) => e.get_id(),
            Self::Group(e) => e.get_id(),
            Self::Expression(e) => e.get_id(),
        }
    }

    /// `toString(includeSource)`.
    pub fn to_display_string(&self, include_source: bool) -> String {
        match self {
            Self::Output(e) => e.to_display_string(include_source),
            Self::Variable(e) => e.to_display_string(),
            Self::Raw(e) => e.to_display_string(),
            Self::Input(e) => e.to_display_string(),
            Self::Result(e) => e.to_display_string(),
            Self::Group(e) => e.to_display_string(include_source),
            Self::Expression(e) => e.display(),
        }
    }

    pub fn source_data(&self) -> Option<IReplElementSource> {
        match self {
            Self::Output(e) => e.source_data.clone(),
            Self::Variable(e) => e.source_data.clone(),
            Self::Raw(e) => e.source_data.clone(),
            Self::Group(e) => e.source_data.clone(),
            _ => None,
        }
    }

    /// Identidade do objeto (`===`).
    pub fn ptr_eq(&self, other: &ReplElement) -> bool {
        match (self, other) {
            (Self::Output(a), Self::Output(b)) => Rc::ptr_eq(a, b),
            (Self::Variable(a), Self::Variable(b)) => Rc::ptr_eq(a, b),
            (Self::Raw(a), Self::Raw(b)) => Rc::ptr_eq(a, b),
            (Self::Input(a), Self::Input(b)) => Rc::ptr_eq(a, b),
            (Self::Result(a), Self::Result(b)) => Rc::ptr_eq(a, b),
            (Self::Group(a), Self::Group(b)) => Rc::ptr_eq(a, b),
            (Self::Expression(a), Self::Expression(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

fn expressions_to_elements(expressions: Promise<Vec<Rc<dyn IExpression>>>) -> Promise<Vec<ReplElement>> {
    expressions.then(|children| children.into_iter().map(ReplElement::Expression).collect())
}

/// General case of data from DAP the `output` event. [`ReplVariableElement`] is used instead only if there is a
/// `variablesReference` with no `output` text.
pub struct ReplOutputElement {
    count: Cell<i64>,
    on_did_change_count: Rc<Emitter<()>>,
    pub session: Rc<dyn IDebugSessionFull>,
    id: String,
    pub value: String,
    pub severity: Severity,
    pub source_data: Option<IReplElementSource>,
    pub expression: Option<Rc<dyn IExpression>>,
}

impl ReplOutputElement {
    pub fn new(
        session: Rc<dyn IDebugSessionFull>,
        id: String,
        value: String,
        severity: Severity,
        source_data: Option<IReplElementSource>,
        expression: Option<Rc<dyn IExpression>>,
    ) -> Rc<Self> {
        Rc::new(Self { count: Cell::new(1), on_did_change_count: Emitter::new(), session, id, value, severity, source_data, expression })
    }

    /// `toString(includeSource = false)`.
    pub fn to_display_string(&self, include_source: bool) -> String {
        let mut value_respect_count = self.value.clone();
        for _ in 1..self.count() {
            value_respect_count.push_str(if value_respect_count.ends_with('\n') { "" } else { "\n" });
            value_respect_count.push_str(&self.value);
        }
        let source_str = match (&self.source_data, include_source) {
            (Some(source_data), true) => format!(" {}", source_data.source.name()),
            _ => String::new(),
        };
        value_respect_count + &source_str
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }

    pub fn get_children(&self) -> Promise<Vec<ReplElement>> {
        match &self.expression {
            Some(expression) => expressions_to_elements(expression.get_children()),
            None => Promise::resolved(Vec::new()),
        }
    }

    /// `set count(value)`.
    pub fn set_count(&self, value: i64) {
        self.count.set(value);
        self.on_did_change_count.fire(());
    }

    pub fn count(&self) -> i64 {
        self.count.get()
    }

    pub fn on_did_change_count(&self) -> Event<()> {
        self.on_did_change_count.event()
    }

    pub fn has_children(&self) -> bool {
        self.expression.as_ref().is_some_and(|e| e.has_children())
    }
}

/// Top-level variable logged via DAP output when there's no `output` string.
pub struct ReplVariableElement {
    pub has_children: bool,
    id: String,
    session: Rc<dyn IDebugSessionFull>,
    pub expression: Rc<dyn IExpression>,
    pub severity: Severity,
    pub source_data: Option<IReplElementSource>,
}

impl ReplVariableElement {
    pub fn new(session: Rc<dyn IDebugSessionFull>, expression: Rc<dyn IExpression>, severity: Severity, source_data: Option<IReplElementSource>) -> Rc<Self> {
        Rc::new(Self { has_children: expression.has_children(), id: generate_uuid(), session, expression, severity, source_data })
    }

    pub fn get_session(&self) -> Rc<dyn IDebugSessionFull> {
        self.session.clone()
    }

    pub fn get_children(&self) -> Promise<Vec<ReplElement>> {
        expressions_to_elements(self.expression.get_children())
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        self.expression.display()
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }
}

/// `RawObjectReplElement`.
pub struct RawObjectReplElement {
    id: String,
    pub name: RefCell<String>,
    pub value_obj: Value,
    pub source_data: Option<IReplElementSource>,
    pub annotation: Option<String>,
}

fn is_object(value: &Value) -> bool {
    value.is_object()
}

impl RawObjectReplElement {
    /// upper bound of children per value
    const MAX_CHILDREN: usize = 1000;

    pub fn new(id: String, name: String, value_obj: Value, source_data: Option<IReplElementSource>, annotation: Option<String>) -> Rc<Self> {
        Rc::new(Self { id, name: RefCell::new(name), value_obj, source_data, annotation })
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }

    pub fn value(&self) -> String {
        match &self.value_obj {
            Value::Null => "null".to_owned(),
            Value::Array(items) => format!("Array[{}]", items.len()),
            value if is_object(value) => "Object".to_owned(),
            Value::String(text) => format!("\"{text}\""),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => number.to_string(),
            _ => String::new(),
        }
    }

    pub fn has_children(&self) -> bool {
        match &self.value_obj {
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
            _ => false,
        }
    }

    pub fn child_elements(&self) -> Vec<Rc<RawObjectReplElement>> {
        match &self.value_obj {
            Value::Array(items) => items
                .iter()
                .take(Self::MAX_CHILDREN)
                .enumerate()
                .map(|(index, v)| RawObjectReplElement::new(format!("{}:{}", self.id, index), index.to_string(), v.clone(), None, None))
                .collect(),
            Value::Object(map) => map
                .iter()
                .take(Self::MAX_CHILDREN)
                .enumerate()
                .map(|(index, (key, v))| RawObjectReplElement::new(format!("{}:{}", self.id, index), key.clone(), v.clone(), None, None))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        format!("{}\n{}", self.name.borrow(), self.value())
    }
}

impl IExpressionContainer for RawObjectReplElement {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn value(&self) -> String {
        RawObjectReplElement::value(self)
    }

    fn expr_type(&self) -> Option<String> {
        None
    }

    fn value_changed(&self) -> bool {
        false
    }

    fn has_children(&self) -> bool {
        RawObjectReplElement::has_children(self)
    }

    fn get_session(&self) -> Option<Rc<dyn IDebugSessionFull>> {
        None
    }

    fn evaluate_lazy(&self) -> Promise<()> {
        Promise::rejected(je_base::errors::Error::new("Method not implemented."))
    }

    fn get_children(&self) -> Promise<Vec<Rc<dyn IExpression>>> {
        Promise::resolved(self.child_elements().into_iter().map(|child| child as Rc<dyn IExpression>).collect())
    }

    fn reference(&self) -> Option<i64> {
        None
    }

    fn memory_reference(&self) -> Option<String> {
        None
    }

    fn presentation_hint(&self) -> Option<Value> {
        None
    }

    fn value_location_reference(&self) -> Option<i64> {
        None
    }

    fn display(&self) -> String {
        self.to_display_string()
    }
}

impl IExpression for RawObjectReplElement {
    fn name(&self) -> String {
        self.name.borrow().clone()
    }
}

/// `ReplEvaluationInput`.
pub struct ReplEvaluationInput {
    id: String,
    pub value: String,
}

impl ReplEvaluationInput {
    pub fn new(value: &str) -> Rc<Self> {
        Rc::new(Self { id: generate_uuid(), value: value.to_owned() })
    }

    pub fn to_display_string(&self) -> String {
        self.value.clone()
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }
}

/// `ReplEvaluationResult` (estende `ExpressionContainer`).
pub struct ReplEvaluationResult {
    pub base: Rc<ExpressionContainer>,
    available: Cell<bool>,
    pub original_expression: String,
}

impl ReplEvaluationResult {
    pub fn new(original_expression: &str) -> Rc<Self> {
        Rc::new(Self {
            base: ExpressionContainer::with_defaults(None, None, Some(0), generate_uuid()),
            available: Cell::new(true),
            original_expression: original_expression.to_owned(),
        })
    }

    pub fn available(&self) -> bool {
        self.available.get()
    }

    /// `evaluateExpression(expression, session, stackFrame, context)`.
    pub fn evaluate_expression(
        self: &Rc<Self>,
        expression: &str,
        session: Option<Rc<dyn IDebugSessionFull>>,
        stack_frame: Option<Rc<super::debug_model::StackFrame>>,
        context: &str,
    ) -> Promise<bool> {
        let this = self.clone();
        let result = self.base.evaluate_expression(expression, session, stack_frame, context, false, None);
        je_base::async_::async_fn(async move {
            let result = result.await?;
            this.available.set(result);
            Ok(result)
        })
    }

    pub fn value(&self) -> String {
        self.base.value()
    }

    pub fn get_id(&self) -> String {
        self.base.get_id()
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        self.base.value()
    }
}

/// `ReplGroup`.
pub struct ReplGroup {
    children: RefCell<Vec<ReplElement>>,
    id: String,
    ended: Cell<bool>,
    pub session: Rc<dyn IDebugSessionFull>,
    pub name: String,
    pub auto_expand: bool,
    pub source_data: Option<IReplElementSource>,
}

impl ReplGroup {
    pub fn new(session: Rc<dyn IDebugSessionFull>, name: &str, auto_expand: bool, source_data: Option<IReplElementSource>) -> Rc<Self> {
        let counter = REPL_GROUP_COUNTER.with(|counter| {
            let value = counter.get();
            counter.set(value + 1);
            value
        });
        Rc::new(Self {
            children: RefCell::new(Vec::new()),
            id: format!("replGroup:{counter}"),
            ended: Cell::new(false),
            session,
            name: name.to_owned(),
            auto_expand,
            source_data,
        })
    }

    pub fn has_children(&self) -> bool {
        true
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }

    pub fn to_display_string(&self, include_source: bool) -> String {
        let source_str = match (&self.source_data, include_source) {
            (Some(source_data), true) => format!(" {}", source_data.source.name()),
            _ => String::new(),
        };
        self.name.clone() + &source_str
    }

    fn last_open_group(&self) -> Option<Rc<ReplGroup>> {
        match self.children.borrow().last() {
            Some(ReplElement::Group(group)) if !group.has_ended() => Some(group.clone()),
            _ => None,
        }
    }

    pub fn add_child(&self, child: ReplElement) {
        match self.last_open_group() {
            Some(last_element) => last_element.add_child(child),
            None => self.children.borrow_mut().push(child),
        }
    }

    pub fn get_children(&self) -> Vec<ReplElement> {
        self.children.borrow().clone()
    }

    pub fn end(&self) {
        match self.last_open_group() {
            Some(last_element) => last_element.end(),
            None => self.ended.set(true),
        }
    }

    pub fn has_ended(&self) -> bool {
        self.ended.get()
    }
}

fn are_sources_equal(first: &Option<IReplElementSource>, second: &Option<IReplElementSource>) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(first), Some(second)) => {
            first.column == second.column && first.line_number == second.line_number && first.source.uri.to_string(false) == second.source.uri.to_string(false)
        }
        _ => false,
    }
}

/// `ReplModel`.
pub struct ReplModel {
    repl_elements: RefCell<Vec<ReplElement>>,
    on_did_change_elements: Rc<Emitter<Option<ReplElement>>>,
    configuration: DebugConsoleConfigurationProvider,
}

impl ReplModel {
    pub fn new(configuration: DebugConsoleConfigurationProvider) -> Rc<Self> {
        Rc::new(Self { repl_elements: RefCell::new(Vec::new()), on_did_change_elements: Emitter::new(), configuration })
    }

    pub fn on_did_change_elements(&self) -> Event<Option<ReplElement>> {
        self.on_did_change_elements.event()
    }

    /// Libera o emissor (o `ReplModel` do original não tem `dispose`; a sessão que o possui descarta o emissor no dela).
    pub fn dispose_emitter(&self) {
        use je_base::lifecycle::IDisposable;
        self.on_did_change_elements.dispose();
    }

    pub fn get_repl_elements(&self) -> Vec<ReplElement> {
        self.repl_elements.borrow().clone()
    }

    pub fn add_repl_expression(
        self: &Rc<Self>,
        session: Rc<dyn IDebugSessionFull>,
        stack_frame: Option<Rc<super::debug_model::StackFrame>>,
        expression: &str,
    ) -> Promise<()> {
        let this = self.clone();
        let expression = expression.to_owned();
        je_base::async_::async_fn(async move {
            this.add_repl_element(ReplElement::Input(ReplEvaluationInput::new(&expression)));
            let result = ReplEvaluationResult::new(&expression);
            result.evaluate_expression(&expression, Some(session), stack_frame, "repl").await?;
            this.add_repl_element(ReplElement::Result(result));
            Ok(())
        })
    }

    pub fn append_to_repl(&self, session: &Rc<dyn IDebugSessionFull>, data: NewReplElementData) {
        let NewReplElementData { mut output, expression, sev, source } = data;
        let clear_ansi_sequence = "\u{1b}[2J";
        if let Some(clear_ansi_index) = output.rfind(clear_ansi_sequence) {
            // [2J is the ansi escape sequence for clearing the display http://ascii-table.com/ansi-escape-sequences.php
            self.remove_repl_expressions();
            self.append_to_repl(
                session,
                NewReplElementData {
                    output: localize("consoleCleared", "Console was cleared", &[]),
                    expression: None,
                    sev: Severity::Ignore,
                    source: None,
                },
            );
            output = output[clear_ansi_index + clear_ansi_sequence.len()..].to_owned();
        }

        if let Some(expression) = expression {
            // if there is an output string, prefer to show that, since the DA could have formatted it nicely e.g. with ANSI color codes.
            self.add_repl_element(if !output.is_empty() {
                ReplElement::Output(ReplOutputElement::new(session.clone(), get_unique_id(), output, sev, source, Some(expression)))
            } else {
                ReplElement::Variable(ReplVariableElement::new(session.clone(), expression, sev, source))
            });
            return;
        }

        self.append_output_to_repl(session, &output, sev, source);
    }

    fn last_output(&self) -> Option<Rc<ReplOutputElement>> {
        match self.repl_elements.borrow().last() {
            Some(ReplElement::Output(element)) => Some(element.clone()),
            _ => None,
        }
    }

    fn append_output_to_repl(&self, session: &Rc<dyn IDebugSessionFull>, output: &str, sev: Severity, source: Option<IReplElementSource>) {
        let config = (self.configuration)();
        let previous_element = self.repl_elements.borrow().last().cloned();

        // Handle concatenation of incomplete lines first
        if let Some(ReplElement::Output(previous)) = &previous_element
            && previous.severity == sev
            && are_sources_equal(&previous.source_data, &source)
            && !previous.value.ends_with('\n')
            && !previous.value.ends_with("\r\n")
            && previous.count() == 1
        {
            // Concatenate with previous incomplete line
            let combined_output = format!("{}{}", previous.value, output);

            let last = self.repl_elements.borrow().len() - 1;
            self.repl_elements.borrow_mut()[last] =
                ReplElement::Output(ReplOutputElement::new(session.clone(), get_unique_id(), combined_output.clone(), sev, source.clone(), None));
            self.on_did_change_elements.fire(None);

            // If the combined output now forms a complete line and collapsing is enabled, check if it can be collapsed with previous elements
            if config.collapse_identical_lines && combined_output.ends_with('\n') {
                self.try_collapse_complete_line(sev, &source);
            }

            // If the combined output contains multiple lines, apply line-level collapsing
            if config.collapse_identical_lines && combined_output.contains('\n') {
                let lines = split_into_lines(&combined_output);
                if lines.len() > 1 {
                    self.apply_line_level_collapsing(session, sev, &source);
                }
            }
            return;
        }

        // If collapsing is enabled and the output contains line breaks, parse and collapse at line level
        if config.collapse_identical_lines && output.contains('\n') {
            self.process_multi_line_output(session, output, sev, &source);
        } else {
            // For simple output without line breaks, use the original logic
            if let Some(ReplElement::Output(previous)) = &previous_element
                && previous.severity == sev
                && are_sources_equal(&previous.source_data, &source)
                && previous.value == output
                && config.collapse_identical_lines
            {
                previous.set_count(previous.count() + 1);
                // No need to fire an event, just the count updates and badge will adjust automatically
                return;
            }

            let element = ReplOutputElement::new(session.clone(), get_unique_id(), output.to_owned(), sev, source, None);
            self.add_repl_element(ReplElement::Output(element));
        }
    }

    fn try_collapse_complete_line(&self, sev: Severity, source: &Option<IReplElementSource>) {
        // Try to collapse the last element with the second-to-last if they are identical complete lines
        let len = self.repl_elements.borrow().len();
        if len < 2 {
            return;
        }

        let (last_element, second_to_last_element) = {
            let elements = self.repl_elements.borrow();
            (elements[len - 1].clone(), elements[len - 2].clone())
        };

        if let (ReplElement::Output(last), ReplElement::Output(second)) = (&last_element, &second_to_last_element)
            && last.severity == sev
            && second.severity == sev
            && are_sources_equal(&last.source_data, source)
            && are_sources_equal(&second.source_data, source)
            && last.value == second.value
            && last.count() == 1
            && last.value.ends_with('\n')
        {
            // Collapse the last element into the second-to-last
            second.set_count(second.count() + last.count());
            self.repl_elements.borrow_mut().pop();
            self.on_did_change_elements.fire(None);
        }
    }

    fn process_multi_line_output(&self, session: &Rc<dyn IDebugSessionFull>, output: &str, sev: Severity, source: &Option<IReplElementSource>) {
        // Split output into lines, preserving line endings
        let lines = split_into_lines(output);

        for line in lines {
            if line.is_empty() {
                continue;
            }

            let previous_element = self.repl_elements.borrow().last().cloned();

            // Check if this line can be collapsed with the previous one
            if let Some(ReplElement::Output(previous)) = &previous_element
                && previous.severity == sev
                && are_sources_equal(&previous.source_data, source)
                && previous.value == line
            {
                previous.set_count(previous.count() + 1);
                // No need to fire an event, just the count updates and badge will adjust automatically
            } else {
                let element = ReplOutputElement::new(session.clone(), get_unique_id(), line, sev, source.clone(), None);
                self.add_repl_element(ReplElement::Output(element));
            }
        }
    }

    fn apply_line_level_collapsing(&self, session: &Rc<dyn IDebugSessionFull>, sev: Severity, source: &Option<IReplElementSource>) {
        // Apply line-level collapsing to the last element if it contains multiple lines
        let Some(last_element) = self.last_output() else {
            return;
        };
        if last_element.severity != sev || !are_sources_equal(&last_element.source_data, source) {
            return;
        }

        let lines = split_into_lines(&last_element.value);
        if lines.len() <= 1 {
            return; // No multiple lines to collapse
        }

        // Remove the last element and reprocess it as multiple lines
        self.repl_elements.borrow_mut().pop();

        // Process each line and try to collapse with existing elements
        for line in lines {
            if line.is_empty() {
                continue;
            }

            let previous_element = self.repl_elements.borrow().last().cloned();

            // Check if this line can be collapsed with the previous one
            if let Some(ReplElement::Output(previous)) = &previous_element
                && previous.severity == sev
                && are_sources_equal(&previous.source_data, source)
                && previous.value == line
            {
                previous.set_count(previous.count() + 1);
            } else {
                let element = ReplOutputElement::new(session.clone(), get_unique_id(), line, sev, source.clone(), None);
                self.add_repl_element(ReplElement::Output(element));
            }
        }

        self.on_did_change_elements.fire(None);
    }

    pub fn start_group(&self, session: &Rc<dyn IDebugSessionFull>, name: &str, auto_expand: bool, source_data: Option<IReplElementSource>) {
        let group = ReplGroup::new(session.clone(), name, auto_expand, source_data);
        self.add_repl_element(ReplElement::Group(group));
    }

    pub fn end_group(&self) {
        let last_element = self.repl_elements.borrow().last().cloned();
        if let Some(ReplElement::Group(group)) = last_element {
            group.end();
        }
    }

    fn add_repl_element(&self, new_element: ReplElement) {
        let last_element = self.repl_elements.borrow().last().cloned();
        match &last_element {
            Some(ReplElement::Group(group)) if !group.has_ended() => group.add_child(new_element.clone()),
            _ => {
                self.repl_elements.borrow_mut().push(new_element.clone());
                let config = (self.configuration)();
                let len = self.repl_elements.borrow().len();
                if len > config.maximum_lines {
                    self.repl_elements.borrow_mut().drain(0..len - config.maximum_lines);
                }
            }
        }

        self.on_did_change_elements.fire(Some(new_element));
    }

    pub fn remove_repl_expressions(&self) {
        if !self.repl_elements.borrow().is_empty() {
            self.repl_elements.borrow_mut().clear();
            self.on_did_change_elements.fire(None);
        }
    }

    /// Returns a new REPL model that's a copy of this one.
    pub fn clone_model(&self) -> Rc<ReplModel> {
        let new_repl = ReplModel::new(self.configuration.clone());
        *new_repl.repl_elements.borrow_mut() = self.repl_elements.borrow().clone();
        new_repl
    }
}

/// `splitIntoLines(text)`: divide o texto preservando os fins de linha, com `indexOf`.
fn split_into_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < text.len() {
        match text[start..].find('\n') {
            None => {
                lines.push(text[start..].to_owned());
                break;
            }
            Some(offset) => {
                let next_lf = start + offset;
                lines.push(text[start..=next_lf].to_owned());
                start = next_lf + 1;
            }
        }
    }
    lines
}
