//! Porte de `src/vs/base/test/browser/ui/tree/objectTree.test.ts` (suítes `ObjectTree`, `TreeNavigator` e `CompressibleObjectTree`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `number` é `i64`; `{ element }` é `IObjectTreeElement`; o `null` (a raiz) é `None`, e o `node.element` que
//!   o renderer recebe é `Option<i64>` (ver `object_tree.rs`).
//! - `container.style.width/height` é `create_container` + `append_child` na raiz da `Ui`; `textContent` é o
//!   `label` do nó; `querySelectorAll('.monaco-list-row')` ordenado por `data-index` é a consulta por classe
//!   ordenada pelo topo das linhas (as linhas são posicionadas pelo índice).
//! - `clickElement` e `dispatchKeydown` despacham `PointerDown` e `Click` (ou `KeyDown`) no nó da linha.
//! - `new Proxy`, `dataset` e `localize` não existem: o contador de `renderTwistie` por nó do teste
//!   `disposing an older render preserves the current node mapping` fica no renderer do teste.
//! - Pendente: `updateOptions preserves wrapped accessibility provider for type navigation re-announce`
//!   confere (pelo `assert` dentro do provedor de acessibilidade) que o `getAriaLabel` recebe o elemento e não
//!   o nó, durante o reanúncio do tipo-para-navegar; nem o tipo-para-navegar nem `aria-label` existem na
//!   `List` do `je-widgets` ainda, então o teste roda os mesmos passos (atualizar as opções, teclar `a`,
//!   esperar 800 ms) e não tem o que conferir.
//! - Os 4 casos de sticky scroll estão em `mod sticky_scroll`, na posição do original (a tradução deles está
//!   no `//!` do bloco: `style.top`/`style.height` são os limites do nó, `requestAnimationFrame` é
//!   `ui.run_animation_frame()`, e a identidade de nós do DOM são `NodeId`).

mod support;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{IDisposable, none};
use je_base::map::SetMap;
use je_ui::{Event, EventKind, KeyData, Modifiers, NodeId, PointerButton, PointerData, Ui};
use je_widgets::list::list_types::{GroupId, IIdentityProvider, IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use je_widgets::tree::abstract_tree::{AbstractTreeOptions, AbstractTreeOptionsUpdate};
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeMethods, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelOptions, ObjectTreeModelSetChildrenOptions};
use je_widgets::tree::tree_renderer::{Collection, TreeRenderer, tree_template_data};
use je_widgets::tree::tree_types::{IObjectTreeElement, ITreeFilter, ITreeNavigator, ITreeRenderer, TreeElementRenderDetails, TreeFilterResult, TreeVisibility};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, query_all, run_with_faked_timers, timeout};

type Model = ObjectTreeModel<i64>;

fn el(element: i64) -> IObjectTreeElement<i64> {
    IObjectTreeElement::new(element)
}

fn node(element: i64, children: Vec<IObjectTreeElement<i64>>) -> IObjectTreeElement<i64> {
    IObjectTreeElement::with_children(element, children)
}

fn set_children(tree: &ObjectTree<i64>, children: Vec<IObjectTreeElement<i64>>) {
    tree.set_children(&None, children, ObjectTreeModelSetChildrenOptions::default());
}

/// `Delegate`: `getHeight() { return 20; }`.
struct Delegate;

impl IListVirtualDelegate<i64> for Delegate {
    fn get_height(&self, _: &i64) -> f64 {
        20.0
    }

    fn get_template_id(&self, _: &i64) -> String {
        "default".to_owned()
    }
}

/// `Renderer`: `renderTemplate(container) { return container; }` e `textContent = element`.
struct Renderer {
    ui: Ui,
    row_class_name: Option<String>,
}

impl Renderer {
    fn new(ui: &Ui) -> Self {
        Self { ui: ui.clone(), row_class_name: None }
    }
}

impl ITreeRenderer<Model> for Renderer {
    fn template_id(&self) -> String {
        "default".to_owned()
    }

