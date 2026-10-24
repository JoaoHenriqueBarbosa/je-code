//! Porte de `src/vs/workbench/contrib/debug/browser/debugExpressionRenderer.ts`: o `DebugExpressionRenderer`, que escreve o
//! valor de uma expressão ou variável num elemento (as classes de cor, o texto truncado, os links, o ANSI e o hover) e o
//! nome, o tipo e o valor de uma variável nos elementos do template.
//!
//! O elemento é o [`Element`] lógico de `link_detector.rs` e o rótulo é o [`IHighlightedLabel`] de `base_debug_view.rs`
//! (ver o cabeçalho dele). O que o original recebe por injeção vira o [`DebugExpressionRendererServices`]:
//!
//! - o `ICommandService` e o `IConfigurationService` são os serviços do porte;
//! - o `IHoverService` é o [`IExpressionHoverService`];
//! - o `LinkDetector` (criado pelo `IInstantiationService`) chega pronto;
//! - `Source.openInEditor` não está portado, então o destino de um `locationReference` abre pelo `source_opener` que o
//!   hospedeiro dá (o [`IDebugSourceOpener`] de cada `Source`).
//!
//! Divergências:
//! - `value === null` (o erro do servidor, uma variável de nome nulo) não existe: o valor é sempre um texto, e o erro chega
//!   como `available == false`.
//! - O `displayType` (`observableConfigValue('debug.showVariableTypes', false, ...)`) é lido do serviço de configuração a
//!   cada renderização, que é o que `observable.get()` faz sem observadores.
//! - O argumento do comando `Variable` vira um objeto dinâmico com a variável como hospedeira (`ObjectRef::with_host`).

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::promise::Promise;
use je_platform::commands::commands::{CommandDescription, ICommandService, commands_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::js::to_number;
use je_platform::workspace::WorkspaceFolder;
use je_widgets::highlighted_label::IHighlight;
use serde_json::Value;

use super::base_debug_view::{HoverActionSpec, IExpressionHoverService, IVariableTemplateData, ManagedHoverContent, split_expression_or_scope_highlights};
use super::debug::IDebugSession;
use super::debug_ansi_handling::handle_ansi_output;
use super::debug_commands::{COPY_EVALUATE_PATH_ID, COPY_VALUE_ID};
use super::debug_model::{Expression, Variable};
use super::debug_source::Source;
use super::debug_types::{IDebugSessionFull, IExpressionContainer};
use super::link_detector::{
    DebugLinkHoverBehavior, DebugLinkHoverBehaviorTypeData, DebugLocation, Element, IDebugSessionLocations, IDebugSourceOpener, ILinkDetector, LinkDetector,
    LinkNode, LinkifyOptions,
};
use super::repl_model::ReplEvaluationResult;

/// `IValueHoverOptions.commands` e `{ id, args }`.
#[derive(Clone)]
pub struct HoverCommand {
    pub id: String,
    pub args: Vec<Dynamic>,
}

/// `hover?: false | IValueHoverOptions`.
#[derive(Clone)]
pub enum ValueHover {
    /// `hover: false`: sem hover gerenciável (os links usam o hover rico).
    Off,
    /// `IValueHoverOptions`.
    On { commands: Vec<HoverCommand> },
}

/// `IRenderValueOptions`.
#[derive(Clone, Default)]
pub struct IRenderValueOptions {
    pub show_changed: bool,
    pub max_value_length: Option<usize>,
    /// `None` é o `undefined` do original: hover ligado, sem comandos.
    pub hover: Option<ValueHover>,
    pub colorize: bool,
    pub highlights: Option<Vec<IHighlight>>,
    /// Indicates areas where VS Code implicitly always supported ANSI escape sequences. These should be rendered as ANSI
    /// when the DA does not specify any value of `supportsANSIStyling`.
    pub was_ansi: Option<bool>,
    pub session: Option<Rc<dyn IDebugSessionFull>>,
    pub location_reference: Option<i64>,
}

/// `IRenderVariableOptions`.
#[derive(Clone, Default)]
pub struct IRenderVariableOptions {
    pub show_changed: bool,
    pub highlights: Vec<IHighlight>,
}

/// O `expressionOrValue` de `renderValue`: um texto, ou um contêiner de expressão (`Expression`, `Variable`, ...), ou o
/// resultado de uma avaliação do REPL.
#[derive(Clone, Copy)]
pub enum ExpressionOrValue<'a> {
    Text(&'a str),
    Container(&'a dyn IExpressionContainer),
    ReplResult(&'a ReplEvaluationResult),
}

/// `MAX_VALUE_RENDER_LENGTH_IN_VIEWLET`.
pub const MAX_VALUE_RENDER_LENGTH_IN_VIEWLET: usize = 1024;

/// O `Cls`: as classes que `renderValue` troca (`allClasses`).
const ALL_CLASSES: [&str; 7] = ["value", "unavailable", "error", "changed", "boolean", "string", "number"];

/// `booleanRegex`: `/^(true|false)$/i`.
fn is_boolean_text(value: &str) -> bool {
    value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
}

/// `stringRegex`: `/^(['"]).*\1$/` (o `.` não casa com terminadores de linha).
fn is_string_text(value: &str) -> bool {
    let mut chars = value.chars();
    let (Some(first), Some(last)) = (chars.next(), value.chars().next_back()) else {
        return false;
    };
    if (first != '\'' && first != '"') || value.chars().count() < 2 || first != last {
        return false;
    }
    let middle: Vec<char> = value.chars().collect();
    middle[1..middle.len() - 1].iter().all(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
}

fn is_truthy_value(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// O que `renderValue` lê de `expressionOrValue`.
struct ValueFacts {
    value: String,
    expr_type: Option<String>,
    value_changed: bool,
    /// O `available` das classes que o têm (`Expression`, `Variable`, `ReplEvaluationResult`).
    availability: Option<bool>,
    is_text: bool,
    session: Option<Rc<dyn IDebugSessionFull>>,
    value_location_reference: Option<i64>,
}

impl ValueFacts {
    fn of(expression_or_value: ExpressionOrValue<'_>) -> Self {
        match expression_or_value {
            ExpressionOrValue::Text(text) => Self {
                value: text.to_owned(),
                expr_type: None,
                value_changed: false,
                availability: None,
                is_text: true,
                session: None,
                value_location_reference: None,
            },
            ExpressionOrValue::Container(container) => Self {
                value: container.value(),
                expr_type: container.expr_type(),
                value_changed: container.value_changed(),
                availability: container.availability(),
                is_text: false,
                session: container.get_session(),
                value_location_reference: container.value_location_reference(),
            },
            ExpressionOrValue::ReplResult(result) => Self {
                value: result.value(),
                expr_type: result.base.expr_type.borrow().clone(),
                value_changed: result.base.value_changed.get(),
                availability: Some(result.available()),
                is_text: false,
                session: result.base.session(),
                value_location_reference: result.base.value_location_reference.get(),
            },
        }
    }

    fn clear_value_changed(&self, expression_or_value: ExpressionOrValue<'_>) {
        match expression_or_value {
            ExpressionOrValue::Text(_) => {}
            ExpressionOrValue::Container(container) => container.clear_value_changed(),
            ExpressionOrValue::ReplResult(result) => result.base.value_changed.set(false),
        }
    }
}

/// `Source` para o `IDebugSourceOpener` que o link de `locationReference` abre.
pub type SourceOpenerFactory = Rc<dyn Fn(Rc<Source>) -> Rc<dyn IDebugSourceOpener>>;

/// O recorte de `IDebugSession` que o detector lê, sobre uma [`IDebugSessionFull`].
struct SessionLocations {
    session: Rc<dyn IDebugSessionFull>,
    source_opener: SourceOpenerFactory,
}

impl IDebugSessionLocations for SessionLocations {
    fn resolve_location_reference(&self, location_reference: f64) -> Promise<DebugLocation> {
        let session = self.session.clone();
        let source_opener = self.source_opener.clone();
        async_fn(async move {
            let location = session.resolve_location_reference(location_reference as i64).await?;
            Ok(DebugLocation {
                source: source_opener(location.source),
                line: location.line as f64,
                column: location.column as f64,
                end_line: location.end_line.map(|line| line as f64),
                end_column: location.end_column.map(|column| column as f64),
            })
        })
    }
}

/// Os serviços injetados no `DebugExpressionRenderer`.
#[derive(Clone)]
pub struct DebugExpressionRendererServices {
    pub command_service: Rc<dyn ICommandService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub hover_service: Rc<dyn IExpressionHoverService>,
    pub link_detector: Rc<LinkDetector>,
    pub source_opener: SourceOpenerFactory,
}

/// `DebugExpressionRenderer`.
pub struct DebugExpressionRenderer {
    services: DebugExpressionRendererServices,
}

impl DebugExpressionRenderer {
    pub fn new(services: DebugExpressionRendererServices) -> Rc<Self> {
        Rc::new(Self { services })
    }

    /// `this.displayType.get()`.
    fn display_type(&self) -> bool {
        self.services.configuration_service.get_value(Some("debug.showVariableTypes"), None).is_truthy()
    }

    /// `renderVariable(data, variable, options)`.
    pub fn render_variable(&self, data: &IVariableTemplateData, variable: &Rc<Variable>, options: &IRenderVariableOptions) -> DisposableRef {
        let display_type = self.display_type();
        let name = variable.name.borrow().clone();
        let highlights = split_expression_or_scope_highlights(name.as_deref().unwrap_or_default(), &options.highlights);
        let value = variable.base.value();
        let variable_type = variable.base.expr_type.borrow().clone().filter(|ty| !ty.is_empty());
        let presentation_hint = variable.base.presentation_hint.borrow().clone();

        if variable.available {
            data.r#type.set_text_content("");
            let mut text = name.clone().unwrap_or_default();
            if !value.is_empty() && name.is_some() {
                match variable_type.as_ref().filter(|_| display_type) {
                    Some(variable_type) => {
                        text.push_str(": ");
                        data.r#type.set_text_content(&format!("{variable_type} ="));
                    }
                    None => text.push_str(" ="),
                }
            }

            let title = match variable_type.as_ref().filter(|_| !display_type) {
                Some(variable_type) => variable_type.clone(),
                None => name.clone().unwrap_or_default(),
            };
            data.label.set(&text, &highlights.name, Some(&title));
            let hint = |key: &str| presentation_hint.as_ref().and_then(|hint| hint.get(key)).and_then(Value::as_str).map(str::to_owned);
            data.name.toggle_class("virtual", hint("kind").as_deref() == Some("virtual"));
            data.name.toggle_class("internal", hint("visibility").as_deref() == Some("internal"));
        } else if !value.is_empty() && name.as_ref().is_some_and(|name| !name.is_empty()) {
            data.label.set(":", &[], None);
        }

        data.expression.toggle_class("lazy", is_truthy_value(presentation_hint.as_ref().and_then(|hint| hint.get("lazy"))));
        let variable_object = || Dynamic::Object(ObjectRef::with_host(variable.clone()));
        let mut commands = vec![HoverCommand { id: COPY_VALUE_ID.to_owned(), args: vec![variable_object(), Dynamic::array([variable_object()])] }];
        if variable.evaluate_name().is_some_and(|evaluate_name| !evaluate_name.is_empty()) {
            commands.push(HoverCommand { id: COPY_EVALUATE_PATH_ID.to_owned(), args: vec![Dynamic::object([("variable", variable_object())])] });
        }

        let container: &dyn IExpressionContainer = &**variable;
        self.render_value(
            &data.value,
            ExpressionOrValue::Container(container),
            &IRenderValueOptions {
                show_changed: options.show_changed,
                max_value_length: Some(MAX_VALUE_RENDER_LENGTH_IN_VIEWLET),
                hover: Some(ValueHover::On { commands }),
                highlights: Some(highlights.value),
                colorize: true,
                session: variable.base.session(),
                ..IRenderValueOptions::default()
            },
        )
    }

    /// `renderValue(container, expressionOrValue, options)`.
    pub fn render_value(&self, container: &Rc<Element>, expression_or_value: ExpressionOrValue<'_>, options: &IRenderValueOptions) -> DisposableRef {
        let store = DisposableStore::new();
        let facts = ValueFacts::of(expression_or_value);
        // Use remembered capabilities so REPL elements can render even once a session ends
        let remembered_ansi = options
            .session
            .as_ref()
            .and_then(|session| session.remembered_capabilities())
            .and_then(|capabilities| capabilities.get("supportsANSIStyling").cloned())
            .filter(|value| !value.is_null())
            .map(|value| is_truthy_value(Some(&value)));
        let supports_ansi: bool = remembered_ansi.or(options.was_ansi).unwrap_or(false);

        let mut value = facts.value.clone();

        // remove stale classes
        for class in ALL_CLASSES {
            container.remove_class(class);
        }
        container.add_classes(["value"]);
        let default_value = Expression::default_value();
        // when resolving expressions we represent errors from the server as a variable with name === null.
        if facts.availability == Some(false) {
            container.add_classes(["unavailable"]);
            if value != default_value {
                container.add_classes(["error"]);
            }
        } else {
            if !facts.is_text && options.show_changed && facts.value_changed && value != default_value {
                // value changed color has priority over other colors.
                container.add_classes(["changed"]);
                facts.clear_value_changed(expression_or_value);
            }

            if options.colorize && !facts.is_text {
                match facts.expr_type.as_deref() {
                    Some(expr_type @ ("number" | "boolean" | "string")) => container.add_classes([expr_type]),
                    _ => {
                        if !to_number(&Dynamic::String(JsString::from(value.as_str()))).is_nan() {
                            container.add_classes(["number"]);
                        } else if is_boolean_text(&value) {
                            container.add_classes(["boolean"]);
                        } else if is_string_text(&value) {
                            container.add_classes(["string"]);
                        }
                    }
                }
            }
        }

        if let Some(max_value_length) = options.max_value_length.filter(|max| *max > 0) {
            let length = value.encode_utf16().count();
            if !value.is_empty() && length > max_value_length {
                let units: Vec<u16> = value.encode_utf16().take(max_value_length).collect();
                value = format!("{}...", String::from_utf16_lossy(&units));
            }
        }

        let session = options.session.clone().or(facts.session.clone());
        // Only use hovers for links if thre's not going to be a hover for the value.
        let hover_behavior = DebugLinkHoverBehaviorTypeData {
            kind: if matches!(options.hover, Some(ValueHover::Off)) { DebugLinkHoverBehavior::Rich } else { DebugLinkHoverBehavior::None },
            store: store.clone(),
        };
        container.clear();
        let location_reference = options.location_reference.or(facts.value_location_reference).filter(|reference| *reference != 0);

        let link_detector: Rc<dyn ILinkDetector> = match (&location_reference, &session) {
            (Some(location_reference), Some(session)) => self.services.link_detector.make_referenced_link_detector(
                *location_reference as f64,
                Rc::new(SessionLocations { session: session.clone(), source_opener: self.services.source_opener.clone() }),
            ),
            _ => self.services.link_detector.clone(),
        };

        let root: Option<WorkspaceFolder> = session.as_ref().and_then(|session| session.root());
        let highlights = options.highlights.clone();
        if supports_ansi {
            container.append([LinkNode::Element(handle_ansi_output(&value, &*link_detector, root.as_ref(), highlights.as_deref(), &hover_behavior))]);
        } else {
            container.append([LinkNode::Element(link_detector.linkify(
                &value,
                &hover_behavior,
                &LinkifyOptions { split_lines: false, workspace_folder: root.clone(), include_fulltext: true, highlights: highlights.clone().unwrap_or_default() },
            ))]);
        }

        if !matches!(options.hover, Some(ValueHover::Off)) {
            let commands = match &options.hover {
                Some(ValueHover::On { commands }) => commands.clone(),
                _ => Vec::new(),
            };
            let link_detector = self.services.link_detector.clone();
            let hover_value = value.clone();
            let hover_store = hover_behavior.clone();
            let content = ManagedHoverContent::Factory(Rc::new(move || {
                let container = Element::create("div");
                let markdown_hover_element = Element::create("div.hover-row");
                let hover_contents_element = Element::create("div.hover-contents");
                let hover_contents_pre = Element::create("pre.debug-var-hover-pre");
                hover_contents_element.append([LinkNode::Element(hover_contents_pre.clone())]);
                markdown_hover_element.append([LinkNode::Element(hover_contents_element)]);
                if supports_ansi {
                    // note: intentionally using `this.linkDetector` so we don't blindly linkify the
                    // entire contents and instead only link file paths that it contains.
                    hover_contents_pre.append([LinkNode::Element(handle_ansi_output(&hover_value, &*link_detector, root.as_ref(), highlights.as_deref(), &hover_store))]);
                } else {
                    hover_contents_pre.set_text_content(&hover_value);
                }
                container.append([LinkNode::Element(markdown_hover_element)]);
                container
            }));
            let actions: Vec<HoverActionSpec> = commands
                .into_iter()
                .map(|HoverCommand { id, args }| {
                    let label = match commands_registry().get_command(&id).and_then(|command| command.metadata).map(|metadata| metadata.description) {
                        Some(CommandDescription::Plain(description)) => description,
                        Some(CommandDescription::Localized(description)) => description.value,
                        None => id.clone(),
                    };
                    let command_service = self.services.command_service.clone();
                    let command_id = id.clone();
                    HoverActionSpec {
                        label,
                        command_id: id,
                        run: Rc::new(move || {
                            let _ = command_service.execute_command(&command_id, args.clone());
                        }),
                    }
                })
                .collect();
            store.add(self.services.hover_service.setup_managed_hover(container, content, actions));
        }

        store
    }
}
