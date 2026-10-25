//! Porte de `src/vs/base/test/browser/dom.test.ts` (suíte `dom`), caso a caso e na mesma ordem. O caso da suíte
//! `ModifierKeyEmitter` mora em `crates/je-widgets/tests/dom.rs`, porque a produção equivalente (`ModifierKeyEmitter`) é do `je-widgets`.
//!
//! Notas de tradução:
//! - `document` é o documento da janela principal (`main_window().document()`); `$` é `create_el`, `h` é `h`, e `classList`
//!   é `class_list_*` do [`Node`]. A produção está em `je_base::browser::dom` e `je_base::browser::css_value`.
//! - `getWindows`, `getWindowById` e companhia são o registro por thread da mesma crate. `getWindowById(...)?.window` é a
//!   própria janela devolvida. O evento `document.createEvent('MouseEvent')` é `Document::create_event`.
//! - `await waitForAnimationFrame()` é pedir um `request_animation_frame` e entregar o quadro com `render_frame`: o callback do
//!   agendador foi pedido antes, então roda antes do callback do teste.
//! - O `FakeResizeObserver` injetado por `resizeObserverCtor` é um `ResizeObserverCtor` que guarda o callback recebido. Um
//!   callback que lança devolve `Err`. O `iframe` do caso de janela auxiliar é uma `CodeWindow` com id 999, e o
//!   `teardown` do suite (um `requestAnimationFrame`) é desnecessário porque o quadro só roda quando o teste o entrega.
//! - A suíte usa `runWithFakedTimers` só nos casos de `trackAttributes`; os de `disposableWindowInterval` usam os timers reais.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, record_unexpected_errors, run_with_faked_timers, run_with_real_timers, timeout};
use je_base::browser::css_value::as_css_value_with_default;
use je_base::browser::dom::{
    AnimationFrameScheduler, ClientRect, Child, CodeWindow, Document, DisposableResizeObserver, DomTarget, HChild, Node, ResizeObserverBackend, ResizeObserverCallback,
    ResizeObserverCtor, ResizeObserverEntry, SafeTriangle, StopAt, copy_attributes, create_el, disposable_window_interval, ensure_code_window, find_parent_with_class, get_document, get_recent_disposable_resize_observer_context_for_loop_error, get_window, get_window_by_id,
    get_window_by_id_or_main, get_window_id, get_windows, get_windows_count, h, has_parent_with_class, has_window, is_auxiliary_window, main_window, track_attributes,
};
use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;
use je_base::promise::DeferredPromise;
use je_base::runtime;

fn document() -> Rc<Document> {
    main_window().document()
}

/// `document.createElement('div')`.
fn div() -> Node {
    document().create_element("div")
}

/// `$(description, attrs, ...children)`.
fn dollar(description: &str, attrs: &[(&str, Option<&str>)], children: &[Child<'_>]) -> Node {
    create_el(&document(), description, attrs, children)
}

fn h_of(tag: &str) -> je_base::browser::dom::HResult {
    h(&document(), tag, &[], &[])
}

#[test]
fn has_class() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let element = div();
    element.set_class_name("foobar boo far");

    assert!(element.class_list_contains("foobar"));
    assert!(element.class_list_contains("boo"));
    assert!(element.class_list_contains("far"));
    assert!(!element.class_list_contains("bar"));
    assert!(!element.class_list_contains("foo"));
    assert!(!element.class_list_contains(""));
}

#[test]
fn find_parent_with_class_supports_multiple_required_classes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = dollar("div.modern-ui.motion-enabled", &[], &[]);
    let intermediate = dollar("div.modern-ui", &[], &[]);
    let child = dollar("div", &[], &[]);
    root.append_child(&intermediate).append_child(&child);

    let actual = (
        find_parent_with_class(&child, &["modern-ui", "motion-enabled"], None) == Some(root.clone()),
        find_parent_with_class(&child, &["modern-ui"], None) == Some(intermediate.clone()),
        has_parent_with_class(&child, &["modern-ui", "missing"], None),
        has_parent_with_class(&child, &["modern-ui", "motion-enabled"], Some(StopAt::Node(&intermediate))),
    );

    assert_eq!(actual, (true, true, false, false));
}

