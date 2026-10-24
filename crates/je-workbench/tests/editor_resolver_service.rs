//! Porte de `src/vs/workbench/services/editor/test/browser/editorResolverService.test.ts` (suíte `EditorResolverService`), caso a caso e na mesma ordem.
//!
//! Os casos rodam sobre o [`EditorResolverService`] real, criado pelo [`EditorServiceAccessor`] (`workbenchInstantiationService`
//! mais `createEditorPart`; as divergências do acessor estão no cabeçalho de `support/editor_service_support.rs`).
//!
//! Notas de tradução:
//! - **`createEditorResolverService(instantiationService)`.** É [`create_env`]: o acessor (com o `TestConfigurationService` do
//!   caso quando o original passa `workbenchInstantiationService({ configurationService: () => ... })`, por
//!   `EditorServiceAccessor::with_configuration_service`), a espera da parte e o resolvedor do acessor, que o acessor cria
//!   depois de a configuração estar no lugar. O `teardown` é o `disposables.clear()` ([`run_async`]).
//! - **`await service.resolveEditor(...)`** é `.await.unwrap()` sobre a [`Promise`]; `typeof resolution === 'number'` é a
//!   variante `ResolvedEditor::Status`, e `resolution !== ABORT && resolution !== NONE` é a variante `Editor`.
//! - **`URI.parse(original.toString())`** (nos casos `Diff editor Resolve`) lê o `toString()` do objeto sem tipo
//!   `{ resource }`, que é `[object Object]`; a entrada de arquivo recebe esse `URI` ([`object_object_uri`]).
//! - **`createInstance(DiffEditorInput, 'name', 'description', original, modified, undefined)`** é `DiffEditorInput::new` com
//!   o `EditorService` do acessor.
//! - **`Object.defineProperty(existingEditor, 'preferredResource', ...)`** é `TestFileEditorInput::override_preferred_resource`.
//! - **`createMutableConfigurationService`** é o `TestConfigurationService` com `stub_update_value(SetsUserConfiguration)`
//!   (a subclasse que conta `updateCount` e chama `setUserConfiguration`); `updateCount` é `update_value_calls().len()`.
//! - **`setDefaultEditor retains the association supplied by the default configuration layer`.** O `TestConfigurationService`
//!   anônimo que sobrescreve `inspect` e `updateValue` é o [`DefaultLayerConfigurationService`], um `IConfigurationService`
//!   que delega ao `TestConfigurationService` e sobrescreve os dois.
//! - **`Object.isFrozen`.** A imutabilidade de `EditorMatches`, de `matches` e de cada regra é a do próprio tipo (campos
//!   privados e só leitura, ver `editor_resolver_service.rs`); o campo `immutable` do `summarize` do caso
//!   `getEditorMatches derives the default rule from the complete match set` não tem como ser medido e fica de fora do
//!   `deepStrictEqual`. `matches.defaultRule === matches.matches[matches.defaultRuleIndex]` é a comparação de endereços.
//! - **`assert.throws(() => new EditorMatches([], 0, 0, false), RangeError)`** é `errors::catch` e a conferência do nome.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, id_of};
use je_base::network::schemas;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::{URI, UriComponents};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::{TestConfigurationService, TestConfigurationValue, UpdateValueBehavior};
use je_platform::configuration::{
    ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue, models::{ConfigurationChangeEvent, ConfigurationKeys},
};
use je_platform::opener::editor_types::EditorOverride;
use je_platform::resources::uri_file;
use je_workbench::browser::parts::editor::editor_part::EditorPart;
use je_workbench::common::editor::diff_editor_input::DiffEditorInput;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputRef, EditorInputWithOptions, TextEditorOptions, UntypedEditorInput, default_editor_association};
use je_workbench::services::editor::browser_editor_resolver_service::EditorResolverService;
use je_workbench::services::editor::editor_groups_service::{EditorInputWithOptionsAndGroup, IEditorGroup, IEditorPart, PreferredGroup};
use je_workbench::services::editor::editor_resolver_service::{
    AgentsWindowDefaultOptions, DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, EDITORS_ASSOCIATIONS_SETTING_ID, EditorAssociation, EditorInputFactoryFunction, EditorInputFactoryObject,
    EditorMatchRuleSource, EditorMatches, EditorResolverServiceGetAllEditorsOptions, EditorResolverServiceGetEditorMatchesOptions, EditorResolverServiceGetEditorsOptions,
    GlobPattern, IEditorResolverService, RegisteredEditorInfo, RegisteredEditorOptions, RegisteredEditorPriority, RegisteredEditorPriorityConfiguration,
    RegisteredEditorPriorityInfo, RegisteredEditorRegistrationInfo, RegisteredEditorRegistrationPriority, ResolvedEditor, ResolvedStatus, SinglePerResource,
    diff_editors_associations_agents_window_default, editors_associations_agents_window_default,
};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::test::browser::workbench_test_services::TestFileEditorInput;
use support::editor_service_support::EditorServiceAccessor;

const TEST_EDITOR_INPUT_ID: &str = "testEditorInputForEditorResolverService";

//#region Infraestrutura da suíte

/// A parte, o resolvedor e o acessor de `createEditorResolverService()`.
struct Env {
    /// Mantém os serviços do acessor vivos até o fim do caso.
    _accessor: EditorServiceAccessor,
    part: Rc<EditorPart>,
    service: Rc<EditorResolverService>,
    ctx: Ctx,
}

/// O que as fábricas de entrada capturam: o `disposables` e o `EditorService` (`accessor.instantiationService`).
#[derive(Clone)]
struct Ctx {
    disposables: Rc<DisposableStore>,
    editor_service: Rc<dyn IEditorService>,
}

impl Ctx {
    /// `constructDisposableFileEditorInput(uri, typeId, disposables)`.
    fn file(&self, resource: URI, type_id: &str) -> Rc<TestFileEditorInput> {
        self.disposables.add_rc(TestFileEditorInput::new(resource, type_id))
    }

    /// `createInstance(DiffEditorInput, 'name', 'description', original, modified, undefined)`.
    fn diff_of_inputs(&self, original: EditorInputRef, modified: EditorInputRef) -> EditorInputRef {
        DiffEditorInput::new(Some("name"), Some("description"), original, modified, None, self.editor_service.clone())
    }

    /// `constructDisposableDiffEditorInput(accessor, original, modified, typeId)`.
    fn diff(&self, original: &UntypedEditorInput, modified: &UntypedEditorInput, type_id: &str) -> EditorInputRef {
        let untitled = || UriComponents { scheme: Some(JsString::from(schemas::UNTITLED)), ..UriComponents::default() };
        let original = self.file(original.resource.clone().unwrap_or_else(|| URI::from(&untitled(), false)), type_id);
        let modified = self.file(modified.resource.clone().unwrap_or_else(|| URI::from(&untitled(), false)), type_id);
        self.diff_of_inputs(original, modified)
    }

    /// O `createDiffEditorInput` que monta a `DiffEditorInput` de arquivos com os recursos `URI.parse(x.toString())`, que
    /// para os objetos sem tipo é `[object Object]` (ver as notas do módulo).
    fn diff_of_object_object_uris(&self) -> EditorInputRef {
        let original = self.file(object_object_uri(), TEST_EDITOR_INPUT_ID);
        let modified = self.file(object_object_uri(), TEST_EDITOR_INPUT_ID);
        self.diff_of_inputs(original, modified)
    }
}

/// `URI.parse(original.toString())` com `original` um objeto sem tipo.
fn object_object_uri() -> URI {
    URI::parse("[object Object]", false)
}

/// `createEditorResolverService(instantiationService)`.
async fn create_env(disposables: &Rc<DisposableStore>, accessor: EditorServiceAccessor) -> Env {
    accessor.part.when_ready().await.unwrap();
    let part = accessor.part.clone();
    let service = accessor.editor_resolver_service.clone();
    let ctx = Ctx { disposables: disposables.clone(), editor_service: accessor.editor_service.clone() };

    Env { _accessor: accessor, part, service, ctx }
}

/// `createEditorResolverService()` com o `workbenchInstantiationService(undefined, disposables)`.
async fn create_default_env(disposables: &Rc<DisposableStore>) -> Env {
    create_env(disposables, EditorServiceAccessor::with_default_backup_service(disposables)).await
}

/// `createEditorResolverService(workbenchInstantiationService({ configurationService: () => configurationService }))`.
async fn create_env_with_configuration(disposables: &Rc<DisposableStore>, configuration_service: Rc<dyn IConfigurationService>) -> Env {
    create_env(disposables, EditorServiceAccessor::with_configuration_service(disposables, move |_| configuration_service)).await
}

