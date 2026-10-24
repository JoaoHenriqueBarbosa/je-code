//! Porte de `src/vs/workbench/api/test/browser/extHostTimeline.test.ts` (suíte `ExtHostTimeline`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; os casos rodam em
//!   `run_with_faked_timers` porque aguardam `$getTimeline`.
//! - `mock<MainThreadTimelineShape>()` com `SingleProxyRPCProtocol` é o [`MockProxy`] (seis métodos, os de registro vazios
//!   e os de evento gravando em `calls`) mais `single_proxy_rpc_protocol`. `mock<ExtHostCommands>()` com
//!   `registerArgumentProcessor` é o [`CapturingCommands`], que implementa [`ArgumentProcessorRegistry`] (a fatia de
//!   `ExtHostCommands` que o `ExtHostTimeline` usa). `mock<CommandsConverter>()` é o [`NoConverter`] (qualquer chamada
//!   falha, como o método ausente do objeto vazio) e o `toInternal` do terceiro caso é o [`InternalConverter`].
//! - Os itens, o `iconPath`, o `tooltip` e o `accessibilityInformation` são `Dynamic` (ver `ext_host_values`): `===` é
//!   [`Dynamic::strict_equals`], `Object.isFrozen` é [`is_frozen`], `{ ...copy }` é [`spread`] (as propriedades próprias
//!   sem o `$$class`) e `assert.deepStrictEqual` é `assert_deep_strict_equal`. O `appendMarkdown(' changed')` do quarto caso
//!   é a atribuição de `value` da cópia do `MarkdownString`, que é o que o método faz (não há protótipo no `Dynamic`).
//! - `try { change() } catch { /* frozen */ }` é [`try_change`] (`errors::catch`): escrever num objeto congelado lança.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Emitter;
use je_base::html_content::{IsTrusted, MarkdownStringTrustedOptions};
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::marshalling_ids::MarshalledId;
use je_base::promise::{DeferredPromise, MaybePromise};
use je_base::themables::{ThemeColor, ThemeIcon};
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_workbench::api::common::ext_host_commands::ArgumentProcessor;
use je_workbench::api::common::ext_host_protocol::{ExtHostTimelineShape, ICommandDto, IMainContext, MainThreadTimelineShape};
use je_workbench::api::common::ext_host_timeline::{
    ApiTimeline, ApiTimelineChange, ApiTimelineOptions, ApiTimelineProvider, ApiWorkspaceTimelineProvider, ArgumentProcessorRegistry, ExtHostTimeline,
};
use je_workbench::api::common::ext_host_type_converters::ICommandsConverter;
use je_workbench::api::common::ext_host_types::{self as types, new_timeline_item, Command};
use je_workbench::api::common::ext_host_values::{ApiValue, CLASS_KEY};
use je_workbench::contrib::timeline::common::timeline::{
    TimelineOptions, TimelineProviderDescriptor, TimelineScheme, TimelineTriggerKind, WorkspaceTimelineProviderDescriptor,
};
use je_workbench::services::extensions::extensions::null_extension_description;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

// ---- os dublês --------------------------------------------------------------------------------------

/// O que `$emitTimelineChangeEvent` e `$emitWorkspaceTimelineChangeEvent` gravam em `calls`.
#[derive(Clone, Debug, PartialEq)]
enum Call {
    Change(String, Option<Vec<String>>),
    WorkspaceChange(String),
}

/// `new class extends mock<MainThreadTimelineShape>() { ... }`.
struct MockProxy {
    calls: Rc<RefCell<Vec<Call>>>,
}

impl IDisposable for MockProxy {
    fn dispose(&self) {}
}

impl MainThreadTimelineShape for MockProxy {
    fn rpc_register_timeline_provider(&self, _provider: TimelineProviderDescriptor) {}

    fn rpc_unregister_timeline_provider(&self, _source: &str) {}

    fn rpc_register_workspace_timeline_provider(&self, _provider: WorkspaceTimelineProviderDescriptor) {}

    fn rpc_unregister_workspace_timeline_provider(&self, _source: &str) {}

    fn rpc_emit_timeline_change_event(&self, id: &str, uris: Option<Vec<URI>>) {
        let uris = uris.map(|uris| uris.iter().map(|uri| uri.to_string(false).to_string_lossy()).collect());
        self.calls.borrow_mut().push(Call::Change(id.to_owned(), uris));
    }

