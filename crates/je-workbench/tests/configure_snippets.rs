//! Porte de `src/vs/workbench/contrib/snippets/test/browser/configureSnippets.test.ts` (suíte `Configure Snippets`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo de cada caso.
//! - **`TestPick`.** `{ label, filepath }` é o [`ISnippetPick`] do porte com os demais campos ausentes; `reorder` devolve os
//!   rótulos dos dois grupos depois de `promoteActiveLanguage`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::quickinput::quick_input::IQuickPickItem;
use je_platform::resources::uri_file;
use je_workbench::contrib::snippets::commands::configure_snippets::{promote_active_language, ISnippetPick};

fn pick(label: &str, path: &str) -> ISnippetPick {
    let mut base = IQuickPickItem::default();
    base.base.label = label.into();
    ISnippetPick { base, filepath: uri_file(path), hint: None }
}

fn file(name: &str) -> ISnippetPick {
    pick(name, &format!("/snippets/{name}"))
}

fn lang(id: &str) -> ISnippetPick {
    pick(id, &format!("/snippets/{id}.json"))
}

fn labels(picks: &[ISnippetPick]) -> Vec<String> {
    picks.iter().map(|pick| pick.base.base.label.to_string_lossy()).collect()
}

fn reorder(mut existing: Vec<ISnippetPick>, mut future: Vec<ISnippetPick>, active_language_id: Option<&str>) -> (Vec<String>, Vec<String>) {
    promote_active_language(&mut existing, &mut future, active_language_id);
    (labels(&existing), labels(&future))
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[test]
fn promotes_the_active_language_in_both_groups() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // existing file for python, and a new-language entry for python -> both jump to front
    let existing = vec![file("global.code-snippets"), file("javascript.json"), file("python.json")];
    let future = vec![lang("csharp"), lang("go"), lang("python")];

    assert_eq!(
        reorder(existing, future, Some("python")),
        (strings(&["python.json", "global.code-snippets", "javascript.json"]), strings(&["python", "csharp", "go"]))
    );
}

#[test]
fn no_op_cases_no_active_language_already_first_or_not_present() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let existing = || vec![file("javascript.json"), file("python.json")];
    let future = || vec![lang("go"), lang("python")];

    // undefined active language leaves order untouched
    assert_eq!(reorder(existing(), future(), None), (strings(&["javascript.json", "python.json"]), strings(&["go", "python"])));

    // already first in the existing group -> untouched
    assert_eq!(
        reorder(existing(), future(), Some("javascript")),
        (strings(&["javascript.json", "python.json"]), strings(&["go", "python"])) // no future entry for javascript, so future untouched too
    );

    // language absent from both groups -> untouched
    assert_eq!(reorder(existing(), future(), Some("rust")), (strings(&["javascript.json", "python.json"]), strings(&["go", "python"])));
}