/// O caso assíncrono: o guarda de vazamentos e o corpo no relógio virtual; o `teardown` é `disposables.clear()`.
fn run_async<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        body(disposables.clone()).await;
        disposables.clear();
    });
}

impl Env {
    /// `part.activeGroup`.
    fn active_group(&self) -> PreferredGroup {
        PreferredGroup::Group(self.part.active_group())
    }

    /// `await service.resolveEditor(editor, part.activeGroup)`.
    async fn resolve(&self, editor: UntypedEditorInput) -> ResolvedEditor {
        self.service.resolve_editor(&editor, Some(&self.active_group())).await.unwrap()
    }

    /// `service.registerEditor(glob, info, options, factories)`.
    fn register(&self, glob: &str, info: RegisteredEditorRegistrationInfo, options: RegisteredEditorOptions, factories: EditorInputFactoryObject) -> DisposableRef {
        self.service.register_editor(GlobPattern::Pattern(glob.to_string()), info, options, factories)
    }

    /// `disposables.add(service.registerEditor(...))`.
    fn register_disposable(&self, glob: &str, info: RegisteredEditorRegistrationInfo, options: RegisteredEditorOptions, factories: EditorInputFactoryObject) {
        self.ctx.disposables.add(self.register(glob, info, options, factories));
    }

    /// `registerDefaultEditorTestEditors(service, additionalDefaultEditorId?)`.
    fn register_default_editor_test_editors(&self, additional_default_editor_id: Option<&str>) {
        let factory = EditorInputFactoryObject {
            create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(editor.resource.clone().expect("recurso"), TEST_EDITOR_INPUT_ID))),
            create_diff_editor_input: Some(editor_factory(|editor| {
                let modified = editor.modified.as_ref().and_then(|modified| modified.resource.clone()).expect("Expected modified resource.");
                TestFileEditorInput::new(modified, TEST_EDITOR_INPUT_ID)
            })),
            ..EditorInputFactoryObject::default()
        };
        let default_association = default_editor_association();
        self.ctx.disposables.add(self.register(
            "*",
            info(DEFAULT_EDITOR_ASSOCIATION_ID, &default_association.display_name, None, RegisteredEditorPriority::Builtin),
            RegisteredEditorOptions::default(),
            factory.clone(),
        ));
        self.ctx.disposables.add(self.register(
            "*.component.html",
            info("test.componentEditor", "Component Editor", None, RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            factory.clone(),
        ));
        if let Some(additional_default_editor_id) = additional_default_editor_id {
            self.ctx.disposables.add(self.register(
                "*.component.html",
                info(additional_default_editor_id, "Additional Component Editor", None, RegisteredEditorPriority::Default),
                RegisteredEditorOptions::default(),
                factory,
            ));
        }
    }
}

/// O `RegisteredEditorRegistrationInfo` de `{ id, label, detail, priority }`.
fn info(id: &str, label: &str, detail: Option<&str>, priority: impl Into<RegisteredEditorRegistrationPriority>) -> RegisteredEditorRegistrationInfo {
    RegisteredEditorRegistrationInfo { id: id.to_string(), label: label.to_string(), detail: detail.map(str::to_string), priority: priority.into() }
}

/// `{ editor, diff, merge? }`.
fn priorities(editor: RegisteredEditorPriority, diff: RegisteredEditorPriority, merge: Option<RegisteredEditorPriority>) -> RegisteredEditorRegistrationPriority {
    RegisteredEditorRegistrationPriority::Configuration(RegisteredEditorPriorityConfiguration { editor, diff, merge })
}

/// Uma fábrica de entrada: `({ resource, ... }, group) => ({ editor })`.
fn editor_factory<E: EditorInput + 'static>(create: impl Fn(&UntypedEditorInput) -> Rc<E> + 'static) -> EditorInputFactoryFunction {
    Rc::new(move |editor: &UntypedEditorInput, _group: &Rc<dyn IEditorGroup>| {
        let created: EditorInputRef = create(editor);
        MaybePromise::Value(EditorInputWithOptions { editor: created, options: None })
    })
}

/// Como [`editor_factory`], para quem devolve uma [`EditorInputRef`] já tipada (as `DiffEditorInput`).
fn editor_ref_factory(create: impl Fn(&UntypedEditorInput) -> EditorInputRef + 'static) -> EditorInputFactoryFunction {
    Rc::new(move |editor: &UntypedEditorInput, _group: &Rc<dyn IEditorGroup>| MaybePromise::Value(EditorInputWithOptions { editor: create(editor), options: None }))
}

/// `createEditorInput: ({ resource }) => ({ editor: new TestFileEditorInput(resource, typeId) })`.
fn plain_factory(type_id: &'static str) -> EditorInputFactoryObject {
    EditorInputFactoryObject {
        create_editor_input: Some(editor_factory(move |editor| TestFileEditorInput::new(editor.resource.clone().expect("recurso"), type_id))),
        ..EditorInputFactoryObject::default()
    }
}

/// `createEditorInput: ({ resource }) => ({ editor: constructDisposableFileEditorInput(URI.parse(resource.toString()), typeId, disposables) })`.
fn reparsed_file_factory(ctx: &Ctx, type_id: &'static str) -> EditorInputFactoryFunction {
    let ctx = ctx.clone();
    editor_factory(move |editor| ctx.file(reparse(&editor.resource.clone().expect("recurso")), type_id))
}

/// `URI.parse(resource.toString())`.
fn reparse(resource: &URI) -> URI {
    URI::parse(resource.to_string(false), false)
}

/// `{ resource }` como `IResourceEditorInput`.
fn untyped(resource: URI) -> UntypedEditorInput {
    UntypedEditorInput { resource: Some(resource), ..UntypedEditorInput::default() }
}

/// `{ original: { resource }, modified: { resource } }`.
fn diff_untyped(original: URI, modified: URI) -> UntypedEditorInput {
    UntypedEditorInput { original: Some(Box::new(untyped(original))), modified: Some(Box::new(untyped(modified))), ..UntypedEditorInput::default() }
}

/// `{ ..., options: { override: id } }`.
fn with_override(mut input: UntypedEditorInput, id: &str) -> UntypedEditorInput {
    let mut options = TextEditorOptions::default();
    options.base.override_ = Some(EditorOverride::Id(JsString::from(id)));
    input.options = Some(options);
    input
}

/// `URI.parse(value)`.
fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

/// `URI.from({ scheme, path })`.
fn uri_from(scheme: &str, path: Option<&str>) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(scheme)), path: path.map(JsString::from), ..UriComponents::default() }, false)
}

/// O editor resolvido: falha se o resultado é `ABORT` ou `NONE` (`assert.fail()`).
fn expect_editor(resolution: ResolvedEditor) -> EditorInputWithOptionsAndGroup {
    match resolution {
        ResolvedEditor::Editor(editor) => editor,
        ResolvedEditor::Status(status) => panic!("esperava um editor resolvido, veio {status:?}"),
    }
}

/// `typeof resolution === 'number'`.
fn is_status(resolution: &ResolvedEditor) -> bool {
    matches!(resolution, ResolvedEditor::Status(_))
}

/// `{ 'a': 'b', ... }` como valor de configuração.
fn associations(entries: &[(&str, &str)]) -> Dynamic {
    Dynamic::object(entries.iter().map(|(pattern, editor)| (*pattern, Dynamic::string(editor))))
}

/// `new TestConfigurationService({ [setting]: associations, ... })`.
fn configuration(settings: &[(&str, &[(&str, &str)])]) -> Rc<TestConfigurationService> {
    TestConfigurationService::new(Some(Dynamic::object(settings.iter().map(|(setting, entries)| (*setting, associations(entries))))))
}

/// `createMutableConfigurationService(configuration)`.
fn mutable_configuration(settings: &[(&str, &[(&str, &str)])]) -> Rc<TestConfigurationService> {
    let service = configuration(settings);
    service.stub_update_value(UpdateValueBehavior::SetsUserConfiguration);
    service
}

/// `service.getAllUserAssociations()` como `(filenamePattern, viewType)`.
fn user_associations(service: &EditorResolverService) -> Vec<(String, String)> {
    service.get_all_user_associations().into_iter().map(|EditorAssociation { view_type, filename_pattern }| (filename_pattern.unwrap_or_default(), view_type)).collect()
}