    fn rpc_emit_workspace_timeline_change_event(&self, id: &str) {
        self.calls.borrow_mut().push(Call::WorkspaceChange(id.to_owned()));
    }
}

/// `new class extends mock<ExtHostCommands>() { override registerArgumentProcessor(processor) { argumentProcessor = processor; } }`.
#[derive(Default)]
struct CapturingCommands {
    processor: RefCell<Option<Rc<dyn ArgumentProcessor>>>,
}

impl ArgumentProcessorRegistry for CapturingCommands {
    fn register_argument_processor(&self, processor: Rc<dyn ArgumentProcessor>) {
        *self.processor.borrow_mut() = Some(processor);
    }
}

/// `new class extends mock<CommandsConverter>() { }`: nenhum caso converte comandos com ele.
struct NoConverter;

impl ICommandsConverter for NoConverter {
    fn from_internal(&self, _command: &ICommandDto) -> Option<Command> {
        panic!("fromInternal is not a function")
    }

    fn to_internal(&self, _command: Option<&Command>, _disposables: &DisposableStore) -> Result<Option<ICommandDto>, Error> {
        panic!("toInternal is not a function")
    }
}

/// `new class extends mock<CommandsConverter>() { override toInternal(command) { return { id: command.command, title: command.title }; } }`.
struct InternalConverter;

impl ICommandsConverter for InternalConverter {
    fn from_internal(&self, _command: &ICommandDto) -> Option<Command> {
        panic!("fromInternal is not a function")
    }

    fn to_internal(&self, command: Option<&Command>, _disposables: &DisposableStore) -> Result<Option<ICommandDto>, Error> {
        Ok(command.map(|command| ICommandDto { ident: None, id: command.command.clone(), title: command.title.clone(), tooltip: None, arguments: None }))
    }
}

// ---- o ambiente -------------------------------------------------------------------------------------

/// `createTimeline()`.
struct Env {
    timeline: Rc<ExtHostTimeline>,
    calls: Rc<RefCell<Vec<Call>>>,
    commands: Rc<CapturingCommands>,
}

impl Env {
    fn new() -> Env {
        let commands = Rc::new(CapturingCommands::default());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let proxy: Rc<dyn MainThreadTimelineShape> = Rc::new(MockProxy { calls: calls.clone() });
        let context: Rc<dyn IMainContext> = single_proxy_rpc_protocol(proxy);
        let timeline = ExtHostTimeline::new(&*context, &*commands);
        Env { timeline, calls, commands }
    }

    /// `processArgument: (arg, extension) => argumentProcessor.processArgument(arg, extension)`.
    fn process_argument(&self, arg: &Dynamic, extension: &ExtensionDescription) -> Dynamic {
        let processor = self.commands.processor.borrow().clone().expect("o ExtHostTimeline registra o processador de argumentos");
        processor.process_argument(arg, Some(extension))
    }
}

/// `const uri = URI.parse('file:///timeline.txt')`.
fn uri() -> URI {
    URI::parse("file:///timeline.txt", false)
}

/// `const options: TimelineOptions = { cacheResults: true, triggerKind: TimelineTriggerKind.Automatic }`.
fn options() -> TimelineOptions {
    TimelineOptions { cache_results: Some(true), ..TimelineOptions::new(TimelineTriggerKind::Automatic) }
}

fn file_scheme() -> TimelineScheme {
    TimelineScheme::One("file".to_owned())
}

fn converter() -> Rc<dyn ICommandsConverter> {
    Rc::new(NoConverter)
}

/// `createExtension(id, enabledApiProposals = ['timeline'])`.
fn create_extension(id: &str, enabled_api_proposals: &[&str]) -> ExtensionDescription {
    let mut extension = null_extension_description();
    extension.identifier = ExtensionIdentifier::new(id);
    extension.manifest.enabled_api_proposals = Some(enabled_api_proposals.iter().map(|proposal| (*proposal).to_owned()).collect());
    extension
}

/// `toContext(handle, source, uri)`.
fn to_context(handle: &str, source: &str, uri: Option<&URI>) -> Dynamic {
    Dynamic::object([
        ("$mid", Dynamic::from(MarshalledId::TimelineActionContext as i32)),
        ("handle", Dynamic::string(handle)),
        ("source", Dynamic::string(source)),
        ("uri", uri.map_or(Dynamic::Undefined, |uri| uri.to_dynamic())),
    ])
}

