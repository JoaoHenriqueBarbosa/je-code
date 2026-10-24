//! Porte de `src/vs/workbench/contrib/debug/browser/baseDebugView.ts`: o esqueleto dos renderizadores de expressões (o
//! `AbstractExpressionsRenderer`), o `renderViewTree`, o corte dos realces entre nome e valor, o rótulo de navegação por
//! teclado e a fonte de dados que troca os filhos pelos visualizados.
//!
//! Como o DOM e os widgets viram o contrato lógico:
//!
//! - O `HTMLElement` é o [`Element`] de `link_detector.rs` (marca, atributos, classes, texto, filhos, estilo, hover gravado e
//!   ouvintes de `click`), o mesmo que a saída de `linkify` e do `handleANSIOutput` já usa.
//! - O `HighlightedLabel` é o [`IHighlightedLabel`] com a implementação lógica [`LogicalHighlightedLabel`], que monta o
//!   `span.monaco-highlighted-label` com o mesmo algoritmo de `HighlightedLabel.render`. O widget nativo
//!   (`je_widgets::highlighted_label`) é preso a nós do `je-ui` e desenha, não escreve DOM.
//! - O `InputBox` é o [`IInputBox`] criado por um [`IInputBoxFactory`], com a [`LogicalInputBox`] para quem não tem widget.
//!   A lógica de `renderInputBox` (esconder o nome e o valor, o `done` de chamada única, Enter, Escape e blur) é a do original.
//! - O `IHoverService` é o [`IExpressionHoverService`] (com o [`NullHoverService`] do `NullHoverService` dos testes) e o
//!   `IDebugService` é o [`IExpressionsDebugService`], que só tem o que os renderizadores chamam. O [`ViewModel`] do porte
//!   implementa o [`IExpressionsViewModel`] por `Rc<ViewModel>`.
//! - O `ITreeNode<IExpression, FuzzyScore>` é o [`IExpressionTreeNode`] (elemento e `filterData`); os demais campos do nó
//!   (profundidade, filhos visíveis) nenhum renderizador lê.
//! - O `AbstractExpressionsRenderer` é a trait [`AbstractExpressionsRenderer`] com os métodos concretos do original como
//!   métodos com corpo; `VariablesRenderer` e `WatchExpressionsRenderer` a implementam.
//!
//! Divergências:
//! - A `ActionBar` (`renderActionBar` e o `.span.actionbar-spacer` do template) não é portada: ela depende do
//!   `IMenuService`, do `MenuId.Debug*Context` e do `IDebugVisualizerService.getApplicableFor`, que o porte ainda não tem.
//!   O restante do template e da renderização é o do original.
//! - O `IRenderValueOptions` de `baseDebugView.ts` (uma cópia antiga, sem uso) não existe; o vigente está em
//!   `debug_expression_renderer.rs`.
//! - O `IDebugVisualizerService.getVisualizedNodeFor` não está portado; a fonte de dados o recebe como [`IVisualizedNodeSource`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::filters::create_matches;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::strings::remove_ansi_escape_codes;
use je_base::themables::ThemeIcon;
use je_widgets::highlighted_label::IHighlight;

use super::debug_ansi_handling::set_style_property;
use super::debug_model::StackFrame;
use super::debug_types::{IExpression, IExpressionContainer};
use super::debug_view_model::{SelectedExpression, ViewModel};
use super::link_detector::{Element, LinkNode, utf16_substring};

/// `renderViewTree(container)`.
pub fn render_view_tree(container: &Rc<Element>) -> Rc<Element> {
    let tree_container = Element::create(".");
    tree_container.add_classes(["debug-view-content", "file-icon-themable-tree"]);
    container.append([LinkNode::Element(tree_container.clone())]);
    tree_container
}

// ---- rótulo com realce

/// O recorte de `HighlightedLabel` que os renderizadores usam.
pub trait IHighlightedLabel {
    /// `set(text, highlights, title)`.
    fn set(&self, text: &str, highlights: &[IHighlight], title: Option<&str>);

    /// `element`: o `span.monaco-highlighted-label`.
    fn element(&self) -> Rc<Element>;
}

