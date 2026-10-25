//! Porte de `src/vs/editor/test/browser/widget/virtualizedItemManager.test.ts`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - Os resultados de `assert.deepStrictEqual` sobre objetos são structs locais com `PartialEq`; `undefined` é `None`.
//! - `assert.throws` é `errors::catch(...).is_err()`. O `TestItem` e as subclasses `TestBinding` e `TestTemplate` guardam a
//!   struct base do módulo (`VirtualizedItemBinding`, `VirtualizedItemTemplate`) no lugar de herdá-la; a ligação guarda um
//!   `Weak` do template (o template guarda a ligação atual).
//! - O `createContext()` não tem nós do DOM (a vista não os tem; ver o módulo dela).
//! - O caso do coletor de lixo (`WeakRef` e `gc()`) usa `Weak` de `Rc`: a contagem de referências libera na hora, então o
//!   `this.skip()` de quando o coletor não está exposto não se aplica e o caso roda sempre, sem `timeout(0)` nem `gc()`.
//! - O `errorHandler.getUnexpectedErrorHandler()` restaurado no `finally` é o tratador do `LeakGuard`
//!   (`record_unexpected_errors`), reinstalado ao fim.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, record_unexpected_errors};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, Observable, autorun, const_observable, observable_value};
use je_editor::compressed_virtualized_scroll_view::{ICompressedVirtualizedScrollItem, ICompressedVirtualizedScrollViewContext, MaxScroll};
use je_editor::multi_diff_editor::virtualized_item_manager::{
    IVirtualizedItemBinding, IVirtualizedItemBindingContext, IVirtualizedItemDelegate, IVirtualizedItemTemplate, VirtualizedItemBinding, VirtualizedItemManager,
    VirtualizedItemTemplate,
};
use je_editor::offset_range::OffsetRange;

type Items = IObservable<Rc<Vec<Rc<TestItem>>>>;
type Manager = VirtualizedItemManager<Rc<TestItem>, TestBinding, TestTemplate>;
type Templates = Rc<RefCell<Vec<Rc<TestTemplate>>>>;

#[derive(Debug, PartialEq)]
struct TransferOutcome {
    template_count: usize,
    first_binding_item: String,
    first_binding_disposed: bool,
    current_binding_item: Option<String>,
    second_binding_item: String,
    virtual_a_size: i32,
    virtual_b_size: i32,
}

#[test]
fn strictly_transfers_a_pooled_template_between_typed_bindings() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestItem::new("a", 100);
    let item_b = TestItem::new("b", 200);
    let items = observable_value::<Rc<Vec<Rc<TestItem>>>, ()>("items", Rc::new(vec![item_a.clone(), item_b.clone()]));
    let templates = Templates::default();
    let manager = create_manager(&guard, items.to_observable(), |_| "test".to_owned(), recording_template_factory(&templates));
    let virtual_items = manager.virtualized_items.get();
    let (virtual_a, virtual_b) = (virtual_items[0].clone(), virtual_items[1].clone());
    let range = OffsetRange::new(0, 100);

    virtual_a.render(range, 0, 800.0, range, None);
    let binding_a = virtual_a.binding.get().unwrap();
    let first_template = templates.borrow()[0].clone();
    assert!(errors::catch(|| first_template.bind(item_b.clone(), no_op_context(item_b.size.get()))).is_err());
    virtual_a.hide();
    virtual_b.render(range, 0, 800.0, range, None);
    let binding_b = virtual_b.binding.get().unwrap();

    assert_eq!(
        TransferOutcome {
            template_count: templates.borrow().len(),
            first_binding_item: binding_a.item().id.clone(),
            first_binding_disposed: binding_a.did_dispose.get(),
            current_binding_item: first_template.current_binding().get().map(|binding| binding.item().id.clone()),
            second_binding_item: binding_b.item().id.clone(),
            virtual_a_size: virtual_a.size.get(),
            virtual_b_size: virtual_b.size.get(),
        },
        TransferOutcome {
            template_count: 1,
            first_binding_item: "a".to_owned(),
            first_binding_disposed: true,
            current_binding_item: Some("b".to_owned()),
            second_binding_item: "b".to_owned(),
            virtual_a_size: 100,
            virtual_b_size: 200,
        }
    );
}

