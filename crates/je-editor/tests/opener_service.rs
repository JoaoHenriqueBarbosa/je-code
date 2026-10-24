//! Porte de `src/vs/editor/test/browser/services/openerService.test.ts` (suíte `OpenerService`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso, e o `store.add`
//!   registra nele o que o caso precisa descartar.
//! - O `TestCodeEditorService` e o `commandService` da suíte são instâncias únicas no original (o `lastInput` e o
//!   `lastCommand` passam de um caso para o outro, e o `setup` zera só o `lastCommand`). Aqui cada caso cria os
//!   seus, o que é equivalente porque todo caso que lê `lastInput` ou `lastCommand` antes escreve nele.
//!   `TestCodeEditorService` e `NullCommandService` são os dublês de `tests/support`.
//! - Os objetos literais `{ open() {...} }`, `{ shouldOpen() {...} }`, `{ openExternal() {...} }` e
//!   `{ resolveExternalUri() {...} }` viram os dublês `FnOpener`, `FnValidator`, `FnExternalOpener` e `FnResolver`,
//!   construídos pelos auxiliares `opener`, `validator`, `external_opener` e `resolver`. Eles devolvem o valor
//!   direto e o dublê o embrulha em `Promise.resolve` (os `async open() { ... }` do original resolvem o mesmo).
//! - `resource.toString()` dentro de um validador é `resource_to_string`: o texto do `URI`, ou o próprio texto
//!   quando o recurso é uma string (o `toString` de `string` devolve ela mesma).
//! - `Math.random()` no id do comando vira um sufixo fixo: o registro de comandos é por thread e cada caso roda
//!   na sua, então não há colisão.
//! - `resource instanceof URI` do caso do PR #118226 é `matches!(resource, OpenResource::Uri(_))`.
//! - `encodeURI` do último caso é `encode_uri` de `je_editor::opener_service`.
//! - `deepStrictEqual` sobre `{ didOpen, calls }` vira `assert_eq!` sobre a tupla `(did_open, calls)`; os
//!   contextos do caso das posições são comparados como tuplas `(sourceUri, preferredOpenerId, viewColumn)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::network::{UriOrString, matches_scheme, matches_some_scheme};
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_editor::opener_service::{OpenerService, encode_uri};
use je_platform::commands::{CommandEvent, CommandHandler, ICommandService, commands_registry};
use je_platform::opener::editor_types::ViewColumn;
use je_platform::opener::{
    AllowCommands, AllowContributedOpeners, DEFAULT_EXTERNAL_URI_OPENER_ID, ExternalOpenerContext, IExternalOpener, IExternalUriResolver,
    IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolvedExternalUri,
};
use support::null_command_service::NullCommandService;
use support::test_code_editor_service::TestCodeEditorService;

/// O `commandService` da suíte: guarda o último comando executado (`lastCommand`).
#[derive(Default)]
struct RecordingCommandService {
    last_command: RefCell<Option<(String, Vec<Dynamic>)>>,
}

impl ICommandService for RecordingCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        *self.last_command.borrow_mut() = Some((command_id.to_string(), args));
        Promise::resolved(Dynamic::Undefined)
    }
}

struct FnOpener(Box<dyn Fn() -> bool>);

impl IOpener for FnOpener {
    fn open(&self, _resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        Promise::resolved((self.0)())
    }
}

struct FnValidator(Box<dyn Fn(&OpenResource) -> bool>);

impl IValidator for FnValidator {
    fn should_open(&self, resource: &OpenResource, _open_options: Option<&OpenOptions>) -> Promise<bool> {
        Promise::resolved((self.0)(resource))
    }
}

struct FnExternalOpener(Box<dyn Fn(&JsString, &ExternalOpenerContext) -> bool>);

impl IExternalOpener for FnExternalOpener {
    fn open_external(&self, href: &JsString, ctx: &ExternalOpenerContext, _token: &je_base::cancellation::CancellationToken) -> Promise<bool> {
        Promise::resolved((self.0)(href, ctx))
    }
}

struct FnResolver(Box<dyn Fn(&URI) -> Option<ResolvedExternalUri>>);

impl IExternalUriResolver for FnResolver {
    fn resolve_external_uri(&self, resource: &URI, _options: Option<&OpenOptions>) -> Promise<Option<ResolvedExternalUri>> {
        Promise::resolved((self.0)(resource))
    }
}

fn opener(open: impl Fn() -> bool + 'static) -> Rc<dyn IOpener> {
    Rc::new(FnOpener(Box::new(open)))
}

