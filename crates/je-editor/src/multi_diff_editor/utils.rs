//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/utils.ts`: o `ActionRunnerWithContext`, um `ActionRunner` que
//! roda cada ação com o contexto que a função dada devolve no momento da execução, ignorando o contexto recebido.
//!
//! Divergência: `class ActionRunnerWithContext extends ActionRunner` com `runAction` sobrescrito não tem herança em Rust. O
//! `ActionRunner` de `je_base::actions` aceita o `runAction` como argumento ([`ActionRunner::with_run_action`]), e a
//! chamada `super.runAction(action, ctx)` é o [`ActionRunner::default_run_action`].

use std::rc::Rc;

use je_base::actions::ActionRunner;
use je_base::dynamic::Dynamic;

/// `new ActionRunnerWithContext(getContext)`.
pub fn action_runner_with_context(get_context: impl Fn() -> Dynamic + 'static) -> Rc<ActionRunner> {
    let super_run_action = ActionRunner::default_run_action();
    ActionRunner::with_run_action(Rc::new(move |action, _context| super_run_action(action, Some(get_context()))))
}