#[test]
fn remove_class() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let element = div();
    element.set_class_name("foobar boo far");

    element.class_list_remove("boo");
    assert!(element.class_list_contains("far"));
    assert!(!element.class_list_contains("boo"));
    assert!(element.class_list_contains("foobar"));
    assert_eq!(element.class_name(), "foobar far");

    let element = div();
    element.set_class_name("foobar boo far");

    element.class_list_remove("far");
    assert!(!element.class_list_contains("far"));
    assert!(element.class_list_contains("boo"));
    assert!(element.class_list_contains("foobar"));
    assert_eq!(element.class_name(), "foobar boo");

    element.class_list_remove("boo");
    assert!(!element.class_list_contains("far"));
    assert!(!element.class_list_contains("boo"));
    assert!(element.class_list_contains("foobar"));
    assert_eq!(element.class_name(), "foobar");

    element.class_list_remove("foobar");
    assert!(!element.class_list_contains("far"));
    assert!(!element.class_list_contains("boo"));
    assert!(!element.class_list_contains("foobar"));
    assert_eq!(element.class_name(), "");
}

#[test]
fn remove_class_should_consider_hyphens() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let element = div();

    element.class_list_add("foo-bar");
    element.class_list_add("bar");

    assert!(element.class_list_contains("foo-bar"));
    assert!(element.class_list_contains("bar"));

    element.class_list_remove("bar");
    assert!(element.class_list_contains("foo-bar"));
    assert!(!element.class_list_contains("bar"));

    element.class_list_remove("foo-bar");
    assert!(!element.class_list_contains("foo-bar"));
    assert!(!element.class_list_contains("bar"));
}

mod dollar_suite {
    use super::*;

    #[test]
    fn should_build_simple_nodes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let node = dollar("div", &[], &[]);
        assert!(node.is_html_element());
        assert_eq!(node.tag_name().as_deref(), Some("DIV"));
        assert!(node.first_child().is_none());
    }

    #[test]
    fn should_build_nodes_with_id() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let node = dollar("div#foo", &[], &[]);
        assert!(node.is_html_element());
        assert_eq!(node.tag_name().as_deref(), Some("DIV"));
        assert_eq!(node.id(), "foo");
    }

    #[test]
    fn should_build_nodes_with_class_name() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let node = dollar("div.foo", &[], &[]);
        assert!(node.is_html_element());
        assert_eq!(node.tag_name().as_deref(), Some("DIV"));
        assert_eq!(node.class_name(), "foo");
    }

    #[test]
    fn should_build_nodes_with_attributes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let node = dollar("div", &[("class", Some("test"))], &[]);
        assert_eq!(node.class_name(), "test");

        // `$('div', undefined)`: sem atributos.
        let node = dollar("div", &[], &[]);
        assert_eq!(node.class_name(), "");
    }

    #[test]
    fn should_build_nodes_with_children() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let span = dollar("span", &[("id", Some("demospan"))], &[]);
        let node = dollar("div", &[], &[Child::Node(&span)]);
        let first_child = node.first_child().expect("o span");
        assert_eq!(first_child.tag_name().as_deref(), Some("SPAN"));
        assert_eq!(first_child.id(), "demospan");

        let node = dollar("div", &[], &[Child::Text("hello")]);

        assert_eq!(node.first_child().map(|child| child.text_content()).as_deref(), Some("hello"));
    }

    #[test]
    fn should_build_nodes_with_text_children() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let node = dollar("div", &[], &[Child::Text("foobar")]);
        let first_child = node.first_child().expect("o texto");
        assert_eq!(first_child.tag_name(), None);
        assert_eq!(first_child.text_content(), "foobar");
    }
}

mod h_suite {
    use super::*;