#[test]
fn uses_separate_pools_for_different_template_ids() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestItem::with_template_id("a", 100, "text");
    let item_b = TestItem::with_template_id("b", 200, "image");
    let created_template_ids: Rc<RefCell<Vec<String>>> = Rc::default();
    let manager = create_manager(
        &guard,
        const_observable(Rc::new(vec![item_a, item_b])),
        |item| item.template_id.clone(),
        {
            let created_template_ids = created_template_ids.clone();
            move |template_id| {
                created_template_ids.borrow_mut().push(template_id.to_owned());
                TestTemplate::new()
            }
        },
    );
    let range = OffsetRange::new(0, 100);
    for item in manager.virtualized_items.get().iter() {
        item.render(range, 0, 800.0, range, None);
    }

    assert_eq!(*created_template_ids.borrow(), ["text", "image"]);
}

#[derive(Debug, PartialEq)]
struct ReuseOutcome {
    current_item: Option<String>,
    first_disposed: bool,
    second_disposed: bool,
    current_binding: Option<String>,
}

#[test]
fn preserves_a_new_binding_when_unbinding_triggers_synchronous_template_reuse() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let template = guard.add_rc(TestTemplate::new());
    let context = no_op_context(100);
    let binding_a = template.bind(TestItem::new("a", 100), context.clone());
    let binding_b: Rc<RefCell<Option<Rc<TestBinding>>>> = Rc::default();
    guard.add(autorun({
        let (template, binding_b, context) = (template.clone(), binding_b.clone(), context.clone());
        move |reader| {
            if template.current_binding().read(reader).is_none() && binding_b.borrow().is_none() {
                let bound = template.bind(TestItem::new("b", 100), context.clone());
                *binding_b.borrow_mut() = Some(bound);
            }
        }
    }));
    binding_a.dispose();
    let current_item = template.current_binding().get().map(|binding| binding.item().id.clone());
    binding_a.dispose();
    let binding_b = binding_b.borrow().clone().unwrap();
    binding_b.dispose();

    assert_eq!(
        ReuseOutcome {
            current_item,
            first_disposed: binding_a.did_dispose.get(),
            second_disposed: binding_b.did_dispose.get(),
            current_binding: template.current_binding().get().map(|binding| binding.item().id.clone()),
        },
        ReuseOutcome { current_item: Some("b".to_owned()), first_disposed: true, second_disposed: true, current_binding: None }
    );
}

#[derive(Debug, PartialEq)]
struct TemplateState {
    disposed: bool,
    current_binding: Option<String>,
}

#[derive(Debug, PartialEq)]
struct DisposalOutcome {
    bindings_disposed: Vec<bool>,
    templates: Vec<TemplateState>,
}

#[test]
fn manager_disposal_releases_both_active_and_idle_templates() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let templates = Templates::default();
    let manager = create_manager(
        &guard,
        const_observable(Rc::new(vec![TestItem::new("a", 100), TestItem::new("b", 100)])),
        |_| "test".to_owned(),
        recording_template_factory(&templates),
    );
    let virtual_items = manager.virtualized_items.get();
    let (item_a, item_b) = (virtual_items[0].clone(), virtual_items[1].clone());
    let range = OffsetRange::new(0, 100);
    item_a.render(range, 0, 800.0, range, None);
    item_b.render(range, 0, 800.0, range, None);
    let bindings = [item_a.binding.get().unwrap(), item_b.binding.get().unwrap()];
    item_a.hide();
    manager.dispose();
    manager.dispose();

    assert_eq!(
        DisposalOutcome {
            bindings_disposed: bindings.iter().map(|binding| binding.did_dispose.get()).collect(),
            templates: templates
                .borrow()
                .iter()
                .map(|template| TemplateState { disposed: template.is_disposed(), current_binding: template.current_binding().get().map(|binding| binding.item().id.clone()) })
                .collect(),
        },
        DisposalOutcome {
            bindings_disposed: vec![true, true],
            templates: vec![TemplateState { disposed: true, current_binding: None }, TemplateState { disposed: true, current_binding: None }],
        }
    );
}

fn capture_disposed_binding(template: &Rc<TestTemplate>) -> (Weak<TestItem>, Weak<TestBinding>) {
    let item = TestItem::new("a", 100);
    let binding = template.bind(item.clone(), no_op_context(100));
    binding.dispose();
    (Rc::downgrade(&item), Rc::downgrade(&binding))
}

#[derive(Debug, PartialEq)]
struct CollectionOutcome {
    current_binding: Option<String>,
    retained_bindings: usize,
    retained_items: usize,
}

