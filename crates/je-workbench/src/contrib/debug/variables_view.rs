//! Porte parcial de `src/vs/workbench/contrib/debug/browser/variablesView.ts`: o `VariablesRenderer` e o `forgetScopes`
//! que a edição de uma variável liga para a próxima atualização da árvore.
//!
//! O contrato do DOM e dos widgets é o do cabeçalho de `base_debug_view.rs` (o `HTMLElement` é o `Element` lógico, o
//! `HighlightedLabel` é o `IHighlightedLabel`, o `IDebugService` é o `IExpressionsDebugService`).
//!
//! Divergências:
//! - A `VariablesView` (a `ViewPane` com o `WorkbenchAsyncDataTree`, os menus e o `VariablesDataSource`) e o
//!   `VariablesAccessibilityProvider` pintam widgets e ficam com a visão nativa; as ações `SetValueAction`,
//!   `CopyValueAction` e `CopyEvaluatePathAction` ficam com os comandos do depurador (`debug_commands.rs`).
//! - `renderActionBar` (menu `DebugVariablesContext` e os visualizadores) e `useVisualizer`/`pickVisualizer` dependem da
//!   `ActionBar`, do `IMenuService` e do `IDebugVisualizerService.getApplicableFor`, que o porte ainda não tem (ver
//!   `base_debug_view.rs`).
//! - `renderExpression` faz `expression as Variable`: aqui só uma `Variable` é renderizada (uma expressão visualizada, que o
//!   TypeScript trata por tipagem estrutural, não existe no porte).

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::nls::localize;
use je_widgets::highlighted_label::IHighlight;

use super::base_debug_view::{
    AbstractExpressionsRenderer, IExpressionHoverService, IExpressionTemplateData, IExpressionTreeNode, IExpressionsDebugService, IInputBoxFactory,
    IInputBoxOptions, IInputValidationOptions, IMessage,
};
use super::debug_expression_renderer::{DebugExpressionRenderer, IRenderVariableOptions};
use super::debug_types::{IExpression, IExpressionContainer};

thread_local! {
    /// `let forgetScopes = true`: quando é falso, a próxima mudança de foco não descarta os escopos do quadro.
    static FORGET_SCOPES: Cell<bool> = const { Cell::new(true) };
}

/// Lê o `forgetScopes`.
pub fn forget_scopes() -> bool {
    FORGET_SCOPES.with(Cell::get)
}

/// Grava o `forgetScopes`.
pub fn set_forget_scopes(value: bool) {
    FORGET_SCOPES.with(|cell| cell.set(value));
}

/// `VariablesRenderer`.
pub struct VariablesRenderer {
    expression_renderer: Rc<DebugExpressionRenderer>,
    debug_service: Rc<dyn IExpressionsDebugService>,
    input_box_factory: Rc<dyn IInputBoxFactory>,
    hover_service: Rc<dyn IExpressionHoverService>,
}

impl VariablesRenderer {
    /// `VariablesRenderer.ID`.
    pub const ID: &'static str = "variable";

    pub fn new(
        expression_renderer: Rc<DebugExpressionRenderer>,
        debug_service: Rc<dyn IExpressionsDebugService>,
        input_box_factory: Rc<dyn IInputBoxFactory>,
        hover_service: Rc<dyn IExpressionHoverService>,
    ) -> Rc<Self> {
        Rc::new(Self { expression_renderer, debug_service, input_box_factory, hover_service })
    }
}

impl AbstractExpressionsRenderer for VariablesRenderer {
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

    fn render_expression(&self, expression: &Rc<dyn IExpression>, data: &Rc<IExpressionTemplateData>, highlights: &[IHighlight]) {
        let Some(variable) = expression.clone().into_variable() else {
            return;
        };
        data.element_disposable.add(self.expression_renderer.render_variable(
            &data.variable_template(),
            &variable,
            &IRenderVariableOptions { highlights: highlights.to_vec(), show_changed: true },
        ));
    }

    fn render_element(&self, node: &IExpressionTreeNode, _index: usize, data: &Rc<IExpressionTemplateData>) {
        data.element_disposable.clear();
        self.render_expression_element(&node.element, node, data);
    }

    fn get_input_box_options(&self, expression: &Rc<dyn IExpression>, _setting_value: bool) -> Option<IInputBoxOptions> {
        let variable = expression.clone().into_variable()?;
        let validation_variable = variable.clone();
        let debug_service = self.debug_service.clone();
        Some(IInputBoxOptions {
            initial_value: IExpressionContainer::value(&**expression),
            aria_label: localize("variableValueAriaLabel", "Type new variable value", &[]),
            placeholder: None,
            validation_options: Some(IInputValidationOptions {
                validation: Some(Rc::new(move |_value: &str| validation_variable.error_message.borrow().clone().map(|content| IMessage { content }))),
            }),
            on_finish: Rc::new(move |value: &str, success: bool| {
                *variable.error_message.borrow_mut() = None;
                let view_model = debug_service.get_view_model();
                let focused_stack_frame = view_model.focused_stack_frame();
                if let Some(focused_stack_frame) = focused_stack_frame.filter(|_| success && variable.base.value() != value) {
                    let pending = variable.set_variable(value, focused_stack_frame);
                    let debug_service = debug_service.clone();
                    // Need to force watch expressions and variables to update since a variable change can have an effect on both
                    let _ = async_fn(async move {
                        pending.await?;
                        // Do not refresh scopes due to a node limitation #15520
                        set_forget_scopes(false);
                        debug_service.get_view_model().update_views();
                        Ok(())
                    });
                }
            }),
        })
    }
}