fn validator(should_open: impl Fn(&OpenResource) -> bool + 'static) -> Rc<dyn IValidator> {
    Rc::new(FnValidator(Box::new(should_open)))
}

fn external_opener(open_external: impl Fn(&JsString, &ExternalOpenerContext) -> bool + 'static) -> Rc<dyn IExternalOpener> {
    Rc::new(FnExternalOpener(Box::new(open_external)))
}

fn resolver(resolve_external_uri: impl Fn(&URI) -> Option<ResolvedExternalUri> + 'static) -> Rc<dyn IExternalUriResolver> {
    Rc::new(FnResolver(Box::new(resolve_external_uri)))
}

/// `{ resolved, dispose() { } }`.
fn resolved(uri: &URI) -> Option<ResolvedExternalUri> {
    Some(ResolvedExternalUri { resolved: uri.clone(), disposable: None })
}

fn resource_to_string(resource: &OpenResource) -> String {
    match resource {
        OpenResource::Uri(uri) => uri.to_string(false).to_string_lossy(),
        OpenResource::Text(text) => text.to_string_lossy(),
    }
}

fn open(service: &OpenerService, resource: impl Into<OpenResource>, options: Option<OpenOptions>) -> Promise<bool> {
    service.open(&resource.into(), options.as_ref())
}

/// O `calls: string[]` que os casos preenchem, e a função que acrescenta uma entrada.
type Calls = Rc<RefCell<Vec<String>>>;

fn log(calls: &Calls, entry: String) {
    calls.borrow_mut().push(entry);
}

fn new_service(editor_service: &Rc<TestCodeEditorService>, command_service: Rc<dyn ICommandService>) -> OpenerService {
    OpenerService::new(editor_service.clone(), command_service)
}

fn allow_commands() -> Option<OpenOptions> {
    Some(OpenOptions { allow_commands: Some(AllowCommands::Bool(true)), ..OpenOptions::default() })
}

fn external_options(allow_contributed_openers: AllowContributedOpeners) -> Option<OpenOptions> {
    Some(OpenOptions {
        open_external: Some(true),
        allow_contributed_openers: Some(allow_contributed_openers),
        ..OpenOptions::default()
    })
}

fn last_selection(editor_service: &TestCodeEditorService) -> je_platform::opener::editor_types::TextEditorSelection {
    editor_service
        .last_input
        .borrow()
        .as_ref()
        .and_then(|input| input.options.as_ref())
        .and_then(|options| options.selection)
        .expect("selection")
}

fn last_fragment(editor_service: &TestCodeEditorService) -> JsString {
    editor_service.last_input.borrow().as_ref().expect("lastInput").resource.fragment().clone()
}

fn noop_command_handler() -> CommandHandler {
    Rc::new(|_, _| Dynamic::Undefined)
}

#[test]
fn delegate_to_editor_service_scheme_fff() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(NullCommandService));

    run_with_faked_timers(async move {
        open(&opener_service, URI::parse("another:///somepath", false), None).await.expect("open");
        let selection = editor_service.last_input.borrow().as_ref().and_then(|input| input.options.as_ref()).and_then(|options| options.selection);
        assert_eq!(selection, None);
    });
}

#[test]
fn delegate_to_editor_service_scheme_fff_l123() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(NullCommandService));

    run_with_faked_timers(async move {
        open(&opener_service, URI::parse("file:///somepath#L23", false), None).await.expect("open");
        let selection = last_selection(&editor_service);
        assert_eq!(selection.start_line_number, 23.0);
        assert_eq!(selection.start_column, 1.0);
        assert_eq!(selection.end_line_number, None);
        assert_eq!(selection.end_column, None);
        assert_eq!(last_fragment(&editor_service), "");

        open(&opener_service, URI::parse("another:///somepath#L23", false), None).await.expect("open");
        let selection = last_selection(&editor_service);
        assert_eq!(selection.start_line_number, 23.0);
        assert_eq!(selection.start_column, 1.0);

        open(&opener_service, URI::parse("another:///somepath#L23,45", false), None).await.expect("open");
        let selection = last_selection(&editor_service);
        assert_eq!(selection.start_line_number, 23.0);
        assert_eq!(selection.start_column, 45.0);
        assert_eq!(selection.end_line_number, None);
        assert_eq!(selection.end_column, None);
        assert_eq!(last_fragment(&editor_service), "");
    });
}