#[test]
fn idle_templates_allow_previous_bindings_and_items_to_be_collected_after_repeated_reuse() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let template = guard.add_rc(TestTemplate::new());
    let captured: Vec<_> = (0..20).map(|_| capture_disposed_binding(&template)).collect();

    assert_eq!(
        CollectionOutcome {
            current_binding: template.current_binding().get().map(|binding| binding.item().id.clone()),
            retained_bindings: captured.iter().filter(|(_, binding)| binding.upgrade().is_some()).count(),
            retained_items: captured.iter().filter(|(item, _)| item.upgrade().is_some()).count(),
        },
        CollectionOutcome { current_binding: None, retained_bindings: 0, retained_items: 0 }
    );
}

#[derive(Debug, PartialEq)]
struct FailureOutcome {
    binding_attempts: Vec<String>,
    errors: Vec<String>,
    virtual_a_binding: Option<String>,
    virtual_a_size: i32,
    virtual_b_binding_item: Option<String>,
}

#[test]
fn isolates_a_failed_binding_without_changing_cached_layout_state() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestItem::new("a", 100);
    let item_b = TestItem::new("b", 200);
    let binding_attempts: Rc<RefCell<Vec<String>>> = Rc::default();
    let reported_errors: Rc<RefCell<Vec<String>>> = Rc::default();
    let manager = create_manager(&guard, const_observable(Rc::new(vec![item_a, item_b])), |_| "test".to_owned(), {
        let binding_attempts = binding_attempts.clone();
        move |_| TestTemplate::with_failure(Some(binding_attempts.clone()), Some("a"))
    });
    errors::set_unexpected_error_handler({
        let reported_errors = reported_errors.clone();
        move |error| reported_errors.borrow_mut().push(error.message.clone())
    });
    let outcome = errors::catch(|| {
        let virtual_items = manager.virtualized_items.get();
        let (virtual_a, virtual_b) = (virtual_items[0].clone(), virtual_items[1].clone());
        let range = OffsetRange::new(0, 100);
        virtual_a.render(range, 0, 800.0, range, None);
        virtual_a.render(range, 0, 800.0, range, None);
        virtual_b.render(range, 0, 800.0, range, None);

        assert_eq!(
            FailureOutcome {
                binding_attempts: binding_attempts.borrow().clone(),
                errors: reported_errors.borrow().clone(),
                virtual_a_binding: virtual_a.binding.get().map(|binding| binding.item().id.clone()),
                virtual_a_size: virtual_a.size.get(),
                virtual_b_binding_item: virtual_b.binding.get().map(|binding| binding.item().id.clone()),
            },
            FailureOutcome {
                binding_attempts: vec!["a".to_owned(), "b".to_owned()],
                errors: vec!["Failed to bind a".to_owned()],
                virtual_a_binding: None,
                virtual_a_size: 100,
                virtual_b_binding_item: Some("b".to_owned()),
            }
        );
    });
    record_unexpected_errors();
    if let Err(error) = outcome {
        errors::throw(error);
    }
}

struct TestItem {
    id: String,
    size: Rc<ObservableValue<i32, ()>>,
    template_id: String,
}

impl TestItem {
    fn new(id: &str, size: i32) -> Rc<Self> {
        Self::with_template_id(id, size, "test")
    }

    fn with_template_id(id: &str, size: i32, template_id: &str) -> Rc<Self> {
        Rc::new(Self { id: id.to_owned(), size: observable_value::<i32, ()>("size", size), template_id: template_id.to_owned() })
    }
}

struct TestBinding {
    base: VirtualizedItemBinding<Rc<TestItem>>,
    template: Weak<TestTemplate>,
    size: IObservable<i32>,
    max_scroll: IObservable<MaxScroll>,
    should_keep_alive: IObservable<bool>,
    did_dispose: Cell<bool>,
}

impl TestBinding {
    fn new(item: Rc<TestItem>, template: Weak<TestTemplate>) -> Self {
        let size = item.size.to_observable();
        Self {
            base: VirtualizedItemBinding::new(item),
            template,
            size,
            max_scroll: const_observable(MaxScroll { max_scroll: 0.0 }),
            should_keep_alive: const_observable(false),
            did_dispose: Cell::new(false),
        }
    }
}