fn set(object: &je_base::dynamic::ObjectRef, key: &str, value: Dynamic) {
    object.set(&JsString::from(key), value);
}

/// `Object.isFrozen(value)`: um valor primitivo é sempre congelado.
fn is_frozen(value: &Dynamic) -> bool {
    match value {
        Dynamic::Object(object) => object.is_frozen(),
        Dynamic::Array(array) => array.is_frozen(),
        _ => true,
    }
}

/// `{ ...value }`: um objeto simples com as propriedades próprias de `value` (o `$$class` não é propriedade de classe).
fn spread(value: &Dynamic) -> Dynamic {
    Dynamic::object(value.own_entries().into_iter().filter(|(key, _)| *key != CLASS_KEY))
}

/// `try { change(); } catch { /* frozen */ }`.
fn try_change(change: impl FnOnce()) {
    let _ = errors::catch(change);
}

/// Um provedor `{ id: 'test-timeline', label: 'Test Timeline', provideTimeline: () => ({ items }) }`.
fn provider_of(items: Vec<Dynamic>) -> ApiTimelineProvider {
    ApiTimelineProvider {
        id: "test-timeline".to_owned(),
        label: "Test Timeline".to_owned(),
        on_did_change_timeline: None,
        provide_timeline: Rc::new(move |_, _, _| MaybePromise::Value(Some(ApiTimeline { items: items.clone(), cursor: None }))),
    }
}

// ---- os casos ---------------------------------------------------------------------------------------

#[test]
fn disposes_cached_timeline_items_with_their_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let env = Env::new();
        let extension = create_extension("test.timeline", &["timeline"]);
        let item = Dynamic::Object(new_timeline_item("first item", 1.0));
        let provider = provider_of(vec![item.clone()]);
        let registration = store.add(env.timeline.register_timeline_provider(file_scheme(), provider.clone(), &extension.identifier, converter()).unwrap());
        let result = env.timeline.rpc_get_timeline(&provider.id, Some(uri()), options(), CancellationToken::None).await.unwrap();
        let result = result.expect("assert.ok(result)");
        let context = to_context(&result.items[0].handle, &provider.id, Some(&uri()));
        assert!(env.process_argument(&context, &extension).strict_equals(&item));

        registration.dispose();
        let replacement = store.add(env.timeline.register_timeline_provider(file_scheme(), provider.clone(), &extension.identifier, converter()).unwrap());
        assert!(env.process_argument(&context, &extension).is_undefined());
        replacement.dispose();
    });
}

#[test]
fn does_not_cache_an_in_flight_timeline_result_after_provider_disposal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let env = Env::new();
        let extension = create_extension("test.timeline", &["timeline"]);
        let deferred_result = Rc::new(DeferredPromise::<Option<ApiTimeline>>::new());
        let pending = deferred_result.clone();
        let provider = ApiTimelineProvider {
            id: "test-timeline".to_owned(),
            label: "Test Timeline".to_owned(),
            on_did_change_timeline: None,
            provide_timeline: Rc::new(move |_, _, _| MaybePromise::Promise(pending.promise.clone())),
        };
        let registration = store.add(env.timeline.register_timeline_provider(file_scheme(), provider.clone(), &extension.identifier, converter()).unwrap());
        let result_promise = env.timeline.rpc_get_timeline(&provider.id, Some(uri()), options(), CancellationToken::None);

        registration.dispose();
        let replacement = store.add(
            env.timeline
                .register_timeline_provider(
                    file_scheme(),
                    ApiTimelineProvider { provide_timeline: Rc::new(|_, _, _| MaybePromise::Value(Some(ApiTimeline { items: vec![], cursor: None }))), ..provider.clone() },
                    &extension.identifier,
                    converter(),
                )
                .unwrap(),
        );
        deferred_result.complete(Some(ApiTimeline { items: vec![Dynamic::Object(new_timeline_item("stale item", 1.0))], cursor: None }));

        assert!(result_promise.await.unwrap().is_none());
        assert!(env.process_argument(&to_context(&format!("{}|1", provider.id), &provider.id, Some(&uri())), &extension).is_undefined());
        replacement.dispose();
    });
}

