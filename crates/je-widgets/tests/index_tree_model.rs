//! Porte de `src/vs/base/test/browser/ui/tree/indexTreeModel.test.ts` (suíte `IndexTreeModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `number` é `f64` e `string` é `String`; `{ element, children, collapsed }` é `ITreeElement`.
//! - O filtro é uma classe com `query.test(element)` sobre `RegExp`s que são todas literais (`/build/`,
//!   `/gulp/`, `/b/`, `/platinum/`, a vazia): `str::contains` é exatamente o mesmo teste.
//! - `Math.random()` do teste `smart diff consistency` é um gerador congruencial com semente fixa (500
//!   rodadas, o mesmo roteiro de edições aleatórias).
//! - `toElements` devolve um [`Elements`]: folha (`number`) ou nó (`{ e, children }`).

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable};
use je_widgets::list::list_types::IIdentityProvider;
use je_widgets::tree::index_tree_model::{IndexTreeModel, IndexTreeModelOptions, IndexTreeModelSpliceOptions, TreeNode};
use je_widgets::tree::tree_types::{ITreeElement, ITreeFilter, TreeFilterResult, TreeVisibility};
use support::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};

type List<T> = Rc<RefCell<Vec<Rc<TreeNode<T>>>>>;

/// `bindListToModel`.
fn bind_list_to_model<T: Clone + 'static>(list: &List<T>, model: &Rc<IndexTreeModel<T>>) -> DisposableRef {
    let list = list.clone();
    model.on_did_splice_rendered_nodes().listen(move |e| {
        let mut list = list.borrow_mut();
        let end = (e.start + e.delete_count).min(list.len());
        list.splice(e.start..end, e.elements.iter().cloned());
    })
}

/// `toArray`.
fn to_array<T: Clone + 'static>(list: &List<T>) -> Vec<T> {
    list.borrow().iter().map(|i| i.element.clone()).collect()
}

/// O que `toElements` devolve: `node.element` (folha) ou `{ e, children }`.
#[derive(Debug, PartialEq)]
enum Elements {
    Leaf(f64),
    Node(f64, Vec<Elements>),
}

