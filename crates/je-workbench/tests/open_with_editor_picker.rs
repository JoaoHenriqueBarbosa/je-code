//! Porte de `src/vs/workbench/services/editor/test/browser/openWithEditorPicker.test.ts` (suíte `OpenWithEditorPicker`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` é [`setup`]; o `teardown` (`disposables.clear()`) é o `drop` do `LeakGuard` ao fim do
//!   corpo, que também faz o `ensureNoDisposablesAreLeakedInTestSuite()`. O fixture fica vivo até o fim do corpo assíncrono.
//! - **`workbenchInstantiationService({ configurationService: () => configurationService }, disposables)`,
//!   `createEditorPart` e `EditorResolverService`.** São o [`EditorServiceAccessor`] (as divergências dele estão em
//!   `support/editor_service_support.rs`) criado por `with_configuration_service`: a parte, o `IEditorGroupsService` (o
//!   `instantiationService.stub(IEditorGroupsService, part)` do original) e o resolvedor já vêm montados nele, em vez de o caso
//!   criar um segundo `EditorResolverService` por `createInstance`. Como no teste do resolvedor, o `TextEditorService` do
//!   acessor (que o original só cria sob demanda e nenhum caso usa) é descartado, para não existir um editor `*` além do que o
//!   caso registra.
//! - **`class extends TestConfigurationService { override async updateValue(...) { await this.setUserConfiguration(...) } }`.** É
//!   o [`TestConfigurationService`] com `stub_update_value(SetsUserConfiguration)`, o mesmo comportamento.
//! - **`TestQuickPick` e `upcastPartial<IQuickInputService>({ createQuickPick })`.** Estão em
//!   `support/open_with_editor_picker_support.rs`. O `respond.shift()` que lança `Unexpected quick pick.` é `throw`.
//! - **`new TestFileEditorInput(resource, editorId)` das fábricas.** É `TestFileEditorInput::new`, registrado em `disposables`.
//! - **`IEditorOptions` do resultado de `pick`.** São as [`TextEditorOptions`]; o `deepStrictEqual` sobre `{ pinned, override,
//!   preserveFocus }` é a comparação do resumo [`PickedOptions`] (os demais campos nascem da entrada e não são tocados).
//! - **`getAssociations`.** `{ ...configurationService.getValue(settingId) }` é a lista (chave, editor) do objeto, vazia quando o
//!   valor não é um objeto.
//! - **`CommandsRegistry.getCommand(CONFIGURE_DEFAULT_EDITOR_COMMAND_ID)!.handler(accessor, { resource }, currentEditor,
//!   part.activeGroup.id)`.** O registro roda na carga do módulo do original e aqui é
//!   [`register_configure_default_editor_command`] (por caso, no registro do thread); o handler recebe o `{ resource }` e o
//!   `EditorInput` como objetos de carga nativa (ver `open_with_editor_picker.rs`) e devolve a promessa do corpo `async`.
//! - **Relógio.** O original não usa timers falsos: os casos rodam em `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_base::lifecycle::IDisposable;
use je_workbench::services::editor::editor_groups_service::{IEditorGroupsService, IEditorPart};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_base::promise::MaybePromise;
use je_base::uri::URI;
use je_platform::commands::commands::commands_registry;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::{TestConfigurationService, UpdateValueBehavior};
use je_platform::opener::editor_types::EditorOptions;
use je_platform::quickinput::quick_input::{IQuickInput, IQuickInputService, IQuickPick, I_QUICK_INPUT_SERVICE, QuickPickInput};
use je_platform::resources::uri_file;
use je_workbench::common::editor::{DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputRef, EditorInputWithOptions, TextEditorOptions, UntypedEditorInput, default_editor_association};
use je_workbench::services::editor::editor_groups_service::IEditorGroup;
use je_workbench::services::editor::editor_resolver_service::{
    DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, EDITORS_ASSOCIATIONS_SETTING_ID, EditorInputFactoryFunction, EditorInputFactoryObject, GlobPattern, IEditorResolverService,
    RegisteredEditorOptions, RegisteredEditorPriority, RegisteredEditorRegistrationInfo, RegisteredEditorRegistrationPriority,
};
use je_workbench::services::editor::open_with_editor_picker::{
    CONFIGURE_DEFAULT_EDITOR_COMMAND_ID, OpenWithEditorPicker, configure_default_editor_command_result, configure_default_editor_current_editor_argument,
    configure_default_editor_untyped_argument, register_configure_default_editor_command,
};
use je_workbench::test::browser::workbench_test_services::TestFileEditorInput;
use support::editor_service_support::EditorServiceAccessor;
use support::open_with_editor_picker_support::{PartialQuickInputService, TestQuickPick};