    fn assert_classes(node: &Node, expected: &[&str]) {
        assert_eq!(node.class_list_length(), expected.len());
        for class in expected {
            assert!(node.class_list_contains(class));
        }
    }

    #[test]
    fn should_build_simple_nodes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let div = h_of("div");
        assert!(div.root.is_html_element());
        assert_eq!(div.root.tag_name().as_deref(), Some("DIV"));

        let span = h_of("span");
        assert!(span.root.is_html_element());
        assert_eq!(span.root.tag_name().as_deref(), Some("SPAN"));

        let img = h_of("img");
        assert!(img.root.is_html_element());
        assert_eq!(img.root.tag_name().as_deref(), Some("IMG"));
    }

    #[test]
    fn should_handle_ids_and_classes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        for tag in ["div", "span"] {
            let upper = tag.to_ascii_uppercase();

            let with_id = h_of(&format!("{tag}#myid"));
            assert_eq!(with_id.root.tag_name().as_deref(), Some(upper.as_str()));
            assert_eq!(with_id.root.id(), "myid");

            let with_class = h_of(&format!("{tag}.a"));
            assert_eq!(with_class.root.tag_name().as_deref(), Some(upper.as_str()));
            assert_classes(&with_class.root, &["a"]);

            let with_classes = h_of(&format!("{tag}.a.b.c"));
            assert_eq!(with_classes.root.tag_name().as_deref(), Some(upper.as_str()));
            assert_classes(&with_classes.root, &["a", "b", "c"]);

            let with_all = h_of(&format!("{tag}#myid.a.b.c"));
            assert_eq!(with_all.root.tag_name().as_deref(), Some(upper.as_str()));
            assert_eq!(with_all.root.id(), "myid");
            assert_classes(&with_all.root, &["a", "b", "c"]);
        }
    }

    #[test]
    fn should_implicitly_handle_ids_and_classes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let div_id = h_of("#myid");
        assert_eq!(div_id.root.tag_name().as_deref(), Some("DIV"));
        assert_eq!(div_id.root.id(), "myid");

        let div_class = h_of(".a");
        assert_eq!(div_class.root.tag_name().as_deref(), Some("DIV"));
        assert_classes(&div_class.root, &["a"]);

        let div_classes = h_of(".a.b.c");
        assert_eq!(div_classes.root.tag_name().as_deref(), Some("DIV"));
        assert_classes(&div_classes.root, &["a", "b", "c"]);

        let div_all = h_of("#myid.a.b.c");
        assert_eq!(div_all.root.tag_name().as_deref(), Some("DIV"));
        assert_eq!(div_all.root.id(), "myid");
        assert_classes(&div_all.root, &["a", "b", "c"]);
    }

    #[test]
    fn should_handle_at_identifiers() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let implicit = h_of("@el");
        assert_eq!(Some(&implicit.root), implicit.get("el"));
        assert_eq!(implicit.get("el").and_then(Node::tag_name).as_deref(), Some("DIV"));

        let explicit = h_of("div@el");
        assert_eq!(Some(&explicit.root), explicit.get("el"));
        assert_eq!(explicit.get("el").and_then(Node::tag_name).as_deref(), Some("DIV"));

        let implicit_id = h_of("#myid@el");
        assert_eq!(Some(&implicit_id.root), implicit_id.get("el"));
        assert_eq!(implicit_id.get("el").and_then(Node::tag_name).as_deref(), Some("DIV"));
        assert_eq!(implicit_id.root.id(), "myid");

        let explicit_id = h_of("div#myid@el");
        assert_eq!(Some(&explicit_id.root), explicit_id.get("el"));
        assert_eq!(explicit_id.get("el").and_then(Node::tag_name).as_deref(), Some("DIV"));
        assert_eq!(explicit_id.root.id(), "myid");

        let implicit_class = h_of(".a@el");
        assert_eq!(Some(&implicit_class.root), implicit_class.get("el"));
        assert_eq!(implicit_class.get("el").and_then(Node::tag_name).as_deref(), Some("DIV"));
        assert_classes(&implicit_class.root, &["a"]);

        let explicit_class = h_of("div.a@el");
        assert_eq!(Some(&explicit_class.root), explicit_class.get("el"));
        assert_eq!(explicit_class.get("el").and_then(Node::tag_name).as_deref(), Some("DIV"));
        assert_classes(&explicit_class.root, &["a"]);
    }
}

