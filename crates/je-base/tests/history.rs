//! Porte de `src/vs/base/test/common/history.test.ts` (suítes `History Navigator` e `History Navigator 2`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque o navegador sem `onDidChange` não cria
//! `IDisposable`. O `new Set([...])` do original é `HistoryNavigator::from_values`, e `null` é `None`.

use std::rc::Rc;

use je_base::errors;
use je_base::history::{HistoryNavigator, HistoryNavigator2};
use je_base::navigator::INavigator;

fn nav(values: &[&str], limit: usize) -> Rc<HistoryNavigator<String>> {
    HistoryNavigator::from_values(values.iter().map(ToString::to_string), limit)
}

fn nav_default(values: &[&str]) -> Rc<HistoryNavigator<String>> {
    nav(values, 10)
}

fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

fn to_array(history_navigator: &HistoryNavigator<String>) -> Vec<String> {
    let mut result = Vec::new();
    history_navigator.first();
    if history_navigator.current().is_some_and(|c| !c.is_empty()) {
        loop {
            result.push(history_navigator.current().unwrap());
            if history_navigator.next().is_none_or(|c| c.is_empty()) {
                break;
            }
        }
    }
    result
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn create_reduces_the_input_to_limit() {
    let test_object = nav(&["1", "2", "3", "4"], 2);

    assert_eq!(strings(&["3", "4"]), to_array(&test_object));
}

#[test]
fn create_sets_the_position_after_last() {
    let test_object = nav(&["1", "2", "3", "4"], 100);

    assert_eq!(test_object.current(), None);
    assert!(test_object.is_nowhere());
    assert!(!test_object.is_first());
    assert!(!test_object.is_last());
    assert_eq!(test_object.next(), None);
    assert_eq!(test_object.previous(), s("4"));
    assert!(!test_object.is_nowhere());
    assert!(!test_object.is_first());
    assert!(test_object.is_last());
}

#[test]
fn last_returns_last_element() {
    let test_object = nav(&["1", "2", "3", "4"], 100);

    assert_eq!(test_object.first(), s("1"));
    assert_eq!(test_object.last(), s("4"));
    assert!(!test_object.is_first());
    assert!(test_object.is_last());
}

#[test]
fn first_returns_first_element() {
    let test_object = nav(&["1", "2", "3", "4"], 3);

    assert_eq!(s("2"), test_object.first());
    assert!(test_object.is_first());
    assert!(!test_object.is_last());
}

#[test]
fn next_returns_next_element() {
    let test_object = nav(&["1", "2", "3", "4"], 3);

    test_object.first();

    assert_eq!(test_object.next(), s("3"));
    assert_eq!(test_object.next(), s("4"));
    assert_eq!(test_object.next(), None);
}

#[test]
fn previous_returns_previous_element() {
    let test_object = nav(&["1", "2", "3", "4"], 3);

    assert_eq!(test_object.previous(), s("4"));
    assert_eq!(test_object.previous(), s("3"));
    assert_eq!(test_object.previous(), s("2"));
    assert_eq!(test_object.previous(), None);
}

#[test]
fn next_on_last_element_returns_null_and_remains_on_last() {
    let test_object = nav(&["1", "2", "3", "4"], 3);

    test_object.first();
    test_object.last();

    assert!(test_object.is_last());
    assert_eq!(test_object.current(), s("4"));
    assert_eq!(test_object.next(), None);
    assert!(!test_object.is_last()); // Stepping past the last element, is no longer "last"
}

#[test]
fn previous_on_first_element_returns_null_and_remains_on_first() {
    let test_object = nav(&["1", "2", "3", "4"], 3);

    test_object.first();

    assert!(test_object.is_first());
    assert_eq!(test_object.current(), s("2"));
    assert_eq!(test_object.previous(), None);
    assert!(test_object.is_first());
}

#[test]
fn add_reduces_the_input_to_limit() {
    let test_object = nav(&["1", "2", "3", "4"], 2);

    test_object.add("5".into());

    assert_eq!(to_array(&test_object), strings(&["4", "5"]));
}

#[test]
fn adding_existing_element_changes_the_position() {
    let test_object = nav(&["1", "2", "3", "4"], 5);

    test_object.add("2".into());

    assert_eq!(to_array(&test_object), strings(&["1", "3", "4", "2"]));
}

#[test]
fn add_resets_the_navigator_to_last() {
    let test_object = nav(&["1", "2", "3", "4"], 3);

    test_object.first();
    test_object.add("5".into());

    assert_eq!(test_object.previous(), s("5"));
    assert!(test_object.is_last());
    assert_eq!(test_object.next(), None);
    assert!(!test_object.is_last());
}

#[test]
fn adding_an_existing_item_changes_the_order() {
    let test_object = nav_default(&["1", "2", "3"]);

    test_object.add("1".into());

    assert_eq!(strings(&["2", "3", "1"]), to_array(&test_object));
}

#[test]
fn previous_returns_null_if_the_current_position_is_the_first_one() {
    let test_object = nav_default(&["1", "2", "3"]);

    test_object.first();

    assert_eq!(test_object.previous(), None);
    assert!(test_object.is_first());
}

#[test]
fn previous_returns_object_if_the_current_position_is_not_the_first_one() {
    let test_object = nav_default(&["1", "2", "3"]);

    test_object.first();
    test_object.next();

    assert_eq!(test_object.previous(), s("1"));
}

#[test]
fn next_returns_null_if_the_current_position_is_the_last_one() {
    let test_object = nav_default(&["1", "2", "3"]);

    test_object.last();

    assert!(test_object.is_last());
    assert_eq!(test_object.next(), None);
    assert!(!test_object.is_last());
}

#[test]
fn next_returns_object_if_the_current_position_is_not_the_last_one() {
    let test_object = nav_default(&["1", "2", "3"]);

    test_object.last();
    test_object.previous();

    assert_eq!(test_object.next(), s("3"));
}

#[test]
fn clear() {
    let test_object = nav_default(&["a", "b", "c"]);
    assert_eq!(test_object.previous(), s("c"));
    test_object.clear();
    assert_eq!(test_object.current(), None);
    assert!(test_object.is_nowhere());
}

fn nav2(values: &[&str]) -> HistoryNavigator2<String, String> {
    HistoryNavigator2::with_defaults(&strings(values))
}

#[test]
fn constructor() {
    let test_object = nav2(&["1", "2", "3", "4"]);

    assert_eq!(test_object.current(), "4");
    assert!(test_object.is_at_end());
}

#[test]
fn constructor_initial_history_is_not_empty() {
    assert!(errors::catch(|| nav2(&[])).is_err());
}

#[test]
fn constructor_capacity_limit() {
    let test_object = HistoryNavigator2::new(&strings(&["1", "2", "3", "4"]), 3, |t: &String| t.clone());

    assert_eq!(test_object.current(), "4");
    assert!(test_object.is_at_end());
    assert!(!test_object.has(&"1".to_string()));
}

#[test]
fn constructor_duplicate_values() {
    let test_object = nav2(&["1", "2", "3", "4", "3", "2", "1"]);

    assert_eq!(test_object.current(), "1");
    assert!(test_object.is_at_end());
}

#[test]
fn navigation() {
    let test_object = nav2(&["1", "2", "3", "4"]);

    assert_eq!(test_object.current(), "4");
    assert!(test_object.is_at_end());

    assert_eq!(test_object.next(), "4");
    assert_eq!(test_object.previous(), "3");
    assert_eq!(test_object.previous(), "2");
    assert_eq!(test_object.previous(), "1");
    assert_eq!(test_object.previous(), "1");

    assert_eq!(test_object.current(), "1");
    assert_eq!(test_object.next(), "2");
    assert_eq!(test_object.reset_cursor(), "4");
}

#[test]
fn add() {
    let mut test_object = nav2(&["1", "2", "3", "4"]);
    test_object.add("5".into());

    assert_eq!(test_object.current(), "5");
    assert!(test_object.is_at_end());
}

#[test]
fn add_existing_value() {
    let mut test_object = nav2(&["1", "2", "3", "4"]);
    test_object.add("2".into());

    assert_eq!(test_object.current(), "2");
    assert!(test_object.is_at_end());

    assert_eq!(test_object.previous(), "4");
    assert_eq!(test_object.previous(), "3");
    assert_eq!(test_object.previous(), "1");
}

#[test]
fn replace_last() {
    let mut test_object = nav2(&["1", "2", "3", "4"]);
    test_object.replace_last("5".into());

    assert_eq!(test_object.current(), "5");
    assert!(test_object.is_at_end());
    assert!(!test_object.has(&"4".to_string()));

    assert_eq!(test_object.previous(), "3");
    assert_eq!(test_object.previous(), "2");
    assert_eq!(test_object.previous(), "1");
}

#[test]
fn replace_last_existing_value() {
    let mut test_object = nav2(&["1", "2", "3", "4"]);
    test_object.replace_last("2".into());

    assert_eq!(test_object.current(), "2");
    assert!(test_object.is_at_end());
    assert!(!test_object.has(&"4".to_string()));

    assert_eq!(test_object.previous(), "3");
    assert_eq!(test_object.previous(), "1");
}

#[test]
fn prepend() {
    let mut test_object = nav2(&["1", "2", "3", "4"]);
    assert_eq!(test_object.current(), "4");
    assert!(test_object.is_at_end());
    assert_eq!(test_object.iter().cloned().collect::<Vec<_>>(), strings(&["1", "2", "3", "4"]));

    test_object.prepend("0".into());
    assert_eq!(test_object.current(), "4");
    assert!(test_object.is_at_end());
    assert_eq!(test_object.iter().cloned().collect::<Vec<_>>(), strings(&["0", "1", "2", "3", "4"]));

    test_object.prepend("2".into());
    assert_eq!(test_object.current(), "4");
    assert!(test_object.is_at_end());
    assert_eq!(test_object.iter().cloned().collect::<Vec<_>>(), strings(&["0", "1", "2", "3", "4"]));
}
