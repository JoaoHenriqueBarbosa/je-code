//! Porte de `src/vs/workbench/test/browser/part.test.ts` (suíte `Workbench parts`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo de cada caso; o `disposables.add(...)` do
//!   original é `guard.add_rc`. O `teardown` (`fixture.remove()` e `disposables.clear()`) é o `Drop` do [`Suite`] seguido do `Drop`
//!   do guarda, nessa ordem.
//! - **DOM vira estado nativo.** O `document` é o [`TestDocument`]: o `body`, os `id`s atribuídos (`element.id = ...`) e o
//!   `getElementById`, que só encontra o elemento ligado ao `body` pela cadeia de pais (como o `document` real). `$('div')` e
//!   `append(parent, child)` são [`append_div`]; `hide(b)` é `b.hide()`. `innerText` não tem estado nativo e não é lido.
//! - **`SimplePart`, `MyPart`, `MyPart2` e `MyPart3`.** A `SimplePart` (limites 50, `layout` e `toJSON` que lançam) é o
//!   [`SimplePart`], um [`PartBehavior`] sobre o conteúdo de cada parte (`SimplePartContent`: `createTitleArea` e
//!   `createContentArea`). O `testGetMemento`, o `testSaveState`, o `testSetHeaderArea`, o `testSetFooterArea` e o
//!   `testLayoutContents` são os métodos públicos da `Part` (`get_memento`, `save_state`, `set_header_area`, `set_footer_area` e
//!   `layout_contents`). `super.createTitleArea(parent)!` devolve `undefined`, que é `None`.
//! - **`TestLayoutService` e `ModernUITestLayoutService`.** O `modernUI` e o `modernUICompact` do `ModernUITestLayoutService` são o
//!   par de chaves `set_floating_panels_enabled` e `set_modern_ui_compact` do `TestLayoutService` (por fio de teste, que é o escopo
//!   de cada caso).
//! - **`StorageScope.PROFILE` e `StorageTarget.MACHINE`** são `StorageScope::Profile` e `StorageTarget::Machine`. O memento do
//!   caso 1 vem do cache de mementos do processo (a chave é o `id` da parte), como no original, e não do serviço de
//!   armazenamento de cada parte.
//! - **`assert.deepStrictEqual`** entre os resultados de layout é `assert_eq!` sobre o [`LayoutContentResult`], e `Dimension.None`
//!   é `NONE_DIMENSION`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[path = "../../je-editor/tests/support/test_color_theme_service.rs"]
mod test_color_theme_service;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::types::is_empty_object;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::IThemeService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_workbench::browser::part::{LayoutContentResult, NONE_DIMENSION, Part, PartBehavior, PartOptions};
use je_workbench::services::layout::layout_service::IWorkbenchLayoutService;
use je_workbench::test::common::workbench_test_services::TestStorageService;
use notification_doubles::{TestLayoutService, set_floating_panels_enabled, set_modern_ui_compact};
use test_color_theme_service::TestThemeService;

const FIXTURE_ID: &str = "workbench-part-fixture";

// region: dublês

/// O `mainWindow.document`: o `body`, os `id`s dos elementos e o `getElementById`.
struct TestDocument {
    body: Rc<ViewElement>,
    ids: RefCell<Vec<(String, Rc<ViewElement>)>>,
}

impl TestDocument {
    fn new() -> Rc<Self> {
        Rc::new(Self { body: ViewElement::new(), ids: RefCell::new(Vec::new()) })
    }

    /// `element.id = id`.
    fn set_id(&self, element: &Rc<ViewElement>, id: &str) {
        self.ids.borrow_mut().push((id.to_owned(), element.clone()));
    }

    /// O elemento está no documento: a cadeia de pais chega ao `body`.
    fn is_connected(&self, element: &Rc<ViewElement>) -> bool {
        let mut current = Some(element.clone());
        while let Some(candidate) = current {
            if Rc::ptr_eq(&candidate, &self.body) {
                return true;
            }
            current = candidate.parent_element();
        }
        false
    }

    /// `getElementById(id)`.
    fn get_element_by_id(&self, id: &str) -> Option<Rc<ViewElement>> {
        self.ids.borrow().iter().find(|(candidate, element)| candidate == id && self.is_connected(element)).map(|(_, element)| element.clone())
    }
}

/// `append(parent, $('div'))`.
fn append_div(parent: &Rc<ViewElement>) -> Rc<ViewElement> {
    let child = ViewElement::new();
    child.set_parent(Some(parent.clone()));
    child
}

/// O `setup` (o `fixture` com o id `workbench-part-fixture` no `body`) e o `teardown` (`fixture.remove()`).
struct Suite {
    document: Rc<TestDocument>,
    fixture: Rc<ViewElement>,
}

