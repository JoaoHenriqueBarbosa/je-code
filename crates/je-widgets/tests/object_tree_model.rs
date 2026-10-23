//! Porte de `src/vs/base/test/browser/ui/tree/objectTreeModel.test.ts` (suíte `ObjectTreeModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `number` é `i32` e `string` é `String`; `{ element, children, collapsed }` é `IObjectTreeElement`, e o
//!   `null` da raiz é `None`. `model.setChildren(x)` sem filhos passa `vec![]`.
//! - `bindListToModel` é genérico no modelo (o nó vem do tipo associado), como o `ITreeModel<T, any, any>`.
//! - O `compare` mutável do teste `resort` e o `fn` mutável do `issue #95641` são `Rc<RefCell<Box<dyn Fn>>>`.
//! - A identidade estrita dos elementos string é a igualdade de valor, como no JavaScript.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelOptions};
use je_widgets::tree::tree_types::{IObjectTreeElement, ITreeFilter, ITreeModel, ITreeSorter, ObjectTreeElementCollapseState, TreeFilterResult, TreeVisibility};
use support::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};

type List<N> = Rc<RefCell<Vec<Rc<N>>>>;

/// `bindListToModel`.
fn bind_list_to_model<M: ITreeModel>(list: &List<M::Node>, model: &M) -> DisposableRef {
    let list = list.clone();
    model.on_did_splice_rendered_nodes().listen(move |e| {
        let mut list = list.borrow_mut();
        let end = (e.start + e.delete_count).min(list.len());
        list.splice(e.start..end, e.elements.iter().cloned());
    })
}

/// `toArray`.
fn to_array<T: Clone + 'static>(list: &List<je_widgets::tree::index_tree_model::TreeNode<Option<T>>>) -> Vec<T> {
    list.borrow().iter().map(|i| i.element.clone().expect("só a raiz tem elemento nulo")).collect()
}

fn el<T>(element: T) -> IObjectTreeElement<T> {
    IObjectTreeElement::new(element)
}

fn str_el(element: &str) -> IObjectTreeElement<String> {
    el(element.to_owned())
}

fn str_with_children(element: &str, children: Vec<IObjectTreeElement<String>>) -> IObjectTreeElement<String> {
    IObjectTreeElement::with_children(element.to_owned(), children)
}

fn s(element: &str) -> Option<String> {
    Some(element.to_owned())
}

type Compare = Rc<RefCell<Box<dyn Fn(&str, &str) -> i32>>>;
type Predicate = Rc<RefCell<Box<dyn Fn(&str) -> bool>>>;

/// `{ compare(a, b) { return compare(a, b); } }`: o `compare` pode ser trocado depois.
struct DelegatingSorter(Compare);

impl ITreeSorter<String> for DelegatingSorter {
    fn compare(&self, a: &String, b: &String) -> i32 {
        (self.0.borrow())(a, b)
    }
}

fn vehicles() -> Vec<IObjectTreeElement<String>> {
    vec![
        str_with_children("cars", vec![str_el("sedan"), str_el("convertible"), str_el("compact")]),
        str_with_children("airplanes", vec![str_el("passenger"), str_el("jet")]),
        str_with_children("bicycles", vec![str_el("dutch"), str_el("mountain"), str_el("electric")]),
    ]
}

mod object_tree_model {
    use super::*;

    #[test]
    fn ctor() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<i32>::new("test", Default::default());
        let disposable = bind_list_to_model(&list, &model);

        assert_eq!(list.borrow().len(), 0);
        assert_eq!(model.size(), 0);