/// `editors.map(editor => editor.id).filter(id => id.startsWith('test.'))`.
fn test_editor_ids(editors: &[RegisteredEditorInfo]) -> Vec<String> {
    editors.iter().map(|editor| editor.id.clone()).filter(|id| id.starts_with("test.")).collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// `{ editor, diff, merge }` iguais.
fn all_priorities(priority: RegisteredEditorPriority) -> RegisteredEditorPriorityInfo {
    RegisteredEditorPriorityInfo { editor: priority, diff: priority, merge: priority }
}

//#endregion

#[test]
fn agents_window_editor_defaults_use_the_integrated_browser_for_html_when_available_and_follow_the_markdown_editor_setting() {
    let pairs = |entries: Vec<(&str, &str)>| entries.into_iter().map(|(pattern, editor)| (pattern.to_string(), editor.to_string())).collect::<Vec<_>>();
    let options = |markdown_default_editor, integrated_browser_available| Some(AgentsWindowDefaultOptions { markdown_default_editor: Some(markdown_default_editor), integrated_browser_available: Some(integrated_browser_available) });

    assert_eq!(editors_associations_agents_window_default(options(true, true)), pairs(vec![("*.html", "workbench.editor.browser"), ("*.md", "vscode.markdown.editor")]));
    assert_eq!(
        editors_associations_agents_window_default(options(false, true)),
        pairs(vec![("*.html", "workbench.editor.browser"), ("*.md", "vscode.markdown.preview.editor")])
    );
    assert_eq!(editors_associations_agents_window_default(options(true, false)), pairs(vec![("*.md", "vscode.markdown.editor")]));
}

#[test]
fn agents_window_diff_editor_default_follows_the_markdown_editor_setting() {
    let options = |markdown_default_editor| Some(AgentsWindowDefaultOptions { markdown_default_editor: Some(markdown_default_editor), integrated_browser_available: None });

    assert_eq!(diff_editors_associations_agents_window_default(options(true)), vec![("*.md".to_string(), "vscode.markdown.editor".to_string())]);
    assert_eq!(diff_editors_associations_agents_window_default(options(false)), vec![("*.md".to_string(), "vscode.markdown.preview.editor".to_string())]);
}

#[test]
fn simple_resolve() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let registered_editor = env.register(
            "*.test",
            info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(reparse(&editor.resource.clone().unwrap()), TEST_EDITOR_INPUT_ID))),
                ..EditorInputFactoryObject::default()
            },
        );

        let resulting_resolution = env.resolve(untyped(uri_file("my://resource-basics.test"))).await;
        let resolved = expect_editor(resulting_resolution);
        assert_eq!(resolved.editor.editor_type_id(), TEST_EDITOR_INPUT_ID);
        resolved.editor.dispose();
        registered_editor.dispose();
    });
}

#[test]
fn single_per_resource_finds_editors_by_preferred_resource() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let resource = uri_file("/workspace/index.test");
        let editor_id = "TEST_EDITOR";
        let existing_editor = env.ctx.file(uri_from(schemas::VSCODE_BROWSER, Some("browser-id")), editor_id);
        existing_editor.override_preferred_resource(&resource);
        let existing: EditorInputRef = existing_editor.clone();
        env.part.active_group().open_editor(&existing, None).await.unwrap();
        let create_count = Rc::new(Cell::new(0));
        let counter = create_count.clone();
        let ctx = env.ctx.clone();
        let factory_resource = resource.clone();
        env.register_disposable(
            "*.test",
            info(editor_id, "Test Editor Label", None, RegisteredEditorPriority::Default),
            RegisteredEditorOptions { single_per_resource: Some(SinglePerResource::Bool(true)), ..RegisteredEditorOptions::default() },
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(move |_editor| {
                    counter.set(counter.get() + 1);
                    ctx.file(factory_resource.clone(), editor_id)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let result = env.resolve(with_override(untyped(resource), editor_id)).await;

        let result = expect_editor(result);
        assert_eq!((id_of(&result.editor) == id_of(&existing_editor), create_count.get()), (true, 0));
    });
}

#[test]
fn untitled_resolve() {
    run_async(|disposables| async move {
        const UNTITLED_TEST_EDITOR_INPUT_ID: &str = "UNTITLED_TEST_INPUT";
        let env = create_default_env(&disposables).await;
        let registered_editor = env.register(
            "*.test",
            info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(reparse(&editor.resource.clone().unwrap()), TEST_EDITOR_INPUT_ID))),
                create_untitled_editor_input: Some(editor_factory(|editor| {
                    TestFileEditorInput::new(editor.resource.clone().unwrap_or_else(|| uri_from(schemas::UNTITLED, None)), UNTITLED_TEST_EDITOR_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        // Untyped untitled - no resource
        let resulting_resolution = env.resolve(UntypedEditorInput { resource: None, ..UntypedEditorInput::default() }).await;
        // We don't expect untitled to match the *.test glob
        assert!(is_status(&resulting_resolution));

        // Untyped untitled - with untitled resource
        let resolved = expect_editor(env.resolve(untyped(uri_from(schemas::UNTITLED, Some("foo.test")))).await);
        assert_eq!(resolved.editor.editor_type_id(), UNTITLED_TEST_EDITOR_INPUT_ID);
        resolved.editor.dispose();

        // Untyped untitled - file resource with forceUntitled
        let resolved = expect_editor(env.resolve(UntypedEditorInput { force_untitled: Some(true), ..untyped(uri_file("/fake.test")) }).await);
        assert_eq!(resolved.editor.editor_type_id(), UNTITLED_TEST_EDITOR_INPUT_ID);
        resolved.editor.dispose();

        registered_editor.dispose();
    });
}

#[test]
fn side_by_side_resolve() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let registered_editor_primary = env.register(
            "*.test-primary",
            info("TEST_EDITOR_PRIMARY", "Test Editor Label Primary", Some("Test Editor Details Primary"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject { create_editor_input: Some(reparsed_file_factory(&env.ctx, TEST_EDITOR_INPUT_ID)), ..EditorInputFactoryObject::default() },
        );

        let registered_editor_secondary = env.register(
            "*.test-secondary",
            info("TEST_EDITOR_SECONDARY", "Test Editor Label Secondary", Some("Test Editor Details Secondary"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject { create_editor_input: Some(reparsed_file_factory(&env.ctx, TEST_EDITOR_INPUT_ID)), ..EditorInputFactoryObject::default() },
        );

        let resulting_resolution = env
            .resolve(UntypedEditorInput {
                primary: Some(Box::new(untyped(uri_file("my://resource-basics.test-primary")))),
                secondary: Some(Box::new(untyped(uri_file("my://resource-basics.test-secondary")))),
                ..UntypedEditorInput::default()
            })
            .await;
        let resolved = expect_editor(resulting_resolution);
        assert_eq!(resolved.editor.editor_type_id(), SideBySideEditorInput::ID);
        resolved.editor.dispose();
        registered_editor_primary.dispose();
        registered_editor_secondary.dispose();
    });
}

#[test]
fn diff_editor_resolve() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let ctx = env.ctx.clone();
        let registered_editor = env.register(
            "*.test-diff",
            info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(reparsed_file_factory(&env.ctx, TEST_EDITOR_INPUT_ID)),
                create_diff_editor_input: Some(editor_ref_factory(move |_editor| ctx.diff_of_object_object_uris())),
                ..EditorInputFactoryObject::default()
            },
        );

        let resulting_resolution = env.resolve(diff_untyped(uri_file("my://resource-basics.test-diff"), uri_file("my://resource-basics.test-diff"))).await;
        let resolved = expect_editor(resulting_resolution);
        assert_eq!(resolved.editor.editor_type_id(), DiffEditorInput::ID);
        resolved.editor.dispose();
        registered_editor.dispose();
    });
}