struct LabelState {
    text: String,
    title: String,
    highlights: Vec<IHighlight>,
    did_ever_render: bool,
    custom_hover: bool,
}

/// `HighlightedLabel` sobre o DOM lógico.
pub struct LogicalHighlightedLabel {
    dom_node: Rc<Element>,
    state: RefCell<LabelState>,
}

impl LogicalHighlightedLabel {
    /// `new HighlightedLabel(container)`: cria o `span.monaco-highlighted-label` como último filho do contêiner.
    pub fn new(container: &Rc<Element>) -> Rc<Self> {
        let dom_node = Element::create("span.monaco-highlighted-label");
        container.append([LinkNode::Element(dom_node.clone())]);
        Rc::new(Self {
            dom_node,
            state: RefCell::new(LabelState { text: String::new(), title: String::new(), highlights: Vec::new(), did_ever_render: false, custom_hover: false }),
        })
    }

    fn render(&self) {
        let (text, title, highlights) = {
            let state = self.state.borrow();
            (state.text.clone(), state.title.clone(), state.highlights.clone())
        };
        let text_length = text.encode_utf16().count();
        let mut children: Vec<LinkNode> = Vec::new();
        let mut pos = 0usize;

        for highlight in &highlights {
            if highlight.end == highlight.start {
                continue;
            }

            if pos < highlight.start {
                children.push(LinkNode::Text(utf16_substring(&text, pos, Some(highlight.start))));
                pos = highlight.start;
            }

            let element = Element::create("span.highlight");
            element.add_classes(highlight.extra_classes.iter().map(String::as_str));
            element.append([LinkNode::Text(utf16_substring(&text, pos, Some(highlight.end)))]);
            children.push(LinkNode::Element(element));
            pos = highlight.end;
        }

        if pos < text_length {
            children.push(LinkNode::Text(utf16_substring(&text, pos, None)));
        }

        self.dom_node.reset(children);

        let mut state = self.state.borrow_mut();
        if !state.custom_hover && !title.is_empty() {
            state.custom_hover = true;
            self.dom_node.set_managed_hover(Some(title));
        } else if state.custom_hover {
            self.dom_node.set_managed_hover(Some(title));
        }
        state.did_ever_render = true;
    }
}

impl IHighlightedLabel for LogicalHighlightedLabel {
    fn set(&self, text: &str, highlights: &[IHighlight], title: Option<&str>) {
        let title = title.unwrap_or_default();
        {
            let mut state = self.state.borrow_mut();
            if state.did_ever_render && state.text == text && state.title == title && state.highlights == highlights {
                return;
            }
            state.text = text.to_owned();
            state.title = title.to_owned();
            state.highlights = highlights.to_vec();
        }
        self.render();
    }

    fn element(&self) -> Rc<Element> {
        self.dom_node.clone()
    }
}

// ---- dados do template

/// `IVariableTemplateData`.
#[derive(Clone)]
pub struct IVariableTemplateData {
    pub expression: Rc<Element>,
    pub name: Rc<Element>,
    pub r#type: Rc<Element>,
    pub value: Rc<Element>,
    pub label: Rc<dyn IHighlightedLabel>,
    pub lazy_button: Rc<Element>,
}

/// `IExpressionTemplateData`.
pub struct IExpressionTemplateData {
    pub expression: Rc<Element>,
    pub name: Rc<Element>,
    pub r#type: Rc<Element>,
    pub value: Rc<Element>,
    pub input_box_container: Rc<Element>,
    pub element_disposable: Rc<DisposableStore>,
    pub template_disposable: Rc<DisposableStore>,
    pub label: Rc<dyn IHighlightedLabel>,
    pub lazy_button: Rc<Element>,
    pub current_element: RefCell<Option<Rc<dyn IExpression>>>,
}

impl IExpressionTemplateData {
    /// O `data` que o original passa a `renderVariable` (um `IExpressionTemplateData` é um `IVariableTemplateData`).
    pub fn variable_template(&self) -> IVariableTemplateData {
        IVariableTemplateData {
            expression: self.expression.clone(),
            name: self.name.clone(),
            r#type: self.r#type.clone(),
            value: self.value.clone(),
            label: self.label.clone(),
            lazy_button: self.lazy_button.clone(),
        }
    }
}

