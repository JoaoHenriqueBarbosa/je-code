//! Porte de `src/vs/workbench/contrib/accessibility/test/browser/accessibleView.test.ts` (suíte `AccessibleView`), caso a caso e
//! na mesma ordem (9 definições de teste, 10 casos: o laço `hasLinks` roda 2).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `disposables.add` vira
//!   `guard.add` / `guard.add_rc`.
//! - **`workbenchInstantiationService({}, disposables)` e `createInstance(AccessibleView)`.** O `AccessibleView` do porte recebe
//!   os serviços reunidos em `AccessibleViewServices` ([`Harness::new`]): dublês do workbench (`notification_doubles`), os de
//!   `je-platform` (configuração, chaves de contexto, armazenamento) e os deste arquivo ([`TestModelService`], [`CountingMenuService`],
//!   [`TestToolbar`], [`TestAccessibilityService`]). O editor é o `TestCodeEditor` do `je-editor` (sem contribuições, o que fixa a linha
//!   de base de ouvintes de `onKeyDown`).
//! - **`RenderingContextViewService` e os dois `mock<IContextViewService>()` literais.** São [`RenderingContextViewService`] (renderiza
//!   num contêiner solto e descarta o resultado ao esconder) e [`PlainContextViewService`] (só guarda o delegado e dispara `onHide`; o
//!   `showContextView` do quarto caso esconde o delegado anterior antes de guardar o novo, o dos demais não). `document.createElement`
//!   é `ViewElement::new`.
//! - **`spy(accessibleView, 'show')` e `show.lastCall.args[0]`.** O que o caso lê é o provedor de ajuda que a visão acabou de mostrar,
//!   que é `current_provider()` depois da micro tarefa.
//! - **`(accessibleView as unknown as { _updateToolbar })`, `_currentProvider`, `_updateLastProvider`.** São `update_toolbar`,
//!   `current_provider` e `update_last_provider`, públicos.
//! - **`editorWidget._onKeyDown._size`.** É `key_down_listener_count()` do `CodeEditorWidget`.
//! - **`IAccessibleViewContentProvider` escrito à mão.** O contrato do porte só tem o `AccessibleContentProvider`, que o enum
//!   `AccessibleViewContentProvider` garante; os três casos do terminal usam um `AccessibleContentProvider` com os mesmos campos.
//! - **`await timeout(0)`.** É [`run_microtasks`]: tudo o que a visão adia é `queueMicrotask` ou `then` de promessa resolvida.
//! - **`model.dispose()` por `getModels()`.** É o `guard.add(to_disposable(...))` sobre o [`TestModelService`].
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas, com os mesmos campos, na mesma ordem.

#[allow(dead_code)]
#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[allow(dead_code, unused_imports)]
#[path = "support/test_quick_input.rs"]
mod test_quick_input;

/// O suporte de editor do `je-editor/tests/support` que o `TestCodeEditor` precisa, no mesmo módulo pai (os arquivos se referem uns
/// aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use editor_support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, create_test_code_editor};
use editor_support::test_text_model::create_text_model_with;
use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime::run_microtasks;
use je_base::uri::URI;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::language_service::ILanguageSelection;
use je_editor::model::{RelaxedTextModelCreationOptions, TextModelCreationOptions};
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_platform::accessibility::accessible_view::{
    AccessibleContentProvider, AccessibleContentProviderArgs, AccessibleViewContentProvider, AccessibleViewProviderId, AccessibleViewType, IAccessibleViewOptions,
};
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::actions::actions::{IMenu, IMenuService, MenuActionGroup, MenuActionOptions, MenuChangeEvent, MenuCreateOptions, MenuId};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{HideData, IContextViewDelegate, IContextViewService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::storage::StorageService;
use je_platform::storage::storage::InMemoryStorageService;
use je_workbench::contrib::accessibility::accessible_view::{
    AccessibleView, AccessibleViewServices, IAccessibleViewToolbar, IChatCodeBlockContextProviderService, ICodeBlockContextProvider,
};
use notification_doubles::{NoCommandService, NoKeybindingService, RecordingOpener, SilentAccessibilitySignals, TestLayoutService};
use test_quick_input::TestQuickInputService;

// region: dublês

/// O `RenderingContextViewService` do arquivo original: renderiza num contêiner solto e descarta o resultado ao esconder, como o
/// context view de verdade.
#[derive(Default)]
struct RenderingContextViewService {
    show_count: Cell<usize>,
    delegate: RefCell<Option<Rc<dyn IContextViewDelegate>>>,
    rendered: RefCell<Option<DisposableRef>>,
}

impl IContextViewService for RenderingContextViewService {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, _container: Option<Rc<ViewElement>>, _shadow_root: bool) {
        self.hide_context_view(None);
        self.show_count.set(self.show_count.get() + 1);
        *self.delegate.borrow_mut() = Some(delegate.clone());
        let rendered = delegate.render(&ViewElement::new());
        *self.rendered.borrow_mut() = rendered;
    }

    fn hide_context_view(&self, _data: Option<HideData>) {
        let delegate = self.delegate.borrow_mut().take();
        let rendered = self.rendered.borrow_mut().take();
        if let Some(delegate) = delegate {
            delegate.on_hide(None);
        }
        if let Some(rendered) = rendered {
            rendered.dispose();
        }
    }

    fn layout(&self) {}

    fn get_context_view_element(&self) -> Rc<ViewElement> {
        ViewElement::new()
    }
}

