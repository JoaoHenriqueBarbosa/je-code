//! Porte de `src/vs/workbench/test/browser/parts/editor/editorCommandsContext.test.ts` (suíte `Resolving Editor Commands Context`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` registra o painel de arquivo, o lado a lado, o de recurso e o `MyTestEditorForEditors`
//!   ([`setup`]); a instância de serviços e o `TestServiceAccessor` do `setup` do original só existem para o
//!   `disposables.add(accessor.untitledTextEditorService)`, e o serviço de sem título do [`EditorServiceAccessor`] já é dono do
//!   dele. O `teardown` é o `disposables.clear()` precedido do `workbenchTeardown` do acessor, que fecha os editores e remove
//!   os grupos antes de o guarda de vazamentos conferir ([`run_case`]). Os casos rodam em `run_with_faked_timers`, e `await` é
//!   `.await.unwrap()`.
//! - **`createServices`.** É o [`EditorServiceAccessor`] (o `workbenchInstantiationService`, o `createEditorPart`, o
//!   `EditorService` real e o `IEditorGroupsService`, que no original é a própria parte): ver `support/editor_service_support.rs`.
//!   Os métodos de grupos que o adaptador do serviço não repassa (`activateGroup`) vão pela parte.
//! - **`TestListService`.** `lastFocusedList = undefined` é o `last_focused_list` sem lista.
//! - **`input(id = String(index++))`.** O `index` é o contador do módulo do original (um por thread de teste, que cada caso roda
//!   numa), e a `URI.parse('file://<id>')` é a mesma.
//! - **`activeGroup.openEditor(...)` e `setSelection(...)` sem `await`.** Os de `await` do original são `.await.unwrap()`; os sem
//!   `await` descartam a promessa (o corpo síncrona do grupo já rodou, como no JavaScript).
//! - **`assert.strictEqual(a, b)` entre entradas e `editorIndex`.** A identidade do objeto é [`id_of`], e o índice do editor no
//!   grupo (`getIndexOfEditor`) é o `usize` do `IEditorCommandsContext`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::TypeId;
use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::lifecycle::{DisposableStore, id_of};
use je_base::uri::URI;
use je_platform::list::list_service::{IListService, IListWidget};
use je_platform::opener::editor_types::EditorOptions;
use je_workbench::browser::parts::editor::editor_commands_context::{CommandArg, ResolvedEditorCommandsContext, resolve_commands_context};
use je_workbench::common::editor::{EditorCommandsContext, EditorInputRef};
use je_workbench::services::editor::editor_groups_service::{GroupDirection, GroupRef, IEditorGroup, IEditorPart};
use je_workbench::test::browser::workbench_test_services::{TestEditorInput, TestFileEditorInput, register_test_editor};
use support::editor_service_support::{EditorServiceAccessor, register_test_file_editor, register_test_resource_editor, register_test_side_by_side_editor};

const TEST_EDITOR_ID: &str = "MyTestEditorForEditors";

/// `class TestListService implements IListService { readonly lastFocusedList = undefined }`.
struct TestListService;

impl IListService for TestListService {
    fn last_focused_list(&self) -> Option<Rc<dyn IListWidget>> {
        None
    }
}

//#region Infraestrutura da suíte

/// O `setup` da suíte.
fn setup(disposables: &Rc<DisposableStore>) {
    disposables.add(register_test_file_editor());
    disposables.add(register_test_side_by_side_editor());
    disposables.add(register_test_resource_editor());
    disposables.add(register_test_editor(TEST_EDITOR_ID, vec![TypeId::of::<TestFileEditorInput>()], None));
}

/// O caso: o guarda de vazamentos, o `setup`, o `createServices()` e o corpo no relógio virtual, seguidos do `teardown`.
fn run_case<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>, Rc<EditorServiceAccessor>) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        setup(&disposables);

        let accessor = Rc::new(EditorServiceAccessor::with_default_backup_service(&disposables));
        accessor.part.when_ready().await.unwrap();

        body(disposables.clone(), accessor.clone()).await;

        accessor.teardown().await.unwrap();
        disposables.clear();
    });
}

thread_local! {
    /// `let index = 0`.
    static INDEX: Cell<usize> = const { Cell::new(0) };
}

/// `input(id = String(index++))`: `disposables.add(new TestEditorInput(URI.parse(`file://${id}`), 'testInput'))`.
fn input(disposables: &Rc<DisposableStore>) -> EditorInputRef {
    let id = INDEX.with(|index| index.replace(index.get() + 1));

    disposables.add_rc(TestEditorInput::new(URI::parse(&format!("file://{id}"), false), "testInput"))
}