#[test]
fn delegate_to_editor_service_scheme_fff_123_123() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(NullCommandService));

    run_with_faked_timers(async move {
        open(&opener_service, URI::parse("file:///somepath#23", false), None).await.expect("open");
        let selection = last_selection(&editor_service);
        assert_eq!(selection.start_line_number, 23.0);
        assert_eq!(selection.start_column, 1.0);
        assert_eq!(selection.end_line_number, None);
        assert_eq!(selection.end_column, None);
        assert_eq!(last_fragment(&editor_service), "");

        open(&opener_service, URI::parse("file:///somepath#23,45", false), None).await.expect("open");
        let selection = last_selection(&editor_service);
        assert_eq!(selection.start_line_number, 23.0);
        assert_eq!(selection.start_column, 45.0);
        assert_eq!(selection.end_line_number, None);
        assert_eq!(selection.end_column, None);
        assert_eq!(last_fragment(&editor_service), "");
    });
}

#[test]
fn delegate_to_commands_service_command_someid() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let command_service = Rc::new(RecordingCommandService::default());
    let opener_service = new_service(&editor_service, command_service.clone());

    let id = "aCommand0.1234567";
    store.add(commands_registry().register_command_handler(id, Some(noop_command_handler())));

    run_with_faked_timers(async move {
        assert!(command_service.last_command.borrow().is_none());
        open(&opener_service, URI::parse(format!("command:{id}"), false), None).await.expect("open");
        assert!(command_service.last_command.borrow().is_none());
    });
}

#[test]
fn delegate_to_commands_service_command_someid_2() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let command_service = Rc::new(RecordingCommandService::default());
    let opener_service = new_service(&editor_service, command_service.clone());

    let id = "aCommand0.7654321";
    store.add(commands_registry().register_command_handler(id, Some(noop_command_handler())));

    let command_uri = move |query: Option<&str>| {
        let uri = URI::parse(format!("command:{id}"), false);
        match query {
            Some(query) => uri.with(&UriChange { query: Field::Set(JsString::from(query)), ..UriChange::default() }),
            None => uri,
        }
    };
    let last_command = move |command_service: &RecordingCommandService| command_service.last_command.borrow().clone().expect("lastCommand");

    run_with_faked_timers(async move {
        open(&opener_service, command_uri(Some("\"123\"")), allow_commands()).await.expect("open");
        let (command_id, args) = last_command(&command_service);
        assert_eq!(command_id, id);
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], Dynamic::string("123"));

        open(&opener_service, command_uri(None), allow_commands()).await.expect("open");
        let (command_id, args) = last_command(&command_service);
        assert_eq!(command_id, id);
        assert_eq!(args.len(), 0);

        open(&opener_service, command_uri(Some("123")), allow_commands()).await.expect("open");
        let (command_id, args) = last_command(&command_service);
        assert_eq!(command_id, id);
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], Dynamic::Number(123.0));

        open(&opener_service, command_uri(Some("[12,true]")), allow_commands()).await.expect("open");
        let (command_id, args) = last_command(&command_service);
        assert_eq!(command_id, id);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], Dynamic::Number(12.0));
        assert_eq!(args[1], Dynamic::Bool(true));
    });
}

#[test]
fn links_are_protected_by_validators() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));

    store.add(opener_service.register_validator(validator(|_| false)));

    run_with_faked_timers(async move {
        let http_result = open(&opener_service, URI::parse("https://www.microsoft.com", false), None).await.expect("open");
        let https_result = open(&opener_service, URI::parse("https://www.microsoft.com", false), None).await.expect("open");
        assert!(!http_result);
        assert!(!https_result);
    });
}

#[test]
fn links_validated_by_validators_go_to_openers() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));

    store.add(opener_service.register_validator(validator(|_| true)));

    let open_count = Rc::new(Cell::new(0));
    let counter = open_count.clone();
    store.add(opener_service.register_opener(opener(move || {
        counter.set(counter.get() + 1);
        true
    })));

    run_with_faked_timers(async move {
        open(&opener_service, URI::parse("http://microsoft.com", false), None).await.expect("open");
        assert_eq!(open_count.get(), 1);
        open(&opener_service, URI::parse("https://microsoft.com", false), None).await.expect("open");
        assert_eq!(open_count.get(), 2);
    });
}