/// Os `mock<IContextViewService>()` literais: guarda o delegado sem renderizar e dispara `onHide` ao esconder.
struct PlainContextViewService {
    hide_previous_on_show: bool,
    show_count: Cell<usize>,
    delegate: RefCell<Option<Rc<dyn IContextViewDelegate>>>,
}

impl PlainContextViewService {
    fn new(hide_previous_on_show: bool) -> Self {
        Self { hide_previous_on_show, show_count: Cell::new(0), delegate: RefCell::new(None) }
    }
}

impl IContextViewService for PlainContextViewService {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, _container: Option<Rc<ViewElement>>, _shadow_root: bool) {
        if self.hide_previous_on_show {
            let previous = self.delegate.borrow().clone();
            if let Some(previous) = previous {
                previous.on_hide(None);
            }
        }
        self.show_count.set(self.show_count.get() + 1);
        *self.delegate.borrow_mut() = Some(delegate);
    }

    fn hide_context_view(&self, _data: Option<HideData>) {
        let delegate = self.delegate.borrow_mut().take();
        if let Some(delegate) = delegate {
            delegate.on_hide(None);
        }
    }

    fn layout(&self) {}

    fn get_context_view_element(&self) -> Rc<ViewElement> {
        ViewElement::new()
    }
}

/// O `IModelService` do `workbenchInstantiationService`: cria e acha os modelos pelo recurso.
#[derive(Default)]
struct TestModelService {
    models: RefCell<HashMap<String, Rc<TextModel>>>,
}

impl TestModelService {
    fn key(resource: &URI) -> String {
        resource.to_string(false).to_string_lossy()
    }
}

impl IModelService for TestModelService {
    fn get_model(&self, resource: &URI) -> Option<Rc<TextModel>> {
        self.models.borrow().get(&Self::key(resource)).cloned()
    }

    fn get_models(&self) -> Vec<Rc<TextModel>> {
        self.models.borrow().values().cloned().collect()
    }

    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        Event::none()
    }

    fn create_model(&self, value: TextSource, _language_selection: Option<&dyn ILanguageSelection>, resource: Option<&URI>, _is_for_simple_widget: bool) -> Rc<TextModel> {
        let model = create_text_model_with(value, None, RelaxedTextModelCreationOptions::default(), resource.cloned());
        if let Some(resource) = resource {
            self.models.borrow_mut().insert(Self::key(resource), model.clone());
        }
        model
    }

    fn update_model(&self, model: &TextModel, value: TextSource, _reason: Option<TextModelEditSource>) {
        model.set_value(value);
    }

    fn destroy_model(&self, resource: &URI) {
        let model = self.models.borrow_mut().remove(&Self::key(resource));
        if let Some(model) = model {
            model.dispose();
        }
    }

    fn get_creation_options(&self, _language: &str, _resource: Option<&URI>, _is_for_simple_widget: bool) -> TextModelCreationOptions {
        TextModel::default_creation_options()
    }
}

