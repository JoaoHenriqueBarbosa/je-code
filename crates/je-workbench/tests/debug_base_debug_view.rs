//! Porte de `src/vs/workbench/contrib/debug/test/browser/baseDebugView.test.ts` (suíte `Debug - Base Debug View`), caso a caso
//! e na mesma ordem.
//!
//! NÃO foram portados (sem `#[test]`), 4 dos 5 casos, todos pelo mesmo motivo: eles exercitam o `DebugExpressionRenderer`
//! e o `renderViewTree` (`browser/debugExpressionRenderer.ts`, `browser/baseDebugView.ts`), que escrevem em elementos do DOM
//! (`className`, `textContent`, `querySelector('a')`) e usam o `HighlightedLabel` e o `LinkDetector` sobre esse DOM, com o
//! `IHoverService` e a configuração `debug.showVariableTypes`. O porte não tem o `DebugExpressionRenderer` (a UI é nativa) e
//! portá-lo só para um DOM de mentira não provaria a pintura real: `render view tree`, `render expression value`,
//! `render variable` e `render variable with display type setting`.
//!
//! O caso portado: `statusbar in debug mode`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` do caso.
//! - `session.configuration.noDebug = true` (muda o objeto da configuração no lugar) é `set_configuration` com a mesma
//!   configuração mais `noDebug`.
//! - `createTestSession(model, undefined, { suppressDebugStatusbar: true })` usa o nome padrão `mockSession`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::debug::debug::IDebugSession;
use je_workbench::contrib::debug::debug_session::DebugSessionOptions;
use je_workbench::contrib::debug::debug_types::{IDebugSessionFull, State};
use je_workbench::contrib::debug::statusbar_color_provider::is_statusbar_in_debug_mode;
use support::mock_debug::{create_mock_debug_model, create_test_session};

#[test]
fn statusbar_in_debug_mode() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = guard.add_rc(create_test_session(&model, "mockSession", None, &guard.store()));
    let session2 = guard.add_rc(create_test_session(
        &model,
        "mockSession",
        Some(DebugSessionOptions { suppress_debug_statusbar: Some(true), ..DebugSessionOptions::default() }),
        &guard.store(),
    ));
    let first: Rc<dyn IDebugSessionFull> = session.clone();
    let second: Rc<dyn IDebugSessionFull> = session2.clone();
    assert!(!is_statusbar_in_debug_mode(State::Inactive, &[]));
    assert!(!is_statusbar_in_debug_mode(State::Initializing, &[first.clone()]));
    assert!(is_statusbar_in_debug_mode(State::Running, &[first.clone()]));
    assert!(is_statusbar_in_debug_mode(State::Stopped, &[first.clone()]));

    assert!(!is_statusbar_in_debug_mode(State::Running, &[second.clone()]));
    assert!(is_statusbar_in_debug_mode(State::Running, &[first.clone(), second.clone()]));

    let mut configuration = session.configuration();
    configuration["noDebug"] = serde_json::json!(true);
    session.set_configuration(configuration, None);
    assert!(!is_statusbar_in_debug_mode(State::Running, &[first.clone()]));
    assert!(!is_statusbar_in_debug_mode(State::Running, &[first, second]));
}
