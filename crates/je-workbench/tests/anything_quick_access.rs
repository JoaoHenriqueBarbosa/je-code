//! Porte de `src/vs/workbench/contrib/search/test/browser/anythingQuickAccess.test.ts` (suíte `AnythingQuickAccess - links`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite`.** O guarda de vazamentos é o [`LeakGuard`] criado no começo de cada
//!   caso. O `workbenchInstantiationService(undefined, store)` com os `stub` do original é o [`AnythingFixture`]
//!   (`tests/support/anything_quick_access_support.rs`): serviços reais onde o porte os tem e dublês para o resto. O
//!   `createQuickPick` do `IQuickInputService` real é o [`TestQuickPick`] (`tests/support/test_quick_input.rs`).
//! - **`await`.** Cada `await pick.accept?.(...)` esvazia as microtarefas (`runtime::run_microtasks`) e os que esperam a busca de
//!   arquivos (o atraso de 200 ms do `fileQueryDelayer`) esvaziam o laço de eventos (`runtime::run_until_idle`).
//! - **`TestAnythingQuickAccessProvider`.** Não há herança: `getPicks(filter, options)` chama o `_getPicks` do provedor
//!   ([`IPickerQuickAccessSource::get_picks`]) com um store novo e `CancellationToken.None`. O `EmptyAnythingQuickAccessProvider`
//!   (que sobrescreve `_getPicks`) é o provedor com `override_get_picks` apontando a [`EmptySource`].
//! - **`Array.isArray(picks)`.** É a variante [`ProvidedPicks::Picks`] com a lista; `null` é [`ProvidedPicks::None`] e a promessa é
//!   [`ProvidedPicks::Promised`].
//! - **`pick.accept?.(...)`, `pick.trigger?.(...)`, `pick.detail`, `pick.buttons`.** `accept` e `trigger` são os
//!   [`PickerItemHandlers`] do item; a ausência dos dois (o link inválido) é um item sem `PickerItemHandlers`.
//! - **Abridor externo (`registerExternalOpener`).** O `OpenerService` real do porte, com um `ICodeEditorService` que não abre
//!   nada e o `NullCommandService`; o contexto do abridor externo traz `sourceUri` e `viewColumn`.
//! - **`for ... of` do original.** Cada iteração é um `#[test]` próprio, gerado pelos macros do arquivo.
//! - **`setUserConfiguration`.** Resolve na hora (o `TestConfigurationService` do porte é síncrono).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/null_command_service.rs"]
mod null_command_service;
#[allow(dead_code)]
#[path = "support/anything_quick_access_support.rs"]
mod anything_quick_access_support;
#[allow(dead_code)]
#[path = "support/test_quick_input.rs"]
mod test_quick_input;

use std::cell::RefCell;
use std::rc::Rc;

use anything_quick_access_support::{AnythingFixture, OpenedLink, RecordedNotification};
use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::opener_service::{CodeEditorHandle, ICodeEditorService, OpenerService, TextResourceEditorInput};
use je_platform::opener::editor_types::ViewColumn;
use je_platform::opener::opener::{ExternalOpenerContext, IExternalOpener, IOpenerService};
use je_platform::quickinput::picker_quick_access::{
    IPickerQuickAccessSource, Picks, PickerItemHandlers, PromisedPicks, ProvidedPicks, TriggerAction, TriggerResult,
};
use je_platform::quickinput::quick_input::{
    IKeyMods, IQuickInputService, IQuickPick, IQuickPickDidAcceptEvent, IQuickPickItem, QuickPickCreateOptions, QuickPickInput, NO_KEY_MODS,
};
use je_workbench::contrib::search::browser::anything_quick_access::{
    AnythingQuickAccessProvider, IAnythingQuickAccessFileSearchDelegate, IAnythingQuickAccessProviderRunOptions,
};
use test_quick_input::TestQuickInputService;

/// `getPicks(filter, options)` do `TestAnythingQuickAccessProvider`.
fn get_picks(provider: &Rc<AnythingQuickAccessProvider>, store: &Rc<DisposableStore>, filter: &str, options: Option<IAnythingQuickAccessProviderRunOptions>) -> ProvidedPicks {
    let disposables = DisposableStore::new();
    store.add_rc(disposables.clone());
    provider.get_picks(filter, &disposables, CancellationToken::None, options.map(Rc::new).as_ref())
}

