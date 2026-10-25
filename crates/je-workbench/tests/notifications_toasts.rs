//! Porte de `src/vs/workbench/test/browser/notificationsToasts.test.ts` (suíte `NotificationsToasts`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (declarado antes de
//!   tudo, para ser o último a cair); `testDisposables.add` vira `store.add` / `store.add_rc`. O `teardown`
//!   (`setNotificationRowHeight(DEFAULT_NOTIFICATION_ROW_HEIGHT)`) é o `Drop` de [`RowHeightReset`].
//! - **`suiteSetup` de aquecimento.** [`warm_up`] roda o mesmo fluxo (um toast de erro até a visibilidade) sobre um
//!   `DisposableStore` próprio, antes do guarda de cada caso (o rastreador e as `static` são por fio).
//! - **`workbenchInstantiationService(undefined, testDisposables)`.** São os serviços montados à mão em [`toasts_services`],
//!   com os dublês de `support/notification_doubles.rs`.
//! - **DOM vira árvore do `je-ui`.** `document.body.appendChild(container)` é um filho da raiz com a classe
//!   `monaco-workbench`; `querySelectorAll('.notification-toast-container').length` é a busca por classe na subárvore.
//! - **`await Promise.resolve()`** é [`run_microtasks`]; **`flushAnimationFrame`** é `Ui::run_animation_frame` seguido das
//!   microtarefas; **`await toastVisible`** (`Event.toPromise(onDidChangeVisibility)`) é um ouvinte que marca o disparo,
//!   conferido depois do quadro.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas, com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::lifecycle::{DisposableStore, to_disposable};
use je_base::runtime::run_microtasks;
use je_editor::core_2d::dimension::Dimension;
use je_platform::contextkey::IContextKeyService;
use je_platform::notification::{Notification, NotificationHandle, Severity};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Ui};
use je_workbench::browser::parts::notifications::notifications_toasts::{NotificationsToasts, NotificationsToastsServices};
use je_workbench::browser::parts::notifications::notifications_viewer::{DEFAULT_NOTIFICATION_ROW_HEIGHT, set_notification_row_height};
use je_workbench::common::notifications::{INotificationsModel, NotificationsModel};
use je_workbench::services::editor::editor_groups_service::IEditorGroupsService;
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::host::host::IHostService;
use je_workbench::services::lifecycle::lifecycle::ILifecycleService;
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestEnvironmentService, TestHostService};
use je_workbench::test::common::workbench_test_services::TestLifecycleService;
use notification_doubles::{EmptyThemeService, RecordingOpener, TestLayoutService, renderer_services_in};

/// O `teardown(() => setNotificationRowHeight(DEFAULT_NOTIFICATION_ROW_HEIGHT))`.
struct RowHeightReset;

impl Drop for RowHeightReset {
    fn drop(&mut self) {
        set_notification_row_height(DEFAULT_NOTIFICATION_ROW_HEIGHT);
    }
}

/// O que `createToasts` devolve (`container`, `model`, `toasts`), mais a [`Ui`] que desenha o contêiner.
struct Fixture {
    ui: Ui,
    container: NodeId,
    model: Rc<NotificationsModel>,
    toasts: Rc<NotificationsToasts>,
}

impl Fixture {
    /// `container.querySelectorAll('.notification-toast-container').length`.
    fn toast_count(&self) -> usize {
        let tree = self.ui.tree();
        tree.subtree(self.container).into_iter().filter(|node| tree.has_class(*node, "notification-toast-container")).count()
    }

    /// `flushAnimationFrame()`.
    fn flush_animation_frame(&self) {
        self.ui.run_animation_frame();
        run_microtasks();
    }

    /// `Event.toPromise(toasts.onDidChangeVisibility)`: a marca de que o evento disparou.
    fn visibility_fired(&self, store: &DisposableStore) -> Rc<Cell<bool>> {
        let fired = Rc::new(Cell::new(false));
        let marker = fired.clone();
        store.add(self.toasts.on_did_change_visibility().once().listen(move |_| marker.set(true)));
        fired
    }
}

fn toasts_services(store: &DisposableStore, ui: &Ui) -> NotificationsToastsServices {
    let editor_groups_service: Rc<dyn IEditorGroupsService> = TestEditorGroupsService::new(None);
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    let lifecycle_service: Rc<dyn ILifecycleService> = TestLifecycleService::new();
    let host_service: Rc<dyn IHostService> = Rc::new(TestHostService);
    let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
    NotificationsToastsServices {
        renderer: renderer_services_in(store, ui, Rc::new(RecordingOpener::default())),
        layout_service: Rc::new(TestLayoutService),
        theme_service: Rc::new(EmptyThemeService),
        editor_groups_service,
        context_key_service,
        lifecycle_service,
        host_service,
        environment_service,
    }
}