#[test]
fn should_recurse() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let document = document();
    let title = h(&document, "div.title@title", &[], &[]);
    let gutter_div = h(&document, "div.gutter@gutterDiv", &[], &[]);
    let editor = h(&document, "span@editor", &[], &[]);
    let container = h(&document, "div.container", &[], &[HChild::Result(&gutter_div), HChild::Result(&editor)]);
    let result = h(&document, "div.code-view", &[], &[HChild::Result(&title), HChild::Result(&container)]);

    let title = result.get("title").expect("title");
    let gutter_div = result.get("gutterDiv").expect("gutterDiv");
    let editor = result.get("editor").expect("editor");

    assert_eq!(result.root.tag_name().as_deref(), Some("DIV"));
    assert_eq!(result.root.class_name(), "code-view");
    assert_eq!(result.root.child_element_count(), 2);
    assert_eq!(result.root.first_element_child().as_ref(), Some(title));
    assert_eq!(title.tag_name().as_deref(), Some("DIV"));
    assert_eq!(title.class_name(), "title");
    assert_eq!(title.child_element_count(), 0);
    assert_eq!(gutter_div.tag_name().as_deref(), Some("DIV"));
    assert_eq!(gutter_div.class_name(), "gutter");
    assert_eq!(gutter_div.child_element_count(), 0);
    assert_eq!(editor.tag_name().as_deref(), Some("SPAN"));
    assert_eq!(editor.class_name(), "");
    assert_eq!(editor.child_element_count(), 0);
}

#[test]
fn css_value_with_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(as_css_value_with_default(Some("red"), "blue"), "red");
    assert_eq!(as_css_value_with_default(None, "blue"), "blue");
    assert_eq!(as_css_value_with_default(Some("var(--my-var)"), "blue"), "var(--my-var, blue)");
    assert_eq!(as_css_value_with_default(Some("var(--my-var, red)"), "blue"), "var(--my-var, red)");
    assert_eq!(as_css_value_with_default(Some("var(--my-var, var(--my-var2))"), "blue"), "var(--my-var, var(--my-var2, blue))");
}

#[test]
fn copy_attributes_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let element_source = div();
    element_source.set_attribute("foo", "bar");
    element_source.set_attribute("bar", "foo");

    let element_target = div();
    copy_attributes(&element_source, &element_target, None);

    assert_eq!(element_target.get_attribute("foo").as_deref(), Some("bar"));
    assert_eq!(element_target.get_attribute("bar").as_deref(), Some("foo"));
}

#[test]
fn track_attributes_unfiltered() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let element_source = div();
        let element_target = div();

        let disposable = track_attributes(&element_source, &element_target, None);

        element_source.set_attribute("foo", "bar");
        element_source.set_attribute("bar", "foo");

        timeout(1).await.expect("timeout");

        assert_eq!(element_target.get_attribute("foo").as_deref(), Some("bar"));
        assert_eq!(element_target.get_attribute("bar").as_deref(), Some("foo"));

        disposable.dispose();
    });
}

#[test]
fn track_attributes_filtered() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let element_source = div();
        let element_target = div();

        let disposable = track_attributes(&element_source, &element_target, Some(&["foo"]));

        element_source.set_attribute("foo", "bar");
        element_source.set_attribute("bar", "foo");

        timeout(1).await.expect("timeout");

        assert_eq!(element_target.get_attribute("foo").as_deref(), Some("bar"));
        assert_eq!(element_target.get_attribute("bar"), None);

        disposable.dispose();
    });
}

