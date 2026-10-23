//! Testes próprios de `src/vs/base/common/hierarchicalKind.ts` (o VS Code não tem
//! `hierarchicalKind.test.ts`).

use je_base::hierarchical_kind::HierarchicalKind;

fn kind(value: &str) -> HierarchicalKind {
    HierarchicalKind::new(value)
}

#[test]
fn contains_follows_the_hierarchy() {
    assert!(kind("refactor").contains(&kind("refactor")));
    assert!(kind("refactor").contains(&kind("refactor.extract")));
    assert!(kind("refactor").contains(&kind("refactor.extract.function")));
    assert!(!kind("refactor.extract").contains(&kind("refactor")));
    // O separador conta: `refactor` não contém `refactoring`.
    assert!(!kind("refactor").contains(&kind("refactoring")));
}

#[test]
fn empty_contains_everything_and_none_contains_nothing_else() {
    assert!(HierarchicalKind::empty().contains(&kind("a.b")));
    assert!(HierarchicalKind::empty().contains(&HierarchicalKind::none()));
    assert!(!HierarchicalKind::none().contains(&kind("a")));
    assert!(HierarchicalKind::none().contains(&HierarchicalKind::none()));
}

#[test]
fn intersects_is_symmetric_containment() {
    assert!(kind("a").intersects(&kind("a.b")));
    assert!(kind("a.b").intersects(&kind("a")));
    assert!(!kind("a.b").intersects(&kind("a.c")));
}

#[test]
fn append_joins_with_the_separator() {
    assert_eq!(HierarchicalKind::empty().append(&["a", "b"]).value, "a.b");
    assert_eq!(kind("x").append(&["a", "b"]).value, "x.a.b");
    assert_eq!(kind("x").append(&[]).value, "x");
    assert!(kind("x").equals(&kind("x")));
}
