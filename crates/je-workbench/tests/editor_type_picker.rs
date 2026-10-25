//! Porte de `src/vs/workbench/test/browser/parts/editor/editorTypePicker.test.ts` (suíte `Editor Type Picker`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **Os laços do original.** Os dois `for` que geram `test(...)` (`capabilities` e `isDiffEditor`) viram um `#[test]` por
//!   combinação, com o título do mocha no nome (`side-open alternatives respect source capabilities 8` é
//!   `side_open_alternatives_respect_source_capabilities_8`), todos sobre o mesmo corpo parametrizado. São 9 casos gerados pelos 7
//!   blocos `test(...)` do arquivo.
//! - **`mock<T>()`.** O `IEditorResolverService` é o [`PartialEditorResolverService`] (`getEditorMatches` e `setDefaultEditor` são os
//!   que o caso sobrescreve, o resto lança `X is not a function`), o `IEditorService` sem sobrescrita é o
//!   [`RecordingEditorService`] (que grava o `openEditor` e roda o gancho do caso) e o `ICommandService` é um dublê que grava
//!   `executeCommand`. O grupo e o painel são dublês de `IEditorGroupView` e `IEditorPane`; o `activateGroup(group) { activate(group) }`
//!   recebe o número do grupo.
//! - **`EditorInput`.** `new class extends EditorInput { ... }` é o [`PickerInput`], com o recurso, o `typeId`, o `editorId`, as
//!   capacidades, o `toUntyped` e o `diffResources` que cada caso define.
//! - **`assert.deepStrictEqual` de objetos.** É a comparação de tuplas dos mesmos campos, na ordem do original. As opções do editor
//!   aberto (`{ ...input, options: { override, pinned, preserveFocus, selection, selectionRevealType } }`) viram o [`OpenView`]; o
//!   `selection` é o `ITextEditorSelection` que a [`TextEditorPaneSelection`] devolve (o `restore` do porte guarda a faixa da
//!   seleção, o `textSelection` do original). `await action.run()` é esperar a promessa dentro de `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
mod editor_input_services;
mod support;
#[path = "support/breadcrumbs_control_support.rs"]
mod breadcrumbs_control_support;
#[path = "support/main_thread_support.rs"]
mod main_thread_support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use breadcrumbs_control_support::{OpenedEditor, PartialEditorResolverService, RecordingEditorService};
use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::actions::{IAction, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::equals::structural_equals;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::selection::Selection;
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::opener::editor_types::{EditorOptions, EditorOverride, TextEditorSelection};
use je_platform::resources::uri_file;
use je_workbench::browser::parts::editor::editor::{IEditorGroupView, IEditorGroupsView};
use je_workbench::browser::parts::editor::editor_type_picker::{IAvailableEditorTypes, create_editor_type_actions, get_available_editor_types};
use je_workbench::browser::parts::editor::text_editor::TextEditorPaneSelection;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputCapabilities, EditorInputRef, GroupIdentifier, IEditorPane, IEditorPaneSelection, TextEditorOptions, TextEditorSelectionRevealType,
    UntypedEditorInput,
};
use je_workbench::services::editor::editor_groups_service::{EditorPartOptions, EditorPartOptionsChangeEvent, IEditorGroup, SIDE_GROUP};
use je_workbench::services::editor::editor_resolver_service::{
    EditorMatchRule, EditorMatchRuleOrigin, EditorMatchRuleSource, EditorMatches, EditorResolverServiceGetEditorMatchesOptions, GlobPattern, RegisteredEditorInfo,
    RegisteredEditorPriority, RegisteredEditorPriorityInfo,
};
use je_workbench::services::layout::layout_service::WindowId;

const GROUP_ID: GroupIdentifier = 42;

fn not_defined(member: &str) -> ! {
    panic!("{member} is not a function")
}

//#region Dublês