/// O `IMenu` dos casos de menu: `dispose() { disposeCount++; }`.
struct CountingMenu {
    dispose_count: Rc<Cell<usize>>,
}

impl IDisposable for CountingMenu {
    fn dispose(&self) {
        self.dispose_count.set(self.dispose_count.get() + 1);
    }
}

impl IMenu for CountingMenu {
    fn on_did_change(&self) -> Event<MenuChangeEvent> {
        Event::none()
    }

    fn get_actions(&self, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        Vec::new()
    }
}

/// `mock<IMenuService>()` com `createMenu(): IMenu`.
struct CountingMenuService {
    dispose_count: Rc<Cell<usize>>,
}

impl IMenuService for CountingMenuService {
    fn create_menu(&self, _id: &MenuId, _context_key_service: Rc<dyn IContextKeyService>, _options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        Rc::new(CountingMenu { dispose_count: self.dispose_count.clone() })
    }

    fn get_menu_actions(&self, _id: &MenuId, _context_key_service: &dyn IContextKeyService, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        Vec::new()
    }

    fn get_menu_contexts(&self, _id: &MenuId) -> HashSet<String> {
        HashSet::new()
    }

    fn reset_hidden_states(&self, _menu_ids: Option<&[MenuId]>) {}
}

/// O `WorkbenchToolBar` do `workbenchInstantiationService`: aceita tudo e nunca tem foco.
struct TestToolbar;

impl IAccessibleViewToolbar for TestToolbar {
    fn set_context(&self, _context: Dynamic) {}
    fn set_aria_label(&self, _label: &str) {}
    fn set_actions(&self, _actions: Vec<ActionRef>) {}
    fn is_focused(&self) -> bool {
        false
    }
    fn on_key_down(&self) -> Event<KeyboardEvent> {
        Event::none()
    }
}

/// O `IChatCodeBlockContextProviderService` dos casos: nenhum deles abre um provedor de chat.
struct NoChatCodeBlocks;

impl IChatCodeBlockContextProviderService for NoChatCodeBlocks {
    fn register_provider(&self, _provider: Rc<dyn ICodeBlockContextProvider>, _id: &str) -> DisposableRef {
        to_disposable(|| {})
    }
}

/// `TestAccessibilityService` do `workbenchInstantiationService`.
struct TestAccessibilityService;

impl IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }
    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
    fn is_screen_reader_optimized(&self) -> bool {
        false
    }
    fn is_motion_reduced(&self) -> bool {
        false
    }
    fn is_transparency_reduced(&self) -> bool {
        false
    }
    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }
    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

// endregion

// region: montagem

/// O que cada caso monta: a visão e os serviços que ele lê de volta.
struct Harness {
    view: Rc<AccessibleView>,
    /// Mantém o editor de teste vivo enquanto a visão o usa.
    _editor: Rc<TestCodeEditor>,
}

impl Harness {
    /// `workbenchInstantiationService({}, disposables)` com `IContextViewService` (e `IMenuService`) trocados, e
    /// `createInstance(AccessibleView)` somado a `disposables.add`.
    fn new(guard: &LeakGuard, context_view_service: Rc<dyn IContextViewService>, menu_service: Option<Rc<dyn IMenuService>>) -> Self {
        let editor = guard.add_rc(create_test_code_editor(None, TestCodeEditorInstantiationOptions::default()));
        let model_service = Rc::new(TestModelService::default());
        {
            let model_service = model_service.clone();
            guard.add(to_disposable(move || model_service.get_models().iter().for_each(|model| model.dispose())));
        }
        let menu_service: Rc<dyn IMenuService> = menu_service.unwrap_or_else(|| Rc::new(CountingMenuService { dispose_count: Rc::new(Cell::new(0)) }));
        let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
        let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
        // O `workbenchInstantiationService({}, disposables)` registra o `TestStorageService` em `disposables`.
        let storage_service: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
        let view = guard.add_rc(AccessibleView::new(AccessibleViewServices {
            opener_service: Rc::new(RecordingOpener::default()),
            configuration_service,
            model_service,
            context_view_service,
            context_key_service,
            accessibility_service: Rc::new(TestAccessibilityService),
            keybinding_service: Rc::new(NoKeybindingService),
            layout_service: Rc::new(TestLayoutService),
            menu_service,
            command_service: Rc::new(NoCommandService),
            code_block_context_provider_service: Rc::new(NoChatCodeBlocks),
            storage_service,
            quick_input_service: TestQuickInputService::new(),
            accessibility_signal_service: Rc::new(SilentAccessibilitySignals),
            editor: editor.widget().clone(),
            toolbar: Rc::new(TestToolbar),
        }));
        Self { view, _editor: editor }
    }
}