    fn row_class_name(&self) -> Option<String> {
        self.row_class_name.clone()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(container) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<je_widgets::tree::index_tree_model::TreeNode<Option<i64>>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let container = *template_data.downcast_ref::<NodeId>().expect("template");
        self.ui.tree_mut().set_label(container, format!("{}", node.element.expect("elemento")));
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

/// `getRowsTextContent`.
fn get_rows_text_content(ui: &Ui, container: NodeId) -> Vec<String> {
    let tree = ui.tree();
    let mut rows = query_all(&tree, container, &["monaco-list-row"]);
    rows.sort_by(|a, b| tree.bounds(*a).y0.total_cmp(&tree.bounds(*b).y0));
    rows.into_iter().map(|row| query_all(&tree, row, &["monaco-tl-contents"]).first().and_then(|c| tree.label(*c)).unwrap_or_default().to_owned()).collect()
}

/// `clickElement`.
fn click_element(ui: &Ui, element: NodeId, ctrl_key: bool) {
    let pointer = PointerData { x: 0.0, y: 0.0, button: PointerButton::Primary, modifiers: Modifiers { ctrl: ctrl_key, ..Modifiers::default() }, click_count: 1 };
    ui.dispatch(&mut Event::new(EventKind::PointerDown, element).with_pointer(pointer));
    ui.dispatch(&mut Event::new(EventKind::Click, element).with_pointer(pointer));
}

/// `dispatchKeydown`.
fn dispatch_keydown(ui: &Ui, element: NodeId, key_code: KeyCode) {
    ui.dispatch(&mut Event::new(EventKind::KeyDown, element).with_key(KeyData { key_code, modifiers: Modifiers::default() }));
}

fn attach(ui: &Ui, width: f64, height: f64) -> NodeId {
    let container = create_container(ui, width, height);
    let root = ui.root();
    ui.tree_mut().append_child(root, container);
    container
}

fn flat(value: Option<Option<i64>>) -> Option<i64> {
    value.flatten()
}

mod object_tree {
    use super::*;

    mod tree_navigator {
        use super::*;

        type SharedPredicate = Rc<RefCell<Box<dyn Fn(i64) -> bool>>>;

        struct PredicateFilter(SharedPredicate);

        impl ITreeFilter<Option<i64>> for PredicateFilter {
            fn filter(&self, element: &Option<i64>, _: TreeVisibility) -> TreeFilterResult<()> {
                TreeFilterResult::Bool((self.0.borrow())(element.expect("elemento")))
            }
        }

        struct Fixture {
            tree: Rc<ObjectTree<i64>>,
            filter: SharedPredicate,
            _guard: support::LeakGuard,
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                self.tree.dispose();
                *self.filter.borrow_mut() = Box::new(|_| true);
            }
        }

        fn setup() -> Fixture {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let ui = Ui::new();
            let container = attach(&ui, 200.0, 200.0);
            let filter: SharedPredicate = Rc::new(RefCell::new(Box::new(|_| true)));
            let options = ObjectTreeOptions { tree: AbstractTreeOptions { filter: Some(Rc::new(PredicateFilter(filter.clone()))), ..AbstractTreeOptions::default() }, sorter: None };
            let tree = ObjectTree::new("test", &ui, container, Rc::new(Delegate), vec![Rc::new(Renderer::new(&ui))], options);
            tree.layout(Some(200.0), None);
            Fixture { tree, filter, _guard: guard }
        }

        fn sample_children() -> Vec<IObjectTreeElement<i64>> {
            vec![node(0, vec![el(10), el(11), el(12)]), el(1), el(2)]
        }

        #[test]
        fn should_be_able_to_navigate() {
            let f = setup();
            set_children(&f.tree, sample_children());

            let mut navigator = f.tree.navigate(None);

            assert_eq!(flat(navigator.current()), None);
            assert_eq!(flat(navigator.next()), Some(0));
            assert_eq!(flat(navigator.current()), Some(0));
            assert_eq!(flat(navigator.next()), Some(10));
            assert_eq!(flat(navigator.current()), Some(10));
            assert_eq!(flat(navigator.next()), Some(11));
            assert_eq!(flat(navigator.current()), Some(11));
            assert_eq!(flat(navigator.next()), Some(12));
            assert_eq!(flat(navigator.current()), Some(12));
            assert_eq!(flat(navigator.next()), Some(1));
            assert_eq!(flat(navigator.current()), Some(1));
            assert_eq!(flat(navigator.next()), Some(2));
            assert_eq!(flat(navigator.current()), Some(2));
            assert_eq!(flat(navigator.previous()), Some(1));
            assert_eq!(flat(navigator.current()), Some(1));
            assert_eq!(flat(navigator.previous()), Some(12));
            assert_eq!(flat(navigator.previous()), Some(11));
            assert_eq!(flat(navigator.previous()), Some(10));
            assert_eq!(flat(navigator.previous()), Some(0));
            assert_eq!(flat(navigator.previous()), None);
            assert_eq!(flat(navigator.next()), Some(0));
            assert_eq!(flat(navigator.next()), Some(10));
            assert_eq!(flat(navigator.first()), Some(0));
            assert_eq!(flat(navigator.last()), Some(2));
        }

        #[test]
        fn should_skip_collapsed_nodes() {
            let f = setup();
            set_children(&f.tree, vec![node(0, vec![el(10), el(11), el(12)]).collapsed(true), el(1), el(2)]);

            let mut navigator = f.tree.navigate(None);

            assert_eq!(flat(navigator.current()), None);
            assert_eq!(flat(navigator.next()), Some(0));
            assert_eq!(flat(navigator.next()), Some(1));
            assert_eq!(flat(navigator.next()), Some(2));
            assert_eq!(flat(navigator.next()), None);
            assert_eq!(flat(navigator.previous()), Some(2));
            assert_eq!(flat(navigator.previous()), Some(1));
            assert_eq!(flat(navigator.previous()), Some(0));
            assert_eq!(flat(navigator.previous()), None);
            assert_eq!(flat(navigator.next()), Some(0));
            assert_eq!(flat(navigator.first()), Some(0));
            assert_eq!(flat(navigator.last()), Some(2));
        }

        #[test]
        fn reports_the_flattened_visible_render_count() {
            let f = setup();
            set_children(
                &f.tree,
                vec![
                    node(0, vec![el(10), el(11)]).collapsible(true).collapsed(false),
                    node(1, vec![el(20)]).collapsible(true).collapsed(true),
                    el(2),
                ],
            );

            let expanded_root = f.tree.get_list_render_count(&None);
            let expanded_subtree = f.tree.get_list_render_count(&Some(0));
            f.tree.collapse(&Some(0), false);

            assert_eq!(
                (expanded_root, expanded_subtree, f.tree.get_list_render_count(&None), f.tree.get_list_render_count(&Some(0))),
                (5, 3, 3, 1)
            );
        }

        #[test]
        fn should_skip_filtered_elements() {
            let f = setup();
            *f.filter.borrow_mut() = Box::new(|el| el % 2 == 0);

            set_children(&f.tree, sample_children());

            let mut navigator = f.tree.navigate(None);

            assert_eq!(flat(navigator.current()), None);
            assert_eq!(flat(navigator.next()), Some(0));
            assert_eq!(flat(navigator.next()), Some(10));
            assert_eq!(flat(navigator.next()), Some(12));
            assert_eq!(flat(navigator.next()), Some(2));
            assert_eq!(flat(navigator.next()), None);
            assert_eq!(flat(navigator.previous()), Some(2));
            assert_eq!(flat(navigator.previous()), Some(12));
            assert_eq!(flat(navigator.previous()), Some(10));
            assert_eq!(flat(navigator.previous()), Some(0));
            assert_eq!(flat(navigator.previous()), None);
            assert_eq!(flat(navigator.next()), Some(0));
            assert_eq!(flat(navigator.next()), Some(10));
            assert_eq!(flat(navigator.first()), Some(0));
            assert_eq!(flat(navigator.last()), Some(2));
        }

        #[test]
        fn should_be_able_to_start_from_node() {
            let f = setup();
            set_children(&f.tree, sample_children());

            let mut navigator = f.tree.navigate(Some(&Some(1)));

            assert_eq!(flat(navigator.current()), Some(1));
            assert_eq!(flat(navigator.next()), Some(2));
            assert_eq!(flat(navigator.current()), Some(2));
            assert_eq!(flat(navigator.previous()), Some(1));
            assert_eq!(flat(navigator.current()), Some(1));
            assert_eq!(flat(navigator.previous()), Some(12));
            assert_eq!(flat(navigator.previous()), Some(11));
            assert_eq!(flat(navigator.previous()), Some(10));
            assert_eq!(flat(navigator.previous()), Some(0));
            assert_eq!(flat(navigator.previous()), None);
            assert_eq!(flat(navigator.next()), Some(0));
            assert_eq!(flat(navigator.next()), Some(10));
            assert_eq!(flat(navigator.first()), Some(0));
            assert_eq!(flat(navigator.last()), Some(2));
        }
    }

    #[test]
    fn applies_renderer_row_class_names() {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);
        let renderer = Renderer { ui: ui.clone(), row_class_name: Some("test-tree-row".to_owned()) };
        let tree = ObjectTree::<i64>::new("test", &ui, container, Rc::new(Delegate), vec![Rc::new(renderer)], ObjectTreeOptions::default());

        tree.layout(Some(200.0), None);
        set_children(&tree, vec![el(0), el(1)]);

        let count = {
            let t = ui.tree();
            query_all(&t, container, &["monaco-list-row"]).into_iter().filter(|r| t.has_class(*r, "test-tree-row")).count()
        };
        assert_eq!(count, 2);

        tree.dispose();
    }


