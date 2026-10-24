//! Porte parcial de `src/vs/workbench/contrib/debug/browser/watchExpressionsView.ts`: o `WatchExpressionsRenderer` e o
//! `ignoreViewUpdates` que a renomeação de uma expressão liga enquanto atualiza as visões.
//!
//! O contrato do DOM e dos widgets é o do cabeçalho de `base_debug_view.rs`.
//!
//! Divergências:
//! - A `WatchExpressionsView` (a `ViewPane`, a árvore, o arrastar e soltar, os menus e o `WatchExpressionsDataSource`) e o
//!   `WatchExpressionsAccessibilityProvider` pintam widgets e ficam com a visão nativa. `renderActionBar` (menu
//!   `DebugWatchContext`) depende da `ActionBar` e do `IMenuService`, que o porte ainda não tem.
//! - `typeof expression.value === 'string'` é sempre verdadeiro: o valor é um texto (o `null` do servidor é uma expressão
//!   indisponível, ver `debug_expression_renderer.rs`).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Truthy;
use je_base::nls::localize;
use je_platform::configuration::service::IConfigurationService;
use je_widgets::highlighted_label::IHighlight;

use super::base_debug_view::{
    AbstractExpressionsRenderer, IExpressionHoverService, IExpressionTemplateData, IExpressionTreeNode, IExpressionsDebugService, IInputBoxFactory,
    IInputBoxOptions,
};
use super::debug_expression_renderer::{DebugExpressionRenderer, ExpressionOrValue, IRenderValueOptions, MAX_VALUE_RENDER_LENGTH_IN_VIEWLET};
use super::debug_types::{IExpression, IExpressionContainer};

thread_local! {
    /// `let ignoreViewUpdates = false`: a visão não se recarrega enquanto a renomeação atualiza as visões.
    static IGNORE_VIEW_UPDATES: Cell<bool> = const { Cell::new(false) };
}

/// Lê o `ignoreViewUpdates`.
pub fn ignore_view_updates() -> bool {
    IGNORE_VIEW_UPDATES.with(Cell::get)
}

fn set_ignore_view_updates(value: bool) {
    IGNORE_VIEW_UPDATES.with(|cell| cell.set(value));
}

/// `WatchExpressionsRenderer`.
pub struct WatchExpressionsRenderer {
    expression_renderer: Rc<DebugExpressionRenderer>,
    debug_service: Rc<dyn IExpressionsDebugService>,
    input_box_factory: Rc<dyn IInputBoxFactory>,
    hover_service: Rc<dyn IExpressionHoverService>,
    configuration_service: Rc<dyn IConfigurationService>,
    me: Weak<WatchExpressionsRenderer>,
}

impl WatchExpressionsRenderer {
    /// `WatchExpressionsRenderer.ID`.
    pub const ID: &'static str = "watchexpression";

    pub fn new(
        expression_renderer: Rc<DebugExpressionRenderer>,
        debug_service: Rc<dyn IExpressionsDebugService>,
        input_box_factory: Rc<dyn IInputBoxFactory>,
        hover_service: Rc<dyn IExpressionHoverService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self { expression_renderer, debug_service, input_box_factory, hover_service, configuration_service, me: me.clone() })
    }

    /// `getValue<IDebugConfiguration>('debug').showVariableTypes`.
    fn show_variable_types(&self) -> bool {
        self.configuration_service.get_value(Some("debug"), None).get_str("showVariableTypes").is_truthy()
    }
}

impl AbstractExpressionsRenderer for WatchExpressionsRenderer {
    fn debug_service(&self) -> Rc<dyn IExpressionsDebugService> {
        self.debug_service.clone()
    }

    fn input_box_factory(&self) -> Rc<dyn IInputBoxFactory> {
        self.input_box_factory.clone()
    }

    fn hover_service(&self) -> Rc<dyn IExpressionHoverService> {
        self.hover_service.clone()
    }