#[test]
fn window_utilities() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let main = main_window();
    let windows = get_windows();
    assert_eq!(windows.len(), 1);
    assert_eq!(get_windows_count(), 1);
    let window_id = get_window_id(&main).expect("a janela principal tem id");
    assert!(get_window_by_id(window_id).is_some_and(|window| Rc::ptr_eq(&window, &main)));
    assert!(Rc::ptr_eq(&get_window_by_id_or_main(None), &main));
    assert!(has_window(window_id));
    assert!(!is_auxiliary_window(&main));
    ensure_code_window(&main, 1);
    assert!(main.vscode_window_id().is_some());

    let element = div();
    assert!(Rc::ptr_eq(&get_window(Some(DomTarget::Node(&element))), &main));
    assert!(Rc::ptr_eq(&get_document(Some(DomTarget::Node(&element))), &main.document()));

    let event = document().create_event();
    assert!(Rc::ptr_eq(&get_window(Some(DomTarget::Event(&event))), &main));
    assert!(Rc::ptr_eq(&get_document(Some(DomTarget::Event(&event))), &main.document()));
}

mod disposable_window_interval_suite {
    use super::*;

    #[test]
    fn basics() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_real_timers(async {
            let count = Rc::new(Cell::new(0));
            let promise = Rc::new(DeferredPromise::<()>::new());
            let interval = {
                let (count, promise) = (count.clone(), promise.clone());
                disposable_window_interval(
                    move || {
                        count.set(count.get() + 1);
                        if count.get() == 3 {
                            promise.complete(());
                            true
                        } else {
                            false
                        }
                    },
                    0,
                    Some(10),
                )
            };

            promise.promise.clone().await.expect("o intervalo completa");
            assert_eq!(count.get(), 3);
            interval.dispose();
        });
    }

    #[test]
    fn iterations() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_real_timers(async {
            let count = Rc::new(Cell::new(0));
            let interval = {
                let count = count.clone();
                disposable_window_interval(
                    move || {
                        count.set(count.get() + 1);
                        false
                    },
                    0,
                    Some(0),
                )
            };

            timeout(5).await.expect("timeout");
            assert_eq!(count.get(), 0);
            interval.dispose();
        });
    }

    #[test]
    fn dispose() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_real_timers(async {
            let count = Rc::new(Cell::new(0));
            let interval = {
                let count = count.clone();
                disposable_window_interval(
                    move || {
                        count.set(count.get() + 1);
                        false
                    },
                    0,
                    Some(10),
                )
            };

            interval.dispose();
            timeout(5).await.expect("timeout");
            assert_eq!(count.get(), 0);
        });
    }
}

mod safe_triangle_suite {
    use super::*;

    /// `fakeElement(left, right, top, bottom)`.
    fn fake_element(left: f64, right: f64, top: f64, bottom: f64) -> ClientRect {
        ClientRect { left, right, top, bottom }
    }

    #[test]
    fn works() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let safe_triangle = SafeTriangle::new(0.0, 0.0, fake_element(10.0, 20.0, 10.0, 20.0));

        assert!(safe_triangle.contains(5.0, 5.0)); // in triangle region
        assert!(!safe_triangle.contains(15.0, 5.0));
        assert!(!safe_triangle.contains(25.0, 5.0));

        assert!(!safe_triangle.contains(5.0, 15.0));
        assert!(safe_triangle.contains(15.0, 15.0));
        assert!(!safe_triangle.contains(25.0, 15.0));

        assert!(!safe_triangle.contains(5.0, 25.0));
        assert!(!safe_triangle.contains(15.0, 25.0));
        assert!(!safe_triangle.contains(25.0, 25.0));
    }

    #[test]
    fn other_dirations() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let a = SafeTriangle::new(30.0, 30.0, fake_element(10.0, 20.0, 10.0, 20.0));
        assert!(a.contains(25.0, 25.0));

        let b = SafeTriangle::new(0.0, 30.0, fake_element(10.0, 20.0, 10.0, 20.0));
        assert!(b.contains(5.0, 25.0));

        let c = SafeTriangle::new(30.0, 0.0, fake_element(10.0, 20.0, 10.0, 20.0));
        assert!(c.contains(25.0, 5.0));
    }
}

