//! Porte da parte lógica de `src/vs/workbench/contrib/debug/browser/debugHover.ts`: `findExpressionInStackFrame` (e o
//! `doFindExpression` que ele usa), que acha no quadro de pilha a expressão de um nome qualificado (`A.B`).
//!
//! Notas de tradução:
//! - `IStackFrame.getScopes()` e `IScope` (`IExpressionContainer` com `expensive`) são os traits [`IStackFrameScopes`] e
//!   [`IScope`]; o `StackFrame` e o `Scope` do modelo os implementam, e os testes do original, que sobrescrevem os
//!   métodos em subclasses, implementam os traits em dublês.
//! - `IExpression.name` vem do trait `IExpression`. Os resultados comparam-se por identidade (`Rc`).
//! - O `DebugHoverWidget` (UI) NÃO foi portado.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::promise::Promise;

use super::debug_model::{Scope, StackFrame};
use super::debug_types::{IExpression, IExpressionContainer};

/// `IScope`: um contêiner de expressões do quadro de pilha, `expensive` ou não.
pub trait IScope: IExpressionContainer {
    fn expensive(&self) -> bool;
}

/// `IStackFrame.getScopes()`.
pub trait IStackFrameScopes {
    fn get_scopes(&self) -> Promise<Vec<Rc<dyn IScope>>>;
}

impl IScope for Scope {
    fn expensive(&self) -> bool {
        self.expensive
    }
}

impl IStackFrameScopes for Rc<StackFrame> {
    fn get_scopes(&self) -> Promise<Vec<Rc<dyn IScope>>> {
        StackFrame::get_scopes(self).then(|scopes| scopes.into_iter().map(|scope| scope as Rc<dyn IScope>).collect())
    }
}

fn do_find_expression(container: Rc<dyn IExpressionContainer>, names_to_find: Vec<String>) -> Promise<Option<Rc<dyn IExpression>>> {
    async_fn(async move {
        let children = container.get_children().await?;
        // look for our variable in the list. First find the parents of the hovered variable if there are any.
        let filtered: Vec<Rc<dyn IExpression>> = children.into_iter().filter(|v| names_to_find.first() == Some(&v.name())).collect();
        if filtered.len() != 1 {
            return Ok(None);
        }

        if names_to_find.len() == 1 {
            Ok(Some(filtered[0].clone()))
        } else {
            let next: Rc<dyn IExpressionContainer> = filtered[0].clone();
            do_find_expression(next, names_to_find[1..].to_vec()).await
        }
    })
}

/// `findExpressionInStackFrame(stackFrame, namesToFind)`.
pub fn find_expression_in_stack_frame(stack_frame: &dyn IStackFrameScopes, names_to_find: &[String]) -> Promise<Option<Rc<dyn IExpression>>> {
    let scopes = stack_frame.get_scopes();
    let names_to_find = names_to_find.to_vec();
    async_fn(async move {
        let scopes = scopes.await?;
        let non_expensive: Vec<Rc<dyn IScope>> = scopes.into_iter().filter(|s| !s.expensive()).collect();
        let mut pending = Vec::new();
        for scope in non_expensive {
            let container: Rc<dyn IExpressionContainer> = scope;
            pending.push(do_find_expression(container, names_to_find.clone()));
        }
        let expressions: Vec<Rc<dyn IExpression>> = Promise::all(pending).await?.into_iter().flatten().collect();

        // only show if all expressions found have the same value
        Ok(if !expressions.is_empty() && expressions.iter().all(|e| e.value() == expressions[0].value()) { Some(expressions[0].clone()) } else { None })
    })
}
