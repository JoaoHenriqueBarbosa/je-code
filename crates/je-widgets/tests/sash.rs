//! Porte de `src/vs/base/test/browser/ui/sash/sash.test.ts` (suíte `Sash`), caso a caso e na mesma ordem.

mod support;

use std::rc::Rc;

use je_ui::Ui;
use je_widgets::sash::{Sash, SashOptions};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, query_all};

#[test]
fn keeps_leased_and_pre_existing_classes_until_their_owners_release_them() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 0.0, 0.0);
    let sash = store.add_rc(Sash::new_vertical(&ui, container, Rc::new(|_: &Sash| 0.0), SashOptions::default()));
    let first_lease = store.add(sash.add_class("leased"));
    let second_lease = store.add(sash.add_class("leased"));
    let pre_existing_class_lease = store.add(sash.add_class("vertical"));
    let sash_element = query_all(&ui.tree(), container, &["monaco-sash"])[0];
    let has = |class: &str| ui.tree().has_class(sash_element, class);
    let mut leased_class_states = vec![has("leased")];

    first_lease.dispose();
    leased_class_states.push(has("leased"));

    second_lease.dispose();
    leased_class_states.push(has("leased"));

    pre_existing_class_lease.dispose();

    assert_eq!(leased_class_states, [true, true, false]);
    assert!(has("vertical"));
}