    mod sticky_scroll {
        use super::*;
        use je_widgets::tree::sticky_scroll::StickyScrollNodeSourceRange;

        type Tree = ObjectTree<i64>;
        type RangeProvider = Rc<dyn Fn(&Option<i64>, StickyScrollNodeSourceRange) -> Option<StickyScrollNodeSourceRange>>;

        /// `{ element: 0, children: [{ element: 1 }, ..., { element: 6 }] }`.
        fn parent_with_six_children() -> IObjectTreeElement<i64> {
            IObjectTreeElement::with_children(0, (1..=6).map(IObjectTreeElement::new).collect())
        }

        fn sticky_options(provider: Option<RangeProvider>) -> ObjectTreeOptions<i64> {
            ObjectTreeOptions {
                tree: AbstractTreeOptions {
                    update: AbstractTreeOptionsUpdate { enable_sticky_scroll: Some(true), sticky_scroll_max_item_count: Some(1), ..Default::default() },
                    sticky_scroll_node_source_range_provider: provider,
                    ..Default::default()
                },
                ..Default::default()
            }
        }

        fn create_tree(ui: &Ui, container: NodeId, options: ObjectTreeOptions<i64>) -> Rc<Tree> {
            ObjectTree::new("test", ui, container, Rc::new(Delegate), vec![Rc::new(Renderer::new(ui))], options)
        }