mod animation_frame_scheduler_suite {
    use super::*;

    /// `await waitForAnimationFrame()`: pede um quadro depois dos já pedidos e o entrega.
    fn wait_for_animation_frame() {
        let arrived = Rc::new(Cell::new(false));
        let flag = arrived.clone();
        let window = main_window();
        window.request_animation_frame(move || flag.set(true));
        window.render_frame();
        assert!(arrived.get(), "o quadro não chegou");
    }

    fn counting_scheduler() -> (AnimationFrameScheduler, Rc<Cell<u32>>) {
        let node = div();
        let call_count = Rc::new(Cell::new(0));
        let counter = call_count.clone();
        (AnimationFrameScheduler::new(&node, move || counter.set(counter.get() + 1)), call_count)
    }

    #[test]
    fn schedules_and_runs_the_callback() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (scheduler, call_count) = counting_scheduler();

        assert!(!scheduler.is_scheduled());
        scheduler.schedule();
        assert!(scheduler.is_scheduled());

        // Wait for the animation frame
        wait_for_animation_frame();

        assert_eq!(call_count.get(), 1);
        assert!(!scheduler.is_scheduled());
        scheduler.dispose();
    }

    #[test]
    fn coalesces_multiple_schedule_calls() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (scheduler, call_count) = counting_scheduler();

        scheduler.schedule();
        scheduler.schedule();
        scheduler.schedule();

        assert!(scheduler.is_scheduled());

        // Wait for the animation frame
        wait_for_animation_frame();

        assert_eq!(call_count.get(), 1);
        scheduler.dispose();
    }

    #[test]
    fn cancel_prevents_execution() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (scheduler, call_count) = counting_scheduler();

        scheduler.schedule();
        assert!(scheduler.is_scheduled());
        scheduler.cancel();
        assert!(!scheduler.is_scheduled());

        // Wait for the animation frame
        wait_for_animation_frame();

        assert_eq!(call_count.get(), 0);
        scheduler.dispose();
    }

    #[test]
    fn dispose_prevents_execution() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (scheduler, call_count) = counting_scheduler();

        scheduler.schedule();
        scheduler.dispose();

        // Wait for the animation frame
        wait_for_animation_frame();

        assert_eq!(call_count.get(), 0);
    }

    #[test]
    fn can_schedule_again_after_execution() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (scheduler, call_count) = counting_scheduler();

        scheduler.schedule();
        wait_for_animation_frame();
        assert_eq!(call_count.get(), 1);

        scheduler.schedule();
        wait_for_animation_frame();
        assert_eq!(call_count.get(), 2);

        scheduler.dispose();
    }
}

mod disposable_resize_observer_suite {
    use super::*;

    const LOOP_MESSAGE: &str = "ResizeObserver loop completed with undelivered notifications.";

    struct FakeBackend {
        disconnects: Rc<Cell<u32>>,
    }

    impl ResizeObserverBackend for FakeBackend {
        fn observe(&self, _target: &Node) {}

        fn unobserve(&self, _target: &Node) {}

        fn disconnect(&self) {
            self.disconnects.set(self.disconnects.get() + 1);
        }
    }

    /// Captura o callback entregue ao `ResizeObserver` para o teste disparar entregas sintéticas.
    struct FakeResizeObserverHandle {
        ctor: ResizeObserverCtor,
        callback: Rc<RefCell<Option<Rc<dyn Fn(&[ResizeObserverEntry])>>>>,
        disconnects: Rc<Cell<u32>>,
    }

    impl FakeResizeObserverHandle {
        fn fire(&self, entries: &[ResizeObserverEntry]) {
            let callback = self.callback.borrow().clone().expect("observer not constructed");
            callback(entries);
        }
    }