#[test]
fn passes_the_original_items_to_the_providing_extension_and_read_only_copies_to_other_extensions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let env = Env::new();
        let owner = create_extension("test.owner", &["timeline"]);
        let first = new_timeline_item("first item", 2.0);
        set(&first, "id", Dynamic::string("first"));
        set(&first, "description", Dynamic::string("description"));
        set(&first, "contextValue", Dynamic::string("commit"));
        let first_resource_uri = URI::parse("git:/timeline.txt?first", false).to_dynamic();
        set(&first, "resourceUri", first_resource_uri.clone());
        set(
            &first,
            "command",
            Dynamic::object([
                ("title", Dynamic::string("Open")),
                ("command", Dynamic::string("test.open")),
                ("arguments", Dynamic::array([Dynamic::object([("private", Dynamic::Bool(true))])])),
            ]),
        );
        let second = new_timeline_item("second item", 1.0);
        set(&second, "id", Dynamic::string("second"));
        let (first, second) = (Dynamic::Object(first), Dynamic::Object(second));
        let provider = provider_of(vec![first.clone(), second.clone()]);
        let to_internal: Rc<dyn ICommandsConverter> = Rc::new(InternalConverter);
        let registration = store.add(env.timeline.register_timeline_provider(file_scheme(), provider.clone(), &owner.identifier, to_internal).unwrap());
        let result = env.timeline.rpc_get_timeline(&provider.id, Some(uri()), options(), CancellationToken::None).await.unwrap();
        let contexts: Vec<Dynamic> = result.unwrap().items.iter().map(|item| to_context(&item.handle, &provider.id, Some(&uri()))).collect();
        let (first_context, second_context) = (contexts[0].clone(), contexts[1].clone());
        let selection = Dynamic::array([first_context.clone(), second_context.clone()]);

        let other = create_extension("test.other", &["timeline"]);
        let copy = env.process_argument(&first_context, &other);
        let copies = env.process_argument(&selection, &other);
        let without_proposals = create_extension("test.other", &[]);
        let originals = [first.clone(), second.clone()];
        let owner_selection = match env.process_argument(&selection, &owner) {
            Dynamic::Array(items) => items.to_vec().iter().enumerate().all(|(index, item)| item.strict_equals(&originals[index])),
            _ => panic!("a seleção do dono é uma lista"),
        };
        let actual = Dynamic::object([
            ("owner", Dynamic::Bool(env.process_argument(&first_context, &owner).strict_equals(&first))),
            ("ownerSelection", Dynamic::Bool(owner_selection)),
            ("copy", spread(&copy)),
            ("copyIsFrozen", Dynamic::Bool(is_frozen(&copy) && !copy.strict_equals(&first))),
            ("copies", match &copies {
                Dynamic::Array(items) => Dynamic::array(items.to_vec().iter().map(|item| item.get_str("label"))),
                _ => panic!("a seleção de outra extensão é uma lista"),
            }),
            ("withoutProposal", env.process_argument(&first_context, &without_proposals)),
            ("selectionWithoutProposal", env.process_argument(&selection, &without_proposals)),
        ]);
        let expected = Dynamic::object([
            ("owner", Dynamic::Bool(true)),
            ("ownerSelection", Dynamic::Bool(true)),
            (
                "copy",
                Dynamic::object([
                    ("label", Dynamic::string("first item")),
                    ("timestamp", Dynamic::Number(2.0)),
                    ("id", Dynamic::string("first")),
                    ("iconPath", Dynamic::Undefined),
                    ("description", Dynamic::string("description")),
                    ("tooltip", Dynamic::Undefined),
                    ("resourceUri", first_resource_uri),
                    ("contextValue", Dynamic::string("commit")),
                    ("accessibilityInformation", Dynamic::Undefined),
                ]),
            ),
            ("copyIsFrozen", Dynamic::Bool(true)),
            ("copies", Dynamic::array([Dynamic::string("first item"), Dynamic::string("second item")])),
            ("withoutProposal", Dynamic::Undefined),
            ("selectionWithoutProposal", Dynamic::Undefined),
        ]);
        assert_deep_strict_equal(&actual, &expected);
        registration.dispose();
    });
}