        /// `document.createElement('div')` com 200x100, já anexado à raiz.
        fn create_attached_container(ui: &Ui) -> NodeId {
            let container = create_container(ui, 200.0, 100.0);
            let root = ui.root();
            ui.tree_mut().append_child(root, container);
            container
        }

        /// `container.querySelectorAll('.monaco-tree-sticky-row')`.
        fn sticky_rows(ui: &Ui, root: NodeId) -> Vec<NodeId> {
            query_all(&ui.tree(), root, &["monaco-tree-sticky-row"])
        }

        /// `container.querySelector('.monaco-tree-sticky-row')?.textContent`.
        fn sticky_text(ui: &Ui, container: NodeId) -> Option<String> {
            let tree = ui.tree();
            let row = *query_all(&tree, container, &["monaco-tree-sticky-row"]).first()?;
            let contents = *query_all(&tree, row, &["monaco-tl-contents"]).first()?;
            tree.label(contents).map(str::to_owned)
        }

        #[test]
        fn tracks_the_sticky_scroll_dom_node_across_runtime_toggles() {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            run_with_faked_timers(async move {
                let ui = Ui::new();
                let disabled_container = create_attached_container(&ui);
                let enabled_container = create_attached_container(&ui);

                let disabled_tree = guard.add_rc(create_tree(&ui, disabled_container, ObjectTreeOptions::default()));
                let enabled_tree = guard.add_rc(create_tree(&ui, enabled_container, sticky_options(None)));
                let sticky_scroll_dom_node_changes: Rc<RefCell<Vec<Option<NodeId>>>> = Rc::default();
                let changes = sticky_scroll_dom_node_changes.clone();
                guard.add(enabled_tree.on_did_change_sticky_scroll_dom_node().listen(move |node| changes.borrow_mut().push(*node)));

                disabled_tree.layout(Some(100.0), None);
                enabled_tree.layout(Some(100.0), None);
                enabled_tree.set_children(&None, vec![parent_with_six_children()], ObjectTreeModelSetChildrenOptions::default());

                let sticky_scroll_dom_node = enabled_tree.sticky_scroll_dom_node().expect("o sticky scroll está ligado");
                let sticky_rows_before_scroll = sticky_rows(&ui, sticky_scroll_dom_node).len();
                enabled_tree.set_scroll_top(1.0);
                let sticky_rows_after_scroll = sticky_rows(&ui, sticky_scroll_dom_node).len();
                let is_real_container = query_all(&ui.tree(), enabled_container, &["monaco-tree-sticky-container"]).first() == Some(&sticky_scroll_dom_node);
                let stable_before_toggle = enabled_tree.sticky_scroll_dom_node() == Some(sticky_scroll_dom_node);
                enabled_tree.update_options(&AbstractTreeOptionsUpdate { enable_sticky_scroll: Some(false), ..Default::default() });
                let sticky_scroll_dom_node_when_disabled = enabled_tree.sticky_scroll_dom_node();
                let old_dom_node_removed = !ui.tree().is_ancestor(enabled_container, sticky_scroll_dom_node);
                enabled_tree.update_options(&AbstractTreeOptionsUpdate { enable_sticky_scroll: Some(true), ..Default::default() });
                let replacement_sticky_scroll_dom_node = enabled_tree.sticky_scroll_dom_node().expect("o sticky scroll voltou a ligar");

                assert_eq!(disabled_tree.sticky_scroll_dom_node(), None);
                assert!(is_real_container);
                assert!(stable_before_toggle);
                assert_eq!(sticky_rows_before_scroll, 0);
                assert_eq!(sticky_rows_after_scroll, 1);
                assert_eq!(sticky_scroll_dom_node_when_disabled, None);
                assert!(old_dom_node_removed);
                assert_eq!(query_all(&ui.tree(), enabled_container, &["monaco-tree-sticky-container"]).first(), Some(&replacement_sticky_scroll_dom_node));
                assert_ne!(replacement_sticky_scroll_dom_node, sticky_scroll_dom_node);
                assert_eq!(sticky_rows(&ui, replacement_sticky_scroll_dom_node).len(), 1);
                let change_kinds: Vec<&str> = sticky_scroll_dom_node_changes.borrow().iter().map(|node| if node.is_some() { "enabled" } else { "disabled" }).collect();
                assert_eq!(change_kinds, ["disabled", "enabled"]);
            });
        }

