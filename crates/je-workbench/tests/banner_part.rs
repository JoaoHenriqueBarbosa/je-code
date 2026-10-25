//! Porte de `src/vs/workbench/test/browser/parts/banner/bannerPart.test.ts` (suíte `BannerPart`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; o `store.add(...)` do original é
//!   `guard.add_rc`/`guard.add`. O `store.add(toDisposable(() => container.remove()))` é a saída do escopo do caso (o contêiner é só
//!   um [`ViewElement`], sem documento).
//! - **DOM vira estado nativo.** O banner desenha pelo [`BannerPartView`] (ver o cabeçalho de `banner_part.rs`); o [`TestBannerView`]
//!   guarda o que o DOM do original guardaria e que os casos leem: o texto do `.message-container`, os textos dos `a` do
//!   `.message-actions-container`, a quantidade de filhos do contêiner (`childElementCount`), o elemento ativo
//!   (`mainWindow.document.activeElement`) e a ação de fechar (`.action-label`, cujo `click()` é [`TestBannerView::click_close`]).
//!   O que o `clearNode` remove do foco (o elemento ativo volta a ser o corpo do documento) também é reproduzido. A classe
//!   `neutral` fica no próprio [`ViewElement`] (`container.classList.contains('neutral')`).
//! - **`workbenchInstantiationService(undefined, store)`.** Só os serviços que o `BannerPart` injeta: o `TestThemeService`, o
//!   `TestStorageService`, o serviço de layout de teste, e o `ContextKeyService` real sobre um `TestConfigurationService`.
//! - **`assert.deepStrictEqual`** entre os objetos literais é `assert_eq!` entre tuplas; `undefined` do `?.textContent` é `None`.

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
use je_base::lifecycle::{DisposableRef, none};
use je_editor::services::theme_service::IThemeService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::storage::StorageService;
use je_workbench::browser::parts::banner::banner_part::{BannerPart, BannerPartView};
use je_workbench::services::banner::banner_service::{BannerIcon, BannerMessage, IBannerItem, IBannerService, ILinkDescriptor};
use je_workbench::services::layout::layout_service::IWorkbenchLayoutService;
use je_workbench::test::common::workbench_test_services::TestStorageService;
use notification_doubles::TestLayoutService;
use test_color_theme_service::TestThemeService;

/// O `mainWindow.document.activeElement`: o corpo do documento ou um elemento do banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveElement {
    Body,
    Container,
    ActionLink(usize),
    CloseAction,
}

/// O DOM do banner, no que os casos leem.
struct TestBannerView {
    active_element: Cell<ActiveElement>,
    message: RefCell<Option<String>>,
    actions: RefCell<Vec<String>>,
    child_count: Cell<usize>,
    close: RefCell<Option<Rc<dyn Fn()>>>,
    focus_listener: RefCell<Option<Rc<dyn Fn()>>>,
}

impl TestBannerView {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            active_element: Cell::new(ActiveElement::Body),
            message: RefCell::new(None),
            actions: RefCell::new(Vec::new()),
            child_count: Cell::new(0),
            close: RefCell::new(None),
            focus_listener: RefCell::new(None),
        })
    }

    /// `container.querySelector('.message-container')?.textContent`.
    fn message(&self) -> Option<String> {
        self.message.borrow().clone()
    }

    /// `[...container.querySelectorAll('.message-actions-container a')].map(link => link.textContent)`.
    fn actions(&self) -> Vec<String> {
        self.actions.borrow().clone()
    }

    /// `container.childElementCount`.
    fn child_count(&self) -> usize {
        self.child_count.get()
    }

    /// `mainWindow.document.activeElement === container`.
    fn container_is_active(&self) -> bool {
        self.active_element.get() == ActiveElement::Container
    }

    /// `container.querySelector<HTMLElement>('.action-label')?.click()`.
    fn click_close(&self) {
        let close = self.close.borrow().clone();
        if let Some(close) = close {
            close();
        }
    }
}

impl BannerPartView for TestBannerView {
    fn has_focus_within(&self, _element: &Rc<ViewElement>) -> bool {
        self.active_element.get() != ActiveElement::Body
    }

    fn focus(&self, _element: &Rc<ViewElement>) {
        self.active_element.set(ActiveElement::Container);

        let listener = self.focus_listener.borrow().clone();
        if let Some(listener) = listener {
            listener();
        }
    }

    fn clear_node(&self, _element: &Rc<ViewElement>) {
        // Remover o filho focado devolve o foco ao corpo do documento.
        if self.active_element.get() != ActiveElement::Container {
            self.active_element.set(ActiveElement::Body);
        }
        *self.message.borrow_mut() = None;
        self.actions.borrow_mut().clear();
        self.child_count.set(0);
        *self.close.borrow_mut() = None;
    }

    fn set_tab_index(&self, _element: &Rc<ViewElement>, _index: i32) {}

    fn set_aria_label(&self, _element: &Rc<ViewElement>, _label: Option<&str>) {}

    fn add_focus_listener(&self, _element: &Rc<ViewElement>, handler: Rc<dyn Fn()>) -> DisposableRef {
        *self.focus_listener.borrow_mut() = Some(handler);

        none()
    }