/// `getLinkPick(filter)`.
fn get_link_pick(provider: &Rc<AnythingQuickAccessProvider>, store: &Rc<DisposableStore>, filter: &str) -> IQuickPickItem {
    let picks = get_picks(provider, store, filter, None);
    let ProvidedPicks::Picks(Picks::Items(items)) = picks else {
        panic!("o provedor devolve uma lista de itens para um link");
    };
    assert_eq!(items.len(), 1);
    match items.into_iter().next() {
        Some(QuickPickInput::Item(item)) => item,
        _ => panic!("o único resultado é um item"),
    }
}

/// `pick.accept?.(keyMods, { inBackground })` seguido do `await`.
fn accept(pick: &IQuickPickItem, key_mods: IKeyMods, in_background: bool) {
    if let Some(accept) = PickerItemHandlers::of(pick).and_then(|handlers| handlers.accept.clone()) {
        accept(&key_mods, &IQuickPickDidAcceptEvent { in_background });
    }
    runtime::run_microtasks();
}

fn alt() -> IKeyMods {
    IKeyMods { alt: true, ..NO_KEY_MODS }
}

fn ctrl_cmd() -> IKeyMods {
    IKeyMods { ctrl_cmd: true, ..NO_KEY_MODS }
}

fn link_open(resource: &str, open_to_side: bool, preserve_focus: bool, pinned: bool) -> OpenedLink {
    OpenedLink {
        resource: resource.to_string(),
        from_user_gesture: Some(true),
        allow_contributed_openers: Some(true),
        open_to_side: Some(open_to_side),
        preserve_focus: Some(preserve_focus),
        pinned: Some(pinned),
    }
}

struct Setup {
    _guard: LeakGuard,
    store: Rc<DisposableStore>,
    fixture: AnythingFixture,
    provider: Rc<AnythingQuickAccessProvider>,
    quick_input_service: Rc<TestQuickInputService>,
}

fn setup() -> Setup {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let fixture = AnythingFixture::new(&store);
    let provider = store.add_rc(fixture.create_provider());
    Setup { _guard: guard, store, fixture, provider, quick_input_service: TestQuickInputService::new() }
}

fn create_picker(t: &Setup) -> Rc<dyn IQuickPick> {
    let picker = t.quick_input_service.create_quick_pick(Some(QuickPickCreateOptions { use_separators: true }));
    t.store.add_rc(Rc::new(PickerDisposer(picker.clone())));
    picker
}

/// `store.add(picker)`.
struct PickerDisposer(Rc<dyn IQuickPick>);

impl IDisposable for PickerDisposer {
    fn dispose(&self) {
        self.0.dispose();
    }
}

/// O `_getPicks` do `EmptyAnythingQuickAccessProvider`: `[]`.
struct EmptySource;

impl IPickerQuickAccessSource<IAnythingQuickAccessProviderRunOptions> for EmptySource {
    fn get_picks(&self, _filter: &str, _disposables: &Rc<DisposableStore>, _token: CancellationToken, _run_options: Option<&Rc<IAnythingQuickAccessProviderRunOptions>>) -> ProvidedPicks {
        ProvidedPicks::Picks(Picks::Items(Vec::new()))
    }
}

fn empty_provider(t: &Setup) -> Rc<AnythingQuickAccessProvider> {
    let provider = t.store.add_rc(t.fixture.create_provider());
    provider.override_get_picks(Rc::new(EmptySource));
    provider
}

// ---------------------------------------------------------------------------------------------------------
// opens through shared handlers without changing the link
// ---------------------------------------------------------------------------------------------------------

fn opens_through_shared_handlers(link: &str) {
    let t = setup();
    let pick = get_link_pick(&t.provider, &t.store, link);
    accept(&pick, NO_KEY_MODS, false);
    assert_eq!(*t.fixture.opener.opened.borrow(), vec![link_open(link, false, false, false)]);
}

macro_rules! link_cases {
    ($helper:ident: $($name:ident => $link:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                $helper($link);
            }
        )*
    };
}

