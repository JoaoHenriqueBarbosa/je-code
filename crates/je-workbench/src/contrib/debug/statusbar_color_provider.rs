//! Porte de `isStatusbarInDebugMode` de `src/vs/workbench/contrib/debug/browser/statusbarColorProvider.ts`: se a barra de
//! status entra no modo de depuração. O `StatusBarColorProvider` (cores, DOM e contribuição do workbench) NÃO foi portado.

use std::rc::Rc;

use serde_json::Value;

use super::debug::IDebugSession;
use super::debug_types::{IDebugSessionFull, State};

/// `isStatusbarInDebugMode(state, sessions)`.
pub fn is_statusbar_in_debug_mode(state: State, sessions: &[Rc<dyn IDebugSessionFull>]) -> bool {
    if state == State::Inactive
        || state == State::Initializing
        || sessions.iter().all(|s| s.suppress_debug_statusbar() || s.configuration().get("noDebug").is_some_and(|no_debug| !matches!(no_debug, Value::Null | Value::Bool(false))))
    {
        return false;
    }

    true
}