/// Um `new class extends EditorInput { ... }` do original: o que o caso define (`resource`, `typeId`, `editorId`, `capabilities`,
/// `toUntyped` e `diffResources`), e `getName() { return 'test'; }`.
#[derive(Default)]
struct PickerInputSpec {
    type_id: &'static str,
    editor_id: Option<&'static str>,
    resource: Option<URI>,
    capabilities: EditorInputCapabilities,
    untyped: Option<UntypedEditorInput>,
    diff_resources: Option<(URI, URI)>,
}

struct PickerInput {
    base: EditorInputBase,
    spec: PickerInputSpec,
}

impl PickerInput {
    fn new(spec: PickerInputSpec) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), spec })
    }
}

impl IDisposable for PickerInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for PickerInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        self.spec.type_id.to_owned()
    }

    fn resource(&self) -> Option<URI> {
        self.spec.resource.clone()
    }

    fn editor_id(&self) -> Option<String> {
        self.spec.editor_id.map(str::to_owned)
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        self.spec.capabilities
    }

    fn get_name(&self) -> String {
        "test".to_owned()
    }

    fn to_untyped(&self, _options: Option<&je_workbench::common::editor::editor_input::UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        self.spec.untyped.clone()
    }

    fn diff_resources(&self) -> Option<(URI, URI)> {
        self.spec.diff_resources.clone()
    }
}

/// `getSelection?.()`: a seleção que o painel devolve a cada chamada.
type SelectionFn = Rc<dyn Fn() -> Rc<dyn IEditorPaneSelection>>;

/// `override readonly activeEditorPane = new class extends mock<IEditorPane>() { input = sourceEditor; getSelection() {...} }`.
struct TestPane {
    input: EditorInputRef,
    selection: Option<SelectionFn>,
}

impl IEditorPane for TestPane {
    fn input(&self) -> Option<EditorInputRef> {
        Some(self.input.clone())
    }

    fn group_id(&self) -> GroupIdentifier {
        not_defined("group")
    }

    fn get_view_state(&self) -> Option<Dynamic> {
        not_defined("getViewState")
    }

    fn on_did_change_control(&self) -> Event<()> {
        not_defined("onDidChangeControl")
    }

    fn get_selection(&self) -> Option<Rc<dyn IEditorPaneSelection>> {
        self.selection.as_ref().map(|selection| selection())
    }
}

/// `override readonly groupsView = new class extends mock<IEditorGroupsView>() { activateGroup(group) { activate(group); return group; } }`.
struct TestGroupsView {
    activate: Rc<dyn Fn(GroupIdentifier)>,
}

impl IEditorGroupsView for TestGroupsView {
    fn activate_group(&self, group: &Rc<dyn IEditorGroupView>) {
        (self.activate)(group.id());
    }

    fn part_options(&self) -> EditorPartOptions {
        not_defined("partOptions")
    }

    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent> {
        not_defined("onDidChangeEditorPartOptions")
    }
}

/// O grupo que `createGroup` devolve: `id = 42`, `activeEditor`, `activeEditorPane`, `getIndexOfEditor() { return 3; }` e `groupsView`.
struct TestGroup {
    source_editor: EditorInputRef,
    pane: Rc<dyn IEditorPane>,
    groups_view: Rc<dyn IEditorGroupsView>,
}

impl IEditorGroup for TestGroup {
    fn id(&self) -> GroupIdentifier {
        GROUP_ID
    }

    fn window_id(&self) -> WindowId {
        not_defined("windowId")
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        Some(self.source_editor.clone())
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        not_defined("editors")
    }

    fn is_locked(&self) -> bool {
        not_defined("isLocked")
    }

    fn is_active(&self, _editor: &je_workbench::common::editor::EditorInputOrUntyped) -> bool {
        not_defined("isActive")
    }

    fn active_editor_pane(&self) -> Option<Rc<dyn IEditorPane>> {
        Some(self.pane.clone())
    }

    fn get_index_of_editor(&self, _editor: &EditorInputRef) -> i64 {
        3
    }
}