/// `{ content }` de `IMessage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IMessage {
    pub content: String,
}

/// `IInputValidationOptions.validation`.
pub type InputValidation = Rc<dyn Fn(&str) -> Option<IMessage>>;

/// `IInputValidationOptions`.
#[derive(Clone, Default)]
pub struct IInputValidationOptions {
    pub validation: Option<InputValidation>,
}

/// `IInputBoxOptions`.
#[derive(Clone)]
pub struct IInputBoxOptions {
    pub initial_value: String,
    pub aria_label: String,
    pub placeholder: Option<String>,
    pub validation_options: Option<IInputValidationOptions>,
    pub on_finish: Rc<dyn Fn(&str, bool)>,
}

/// `ITreeNode<IExpression, FuzzyScore>`, com o que os renderizadores leem.
#[derive(Clone)]
pub struct IExpressionTreeNode {
    pub element: Rc<dyn IExpression>,
    /// `filterData`: o `FuzzyScore` do filtro (`None` sem filtro).
    pub filter_data: Option<Vec<i32>>,
}

// ---- serviços

/// O recorte de `IViewModel` que os renderizadores e a fonte de dados usam.
pub trait IExpressionsViewModel {
    fn focused_stack_frame(&self) -> Option<Rc<StackFrame>>;
    fn get_selected_expression(&self) -> Option<SelectedExpression>;
    fn set_selected_expression(&self, expression: Option<Rc<dyn IExpression>>, setting_watch: bool);
    fn update_views(&self);
    /// `evaluateLazyExpression(expression)`.
    fn evaluate_lazy_expression(&self, expression: Rc<dyn IExpression>) -> Promise<()>;
    /// `getVisualizedExpression(expression)`: a expressão visualizada ou o id da árvore preferida.
    fn get_visualized_expression(&self, expression: &Rc<dyn IExpression>) -> Option<Result<Rc<dyn IExpression>, String>>;
    /// `setVisualizedExpression(original, visualized)`.
    fn set_visualized_expression(&self, original: Rc<dyn IExpression>, visualized: Option<(Rc<dyn IExpression>, String)>);
}

impl IExpressionsViewModel for Rc<ViewModel> {
    fn focused_stack_frame(&self) -> Option<Rc<StackFrame>> {
        ViewModel::focused_stack_frame(self)
    }

    fn get_selected_expression(&self) -> Option<SelectedExpression> {
        ViewModel::get_selected_expression(self)
    }

    fn set_selected_expression(&self, expression: Option<Rc<dyn IExpression>>, setting_watch: bool) {
        ViewModel::set_selected_expression(self, expression, setting_watch);
    }

    fn update_views(&self) {
        ViewModel::update_views(self);
    }

    fn evaluate_lazy_expression(&self, expression: Rc<dyn IExpression>) -> Promise<()> {
        let container: Rc<dyn IExpressionContainer> = expression;
        ViewModel::evaluate_lazy_expression(self, container)
    }

    fn get_visualized_expression(&self, expression: &Rc<dyn IExpression>) -> Option<Result<Rc<dyn IExpression>, String>> {
        ViewModel::get_visualized_expression(self, expression)
    }

    fn set_visualized_expression(&self, original: Rc<dyn IExpression>, visualized: Option<(Rc<dyn IExpression>, String)>) {
        ViewModel::set_visualized_expression(self, original, visualized);
    }
}

/// O recorte de `IDebugService` que os renderizadores e a fonte de dados usam.
pub trait IExpressionsDebugService {
    fn get_view_model(&self) -> Rc<dyn IExpressionsViewModel>;
    /// `renameWatchExpression(id, newName)`.
    fn rename_watch_expression(&self, id: &str, new_name: &str);
    /// `removeWatchExpressions(id?)`.
    fn remove_watch_expressions(&self, id: Option<&str>);
}

/// O conteúdo que o `setupManagedHover` recebe: um texto ou uma fábrica do elemento do hover.
#[derive(Clone)]
pub enum ManagedHoverContent {
    Text(String),
    Factory(Rc<dyn Fn() -> Rc<Element>>),
}