/// `new AccessibleContentProvider(id, { type }, () => content, () => { }, 'test.verbosity', ...)`: os argumentos opcionais ficam
/// em `args`, que o caso preenche antes de criar o provedor.
fn provider_args(id: AccessibleViewProviderId, content: &'static str) -> AccessibleContentProviderArgs {
    AccessibleContentProviderArgs::new(id, IAccessibleViewOptions::new(AccessibleViewType::View), Rc::new(move || content.to_owned()), Rc::new(|| {}), "test.verbosity")
}

/// `disposables.add(new AccessibleContentProvider(...))`.
fn add_provider(guard: &LeakGuard, args: AccessibleContentProviderArgs) -> Rc<AccessibleContentProvider> {
    guard.add_rc(AccessibleContentProvider::new(args))
}

/// O provedor de conteúdo de uma visão (`Content`).
fn content_of(provider: AccessibleViewContentProvider) -> Rc<AccessibleContentProvider> {
    match provider {
        AccessibleViewContentProvider::Content(content) => content,
        AccessibleViewContentProvider::Extension(_) => panic!("esperava um AccessibleContentProvider"),
    }
}

/// `accessibleView.show(provider, undefined, showAccessibleViewHelp)`.
fn show(view: &Rc<AccessibleView>, provider: &Rc<AccessibleContentProvider>, show_accessible_view_help: bool) {
    view.show(Some(AccessibleViewContentProvider::Content(provider.clone())), None, show_accessible_view_help, None);
}

/// O `IAccessibleViewContentProvider` escrito à mão dos casos do terminal (ver as notas do módulo).
fn terminal_provider(guard: &LeakGuard, on_did_request_clear_last_provider: &Rc<Emitter<AccessibleViewProviderId>>) -> Rc<AccessibleContentProvider> {
    let mut options = IAccessibleViewOptions::new(AccessibleViewType::View);
    options.id = Some(AccessibleViewProviderId::Terminal);
    let mut args = provider_args(AccessibleViewProviderId::Terminal, "content");
    args.options = options;
    args.on_did_request_clear_last_provider = Some(on_did_request_clear_last_provider.event());
    add_provider(guard, args)
}

// endregion

fn notification_help_describes_links_conditionally(has_links: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_view_service = Rc::new(RenderingContextViewService::default());
    let harness = Harness::new(&guard, context_view_service.clone(), None);
    let mut args = provider_args(AccessibleViewProviderId::Notification, "Synthetic notification");
    args.actions = Some(if has_links {
        vec![to_action(ToActionProps {
            id: "notification.link.0".to_owned(),
            label: "Learn More".to_owned(),
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(|_| MaybePromise::Value(Dynamic::Undefined)),
        })]
    } else {
        Vec::new()
    });
    let provider = add_provider(&guard, args);

    show(&harness.view, &provider, false);
    harness.view.show_accessible_view_help();
    run_microtasks();

    let value = harness.view.editor_widget().get_value(None).to_string_lossy();
    assert!(value.contains("If a notification contains links, they are available in the toolbar."));
    let help_provider = content_of(harness.view.current_provider().expect("a ajuda está aberta"));
    guard.add_rc(help_provider.clone());
    (help_provider.on_close)();
    run_microtasks();
    context_view_service.hide_context_view(None);
}

#[test]
fn notification_help_describes_links_conditionally_links_false() {
    notification_help_describes_links_conditionally(false);
}

#[test]
fn notification_help_describes_links_conditionally_links_true() {
    notification_help_describes_links_conditionally(true);
}