#[test]
fn diff_editor_resolve_falls_back_to_editor_associations() {
    run_async(|disposables| async move {
        const CUSTOM_EDITOR_INPUT_ID: &str = "testCustomEditorInput";
        let configuration_service = configuration(&[(EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.test-diff-association", "TEST_EDITOR")])]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        let custom_diff_counter = Rc::new(Cell::new(0));
        let default_diff_counter = Rc::new(Cell::new(0));

        let (ctx, counter) = (env.ctx.clone(), default_diff_counter.clone());
        let default_registered_editor = env.register(
            "*",
            info("default", "Default Editor", Some("Default"), RegisteredEditorPriority::Builtin),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some({
                    let ctx = env.ctx.clone();
                    editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), TEST_EDITOR_INPUT_ID))
                }),
                create_diff_editor_input: Some(editor_ref_factory(move |editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff(editor.original.as_ref().unwrap(), editor.modified.as_ref().unwrap(), TEST_EDITOR_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let (ctx, counter) = (env.ctx.clone(), custom_diff_counter.clone());
        let custom_registered_editor = env.register(
            "*.test-diff-association",
            info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some({
                    let ctx = env.ctx.clone();
                    editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), CUSTOM_EDITOR_INPUT_ID))
                }),
                create_diff_editor_input: Some(editor_ref_factory(move |editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff(editor.original.as_ref().unwrap(), editor.modified.as_ref().unwrap(), CUSTOM_EDITOR_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let resulting_resolution = env.resolve(diff_untyped(uri_file("resource-basics.test-diff-association"), uri_file("resource-basics.test-diff-association"))).await;
        let resolved = expect_editor(resulting_resolution);
        assert_eq!(custom_diff_counter.get(), 1);
        assert_eq!(default_diff_counter.get(), 0);
        resolved.editor.dispose();

        default_registered_editor.dispose();
        custom_registered_editor.dispose();
    });
}

#[test]
fn diff_editor_resolve_diff_associations_override_editor_associations() {
    run_async(|disposables| async move {
        const EDITOR_ASSOCIATION_INPUT_ID: &str = "testEditorAssociationInput";
        const DIFF_ASSOCIATION_INPUT_ID: &str = "testDiffAssociationInput";
        let configuration_service = configuration(&[
            (EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.test-diff-association", "TEST_EDITOR")]),
            (DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.test-diff-association", "TEST_DIFF_EDITOR")]),
        ]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        let editor_association_diff_counter = Rc::new(Cell::new(0));
        let diff_association_diff_counter = Rc::new(Cell::new(0));

        let (ctx, counter) = (env.ctx.clone(), editor_association_diff_counter.clone());
        let editor_association_registered_editor = env.register(
            "*.test-diff-association",
            info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some({
                    let ctx = env.ctx.clone();
                    editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), EDITOR_ASSOCIATION_INPUT_ID))
                }),
                create_diff_editor_input: Some(editor_ref_factory(move |editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff(editor.original.as_ref().unwrap(), editor.modified.as_ref().unwrap(), EDITOR_ASSOCIATION_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let (ctx, counter) = (env.ctx.clone(), diff_association_diff_counter.clone());
        let diff_association_registered_editor = env.register(
            "*.test-diff-association",
            info("TEST_DIFF_EDITOR", "Test Diff Editor Label", Some("Test Diff Editor Details"), RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some({
                    let ctx = env.ctx.clone();
                    editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), DIFF_ASSOCIATION_INPUT_ID))
                }),
                create_diff_editor_input: Some(editor_ref_factory(move |editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff(editor.original.as_ref().unwrap(), editor.modified.as_ref().unwrap(), DIFF_ASSOCIATION_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let diff_resolution = env.resolve(diff_untyped(uri_file("resource-basics.test-diff-association"), uri_file("resource-basics.test-diff-association"))).await;
        let resolved = expect_editor(diff_resolution);
        assert_eq!(editor_association_diff_counter.get(), 0);
        assert_eq!(diff_association_diff_counter.get(), 1);
        resolved.editor.dispose();

        let editor_resolution = env.resolve(untyped(uri_file("resource-basics.test-diff-association"))).await;
        let resolved = expect_editor(editor_resolution);
        assert_eq!(resolved.editor.editor_type_id(), EDITOR_ASSOCIATION_INPUT_ID);
        resolved.editor.dispose();

        editor_association_registered_editor.dispose();
        diff_association_registered_editor.dispose();
    });
}

#[test]
fn editor_resolve_editor_associations_only_select_an_explicit_editor_in_the_associated_mode() {
    run_async(|disposables| async move {
        const DEFAULT_DIFF_INPUT_ID: &str = "testDefaultDiffInput";
        const EXPLICIT_DIFF_INPUT_ID: &str = "testExplicitDiffInput";
        let configuration_service = configuration(&[(EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.test-explicit-diff", "EXPLICIT_DIFF_EDITOR")])]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        let default_diff_counter = Rc::new(Cell::new(0));
        let explicit_diff_counter = Rc::new(Cell::new(0));

        let (ctx, counter) = (env.ctx.clone(), default_diff_counter.clone());
        let default_registered_editor = env.register(
            "*",
            info("default", "Default Editor", Some("Default"), RegisteredEditorPriority::Builtin),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some({
                    let ctx = env.ctx.clone();
                    editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), TEST_EDITOR_INPUT_ID))
                }),
                create_diff_editor_input: Some(editor_ref_factory(move |editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff(editor.original.as_ref().unwrap(), editor.modified.as_ref().unwrap(), DEFAULT_DIFF_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let (ctx, counter) = (env.ctx.clone(), explicit_diff_counter.clone());
        let explicit_diff_registered_editor = env.register(
            "*.test-explicit-diff",
            info(
                "EXPLICIT_DIFF_EDITOR",
                "Explicit Diff Editor Label",
                Some("Explicit Diff Editor Details"),
                priorities(RegisteredEditorPriority::Explicit, RegisteredEditorPriority::Explicit, None),
            ),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some({
                    let ctx = env.ctx.clone();
                    editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), EXPLICIT_DIFF_INPUT_ID))
                }),
                create_diff_editor_input: Some(editor_ref_factory(move |editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff(editor.original.as_ref().unwrap(), editor.modified.as_ref().unwrap(), EXPLICIT_DIFF_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        // The text-mode association does not opt the editor into diff mode.
        let diff_resolution = env.resolve(diff_untyped(uri_file("resource-basics.test-explicit-diff"), uri_file("resource-basics.test-explicit-diff"))).await;
        let resolved = expect_editor(diff_resolution);
        assert_eq!(explicit_diff_counter.get(), 0);
        assert_eq!(default_diff_counter.get(), 1);
        resolved.editor.dispose();

        let editor_resolution = env.resolve(untyped(uri_file("resource-basics.test-explicit-diff"))).await;
        let resolved = expect_editor(editor_resolution);
        assert_eq!(resolved.editor.editor_type_id(), EXPLICIT_DIFF_INPUT_ID);
        resolved.editor.dispose();

        default_registered_editor.dispose();
        explicit_diff_registered_editor.dispose();
    });
}

#[test]
fn diff_editor_resolve_diff_editor_associations_select_an_explicit_diff_editor() {
    run_async(|disposables| async move {
        const DEFAULT_DIFF_INPUT_ID: &str = "testDefaultDiffInput";
        const EXPLICIT_DIFF_INPUT_ID: &str = "testExplicitDiffInput";
        let configuration_service = configuration(&[(DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.test-explicit-diff", "EXPLICIT_DIFF_EDITOR")])]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        let default_diff_counter = Rc::new(Cell::new(0));
        let explicit_diff_counter = Rc::new(Cell::new(0));

        let (ctx, counter) = (env.ctx.clone(), default_diff_counter.clone());
        let default_registered_editor = env.register(
            "*",
            info("default", "Default Editor", Some("Default"), RegisteredEditorPriority::Builtin),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some({
                    let ctx = env.ctx.clone();
                    editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), TEST_EDITOR_INPUT_ID))
                }),
                create_diff_editor_input: Some(editor_ref_factory(move |editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff(editor.original.as_ref().unwrap(), editor.modified.as_ref().unwrap(), DEFAULT_DIFF_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let (ctx, counter) = (env.ctx.clone(), explicit_diff_counter.clone());
        let explicit_diff_registered_editor = env.register(
            "*.test-explicit-diff",
            info(
                "EXPLICIT_DIFF_EDITOR",
                "Explicit Diff Editor Label",
                Some("Explicit Diff Editor Details"),
                priorities(RegisteredEditorPriority::Option, RegisteredEditorPriority::Explicit, None),
            ),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some({
                    let ctx = env.ctx.clone();
                    editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), EXPLICIT_DIFF_INPUT_ID))
                }),
                create_diff_editor_input: Some(editor_ref_factory(move |editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff(editor.original.as_ref().unwrap(), editor.modified.as_ref().unwrap(), EXPLICIT_DIFF_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let diff_resolution = env.resolve(diff_untyped(uri_file("resource-basics.test-explicit-diff"), uri_file("resource-basics.test-explicit-diff"))).await;
        let resolved = expect_editor(diff_resolution);
        assert_eq!(default_diff_counter.get(), 0);
        assert_eq!(explicit_diff_counter.get(), 1);
        resolved.editor.dispose();

        default_registered_editor.dispose();
        explicit_diff_registered_editor.dispose();
    });
}

#[test]
fn get_binary_diff_fallback_editor_returns_a_diff_capable_explicit_editor_and_ignores_non_diff_editors() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;

        let ctx = env.ctx.clone();
        let diff_ctx = env.ctx.clone();
        let explicit_with_diff = env.register(
            "*.bin",
            info(
                "BINARY_EDITOR",
                "Binary Editor",
                Some("Binary Editor Details"),
                priorities(RegisteredEditorPriority::Default, RegisteredEditorPriority::Explicit, None),
            ),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), "binaryInput"))),
                create_diff_editor_input: Some(editor_factory(move |editor| {
                    let modified = editor.modified.as_ref().and_then(|modified| modified.resource.clone());
                    let original = editor.original.as_ref().and_then(|original| original.resource.clone());
                    diff_ctx.file(modified.or(original).expect("recurso"), "binaryDiffInput")
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        // A custom editor that provides no diff factory must never be used as a binary diff fallback.
        let ctx = env.ctx.clone();
        let no_diff = env.register(
            "*.noDiff",
            info("NO_DIFF_EDITOR", "No Diff Editor", Some("No Diff Editor Details"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(move |editor| ctx.file(editor.resource.clone().unwrap(), "noDiffInput"))),
                ..EditorInputFactoryObject::default()
            },
        );

        assert_eq!(env.service.get_binary_diff_fallback_editor(&uri_file("file.bin")), Some("BINARY_EDITOR".to_string()));
        assert_eq!(env.service.get_binary_diff_fallback_editor(&uri_file("file.noDiff")), None);
        assert_eq!(env.service.get_binary_diff_fallback_editor(&uri_file("file.unrelated")), None);

        explicit_with_diff.dispose();
        no_diff.dispose();
    });
}

#[test]
fn diff_editor_resolve_different_types() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let diff_one_counter = Rc::new(Cell::new(0));
        let diff_two_counter = Rc::new(Cell::new(0));
        let default_diff_counter = Rc::new(Cell::new(0));

        let (ctx, counter) = (env.ctx.clone(), diff_one_counter.clone());
        let registered_editor = env.register(
            "*.test-diff",
            info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(reparsed_file_factory(&env.ctx, TEST_EDITOR_INPUT_ID)),
                create_diff_editor_input: Some(editor_ref_factory(move |_editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff_of_object_object_uris()
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let (ctx, counter) = (env.ctx.clone(), diff_two_counter.clone());
        let second_registered_editor = env.register(
            "*.test-secondDiff",
            info("TEST_EDITOR_2", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(reparse(&editor.resource.clone().unwrap()), TEST_EDITOR_INPUT_ID))),
                create_diff_editor_input: Some(editor_ref_factory(move |_editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff_of_object_object_uris()
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let (ctx, counter) = (env.ctx.clone(), default_diff_counter.clone());
        let default_registered_editor = env.register(
            "*",
            info("default", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(reparse(&editor.resource.clone().unwrap()), TEST_EDITOR_INPUT_ID))),
                create_diff_editor_input: Some(editor_ref_factory(move |_editor| {
                    counter.set(counter.get() + 1);
                    ctx.diff_of_object_object_uris()
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        let counters = || (diff_one_counter.get(), diff_two_counter.get(), default_diff_counter.get());

        let resolved = expect_editor(env.resolve(diff_untyped(uri_file("my://resource-basics.test-diff"), uri_file("my://resource-basics.test-diff"))).await);
        assert_eq!(counters(), (1, 0, 0));
        assert_eq!(resolved.editor.editor_type_id(), DiffEditorInput::ID);
        resolved.editor.dispose();

        let resolved = expect_editor(env.resolve(diff_untyped(uri_file("my://resource-basics.test-secondDiff"), uri_file("my://resource-basics.test-secondDiff"))).await);
        assert_eq!(counters(), (1, 1, 0));
        assert_eq!(resolved.editor.editor_type_id(), DiffEditorInput::ID);
        resolved.editor.dispose();

        let resolved = expect_editor(env.resolve(diff_untyped(uri_file("my://resource-basics.test-secondDiff"), uri_file("my://resource-basics.test-diff"))).await);
        assert_eq!(counters(), (1, 1, 1));
        assert_eq!(resolved.editor.editor_type_id(), DiffEditorInput::ID);
        resolved.editor.dispose();

        let resolved = expect_editor(env.resolve(diff_untyped(uri_file("my://resource-basics.test-diff"), uri_file("my://resource-basics.test-secondDiff"))).await);
        assert_eq!(counters(), (1, 1, 2));
        assert_eq!(resolved.editor.editor_type_id(), DiffEditorInput::ID);
        resolved.editor.dispose();

        let resolved = expect_editor(
            env.resolve(with_override(diff_untyped(uri_file("my://resource-basics.test-secondDiff"), uri_file("my://resource-basics.test-diff")), "TEST_EDITOR")).await,
        );
        assert_eq!(counters(), (2, 1, 2));
        assert_eq!(resolved.editor.editor_type_id(), DiffEditorInput::ID);
        resolved.editor.dispose();

        registered_editor.dispose();
        second_registered_editor.dispose();
        default_registered_editor.dispose();
    });
}

#[test]
fn registry_and_events() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let event_counter = Rc::new(Cell::new(0));
        let counter = event_counter.clone();
        let changes: Event<()> = env.service.on_did_change_editor_registrations();
        disposables.add(changes.listen(move |_| counter.set(counter.get() + 1)));

        let editors = env.service.get_all_editors(None);

        let registered_editor = env.register(
            "*.test",
            info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(reparse(&editor.resource.clone().unwrap()), TEST_EDITOR_INPUT_ID))),
                ..EditorInputFactoryObject::default()
            },
        );

        assert_eq!(event_counter.get(), 1);
        assert_eq!(env.service.get_all_editors(None).len(), editors.len() + 1);
        assert!(env.service.get_all_editors(None).iter().any(|editor| editor.id == "TEST_EDITOR"));

        registered_editor.dispose();

        assert_eq!(event_counter.get(), 2);
        assert_eq!(env.service.get_all_editors(None).len(), editors.len());
        assert!(!env.service.get_all_editors(None).iter().any(|editor| editor.id == "TEST_EDITOR"));
    });
}

#[test]
fn get_editors_excludes_exclusive_registrations_before_deduplicating_editor_ids() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let factory = plain_factory(TEST_EDITOR_INPUT_ID);
        env.register_disposable(
            "exclusive:/**",
            info("test.multiPriority", "Multi-Priority Editor", None, RegisteredEditorPriority::Exclusive),
            RegisteredEditorOptions::default(),
            factory.clone(),
        );
        env.register_disposable(
            "file:/**/*.html",
            info("test.multiPriority", "Multi-Priority Editor", None, RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            factory,
        );

        let multi_priority = |editors: Vec<RegisteredEditorInfo>| editors.into_iter().filter(|editor| editor.id == "test.multiPriority").collect::<Vec<_>>();
        let all = multi_priority(env.service.get_all_editors(None));
        let association_candidates = multi_priority(env.service.get_all_editors(Some(&EditorResolverServiceGetAllEditorsOptions {
            exclude_exclusive_editors: Some(true),
            ..EditorResolverServiceGetAllEditorsOptions::default()
        })));

        let expected = |priority| {
            vec![RegisteredEditorInfo { id: "test.multiPriority".to_string(), label: "Multi-Priority Editor".to_string(), detail: None, priority: all_priorities(priority) }]
        };
        assert_eq!(all, expected(RegisteredEditorPriority::Exclusive));
        assert_eq!(association_candidates, expected(RegisteredEditorPriority::Option));
    });
}

#[test]
fn get_editors_excludes_registrations_without_diff_support_when_requested() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let create_editor_input = editor_factory(|editor| TestFileEditorInput::new(editor.resource.clone().unwrap(), TEST_EDITOR_INPUT_ID));
        env.register_disposable(
            "*.diffable",
            info("test.diffable", "Diffable Editor", None, RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(create_editor_input.clone()),
                create_diff_editor_input: Some(editor_factory(|editor| {
                    TestFileEditorInput::new(editor.modified.as_ref().and_then(|modified| modified.resource.clone()).unwrap(), TEST_EDITOR_INPUT_ID)
                })),
                ..EditorInputFactoryObject::default()
            },
        );
        env.register_disposable(
            "*.notDiffable",
            info("test.notDiffable", "Non-Diffable Editor", None, RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject { create_editor_input: Some(create_editor_input), ..EditorInputFactoryObject::default() },
        );

        let all = test_editor_ids(&env.service.get_all_editors(None));
        let diff = test_editor_ids(&env.service.get_all_editors(Some(&EditorResolverServiceGetAllEditorsOptions {
            is_diff_editor: Some(true),
            ..EditorResolverServiceGetAllEditorsOptions::default()
        })));
        assert_eq!((all, diff), (strings(&["test.diffable", "test.notDiffable"]), strings(&["test.diffable"])));
    });
}

#[test]
fn editor_associations_only_apply_where_the_registered_editor_supports_the_resource() {
    run_async(|disposables| async move {
        let configuration_service = configuration(&[(EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.html", "test.fileOnly")])]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        env.register_disposable(
            "*",
            info("test.default", "Default Editor", None, RegisteredEditorPriority::Builtin),
            RegisteredEditorOptions::default(),
            plain_factory("test.defaultInput"),
        );
        env.register_disposable(
            "file:/**/*.html",
            info("test.fileOnly", "File-Only Editor", None, RegisteredEditorPriority::Option),
            RegisteredEditorOptions { can_support_resource: Some(Rc::new(|resource: &URI| *resource.scheme() == schemas::FILE)), ..RegisteredEditorOptions::default() },
            plain_factory("test.fileOnlyInput"),
        );

        let file_result = env.resolve(untyped(uri_file("/workspace/index.html"))).await;
        let remote_resource = parse("vscode-remote://host/workspace/index.html");
        let remote_candidates: Vec<String> = env.service.get_editors(&remote_resource, None).into_iter().map(|editor| editor.id).collect();
        let remote_result = env.resolve(untyped(remote_resource)).await;
        let file_result = expect_editor(file_result);
        let remote_result = expect_editor(remote_result);

        assert_eq!(file_result.editor.editor_type_id(), "test.fileOnlyInput");
        assert_eq!(remote_result.editor.editor_type_id(), "test.defaultInput");
        assert_eq!(remote_candidates, strings(&["test.default"]));
        file_result.editor.dispose();
        remote_result.editor.dispose();
    });
}

#[test]
fn get_editors_excludes_inactive_universal_optional_editors_when_requested() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let resource = uri_file("/workspace/index.html");
        let factory = plain_factory(TEST_EDITOR_INPUT_ID);
        env.register_disposable(
            "*",
            info("test.universalOptional", "Universal Optional", None, RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            factory.clone(),
        );
        env.register_disposable(
            "*.html",
            info("test.specificOptional", "Specific Optional", None, RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            factory,
        );

        let relevant_ids = |current_editor_id: Option<&str>| {
            test_editor_ids(&env.service.get_editors(
                &resource,
                Some(&EditorResolverServiceGetEditorsOptions {
                    exclude_unconfigured_universal_optional_editors: Some(true),
                    current_editor_id: current_editor_id.map(str::to_string),
                    ..EditorResolverServiceGetEditorsOptions::default()
                }),
            ))
        };

        let all = test_editor_ids(&env.service.get_editors(&resource, None));
        let filtered = relevant_ids(None);
        let current_universal = relevant_ids(Some("test.universalOptional"));
        let diff = test_editor_ids(&env.service.get_editors(
            &resource,
            Some(&EditorResolverServiceGetEditorsOptions {
                exclude_unconfigured_universal_optional_editors: Some(true),
                is_diff_editor: Some(true),
                ..EditorResolverServiceGetEditorsOptions::default()
            }),
        ));
        assert_eq!(all, strings(&["test.specificOptional", "test.universalOptional"]));
        assert_eq!(filtered, strings(&["test.specificOptional"]));
        assert_eq!(current_universal, strings(&["test.specificOptional", "test.universalOptional"]));
        assert_eq!(diff, Vec::<String>::new());
    });
}

/// O que o `summarize` do original mede de um [`EditorMatches`] (sem o `immutable`, ver as notas do módulo).
#[derive(Debug, PartialEq, Eq)]
struct MatchesSummary {
    editor_ids: Vec<String>,
    default_rule_index: usize,
    default_rule: (String, EditorMatchRuleSource, String),
    default_is_array_item: bool,
    natural_default_editor_id: String,
    natural_default_is_array_item: bool,
}

fn summarize(matches: &EditorMatches) -> MatchesSummary {
    let default_rule = matches.default_rule();
    MatchesSummary {
        editor_ids: matches.matches().iter().map(|rule| rule.editor.id.clone()).collect(),
        default_rule_index: matches.default_rule_index(),
        default_rule: (default_rule.editor.id.clone(), default_rule.source(), default_rule.association_pattern.clone()),
        default_is_array_item: std::ptr::eq(default_rule, &matches.matches()[matches.default_rule_index()]),
        natural_default_editor_id: matches.natural_default_rule().editor.id.clone(),
        natural_default_is_array_item: std::ptr::eq(matches.natural_default_rule(), &matches.matches()[matches.natural_default_rule_index()]),
    }
}

#[test]
fn get_editor_matches_derives_the_default_rule_from_the_complete_match_set() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let factory = EditorInputFactoryObject {
            create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(editor.resource.clone().unwrap(), TEST_EDITOR_INPUT_ID))),
            create_diff_editor_input: Some(editor_factory(|editor| {
                let modified = editor.modified.as_ref().and_then(|modified| modified.resource.clone()).expect("Expected modified resource.");
                TestFileEditorInput::new(modified, TEST_EDITOR_INPUT_ID)
            })),
            ..EditorInputFactoryObject::default()
        };
        let default_association = default_editor_association();
        env.register_disposable(
            "*",
            info(DEFAULT_EDITOR_ASSOCIATION_ID, &default_association.display_name, None, RegisteredEditorPriority::Builtin),
            RegisteredEditorOptions::default(),
            factory.clone(),
        );
        env.register_disposable(
            "file:/**/*.html",
            info("test.browser", "Browser", None, RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            factory.clone(),
        );
        env.register_disposable(
            "*.component.html",
            info("test.componentEditor", "Component Editor", None, priorities(RegisteredEditorPriority::Default, RegisteredEditorPriority::Explicit, None)),
            RegisteredEditorOptions::default(),
            factory,
        );

        let component_resource = uri_file("/workspace/example.component.html");
        let html_resource = uri_file("/workspace/example.html");
        let diff = EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(true) };
        let component = summarize(&env.service.get_editor_matches(&component_resource, None));
        let component_diff = summarize(&env.service.get_editor_matches(&component_resource, Some(&diff)));
        let html = summarize(&env.service.get_editor_matches(&html_resource, None));

        assert_eq!(
            component,
            MatchesSummary {
                editor_ids: strings(&["test.componentEditor", DEFAULT_EDITOR_ASSOCIATION_ID, "test.browser"]),
                default_rule_index: 0,
                default_rule: ("test.componentEditor".to_string(), EditorMatchRuleSource::EditorRegistration, "*.component.html".to_string()),
                default_is_array_item: true,
                natural_default_editor_id: "test.componentEditor".to_string(),
                natural_default_is_array_item: true,
            }
        );
        let fallback = |editor_ids: Vec<String>| MatchesSummary {
            editor_ids,
            default_rule_index: 0,
            default_rule: (DEFAULT_EDITOR_ASSOCIATION_ID.to_string(), EditorMatchRuleSource::Fallback, "*.html".to_string()),
            default_is_array_item: true,
            natural_default_editor_id: DEFAULT_EDITOR_ASSOCIATION_ID.to_string(),
            natural_default_is_array_item: true,
        };
        assert_eq!(component_diff, fallback(strings(&[DEFAULT_EDITOR_ASSOCIATION_ID, "test.browser", "test.componentEditor"])));
        assert_eq!(html, fallback(strings(&[DEFAULT_EDITOR_ASSOCIATION_ID, "test.browser"])));

        let thrown = errors::catch(|| EditorMatches::new(Vec::new(), 0, 0, false));
        assert!(matches!(thrown, Err(Error { ref name, .. }) if name == "RangeError"));
    });
}

#[test]
fn get_editor_matches_reports_the_user_association_that_selected_the_default() {
    run_async(|disposables| async move {
        let configuration_service = configuration(&[(EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.html", DEFAULT_EDITOR_ASSOCIATION_ID)])]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        env.register_default_editor_test_editors(None);

        let matches = env.service.get_editor_matches(&uri_file("/workspace/example.component.html"), None);
        let default_rule = matches.default_rule();
        assert!(std::ptr::eq(default_rule, &matches.matches()[matches.default_rule_index()]));
        assert_eq!(default_rule.editor.id, DEFAULT_EDITOR_ASSOCIATION_ID);
        assert_eq!(matches.natural_default_rule().editor.id, "test.componentEditor");
        assert_eq!(default_rule.source(), EditorMatchRuleSource::UserAssociation);
        assert_eq!(default_rule.association_pattern, "*.html");
    });
}

#[test]
fn set_default_editor_removes_the_association_when_restoring_the_natural_registered_default() {
    run_async(|disposables| async move {
        let configuration_service = mutable_configuration(&[(EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.component.html", DEFAULT_EDITOR_ASSOCIATION_ID)])]);
        let env = create_env_with_configuration(&disposables, configuration_service.clone()).await;
        env.register_default_editor_test_editors(None);
        let resource = uri_file("/workspace/example.component.html");

        env.service.set_default_editor(&resource, "test.componentEditor", None);
        env.service.set_default_editor(&resource, "test.componentEditor", None);

        assert_eq!(user_associations(&env.service), Vec::<(String, String)>::new());
        assert_eq!(env.service.get_editor_matches(&resource, None).default_rule().editor.id, "test.componentEditor");
        assert_eq!(configuration_service.update_value_calls().len(), 1);
    });
}

#[test]
fn set_default_editor_retains_the_association_when_a_broader_rule_masks_the_natural_default() {
    run_async(|disposables| async move {
        let configuration_service = mutable_configuration(&[(
            EDITORS_ASSOCIATIONS_SETTING_ID,
            &[("*.html", DEFAULT_EDITOR_ASSOCIATION_ID), ("*.component.html", DEFAULT_EDITOR_ASSOCIATION_ID)],
        )]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        env.register_default_editor_test_editors(None);
        let resource = uri_file("/workspace/example.component.html");

        env.service.set_default_editor(&resource, "test.componentEditor", None);

        assert_eq!(
            user_associations(&env.service),
            vec![("*.html".to_string(), DEFAULT_EDITOR_ASSOCIATION_ID.to_string()), ("*.component.html".to_string(), "test.componentEditor".to_string())]
        );
        assert_eq!(env.service.get_editor_matches(&resource, None).default_rule().editor.id, "test.componentEditor");
    });
}

#[test]
fn set_default_editor_retains_the_diff_association_when_the_inherited_rule_masks_the_natural_default() {
    run_async(|disposables| async move {
        let configuration_service = mutable_configuration(&[
            (EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.html", DEFAULT_EDITOR_ASSOCIATION_ID)]),
            (DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.component.html", DEFAULT_EDITOR_ASSOCIATION_ID)]),
        ]);
        let env = create_env_with_configuration(&disposables, configuration_service.clone()).await;
        env.register_default_editor_test_editors(None);
        let resource = uri_file("/workspace/example.component.html");

        env.service.set_default_editor(&resource, "test.componentEditor", Some(true));

        let diff_associations: Vec<(String, String)> = configuration_service
            .get_value(Some(DIFF_EDITORS_ASSOCIATIONS_SETTING_ID), None)
            .own_entries()
            .into_iter()
            .map(|(pattern, editor)| match editor {
                Dynamic::String(editor) => (pattern.to_string_lossy(), editor.to_string_lossy()),
                other => panic!("esperava um texto, veio {other:?}"),
            })
            .collect();
        assert_eq!(diff_associations, vec![("*.component.html".to_string(), "test.componentEditor".to_string())]);
        assert_eq!(
            env.service.get_editor_matches(&resource, Some(&EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(true) })).default_rule().editor.id,
            "test.componentEditor"
        );
    });
}

/// O `TestConfigurationService` anônimo do caso `retains the association supplied by the default configuration layer`:
/// `inspect` devolve o valor mesclado da camada padrão com a do usuário, e `updateValue` guarda as associações do usuário.
struct DefaultLayerConfigurationService {
    inner: Rc<TestConfigurationService>,
    default_associations: Vec<(String, String)>,
    user_associations: RefCell<Option<Vec<(String, String)>>>,
}

impl DefaultLayerConfigurationService {
    fn dynamic(entries: &[(String, String)]) -> Dynamic {
        Dynamic::object(entries.iter().map(|(pattern, editor)| (pattern.as_str(), Dynamic::string(editor))))
    }
}

impl IConfigurationService for DefaultLayerConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.inner.on_did_change_configuration()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        self.inner.get_configuration_data()
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        self.inner.get_value(section, overrides)
    }

    fn update_value(&self, key: &str, value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, _target: Option<ConfigurationTarget>) -> Promise<()> {
        let Dynamic::Object(_) = &value else {
            return Promise::rejected(Error::new("Expected editor associations."));
        };
        if key != EDITORS_ASSOCIATIONS_SETTING_ID {
            return Promise::rejected(Error::new("Expected editor associations."));
        }
        let mut entries = Vec::new();
        for (pattern, editor) in value.own_entries() {
            match editor {
                Dynamic::String(editor) => entries.push((pattern.to_string_lossy(), editor.to_string_lossy())),
                _ => return Promise::rejected(Error::new("Expected editor association values.")),
            }
        }
        *self.user_associations.borrow_mut() = Some(entries);
        Promise::resolved(())
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        if key != EDITORS_ASSOCIATIONS_SETTING_ID {
            return self.inner.inspect(key, overrides);
        }
        let user = self.user_associations.borrow().clone();
        // `{ ...defaultAssociations, ...userAssociations }`
        let mut merged = self.default_associations.clone();
        for (pattern, editor) in user.iter().flatten() {
            match merged.iter_mut().find(|(existing, _)| existing == pattern) {
                Some((_, existing)) => *existing = editor.clone(),
                None => merged.push((pattern.clone(), editor.clone())),
            }
        }
        let user_value = user.as_deref().map_or(Dynamic::Undefined, Self::dynamic);
        Rc::new(TestConfigurationValue {
            value: Self::dynamic(&merged),
            default_value: Self::dynamic(&self.default_associations),
            user_value: user_value.clone(),
            user_local_value: user_value,
            ..TestConfigurationValue::default()
        })
    }

    fn reload_configuration(&self) -> Promise<()> {
        self.inner.reload_configuration()
    }

    fn keys(&self) -> ConfigurationKeys {
        self.inner.keys()
    }
}

#[test]
fn set_default_editor_retains_the_association_supplied_by_the_default_configuration_layer() {
    run_async(|disposables| async move {
        let configuration_service = Rc::new(DefaultLayerConfigurationService {
            inner: TestConfigurationService::new(None),
            default_associations: vec![("*.component.html".to_string(), DEFAULT_EDITOR_ASSOCIATION_ID.to_string())],
            user_associations: RefCell::new(None),
        });
        let env = create_env_with_configuration(&disposables, configuration_service.clone()).await;
        env.register_default_editor_test_editors(None);
        let resource = uri_file("/workspace/example.component.html");

        env.service.set_default_editor(&resource, "test.componentEditor", None);

        assert_eq!(*configuration_service.user_associations.borrow(), Some(vec![("*.component.html".to_string(), "test.componentEditor".to_string())]));
        assert_eq!(env.service.get_editor_matches(&resource, None).default_rule().editor.id, "test.componentEditor");
    });
}

#[test]
fn set_default_editor_pins_the_selected_editor_when_natural_defaults_conflict() {
    run_async(|disposables| async move {
        let configuration_service = mutable_configuration(&[]);
        let env = create_env_with_configuration(&disposables, configuration_service.clone()).await;
        env.register_default_editor_test_editors(Some("test.additionalComponentEditor"));
        let resource = uri_file("/workspace/example.component.html");

        env.service.set_default_editor(&resource, "test.componentEditor", None);
        env.service.set_default_editor(&resource, "test.componentEditor", None);

        let matches = env.service.get_editor_matches(&resource, None);
        assert_eq!(user_associations(&env.service), vec![("*.component.html".to_string(), "test.componentEditor".to_string())]);
        assert_eq!(matches.default_rule().editor.id, "test.componentEditor");
        assert!(!matches.conflicting_default());
        assert_eq!(configuration_service.update_value_calls().len(), 2);
    });
}

#[test]
fn set_default_editor_ignores_a_more_specific_association_for_an_unregistered_editor() {
    run_async(|disposables| async move {
        let configuration_service = mutable_configuration(&[(EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.component.html", "test.unregisteredEditor")])]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        let factory = plain_factory(TEST_EDITOR_INPUT_ID);
        let default_association = default_editor_association();
        env.register_disposable(
            "*",
            info(DEFAULT_EDITOR_ASSOCIATION_ID, &default_association.display_name, None, RegisteredEditorPriority::Builtin),
            RegisteredEditorOptions::default(),
            factory.clone(),
        );
        env.register_disposable("*.html", info("test.htmlEditor", "HTML Editor", None, RegisteredEditorPriority::Default), RegisteredEditorOptions::default(), factory);
        let resource = uri_file("/workspace/example.component.html");

        let displayed_association_pattern = env.service.get_editor_matches(&resource, None).default_rule().association_pattern.clone();
        env.service.set_default_editor(&resource, DEFAULT_EDITOR_ASSOCIATION_ID, None);

        assert_eq!(displayed_association_pattern, "*.html");
        assert_eq!(
            user_associations(&env.service),
            vec![("*.component.html".to_string(), "test.unregisteredEditor".to_string()), ("*.html".to_string(), DEFAULT_EDITOR_ASSOCIATION_ID.to_string())]
        );
        assert_eq!(env.service.get_editor_matches(&resource, None).default_rule().editor.id, DEFAULT_EDITOR_ASSOCIATION_ID);
    });
}

#[test]
fn get_editors_uses_the_effective_diff_priority() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let resource = uri_file("/workspace/index.html");
        env.register_disposable(
            "*.html",
            info("test.exclusiveDiff", "Exclusive Diff", None, priorities(RegisteredEditorPriority::Option, RegisteredEditorPriority::Exclusive, None)),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(editor.resource.clone().unwrap(), TEST_EDITOR_INPUT_ID))),
                create_diff_editor_input: Some(Rc::new(|_editor: &UntypedEditorInput, _group: &Rc<dyn IEditorGroup>| panic!("Unexpected diff editor creation."))),
                ..EditorInputFactoryObject::default()
            },
        );

        let editor = test_editor_ids(&env.service.get_editors(&resource, None));
        let diff = test_editor_ids(&env.service.get_editors(
            &resource,
            Some(&EditorResolverServiceGetEditorsOptions { is_diff_editor: Some(true), ..EditorResolverServiceGetEditorsOptions::default() }),
        ));
        assert_eq!((editor, diff), (strings(&["test.exclusiveDiff"]), Vec::<String>::new()));
    });
}

#[test]
fn get_editors_preserves_configured_universal_optional_editors() {
    run_async(|disposables| async move {
        let configuration_service = configuration(&[(EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.html", "test.universalOptional")])]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;
        let resource = uri_file("/workspace/index.html");
        env.register_disposable(
            "*",
            info("test.universalOptional", "Universal Optional", None, RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(editor.resource.clone().unwrap(), TEST_EDITOR_INPUT_ID))),
                ..EditorInputFactoryObject::default()
            },
        );

        assert_eq!(
            test_editor_ids(&env.service.get_editors(
                &resource,
                Some(&EditorResolverServiceGetEditorsOptions { exclude_unconfigured_universal_optional_editors: Some(true), ..EditorResolverServiceGetEditorsOptions::default() })
            )),
            strings(&["test.universalOptional"])
        );
    });
}

#[test]
fn multiple_registrations_to_same_glob_and_id_155859() {
    run_async(|disposables| async move {
        let env = create_default_env(&disposables).await;
        let test_editor_info = info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Default);
        let registered_single_editor = env.register(
            "*.test",
            test_editor_info.clone(),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(reparse(&editor.resource.clone().unwrap()), TEST_EDITOR_INPUT_ID))),
                ..EditorInputFactoryObject::default()
            },
        );

        let ctx = env.ctx.clone();
        let registered_diff_editor = env.register(
            "*.test",
            test_editor_info,
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_diff_editor_input: Some(editor_ref_factory(move |_editor| ctx.diff_of_object_object_uris())),
                ..EditorInputFactoryObject::default()
            },
        );

        // Resolve a diff
        let resolved = expect_editor(env.resolve(diff_untyped(uri_file("my://resource-basics.test"), uri_file("my://resource-basics.test"))).await);
        assert_eq!(resolved.editor.editor_type_id(), DiffEditorInput::ID);
        resolved.editor.dispose();

        // Remove diff registration
        registered_diff_editor.dispose();

        // Resolve a diff again, expected failure
        let resulting_resolution = env.resolve(diff_untyped(uri_file("my://resource-basics.test"), uri_file("my://resource-basics.test"))).await;
        assert!(matches!(resulting_resolution, ResolvedEditor::Status(ResolvedStatus::None)));

        registered_single_editor.dispose();
    });
}

#[test]
fn user_configured_editor_association_resolves_on_first_startup_with_empty_cache_244597() {
    run_async(|disposables| async move {
        const CUSTOM_EDITOR_INPUT_ID: &str = "testCustomEditorInput";

        // Set up a configuration with a user-configured editor association
        let configuration_service = configuration(&[(EDITORS_ASSOCIATIONS_SETTING_ID, &[("*.md", "CUSTOM_MD_EDITOR")])]);
        let env = create_env_with_configuration(&disposables, configuration_service).await;

        // Register both the default text editor and the custom markdown editor with 'option' priority
        // (matching how markdown preview is registered in package.json)
        let default_editor = env.register(
            "*",
            info("default", "Default Editor", Some("Default"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(reparse(&editor.resource.clone().unwrap()), TEST_EDITOR_INPUT_ID))),
                ..EditorInputFactoryObject::default()
            },
        );

        let custom_editor = env.register(
            "*.md",
            info("CUSTOM_MD_EDITOR", "Markdown Preview", Some("Markdown Preview Details"), RegisteredEditorPriority::Option),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(editor_factory(|editor| TestFileEditorInput::new(reparse(&editor.resource.clone().unwrap()), CUSTOM_EDITOR_INPUT_ID))),
                ..EditorInputFactoryObject::default()
            },
        );

        // Resolve a .md file - should use the custom editor due to user association
        let resulting_resolution = env.resolve(untyped(uri_file("test.md"))).await;
        let resolved = expect_editor(resulting_resolution);
        assert_eq!(resolved.editor.editor_type_id(), CUSTOM_EDITOR_INPUT_ID, "Should resolve to custom editor when user has configured editor association");
        resolved.editor.dispose();

        default_editor.dispose();
        custom_editor.dispose();
    });
}

#[test]
fn diff_editor_resolve_priority_diff_overrides_priority_editor_for_diffs() {
    run_async(|disposables| async move {
        const CUSTOM_EDITOR_INPUT_ID: &str = "testCustomEditorForDiffPriority";
        let env = create_default_env(&disposables).await;
        let ctx = env.ctx.clone();
        let registered_editor = env.register(
            "*.test-diff-priority",
            info(
                "TEST_EDITOR",
                "Test Editor Label",
                Some("Test Editor Details"),
                priorities(RegisteredEditorPriority::Default, RegisteredEditorPriority::Option, Some(RegisteredEditorPriority::Default)),
            ),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(reparsed_file_factory(&env.ctx, CUSTOM_EDITOR_INPUT_ID)),
                create_diff_editor_input: Some(editor_ref_factory(move |_editor| {
                    let original = ctx.file(object_object_uri(), CUSTOM_EDITOR_INPUT_ID);
                    let modified = ctx.file(object_object_uri(), CUSTOM_EDITOR_INPUT_ID);
                    ctx.diff_of_inputs(original, modified)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        // Regular editor should use custom editor (priority.editor: default)
        let editor_resolution = env.resolve(untyped(uri_file("my://resource.test-diff-priority"))).await;
        let resolved = expect_editor(editor_resolution);
        assert_eq!(resolved.editor.editor_type_id(), CUSTOM_EDITOR_INPUT_ID);
        resolved.editor.dispose();

        // Diff editor should NOT use custom editor (priority.diff: option)
        let diff_resolution = env.resolve(diff_untyped(uri_file("my://resource.test-diff-priority"), uri_file("my://resource.test-diff-priority"))).await;
        // With priority.diff: option, the custom editor should not be selected as default
        if let ResolvedEditor::Editor(resolved) = diff_resolution {
            assert_ne!(resolved.editor.editor_type_id(), CUSTOM_EDITOR_INPUT_ID, "Custom editor with priority.diff:option should not be used for diffs");
            resolved.editor.dispose();
        }

        registered_editor.dispose();
    });
}

#[test]
fn diff_editor_resolve_string_priority_expands_to_diff_priority() {
    run_async(|disposables| async move {
        const CUSTOM_EDITOR_INPUT_ID: &str = "testCustomEditorNoDiffPriority";
        let env = create_default_env(&disposables).await;
        let ctx = env.ctx.clone();
        let registered_editor = env.register(
            "*.test-no-diff-priority",
            info("TEST_EDITOR", "Test Editor Label", Some("Test Editor Details"), RegisteredEditorPriority::Default),
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(reparsed_file_factory(&env.ctx, CUSTOM_EDITOR_INPUT_ID)),
                create_diff_editor_input: Some(editor_ref_factory(move |_editor| {
                    let original = ctx.file(object_object_uri(), CUSTOM_EDITOR_INPUT_ID);
                    let modified = ctx.file(object_object_uri(), CUSTOM_EDITOR_INPUT_ID);
                    ctx.diff_of_inputs(original, modified)
                })),
                ..EditorInputFactoryObject::default()
            },
        );

        // Diff editor should use custom editor since string priority expands to priority.diff: default
        let diff_resolution = env.resolve(diff_untyped(uri_file("my://resource.test-no-diff-priority"), uri_file("my://resource.test-no-diff-priority"))).await;
        let resolved = expect_editor(diff_resolution);
        assert_eq!(resolved.editor.editor_type_id(), DiffEditorInput::ID);
        resolved.editor.dispose();

        registered_editor.dispose();
    });
}
