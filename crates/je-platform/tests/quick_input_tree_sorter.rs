//! Porte de `src/vs/platform/quickinput/test/browser/tree/quickInputTreeSorter.test.ts` (suíte `QuickInputTreeSorter`), caso a caso e na mesma ordem.
//!
//! Divergências: `store.add(new QuickInputTreeSorter())` é `guard.add_rc(QuickInputTreeSorter::new())`. O
//! literal `{ label, description }` é o helper `item`, que preenche o `base` do `IQuickTreeItem`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::quickinput::quick_input::{IQuickItem, IQuickTreeItem};
use je_platform::quickinput::tree::quick_input_tree_sorter::QuickInputTreeSorter;

fn item(label: &str, description: Option<&str>) -> IQuickTreeItem {
    IQuickTreeItem { base: IQuickItem { label: label.into(), description: description.map(Into::into), ..Default::default() }, ..Default::default() }
}

#[test]
fn sort_by_label_property_defaults_to_true() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sorter = guard.add_rc(QuickInputTreeSorter::new());
    assert!(sorter.sort_by_label());
}

#[test]
fn sort_by_label_property_can_be_set_to_false() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sorter = guard.add_rc(QuickInputTreeSorter::new());
    sorter.set_sort_by_label(false);
    assert!(!sorter.sort_by_label());
}

#[test]
fn compare_returns_0_when_sort_by_label_is_false() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sorter = guard.add_rc(QuickInputTreeSorter::new());
    sorter.set_sort_by_label(false);

    let item1 = item("a", None);
    let item2 = item("b", None);

    assert_eq!(sorter.compare(&item1, &item2), 0);
    assert_eq!(sorter.compare(&item2, &item1), 0);
    assert_eq!(sorter.compare(&item1, &item1), 0);
}

#[test]
fn compare_sorts_by_label_alphabetically() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sorter = guard.add_rc(QuickInputTreeSorter::new());

    let item1 = item("a", None);
    let item2 = item("b", None);
    let item3 = item("c", None);

    assert_eq!(sorter.compare(&item1, &item2), -1);
    assert_eq!(sorter.compare(&item2, &item1), 1);
    assert_eq!(sorter.compare(&item1, &item1), 0);
    assert_eq!(sorter.compare(&item2, &item3), -1);
    assert_eq!(sorter.compare(&item3, &item2), 1);
}

#[test]
fn compare_sorts_by_description_when_labels_are_equal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sorter = guard.add_rc(QuickInputTreeSorter::new());

    let item1 = item("a", Some("x"));
    let item2 = item("a", Some("y"));
    let item3 = item("a", Some("z"));
    let item4 = item("a", None);

    assert_eq!(sorter.compare(&item1, &item2), -1);
    assert_eq!(sorter.compare(&item2, &item1), 1);
    assert_eq!(sorter.compare(&item2, &item3), -1);
    assert_eq!(sorter.compare(&item3, &item2), 1);
    assert_eq!(sorter.compare(&item1, &item4), -1);
    assert_eq!(sorter.compare(&item4, &item1), 1);
    assert_eq!(sorter.compare(&item4, &item4), 0);
}

#[test]
fn compare_handles_items_with_no_description() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sorter = guard.add_rc(QuickInputTreeSorter::new());

    let item1 = item("a", Some("desc"));
    let item2 = item("b", None);

    assert_eq!(sorter.compare(&item1, &item2), -1);
    assert_eq!(sorter.compare(&item2, &item1), 1);
}

#[test]
fn compare_handles_empty_labels() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sorter = guard.add_rc(QuickInputTreeSorter::new());

    let item1 = item("", None);
    let item2 = item("a", None);
    let item3 = item("", None);

    assert_eq!(sorter.compare(&item1, &item2), -1);
    assert_eq!(sorter.compare(&item2, &item1), 1);
    assert_eq!(sorter.compare(&item1, &item3), 0);
}