impl IEditorGroupView for TestGroup {
    fn groups_view(&self) -> Rc<dyn IEditorGroupsView> {
        self.groups_view.clone()
    }
}

/// `createGroup(input, activate, getSelection, capabilities)`.
fn create_group(
    guard: &LeakGuard,
    input: Option<UntypedEditorInput>,
    activate: Rc<dyn Fn(GroupIdentifier)>,
    selection: Option<SelectionFn>,
    capabilities: EditorInputCapabilities,
) -> Rc<dyn IEditorGroupView> {
    let source_editor: EditorInputRef = guard.add_rc(PickerInput::new(PickerInputSpec { type_id: "test.editor", capabilities, untyped: input, ..PickerInputSpec::default() }));
    let pane: Rc<dyn IEditorPane> = Rc::new(TestPane { input: source_editor.clone(), selection });
    Rc::new(TestGroup { source_editor, pane, groups_view: Rc::new(TestGroupsView { activate }) })
}

/// `new class extends mock<ICommandService>() { override async executeCommand(id, ...args) { commands.push(...); return undefined; } }`.
#[derive(Default)]
struct RecordingCommandService {
    calls: RefCell<Vec<(String, Vec<Dynamic>)>>,
}

impl ICommandService for RecordingCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        not_defined("onWillExecuteCommand")
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        not_defined("onDidExecuteCommand")
    }

    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.calls.borrow_mut().push((command_id.to_owned(), args));
        Promise::resolved(Dynamic::Undefined)
    }
}

//#endregion

//#region Auxiliares

/// `editor(id, editorPriority, diffPriority = editorPriority)`.
fn editor(id: &str, editor_priority: RegisteredEditorPriority, diff_priority: Option<RegisteredEditorPriority>) -> RegisteredEditorInfo {
    RegisteredEditorInfo {
        id: id.to_owned(),
        label: id.to_owned(),
        detail: None,
        priority: RegisteredEditorPriorityInfo { editor: editor_priority, diff: diff_priority.unwrap_or(editor_priority), merge: editor_priority },
    }
}

/// Uma regra que `editorMatches` e os casos montam: `{ editor, priority: editor.priority.editor, source: EditorRegistration,
/// globPattern, associationPattern }`.
fn registration_rule(editor: &RegisteredEditorInfo, glob_pattern: &str, association_pattern: &str) -> EditorMatchRule {
    EditorMatchRule {
        editor: editor.clone(),
        priority: editor.priority.editor,
        association_pattern: association_pattern.to_owned(),
        origin: EditorMatchRuleOrigin::EditorRegistration { glob_pattern: GlobPattern::from(glob_pattern) },
    }
}

/// `editorMatches(editors, defaultEditorId = DEFAULT_EDITOR_ASSOCIATION.id)`.
fn editor_matches(editors: &[RegisteredEditorInfo], default_editor_id: &str) -> EditorMatches {
    let matches: Vec<EditorMatchRule> = editors.iter().map(|editor| registration_rule(editor, "*.test", "*.test")).collect();
    let default_rule_index = matches.iter().position(|rule| rule.editor.id == default_editor_id).expect("o editor padrão está entre os registrados");
    EditorMatches::new(matches, default_rule_index, default_rule_index, false)
}

/// O que `assert.deepStrictEqual` compara de um `EditorMatches`: as regras e os índices.
type MatchesView = (Vec<(RegisteredEditorInfo, RegisteredEditorPriority, String, EditorMatchRuleSource)>, usize, usize, bool);

fn matches_view(matches: &EditorMatches) -> MatchesView {
    (
        matches.matches().iter().map(|rule| (rule.editor.clone(), rule.priority, rule.association_pattern.clone(), rule.source())).collect(),
        matches.default_rule_index(),
        matches.natural_default_rule_index(),
        matches.conflicting_default(),
    )
}