/// Uma ação do rodapé do hover (`IManagedHoverOptions.actions`).
#[derive(Clone)]
pub struct HoverActionSpec {
    pub label: String,
    pub command_id: String,
    pub run: Rc<dyn Fn()>,
}

/// O recorte de `IHoverService` que os renderizadores usam: `setupManagedHover`.
pub trait IExpressionHoverService {
    fn setup_managed_hover(&self, target: &Rc<Element>, content: ManagedHoverContent, actions: Vec<HoverActionSpec>) -> DisposableRef;
}

/// O `NullHoverService` dos testes: nenhum hover aparece e o descartável não faz nada.
pub struct NullHoverService;

impl IExpressionHoverService for NullHoverService {
    fn setup_managed_hover(&self, _target: &Rc<Element>, _content: ManagedHoverContent, _actions: Vec<HoverActionSpec>) -> DisposableRef {
        to_disposable(|| {})
    }
}

/// O `InputBox` que `renderInputBox` cria: o valor, o foco e os ouvintes do `inputElement`.
pub trait IInputBox: IDisposable {
    fn value(&self) -> String;
    fn set_value(&self, value: &str);
    fn focus(&self);
    fn select(&self);
    /// `addStandardDisposableListener(inputElement, KEY_DOWN, ...)`.
    fn on_key_down(&self, handler: Rc<dyn Fn(KeyCode)>) -> DisposableRef;
    /// `addDisposableListener(inputElement, BLUR, ...)`.
    fn on_blur(&self, handler: Rc<dyn Fn()>) -> DisposableRef;
}

/// `new InputBox(container, contextViewService, { ...options, inputBoxStyles })`.
pub trait IInputBoxFactory {
    fn create(&self, container: &Rc<Element>, options: &IInputBoxOptions) -> Rc<dyn IInputBox>;
}

/// Um `InputBox` lógico: um `input` no contêiner, com o valor, o foco e os eventos dirigidos pelo chamador
/// ([`LogicalInputBox::press_key`], [`LogicalInputBox::blur`]).
pub struct LogicalInputBox {
    input_element: Rc<Element>,
    value: RefCell<String>,
    focused: Cell<bool>,
    selected: Cell<bool>,
    disposed: Cell<bool>,
    key_down_handlers: RefCell<Vec<(u64, Rc<dyn Fn(KeyCode)>)>>,
    blur_handlers: RefCell<Vec<(u64, Rc<dyn Fn()>)>>,
    next_id: Cell<u64>,
    me: Weak<LogicalInputBox>,
}

impl LogicalInputBox {
    pub fn new(container: &Rc<Element>) -> Rc<Self> {
        let input_element = Element::create("input.input");
        container.append([LinkNode::Element(input_element.clone())]);
        Rc::new_cyclic(|me| Self {
            input_element,
            value: RefCell::new(String::new()),
            focused: Cell::new(false),
            selected: Cell::new(false),
            disposed: Cell::new(false),
            key_down_handlers: RefCell::new(Vec::new()),
            blur_handlers: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
            me: me.clone(),
        })
    }

    pub fn input_element(&self) -> Rc<Element> {
        self.input_element.clone()
    }

    pub fn is_focused(&self) -> bool {
        self.focused.get()
    }

    pub fn is_selected(&self) -> bool {
        self.selected.get()
    }

    /// Uma tecla pressionada no campo.
    pub fn press_key(&self, key_code: KeyCode) {
        let handlers: Vec<Rc<dyn Fn(KeyCode)>> = self.key_down_handlers.borrow().iter().map(|(_, handler)| handler.clone()).collect();
        for handler in handlers {
            handler(key_code);
        }
    }

    /// O campo perde o foco.
    pub fn blur(&self) {
        self.focused.set(false);
        let handlers: Vec<Rc<dyn Fn()>> = self.blur_handlers.borrow().iter().map(|(_, handler)| handler.clone()).collect();
        for handler in handlers {
            handler();
        }
    }

    fn next_listener_id(&self) -> u64 {
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }
}