link_cases! { opens_through_shared_handlers:
    opens_through_shared_handlers_without_changing_the_link_github_pull => "https://github.com/microsoft/vscode/pull/123?view=files#diff-123",
    opens_through_shared_handlers_without_changing_the_link_port_and_range => "https://example.com:8443/path#L12,3-L14,5",
    opens_through_shared_handlers_without_changing_the_link_user_info => "https://user@example.com/path",
    opens_through_shared_handlers_without_changing_the_link_file => "file:///workspace/file.ts#L12",
    opens_through_shared_handlers_without_changing_the_link_vscode => "vscode://publisher.extension/open?id=123",
    opens_through_shared_handlers_without_changing_the_link_custom_editor => "custom-editor://session/123",
}

#[test]
fn alt_enter_opens_beside_the_originating_group_even_after_the_active_group_changes() {
    let t = setup();
    let pick = get_link_pick(&t.provider, &t.store, "https://example.com");
    *t.fixture.groups.active_group.borrow_mut() = t.fixture.groups.other_group.clone();
    accept(&pick, alt(), false);
    let opened = t.fixture.opener.opened.borrow();
    assert_eq!(*t.fixture.groups.activated_groups.borrow(), vec![1]);
    assert_eq!(opened[0], link_open("https://example.com", true, false, false));
    assert_eq!(pick.detail.as_deref(), Some("Hold Alt while accepting to open to the side."));
    assert_eq!(pick.base.buttons.as_ref().and_then(|buttons| buttons[0].tooltip.clone()).as_deref(), Some("Open to the Side"));
}

#[test]
fn side_button_uses_shared_handlers_and_closes_the_picker() {
    let t = setup();
    let pick = get_link_pick(&t.provider, &t.store, "https://example.com");
    let trigger = PickerItemHandlers::of(&pick).and_then(|handlers| handlers.trigger.clone()).expect("o link tem gatilho");
    let result = trigger(0, &NO_KEY_MODS);
    runtime::run_microtasks();
    let action = match result {
        TriggerResult::Now(action) => action,
        TriggerResult::Later(promise) => promise.result().expect("o gatilho resolveu").expect("sem erro"),
    };
    assert_eq!(action, TriggerAction::ClosePicker);
    assert_eq!(t.fixture.opener.opened.borrow()[0].open_to_side, Some(true));
}

#[test]
fn renders_the_link_icon_inline_rather_than_applying_icon_font_classes_to_the_row() {
    let t = setup();
    let pick = get_link_pick(&t.provider, &t.store, "https://example.com");
    assert_eq!(pick.base.label, "$(link-external) Open Link");
    assert_eq!(pick.base.icon_classes, None);
}

/// O abridor externo contribuído do caso (`registerExternalOpener`).
struct RecordingExternalOpener {
    targets: Rc<RefCell<Vec<(String, String, Option<ViewColumn>)>>>,
}

impl IExternalOpener for RecordingExternalOpener {
    fn open_external(&self, href: &JsString, ctx: &ExternalOpenerContext, _token: &CancellationToken) -> Promise<bool> {
        self.targets.borrow_mut().push((href.to_string_lossy(), ctx.source_uri.to_string(false).to_string_lossy(), ctx.view_column));
        Promise::resolved(true)
    }
}

/// O `ICodeEditorService` do `workbenchInstantiationService`, sem editores.
struct NoCodeEditors;

impl ICodeEditorService for NoCodeEditors {
    fn get_focused_code_editor(&self) -> Option<CodeEditorHandle> {
        None
    }

    fn open_code_editor(&self, _input: TextResourceEditorInput, _source: Option<CodeEditorHandle>, _side_by_side: Option<bool>) -> Promise<Option<CodeEditorHandle>> {
        Promise::resolved(None)
    }
}

#[test]
fn side_opening_intent_reaches_a_contributed_external_uri_handler() {
    let mut t = setup();
    let opener_service = Rc::new(OpenerService::new(Rc::new(NoCodeEditors), Rc::new(null_command_service::NullCommandService)));
    let targets = Rc::new(RefCell::new(Vec::new()));
    let registration = opener_service.register_external_opener(Rc::new(RecordingExternalOpener { targets: targets.clone() }));
    t.store.add(registration);
    let opener_dyn: Rc<dyn IOpenerService> = opener_service;
    t.fixture.stub_opener_service(opener_dyn);
    let provider = t.store.add_rc(t.fixture.create_provider());
    let link = "https://github.com/microsoft/vscode/pull/123?view=files#diff-123";
    let pick = get_link_pick(&provider, &t.store, link);
    accept(&pick, alt(), false);
    assert_eq!(*targets.borrow(), vec![(link.to_string(), URI::parse(link, false).to_string(false).to_string_lossy(), Some(ViewColumn::Beside))]);
}