#[test]
fn disposes_toolbar_menus_when_they_are_replaced_and_when_the_view_is_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let dispose_count = Rc::new(Cell::new(0));
    let menu_service: Rc<dyn IMenuService> = Rc::new(CountingMenuService { dispose_count: dispose_count.clone() });
    let harness = Harness::new(&guard, Rc::new(PlainContextViewService::new(false)), Some(menu_service));

    harness.view.update_toolbar(None, None);
    harness.view.update_toolbar(None, None);
    assert_eq!(dispose_count.get(), 1);

    harness.view.dispose();
    assert_eq!(dispose_count.get(), 2);
}

#[test]
fn disposes_the_toolbar_menu_when_the_context_view_hides() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let dispose_count = Rc::new(Cell::new(0));
    let context_view_service = Rc::new(PlainContextViewService::new(false));
    let menu_service: Rc<dyn IMenuService> = Rc::new(CountingMenuService { dispose_count: dispose_count.clone() });
    let harness = Harness::new(&guard, context_view_service.clone(), Some(menu_service));
    let provider = add_provider(&guard, provider_args(AccessibleViewProviderId::Editor, "content"));

    harness.view.update_toolbar(None, None);

    show(&harness.view, &provider, true);
    assert_eq!(dispose_count.get(), 0);

    context_view_service.hide_context_view(None);
    assert_eq!(dispose_count.get(), 1);

    harness.view.dispose();
    assert_eq!(dispose_count.get(), 1);
}

#[test]
fn releases_provider_listeners_when_the_context_view_hides() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_view_service = Rc::new(PlainContextViewService::new(true));
    let harness = Harness::new(&guard, context_view_service.clone(), None);

    let listener_count = Rc::new(Cell::new(0usize));
    let on_did_change_content = guard.add_rc(Emitter::<()>::new());
    let counting_event: Event<()> = {
        let listener_count = listener_count.clone();
        let on_did_change_content = on_did_change_content.clone();
        Event::new(move |listener| {
            listener_count.set(listener_count.get() + 1);
            let listener_disposable = on_did_change_content.event().listen_rc(listener);
            let listener_count = listener_count.clone();
            to_disposable(move || {
                listener_count.set(listener_count.get() - 1);
                listener_disposable.dispose();
            })
        })
    };
    let mut args = provider_args(AccessibleViewProviderId::Editor, "content");
    args.on_did_change_content = Some(counting_event);
    let provider = add_provider(&guard, args);

    let mut counts = Vec::new();
    show(&harness.view, &provider, true);
    counts.push(listener_count.get());
    show(&harness.view, &provider, true);
    counts.push(listener_count.get());
    context_view_service.hide_context_view(None);
    counts.push(listener_count.get());

    assert_eq!(counts, vec![1, 1, 0]);
}

#[test]
fn forgets_a_directly_implemented_last_provider_when_it_requests_to_be_cleared() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_view_service = Rc::new(PlainContextViewService::new(false));
    let harness = Harness::new(&guard, context_view_service.clone(), None);

    let on_did_request_clear_last_provider = guard.add_rc(Emitter::<AccessibleViewProviderId>::new());
    let provider = terminal_provider(&guard, &on_did_request_clear_last_provider);

    show(&harness.view, &provider, true);
    context_view_service.hide_context_view(None);
    harness.view.show_last_provider(AccessibleViewProviderId::Terminal);
    let shows_before_clear = context_view_service.show_count.get();

    on_did_request_clear_last_provider.fire(AccessibleViewProviderId::Terminal);
    harness.view.show_last_provider(AccessibleViewProviderId::Terminal);

    assert_eq!((shows_before_clear, context_view_service.show_count.get()), (2, 2));
}