        #[test]
        fn shows_the_default_sticky_node_after_its_source_row_starts_scrolling_out() {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            run_with_faked_timers(async move {
                let ui = Ui::new();
                let container = create_attached_container(&ui);

                let tree = guard.add_rc(create_tree(&ui, container, sticky_options(None)));
                tree.layout(Some(100.0), None);
                tree.set_children(&None, vec![parent_with_six_children()], ObjectTreeModelSetChildrenOptions::default());

                let mut states = vec![sticky_text(&ui, container)];
                tree.set_scroll_top(1.0);
                states.push(sticky_text(&ui, container));

                assert_eq!(states, [None, Some("0".to_owned())]);
            });
        }

        #[test]
        fn evaluates_a_custom_sticky_source_range_once_per_scroll_update() {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            run_with_faked_timers(async move {
                let ui = Ui::new();
                let container = create_attached_container(&ui);
                let provider_calls: Rc<RefCell<Vec<(i64, StickyScrollNodeSourceRange)>>> = Rc::default();
                let calls = provider_calls.clone();

                let tree = guard.add_rc(create_tree(
                    &ui,
                    container,
                    sticky_options(Some(Rc::new(move |element: &Option<i64>, default_range| {
                        calls.borrow_mut().push((element.expect("só a raiz tem elemento nulo"), default_range));
                        Some(StickyScrollNodeSourceRange { start: 5.0, end: 15.0, sticky_node_height: None, estimated: false })
                    }))),
                ));
                tree.layout(Some(100.0), None);
                tree.set_children(&None, vec![parent_with_six_children()], ObjectTreeModelSetChildrenOptions::default());

                tree.set_scroll_top(5.0);
                let sticky_at_range_start = sticky_text(&ui, container);
                tree.set_scroll_top(6.0);
                let sticky_after_range_start = sticky_text(&ui, container);

                assert_eq!(sticky_at_range_start, None);
                assert_eq!(sticky_after_range_start, Some("0".to_owned()));
                let default_range = StickyScrollNodeSourceRange { start: 0.0, end: 20.0, sticky_node_height: None, estimated: false };
                assert_eq!(*provider_calls.borrow(), [(0, default_range), (0, default_range)]);
            });
        }

        #[test]
        fn shrinks_a_sticky_node_to_its_final_pixel_before_the_next_root() {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            run_with_faked_timers(async move {
                let ui = Ui::new();
                let container = create_attached_container(&ui);

                let tree = guard.add_rc(create_tree(&ui, container, sticky_options(None)));
                tree.layout(Some(100.0), None);
                tree.set_children(
                    &None,
                    vec![
                        IObjectTreeElement::with_children(100, vec![IObjectTreeElement::new(1), IObjectTreeElement::new(10)]),
                        IObjectTreeElement::new(2),
                        IObjectTreeElement::new(3),
                        IObjectTreeElement::new(4),
                        IObjectTreeElement::new(5),
                        IObjectTreeElement::new(6),
                    ],
                    ObjectTreeModelSetChildrenOptions::default(),
                );

                tree.set_scroll_top(1.0);
                ui.run_animation_frame();
                tree.set_scroll_top(59.0);
                ui.run_animation_frame();
                let sticky_before_boundary = sticky_rows(&ui, container).first().copied();
                let bounds_before_boundary = sticky_before_boundary.map(|row| ui.tree().bounds(row));
                let position_before_boundary = bounds_before_boundary.map(|bounds| bounds.y0);
                let visible_height_before_boundary = bounds_before_boundary.map(|bounds| bounds.y0 + bounds.height());
                tree.set_scroll_top(60.0);
                let position_at_boundary = sticky_rows(&ui, container).first().map(|row| ui.tree().bounds(*row).y0);
                ui.run_animation_frame();
                let sticky_after_boundary = !sticky_rows(&ui, container).is_empty();

                assert_eq!(position_before_boundary, Some(-19.0));
                assert_eq!(visible_height_before_boundary, Some(1.0));
                assert_eq!(position_at_boundary, None);
                assert!(!sticky_after_boundary);
            });
        }
    }