impl IDisposable for LogicalInputBox {
    fn dispose(&self) {
        self.disposed.set(true);
        self.key_down_handlers.borrow_mut().clear();
        self.blur_handlers.borrow_mut().clear();
    }
}

impl IInputBox for LogicalInputBox {
    fn value(&self) -> String {
        self.value.borrow().clone()
    }

    fn set_value(&self, value: &str) {
        *self.value.borrow_mut() = value.to_owned();
    }

    fn focus(&self) {
        self.focused.set(true);
    }

    fn select(&self) {
        self.selected.set(true);
    }

    fn on_key_down(&self, handler: Rc<dyn Fn(KeyCode)>) -> DisposableRef {
        let id = self.next_listener_id();
        self.key_down_handlers.borrow_mut().push((id, handler));
        let me = self.me.clone();
        to_disposable(move || {
            if let Some(me) = me.upgrade() {
                me.key_down_handlers.borrow_mut().retain(|(handler_id, _)| *handler_id != id);
            }
        })
    }

    fn on_blur(&self, handler: Rc<dyn Fn()>) -> DisposableRef {
        let id = self.next_listener_id();
        self.blur_handlers.borrow_mut().push((id, handler));
        let me = self.me.clone();
        to_disposable(move || {
            if let Some(me) = me.upgrade() {
                me.blur_handlers.borrow_mut().retain(|(handler_id, _)| *handler_id != id);
            }
        })
    }
}

/// A fábrica que cria [`LogicalInputBox`].
pub struct LogicalInputBoxFactory;

impl IInputBoxFactory for LogicalInputBoxFactory {
    fn create(&self, container: &Rc<Element>, _options: &IInputBoxOptions) -> Rc<dyn IInputBox> {
        LogicalInputBox::new(container)
    }
}

// ---- realces

/// O resultado de `splitExpressionOrScopeHighlights`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SplitHighlights {
    pub name: Vec<IHighlight>,
    pub value: Vec<IHighlight>,
}

/// `splitExpressionOrScopeHighlights(e, highlights)`: divide os realces conforme o [`expression_and_scope_keyboard_label`]
/// (`name: value`). O `e` entra só pelo nome, em unidades UTF-16.
pub fn split_expression_or_scope_highlights(name: &str, highlights: &[IHighlight]) -> SplitHighlights {
    let name_ends_at = name.encode_utf16().count();
    let label_begins_at = name_ends_at + 2;
    let mut split = SplitHighlights::default();
    for hl in highlights {
        if hl.start < name_ends_at {
            split.name.push(IHighlight::new(hl.start, hl.end.min(name_ends_at)));
        }
        if hl.end > label_begins_at {
            split.value.push(IHighlight::new(hl.start.saturating_sub(label_begins_at), hl.end - label_begins_at));
        }
    }
    split
}

/// `expressionAndScopeLabelProvider.getKeyboardNavigationLabel(e)`.
pub fn expression_and_scope_keyboard_label(name: &str, container: &dyn IExpressionContainer) -> String {
    let strip_ansi = container
        .get_session()
        .and_then(|session| session.remembered_capabilities())
        .and_then(|capabilities| capabilities.get("supportsANSIStyling").cloned())
        .is_some_and(|value| value.as_bool().unwrap_or(false));
    let value = container.value();
    let value = if strip_ansi {
        let units: Vec<u16> = value.encode_utf16().collect();
        let stripped: JsString = remove_ansi_escape_codes(&units);
        String::from_utf16_lossy(stripped.units())
    } else {
        value
    };
    format!("{name}: {value}")
}

/// `IDebugVisualizerService.getVisualizedNodeFor(treeId, expression)`.
pub trait IVisualizedNodeSource {
    fn get_visualized_node_for(&self, tree_id: &str, expression: &Rc<dyn IExpression>) -> Promise<Option<Rc<dyn IExpression>>>;
}