    fn template_id(&self) -> &'static str {
        Self::ID
    }

    fn render_element(&self, node: &IExpressionTreeNode, _index: usize, data: &Rc<IExpressionTemplateData>) {
        data.element_disposable.clear();
        let me = self.me.clone();
        let weak_data = Rc::downgrade(data);
        let node_for_changes = node.clone();
        data.element_disposable.add(self.configuration_service.on_did_change_configuration().listen(move |event| {
            if event.affects_configuration("debug.showVariableTypes", None)
                && let (Some(renderer), Some(data)) = (me.upgrade(), weak_data.upgrade())
            {
                renderer.render_expression_element(&node_for_changes.element, &node_for_changes, &data);
            }
        }));
        self.render_expression_element(&node.element, node, data);
    }

    fn render_expression(&self, expression: &Rc<dyn IExpression>, data: &Rc<IExpressionTemplateData>, highlights: &[IHighlight]) {
        data.r#type.set_text_content("");
        let show_type = self.show_variable_types();
        let name = expression.name();
        let expression_type = expression.expr_type().filter(|expression_type| !expression_type.is_empty());
        let text = match expression_type.as_ref().filter(|_| show_type) {
            Some(expression_type) => {
                //render type
                data.r#type.set_text_content(&format!("{expression_type} ="));
                format!("{name}: ")
            }
            None => format!("{name} ="),
        };

        let title = match &expression_type {
            Some(_) if show_type => name.clone(),
            Some(expression_type) => expression_type.clone(),
            None => expression.value(),
        };

        data.label.set(&text, highlights, Some(&title));
        let container: &dyn IExpressionContainer = &**expression;
        data.element_disposable.add(self.expression_renderer.render_value(
            &data.value,
            ExpressionOrValue::Container(container),
            &IRenderValueOptions {
                show_changed: true,
                max_value_length: Some(MAX_VALUE_RENDER_LENGTH_IN_VIEWLET),
                colorize: true,
                session: expression.get_session(),
                ..IRenderValueOptions::default()
            },
        ));
    }

    fn get_input_box_options(&self, expression: &Rc<dyn IExpression>, setting_value: bool) -> Option<IInputBoxOptions> {
        let debug_service = self.debug_service.clone();
        if setting_value {
            let expression = expression.clone();
            return Some(IInputBoxOptions {
                initial_value: expression.value(),
                aria_label: localize("typeNewValue", "Type new value", &[]),
                placeholder: None,
                validation_options: None,
                on_finish: Rc::new(move |value: &str, success: bool| {
                    if success && !value.is_empty() {
                        let focused_frame = debug_service.get_view_model().focused_stack_frame();
                        if let Some(focused_frame) = focused_frame {
                            let value = value.to_owned();
                            let pending = match (expression.clone().into_variable(), expression.clone().into_expression()) {
                                (Some(variable), _) => Some(variable.set_expression(&value, focused_frame)),
                                (None, Some(watch_expression)) => Some(watch_expression.set_expression(&value, focused_frame)),
                                (None, None) => None,
                            };
                            if let Some(pending) = pending {
                                let debug_service = debug_service.clone();
                                let _ = async_fn(async move {
                                    pending.await?;
                                    debug_service.get_view_model().update_views();
                                    Ok(())
                                });
                            }
                        }
                    }
                }),
            });
        }

        let expression = expression.clone();
        Some(IInputBoxOptions {
            initial_value: expression.name(),
            aria_label: localize("watchExpressionInputAriaLabel", "Type watch expression", &[]),
            placeholder: Some(localize("watchExpressionPlaceholder", "Expression to watch", &[])),
            validation_options: None,
            on_finish: Rc::new(move |value: &str, success: bool| {
                if success && !value.is_empty() {
                    debug_service.rename_watch_expression(&expression.get_id(), value);
                    set_ignore_view_updates(true);
                    debug_service.get_view_model().update_views();
                    set_ignore_view_updates(false);
                } else if expression.name().is_empty() {
                    debug_service.remove_watch_expressions(Some(&expression.get_id()));
                }
            }),
        })
    }
}