    /// O `IdentityProvider` do original: `getId(element) { return `${element % 100}`; }`.
    struct ModIdentityProvider;

    impl IIdentityProvider<Option<i64>> for ModIdentityProvider {
        fn get_id(&self, element: &Option<i64>) -> String {
            format!("{}", element.map_or(-1, |e| e % 100))
        }
    }

    /// `disposing an older render preserves the current node mapping`.
    struct TwistieRenderer {
        on_did_change_twistie_state: Rc<Emitter<i64>>,
        render_counts: Rc<RefCell<HashMap<NodeId, u32>>>,
    }

    impl ITreeRenderer<Model> for TwistieRenderer {
        fn template_id(&self) -> String {
            "default".to_owned()
        }

        fn on_did_change_twistie_state(&self) -> Option<BaseEvent<Option<i64>>> {
            Some(self.on_did_change_twistie_state.event().map(|e| Some(*e), None))
        }

        fn render_template(&self, _container: NodeId) -> TemplateData {
            Rc::new(()) as Rc<dyn Any>
        }

        fn render_element(&self, _: &Rc<je_widgets::tree::index_tree_model::TreeNode<Option<i64>>>, _: usize, _: &TemplateData, _: Option<&TreeElementRenderDetails>) {}

        fn render_twistie(&self, _element: &Option<i64>, twistie: NodeId) -> bool {
            *self.render_counts.borrow_mut().entry(twistie).or_insert(0) += 1;
            true
        }

        fn dispose_template(&self, _: &TemplateData) {}
    }

    struct EmptyCollection;

    impl Collection<je_widgets::tree::index_tree_model::TreeNode<Option<i64>>> for EmptyCollection {
        fn elements(&self) -> Vec<Rc<je_widgets::tree::index_tree_model::TreeNode<Option<i64>>>> {
            Vec::new()
        }

        fn on_did_change(&self) -> BaseEvent<Vec<Rc<je_widgets::tree::index_tree_model::TreeNode<Option<i64>>>>> {
            BaseEvent::none()
        }
    }

    #[test]
    fn disposing_an_older_render_preserves_the_current_node_mapping() {
        let ui = Ui::new();
        let on_did_change_twistie_state = Emitter::<i64>::new();
        let render_counts: Rc<RefCell<HashMap<NodeId, u32>>> = Rc::default();
        let renderer = Rc::new(TwistieRenderer { on_did_change_twistie_state: on_did_change_twistie_state.clone(), render_counts: render_counts.clone() });
        let model = Rc::new(ObjectTreeModel::<i64>::new("test", ObjectTreeModelOptions::default()));
        model.set_children(&None, vec![el(1)], ObjectTreeModelSetChildrenOptions::default());

        let tree_renderer = TreeRenderer::new(
            &ui,
            renderer,
            model.clone(),
            je_widgets::tree::tree_types::ITreeModel::on_did_change_collapse_state(&*model),
            Rc::new(EmptyCollection),
            Rc::new(RefCell::new(SetMap::new())),
            &Default::default(),
        );

        let node = je_widgets::tree::tree_types::ITreeModel::get_node(&*model, &Some(1));
        let first_template = tree_renderer.render_template(create_container(&ui, 10.0, 10.0));
        let second_template = tree_renderer.render_template(create_container(&ui, 10.0, 10.0));
        let details = ListElementRenderDetails { height: Some(100.0), on_scroll: None };
        tree_renderer.render_element(&node, 0, &first_template, Some(&details));
        tree_renderer.render_element(&node, 0, &second_template, Some(&details));

        tree_renderer.dispose_element(&node, 0, &first_template, Some(&details));
        on_did_change_twistie_state.fire(1);

        let counts = render_counts.borrow();
        assert_eq!((counts[&tree_template_data(&first_template).twistie], counts[&tree_template_data(&second_template).twistie]), (1, 2));
        drop(counts);

        tree_renderer.dispose();
        on_did_change_twistie_state.dispose();
        let _ = none();
    }