fn to_elements(node: &Rc<TreeNode<f64>>) -> Elements {
    let children = node.children.borrow();
    if children.is_empty() { Elements::Leaf(node.element) } else { Elements::Node(node.element, children.iter().map(to_elements).collect()) }
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct FnFilter<T>(Box<dyn Fn(&T, TreeVisibility) -> TreeFilterResult<()>>);

impl<T> ITreeFilter<T> for FnFilter<T> {
    fn filter(&self, element: &T, parent_visibility: TreeVisibility) -> TreeFilterResult<()> {
        (self.0)(element, parent_visibility)
    }
}

/// `{ getId: (n: number) => String(n) }`.
struct NumberIdentity;

impl IIdentityProvider<f64> for NumberIdentity {
    fn get_id(&self, n: &f64) -> String {
        n.to_string()
    }
}

fn diff_identity_provider() -> Rc<dyn IIdentityProvider<f64>> {
    Rc::new(NumberIdentity)
}

/// `withSmartSplice`: roda o teste duas vezes, com opções vazias e com `diffIdentityProvider`.
fn with_smart_splice(f: impl Fn(&IndexTreeModelSpliceOptions<f64, ()>)) {
    f(&IndexTreeModelSpliceOptions::default());
    f(&IndexTreeModelSpliceOptions { diff_identity_provider: Some(diff_identity_provider()), ..Default::default() });
}

mod index_tree_model {
    use super::*;
    #[test]
    fn ctor() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<f64> = Rc::default();
        let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
        let _ = &model;
        assert_eq!(list.borrow().len(), 0);
    }
    #[test]
    fn insert() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], options);
            assert_eq!(list.borrow().len(), 3);
            assert_eq!(list.borrow()[0].element, 0.0);
            assert_eq!(list.borrow()[0].collapsed.get(), false);
            assert_eq!(list.borrow()[0].depth, 1);
            assert_eq!(list.borrow()[1].element, 1.0);
            assert_eq!(list.borrow()[1].collapsed.get(), false);
            assert_eq!(list.borrow()[1].depth, 1);
            assert_eq!(list.borrow()[2].element, 2.0);
            assert_eq!(list.borrow()[2].collapsed.get(), false);
            assert_eq!(list.borrow()[2].depth, 1);
            disposable.dispose();
        });
    }
    #[test]
    fn deep_insert() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 11.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 12.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], &Default::default());
            assert_eq!(list.borrow().len(), 6);
            assert_eq!(list.borrow()[0].element, 0.0);
            assert_eq!(list.borrow()[0].collapsed.get(), false);
            assert_eq!(list.borrow()[0].depth, 1);
            assert_eq!(list.borrow()[1].element, 10.0);
            assert_eq!(list.borrow()[1].collapsed.get(), false);
            assert_eq!(list.borrow()[1].depth, 2);
            assert_eq!(list.borrow()[2].element, 11.0);
            assert_eq!(list.borrow()[2].collapsed.get(), false);
            assert_eq!(list.borrow()[2].depth, 2);
            assert_eq!(list.borrow()[3].element, 12.0);
            assert_eq!(list.borrow()[3].collapsed.get(), false);
            assert_eq!(list.borrow()[3].depth, 2);
            assert_eq!(list.borrow()[4].element, 1.0);
            assert_eq!(list.borrow()[4].collapsed.get(), false);
            assert_eq!(list.borrow()[4].depth, 1);
            assert_eq!(list.borrow()[5].element, 2.0);
            assert_eq!(list.borrow()[5].collapsed.get(), false);
            assert_eq!(list.borrow()[5].depth, 1);
            disposable.dispose();
        });
    }
    #[test]
    fn deep_insert_collapsed() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 11.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 12.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: Some(true) }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], options);
            assert_eq!(list.borrow().len(), 3);
            assert_eq!(list.borrow()[0].element, 0.0);
            assert_eq!(list.borrow()[0].collapsed.get(), true);
            assert_eq!(list.borrow()[0].depth, 1);
            assert_eq!(list.borrow()[1].element, 1.0);
            assert_eq!(list.borrow()[1].collapsed.get(), false);
            assert_eq!(list.borrow()[1].depth, 1);
            assert_eq!(list.borrow()[2].element, 2.0);
            assert_eq!(list.borrow()[2].collapsed.get(), false);
            assert_eq!(list.borrow()[2].depth, 1);
            disposable.dispose();
        });
    }
    #[test]
    fn delete() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], options);
            assert_eq!(list.borrow().len(), 3);
            model.splice(&[1], 1, vec![], options);
            assert_eq!(list.borrow().len(), 2);
            assert_eq!(list.borrow()[0].element, 0.0);
            assert_eq!(list.borrow()[0].collapsed.get(), false);
            assert_eq!(list.borrow()[0].depth, 1);
            assert_eq!(list.borrow()[1].element, 2.0);
            assert_eq!(list.borrow()[1].collapsed.get(), false);
            assert_eq!(list.borrow()[1].depth, 1);
            model.splice(&[0], 2, vec![], options);
            assert_eq!(list.borrow().len(), 0);
            disposable.dispose();
        });
    }
    #[test]
    fn nested_delete() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 11.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 12.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], options);
            assert_eq!(list.borrow().len(), 6);
            model.splice(&[1], 2, vec![], options);
            assert_eq!(list.borrow().len(), 4);
            assert_eq!(list.borrow()[0].element, 0.0);
            assert_eq!(list.borrow()[0].collapsed.get(), false);
            assert_eq!(list.borrow()[0].depth, 1);
            assert_eq!(list.borrow()[1].element, 10.0);
            assert_eq!(list.borrow()[1].collapsed.get(), false);
            assert_eq!(list.borrow()[1].depth, 2);
            assert_eq!(list.borrow()[2].element, 11.0);
            assert_eq!(list.borrow()[2].collapsed.get(), false);
            assert_eq!(list.borrow()[2].depth, 2);
            assert_eq!(list.borrow()[3].element, 12.0);
            assert_eq!(list.borrow()[3].collapsed.get(), false);
            assert_eq!(list.borrow()[3].depth, 2);
            disposable.dispose();
        });
    }
    #[test]
    fn deep_delete() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 11.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 12.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], options);
            assert_eq!(list.borrow().len(), 6);
            model.splice(&[0], 1, vec![], options);
            assert_eq!(list.borrow().len(), 2);
            assert_eq!(list.borrow()[0].element, 1.0);
            assert_eq!(list.borrow()[0].collapsed.get(), false);
            assert_eq!(list.borrow()[0].depth, 1);
            assert_eq!(list.borrow()[1].element, 2.0);
            assert_eq!(list.borrow()[1].collapsed.get(), false);
            assert_eq!(list.borrow()[1].depth, 1);
            disposable.dispose();
        });
    }
    #[test]
    fn smart_splice_deep() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<f64> = Rc::default();
        let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
        let disposable = bind_list_to_model(&list, &model);

        model.splice(&[0], 0, vec![ITreeElement::new(0.0), ITreeElement::new(1.0), ITreeElement::new(2.0), ITreeElement::new(3.0)], &IndexTreeModelSpliceOptions { diff_identity_provider: Some(diff_identity_provider()), ..Default::default() });

        let depth_one = |list: &List<f64>| list.borrow().iter().filter(|l| l.depth == 1).map(to_elements).collect::<Vec<_>>();
        assert_eq!(depth_one(&list), vec![Elements::Leaf(0.0), Elements::Leaf(1.0), Elements::Leaf(2.0), Elements::Leaf(3.0)]);

        model.splice(
            &[0],
            3,
            vec![
                ITreeElement::new(-0.5),
                ITreeElement::with_children(0.0, vec![ITreeElement::new(0.1)]),
                ITreeElement::new(1.0),
                ITreeElement::with_children(2.0, vec![ITreeElement::new(2.1), ITreeElement::with_children(2.2, vec![ITreeElement::new(2.21)])]),
            ],
            &IndexTreeModelSpliceOptions { diff_identity_provider: Some(diff_identity_provider()), diff_depth: Some(usize::MAX >> 11), ..Default::default() },
        );

        assert_eq!(
            depth_one(&list),
            vec![
                Elements::Leaf(-0.5),
                Elements::Node(0.0, vec![Elements::Leaf(0.1)]),
                Elements::Leaf(1.0),
                Elements::Node(2.0, vec![Elements::Leaf(2.1), Elements::Node(2.2, vec![Elements::Leaf(2.21)])]),
                Elements::Leaf(3.0),
            ]
        );

        disposable.dispose();
    }
    #[test]
    fn hidden_delete() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 11.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 12.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: Some(true) }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], options);
            assert_eq!(list.borrow().len(), 3);
            model.splice(&[0, 1], 1, vec![], options);
            assert_eq!(list.borrow().len(), 3);
            model.splice(&[0, 0], 2, vec![], options);
            assert_eq!(list.borrow().len(), 3);
            disposable.dispose();
        });
    }
    #[test]
    fn collapse() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 11.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 12.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], options);
            assert_eq!(list.borrow().len(), 6);
            model.set_collapsed(&[0], Some(true), None);
            assert_eq!(list.borrow().len(), 3);
            assert_eq!(list.borrow()[0].element, 0.0);
            assert_eq!(list.borrow()[0].collapsed.get(), true);
            assert_eq!(list.borrow()[0].depth, 1);
            assert_eq!(list.borrow()[1].element, 1.0);
            assert_eq!(list.borrow()[1].collapsed.get(), false);
            assert_eq!(list.borrow()[1].depth, 1);
            assert_eq!(list.borrow()[2].element, 2.0);
            assert_eq!(list.borrow()[2].collapsed.get(), false);
            assert_eq!(list.borrow()[2].depth, 1);
            disposable.dispose();
        });
    }
    #[test]
    fn expand() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let _ = options;
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 11.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 12.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: Some(true) }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], options);
            assert_eq!(list.borrow().len(), 3);
            model.expand_to(&[0, 1]);
            assert_eq!(list.borrow().len(), 6);
            assert_eq!(list.borrow()[0].element, 0.0);
            assert_eq!(list.borrow()[0].collapsed.get(), false);
            assert_eq!(list.borrow()[0].depth, 1);
            assert_eq!(list.borrow()[1].element, 10.0);
            assert_eq!(list.borrow()[1].collapsed.get(), false);
            assert_eq!(list.borrow()[1].depth, 2);
            assert_eq!(list.borrow()[2].element, 11.0);
            assert_eq!(list.borrow()[2].collapsed.get(), false);
            assert_eq!(list.borrow()[2].depth, 2);
            assert_eq!(list.borrow()[3].element, 12.0);
            assert_eq!(list.borrow()[3].collapsed.get(), false);
            assert_eq!(list.borrow()[3].depth, 2);
            assert_eq!(list.borrow()[4].element, 1.0);
            assert_eq!(list.borrow()[4].collapsed.get(), false);
            assert_eq!(list.borrow()[4].depth, 1);
            assert_eq!(list.borrow()[5].element, 2.0);
            assert_eq!(list.borrow()[5].collapsed.get(), false);
            assert_eq!(list.borrow()[5].depth, 1);
            disposable.dispose();
        });
    }
    #[test]
    fn smart_diff_consistency() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let times = 500;
        let min_edits = 1.0;
        let max_edits = 10.0;
        let max_inserts = 5.0;
        // `Math.random()` do original: um gerador determinístico, para o teste ser reproduzível.
        let mut rng = Lcg(0x2545_F491_4F6C_DD1D);

        for _ in 0..times {
            let list: List<f64> = Rc::default();
            let options = IndexTreeModelSpliceOptions { diff_identity_provider: Some(diff_identity_provider()), ..Default::default() };
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);

            let mut changes: Vec<String> = Vec::new();
            let mut expected: Vec<f64> = Vec::new();
            let mut element_counter = 0.0;

            let mut edits = rng.next() * (max_edits - min_edits) + min_edits;
            while edits > 0.0 {
                let len = list.borrow().len() as f64;
                let splice_index = (rng.next() * len).floor() as usize;
                let delete_count = (rng.next() * (len - splice_index as f64)).ceil() as usize;
                let insert_count = (rng.next() * max_inserts + 1.0).floor() as usize;

                let mut inserts: Vec<ITreeElement<f64>> = Vec::new();
                for _ in 0..insert_count {
                    let element = element_counter;
                    element_counter += 1.0;
                    inserts.push(ITreeElement { element, children: Some(vec![]), collapsible: None, collapsed: None });
                }

                // move existing items
                if rng.next() < 0.5 {
                    let elements: Vec<f64> = list.borrow().iter().skip(splice_index).take(delete_count / 2).map(|n| n.element).collect();
                    inserts.extend(elements.into_iter().map(|element| ITreeElement { element, children: Some(vec![]), collapsible: None, collapsed: None }));
                }

                let inserted_elements: Vec<f64> = inserts.iter().map(|i| i.element).collect();
                model.splice(&[splice_index], delete_count, inserts, &options);
                let end = (splice_index + delete_count).min(expected.len());
                expected.splice(splice_index..end, inserted_elements.iter().copied());

                let list_elements = to_array(&list);
                changes.push(format!("splice({splice_index}, {delete_count}, {inserted_elements:?}) -> {list_elements:?}"));

                assert_eq!(expected, list_elements, "Expected {list_elements:?} to equal {expected:?}. Steps:\n\n{}", changes.join("\n"));
                edits -= 1.0;
            }

            disposable.dispose();
        }
    }
    #[test]
    fn collapse_should_recursively_adjust_visible_count() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<f64> = Rc::default();
        let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: 1.0, children: Some(vec![ITreeElement { element: 11.0, children: Some(vec![ITreeElement { element: 111.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: Some(vec![ITreeElement { element: 21.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(list.borrow().len(), 5);
        assert_eq!(to_array(&list), vec![1.0, 11.0, 111.0, 2.0, 21.0]);
        model.set_collapsed(&[0, 0], Some(true), None);
        assert_eq!(list.borrow().len(), 4);
        assert_eq!(to_array(&list), vec![1.0, 11.0, 2.0, 21.0]);
        model.set_collapsed(&[1], Some(true), None);
        assert_eq!(list.borrow().len(), 3);
        assert_eq!(to_array(&list), vec![1.0, 11.0, 2.0]);
        disposable.dispose();
    }
    #[test]
    fn set_collapsible() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<f64> = Rc::default();
        let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(list.borrow().len(), 2);
        model.set_collapsible(&[0], Some(false));
        assert_eq!(list.borrow().len(), 2);
        assert_eq!(list.borrow()[0].element, 0.0);
        assert_eq!(list.borrow()[0].collapsible.get(), false);
        assert_eq!(list.borrow()[0].collapsed.get(), false);
        assert_eq!(list.borrow()[1].element, 10.0);
        assert_eq!(list.borrow()[1].collapsible.get(), false);
        assert_eq!(list.borrow()[1].collapsed.get(), false);
        assert_eq!(model.set_collapsed(&[0], Some(true), None), false);
        assert_eq!(list.borrow()[0].element, 0.0);
        assert_eq!(list.borrow()[0].collapsible.get(), false);
        assert_eq!(list.borrow()[0].collapsed.get(), false);
        assert_eq!(list.borrow()[1].element, 10.0);
        assert_eq!(list.borrow()[1].collapsible.get(), false);
        assert_eq!(list.borrow()[1].collapsed.get(), false);
        assert_eq!(model.set_collapsed(&[0], Some(false), None), false);
        assert_eq!(list.borrow()[0].element, 0.0);
        assert_eq!(list.borrow()[0].collapsible.get(), false);
        assert_eq!(list.borrow()[0].collapsed.get(), false);
        assert_eq!(list.borrow()[1].element, 10.0);
        assert_eq!(list.borrow()[1].collapsible.get(), false);
        assert_eq!(list.borrow()[1].collapsed.get(), false);
        model.set_collapsible(&[0], Some(true));
        assert_eq!(list.borrow().len(), 2);
        assert_eq!(list.borrow()[0].element, 0.0);
        assert_eq!(list.borrow()[0].collapsible.get(), true);
        assert_eq!(list.borrow()[0].collapsed.get(), false);
        assert_eq!(list.borrow()[1].element, 10.0);
        assert_eq!(list.borrow()[1].collapsible.get(), false);
        assert_eq!(list.borrow()[1].collapsed.get(), false);
        assert_eq!(model.set_collapsed(&[0], Some(true), None), true);
        assert_eq!(list.borrow().len(), 1);
        assert_eq!(list.borrow()[0].element, 0.0);
        assert_eq!(list.borrow()[0].collapsible.get(), true);
        assert_eq!(list.borrow()[0].collapsed.get(), true);
        assert_eq!(model.set_collapsed(&[0], Some(false), None), true);
        assert_eq!(list.borrow()[0].element, 0.0);
        assert_eq!(list.borrow()[0].collapsible.get(), true);
        assert_eq!(list.borrow()[0].collapsed.get(), false);
        assert_eq!(list.borrow()[1].element, 10.0);
        assert_eq!(list.borrow()[1].collapsible.get(), false);
        assert_eq!(list.borrow()[1].collapsed.get(), false);
        disposable.dispose();
    }
    #[test]
    fn simple_filter() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<f64> = Rc::default();
        let filter: Rc<dyn ITreeFilter<f64>> = Rc::new(FnFilter(Box::new(move |element: &f64, _| TreeFilterResult::Visibility(if element % 2.0 == 0.0 { TreeVisibility::Visible } else { TreeVisibility::Hidden }))));
        let model = IndexTreeModel::<f64>::new("test", -1.0, IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 3.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 4.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 5.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 6.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 7.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(list.borrow().len(), 4);
        assert_eq!(to_array(&list), vec![0.0, 2.0, 4.0, 6.0]);
        model.set_collapsed(&[0], Some(true), None);
        assert_eq!(to_array(&list), vec![0.0]);
        model.set_collapsed(&[0], Some(false), None);
        assert_eq!(to_array(&list), vec![0.0, 2.0, 4.0, 6.0]);
        disposable.dispose();
    }
    #[test]
    fn recursive_filter_on_initial_model() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<f64> = Rc::default();
        let filter: Rc<dyn ITreeFilter<f64>> = Rc::new(FnFilter(Box::new(move |element: &f64, _| TreeFilterResult::Visibility(if *element == 0.0 { TreeVisibility::Recurse } else { TreeVisibility::Hidden }))));
        let model = IndexTreeModel::<f64>::new("test", -1.0, IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(to_array(&list), Vec::<f64>::new());
        disposable.dispose();
    }
    #[test]
    fn refilter() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<f64> = Rc::default();
        let should_filter = Rc::new(Cell::new(false));
        let filter: Rc<dyn ITreeFilter<f64>> = Rc::new(FnFilter(Box::new({let should_filter = should_filter.clone(); move |element: &f64, _| TreeFilterResult::Visibility(if !should_filter.get() || element % 2.0 == 0.0 { TreeVisibility::Visible } else { TreeVisibility::Hidden })})));
        let model = IndexTreeModel::<f64>::new("test", -1.0, IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 3.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 4.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 5.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 6.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 7.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(to_array(&list), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        model.refilter();
        assert_eq!(to_array(&list), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        should_filter.set(true);
        model.refilter();
        assert_eq!(to_array(&list), vec![0.0, 2.0, 4.0, 6.0]);
        should_filter.set(false);
        model.refilter();
        assert_eq!(to_array(&list), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        disposable.dispose();
    }
    #[test]
    fn recursive_filter() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<String> = Rc::default();
        let query = Rc::new(RefCell::new(String::from("")));
        let filter: Rc<dyn ITreeFilter<String>> = Rc::new(FnFilter(Box::new({let query = query.clone(); move |element: &String, _| TreeFilterResult::Visibility(if element.contains(query.borrow().as_str()) { TreeVisibility::Visible } else { TreeVisibility::Recurse })})));
        let model = IndexTreeModel::<String>::new("test", "root".to_owned(), IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: "vscode".to_owned(), children: Some(vec![ITreeElement { element: ".build".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "git".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "github".to_owned(), children: Some(vec![ITreeElement { element: "calendar.yml".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "endgame".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "build.js".to_owned(), children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: "build".to_owned(), children: Some(vec![ITreeElement { element: "lib".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "gulpfile.js".to_owned(), children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(list.borrow().len(), 10);
        *query.borrow_mut() = String::from("build");
        model.refilter();
        assert_eq!(to_array(&list), vec!["vscode".to_owned(), ".build".to_owned(), "github".to_owned(), "build.js".to_owned(), "build".to_owned()]);
        model.set_collapsed(&[0], Some(true), None);
        assert_eq!(to_array(&list), vec!["vscode".to_owned()]);
        model.set_collapsed(&[0], Some(false), None);
        assert_eq!(to_array(&list), vec!["vscode".to_owned(), ".build".to_owned(), "github".to_owned(), "build.js".to_owned(), "build".to_owned()]);
        disposable.dispose();
    }
    #[test]
    fn recursive_filter_updates_when_children_change_133272() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let list: List<String> = Rc::default();
            let query = Rc::new(RefCell::new(String::new()));
            let filter: Rc<dyn ITreeFilter<String>> = Rc::new(FnFilter(Box::new({
                let query = query.clone();
                move |element: &String, _| TreeFilterResult::Visibility(if element.contains(query.borrow().as_str()) { TreeVisibility::Visible } else { TreeVisibility::Recurse })
            })));

            let model = IndexTreeModel::<String>::new("test", "root".to_owned(), IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
            let disposable = bind_list_to_model(&list, &model);

            model.splice(&[0], 0, vec![ITreeElement::with_children("a".to_owned(), vec![ITreeElement::new("b".to_owned())])], &Default::default());

            assert_eq!(to_array(&list), ["a", "b"]);
            *query.borrow_mut() = "visible".to_owned();
            model.refilter();
            assert_eq!(to_array(&list), Vec::<String>::new());

            model.splice(&[0, 0, 0], 0, vec![ITreeElement::with_children("visible".to_owned(), vec![])], &Default::default());

            timeout(0).await.unwrap(); // wait for refilter microtask

            assert_eq!(to_array(&list), ["a", "b", "visible"]);

            disposable.dispose();
            
        });
    }
    #[test]
    fn recursive_filter_with_collapse() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<String> = Rc::default();
        let query = Rc::new(RefCell::new(String::from("")));
        let filter: Rc<dyn ITreeFilter<String>> = Rc::new(FnFilter(Box::new({let query = query.clone(); move |element: &String, _| TreeFilterResult::Visibility(if element.contains(query.borrow().as_str()) { TreeVisibility::Visible } else { TreeVisibility::Recurse })})));
        let model = IndexTreeModel::<String>::new("test", "root".to_owned(), IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: "vscode".to_owned(), children: Some(vec![ITreeElement { element: ".build".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "git".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "github".to_owned(), children: Some(vec![ITreeElement { element: "calendar.yml".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "endgame".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "build.js".to_owned(), children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: "build".to_owned(), children: Some(vec![ITreeElement { element: "lib".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "gulpfile.js".to_owned(), children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(list.borrow().len(), 10);
        *query.borrow_mut() = String::from("gulp");
        model.refilter();
        assert_eq!(to_array(&list), vec!["vscode".to_owned(), "build".to_owned(), "gulpfile.js".to_owned()]);
        model.set_collapsed(&[0, 3], Some(true), None);
        assert_eq!(to_array(&list), vec!["vscode".to_owned(), "build".to_owned()]);
        model.set_collapsed(&[0], Some(true), None);
        assert_eq!(to_array(&list), vec!["vscode".to_owned()]);
        disposable.dispose();
    }
    #[test]
    fn recursive_filter_while_collapsed() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<String> = Rc::default();
        let query = Rc::new(RefCell::new(String::from("")));
        let filter: Rc<dyn ITreeFilter<String>> = Rc::new(FnFilter(Box::new({let query = query.clone(); move |element: &String, _| TreeFilterResult::Visibility(if element.contains(query.borrow().as_str()) { TreeVisibility::Visible } else { TreeVisibility::Recurse })})));
        let model = IndexTreeModel::<String>::new("test", "root".to_owned(), IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: "vscode".to_owned(), children: Some(vec![ITreeElement { element: ".build".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "git".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "github".to_owned(), children: Some(vec![ITreeElement { element: "calendar.yml".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "endgame".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "build.js".to_owned(), children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: "build".to_owned(), children: Some(vec![ITreeElement { element: "lib".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "gulpfile.js".to_owned(), children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }]), collapsible: None, collapsed: Some(true) }], &Default::default());
        assert_eq!(to_array(&list), vec!["vscode".to_owned()]);
        *query.borrow_mut() = String::from("gulp");
        model.refilter();
        assert_eq!(to_array(&list), vec!["vscode".to_owned()]);
        model.set_collapsed(&[0], Some(false), None);
        assert_eq!(to_array(&list), vec!["vscode".to_owned(), "build".to_owned(), "gulpfile.js".to_owned()]);
        model.set_collapsed(&[0], Some(true), None);
        assert_eq!(to_array(&list), vec!["vscode".to_owned()]);
        *query.borrow_mut() = String::from("");
        model.refilter();
        assert_eq!(to_array(&list), vec!["vscode".to_owned()]);
        model.set_collapsed(&[0], Some(false), None);
        assert_eq!(list.borrow().len(), 10);
        disposable.dispose();
    }
    mod get_node_location {
        use super::*;
        #[test]
        fn simple() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let list: List<f64> = Rc::default();
            let model = IndexTreeModel::<f64>::new("test", -1.0, Default::default());
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 10.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 11.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 12.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }], &Default::default());
            assert_eq!(model.get_node_location(&list.borrow()[0]), vec![0]);
            assert_eq!(model.get_node_location(&list.borrow()[1]), vec![0, 0]);
            assert_eq!(model.get_node_location(&list.borrow()[2]), vec![0, 1]);
            assert_eq!(model.get_node_location(&list.borrow()[3]), vec![0, 2]);
            assert_eq!(model.get_node_location(&list.borrow()[4]), vec![1]);
            assert_eq!(model.get_node_location(&list.borrow()[5]), vec![2]);
            disposable.dispose();
        }
        #[test]
        fn with_filter() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let list: List<f64> = Rc::default();
            let filter: Rc<dyn ITreeFilter<f64>> = Rc::new(FnFilter(Box::new(move |element: &f64, _| TreeFilterResult::Visibility(if element % 2.0 == 0.0 { TreeVisibility::Visible } else { TreeVisibility::Hidden }))));
            let model = IndexTreeModel::<f64>::new("test", -1.0, IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
            let disposable = bind_list_to_model(&list, &model);
            model.splice(&[0], 0, vec![ITreeElement { element: 0.0, children: Some(vec![ITreeElement { element: 1.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 2.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 3.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 4.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 5.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 6.0, children: None, collapsible: None, collapsed: None }, ITreeElement { element: 7.0, children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
            assert_eq!(model.get_node_location(&list.borrow()[0]), vec![0]);
            assert_eq!(model.get_node_location(&list.borrow()[1]), vec![0, 1]);
            assert_eq!(model.get_node_location(&list.borrow()[2]), vec![0, 3]);
            assert_eq!(model.get_node_location(&list.borrow()[3]), vec![0, 5]);
            disposable.dispose();
        }
    }
    #[test]
    fn refilter_with_filtered_out_nodes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<String> = Rc::default();
        let query = Rc::new(RefCell::new(String::from("")));
        let filter: Rc<dyn ITreeFilter<String>> = Rc::new(FnFilter(Box::new({let query = query.clone(); move |element: &String, _| TreeFilterResult::Bool(element.contains(query.borrow().as_str()))})));
        let model = IndexTreeModel::<String>::new("test", "root".to_owned(), IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: "silver".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "gold".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "platinum".to_owned(), children: None, collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(to_array(&list), vec!["silver".to_owned(), "gold".to_owned(), "platinum".to_owned()]);
        *query.borrow_mut() = String::from("platinum");
        model.refilter();
        assert_eq!(to_array(&list), vec!["platinum".to_owned()]);
        model.splice(&[0], usize::MAX, vec![ITreeElement { element: "silver".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "gold".to_owned(), children: None, collapsible: None, collapsed: None }, ITreeElement { element: "platinum".to_owned(), children: None, collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(to_array(&list), vec!["platinum".to_owned()]);
        model.refilter();
        assert_eq!(to_array(&list), vec!["platinum".to_owned()]);
        disposable.dispose();
    }
    #[test]
    fn explicit_hidden_nodes_should_have_render_node_count_0_issue_83211() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let list: List<String> = Rc::default();
        let query = Rc::new(RefCell::new(String::from("")));
        let filter: Rc<dyn ITreeFilter<String>> = Rc::new(FnFilter(Box::new({let query = query.clone(); move |element: &String, _| TreeFilterResult::Bool(element.contains(query.borrow().as_str()))})));
        let model = IndexTreeModel::<String>::new("test", "root".to_owned(), IndexTreeModelOptions { filter: Some(filter.clone()), ..Default::default() });
        let disposable = bind_list_to_model(&list, &model);
        model.splice(&[0], 0, vec![ITreeElement { element: "a".to_owned(), children: Some(vec![ITreeElement { element: "aa".to_owned(), children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }, ITreeElement { element: "b".to_owned(), children: Some(vec![ITreeElement { element: "bb".to_owned(), children: None, collapsible: None, collapsed: None }]), collapsible: None, collapsed: None }], &Default::default());
        assert_eq!(to_array(&list), vec!["a".to_owned(), "aa".to_owned(), "b".to_owned(), "bb".to_owned()]);
        assert_eq!(model.get_list_index(&[0]), 0);
        assert_eq!(model.get_list_index(&[0, 0]), 1);
        assert_eq!(model.get_list_index(&[1]), 2);
        assert_eq!(model.get_list_index(&[1, 0]), 3);
        *query.borrow_mut() = String::from("b");
        model.refilter();
        assert_eq!(to_array(&list), vec!["b".to_owned(), "bb".to_owned()]);
        assert_eq!(model.get_list_index(&[0]), -1);
        assert_eq!(model.get_list_index(&[0, 0]), -1);
        assert_eq!(model.get_list_index(&[1]), 0);
        assert_eq!(model.get_list_index(&[1, 0]), 1);
        disposable.dispose();
    }
}