fn setup() -> Suite {
    let document = TestDocument::new();
    let fixture = ViewElement::new();
    document.set_id(&fixture, FIXTURE_ID);
    fixture.set_parent(Some(document.body.clone()));
    Suite { document, fixture }
}

impl Suite {
    /// `const b = document.createElement('div'); mainWindow.document.getElementById(fixtureId)!.appendChild(b); hide(b);`.
    fn hidden_container(&self) -> Rc<ViewElement> {
        let b = ViewElement::new();
        b.set_parent(Some(self.document.get_element_by_id(FIXTURE_ID).expect("o fixture está no documento")));
        b.hide();
        b
    }
}

impl Drop for Suite {
    fn drop(&mut self) {
        self.fixture.set_parent(None);
    }
}

/// O que `MyPart`, `MyPart2` e `MyPart3` sobrescrevem de `SimplePart`.
trait SimplePartContent {
    fn create_title_area(&self, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>>;
    fn create_content_area(&self, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>>;
}

/// `SimplePart`: limites de 50, `layout` e `toJSON` que lançam.
struct SimplePart<C: SimplePartContent>(C);

impl<C: SimplePartContent> PartBehavior for SimplePart<C> {
    fn create_title_area(&self, parent: &Rc<ViewElement>, _options: Option<&Dynamic>) -> Option<Rc<ViewElement>> {
        self.0.create_title_area(parent)
    }

    fn create_content_area(&self, parent: &Rc<ViewElement>, _options: Option<&Dynamic>) -> Option<Rc<ViewElement>> {
        self.0.create_content_area(parent)
    }

    fn layout(&self, _part: &Part, _width: f64, _height: f64, _top: f64, _left: f64) {
        panic!("Method not implemented.");
    }

    fn minimum_width(&self) -> f64 {
        50.0
    }

    fn maximum_width(&self) -> f64 {
        50.0
    }

    fn minimum_height(&self) -> f64 {
        50.0
    }

    fn maximum_height(&self) -> f64 {
        50.0
    }

    fn to_json(&self) -> Dynamic {
        panic!("Method not implemented.");
    }
}

/// `MyPart`: confere o `parent` das duas áreas e devolve o `undefined` do `super`.
struct MyPart {
    expected_parent: Rc<ViewElement>,
}

impl SimplePartContent for MyPart {
    fn create_title_area(&self, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        assert!(Rc::ptr_eq(parent, &self.expected_parent));
        None
    }

    fn create_content_area(&self, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        assert!(Rc::ptr_eq(parent, &self.expected_parent));
        None
    }
}

/// `MyPart2`: título (`myPart.title`) e conteúdo (`myPart.content`).
struct MyPart2 {
    document: Rc<TestDocument>,
}

impl SimplePartContent for MyPart2 {
    fn create_title_area(&self, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        let title_container = append_div(parent);
        let title_label = append_div(&title_container);
        self.document.set_id(&title_label, "myPart.title");

        Some(title_container)
    }

    fn create_content_area(&self, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        create_content_with_id(&self.document, parent)
    }
}

/// `MyPart3`: sem título (`createTitleArea` devolve `null`) e só o conteúdo.
struct MyPart3 {
    document: Rc<TestDocument>,
}

impl SimplePartContent for MyPart3 {
    fn create_title_area(&self, _parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        None
    }

    fn create_content_area(&self, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        create_content_with_id(&self.document, parent)
    }
}

/// O `createContentArea` de `MyPart2` e `MyPart3`: um contêiner com um `span` de id `myPart.content`.
fn create_content_with_id(document: &Rc<TestDocument>, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
    let content_container = append_div(parent);
    let content_span = append_div(&content_container);
    document.set_id(&content_span, "myPart.content");

    Some(content_container)
}

// endregion

/// `new MyPart*(...)` com um `TestThemeService`, um `TestStorageService` e o serviço de layout dados.
fn create_part<C: SimplePartContent + 'static>(guard: &LeakGuard, id: &str, options: PartOptions, layout_service: Rc<dyn IWorkbenchLayoutService>, content: C) -> Rc<Part> {
    let theme_service: Rc<dyn IThemeService> = guard.add_rc(TestThemeService::new_default());
    let storage = guard.add_rc(TestStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);

    guard.add_rc(Part::new(id, options, theme_service, storage_service, layout_service, Rc::new(SimplePart(content))))
}

/// `new MyPart(b)`.
fn create_my_part(guard: &LeakGuard, expected_parent: &Rc<ViewElement>) -> Rc<Part> {
    let options = PartOptions { has_title: true, ..PartOptions::default() };
    create_part(guard, "myPart", options, Rc::new(TestLayoutService), MyPart { expected_parent: expected_parent.clone() })
}

/// `new MyPart2(layoutService, titleHeight)`.
fn create_my_part2(guard: &LeakGuard, document: &Rc<TestDocument>, title_height: Option<Rc<dyn Fn() -> Option<f64>>>) -> Rc<Part> {
    let options = PartOptions { has_title: true, title_height, ..PartOptions::default() };
    create_part(guard, "myPart2", options, Rc::new(TestLayoutService), MyPart2 { document: document.clone() })
}

fn key(name: &str) -> JsString {
    JsString::from(name)
}

fn dimension(width: f64, height: f64) -> Dimension {
    Dimension { width, height }
}

#[test]
fn creation() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let suite = setup();
    let b = suite.hidden_container();