#[test]
fn recreates_on_open_resources_for_re_shown_providers_and_providers_restored_from_help() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_view_service = Rc::new(RenderingContextViewService::default());
    let harness = Harness::new(&guard, context_view_service.clone(), None);

    let close_event = guard.add_rc(Emitter::<()>::new());
    let calls = Rc::new(Cell::new(0usize));
    let mut args = provider_args(AccessibleViewProviderId::Notification, "Synthetic notification");
    args.on_open = Some({
        let close_event = close_event.clone();
        let calls = calls.clone();
        Rc::new(move || {
            let calls = calls.clone();
            Some(close_event.event().listen(move |_| calls.set(calls.get() + 1)))
        })
    });
    let provider = add_provider(&guard, args);

    let mut counts = Vec::new();
    show(&harness.view, &provider, false);
    close_event.fire(());
    counts.push(calls.get());
    show(&harness.view, &provider, false);
    close_event.fire(());
    counts.push(calls.get());
    harness.view.show_accessible_view_help();
    run_microtasks();
    close_event.fire(());
    counts.push(calls.get());
    let help_provider = guard.add_rc(content_of(harness.view.current_provider().expect("a ajuda está aberta")));
    (help_provider.on_close)();
    run_microtasks();
    close_event.fire(());
    counts.push(calls.get());
    context_view_service.hide_context_view(None);
    close_event.fire(());
    counts.push(calls.get());

    assert_eq!(counts, vec![1, 2, 2, 3, 3]);
}

#[test]
fn releases_the_listeners_of_every_render_when_the_view_hides() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_view_service = Rc::new(RenderingContextViewService::default());
    let harness = Harness::new(&guard, context_view_service.clone(), None);

    let on_did_change_content = guard.add_rc(Emitter::<()>::new());
    let mut args = provider_args(AccessibleViewProviderId::Editor, "content");
    args.on_did_change_content = Some(on_did_change_content.event());
    let provider = add_provider(&guard, args);

    let editor = harness.view.editor_widget().clone();
    // Attach the text model first, editor contributions add their own listeners when it's attached
    show(&harness.view, &provider, true);
    run_microtasks();
    context_view_service.hide_context_view(None);
    let baseline = editor.key_down_listener_count();

    show(&harness.view, &provider, true);
    on_did_change_content.fire(());
    on_did_change_content.fire(());
    run_microtasks();
    let added_while_shown = editor.key_down_listener_count() - baseline;
    context_view_service.hide_context_view(None);
    run_microtasks();

    assert_eq!((added_while_shown, editor.key_down_listener_count() - baseline), (1, 0));
}

#[test]
fn a_last_provider_restored_from_accessibility_help_still_honors_clear_requests() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_view_service = Rc::new(RenderingContextViewService::default());
    let harness = Harness::new(&guard, context_view_service.clone(), None);

    let on_did_request_clear_last_provider = guard.add_rc(Emitter::<AccessibleViewProviderId>::new());
    let provider = terminal_provider(&guard, &on_did_request_clear_last_provider);

    show(&harness.view, &provider, true);
    // Accessibility help shows a copy of the current provider again once it closes
    let restored_provider = guard.add_rc(content_of(harness.view.update_last_provider().expect("há um provedor atual")));
    context_view_service.hide_context_view(None);
    show(&harness.view, &restored_provider, true);
    run_microtasks();
    context_view_service.hide_context_view(None);
    let shows_before_clear = context_view_service.show_count.get();

    on_did_request_clear_last_provider.fire(AccessibleViewProviderId::Terminal);
    harness.view.show_last_provider(AccessibleViewProviderId::Terminal);

    assert_eq!((shows_before_clear, context_view_service.show_count.get()), (2, 2));
}

#[test]
fn does_not_restore_a_provider_that_requested_to_be_cleared_while_accessibility_help_was_open() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_view_service = Rc::new(RenderingContextViewService::default());
    let harness = Harness::new(&guard, context_view_service.clone(), None);

    let on_did_request_clear_last_provider = guard.add_rc(Emitter::<AccessibleViewProviderId>::new());
    let provider = terminal_provider(&guard, &on_did_request_clear_last_provider);

    show(&harness.view, &provider, true);
    harness.view.show_accessible_view_help();
    run_microtasks();
    let help_provider = guard.add_rc(content_of(harness.view.current_provider().expect("a ajuda está aberta")));
    let shows_with_help = context_view_service.show_count.get();

    on_did_request_clear_last_provider.fire(AccessibleViewProviderId::Terminal);
    (help_provider.on_close)();
    run_microtasks();
    harness.view.show_last_provider(AccessibleViewProviderId::Terminal);

    assert_eq!((shows_with_help, context_view_service.show_count.get()), (2, 2));
}