/// `AbstractExpressionDataSource.getChildren`: troca cada filho pela expressão visualizada (a do `ViewModel` ou a que a
/// árvore preferida resolve). `children` é o que `doGetChildren(element)` devolveu.
pub fn expression_data_source_children(
    debug_service: &Rc<dyn IExpressionsDebugService>,
    visualizer: &Rc<dyn IVisualizedNodeSource>,
    children: Vec<Rc<dyn IExpression>>,
) -> Promise<Vec<Rc<dyn IExpression>>> {
    let debug_service = debug_service.clone();
    let visualizer = visualizer.clone();
    je_base::async_::async_fn(async move {
        let vm = debug_service.get_view_model();
        let mut result: Vec<Rc<dyn IExpression>> = Vec::with_capacity(children.len());
        for child in children {
            match vm.get_visualized_expression(&child) {
                Some(Err(tree_id)) => match visualizer.get_visualized_node_for(&tree_id, &child).await? {
                    Some(visualized) => {
                        vm.set_visualized_expression(child.clone(), Some((visualized.clone(), tree_id)));
                        result.push(visualized);
                    }
                    None => result.push(child),
                },
                Some(Ok(visualized)) => result.push(visualized),
                None => result.push(child),
            }
        }
        Ok(result)
    })
}

fn same_expression(a: &Rc<dyn IExpression>, b: &Rc<dyn IExpression>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

// ---- renderizador

/// `AbstractExpressionsRenderer`.
pub trait AbstractExpressionsRenderer {
    fn debug_service(&self) -> Rc<dyn IExpressionsDebugService>;
    fn input_box_factory(&self) -> Rc<dyn IInputBoxFactory>;
    fn hover_service(&self) -> Rc<dyn IExpressionHoverService>;

    /// `templateId`.
    fn template_id(&self) -> &'static str;

    /// `renderElement(node, index, data)`.
    fn render_element(&self, node: &IExpressionTreeNode, index: usize, data: &Rc<IExpressionTemplateData>);

    /// `renderExpression(expression, data, highlights)`.
    fn render_expression(&self, expression: &Rc<dyn IExpression>, data: &Rc<IExpressionTemplateData>, highlights: &[IHighlight]);

    /// `getInputBoxOptions(expression, settingValue)`.
    fn get_input_box_options(&self, expression: &Rc<dyn IExpression>, setting_value: bool) -> Option<IInputBoxOptions>;

    /// `renderTemplate(container)`.
    fn render_template(&self, container: &Rc<Element>) -> Rc<IExpressionTemplateData> {
        let template_disposable = DisposableStore::new();
        let expression = Element::create(".expression");
        container.append([LinkNode::Element(expression.clone())]);
        let name = Element::create("span.name");
        expression.append([LinkNode::Element(name.clone())]);
        let lazy_button = Element::create("span.lazy-button");
        expression.append([LinkNode::Element(lazy_button.clone())]);
        let eye_classes = ThemeIcon::from_id("eye").as_class_name_array();
        lazy_button.add_classes(eye_classes.iter().map(String::as_str));

        template_disposable.add(self.hover_service().setup_managed_hover(
            &lazy_button,
            ManagedHoverContent::Text(localize("debug.lazyButton.tooltip", "Click to expand", &[])),
            Vec::new(),
        ));
        let r#type = Element::create("span.type");
        expression.append([LinkNode::Element(r#type.clone())]);

        let value = Element::create("span.value");
        expression.append([LinkNode::Element(value.clone())]);

        let label: Rc<dyn IHighlightedLabel> = LogicalHighlightedLabel::new(&name);

        let input_box_container = Element::create(".inputBoxContainer");
        expression.append([LinkNode::Element(input_box_container.clone())]);

        let debug_service = self.debug_service();
        let template = Rc::new_cyclic(|template: &Weak<IExpressionTemplateData>| {
            let template = template.clone();
            template_disposable.add(lazy_button.add_click_listener(Rc::new(move || {
                let Some(template) = template.upgrade() else {
                    return;
                };
                let current = template.current_element.borrow().clone();
                if let Some(current) = current {
                    let _ = debug_service.get_view_model().evaluate_lazy_expression(current);
                }
            })));
            IExpressionTemplateData {
                expression: expression.clone(),
                name: name.clone(),
                r#type: r#type.clone(),
                value: value.clone(),
                input_box_container: input_box_container.clone(),
                element_disposable: DisposableStore::new(),
                template_disposable: template_disposable.clone(),
                label: label.clone(),
                lazy_button: lazy_button.clone(),
                current_element: RefCell::new(None),
            }
        });
        template
    }

    /// `renderExpressionElement(element, node, data)`.
    fn render_expression_element(&self, element: &Rc<dyn IExpression>, node: &IExpressionTreeNode, data: &Rc<IExpressionTemplateData>) {
        *data.current_element.borrow_mut() = Some(element.clone());
        let highlights: Vec<IHighlight> = create_matches(node.filter_data.as_deref()).into_iter().map(|m| IHighlight::new(m.start, m.end)).collect();
        self.render_expression(&node.element, data, &highlights);
        let selected_expression = self.debug_service().get_view_model().get_selected_expression();
        let is_selected = selected_expression.as_ref().is_some_and(|selected| same_expression(element, &selected.expression));
        let has_error = element.as_variable().is_some_and(|variable| variable.error_message.borrow().is_some());
        if is_selected || has_error {
            let setting_watch = selected_expression.as_ref().is_some_and(|selected| selected.setting_watch);
            if let Some(options) = self.get_input_box_options(element, setting_watch) {
                data.element_disposable.add(self.render_input_box(&data.name, &data.value, &data.input_box_container, &options));
            }
        }
    }

    /// `renderInputBox(nameElement, valueElement, inputBoxContainer, options)`.
    fn render_input_box(&self, name_element: &Rc<Element>, value_element: &Rc<Element>, input_box_container: &Rc<Element>, options: &IInputBoxOptions) -> DisposableRef {
        set_style_property(name_element, "display", "none");
        set_style_property(value_element, "display", "none");
        set_style_property(input_box_container, "display", "initial");
        input_box_container.clear();

        let input_box = self.input_box_factory().create(input_box_container, options);

        input_box.set_value(&options.initial_value);
        input_box.focus();
        input_box.select();

        let to_dispose: Rc<RefCell<Vec<DisposableRef>>> = Rc::new(RefCell::new(Vec::new()));
        let called = Rc::new(Cell::new(false));
        let done: Rc<dyn Fn(bool, bool)> = {
            let name_element = name_element.clone();
            let value_element = value_element.clone();
            let input_box_container = input_box_container.clone();
            let input_box = input_box.clone();
            let to_dispose = to_dispose.clone();
            let debug_service = self.debug_service();
            let on_finish = options.on_finish.clone();
            // `createSingleCallFunction`
            Rc::new(move |success: bool, finish_editing: bool| {
                if called.replace(true) {
                    return;
                }
                set_style_property(&name_element, "display", "");
                set_style_property(&value_element, "display", "");
                set_style_property(&input_box_container, "display", "none");
                let value = input_box.value();
                let disposables: Vec<DisposableRef> = to_dispose.borrow_mut().drain(..).collect();
                for disposable in disposables {
                    disposable.dispose();
                }
                input_box.dispose();

                if finish_editing {
                    debug_service.get_view_model().set_selected_expression(None, false);
                    on_finish(&value, success);
                }
            })
        };

        {
            let done = done.clone();
            to_dispose.borrow_mut().push(input_box.on_key_down(Rc::new(move |key_code: KeyCode| {
                let is_escape = key_code == KeyCode::Escape;
                let is_enter = key_code == KeyCode::Enter;
                if is_escape || is_enter {
                    done(is_enter, true);
                }
            })));
        }
        {
            let done = done.clone();
            to_dispose.borrow_mut().push(input_box.on_blur(Rc::new(move || done(true, true))));
        }

        to_disposable(move || done(false, false))
    }

    /// `disposeElement(node, index, templateData)`.
    fn dispose_element(&self, _node: &IExpressionTreeNode, _index: usize, template_data: &IExpressionTemplateData) {
        template_data.element_disposable.clear();
    }

    /// `disposeTemplate(templateData)`.
    fn dispose_template(&self, template_data: &IExpressionTemplateData) {
        template_data.element_disposable.dispose();
        template_data.template_disposable.dispose();
    }
}