//#region Infraestrutura da suíte

const COMPONENT_EDITOR_ID: &str = "test.componentEditor";

/// `resource`: `URI.file('/workspace/example.component.html')`.
fn resource() -> URI {
    uri_file("/workspace/example.component.html")
}

/// `IShownPicker`.
#[derive(Debug, PartialEq)]
struct ShownPicker {
    placeholder: Option<String>,
    items: Vec<String>,
}

/// O resumo `{ pinned, override, preserveFocus }` das opções devolvidas.
#[derive(Debug, PartialEq)]
struct PickedOptions {
    pinned: Option<bool>,
    override_id: Option<String>,
    preserve_focus: Option<bool>,
}

fn summarize(options: &TextEditorOptions) -> PickedOptions {
    PickedOptions { pinned: options.pinned, override_id: options.override_id(), preserve_focus: options.preserve_focus }
}

/// Um item de `respond: ((quickPick: TestQuickPick) => void)[]`.
type Responder = Box<dyn FnOnce(&TestQuickPick)>;

/// `quickPick => quickPick.acceptItem(id, inBackground)`.
fn accept(id: &'static str, in_background: bool) -> Responder {
    Box::new(move |quick_pick| quick_pick.accept_item(id, in_background))
}

/// `quickPick => quickPick.hide()`.
fn hide() -> Responder {
    Box::new(|quick_pick| IQuickInput::hide(quick_pick))
}

/// O que `setup` devolve: `{ instantiationService, part, configurationService, shown }` e o acessor que mantém os serviços vivos.
struct Fixture {
    accessor: EditorServiceAccessor,
    configuration_service: Rc<TestConfigurationService>,
    shown: Rc<RefCell<Vec<ShownPicker>>>,
}

/// Uma fábrica de entrada do teste: `({ resource }) => ({ editor: disposables.add(new TestFileEditorInput(resource, editorId)) })`.
fn factory(disposables: &Rc<DisposableStore>, editor_id: &'static str) -> EditorInputFactoryObject {
    let create = |disposables: Rc<DisposableStore>, resource_of: fn(&UntypedEditorInput) -> URI| -> EditorInputFactoryFunction {
        Rc::new(move |editor: &UntypedEditorInput, _group: &Rc<dyn IEditorGroup>| {
            let created = disposables.add_rc(TestFileEditorInput::new(resource_of(editor), editor_id));
            let created: EditorInputRef = created;
            MaybePromise::Value(EditorInputWithOptions { editor: created, options: None })
        })
    };

    EditorInputFactoryObject {
        create_editor_input: Some(create(disposables.clone(), |editor| editor.resource.clone().expect("recurso"))),
        create_diff_editor_input: Some(create(disposables.clone(), |editor| {
            editor.modified.as_ref().and_then(|modified| modified.resource.clone()).expect("recurso modificado")
        })),
        ..EditorInputFactoryObject::default()
    }
}

/// `RegisteredEditorRegistrationInfo` de `{ id, label, priority }`.
fn info(id: &str, label: &str, priority: RegisteredEditorPriority) -> RegisteredEditorRegistrationInfo {
    RegisteredEditorRegistrationInfo { id: id.to_string(), label: label.to_string(), detail: None, priority: RegisteredEditorRegistrationPriority::Single(priority) }
}