    fn render_icon(&self, _element: &Rc<ViewElement>, _icon: Option<&BannerIcon>) {
        self.child_count.set(self.child_count.get() + 1);
    }

    fn render_message(&self, _element: &Rc<ViewElement>, message: &BannerMessage) -> DisposableRef {
        *self.message.borrow_mut() = Some(message.to_text());
        self.child_count.set(self.child_count.get() + 1);

        none()
    }

    fn render_actions(&self, _element: &Rc<ViewElement>, actions: &[ILinkDescriptor]) -> DisposableRef {
        *self.actions.borrow_mut() = actions.iter().map(|action| action.label.clone()).collect();
        self.child_count.set(self.child_count.get() + 1);

        none()
    }

    fn render_close_action(&self, _element: &Rc<ViewElement>, _label: &str, on_run: Rc<dyn Fn()>) -> DisposableRef {
        *self.close.borrow_mut() = Some(on_run);
        self.child_count.set(self.child_count.get() + 1);

        none()
    }

    fn focus_action_link(&self, index: usize) {
        if index < self.actions.borrow().len() {
            self.active_element.set(ActiveElement::ActionLink(index));
        }
    }

    fn focus_close_action(&self) {
        self.active_element.set(ActiveElement::CloseAction);
    }
}

struct Banner {
    banner: Rc<BannerPart>,
    container: Rc<ViewElement>,
    view: Rc<TestBannerView>,
}

/// `createBanner()`.
fn create_banner(guard: &LeakGuard) -> Banner {
    let configuration_service = TestConfigurationService::new(None);
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();

    let theme_service: Rc<dyn IThemeService> = guard.add_rc(TestThemeService::new_default());
    let storage = guard.add_rc(TestStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let layout_service: Rc<dyn IWorkbenchLayoutService> = Rc::new(TestLayoutService);
    let context_key_service: Rc<dyn IContextKeyService> = guard.add_rc(ContextKeyService::new(configuration));

    let view = TestBannerView::new();
    let container = ViewElement::with_classes(&["part", "banner"]);
    let banner = guard.add_rc(BannerPart::new(theme_service, layout_service, storage_service, context_key_service, view.clone()));
    banner.create(&container);

    Banner { banner, container, view }
}

fn text(message: &str) -> BannerMessage {
    BannerMessage::Text(message.to_string())
}

fn link(label: &str, href: &str) -> ILinkDescriptor {
    ILinkDescriptor { label: label.to_string(), href: href.to_string(), ..ILinkDescriptor::default() }
}

#[test]
fn updates_same_id_content_and_actions_without_accumulating_old_dom_or_losing_banner_focus() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Banner { banner, container, view } = create_banner(&guard);

    let item = IBannerItem {
        id: "update".to_string(),
        icon: None,
        message: text("Update required"),
        actions: Some(vec![link("Check for Updates", "command:update.checkForUpdate")]),
        aria_label: None,
        on_close: None,
        close_label: None,
        priority: None,
        neutral: Some(true),
    };
    banner.show(item.clone());
    banner.focus_next_action();
    banner.show(IBannerItem {
        message: text("New minimum version"),
        actions: Some(vec![link("Restart to Update", "command:update.restartToUpdate")]),
        ..item.clone()
    });
    let updated = (view.message(), view.actions(), view.container_is_active(), container.class_list_contains("neutral"), banner.minimum_height());
    banner.hide("update");

    assert_eq!(
        (updated, view.child_count(), banner.minimum_height()),
        (
            (Some("New minimum version".to_string()), vec!["Restart to Update".to_string()], true, true, 26.0),
            0,
            0.0,
        )
    );
}

#[test]
fn preserves_higher_priority_notices_and_restores_the_latest_pending_banner_after_hide_or_close() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Banner { banner, view, .. } = create_banner(&guard);

    let closed = Rc::new(Cell::new(0));
    let trust = IBannerItem { id: "trust".to_string(), icon: None, message: text("Restricted Mode"), actions: None, aria_label: None, on_close: None, close_label: None, priority: None, neutral: None };
    let on_close_counter = closed.clone();
    let update = IBannerItem {
        id: "update".to_string(),
        icon: None,
        message: text("Update required"),
        actions: None,
        aria_label: None,
        on_close: Some(Rc::new(move || on_close_counter.set(on_close_counter.get() + 1))),
        close_label: None,
        priority: Some(-1),
        neutral: None,
    };
    let messages: RefCell<Vec<Option<String>>> = RefCell::new(Vec::new());
    let capture = || messages.borrow_mut().push(view.message());
    banner.show(trust.clone());
    banner.show(update.clone());
    capture();
    banner.show(IBannerItem { message: text("Updated requirement"), ..update.clone() });
    capture();
    banner.hide("trust");
    capture();
    banner.show(trust.clone());
    banner.hide("update");
    capture();
    banner.hide("trust");
    banner.show(update.clone());
    view.click_close();
    capture();

    assert_eq!(
        (messages.into_inner(), closed.get(), banner.minimum_height()),
        (
            vec![Some("Restricted Mode".to_string()), Some("Restricted Mode".to_string()), Some("Updated requirement".to_string()), Some("Restricted Mode".to_string()), None],
            1,
            0.0,
        )
    );
}