/// `new class extends mock<IEditorResolverService>() { override getEditorMatches() { return matches; } }`.
fn resolver_returning(matches: &EditorMatches) -> Rc<PartialEditorResolverService> {
    let matches = matches.clone();
    Rc::new(PartialEditorResolverService { get_editor_matches: Some(Box::new(move |_, _| matches.clone())), ..PartialEditorResolverService::default() })
}

/// `new class extends TestEditorInput { typeId, editorId, resource, getName }`.
fn input(guard: &LeakGuard, type_id: &'static str, editor_id: &'static str, resource: &URI) -> EditorInputRef {
    guard.add_rc(PickerInput::new(PickerInputSpec { type_id, editor_id: Some(editor_id), resource: Some(resource.clone()), ..PickerInputSpec::default() }))
}

fn editor_ids(available: Option<IAvailableEditorTypes>) -> Option<Vec<String>> {
    available.map(|available| available.editors.iter().map(|editor| editor.id.clone()).collect())
}

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `args` de `executeCommand` iguais, na ordem, pela igualdade estrutural do `deepStrictEqual`.
fn same_args(actual: &[Dynamic], expected: &[Dynamic]) -> bool {
    actual.len() == expected.len() && actual.iter().zip(expected).all(|(actual, expected)| structural_equals(actual, expected))
}

/// `[DEFAULT_EDITOR_ASSOCIATION.id, { groupId: 42, editorIndex: 3 }]` com o id do editor.
fn reopen_args(editor_id: &str) -> Vec<Dynamic> {
    vec![Dynamic::string(editor_id), Dynamic::object([("groupId", Dynamic::Number(42.0)), ("editorIndex", Dynamic::Number(3.0))])]
}

fn noop_activate() -> Rc<dyn Fn(GroupIdentifier)> {
    Rc::new(|_| {})
}

//#endregion

#[test]
fn inline_custom_diff_editor_is_classified_as_a_diff_editor() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = uri_file("/original/test.md");
    let modified = uri_file("/modified/test.md");
    let registered_editors = vec![
        editor(DEFAULT_EDITOR_ASSOCIATION_ID, RegisteredEditorPriority::Builtin, None),
        editor("test.markdownEditor", RegisteredEditorPriority::Option, Some(RegisteredEditorPriority::Explicit)),
    ];
    let matches = editor_matches(&registered_editors, DEFAULT_EDITOR_ASSOCIATION_ID);
    // `new class extends EditorInput implements IEditorInputWithDiffResources`
    let input: EditorInputRef = guard.add_rc(PickerInput::new(PickerInputSpec {
        type_id: "test.inlineCustomDiffEditor",
        editor_id: Some("test.markdownEditor"),
        resource: Some(modified.clone()),
        diff_resources: Some((original.clone(), modified.clone())),
        ..PickerInputSpec::default()
    }));
    let requested_resources: Rc<RefCell<Vec<String>>> = Rc::default();
    let requested_options: Rc<RefCell<Vec<Option<EditorResolverServiceGetEditorMatchesOptions>>>> = Rc::default();
    let editor_resolver_service = Rc::new(PartialEditorResolverService {
        get_editor_matches: Some(Box::new({
            let (requested_resources, requested_options, matches) = (requested_resources.clone(), requested_options.clone(), matches.clone());
            move |resource, options| {
                requested_resources.borrow_mut().push(text(resource));
                requested_options.borrow_mut().push(options.copied());
                matches.clone()
            }
        })),
        ..PartialEditorResolverService::default()
    });

    let result = get_available_editor_types(Some(&input), &*editor_resolver_service, None).expect("o editor inline tem tipos disponíveis");

    assert_eq!(
        (
            requested_resources.borrow().clone(),
            requested_options.borrow().clone(),
            (
                text(&result.resource),
                result.is_diff_editor,
                result.original_resource.as_ref().map(text),
                result.modified_resource.as_ref().map(text),
                result.current_id.clone(),
                matches_view(&result.editor_matches),
                result.editors.clone(),
            ),
        ),
        (
            vec![text(&modified)],
            vec![Some(EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(true) })],
            (
                text(&modified),
                true,
                Some(text(&original)),
                Some(text(&modified)),
                "test.markdownEditor".to_owned(),
                matches_view(&matches),
                registered_editors.clone(),
            ),
        )
    );
}