/// Sets up an editor resolver with the text editor and a component editor, and a quick input service whose quick picks are
/// answered by `respond` in the order in which they are shown.
async fn setup(disposables: &LeakGuard, configuration: Dynamic, respond: Vec<Responder>) -> Fixture {
    let store = disposables.store();
    let configuration_service = TestConfigurationService::new(Some(configuration));
    configuration_service.stub_update_value(UpdateValueBehavior::SetsUserConfiguration);
    disposables.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let accessor = {
        let configuration_service = configuration_service.clone();
        EditorServiceAccessor::with_configuration_service(&store, move |_| -> Rc<dyn IConfigurationService> { configuration_service })
    };
    accessor.part.when_ready().await.unwrap();
    accessor.text_editor_service.dispose();

    let editor_resolver_service = accessor.editor_resolver_service.clone();
    let default_association = default_editor_association();
    disposables.add(editor_resolver_service.register_editor(
        GlobPattern::from("*"),
        info(DEFAULT_EDITOR_ASSOCIATION_ID, &default_association.display_name, RegisteredEditorPriority::Builtin),
        RegisteredEditorOptions::default(),
        factory(&store, DEFAULT_EDITOR_ASSOCIATION_ID),
    ));
    disposables.add(editor_resolver_service.register_editor(
        GlobPattern::from("*.component.html"),
        info(COMPONENT_EDITOR_ID, "Component Editor", RegisteredEditorPriority::Default),
        RegisteredEditorOptions::default(),
        factory(&store, COMPONENT_EDITOR_ID),
    ));

    let shown: Rc<RefCell<Vec<ShownPicker>>> = Rc::default();
    let respond: Rc<RefCell<VecDeque<Responder>>> = Rc::new(RefCell::new(respond.into()));
    let quick_input_service = {
        let shown = shown.clone();
        PartialQuickInputService::new(move |_options| {
            let shown = shown.clone();
            let respond = respond.clone();
            let quick_pick: Rc<dyn IQuickPick> = TestQuickPick::new(move |quick_pick| {
                shown.borrow_mut().push(ShownPicker {
                    placeholder: IQuickPick::placeholder(quick_pick.as_ref()),
                    items: IQuickPick::items(quick_pick.as_ref())
                        .iter()
                        .map(|item| match item {
                            QuickPickInput::Separator(_) => "---".to_string(),
                            QuickPickInput::Item(item) => {
                                format!("{}: {}", item.base.id.as_deref().unwrap_or("undefined"), item.base.description.as_ref().map(|description| description.to_string_lossy()).unwrap_or_default())
                            }
                        })
                        .collect(),
                });
                let response = respond.borrow_mut().pop_front();
                let Some(response) = response else {
                    je_base::errors::throw(je_base::errors::Error::new("Unexpected quick pick."));
                };
                response(quick_pick);
            });
            quick_pick
        })
    };
    accessor.collection.set(I_QUICK_INPUT_SERVICE, quick_input_service as Rc<dyn IQuickInputService>);

    Fixture { accessor, configuration_service, shown }
}

/// `instantiationService.createInstance(OpenWithEditorPicker).pick(editor)`.
fn pick(fixture: &Fixture, editor: &UntypedEditorInput) -> je_base::promise::Promise<Option<TextEditorOptions>> {
    fixture.accessor.instantiation_service.create_instance::<OpenWithEditorPicker>(()).pick(editor, None)
}

/// `getAssociations(configurationService, settingId)`.
fn get_associations(configuration_service: &TestConfigurationService, setting_id: &str) -> Vec<(String, String)> {
    match configuration_service.get_value(Some(setting_id), None) {
        Dynamic::Object(object) => object
            .entries()
            .into_iter()
            .map(|(key, value)| (key.to_string_lossy(), if let Dynamic::String(editor) = value { editor.to_string_lossy() } else { String::new() }))
            .collect(),
        _ => Vec::new(),
    }
}

fn pinned_options() -> TextEditorOptions {
    TextEditorOptions::from(EditorOptions { pinned: Some(true), ..EditorOptions::default() })
}

//#endregion

#[test]
fn returns_the_options_to_open_the_picked_editor_with() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables, Dynamic::object(Vec::<(&str, Dynamic)>::new()), vec![accept(COMPONENT_EDITOR_ID, true)]).await;

        let options = pick(&fixture, &UntypedEditorInput { resource: Some(resource()), options: Some(pinned_options()), ..UntypedEditorInput::default() }).await.unwrap();

        assert_eq!(
            *fixture.shown.borrow(),
            vec![ShownPicker {
                placeholder: Some("Select editor for 'example.component.html'".to_string()),
                items: vec![
                    format!("{DEFAULT_EDITOR_ASSOCIATION_ID}: "),
                    format!("{COMPONENT_EDITOR_ID}: Default"),
                    "---".to_string(),
                    "promptOpenWith.configureDefault: ".to_string()
                ]
            }]
        );
        assert_eq!(options.as_ref().map(summarize), Some(PickedOptions { pinned: Some(true), override_id: Some(COMPONENT_EDITOR_ID.to_string()), preserve_focus: Some(true) }));

        drop(options);
        drop(fixture);
        drop(disposables);
    });
}

#[test]
fn returns_undefined_when_the_picker_is_cancelled() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables, Dynamic::object(Vec::<(&str, Dynamic)>::new()), vec![hide()]).await;

        assert!(pick(&fixture, &UntypedEditorInput { resource: Some(resource()), ..UntypedEditorInput::default() }).await.unwrap().is_none());

        drop(fixture);
        drop(disposables);
    });
}