    fn create_fake_resize_observer_ctor() -> FakeResizeObserverHandle {
        let callback: Rc<RefCell<Option<Rc<dyn Fn(&[ResizeObserverEntry])>>>> = Rc::new(RefCell::new(None));
        let disconnects = Rc::new(Cell::new(0));
        let (slot, counter) = (callback.clone(), disconnects.clone());
        let ctor: ResizeObserverCtor = Rc::new(move |received: Rc<dyn Fn(&[ResizeObserverEntry])>| {
            *slot.borrow_mut() = Some(received);
            Rc::new(FakeBackend { disconnects: counter.clone() }) as Rc<dyn ResizeObserverBackend>
        });
        FakeResizeObserverHandle { ctor, callback, disconnects }
    }

    fn fake_entry() -> ResizeObserverEntry {
        ResizeObserverEntry { target: div() }
    }

    fn noop() -> ResizeObserverCallback {
        Rc::new(|_: &[ResizeObserverEntry]| Ok(()))
    }

    fn observer(name: &str, callback: ResizeObserverCallback, window: &Rc<CodeWindow>, fake: &FakeResizeObserverHandle) -> DisposableResizeObserver {
        DisposableResizeObserver::new(name, callback, window, Some(fake.ctor.clone())).expect("o construtor injetado")
    }