#[test]
fn hidden_editor_types_are_omitted_unless_currently_active() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = uri_file("/workspace/test.md");
    let registered_editors = vec![
        editor(DEFAULT_EDITOR_ASSOCIATION_ID, RegisteredEditorPriority::Builtin, None),
        editor("test.markdownEditor", RegisteredEditorPriority::Option, None),
        editor("test.markdownPreview", RegisteredEditorPriority::Option, None),
    ];
    let matches = editor_matches(&registered_editors, DEFAULT_EDITOR_ASSOCIATION_ID);
    let editor_resolver_service = resolver_returning(&matches);
    let markdown_editor = input(&guard, "test.editor", "test.markdownEditor", &resource);
    let markdown_preview = input(&guard, "test.editor", "test.markdownPreview", &resource);
    let hidden = ["test.markdownPreview".to_owned()];
    let get_editor_ids = |input: &EditorInputRef| editor_ids(get_available_editor_types(Some(input), &*editor_resolver_service, Some(&hidden)));

    assert_eq!(
        (get_editor_ids(&markdown_editor), get_editor_ids(&markdown_preview)),
        (
            Some(vec![DEFAULT_EDITOR_ASSOCIATION_ID.to_owned(), "test.markdownEditor".to_owned()]),
            Some(vec![DEFAULT_EDITOR_ASSOCIATION_ID.to_owned(), "test.markdownEditor".to_owned(), "test.markdownPreview".to_owned()]),
        )
    );
}

#[test]
fn exclusive_matches_suppress_the_editor_type_picker() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = uri_file("/workspace/test.hex");
    let registered_editors = vec![
        editor(DEFAULT_EDITOR_ASSOCIATION_ID, RegisteredEditorPriority::Builtin, None),
        editor("test.hexEditor", RegisteredEditorPriority::Exclusive, None),
    ];
    let matches = editor_matches(&registered_editors, "test.hexEditor");
    let hex_editor = input(&guard, "test.hexEditorInput", "test.hexEditor", &resource);
    let editor_resolver_service = resolver_returning(&matches);

    assert!(get_available_editor_types(Some(&hex_editor), &*editor_resolver_service, None).is_none());
}

#[test]
fn unconfigured_universal_optional_matches_only_make_the_picker_visible_while_active() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = uri_file("/workspace/test.md");
    let registered_editors = vec![
        editor(DEFAULT_EDITOR_ASSOCIATION_ID, RegisteredEditorPriority::Builtin, None),
        editor("test.universalPreview", RegisteredEditorPriority::Option, None),
    ];
    let matches = EditorMatches::new(registered_editors.iter().map(|editor| registration_rule(editor, "*", "*.md")).collect(), 0, 0, false);
    let editor_resolver_service = resolver_returning(&matches);
    let text_editor = input(&guard, "test.editorInput", DEFAULT_EDITOR_ASSOCIATION_ID, &resource);
    let universal_preview = input(&guard, "test.editorInput", "test.universalPreview", &resource);

    assert_eq!(
        (
            editor_ids(get_available_editor_types(Some(&text_editor), &*editor_resolver_service, None)),
            editor_ids(get_available_editor_types(Some(&universal_preview), &*editor_resolver_service, None)),
        ),
        (None, Some(vec![DEFAULT_EDITOR_ASSOCIATION_ID.to_owned(), "test.universalPreview".to_owned()]))
    );
}