/// `createToasts(testDisposables)`.
fn create_toasts(store: &DisposableStore) -> Fixture {
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 800.0, 600.0));
        let container = tree.create(root, Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, 800.0, 600.0));
        tree.add_class(container, "monaco-workbench");
        container
    };
    {
        let ui = ui.clone();
        store.add(to_disposable(move || ui.remove(container)));
    }

    let model = store.add_rc(NotificationsModel::new());
    {
        let model = model.clone();
        store.add(to_disposable(move || {
            for notification in model.notifications() {
                notification.close();
            }
        }));
    }

    let model_dyn: Rc<dyn INotificationsModel> = model.clone();
    let toasts = store.add_rc(NotificationsToasts::new(container, model_dyn, toasts_services(store, &ui)));
    // Avoid viewport-dependent hiding because these tests assert scheduled toast counts.
    toasts.layout(Some(Dimension { width: 1024.0, height: 9007199254740991.0 }));
    run_microtasks();

    Fixture { ui, container, model, toasts }
}

/// O `suiteSetup`: um toast de erro até a visibilidade, sobre um `DisposableStore` descartável.
fn warm_up() {
    let warmup_disposables = DisposableStore::new();
    let fixture = create_toasts(&warmup_disposables);
    let toast_visible = fixture.visibility_fired(&warmup_disposables);
    fixture.model.add_notification(Notification::new(Severity::Error, "Warmup"));
    fixture.flush_animation_frame();
    assert!(toast_visible.get());
    warmup_disposables.dispose();
}

fn start() -> (LeakGuard, RowHeightReset) {
    warm_up();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    (guard, RowHeightReset)
}

#[test]
fn shows_one_toast_for_rapidly_added_duplicate_notifications() {
    let (guard, _reset) = start();
    let fixture = create_toasts(&guard.store());
    let toast_visible = fixture.visibility_fired(&guard.store());

    for _ in 0..15 {
        fixture.model.add_notification(Notification::new(Severity::Error, "Hello!"));
    }
    run_microtasks();

    let before_animation_frame = (fixture.model.notifications().len(), fixture.toast_count());

    fixture.flush_animation_frame();
    assert!(toast_visible.get());
    assert_eq!(
        (before_animation_frame, fixture.model.notifications().len(), fixture.toast_count(), fixture.toasts.is_visible()),
        ((1, 0), 1, 1, true)
    );
}

#[test]
fn limits_rapidly_added_distinct_notification_toasts() {
    let (guard, _reset) = start();
    let fixture = create_toasts(&guard.store());
    let toast_visible = fixture.visibility_fired(&guard.store());

    for i in 0..15 {
        fixture.model.add_notification(Notification::new(Severity::Error, format!("Message {i}")));
    }

    fixture.flush_animation_frame();
    assert!(toast_visible.get());

    assert_eq!((fixture.model.notifications().len(), fixture.toast_count(), fixture.toasts.is_visible()), (15, 3, true));
}

#[test]
fn does_not_show_a_pending_notification_removed_before_rendering() {
    let (guard, _reset) = start();
    let fixture = create_toasts(&guard.store());
    let handle = fixture.model.add_notification(Notification::new(Severity::Error, "Hello!"));

    handle.close();
    run_microtasks();
    fixture.flush_animation_frame();

    assert_eq!((fixture.model.notifications().len(), fixture.toast_count(), fixture.toasts.is_visible()), (0, 0, false));
}

#[test]
fn recalculates_visible_toasts_when_row_height_changes() {
    let (guard, _reset) = start();
    set_notification_row_height(34.0);
    let fixture = create_toasts(&guard.store());

    let sticky = |message: &str| {
        let mut notification = Notification::new(Severity::Error, message);
        notification.properties.sticky = true;
        notification
    };
    fixture.model.add_notification(sticky("First"));
    fixture.model.add_notification(sticky("Second"));
    fixture.flush_animation_frame();

    let mut compact_fit_height: Option<u32> = None;
    for height in 1..=500u32 {
        fixture.toasts.layout(Some(Dimension { width: 1024.0, height: f64::from(height) }));
        if fixture.toast_count() == 2 {
            compact_fit_height = Some(height);
            break;
        }
    }

    set_notification_row_height(DEFAULT_NOTIFICATION_ROW_HEIGHT);
    let standard_visible_toasts = fixture.toast_count();
    set_notification_row_height(34.0);

    assert_eq!((compact_fit_height.is_some(), standard_visible_toasts, fixture.toast_count()), (true, 1, 2));
}