/// `{ pinned: true }`.
fn pinned() -> Option<EditorOptions> {
    Some(EditorOptions { pinned: Some(true), ..EditorOptions::default() })
}

/// `group.getIndexOfEditor(editor)` como o `editorIndex` do contexto.
fn index_of(group: &Rc<dyn IEditorGroup>, editor: &EditorInputRef) -> usize {
    usize::try_from(group.get_index_of_editor(editor)).expect("o editor está no grupo")
}

/// `assert.strictEqual(a, b)` entre entradas.
fn same(a: &EditorInputRef, b: &EditorInputRef) -> bool {
    id_of(a) == id_of(b)
}

/// `resolveCommandsContext(args, accessor.editorService, accessor.editorGroupService, testListService)`.
fn resolve(accessor: &EditorServiceAccessor, args: &[CommandArg]) -> ResolvedEditorCommandsContext {
    resolve_commands_context(args, &*accessor.editor_service, &*accessor.editor_groups_service, &TestListService)
}

//#endregion

#[test]
fn use_editor_group_selection() {
    run_case(|disposables, accessor| async move {
        let active_group = accessor.editor_groups_service.active_group();

        let input1 = input(&disposables);
        let input2 = input(&disposables);
        let input3 = input(&disposables);
        let _opening1 = active_group.open_editor(&input1, pinned());
        let _opening2 = active_group.open_editor(&input2, pinned());
        let _opening3 = active_group.open_editor(&input3, pinned());

        let _selecting = active_group.set_selection(&input1, vec![input2.clone()]);

        // use editor commands context
        let editor_command_context =
            EditorCommandsContext { group_id: active_group.id(), editor_index: Some(index_of(&active_group, &input1)), preserve_focus: Some(true) };
        let resolved_context1 = resolve(&accessor, &[CommandArg::Context(editor_command_context)]);

        assert_eq!(resolved_context1.grouped_editors.len(), 1);
        assert_eq!(resolved_context1.grouped_editors[0].group.id(), active_group.id());
        assert_eq!(resolved_context1.grouped_editors[0].editors.len(), 2);
        assert!(same(&resolved_context1.grouped_editors[0].editors[0], &input1));
        assert!(same(&resolved_context1.grouped_editors[0].editors[1], &input2));
        assert!(resolved_context1.preserve_focus);

        // use URI
        let resolved_context2 = resolve(&accessor, &[CommandArg::Uri(input2.resource().expect("a entrada tem recurso"))]);

        assert_eq!(resolved_context2.grouped_editors.len(), 1);
        assert_eq!(resolved_context2.grouped_editors[0].group.id(), active_group.id());
        assert_eq!(resolved_context2.grouped_editors[0].editors.len(), 2);
        assert!(same(&resolved_context2.grouped_editors[0].editors[0], &input2));
        assert!(same(&resolved_context2.grouped_editors[0].editors[1], &input1));
        assert!(!resolved_context2.preserve_focus);

        // use URI and commandContext
        let editor1_command_context =
            EditorCommandsContext { group_id: active_group.id(), editor_index: Some(index_of(&active_group, &input1)), preserve_focus: Some(true) };
        let resolved_context3 = resolve(&accessor, &[CommandArg::Context(editor1_command_context)]);

        assert_eq!(resolved_context3.grouped_editors.len(), 1);
        assert_eq!(resolved_context3.grouped_editors[0].group.id(), active_group.id());
        assert_eq!(resolved_context3.grouped_editors[0].editors.len(), 2);
        assert!(same(&resolved_context3.grouped_editors[0].editors[0], &input1));
        assert!(same(&resolved_context3.grouped_editors[0].editors[1], &input2));
        assert!(resolved_context3.preserve_focus);
    });
}

