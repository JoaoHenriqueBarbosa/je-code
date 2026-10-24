//! Porte de `src/vs/workbench/contrib/search/test/browser/searchActions.test.ts` (suíte `Search Actions`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O guarda de vazamentos é o [`LeakGuard`] criado no começo de cada caso, e o `TestInstantiationService`
//!   é o [`Fixture`] de `support/search_support.rs` (modelo, notebook, rótulo e chaveiro dublê já estão lá; o `lookupKeybinding` e o
//!   `resolveKeybinding` do original não são usados por nenhum caso).
//! - **`MockObjectTree`.** É a struct de mesmo nome daqui, que implementa [`ISearchActionsTree`] (os métodos que as funções
//!   usam) com o `ArrayNavigator` do original; `expand` devolve `true`, e o pai de um elemento é o `parent()` dele.
//! - **`async` e `assert.strictEqual`.** As funções devolvem uma `Promise`; o teste roda o loop de microtarefas e lê o resultado.
//!   A identidade (`===`) é [`RenderableMatch::is_same`].
//! - **`aFileMatch()`.** É o arquivo de [`Fixture::a_file_match_in_folder`] sobre uma pasta nova, com o `somepath<contador>` do original.
//! - **`aMatch(fileMatch)`.** É [`MatchImpl::new`] com os mesmos intervalos, adicionado ao arquivo.

mod support;

use std::cell::Cell;
use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::promise::Promise;
use je_base::runtime::run_until_idle;
use je_workbench::contrib::search::browser::search_actions_remove_replace::{
    ISearchActionsTree, ITreeNavigator, get_element_to_focus_after_removed, get_last_node_from_same_type,
};
use je_workbench::contrib::search::notebook_search::notebook_search_model_base::INotebookFileInstanceMatch;
use je_workbench::contrib::search::search_tree_model::r#match::MatchImpl;
use je_workbench::contrib::search::search_tree_model::search_tree_common::{ISearchTreeFileMatch, ISearchTreeMatch, RenderableMatch};
use je_workbench::services::search::search::SearchRange;
use support::search_support::{Fixture, LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, setup};

/// O `ArrayNavigator` de `mockSearchTree.ts`.
struct ArrayNavigator {
    elements: Vec<RenderableMatch>,
    index: isize,
}

impl ArrayNavigator {
    fn at(&self) -> Option<RenderableMatch> {
        usize::try_from(self.index).ok().and_then(|index| self.elements.get(index).cloned())
    }
}

impl ITreeNavigator for ArrayNavigator {
    fn current(&self) -> Option<RenderableMatch> {
        self.at()
    }

    fn previous(&mut self) -> Option<RenderableMatch> {
        self.index -= 1;
        self.at()
    }

    fn first(&mut self) -> Option<RenderableMatch> {
        self.index = 0;
        self.at()
    }

    fn last(&mut self) -> Option<RenderableMatch> {
        self.index = self.elements.len() as isize - 1;
        self.at()
    }

    fn next(&mut self) -> Option<RenderableMatch> {
        self.index += 1;
        self.at()
    }
}

/// `MockObjectTree` de `mockSearchTree.ts`.
struct MockObjectTree {
    elements: Vec<RenderableMatch>,
}

impl ISearchActionsTree for MockObjectTree {
    fn navigate(&self, start: Option<&RenderableMatch>) -> Box<dyn ITreeNavigator> {
        let index = match start {
            Some(start) => self.elements.iter().position(|element| element.is_same(start)).map_or(-1, |index| index as isize),
            None => 0,
        };
        Box::new(ArrayNavigator { elements: self.elements.clone(), index })
    }

    fn expand(&self, _element: &RenderableMatch) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn last_visible_element(&self) -> Option<RenderableMatch> {
        self.elements.last().cloned()
    }

    fn get_parent_element(&self, element: &RenderableMatch) -> Option<RenderableMatch> {
        element.parent()
    }
}

/// Roda o loop e devolve o valor da promessa.
fn settle(promise: Promise<Option<RenderableMatch>>) -> Option<RenderableMatch> {
    run_until_idle();
    promise.result().expect("a promessa assentou").expect("a promessa foi cumprida")
}

fn assert_same(expected: Option<&RenderableMatch>, actual: Option<RenderableMatch>) {
    match (expected, actual) {
        (None, None) => {}
        (Some(expected), Some(actual)) => assert!(expected.is_same(&actual)),
        _ => panic!("os elementos diferem"),
    }
}

struct Context<'a> {
    t: Fixture,
    guard: &'a LeakGuard,
    counter: Cell<i64>,
}