    let mut part = create_my_part(&guard, &b);
    part.create(&b, None);

    assert_eq!(part.get_id(), "myPart");

    // Memento
    let memento = part.get_memento(StorageScope::Profile, StorageTarget::Machine);
    memento.set(&key("foo"), Dynamic::string("bar"));
    memento.set(&key("bar"), Dynamic::array([1.0, 2.0, 3.0].map(Dynamic::Number)));

    part.save_state();

    // Re-Create to assert memento contents
    part = create_my_part(&guard, &b);

    let memento = part.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert_eq!(memento.get(&key("foo")), Some(Dynamic::string("bar")));
    let Some(Dynamic::Array(bar)) = memento.get(&key("bar")) else {
        panic!("o memento deveria ter o array `bar`");
    };
    assert_eq!(bar.len(), 3);

    // Empty Memento stores empty object
    memento.delete(&key("foo"));
    memento.delete(&key("bar"));

    part.save_state();
    part = create_my_part(&guard, &b);
    let memento = part.get_memento(StorageScope::Profile, StorageTarget::Machine);
    assert!(is_empty_object(&Dynamic::Object(memento)));
}

#[test]
fn part_layout_with_title_and_content() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let suite = setup();
    let b = suite.hidden_container();

    let part = create_my_part2(&guard, &suite.document, None);
    part.create(&b, None);

    assert!(suite.document.get_element_by_id("myPart.title").is_some());
    assert!(suite.document.get_element_by_id("myPart.content").is_some());
}

#[test]
fn part_layout_preserves_modern_ui_chrome_across_densities() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let suite = setup();

    let part = create_my_part2(&guard, &suite.document, None);
    part.create(&suite.fixture, None);
    part.set_header_area(&ViewElement::new());
    part.set_footer_area(&ViewElement::new());

    let classic_layout = part.layout_contents(100.0, 200.0);
    set_floating_panels_enabled(true);
    let modern_ui_layout = part.layout_contents(100.0, 200.0);
    set_modern_ui_compact(true);
    let compact_modern_ui_layout = part.layout_contents(100.0, 200.0);

    assert_eq!(
        (classic_layout, modern_ui_layout, compact_modern_ui_layout),
        (
            LayoutContentResult {
                header_size: dimension(100.0, 35.0),
                title_size: dimension(100.0, 35.0),
                content_size: dimension(100.0, 95.0),
                footer_size: dimension(100.0, 35.0),
            },
            LayoutContentResult {
                header_size: dimension(100.0, 32.0),
                title_size: dimension(100.0, 32.0),
                content_size: dimension(100.0, 104.0),
                footer_size: dimension(100.0, 32.0),
            },
            LayoutContentResult {
                header_size: dimension(100.0, 32.0),
                title_size: dimension(100.0, 32.0),
                content_size: dimension(100.0, 104.0),
                footer_size: dimension(100.0, 32.0),
            },
        )
    );
}

#[test]
fn part_layout_supports_a_custom_title_height() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let suite = setup();

    let title_height: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(Some(28.0)));
    let title_height_source = title_height.clone();
    let part = create_my_part2(&guard, &suite.document, Some(Rc::new(move || title_height_source.get())));
    part.create(&suite.fixture, None);

    let custom_layout = part.layout_contents(100.0, 200.0);
    title_height.set(None);
    let default_layout = part.layout_contents(100.0, 200.0);

    assert_eq!(
        (custom_layout, default_layout),
        (
            LayoutContentResult {
                header_size: NONE_DIMENSION,
                title_size: dimension(100.0, 28.0),
                content_size: dimension(100.0, 172.0),
                footer_size: NONE_DIMENSION,
            },
            LayoutContentResult {
                header_size: NONE_DIMENSION,
                title_size: dimension(100.0, 35.0),
                content_size: dimension(100.0, 165.0),
                footer_size: NONE_DIMENSION,
            },
        )
    );
}

#[test]
fn part_layout_with_content_only() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let suite = setup();
    let b = suite.hidden_container();

    let options = PartOptions { has_title: false, ..PartOptions::default() };
    let part = create_part(&guard, "myPart2", options, Rc::new(TestLayoutService), MyPart3 { document: suite.document.clone() });
    part.create(&b, None);

    assert!(suite.document.get_element_by_id("myPart.title").is_none());
    assert!(suite.document.get_element_by_id("myPart.content").is_some());
}