#[test]
fn set_default_uses_the_effective_default_scope_instead_of_the_active_editor_type() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let resource = uri_file("/workspace/example.component.html");
        let registered_editors = vec![
            editor(DEFAULT_EDITOR_ASSOCIATION_ID, RegisteredEditorPriority::Builtin, None),
            editor("test.componentEditor", RegisteredEditorPriority::Default, None),
        ];
        let updates: Rc<RefCell<Vec<(String, String, Option<bool>)>>> = Rc::default();
        let commands = Rc::new(RecordingCommandService::default());
        let matches = EditorMatches::new(
            registered_editors
                .iter()
                .map(|editor| {
                    let is_component = editor.id == "test.componentEditor";
                    registration_rule(editor, if is_component { "*.component.html" } else { "*" }, if is_component { "*.component.html" } else { "*.html" })
                })
                .collect(),
            1,
            1,
            false,
        );
        let editor_resolver_service = Rc::new(PartialEditorResolverService {
            set_default_editor: Some(Box::new({
                let updates = updates.clone();
                move |resource, editor_id, for_diff_editor| updates.borrow_mut().push((text(resource), editor_id.to_owned(), for_diff_editor))
            })),
            ..PartialEditorResolverService::default()
        });
        let group = create_group(&guard, Some(UntypedEditorInput::from_resource(resource.clone())), noop_activate(), None, EditorInputCapabilities::NONE);
        let actions = create_editor_type_actions(
            &IAvailableEditorTypes {
                resource: resource.clone(),
                is_diff_editor: false,
                original_resource: None,
                modified_resource: None,
                current_id: DEFAULT_EDITOR_ASSOCIATION_ID.to_owned(),
                editor_matches: matches,
                editors: registered_editors,
            },
            group,
            editor_resolver_service,
            commands.clone(),
            Rc::new(RecordingEditorService::default()),
        );
        let set_default_submenu = actions.iter().find(|action| action.as_any().is::<SubmenuAction>()).expect("há o submenu de padrão").clone();
        let submenu = set_default_submenu.as_any().downcast_ref::<SubmenuAction>().expect("é um SubmenuAction");

        submenu.actions()[0].run(&[]).await.unwrap();

        let recorded = commands.calls.borrow().clone();
        assert_eq!(
            (set_default_submenu.label(), submenu.actions().iter().map(|action| action.checked()).collect::<Vec<_>>(), updates.borrow().clone(), recorded.len(), recorded[0].0.clone()),
            (
                "Set Default for '*.component.html'".to_owned(),
                vec![Some(false), Some(true)],
                vec![(text(&resource), DEFAULT_EDITOR_ASSOCIATION_ID.to_owned(), Some(false))],
                1,
                "reopenActiveEditorWith".to_owned(),
            )
        );
        assert!(same_args(&recorded[0].1, &reopen_args(DEFAULT_EDITOR_ASSOCIATION_ID)));
    });
}

/// `for (const capabilities of [EditorInputCapabilities.None, EditorInputCapabilities.Singleton])`.
fn side_open_alternatives_respect_source_capabilities(capabilities: EditorInputCapabilities) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = uri_file("/workspace/test.md");
    let group = create_group(&guard, Some(UntypedEditorInput::from_resource(resource.clone())), noop_activate(), None, capabilities);
    let registered_editors = vec![editor("test.current", RegisteredEditorPriority::Builtin, None), editor("test.other", RegisteredEditorPriority::Option, None)];
    let matches = editor_matches(&registered_editors, "test.current");
    let actions = create_editor_type_actions(
        &IAvailableEditorTypes {
            resource,
            is_diff_editor: false,
            original_resource: None,
            modified_resource: None,
            current_id: "test.current".to_owned(),
            editor_matches: matches,
            editors: registered_editors,
        },
        group,
        Rc::new(PartialEditorResolverService::default()),
        Rc::new(RecordingCommandService::default()),
        Rc::new(RecordingEditorService::default()),
    );

    let view: Vec<(String, Option<String>)> = actions[..2].iter().map(|action| (action.id(), action.alt().map(|alt| alt.id()))).collect();

    assert_eq!(
        view,
        vec![
            ("test.current".to_owned(), if capabilities == EditorInputCapabilities::SINGLETON { None } else { Some("editorType.openToSide.test.current".to_owned()) }),
            ("test.other".to_owned(), Some("editorType.openToSide.test.other".to_owned())),
        ]
    );
}