#[test]
fn changing_a_copy_for_another_extension_does_not_change_the_original_item() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let env = Env::new();
        let owner = create_extension("test.owner", &["timeline"]);
        let item = new_timeline_item("item", 1.0);
        let mut tooltip = types::MarkdownString::new(Some("tooltip"), false);
        tooltip.is_trusted = Some(IsTrusted::Options(MarkdownStringTrustedOptions { enabled_commands: vec!["test.command".to_owned()] }));
        set(&item, "tooltip", tooltip.to_dynamic());
        set(&item, "accessibilityInformation", Dynamic::object([("label", Dynamic::string("label"))]));
        set(&item, "iconPath", ThemeIcon { id: "git-commit".to_owned(), color: Some(ThemeColor::new("color")) }.to_dynamic());
        let item = Dynamic::Object(item);
        let registration = store.add(env.timeline.register_timeline_provider(file_scheme(), provider_of(vec![item.clone()]), &owner.identifier, converter()).unwrap());
        let result = env.timeline.rpc_get_timeline("test-timeline", Some(uri()), options(), CancellationToken::None).await.unwrap();

        let copy = env.process_argument(&to_context(&result.unwrap().items[0].handle, "test-timeline", Some(&uri())), &create_extension("test.other", &["timeline"]));
        try_change(|| {
            let tooltip = copy.get_str("tooltip");
            let Dynamic::Object(tooltip_object) = &tooltip else { panic!("a dica é um objeto") };
            let Dynamic::String(value) = tooltip.get_str("value") else { panic!("a dica tem texto") };
            tooltip_object.set(&JsString::from("value"), Dynamic::string(&format!("{} changed", value.to_string_lossy())));
        });
        try_change(|| {
            let Dynamic::Array(enabled_commands) = copy.get_str("tooltip").get_str("isTrusted").get_str("enabledCommands") else { panic!("lista de comandos") };
            enabled_commands.push(Dynamic::string("other.command"));
        });
        try_change(|| {
            let Dynamic::Object(accessibility_information) = copy.get_str("accessibilityInformation") else { panic!("informação de acessibilidade") };
            accessibility_information.set(&JsString::from("label"), Dynamic::string("changed"));
        });
        try_change(|| {
            let Dynamic::Object(color) = copy.get_str("iconPath").get_str("color") else { panic!("a cor do ícone") };
            color.set(&JsString::from("id"), Dynamic::string("changed"));
        });

        let tooltip = item.get_str("tooltip");
        let actual = Dynamic::object([
            ("tooltip", tooltip.get_str("value")),
            ("isTrusted", tooltip.get_str("isTrusted")),
            ("accessibilityInformation", item.get_str("accessibilityInformation")),
            ("color", item.get_str("iconPath").get_str("color").get_str("id")),
            ("copiedTooltip", copy.get_str("tooltip").get_str("value")),
            (
                "ownerIsFrozen",
                Dynamic::Bool(is_frozen(&item) || is_frozen(&item.get_str("accessibilityInformation")) || is_frozen(&item.get_str("iconPath"))),
            ),
        ]);
        let expected = Dynamic::object([
            ("tooltip", Dynamic::string("tooltip")),
            ("isTrusted", Dynamic::object([("enabledCommands", Dynamic::array([Dynamic::string("test.command")]))])),
            ("accessibilityInformation", Dynamic::object([("label", Dynamic::string("label"))])),
            ("color", Dynamic::string("color")),
            ("copiedTooltip", Dynamic::string("tooltip changed")),
            ("ownerIsFrozen", Dynamic::Bool(false)),
        ]);
        assert_deep_strict_equal(&actual, &expected);
        registration.dispose();
    });
}