    #[test]
    fn traits_are_preserved_according_to_string_identity() {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);
        let options = ObjectTreeOptions { tree: AbstractTreeOptions { identity_provider: Some(Rc::new(ModIdentityProvider)), ..AbstractTreeOptions::default() }, sorter: None };
        let tree = ObjectTree::<i64>::new("test", &ui, container, Rc::new(Delegate), vec![Rc::new(Renderer::new(&ui))], options);
        tree.layout(Some(200.0), None);

        set_children(&tree, vec![el(0), el(1), el(2), el(3)]);
        tree.set_focus(&[Some(1)], None);
        assert_eq!(tree.get_focus(), vec![Some(1)]);

        set_children(&tree, vec![el(100), el(101), el(102), el(103)]);
        assert_eq!(tree.get_focus(), vec![Some(101)]);
    }

    /// O provedor de `updateOptions preserves wrapped identity provider in view options`: `getId` é o próprio
    /// elemento e `getGroupId` é `element % 2`.
    struct GroupIdentityProvider;

    impl IIdentityProvider<Option<i64>> for GroupIdentityProvider {
        fn get_id(&self, element: &Option<i64>) -> String {
            format!("{}", element.unwrap_or(-1))
        }

        fn get_group_id(&self, element: &Option<i64>) -> Option<GroupId> {
            element.map(|e| GroupId::Group(e % 2))
        }
    }

    #[test]
    fn update_options_preserves_wrapped_identity_provider_in_view_options() {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);
        let options = ObjectTreeOptions { tree: AbstractTreeOptions { identity_provider: Some(Rc::new(GroupIdentityProvider)), ..AbstractTreeOptions::default() }, sorter: None };
        let tree = ObjectTree::<i64>::new("test", &ui, container, Rc::new(Delegate), vec![Rc::new(Renderer::new(&ui))], options);

        tree.layout(Some(200.0), None);
        set_children(&tree, vec![el(0), el(1), el(2), el(3)]);

        let rows = {
            let t = ui.tree();
            let mut rows = query_all(&t, container, &["monaco-list-row"]);
            rows.sort_by(|a, b| t.bounds(*a).y0.total_cmp(&t.bounds(*b).y0));
            rows
        };
        let (first_row, second_row) = (rows[0], rows[1]);
        click_element(&ui, first_row, false);
        assert_eq!(tree.get_selection(), vec![Some(0)]);

        tree.update_options(&AbstractTreeOptionsUpdate { indent: Some(12.0), ..AbstractTreeOptionsUpdate::default() });

        click_element(&ui, second_row, true);

        assert_eq!(tree.get_selection(), vec![Some(1)]);

        tree.dispose();
    }

    #[test]
    fn update_options_preserves_wrapped_accessibility_provider_for_type_navigation_re_announce() {
        run_with_faked_timers(async {
            let ui = Ui::new();
            let container = attach(&ui, 200.0, 200.0);
            let tree = ObjectTree::<i64>::new("test", &ui, container, Rc::new(Delegate), vec![Rc::new(Renderer::new(&ui))], ObjectTreeOptions::default());

            tree.layout(Some(200.0), None);
            set_children(&tree, vec![el(0)]);
            tree.set_focus(&[Some(0)], None);
            tree.dom_focus();

            tree.update_options(&AbstractTreeOptionsUpdate { indent: Some(12.0), ..AbstractTreeOptionsUpdate::default() });

            dispatch_keydown(&ui, tree.get_html_element(), KeyCode::KeyA);
            // Deixa o debounce de limpeza do tipo-para-navegar reanunciar o elemento antes de descartar.
            timeout(800).await.unwrap();
            tree.dispose();
        });
    }
}

mod compressible_object_tree {
    use super::*;
    use je_widgets::tree::compressed_object_tree_model::{CompressedRef, CompressibleObjectTreeModel, ICompressedTreeElement};
    use je_widgets::tree::compressible_object_tree::{CompressibleObjectTree, CompressibleObjectTreeOptions, CompressibleObjectTreeOptionsUpdate, ICompressibleTreeRenderer};
    use je_widgets::tree::index_tree_model::TreeNode;

    type CModel = CompressibleObjectTreeModel<i64>;

    /// `Renderer` da suíte: `textContent` é o elemento, ou os elementos juntos por `/` quando comprimido.
    struct CompressibleRenderer {
        ui: Ui,
    }

    impl ITreeRenderer<CModel> for CompressibleRenderer {
        fn template_id(&self) -> String {
            "default".to_owned()
        }

        fn render_template(&self, container: NodeId) -> TemplateData {
            Rc::new(container) as Rc<dyn Any>
        }

        fn render_element(&self, node: &Rc<<CModel as je_widgets::tree::tree_types::ITreeModel>::Node>, _: usize, template_data: &TemplateData, _: Option<&TreeElementRenderDetails>) {
            let container = *template_data.downcast_ref::<NodeId>().expect("template");
            self.ui.tree_mut().set_label(container, format!("{}", node.element().expect("elemento")));
        }