#[test]
fn respects_preview_configuration_and_the_existing_ctrl_cmd_side_opening_gesture() {
    let t = setup();
    let _ = t.fixture.configuration_service.set_user_configuration(
        "workbench",
        Dynamic::object([(
            "editor",
            Dynamic::object([
                ("enablePreview", Dynamic::Bool(false)),
                ("enablePreviewFromQuickOpen", Dynamic::Bool(true)),
                ("openSideBySideDirection", Dynamic::string("right")),
            ]),
        )]),
        None,
    );
    let pick = get_link_pick(&t.provider, &t.store, "https://example.com");
    accept(&pick, ctrl_cmd(), false);
    let opened = t.fixture.opener.opened.borrow();
    assert_eq!(opened[0].open_to_side, Some(true));
    assert_eq!((opened[0].from_user_gesture, opened[0].allow_contributed_openers), (Some(true), Some(true)));
    assert_eq!((opened[0].preserve_focus, opened[0].pinned), (Some(false), Some(true)));
}

#[test]
fn background_acceptance_preserves_focus_and_pins_the_editor() {
    let t = setup();
    let pick = get_link_pick(&t.provider, &t.store, "https://example.com");
    accept(&pick, NO_KEY_MODS, true);
    let opened = t.fixture.opener.opened.borrow();
    assert_eq!((opened[0].preserve_focus, opened[0].pinned), (Some(true), Some(true)));
}

// ---------------------------------------------------------------------------------------------------------
// preserves file queries and Quick Access prefixes
// ---------------------------------------------------------------------------------------------------------

fn preserves_file_queries(filter: &str) {
    let t = setup();
    let picks = get_picks(&t.provider, &t.store, filter, None);
    assert!(!matches!(picks, ProvidedPicks::Picks(_)));
    // O trabalho assíncrono dos resultados lentos termina antes de o guarda conferir os vazamentos.
    runtime::run_until_idle();
}

link_cases! { preserves_file_queries:
    preserves_file_queries_and_quick_access_prefixes_readme => "readme.md",
    preserves_file_queries_and_quick_access_prefixes_file_with_range => "src/file.ts:12:3",
    preserves_file_queries_and_quick_access_prefixes_file_with_symbol => "file.ts@symbol",
    preserves_file_queries_and_quick_access_prefixes_drive_letter_slash => "C:/src/file.ts",
    preserves_file_queries_and_quick_access_prefixes_drive_letter_backslash => "C:\\src\\file.ts",
    preserves_file_queries_and_quick_access_prefixes_command => ">command",
    preserves_file_queries_and_quick_access_prefixes_symbol => "@symbol",
    preserves_file_queries_and_quick_access_prefixes_line => ":12",
    preserves_file_queries_and_quick_access_prefixes_host => "example.com",
}

// ---------------------------------------------------------------------------------------------------------
// reports an invalid or disallowed link without an open action
// ---------------------------------------------------------------------------------------------------------

fn reports_an_invalid_link(link: &str) {
    let t = setup();
    let pick = get_link_pick(&t.provider, &t.store, link);
    assert_eq!(pick.base.label, "Invalid Link");
    assert!(PickerItemHandlers::of(&pick).is_none(), "sem accept e sem trigger");
}

link_cases! { reports_an_invalid_link:
    reports_an_invalid_or_disallowed_link_without_an_open_action_empty_authority => "https://",
    reports_an_invalid_or_disallowed_link_without_an_open_action_missing_host => "https:///path",
    reports_an_invalid_or_disallowed_link_without_an_open_action_bad_ipv6 => "https://[invalid",
    reports_an_invalid_or_disallowed_link_without_an_open_action_whitespace => "https://example.com/not a link",
    reports_an_invalid_or_disallowed_link_without_an_open_action_command => "command://run",
    reports_an_invalid_or_disallowed_link_without_an_open_action_javascript => "javascript://run",
    reports_an_invalid_or_disallowed_link_without_an_open_action_data => "data://payload",
}