impl Context<'_> {
    fn next_counter(&self) -> i64 {
        self.counter.set(self.counter.get() + 1);
        self.counter.get()
    }

    fn a_file_match(&self) -> Rc<dyn INotebookFileInstanceMatch> {
        let path = format!("somepath{}", self.next_counter());
        let folder_match = self.t.a_folder_match(self.guard, "", 0, None);
        self.t.a_file_match_in_folder(self.guard, &path, folder_match, Vec::new())
    }

    fn a_match(&self, file_match: &Rc<dyn INotebookFileInstanceMatch>) -> RenderableMatch {
        let line = self.next_counter().try_into().expect("o contador cabe no número de linha");
        let parent: Rc<dyn ISearchTreeFileMatch> = file_match.clone();
        let match_: Rc<dyn ISearchTreeMatch> = MatchImpl::new(
            &parent,
            vec![JsString::from("some match")],
            SearchRange::new(0, 0, 0, 2),
            SearchRange::new(line, 0, line, 2),
            false,
        );
        parent.add(match_.clone(), false);
        RenderableMatch::Match(match_)
    }
}

fn file_element(file_match: &Rc<dyn INotebookFileInstanceMatch>) -> RenderableMatch {
    RenderableMatch::FileMatch(file_match.clone())
}

fn a_tree(elements: Vec<RenderableMatch>) -> Rc<dyn ISearchActionsTree> {
    Rc::new(MockObjectTree { elements })
}

#[test]
fn get_next_element_to_focus_after_removing_a_match_when_it_has_next_sibling_file() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let c = Context { t: setup(&guard), guard: &guard, counter: Cell::new(0) };
    let file_match1 = c.a_file_match();
    let file_match2 = c.a_file_match();
    let data = vec![
        file_element(&file_match1),
        c.a_match(&file_match1),
        c.a_match(&file_match1),
        file_element(&file_match2),
        c.a_match(&file_match2),
        c.a_match(&file_match2),
    ];
    let tree = a_tree(data.clone());
    let target = data[2].clone();

    let actual = settle(get_element_to_focus_after_removed(&tree, &target, &[target.clone()]));
    assert_same(Some(&data[4]), actual);
}

#[test]
fn get_next_element_to_focus_after_removing_a_match_when_it_is_the_only_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let c = Context { t: setup(&guard), guard: &guard, counter: Cell::new(0) };
    let file_match1 = c.a_file_match();
    let data = vec![file_element(&file_match1), c.a_match(&file_match1)];
    let tree = a_tree(data.clone());
    let target = data[1].clone();

    let actual = settle(get_element_to_focus_after_removed(&tree, &target, &[target.clone()]));
    assert_same(None, actual);
}

#[test]
fn get_next_element_to_focus_after_removing_a_file_match_when_it_has_next_sibling() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let c = Context { t: setup(&guard), guard: &guard, counter: Cell::new(0) };
    let file_match1 = c.a_file_match();
    let file_match2 = c.a_file_match();
    let file_match3 = c.a_file_match();
    let data = vec![
        file_element(&file_match1),
        c.a_match(&file_match1),
        file_element(&file_match2),
        c.a_match(&file_match2),
        file_element(&file_match3),
        c.a_match(&file_match3),
    ];
    let tree = a_tree(data.clone());
    let target = data[2].clone();

    let actual = settle(get_element_to_focus_after_removed(&tree, &target, &[]));
    assert_same(Some(&data[4]), actual);
}

#[test]
fn find_last_file_match_in_tree() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let c = Context { t: setup(&guard), guard: &guard, counter: Cell::new(0) };
    let file_match1 = c.a_file_match();
    let file_match2 = c.a_file_match();
    let file_match3 = c.a_file_match();
    let data = vec![
        file_element(&file_match1),
        c.a_match(&file_match1),
        file_element(&file_match2),
        c.a_match(&file_match2),
        file_element(&file_match3),
        c.a_match(&file_match3),
    ];
    let tree = a_tree(data);

    let actual = settle(get_last_node_from_same_type(&tree, &file_element(&file_match1)));
    assert_same(Some(&file_element(&file_match3)), actual);
}

#[test]
fn find_last_match_in_tree() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let c = Context { t: setup(&guard), guard: &guard, counter: Cell::new(0) };
    let file_match1 = c.a_file_match();
    let file_match2 = c.a_file_match();
    let file_match3 = c.a_file_match();
    let data = vec![
        file_element(&file_match1),
        c.a_match(&file_match1),
        file_element(&file_match2),
        c.a_match(&file_match2),
        file_element(&file_match3),
        c.a_match(&file_match3),
    ];
    let tree = a_tree(data.clone());

    let actual = settle(get_last_node_from_same_type(&tree, &c.a_match(&file_match1)));
    assert_same(Some(&data[5]), actual);
}

#[test]
fn get_next_element_to_focus_after_removing_a_file_match_when_it_is_only_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let c = Context { t: setup(&guard), guard: &guard, counter: Cell::new(0) };
    let file_match1 = c.a_file_match();
    let data = vec![file_element(&file_match1), c.a_match(&file_match1)];
    let tree = a_tree(data.clone());
    let target = data[0].clone();

    let actual = settle(get_element_to_focus_after_removed(&tree, &target, &[]));
    assert_same(None, actual);
}