#[test]
fn don_t_use_editor_group_selection() {
    run_case(|disposables, accessor| async move {
        let active_group = accessor.editor_groups_service.active_group();

        let input1 = input(&disposables);
        let input2 = input(&disposables);
        let input3 = input(&disposables);
        let _opening1 = active_group.open_editor(&input1, pinned());
        let _opening2 = active_group.open_editor(&input2, pinned());
        let _opening3 = active_group.open_editor(&input3, pinned());

        let _selecting = active_group.set_selection(&input1, vec![input2.clone()]);

        // use editor commands context
        let editor_command_context =
            EditorCommandsContext { group_id: active_group.id(), editor_index: Some(index_of(&active_group, &input3)), preserve_focus: Some(true) };
        let resolved_context1 = resolve(&accessor, &[CommandArg::Context(editor_command_context)]);

        assert_eq!(resolved_context1.grouped_editors.len(), 1);
        assert_eq!(resolved_context1.grouped_editors[0].group.id(), active_group.id());
        assert_eq!(resolved_context1.grouped_editors[0].editors.len(), 1);
        assert!(same(&resolved_context1.grouped_editors[0].editors[0], &input3));
        assert!(resolved_context1.preserve_focus);

        // use URI
        let resolved_context2 = resolve(&accessor, &[CommandArg::Uri(input3.resource().expect("a entrada tem recurso"))]);

        assert_eq!(resolved_context2.grouped_editors.len(), 1);
        assert_eq!(resolved_context2.grouped_editors[0].group.id(), active_group.id());
        assert_eq!(resolved_context2.grouped_editors[0].editors.len(), 1);
        assert!(same(&resolved_context2.grouped_editors[0].editors[0], &input3));
        assert!(!resolved_context2.preserve_focus);
    });
}

#[test]
fn inactive_edior_group_command_context() {
    run_case(|disposables, accessor| async move {
        let editor_group_service = accessor.editor_groups_service.clone();

        let group1 = editor_group_service.active_group();
        let group2 = editor_group_service.add_group(Some(GroupRef::Group(group1.clone())), GroupDirection::Right);

        let input11 = input(&disposables);
        let input12 = input(&disposables);
        let _opening11 = group1.open_editor(&input11, pinned());
        let _opening12 = group1.open_editor(&input12, pinned());

        let input21 = input(&disposables);
        let _opening21 = group2.open_editor(&input21, pinned());

        IEditorPart::activate_group(&*accessor.part, GroupRef::Group(group1.clone()));
        let _selecting = group1.set_selection(&input11, vec![input12.clone()]);

        // use editor commands context of inactive group with editor index
        let editor_command_context1 =
            EditorCommandsContext { group_id: group2.id(), editor_index: Some(index_of(&group2, &input21)), preserve_focus: Some(true) };
        let resolved_context1 = resolve(&accessor, &[CommandArg::Context(editor_command_context1)]);

        assert_eq!(resolved_context1.grouped_editors.len(), 1);
        assert_eq!(resolved_context1.grouped_editors[0].group.id(), group2.id());
        assert_eq!(resolved_context1.grouped_editors[0].editors.len(), 1);
        assert!(same(&resolved_context1.grouped_editors[0].editors[0], &input21));
        assert!(resolved_context1.preserve_focus);

        // use editor commands context of inactive group without editor index
        let editor_command_context2 = EditorCommandsContext { group_id: group2.id(), editor_index: None, preserve_focus: Some(true) };
        let resolved_context2 = resolve(&accessor, &[CommandArg::Context(editor_command_context2)]);

        assert_eq!(resolved_context2.grouped_editors.len(), 1);
        assert_eq!(resolved_context2.grouped_editors[0].group.id(), group2.id());
        assert_eq!(resolved_context2.grouped_editors[0].editors.len(), 1);
        assert!(same(&resolved_context1.grouped_editors[0].editors[0], &input21));
        assert!(resolved_context2.preserve_focus);
    });
}

#[test]
fn resolves_context_from_right_clicked_editor_not_active() {
    run_case(|disposables, accessor| async move {
        let group = accessor.editor_groups_service.active_group();

        let input1 = input(&disposables);
        let input2 = input(&disposables);
        group.open_editor(&input1, pinned()).await.unwrap();
        group.open_editor(&input2, pinned()).await.unwrap();

        // input2 is now active (last opened), but we simulate right-click on input1
        assert!(group.active_editor().is_some_and(|active_editor| same(&active_editor, &input2)));

        let editor_command_context = EditorCommandsContext { group_id: group.id(), editor_index: Some(index_of(&group, &input1)), preserve_focus: Some(false) };
        let resolved_context = resolve(&accessor, &[CommandArg::Context(editor_command_context)]);

        // Should resolve to input1 (right-clicked editor), not input2 (active editor)
        assert_eq!(resolved_context.grouped_editors.len(), 1);
        assert_eq!(resolved_context.grouped_editors[0].group.id(), group.id());
        assert_eq!(resolved_context.grouped_editors[0].editors.len(), 1);
        assert!(same(&resolved_context.grouped_editors[0].editors[0], &input1));
        assert!(!same(&resolved_context.grouped_editors[0].editors[0], &input2));
        assert!(!resolved_context.preserve_focus);
    });
}