        fn dispose_template(&self, _: &TemplateData) {}
    }

    impl ICompressibleTreeRenderer<i64, ()> for CompressibleRenderer {
        fn render_compressed_elements(&self, node: &Rc<TreeNode<Option<CompressedRef<i64>>>>, _: usize, template_data: &TemplateData, _: Option<&TreeElementRenderDetails>) {
            let container = *template_data.downcast_ref::<NodeId>().expect("template");
            let elements: Vec<String> = node.element.as_ref().expect("nó comprimido").elements.iter().map(ToString::to_string).collect();
            self.ui.tree_mut().set_label(container, elements.join("/"));
        }
    }

    use je_widgets::tree::tree_types::TreeNodeApi;

    fn setup(guard: &support::LeakGuard) -> (Ui, NodeId, Rc<CompressibleObjectTree<i64>>) {
        let ui = Ui::new();
        let container = attach(&ui, 200.0, 200.0);
        let tree = CompressibleObjectTree::<i64>::new("test", &ui, container, Rc::new(Delegate), vec![Rc::new(CompressibleRenderer { ui: ui.clone() })], CompressibleObjectTreeOptions::default());
        let tree = guard.add_rc(tree);
        tree.layout(Some(200.0), None);
        (ui, container, tree)
    }

    fn celement(element: i64, children: Vec<ICompressedTreeElement<i64>>) -> ICompressedTreeElement<i64> {
        ICompressedTreeElement::with_children(element, children)
    }

    fn cleaf(element: i64) -> ICompressedTreeElement<i64> {
        ICompressedTreeElement::new(element)
    }

    fn set_compressed(tree: &CompressibleObjectTree<i64>, element: Option<i64>, children: Vec<ICompressedTreeElement<i64>>) {
        tree.set_children(&element, children, ObjectTreeModelSetChildrenOptions::default());
    }

    fn deep_chain() -> Vec<ICompressedTreeElement<i64>> {
        vec![celement(1, vec![celement(11, vec![celement(111, vec![cleaf(1111), cleaf(1112), cleaf(1113)])])])]
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn empty() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (ui, container, _tree) = setup(&guard);

        assert_eq!(get_rows_text_content(&ui, container).len(), 0);
    }

    #[test]
    fn simple() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (ui, container, tree) = setup(&guard);

        set_compressed(&tree, None, vec![celement(0, vec![cleaf(10), cleaf(11), cleaf(12)]), cleaf(1), cleaf(2)]);

        assert_eq!(get_rows_text_content(&ui, container), strings(&["0", "10", "11", "12", "1", "2"]));
    }

    #[test]
    fn compressed() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (ui, container, tree) = setup(&guard);

        set_compressed(&tree, None, deep_chain());

        assert_eq!(get_rows_text_content(&ui, container), strings(&["1/11/111", "1111", "1112", "1113"]));

        set_compressed(&tree, Some(11), vec![cleaf(111), cleaf(112), cleaf(113)]);

        assert_eq!(get_rows_text_content(&ui, container), strings(&["1/11", "111", "112", "113"]));

        set_compressed(&tree, Some(113), vec![cleaf(1131)]);

        assert_eq!(get_rows_text_content(&ui, container), strings(&["1/11", "111", "112", "113/1131"]));

        set_compressed(&tree, Some(1131), vec![cleaf(1132)]);

        assert_eq!(get_rows_text_content(&ui, container), strings(&["1/11", "111", "112", "113/1131/1132"]));

        set_compressed(&tree, Some(1131), vec![cleaf(1132), cleaf(1133)]);

        assert_eq!(get_rows_text_content(&ui, container), strings(&["1/11", "111", "112", "113/1131", "1132", "1133"]));
    }

    #[test]
    fn enable_compression() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (ui, container, tree) = setup(&guard);

        set_compressed(&tree, None, deep_chain());

        assert_eq!(get_rows_text_content(&ui, container), strings(&["1/11/111", "1111", "1112", "1113"]));

        tree.update_options(&CompressibleObjectTreeOptionsUpdate { compression_enabled: Some(false), ..Default::default() });
        assert_eq!(get_rows_text_content(&ui, container), strings(&["1", "11", "111", "1111", "1112", "1113"]));

        tree.update_options(&CompressibleObjectTreeOptionsUpdate { compression_enabled: Some(true), ..Default::default() });
        assert_eq!(get_rows_text_content(&ui, container), strings(&["1/11/111", "1111", "1112", "1113"]));
    }
}