#[test]
fn side_open_alternatives_respect_source_capabilities_0() {
    side_open_alternatives_respect_source_capabilities(EditorInputCapabilities::NONE);
}

#[test]
fn side_open_alternatives_respect_source_capabilities_8() {
    side_open_alternatives_respect_source_capabilities(EditorInputCapabilities::SINGLETON);
}

/// O que `assert.deepStrictEqual` compara de uma abertura: `{ input: { ...input, options }, target, sourceGroupId }`.
#[derive(Debug, PartialEq)]
struct OpenView {
    resource: Option<String>,
    original: Option<String>,
    modified: Option<String>,
    override_id: Option<String>,
    pinned: Option<bool>,
    preserve_focus: Option<bool>,
    selection: Option<TextEditorSelection>,
    selection_reveal_type: Option<TextEditorSelectionRevealType>,
    selection_source: Option<String>,
    target: Option<GroupIdentifier>,
    source_group_id: GroupIdentifier,
}

impl OpenView {
    fn of(opened: &OpenedEditor, source_group_id: GroupIdentifier) -> Self {
        let input = &opened.input;
        let options = input.options.as_ref();
        Self {
            resource: input.resource.as_ref().map(text),
            original: input.original.as_ref().and_then(|original| original.resource.as_ref()).map(text),
            modified: input.modified.as_ref().and_then(|modified| modified.resource.as_ref()).map(text),
            override_id: options.and_then(TextEditorOptions::override_id),
            pinned: options.and_then(|options| options.base.pinned),
            preserve_focus: options.and_then(|options| options.base.preserve_focus),
            selection: options.and_then(|options| options.selection),
            selection_reveal_type: options.and_then(|options| options.selection_reveal_type),
            selection_source: options.and_then(|options| options.selection_source.clone()),
            target: opened.group,
            source_group_id,
        }
    }
}

/// `{ startLineNumber, startColumn, endLineNumber, endColumn }` da seleção do painel.
fn text_selection(selection: Selection) -> TextEditorSelection {
    TextEditorSelection {
        start_line_number: f64::from(selection.start_line_number),
        start_column: f64::from(selection.start_column),
        end_line_number: Some(f64::from(selection.end_line_number)),
        end_column: Some(f64::from(selection.end_column)),
    }
}

