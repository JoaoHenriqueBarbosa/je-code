//! Porte de `src/vs/base/test/browser/progressBar.test.ts` (suíte `ProgressBar`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: os `assert(bar.infinite())` do original só conferem que cada método devolve a própria
//! barra (um valor "verdadeiro"); em Rust os métodos devolvem `&ProgressBar`, e o encadeamento compilar é a
//! mesma garantia. O `fixture` é um nó do corpo da árvore, removido no `teardown`.

mod support;

use je_ui::{Role, Ui};
use je_widgets::dom::create_el;
use je_widgets::progress_bar::ProgressBar;
use support::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;

#[test]
fn progress_bar() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let fixture = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        create_el(&mut tree, root, Role::Generic, "")
    };

    let bar = ProgressBar::new(&ui, fixture, None);
    let _ = bar.infinite().total(100.0).worked(50.0).set_worked(70.0).worked(30.0).done();

    bar.dispose();
    ui.tree_mut().remove(fixture);
}