#[test]
fn does_not_show_a_picker_for_side_by_side_editors() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables, Dynamic::object(Vec::<(&str, Dynamic)>::new()), vec![]).await;

        let side = || Box::new(UntypedEditorInput { resource: Some(resource()), ..UntypedEditorInput::default() });
        let options =
            pick(&fixture, &UntypedEditorInput { primary: Some(side()), secondary: Some(side()), options: Some(pinned_options()), ..UntypedEditorInput::default() })
                .await
                .unwrap();

        assert!(fixture.shown.borrow().is_empty());
        assert_eq!(options.as_ref().map(summarize), Some(PickedOptions { pinned: Some(true), override_id: None, preserve_focus: None }));

        drop(options);
        drop(fixture);
        drop(disposables);
    });
}

#[test]
fn configuring_the_general_default_from_a_diff_persists_it_and_clears_the_diff_only_default() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Dynamic::object([(DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, Dynamic::object([("*.component.html", Dynamic::string(COMPONENT_EDITOR_ID))]))]);
        let fixture = setup(&disposables, configuration, vec![accept("promptOpenWith.configureDefault", false), accept(DEFAULT_EDITOR_ASSOCIATION_ID, false)]).await;

        let side = || Some(Box::new(UntypedEditorInput { resource: Some(resource()), ..UntypedEditorInput::default() }));
        let options = pick(&fixture, &UntypedEditorInput { original: side(), modified: side(), ..UntypedEditorInput::default() }).await.unwrap();

        let placeholders: Vec<Option<String>> = fixture.shown.borrow().iter().map(|picker| picker.placeholder.clone()).collect();
        assert_eq!(placeholders, vec![Some("Select editor for 'example.component.html'".to_string()), Some("Select new default editor for '*.component.html'".to_string())]);
        assert_eq!(options.as_ref().map(summarize), Some(PickedOptions { pinned: None, override_id: Some(DEFAULT_EDITOR_ASSOCIATION_ID.to_string()), preserve_focus: None }));
        assert_eq!(
            get_associations(&fixture.configuration_service, EDITORS_ASSOCIATIONS_SETTING_ID),
            vec![("*.component.html".to_string(), DEFAULT_EDITOR_ASSOCIATION_ID.to_string())]
        );
        assert_eq!(get_associations(&fixture.configuration_service, DIFF_EDITORS_ASSOCIATIONS_SETTING_ID), Vec::<(String, String)>::new());

        drop(options);
        drop(fixture);
        drop(disposables);
    });
}

#[test]
fn configure_default_editor_command_replaces_the_editor_with_the_picked_default() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        disposables.add(register_configure_default_editor_command());
        let fixture = setup(&disposables, Dynamic::object(Vec::<(&str, Dynamic)>::new()), vec![accept(DEFAULT_EDITOR_ASSOCIATION_ID, false)]).await;
        let part = fixture.accessor.part.clone();
        let current_editor = disposables.add_rc(TestFileEditorInput::new(resource(), COMPONENT_EDITOR_ID));
        let current_editor_ref: EditorInputRef = current_editor;
        part.active_group().open_editor(&current_editor_ref, Some(EditorOptions { pinned: Some(true), ..EditorOptions::default() })).await.unwrap();

        let command = commands_registry().get_command(CONFIGURE_DEFAULT_EDITOR_COMMAND_ID).expect("o comando está registrado");
        let args = vec![
            configure_default_editor_untyped_argument(UntypedEditorInput { resource: Some(resource()), ..UntypedEditorInput::default() }),
            configure_default_editor_current_editor_argument(current_editor_ref.clone()),
            Dynamic::Number(part.active_group().id() as f64),
        ];
        let result = fixture.accessor.instantiation_service.invoke_function(|accessor| (command.handler)(accessor, &args));
        configure_default_editor_command_result(&result).expect("o comando devolve a promessa do corpo async").await.unwrap();

        let editors: Vec<Option<String>> = part.active_group().editors().iter().map(|editor| editor.editor_id()).collect();
        assert_eq!(editors, vec![Some(DEFAULT_EDITOR_ASSOCIATION_ID.to_string())]);
        assert_eq!(
            get_associations(&fixture.configuration_service, EDITORS_ASSOCIATIONS_SETTING_ID),
            vec![("*.component.html".to_string(), DEFAULT_EDITOR_ASSOCIATION_ID.to_string())]
        );

        drop(args);
        drop(result);
        drop(current_editor_ref);
        drop(part);
        drop(fixture);
        drop(disposables);
    });
}
