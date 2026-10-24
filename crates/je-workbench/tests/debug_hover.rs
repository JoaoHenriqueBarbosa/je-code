//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugHover.test.ts` (suíte `Debug - Hover`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` do caso.
//! - As subclasses anônimas do original (`Thread` com `getCallStack`, `StackFrame` com `getScopes`, `Scope` e `Variable` com
//!   `getChildren`) são o dublê [`Node`] (um `IExpression` e `IScope` com filhos fixos) e o [`Frame`]
//!   (`IStackFrameScopes` com um escopo fixo), porque o `findExpressionInStackFrame` do porte lê só o contrato
//!   (`getScopes`, `expensive`, `getChildren`, `name`, `value`). O nome, o valor e o `expensive` seguem os do original
//!   (`Variable` sem valor tem valor vazio).
//! - `assert.strictEqual(x, variableA)` é a comparação de identidade do `Rc`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::promise::Promise;
use je_workbench::contrib::debug::debug_hover::{IScope, IStackFrameScopes, find_expression_in_stack_frame};
use je_workbench::contrib::debug::debug_types::{IDebugSessionFull, IExpression, IExpressionContainer};
use serde_json::Value;

/// Uma variável ou escopo do dublê.
struct Node {
    name: String,
    expensive: Cell<bool>,
    children: RefCell<Vec<Rc<dyn IExpression>>>,
}

impl Node {
    fn new(name: &str) -> Rc<Self> {
        Rc::new(Self { name: name.to_owned(), expensive: Cell::new(false), children: RefCell::new(Vec::new()) })
    }
}

impl IExpressionContainer for Node {
    fn get_id(&self) -> String {
        self.name.clone()
    }

    fn value(&self) -> String {
        String::new()
    }

    fn expr_type(&self) -> Option<String> {
        Some("string".to_owned())
    }

    fn value_changed(&self) -> bool {
        false
    }

    fn has_children(&self) -> bool {
        !self.children.borrow().is_empty()
    }

    fn get_session(&self) -> Option<Rc<dyn IDebugSessionFull>> {
        None
    }

    fn evaluate_lazy(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_children(&self) -> Promise<Vec<Rc<dyn IExpression>>> {
        Promise::resolved(self.children.borrow().clone())
    }

    fn reference(&self) -> Option<i64> {
        Some(2)
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
}

impl IExpression for Node {
    fn name(&self) -> String {
        self.name.clone()
    }
}

impl IScope for Node {
    fn expensive(&self) -> bool {
        self.expensive.get()
    }
}

/// O `StackFrame` com `getScopes` sobrescrito.
struct Frame {
    scope: Rc<Node>,
}

impl IStackFrameScopes for Frame {
    fn get_scopes(&self) -> Promise<Vec<Rc<dyn IScope>>> {
        Promise::resolved(vec![self.scope.clone() as Rc<dyn IScope>])
    }
}

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn same(found: &Option<Rc<dyn IExpression>>, expected: &Rc<Node>) -> bool {
    found.as_ref().is_some_and(|found| std::ptr::addr_eq(Rc::as_ptr(found), Rc::as_ptr(expected)))
}

#[test]
fn find_expression_in_stack_frame_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scope = Node::new("local");
        let variable_a = Node::new("A");
        let variable_b = Node::new("B");
        scope.children.borrow_mut().push(variable_a.clone());
        variable_a.children.borrow_mut().push(variable_b.clone());
        let stack_frame = Frame { scope: scope.clone() };

        assert!(find_expression_in_stack_frame(&stack_frame, &names(&[])).await.unwrap().is_none());
        assert!(same(&find_expression_in_stack_frame(&stack_frame, &names(&["A"])).await.unwrap(), &variable_a));
        assert!(find_expression_in_stack_frame(&stack_frame, &names(&["doesNotExist", "no"])).await.unwrap().is_none());
        assert!(find_expression_in_stack_frame(&stack_frame, &names(&["a"])).await.unwrap().is_none());
        assert!(find_expression_in_stack_frame(&stack_frame, &names(&["B"])).await.unwrap().is_none());
        assert!(same(&find_expression_in_stack_frame(&stack_frame, &names(&["A", "B"])).await.unwrap(), &variable_b));
        assert!(find_expression_in_stack_frame(&stack_frame, &names(&["A", "C"])).await.unwrap().is_none());

        // We do not search in expensive scopes
        scope.expensive.set(true);
        assert!(find_expression_in_stack_frame(&stack_frame, &names(&["A"])).await.unwrap().is_none());
    });
}