        disposable.dispose();
    }

    #[test]
    fn flat() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<i32>::new("test", Default::default());
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(&None, vec![el(0), el(1), el(2)], Default::default());

        assert_eq!(to_array(&list), [0, 1, 2]);
        assert_eq!(model.size(), 3);

        model.set_children(&None, vec![el(3), el(4), el(5)], Default::default());

        assert_eq!(to_array(&list), [3, 4, 5]);
        assert_eq!(model.size(), 3);

        model.set_children(&None, vec![], Default::default());
        assert_eq!(to_array(&list), Vec::<i32>::new());
        assert_eq!(model.size(), 0);

        disposable.dispose();
    }

    #[test]
    fn nested() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<i32>::new("test", Default::default());
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(&None, vec![IObjectTreeElement::with_children(0, vec![el(10), el(11), el(12)]), el(1), el(2)], Default::default());

        assert_eq!(to_array(&list), [0, 10, 11, 12, 1, 2]);
        assert_eq!(model.size(), 6);

        model.set_children(&Some(12), vec![el(120), el(121)], Default::default());

        assert_eq!(to_array(&list), [0, 10, 11, 12, 120, 121, 1, 2]);
        assert_eq!(model.size(), 8);

        model.set_children(&Some(0), vec![], Default::default());
        assert_eq!(to_array(&list), [0, 1, 2]);
        assert_eq!(model.size(), 3);

        model.set_children(&None, vec![], Default::default());
        assert_eq!(to_array(&list), Vec::<i32>::new());
        assert_eq!(model.size(), 0);

        disposable.dispose();
    }

    #[test]
    fn set_children_on_collapsed_node() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<i32>::new("test", Default::default());
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(&None, vec![el(0).collapsed(true)], Default::default());

        assert_eq!(to_array(&list), [0]);

        model.set_children(&Some(0), vec![el(1), el(2)], Default::default());

        assert_eq!(to_array(&list), [0]);

        model.set_collapsed(&Some(0), Some(false), None);
        assert_eq!(to_array(&list), [0, 1, 2]);

        disposable.dispose();
    }

    #[test]
    fn set_children_on_expanded_unrevealed_node() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<i32>::new("test", Default::default());
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(&None, vec![IObjectTreeElement::with_children(1, vec![el(11).collapsed(false)]).collapsed(true), el(2)], Default::default());

        assert_eq!(to_array(&list), [1, 2]);

        model.set_children(&Some(11), vec![el(111), el(112)], Default::default());

        assert_eq!(to_array(&list), [1, 2]);

        model.set_collapsed(&Some(1), Some(false), None);
        assert_eq!(to_array(&list), [1, 11, 111, 112, 2]);

        disposable.dispose();
    }

    #[test]
    fn collapse_state_is_preserved_with_strict_identity() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<String>::new("test", ObjectTreeModelOptions { collapse_by_default: true, ..Default::default() });
        let data = vec![str_with_children("father", vec![str_el("child")])];
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(&None, data.clone(), Default::default());
        assert_eq!(to_array(&list), ["father"]);

        model.set_collapsed(&s("father"), Some(false), None);
        assert_eq!(to_array(&list), ["father", "child"]);

        model.set_children(&None, data.clone(), Default::default());
        assert_eq!(to_array(&list), ["father", "child"]);

        let data2 = vec![str_with_children("father", vec![str_el("child")]), str_el("uncle")];
        model.set_children(&None, data2.clone(), Default::default());
        assert_eq!(to_array(&list), ["father", "child", "uncle"]);

        model.set_children(&None, vec![str_el("uncle")], Default::default());
        assert_eq!(to_array(&list), ["uncle"]);

        model.set_children(&None, data2, Default::default());
        assert_eq!(to_array(&list), ["father", "uncle"]);

        model.set_children(&None, data, Default::default());
        assert_eq!(to_array(&list), ["father"]);

        disposable.dispose();
    }

    #[test]
    fn collapse_state_can_be_optionally_preserved_with_strict_identity() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<String>::new("test", ObjectTreeModelOptions { collapse_by_default: true, ..Default::default() });
        let data = vec![str_with_children("father", vec![str_el("child")]).collapse_state(ObjectTreeElementCollapseState::PreserveOrExpanded)];
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(&None, data.clone(), Default::default());
        assert_eq!(to_array(&list), ["father", "child"]);

        model.set_collapsed(&s("father"), Some(true), None);
        assert_eq!(to_array(&list), ["father"]);

        model.set_children(&None, data.clone(), Default::default());
        assert_eq!(to_array(&list), ["father"]);

        model.set_collapsed(&s("father"), Some(false), None);
        assert_eq!(to_array(&list), ["father", "child"]);

        model.set_children(&None, data, Default::default());
        assert_eq!(to_array(&list), ["father", "child"]);

        disposable.dispose();
    }

    #[test]
    fn sorter() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let compare: Compare = Rc::new(RefCell::new(Box::new(|a, b| if a < b { -1 } else { 1 })));

        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<String>::new("test", ObjectTreeModelOptions { sorter: Some(Rc::new(DelegatingSorter(compare))), ..Default::default() });
        let data = vehicles();
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(&None, data, Default::default());
        assert_eq!(to_array(&list), ["airplanes", "jet", "passenger", "bicycles", "dutch", "electric", "mountain", "cars", "compact", "convertible", "sedan"]);

        disposable.dispose();
    }

    #[test]
    fn resort() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let compare: Compare = Rc::new(RefCell::new(Box::new(|_, _| 0)));

        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<String>::new("test", ObjectTreeModelOptions { sorter: Some(Rc::new(DelegatingSorter(compare.clone()))), ..Default::default() });
        let data = vehicles();
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(&None, data, Default::default());
        assert_eq!(to_array(&list), ["cars", "sedan", "convertible", "compact", "airplanes", "passenger", "jet", "bicycles", "dutch", "mountain", "electric"]);

        // lexicographical
        *compare.borrow_mut() = Box::new(|a, b| if a < b { -1 } else { 1 });

        // non-recursive
        model.resort(&None, false);
        assert_eq!(to_array(&list), ["airplanes", "passenger", "jet", "bicycles", "dutch", "mountain", "electric", "cars", "sedan", "convertible", "compact"]);

        // recursive
        model.resort(&None, true);
        assert_eq!(to_array(&list), ["airplanes", "jet", "passenger", "bicycles", "dutch", "electric", "mountain", "cars", "compact", "convertible", "sedan"]);

        // reverse
        *compare.borrow_mut() = Box::new(|a, b| if a < b { 1 } else { -1 });

        // scoped
        model.resort(&s("cars"), true);
        assert_eq!(to_array(&list), ["airplanes", "jet", "passenger", "bicycles", "dutch", "electric", "mountain", "cars", "sedan", "convertible", "compact"]);

        // recursive
        model.resort(&None, true);
        assert_eq!(to_array(&list), ["cars", "sedan", "convertible", "compact", "bicycles", "mountain", "electric", "dutch", "airplanes", "passenger", "jet"]);

        disposable.dispose();
    }

    #[test]
    fn expand_to() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<_> = Rc::default();
        let model = ObjectTreeModel::<i32>::new("test", ObjectTreeModelOptions { collapse_by_default: true, ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);

        model.set_children(
            &None,
            vec![
                IObjectTreeElement::with_children(
                    0,
                    vec![IObjectTreeElement::with_children(10, vec![IObjectTreeElement::with_children(100, vec![el(1000)])]), el(11), el(12)],
                ),
                el(1),
                el(2),
            ],
            Default::default(),
        );

        assert_eq!(to_array(&list), [0, 1, 2]);
        model.expand_to(&Some(1000));
        assert_eq!(to_array(&list), [0, 10, 100, 1000, 11, 12, 1, 2]);

        disposable.dispose();
    }

    #[test]
    fn issue_95641() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let list: List<_> = Rc::default();
            let predicate: Predicate = Rc::new(RefCell::new(Box::new(|_| true)));

            struct Filter(Predicate);

            impl ITreeFilter<String> for Filter {
                fn filter(&self, element: &String, parent_visibility: TreeVisibility) -> TreeFilterResult<()> {
                    if element == "file" {
                        return TreeFilterResult::Visibility(TreeVisibility::Recurse);
                    }

                    TreeFilterResult::Visibility(if (self.0.borrow())(element) { TreeVisibility::Visible } else { parent_visibility })
                }
            }

            let model = ObjectTreeModel::<String>::new("test", ObjectTreeModelOptions { filter: Some(Rc::new(Filter(predicate.clone()))), ..Default::default() });
            let disposable = bind_list_to_model(&list, &model);

            model.set_children(&None, vec![str_with_children("file", vec![str_el("hello")])], Default::default());
            assert_eq!(to_array(&list), ["file", "hello"]);

            *predicate.borrow_mut() = Box::new(|el| el == "world");
            model.refilter();
            assert_eq!(to_array(&list), Vec::<String>::new());

            model.set_children(&s("file"), vec![str_el("world")], Default::default());
            timeout(0).await.unwrap(); // wait for refilter microtask
            assert_eq!(to_array(&list), ["file", "world"]);

            model.set_children(&s("file"), vec![str_el("hello")], Default::default());
            timeout(0).await.unwrap(); // wait for refilter microtask
            assert_eq!(to_array(&list), Vec::<String>::new());

            model.set_children(&s("file"), vec![str_el("world")], Default::default());
            timeout(0).await.unwrap(); // wait for refilter microtask
            assert_eq!(to_array(&list), ["file", "world"]);

            disposable.dispose();
        });
    }
}