#[test]
fn reports_an_invalid_or_disallowed_link_without_an_open_action_internal_scheme() {
    reports_an_invalid_link(&format!("{}://target", schemas::INTERNAL));
}

#[test]
fn respects_caller_filtering() {
    let t = setup();
    let options = IAnythingQuickAccessProviderRunOptions { filter: Some(Rc::new(|_| false)), ..IAnythingQuickAccessProviderRunOptions::default() };
    let picks = get_picks(&t.provider, &t.store, "https://example.com", Some(options));
    assert!(matches!(picks, ProvidedPicks::Picks(Picks::Items(ref items)) if items.is_empty()));
}

#[test]
fn file_only_and_delegated_pickers_do_not_offer_link_actions() {
    let t = setup();
    let files_only = get_picks(
        &t.provider,
        &t.store,
        "https://example.com",
        Some(IAnythingQuickAccessProviderRunOptions { files_only: Some(true), ..IAnythingQuickAccessProviderRunOptions::default() }),
    );
    let delegated = get_picks(
        &t.provider,
        &t.store,
        "https://example.com",
        Some(IAnythingQuickAccessProviderRunOptions {
            file_search_delegate: Some(IAnythingQuickAccessFileSearchDelegate {
                search: Rc::new(|_, _| {
                    Promise::resolved(je_workbench::services::search::search::ISearchComplete {
                        limit_hit: None,
                        messages: Vec::new(),
                        stats: None,
                        results: Vec::new(),
                        exit: None,
                        ai_keywords: None,
                    })
                }),
                get_item_description: None,
            }),
            ..IAnythingQuickAccessProviderRunOptions::default()
        }),
    );
    let ProvidedPicks::Promised(files_only) = files_only else {
        panic!("a busca só de arquivos é uma promessa");
    };
    runtime::run_until_idle();
    let files_only = files_only.result().expect("a busca resolveu").expect("sem erro");
    assert!(matches!(files_only, PromisedPicks::Picks(Picks::Items(ref items)) if items.is_empty()));
    assert!(!matches!(delegated, ProvidedPicks::Picks(_)));
    runtime::run_until_idle();
}

// ---------------------------------------------------------------------------------------------------------
// refreshes file picks after {before} -> {link} -> {after}
// ---------------------------------------------------------------------------------------------------------

fn refreshes_file_picks(before: &str, link: &str, after: &str) {
    let t = setup();
    get_picks(&t.provider, &t.store, before, None);
    get_link_pick(&t.provider, &t.store, link);
    let picks = get_picks(&t.provider, &t.store, after, None);
    assert!(!matches!(picks, ProvidedPicks::None));
    assert!(!matches!(picks, ProvidedPicks::Picks(_)), "Returning to a file query must replace the link pick");
    assert!(matches!(get_picks(&t.provider, &t.store, "readme.md:13", None), ProvidedPicks::None), "Range-only changes should still preserve file picks");
    runtime::run_until_idle();
}

macro_rules! refresh_cases {
    ($($name:ident => ($before:expr, $link:expr, $after:expr)),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                refreshes_file_picks($before, $link, $after);
            }
        )*
    };
}

refresh_cases! {
    refreshes_file_picks_after_readme_md_link_example_com_readme_md_12 => ("readme.md", "https://example.com", "readme.md:12"),
    refreshes_file_picks_after_readme_md_link_https_readme_md_12 => ("readme.md", "https://", "readme.md:12"),
    refreshes_file_picks_after_readme_md_12_link_example_com_readme_md => ("readme.md:12", "https://example.com", "readme.md"),
    refreshes_file_picks_after_readme_md_12_link_https_readme_md => ("readme.md:12", "https://", "readme.md"),
    refreshes_file_picks_after_readme_md_link_example_com_readme_md_again => ("readme.md", "https://example.com", "readme.md"),
    refreshes_file_picks_after_readme_md_link_https_readme_md_again => ("readme.md", "https://", "readme.md"),
}

#[test]
fn refreshes_file_picks_after_a_link_was_filtered_out() {
    let t = setup();
    get_picks(&t.provider, &t.store, "readme.md", None);
    let filtered = get_picks(
        &t.provider,
        &t.store,
        "https://example.com",
        Some(IAnythingQuickAccessProviderRunOptions { filter: Some(Rc::new(|_| false)), ..IAnythingQuickAccessProviderRunOptions::default() }),
    );
    assert!(matches!(filtered, ProvidedPicks::Picks(Picks::Items(ref items)) if items.is_empty()));
    assert!(!matches!(get_picks(&t.provider, &t.store, "readme.md:12", None), ProvidedPicks::None));
    runtime::run_until_idle();
}