/// `for (const isDiffEditor of [false, true])`.
fn opens_the_source_with_the_selected_type_to_its_side_without_reopening_it(is_diff_editor: bool) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let resource = uri_file("/workspace/test.md");
        let original = uri_file("/workspace/original.md");
        let options = TextEditorOptions::from(EditorOptions {
            override_: Some(EditorOverride::Id("test.oldEditor".into())),
            pinned: Some(false),
            preserve_focus: Some(true),
            ..EditorOptions::default()
        });
        let input = if is_diff_editor {
            UntypedEditorInput {
                original: Some(Box::new(UntypedEditorInput::from_resource(original.clone()))),
                modified: Some(Box::new(UntypedEditorInput::from_resource(resource.clone()))),
                options: Some(options.clone()),
                ..UntypedEditorInput::default()
            }
        } else {
            UntypedEditorInput { resource: Some(resource.clone()), options: Some(options.clone()), ..UntypedEditorInput::default() }
        };
        let active_group_id = Rc::new(Cell::new(99));
        let selection = Rc::new(Cell::new(Selection::new(10, 3, 12, 7)));
        let editor_service = Rc::new(RecordingEditorService::default());
        let source_group_ids: Rc<RefCell<Vec<GroupIdentifier>>> = Rc::default();
        // `opens.push({ input, target, sourceGroupId: activeGroupId }); activeGroupId = 100;`
        editor_service.on_open(Rc::new({
            let (active_group_id, source_group_ids) = (active_group_id.clone(), source_group_ids.clone());
            move || {
                source_group_ids.borrow_mut().push(active_group_id.get());
                active_group_id.set(100);
            }
        }));
        let commands = Rc::new(RecordingCommandService::default());
        let selection_of_pane: SelectionFn = Rc::new({
            let selection = selection.clone();
            move || Rc::new(TextEditorPaneSelection::new(selection.get())) as Rc<dyn IEditorPaneSelection>
        });
        let group = create_group(
            &guard,
            Some(input.clone()),
            Rc::new({
                let active_group_id = active_group_id.clone();
                move |group_id| active_group_id.set(group_id)
            }),
            Some(selection_of_pane),
            EditorInputCapabilities::NONE,
        );
        let registered_editors = vec![editor("test.oldEditor", RegisteredEditorPriority::Builtin, None), editor("test.newEditor", RegisteredEditorPriority::Option, None)];
        let matches = editor_matches(&registered_editors, "test.oldEditor");
        let actions = create_editor_type_actions(
            &IAvailableEditorTypes {
                resource: resource.clone(),
                is_diff_editor,
                original_resource: None,
                modified_resource: None,
                current_id: "test.oldEditor".to_owned(),
                editor_matches: matches,
                editors: registered_editors,
            },
            group,
            Rc::new(PartialEditorResolverService::default()),
            commands.clone(),
            editor_service.clone(),
        );

        let first_selection = selection.get();
        let alt = actions[1].alt().expect("o editor novo tem a alternativa de abrir ao lado");
        alt.run(&[]).await.unwrap();
        selection.set(Selection::new(20, 1, 23, 5));
        alt.run(&[]).await.unwrap();

        let opens: Vec<OpenView> = editor_service.opened().iter().zip(source_group_ids.borrow().iter()).map(|(opened, source_group_id)| OpenView::of(opened, *source_group_id)).collect();
        let expected_open = |selection: Selection| OpenView {
            resource: (!is_diff_editor).then(|| text(&resource)),
            original: is_diff_editor.then(|| text(&original)),
            modified: is_diff_editor.then(|| text(&resource)),
            override_id: Some("test.newEditor".to_owned()),
            pinned: Some(true),
            preserve_focus: Some(false),
            selection: Some(text_selection(selection)),
            selection_reveal_type: Some(TextEditorSelectionRevealType::CenterIfOutsideViewport),
            selection_source: None,
            target: Some(SIDE_GROUP),
            source_group_id: GROUP_ID,
        };
        let source_options = input.options.as_ref().expect("a entrada tem opções");
        assert_eq!(
            (
                commands.calls.borrow().len(),
                opens,
                (source_options.override_id(), source_options.base.pinned, source_options.base.preserve_focus),
                (actions[1].label(), alt.label()),
            ),
            (
                0,
                vec![expected_open(first_selection), expected_open(selection.get())],
                (Some("test.oldEditor".to_owned()), Some(false), Some(true)),
                ("test.newEditor".to_owned(), "test.newEditor (Open to the Side)".to_owned()),
            )
        );

        actions[1].run(&[]).await.unwrap();
        let recorded = commands.calls.borrow().clone();
        assert_eq!(recorded.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(), vec!["reopenActiveEditorWith".to_owned()]);
        assert!(same_args(&recorded[0].1, &reopen_args("test.newEditor")));
    });
}

#[test]
fn opens_the_source_editor_with_the_selected_type_to_its_side_without_reopening_it() {
    opens_the_source_with_the_selected_type_to_its_side_without_reopening_it(false);
}

#[test]
fn opens_the_source_diff_with_the_selected_type_to_its_side_without_reopening_it() {
    opens_the_source_with_the_selected_type_to_its_side_without_reopening_it(true);
}