#[test]
fn requests_timelines_with_public_options_and_forwards_change_events() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let env = Env::new();
        let extension = create_extension("test.timeline", &["timeline"]);
        let requests: Rc<RefCell<Vec<ApiTimelineOptions>>> = Rc::default();
        let on_did_change_timeline = store.add_rc(Emitter::<Option<ApiTimelineChange>>::new());
        let recorded = requests.clone();
        let provider = ApiTimelineProvider {
            id: "test-timeline".to_owned(),
            label: "Test Timeline".to_owned(),
            on_did_change_timeline: Some(on_did_change_timeline.event()),
            provide_timeline: Rc::new(move |_uri, options, _token| {
                recorded.borrow_mut().push(options.clone());
                MaybePromise::Value(Some(ApiTimeline { items: vec![Dynamic::Object(new_timeline_item("item", 1.0))], cursor: Some("next".to_owned()) }))
            }),
        };
        let registration = store.add(env.timeline.register_timeline_provider(file_scheme(), provider.clone(), &extension.identifier, converter()).unwrap());

        let result = env
            .timeline
            .rpc_get_timeline(
                &provider.id,
                Some(uri()),
                TimelineOptions {
                    cursor: Some("start".to_owned()),
                    limit: Some(10.0),
                    trigger_kind: TimelineTriggerKind::Invoke,
                    cache_results: Some(true),
                    reset_cache: Some(true),
                },
                CancellationToken::None,
            )
            .await
            .unwrap();
        on_did_change_timeline.fire(Some(ApiTimelineChange::Uri(uri())));
        on_did_change_timeline.fire(Some(ApiTimelineChange::Uris(vec![uri(), URI::parse("file:///other.txt", false)])));
        on_did_change_timeline.fire(None);

        assert_eq!(
            *requests.borrow(),
            vec![ApiTimelineOptions { cursor: Some("start".to_owned()), limit: Some(10.0), trigger_kind: types::TimelineTriggerKind::Invoke }]
        );
        assert_eq!(result.and_then(|result| result.cursor), Some("next".to_owned()));
        assert_eq!(
            *env.calls.borrow(),
            vec![
                Call::Change(provider.id.clone(), Some(vec!["file:///timeline.txt".to_owned()])),
                Call::Change(provider.id.clone(), Some(vec!["file:///timeline.txt".to_owned(), "file:///other.txt".to_owned()])),
                Call::Change(provider.id.clone(), None),
            ]
        );
        registration.dispose();
    });
}

#[test]
fn workspace_timeline_providers_share_an_id_only_with_providers_of_the_same_extension() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let env = Env::new();
        let extension = create_extension("test.timeline", &["timeline"]);
        let item = Dynamic::Object(new_timeline_item("workspace item", 1.0));
        let on_did_change_workspace_timeline = store.add_rc(Emitter::<()>::new());
        let provided = item.clone();
        let workspace_registration = store.add(
            env.timeline
                .register_workspace_timeline_provider(
                    ApiWorkspaceTimelineProvider {
                        id: "test-timeline".to_owned(),
                        label: "Test Timeline".to_owned(),
                        on_did_change_workspace_timeline: Some(on_did_change_workspace_timeline.event()),
                        provide_workspace_timeline: Rc::new(move |_, _| MaybePromise::Value(Some(ApiTimeline { items: vec![provided.clone()], cursor: None }))),
                    },
                    &extension.identifier,
                    converter(),
                )
                .unwrap(),
        );
        let file_registration = store.add(env.timeline.register_timeline_provider(file_scheme(), provider_of(vec![]), &extension.identifier, converter()).unwrap());

        let result = env.timeline.rpc_get_timeline("test-timeline", None, options(), CancellationToken::None).await.unwrap().unwrap();
        on_did_change_workspace_timeline.fire(());

        let workspace_item = env.process_argument(&to_context(&result.items[0].handle, "test-timeline", None), &extension).strict_equals(&item);
        let file_item = env.process_argument(&to_context(&result.items[0].handle, "test-timeline", Some(&uri())), &extension);
        assert!(workspace_item);
        assert!(file_item.is_undefined());
        assert_eq!(*env.calls.borrow(), vec![Call::WorkspaceChange("test-timeline".to_owned())]);
        assert!(
            env.timeline
                .register_workspace_timeline_provider(
                    ApiWorkspaceTimelineProvider {
                        id: "test-timeline".to_owned(),
                        label: "Duplicate".to_owned(),
                        on_did_change_workspace_timeline: None,
                        provide_workspace_timeline: Rc::new(|_, _| MaybePromise::Value(None)),
                    },
                    &extension.identifier,
                    converter(),
                )
                .is_err()
        );
        file_registration.dispose();
        let other = ApiTimelineProvider {
            id: "test-timeline".to_owned(),
            label: "Other".to_owned(),
            on_did_change_timeline: None,
            provide_timeline: Rc::new(|_, _, _| MaybePromise::Value(None)),
        };
        assert!(env.timeline.register_timeline_provider(file_scheme(), other, &create_extension("test.other", &["timeline"]).identifier, converter()).is_err());

        workspace_registration.dispose();
    });
}