    #[test]
    fn callback_runs_synchronously_with_the_entries_the_browser_delivered() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fake = create_fake_resize_observer_ctor();
        let calls = Rc::new(Cell::new(0));
        let received: Rc<RefCell<Vec<ResizeObserverEntry>>> = Rc::new(RefCell::new(Vec::new()));
        let (counter, sink) = (calls.clone(), received.clone());
        let observer = observer(
            "test.sync",
            Rc::new(move |entries: &[ResizeObserverEntry]| {
                counter.set(counter.get() + 1);
                *sink.borrow_mut() = entries.to_vec();
                Ok(())
            }),
            &main_window(),
            &fake,
        );
        let a = fake_entry();
        let b = fake_entry();
        fake.fire(&[a.clone(), b.clone()]);
        assert_eq!(calls.get(), 1, "callback runs synchronously inside the resize-observation phase");
        assert_eq!(*received.borrow(), vec![a, b], "entries are forwarded as-is");
        observer.dispose();
    }

    #[test]
    fn each_native_delivery_invokes_the_callback_once_no_batching() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fake = create_fake_resize_observer_ctor();
        let calls = Rc::new(Cell::new(0));
        let counter = calls.clone();
        let observer = observer(
            "test.noBatch",
            Rc::new(move |_: &[ResizeObserverEntry]| {
                counter.set(counter.get() + 1);
                Ok(())
            }),
            &main_window(),
            &fake,
        );
        fake.fire(&[fake_entry()]);
        fake.fire(&[fake_entry()]);
        assert_eq!(calls.get(), 2, "wrapper does not coalesce deliveries");
        observer.dispose();
    }

    #[test]
    fn dispose_disconnects_the_underlying_observer() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fake = create_fake_resize_observer_ctor();
        let observer = observer("test.dispose", noop(), &main_window(), &fake);
        observer.dispose();
        assert_eq!(fake.disconnects.get(), 1);
    }

    #[test]
    fn exceptions_in_the_user_callback_do_not_propagate() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fake = create_fake_resize_observer_ctor();
        let observer = observer("test.throw", Rc::new(|_: &[ResizeObserverEntry]| -> Result<(), Error> { Err(Error::new("boom")) }), &main_window(), &fake);
        // Browser would not catch a throw out of the native callback; we
        // must guard so a single bad consumer does not break delivery for
        // every other observer in the realm. The wrapper routes the throw
        // to onUnexpectedError, so swap the handler for the duration of
        // this test so the test runner does not flag it as a failure.
        let reported = Rc::new(Cell::new(0));
        let counter = reported.clone();
        errors::set_unexpected_error_handler(move |_| counter.set(counter.get() + 1));
        fake.fire(&[fake_entry()]);
        record_unexpected_errors();
        assert_eq!(reported.get(), 1, "o erro vai para onUnexpectedError e não sobe");
        observer.dispose();
    }

    #[test]
    fn exposes_the_configured_name_for_loop_warning_context() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fake = create_fake_resize_observer_ctor();
        let observer = observer("my-observer", noop(), &main_window(), &fake);
        assert_eq!(observer.name, "my-observer");
        observer.dispose();
    }

    #[test]
    fn get_recent_context_for_loop_error_returns_undefined_for_unrelated_messages() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(get_recent_disposable_resize_observer_context_for_loop_error(None, None), None);
        assert_eq!(get_recent_disposable_resize_observer_context_for_loop_error(Some("Uncaught TypeError: foo"), None), None);
    }

    #[test]
    fn get_recent_context_for_loop_error_returns_sorted_unique_wrapped_observers_from_the_current_frame() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let main = main_window();
        let fake = create_fake_resize_observer_ctor();
        let a = observer("a", noop(), &main, &fake);
        fake.fire(&[fake_entry()]);
        let fake_b = create_fake_resize_observer_ctor();
        let b = observer("b", noop(), &main, &fake_b);
        fake_b.fire(&[fake_entry()]);
        fake.fire(&[fake_entry()]);
        let context = get_recent_disposable_resize_observer_context_for_loop_error(Some(LOOP_MESSAGE), None);
        assert_eq!(context.as_deref(), Some("[ResizeObserverLoopContext(a,b)] ResizeObserver loop completed with undelivered notifications."));
        a.dispose();
        b.dispose();
    }

    #[test]
    fn get_recent_context_for_loop_error_marks_bounded_participant_overflow() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let main = main_window();
        let mut observers = Vec::new();
        for i in (0..=8).rev() {
            let fake = create_fake_resize_observer_ctor();
            observers.push(observer(&format!("observer-{i}"), noop(), &main, &fake));
            fake.fire(&[fake_entry()]);
        }
        assert_eq!(
            get_recent_disposable_resize_observer_context_for_loop_error(Some(LOOP_MESSAGE), None).as_deref(),
            Some("[ResizeObserverLoopContext(observer-0,observer-1,observer-2,observer-3,observer-4,observer-5,observer-6,observer-7,<overflow>)] ResizeObserver loop completed with undelivered notifications."),
        );
        for observer in &observers {
            observer.dispose();
        }
    }

    #[test]
    fn get_recent_context_for_loop_error_is_scoped_to_the_observer_window() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let main = main_window();
        let auxiliary_window = CodeWindow::new();
        ensure_code_window(&auxiliary_window, 999);

        let fake = create_fake_resize_observer_ctor();
        let observer = observer("auxiliary", noop(), &auxiliary_window, &fake);
        fake.fire(&[fake_entry()]);

        assert_eq!(get_recent_disposable_resize_observer_context_for_loop_error(Some(LOOP_MESSAGE), Some(&main)), None);
        assert_eq!(
            get_recent_disposable_resize_observer_context_for_loop_error(Some(LOOP_MESSAGE), Some(&auxiliary_window)).as_deref(),
            Some("[ResizeObserverLoopContext(auxiliary)] ResizeObserver loop completed with undelivered notifications."),
        );

        observer.dispose();
        auxiliary_window.render_frame();
    }

    #[test]
    fn get_recent_context_for_loop_error_clears_at_the_next_animation_frame() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let main = main_window();
        let fake = create_fake_resize_observer_ctor();
        let observer = observer("scoped", noop(), &main, &fake);
        fake.fire(&[fake_entry()]);
        // Context is recorded synchronously and survives microtasks (so it is
        // still set when Chromium dispatches the loop warning at the end
        // of the resize-observation phase).
        runtime::run_microtasks();
        assert!(get_recent_disposable_resize_observer_context_for_loop_error(Some(LOOP_MESSAGE), None).is_some());
        main.render_frame();
        assert_eq!(
            get_recent_disposable_resize_observer_context_for_loop_error(Some(LOOP_MESSAGE), None),
            None,
            "context must be cleared at the next frame so a later rendering update does not inherit stale observers"
        );
        observer.dispose();
    }
}
