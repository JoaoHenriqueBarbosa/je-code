//! Testes de `FindFilter`, `FindToggles` e `contiguousFuzzyScore` de `src/vs/base/browser/ui/tree/abstractTree.ts`
//! (sem teste no VS Code: casos novos, não é porte).

use std::rc::Rc;

use je_widgets::list::list_types::IKeyboardNavigationLabelProvider;
use je_widgets::tree::abstract_tree::{TreeFindMatchType, TreeFindMode};
use je_widgets::tree::find::{FindFilter, FindFilterData, FindToggles, TreeFindToggleContribution, contiguous_fuzzy_score};
use je_widgets::tree::tree_types::{ITreeFilter, TreeFilterResult, TreeVisibility, VisibilityValue};

struct Labels;

impl IKeyboardNavigationLabelProvider<&'static str> for Labels {
    fn get_keyboard_navigation_label(&self, element: &&'static str) -> Vec<Option<String>> {
        vec![Some((*element).to_owned())]
    }
}

fn filter() -> Rc<FindFilter<&'static str>> {
    FindFilter::new(Rc::new(Labels), None, None)
}

fn data(result: TreeFilterResult<FindFilterData>) -> (FindFilterData, TreeVisibility) {
    match result {
        TreeFilterResult::Data { data, visibility: VisibilityValue::Visibility(v) } => (data, v),
        _ => panic!("esperava o dado do filtro"),
    }
}

#[test]
fn contiguous_score_finds_the_substring() {
    assert_eq!(contiguous_fuzzy_score("ell", "hello"), Some(vec![i32::MAX, 0, 3, 2, 1]));
    assert_eq!(contiguous_fuzzy_score("xyz", "hello"), None);
}

#[test]
fn empty_pattern_matches_everything_with_the_default_score() {
    let filter = filter();
    let (score, visibility) = data(filter.filter(&"hello", TreeVisibility::Visible));

    assert!(FindFilterData::is_default(Some(&score)));
    assert_eq!(visibility, TreeVisibility::Visible);
    assert_eq!((filter.total_count(), filter.match_count()), (1, 1));
}

#[test]
fn highlight_mode_keeps_non_matching_elements_with_the_default_score() {
    let filter = filter();
    filter.set_pattern("hel");

    let (matched, _) = data(filter.filter(&"hello", TreeVisibility::Visible));
    let (other, _) = data(filter.filter(&"world", TreeVisibility::Visible));

    assert!(!FindFilterData::is_default(Some(&matched)));
    assert!(FindFilterData::is_default(Some(&other)));
    assert_eq!((filter.total_count(), filter.match_count()), (2, 1));

    filter.reset();
    assert_eq!((filter.total_count(), filter.match_count()), (0, 0));
}

#[test]
fn filter_mode_recurses_into_non_matching_elements() {
    let filter = filter();
    filter.set_pattern("hel");
    filter.set_find_mode(TreeFindMode::Filter);

    assert!(matches!(filter.filter(&"world", TreeVisibility::Visible), TreeFilterResult::Visibility(TreeVisibility::Recurse)));
}

#[test]
fn contiguous_match_type_rejects_scattered_letters() {
    let filter = filter();
    filter.set_pattern("hlo");
    let (fuzzy, _) = data(filter.filter(&"hello", TreeVisibility::Visible));
    assert!(!FindFilterData::is_default(Some(&fuzzy)));

    filter.set_find_match_type(TreeFindMatchType::Contiguous);
    let (contiguous, _) = data(filter.filter(&"hello", TreeVisibility::Visible));
    assert!(FindFilterData::is_default(Some(&contiguous)));
}

#[test]
fn toggles_report_whether_the_value_changed() {
    let toggles = FindToggles::new(vec![TreeFindToggleContribution { id: "mode".to_owned(), title: "Filter".to_owned(), icon: je_base::themables::ThemeIcon::from_id("list-filter"), is_checked: false }]);

    assert!(!toggles.get("mode"));
    assert!(toggles.set("mode", true));
    assert!(!toggles.set("mode", true));
    assert!(toggles.get("mode"));
}