#[test]
fn contributed_external_uri_openers_run_before_validators() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));
    let source_uri = URI::parse("https://source.example.com", false);
    let resolved_uri = URI::parse("https://resolved.example.com", false);
    let calls: Calls = Rc::default();

    let (log_calls, target) = (calls.clone(), resolved_uri.clone());
    store.add(opener_service.register_external_uri_resolver(resolver(move |_| {
        log(&log_calls, "resolve".to_string());
        resolved(&target)
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_opener(opener(move || {
        log(&log_calls, "opener".to_string());
        false
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_validator(validator(move |_| {
        log(&log_calls, "validate".to_string());
        false
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_external_opener(external_opener(move |href, context| {
        log(&log_calls, format!("contributed:{href}:{}", context.source_uri.to_string(false)));
        true
    })));

    run_with_faked_timers({
        let source_uri = source_uri.clone();
        async move {
            let options = external_options(AllowContributedOpeners::Bool(true));
            let did_open = open(&opener_service, source_uri.clone(), options).await.expect("open");

            assert_eq!(
                (did_open, calls.borrow().clone()),
                (
                    true,
                    vec![
                        "resolve".to_string(),
                        format!("contributed:{}:{}", resolved_uri.to_string(false), source_uri.to_string(false)),
                    ]
                )
            );
        }
    });
}

#[test]
fn external_uri_opener_contexts_normalize_placement_without_forwarding_open_to_side() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));
    let source_uri = URI::parse("https://source.example.com", false);

    type Context = (URI, Option<JsString>, Option<ViewColumn>);
    let contexts: Rc<RefCell<Vec<Context>>> = Rc::default();
    let collected = contexts.clone();
    store.add(opener_service.register_external_opener(external_opener(move |_href, context| {
        collected.borrow_mut().push((context.source_uri.clone(), context.preferred_opener_id.clone(), context.view_column));
        true
    })));

    run_with_faked_timers(async move {
        for open_to_side in [Some(true), Some(false), None] {
            let options = OpenOptions {
                open_to_side,
                allow_contributed_openers: Some(AllowContributedOpeners::Bool(true)),
                ..OpenOptions::default()
            };
            open(&opener_service, source_uri.clone(), Some(options)).await.expect("open");
        }

        let expected: Vec<Context> = vec![
            (source_uri.clone(), None, Some(ViewColumn::Beside)),
            (source_uri.clone(), None, None),
            (source_uri.clone(), None, None),
        ];
        assert_eq!(*contexts.borrow(), expected);
    });
}

#[test]
fn external_uri_fallback_validates_the_resolved_uri() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));
    let source_uri = URI::parse("https://source.example.com", false);
    let resolved_uri = URI::parse("https://resolved.example.com", false);
    let calls: Calls = Rc::default();

    let (log_calls, target) = (calls.clone(), resolved_uri.clone());
    store.add(opener_service.register_external_uri_resolver(resolver(move |_| {
        log(&log_calls, "resolve".to_string());
        resolved(&target)
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_external_opener(external_opener(move |href, context| {
        log(&log_calls, format!("contributed:{href}:{}", context.source_uri.to_string(false)));
        false
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_validator(validator(move |resource| {
        log(&log_calls, format!("validate:{}", resource_to_string(resource)));
        false
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_opener(opener(move || {
        log(&log_calls, "opener".to_string());
        true
    })));
    let log_calls = calls.clone();
    opener_service.set_default_external_opener(external_opener(move |href, _context| {
        log(&log_calls, format!("default:{href}"));
        true
    }));

    run_with_faked_timers(async move {
        let options = external_options(AllowContributedOpeners::Bool(true));
        let did_open = open(&opener_service, source_uri.clone(), options).await.expect("open");

        assert_eq!(
            (did_open, calls.borrow().clone()),
            (
                false,
                vec![
                    "resolve".to_string(),
                    format!("contributed:{}:{}", resolved_uri.to_string(false), source_uri.to_string(false)),
                    format!("validate:{}", resolved_uri.to_string(false)),
                ]
            )
        );
    });
}

#[test]
fn default_external_uri_opener_validates_before_regular_openers() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));
    let source_uri = URI::parse("https://source.example.com", false);
    let calls: Calls = Rc::default();

    let log_calls = calls.clone();
    store.add(opener_service.register_external_opener(external_opener(move |_href, _context| {
        log(&log_calls, "contributed".to_string());
        true
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_validator(validator(move |resource| {
        log(&log_calls, format!("validate:{}", resource_to_string(resource)));
        false
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_opener(opener(move || {
        log(&log_calls, "opener".to_string());
        true
    })));

    run_with_faked_timers(async move {
        let options = external_options(AllowContributedOpeners::Id(JsString::from(DEFAULT_EXTERNAL_URI_OPENER_ID)));
        let did_open = open(&opener_service, source_uri.clone(), options).await.expect("open");

        assert_eq!((did_open, calls.borrow().clone()), (false, vec![format!("validate:{}", source_uri.to_string(false))]));
    });
}

#[test]
fn default_external_uri_opener_skips_contributed_openers() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));
    let source_uri = URI::parse("https://source.example.com", false);
    let calls: Calls = Rc::default();

    let log_calls = calls.clone();
    store.add(opener_service.register_external_opener(external_opener(move |_href, _context| {
        log(&log_calls, "contributed".to_string());
        true
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_validator(validator(move |resource| {
        log(&log_calls, format!("validate:{}", resource_to_string(resource)));
        true
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_opener(opener(move || {
        log(&log_calls, "opener".to_string());
        false
    })));
    let log_calls = calls.clone();
    opener_service.set_default_external_opener(external_opener(move |href, _context| {
        log(&log_calls, format!("default:{href}"));
        true
    }));

    run_with_faked_timers(async move {
        let options = external_options(AllowContributedOpeners::Id(JsString::from(DEFAULT_EXTERNAL_URI_OPENER_ID)));
        let did_open = open(&opener_service, source_uri.clone(), options).await.expect("open");

        assert_eq!(
            (did_open, calls.borrow().clone()),
            (
                true,
                vec![
                    format!("validate:{}", source_uri.to_string(false)),
                    "opener".to_string(),
                    format!("default:{}", source_uri.to_string(false)),
                ]
            )
        );
    });
}

#[test]
fn external_uri_fallback_preserves_strings_for_validation_and_opening() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));
    let source = "https://source.example.com/path?value=%2B";
    let calls: Calls = Rc::default();

    let log_calls = calls.clone();
    store.add(opener_service.register_external_opener(external_opener(move |_href, _context| {
        log(&log_calls, "contributed".to_string());
        false
    })));
    let log_calls = calls.clone();
    store.add(opener_service.register_validator(validator(move |resource| {
        log(&log_calls, format!("validate:{}", resource_to_string(resource)));
        true
    })));
    let log_calls = calls.clone();
    opener_service.set_default_external_opener(external_opener(move |href, _context| {
        log(&log_calls, format!("default:{href}"));
        true
    }));

    run_with_faked_timers(async move {
        let options = external_options(AllowContributedOpeners::Bool(true));
        let did_open = open(&opener_service, source, options).await.expect("open");

        assert_eq!(
            (did_open, calls.borrow().clone()),
            (true, vec!["contributed".to_string(), format!("validate:{source}"), format!("default:{source}")])
        );
    });
}

#[test]
fn links_arent_manipulated_before_being_passed_to_validator_pr_118226() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));

    store.add(opener_service.register_validator(validator(|resource| {
        // We don't want it to convert strings into URIs
        assert!(!matches!(resource, OpenResource::Uri(_)));
        false
    })));

    run_with_faked_timers(async move {
        open(&opener_service, "https://wwww.microsoft.com", None).await.expect("open");
        open(&opener_service, "https://www.microsoft.com??params=CountryCode%3DUSA%26Name%3Dvscode\"", None).await.expect("open");
    });
}

#[test]
fn links_validated_by_multiple_validators() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));

    let v1 = Rc::new(Cell::new(0));
    let counter = v1.clone();
    opener_service.register_validator(validator(move |_| {
        counter.set(counter.get() + 1);
        true
    }));

    let v2 = Rc::new(Cell::new(0));
    let counter = v2.clone();
    opener_service.register_validator(validator(move |_| {
        counter.set(counter.get() + 1);
        true
    }));

    let open_count = Rc::new(Cell::new(0));
    let counter = open_count.clone();
    opener_service.register_opener(opener(move || {
        counter.set(counter.get() + 1);
        true
    }));

    run_with_faked_timers(async move {
        open(&opener_service, URI::parse("http://microsoft.com", false), None).await.expect("open");
        assert_eq!(open_count.get(), 1);
        assert_eq!(v1.get(), 1);
        assert_eq!(v2.get(), 1);
        open(&opener_service, URI::parse("https://microsoft.com", false), None).await.expect("open");
        assert_eq!(open_count.get(), 2);
        assert_eq!(v1.get(), 2);
        assert_eq!(v2.get(), 2);
    });
}

#[test]
fn links_invalidated_by_first_validator_do_not_continue_validating() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(RecordingCommandService::default()));

    let v1 = Rc::new(Cell::new(0));
    let counter = v1.clone();
    opener_service.register_validator(validator(move |_| {
        counter.set(counter.get() + 1);
        false
    }));

    let v2 = Rc::new(Cell::new(0));
    let counter = v2.clone();
    opener_service.register_validator(validator(move |_| {
        counter.set(counter.get() + 1);
        true
    }));

    let open_count = Rc::new(Cell::new(0));
    let counter = open_count.clone();
    opener_service.register_opener(opener(move || {
        counter.set(counter.get() + 1);
        true
    }));

    run_with_faked_timers(async move {
        open(&opener_service, URI::parse("http://microsoft.com", false), None).await.expect("open");
        assert_eq!(open_count.get(), 0);
        assert_eq!(v1.get(), 1);
        assert_eq!(v2.get(), 0);
        open(&opener_service, URI::parse("https://microsoft.com", false), None).await.expect("open");
        assert_eq!(open_count.get(), 0);
        assert_eq!(v1.get(), 2);
        assert_eq!(v2.get(), 0);
    });
}

#[test]
fn matches_scheme_test() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let text = |value: &'static str| UriOrString::Str(value);
    assert!(matches_scheme(text("https://microsoft.com"), "https"));
    assert!(matches_scheme(text("http://microsoft.com"), "http"));
    assert!(matches_scheme(text("hTTPs://microsoft.com"), "https"));
    assert!(matches_scheme(text("httP://microsoft.com"), "http"));
    assert!(matches_scheme(UriOrString::Uri(&URI::parse("https://microsoft.com", false)), "https"));
    assert!(matches_scheme(UriOrString::Uri(&URI::parse("http://microsoft.com", false)), "http"));
    assert!(matches_scheme(UriOrString::Uri(&URI::parse("hTTPs://microsoft.com", false)), "https"));
    assert!(matches_scheme(UriOrString::Uri(&URI::parse("httP://microsoft.com", false)), "http"));
    assert!(!matches_scheme(UriOrString::Uri(&URI::parse("https://microsoft.com", false)), "http"));
    assert!(!matches_scheme(UriOrString::Uri(&URI::parse("htt://microsoft.com", false)), "http"));
    assert!(!matches_scheme(UriOrString::Uri(&URI::parse("z://microsoft.com", false)), "http"));
}

#[test]
fn matches_some_scheme_test() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(matches_some_scheme(UriOrString::Str("https://microsoft.com"), &["http", "https"]));
    assert!(matches_some_scheme(UriOrString::Str("http://microsoft.com"), &["http", "https"]));
    assert!(!matches_some_scheme(UriOrString::Str("x://microsoft.com"), &["http", "https"]));
}

#[test]
fn resolve_external_uri() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(NullCommandService));

    run_with_faked_timers(async move {
        let folder = URI::parse("file:///Users/user/folder", false);
        assert!(opener_service.resolve_external_uri(&folder, None).await.is_err(), "Should not reach here");

        let disposable = opener_service.register_external_uri_resolver(resolver(resolved_echo));

        let result = opener_service.resolve_external_uri(&folder, None).await.expect("resolveExternalUri");
        assert_eq!(result.resolved.to_string(false), "file:///Users/user/folder");
        disposable.dispose();
    });
}

/// `async resolveExternalUri(uri) { return { resolved: uri, dispose() { } }; }`.
fn resolved_echo(uri: &URI) -> Option<ResolvedExternalUri> {
    resolved(uri)
}

#[test]
fn vscode_open_command_cant_open_http_url_with_hash_in_it_extension_development_140907() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = Rc::new(TestCodeEditorService::default());
    let opener_service = new_service(&editor_service, Rc::new(NullCommandService));

    let actual: Rc<RefCell<Vec<JsString>>> = Rc::default();
    let collected = actual.clone();
    opener_service.set_default_external_opener(external_opener(move |href, _context| {
        collected.borrow_mut().push(href.clone());
        true
    }));

    let href = "https://gitlab.com/viktomas/test-project/merge_requests/new?merge_request%5Bsource_branch%5D=test-%23-hash";
    let uri = URI::parse(href, false);

    run_with_faked_timers(async move {
        assert!(open(&opener_service, uri.clone(), None).await.expect("open"));
        assert!(open(&opener_service, href, None).await.expect("open"));

        let expected = vec![
            encode_uri(&uri.to_string(true)).expect("encodeURI"), // BAD, the encoded # (%23) is double encoded to %2523 (% is double encoded)
            JsString::from(href),                                  // good
        ];
        assert_eq!(*actual.borrow(), expected);
    });
}