impl IVirtualizedItemBinding<Rc<TestItem>> for TestBinding {
    fn item(&self) -> &Rc<TestItem> {
        &self.base.item
    }

    fn size(&self) -> IObservable<i32> {
        self.size.clone()
    }

    fn max_scroll(&self) -> IObservable<MaxScroll> {
        self.max_scroll.clone()
    }

    fn should_keep_alive(&self) -> IObservable<bool> {
        self.should_keep_alive.clone()
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.base.on_did_dispose()
    }

    fn render(&self, _rendered_range: OffsetRange, _scroll_offset: i32, _width: f64, _rendered_viewport: OffsetRange) {}

    fn hide(&self) {}
}

impl IDisposable for TestBinding {
    fn dispose(&self) {
        if self.did_dispose.replace(true) {
            return;
        }
        if let Some(template) = self.template.upgrade() {
            template.unbind(self);
        }
        self.base.dispose();
    }
}

struct TestTemplate {
    base: VirtualizedItemTemplate<Rc<TestItem>, TestBinding>,
    me: Weak<TestTemplate>,
    binding_attempts: Option<Rc<RefCell<Vec<String>>>>,
    item_to_fail: Option<String>,
}

impl TestTemplate {
    fn new() -> Rc<Self> {
        Self::with_failure(None, None)
    }

    fn with_failure(binding_attempts: Option<Rc<RefCell<Vec<String>>>>, item_to_fail: Option<&str>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self { base: VirtualizedItemTemplate::new(), me: me.clone(), binding_attempts, item_to_fail: item_to_fail.map(str::to_owned) })
    }

    fn is_disposed(&self) -> bool {
        self.base.store().is_disposed()
    }

    fn create_binding(&self, item: Rc<TestItem>, _context: IVirtualizedItemBindingContext) -> Rc<TestBinding> {
        if let Some(binding_attempts) = &self.binding_attempts {
            binding_attempts.borrow_mut().push(item.id.clone());
        }
        if self.item_to_fail.as_deref() == Some(item.id.as_str()) {
            errors::throw(Error::new(format!("Failed to bind {}", item.id)));
        }
        Rc::new(TestBinding::new(item, self.me.clone()))
    }

    fn unbind(&self, binding: &TestBinding) {
        if !self.current_binding().get().is_some_and(|current| std::ptr::eq(Rc::as_ptr(&current), binding)) {
            errors::throw(Error::new("Binding does not own this template"));
        }
    }
}

impl IVirtualizedItemTemplate<Rc<TestItem>, TestBinding> for TestTemplate {
    fn current_binding(&self) -> IObservable<Option<Rc<TestBinding>>> {
        self.base.current_binding()
    }

    fn bind(&self, item: Rc<TestItem>, context: IVirtualizedItemBindingContext) -> Rc<TestBinding> {
        self.base.bind(item, context, |item, context| self.create_binding(item, context))
    }
}

impl IDisposable for TestTemplate {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `{ initialSize, runWithScrollAnchor: () => { } }`.
fn no_op_context(initial_size: i32) -> IVirtualizedItemBindingContext {
    IVirtualizedItemBindingContext::new(initial_size, |_, _| {})
}

fn create_context() -> ICompressedVirtualizedScrollViewContext {
    ICompressedVirtualizedScrollViewContext { scroll_left: const_observable(0.0) }
}

/// O `createTemplate` que guarda cada template criado em `templates`.
fn recording_template_factory(templates: &Templates) -> impl Fn(&str) -> Rc<TestTemplate> + 'static {
    let templates = templates.clone();
    move |_| {
        let template = TestTemplate::new();
        templates.borrow_mut().push(template.clone());
        template
    }
}

/// O `new VirtualizedItemManager(items, createContext(), { getId, getTemplateId, getUnboundSize, createTemplate })`
/// registrado em `disposables.add(...)`.
fn create_manager(
    guard: &LeakGuard,
    items: Items,
    get_template_id: impl Fn(&Rc<TestItem>) -> String + 'static,
    create_template: impl Fn(&str) -> Rc<TestTemplate> + 'static,
) -> Rc<Manager> {
    let delegate = IVirtualizedItemDelegate::new(
        |item: &Rc<TestItem>| Dynamic::string(&item.id),
        get_template_id,
        |item: &Rc<TestItem>| item.size.to_observable(),
        move |template_id, _context| create_template(template_id),
    );
    guard.add_rc(Manager::new(items, create_context(), delegate))
}