// ---------------------------------------------------------------------------------------------------------
// does not require an active editor group for a specialized picker
// ---------------------------------------------------------------------------------------------------------

#[test]
fn does_not_require_an_active_editor_group_for_a_specialized_picker_files_only() {
    let t = setup();
    let picker = create_picker(&t);
    let provider = empty_provider(&t);
    t.fixture.groups.allow_active_group_access.set(false);
    t.store.add(provider.provide(picker, CancellationToken::None, Some(Rc::new(IAnythingQuickAccessProviderRunOptions { files_only: Some(true), ..IAnythingQuickAccessProviderRunOptions::default() }))));
}

#[test]
fn does_not_require_an_active_editor_group_for_a_specialized_picker_file_search_delegate() {
    let t = setup();
    let picker = create_picker(&t);
    let provider = empty_provider(&t);
    t.fixture.groups.allow_active_group_access.set(false);
    let delegate = IAnythingQuickAccessFileSearchDelegate {
        search: Rc::new(|_, _| {
            Promise::resolved(je_workbench::services::search::search::ISearchComplete {
                limit_hit: None,
                messages: Vec::new(),
                stats: None,
                results: Vec::new(),
                exit: None,
                ai_keywords: None,
            })
        }),
        get_item_description: None,
    };
    t.store.add(provider.provide(
        picker,
        CancellationToken::None,
        Some(Rc::new(IAnythingQuickAccessProviderRunOptions { file_search_delegate: Some(delegate), ..IAnythingQuickAccessProviderRunOptions::default() })),
    ));
}

#[test]
fn does_not_require_an_active_editor_group_for_a_specialized_picker_handle_accept() {
    let t = setup();
    let picker = create_picker(&t);
    let provider = empty_provider(&t);
    t.fixture.groups.allow_active_group_access.set(false);
    let mut options = IAnythingQuickAccessProviderRunOptions::default();
    options.base.handle_accept = Some(Rc::new(|_, _| {}));
    t.store.add(provider.provide(picker, CancellationToken::None, Some(Rc::new(options))));
}

#[test]
fn does_not_require_an_active_editor_group_for_quick_navigation() {
    let t = setup();
    let picker = create_picker(&t);
    let provider = empty_provider(&t);
    picker.set_quick_navigate(Some(je_platform::quickinput::quick_input::IQuickNavigateConfiguration { keybindings: Vec::new() }));
    t.fixture.groups.allow_active_group_access.set(false);
    t.store.add(provider.provide(picker, CancellationToken::None, None));
}

#[test]
fn reports_unhandled_links() {
    let t = setup();
    t.fixture.opener.handled.set(false);
    let pick = get_link_pick(&t.provider, &t.store, "unsupported://target");
    accept(&pick, NO_KEY_MODS, false);
    assert_eq!(
        *t.fixture.notifications.notifications.borrow(),
        vec![RecordedNotification::Text("The link could not be opened. It may be unsupported or opening it was canceled.".to_string())]
    );
}

#[test]
fn reports_opener_errors() {
    let t = setup();
    let open_error = Error::new("Failed to open link");
    *t.fixture.opener.open_error.borrow_mut() = Some(open_error.clone());
    let pick = get_link_pick(&t.provider, &t.store, "https://example.com");
    accept(&pick, NO_KEY_MODS, false);
    assert_eq!(*t.fixture.notifications.notifications.borrow(), vec![RecordedNotification::Error(open_error)]);
}

#[test]
fn captures_the_source_group_when_the_picker_is_provided_not_when_the_link_is_typed() {
    let t = setup();
    let picker = create_picker(&t);
    picker.set_value("https://example.com".to_string());
    t.store.add(t.provider.provide(picker, CancellationToken::None, None));
    *t.fixture.groups.active_group.borrow_mut() = t.fixture.groups.other_group.clone();
    let pick = get_link_pick(&t.provider, &t.store, "https://example.com/another");
    accept(&pick, alt(), false);
    assert_eq!(*t.fixture.groups.activated_groups.borrow(), vec![1]);
}
