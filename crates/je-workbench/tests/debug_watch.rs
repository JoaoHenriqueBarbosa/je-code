//! Porte de `src/vs/workbench/contrib/debug/test/browser/watch.test.ts` (suíte `Debug - Watch`), caso a caso e na mesma ordem.
//!
//! O único caso, `watch expressions`, está portado.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` do caso; o `setup` (`createMockDebugModel`) roda no
//!   começo dele.
//! - `addWatchExpression('console')` é `add_watch_expression(Some("console"))`; `we.reference` (sempre `0` numa expressão
//!   de observação sem sessão) é o `reference()` do contêiner base, que o porte tipa como `Option<i64>`.
//! - `moveWatchExpression(id, 1)` recebe a posição como `usize`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::debug::debug_model::Expression;
use std::rc::Rc;
use support::mock_debug::create_mock_debug_model;

// Expressions

fn assert_watch_expressions(watch_expressions: &[Rc<Expression>], expected_name: &str) {
    assert_eq!(watch_expressions.len(), 2);
    for we in watch_expressions {
        assert!(!we.available.get());
        assert_eq!(we.base.reference(), Some(0));
        assert_eq!(*we.name.borrow(), expected_name);
    }
}

#[test]
fn watch_expressions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());

    assert_eq!(model.get_watch_expressions().len(), 0);
    model.add_watch_expression(Some("console"));
    model.add_watch_expression(Some("console"));
    let mut watch_expressions = model.get_watch_expressions();
    assert_watch_expressions(&watch_expressions, "console");

    model.rename_watch_expression(&watch_expressions[0].base.get_id(), "new_name");
    model.rename_watch_expression(&watch_expressions[1].base.get_id(), "new_name");
    assert_watch_expressions(&model.get_watch_expressions(), "new_name");

    assert_watch_expressions(&model.get_watch_expressions(), "new_name");

    model.add_watch_expression(Some("mockExpression"));
    let third_id = model.get_watch_expressions()[2].base.get_id();
    model.move_watch_expression(&third_id, 1);
    watch_expressions = model.get_watch_expressions();
    assert_eq!(*watch_expressions[0].name.borrow(), "new_name");
    assert_eq!(*watch_expressions[1].name.borrow(), "mockExpression");
    assert_eq!(*watch_expressions[2].name.borrow(), "new_name");

    model.remove_watch_expressions(None);
    assert_eq!(model.get_watch_expressions().len(), 0);
}
