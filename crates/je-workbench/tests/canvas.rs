//! Porte de `src/vs/workbench/contrib/canvases/test/common/canvas.test.ts` (suíte `CanvasInput`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `upcastPartial<IEditorGroupsService>({ mainPart, getGroup })` vira os tipos locais abaixo: só o que o teste lê
//!   responde, e o resto lança como o objeto parcial (que não tem o membro) faria ao ser usado.
//! - `upcastPartial<IWorkbenchEnvironmentService>({ isSessionsWindow })` é o `TestEnvironmentService` com o valor trocado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors::{Error, throw};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, ModalEditorPartOptions};
use je_workbench::common::editor::editor_input::{CanMove, EditorInput};
use je_workbench::common::editor::{EditorInputOrUntyped, EditorInputRef, GroupIdentifier};
use je_workbench::contrib::canvases::canvas::{CanvasInput, ICanvas, ICanvasOwner, ICanvasReference};
use je_workbench::services::editor::editor_groups_service::{
    FindGroupScope, GroupDirection, GroupRef, GroupsOrder, IEditorGroup, IEditorGroupsService, IEditorPart, IModalEditorPart,
};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::layout::layout_service::WindowId;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};

fn uri(value: &str) -> URI {
    URI::parse(value, false)
}

/// `upcastPartial<IEditorGroup>({ windowId })`.
struct PartialGroup {
    window_id: WindowId,
}

impl IEditorGroup for PartialGroup {
    fn id(&self) -> GroupIdentifier {
        throw(Error::new("not implemented"))
    }

    fn window_id(&self) -> WindowId {
        self.window_id
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        throw(Error::new("not implemented"))
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        throw(Error::new("not implemented"))
    }

    fn is_locked(&self) -> bool {
        throw(Error::new("not implemented"))
    }

    fn is_active(&self, _editor: &EditorInputOrUntyped) -> bool {
        throw(Error::new("not implemented"))
    }

    fn close_editor(&self, _editor: Option<&EditorInputRef>) -> Promise<bool> {
        throw(Error::new("not implemented"))
    }

    fn move_editor(&self, _editor: &EditorInputRef, _target: &Rc<dyn IEditorGroup>) -> bool {
        throw(Error::new("not implemented"))
    }
}

/// `upcastPartial<IEditorPart>({ windowId: 1 })`.
struct PartialPart {
    window_id: WindowId,
}

impl IEditorPart for PartialPart {
    fn window_id(&self) -> WindowId {
        self.window_id
    }

    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        throw(Error::new("not implemented"))
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        throw(Error::new("not implemented"))
    }
}

/// O `editorGroupsService` da suíte: `mainPart` é a janela 1 e `getGroup(id)` devolve um grupo da janela 1 (para `id === 1`)
/// ou da janela 2.
struct PartialEditorGroupsService;

impl IEditorGroupsService for PartialEditorGroupsService {
    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        throw(Error::new("not implemented"))
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        throw(Error::new("not implemented"))
    }

    fn get_groups(&self, _order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        throw(Error::new("not implemented"))
    }

    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        Some(Rc::new(PartialGroup { window_id: WindowId(if identifier == 1 { 1 } else { 2 }) }))
    }

    fn find_group(&self, _scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        throw(Error::new("not implemented"))
    }

    fn add_group(&self, _location: Option<GroupRef>, _direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        throw(Error::new("not implemented"))
    }

    fn main_part(&self) -> Rc<dyn IEditorPart> {
        Rc::new(PartialPart { window_id: WindowId(1) })
    }

    fn create_auxiliary_editor_part(&self, _options: Option<AuxiliaryEditorOptions>) -> Promise<Rc<dyn IEditorPart>> {
        throw(Error::new("not implemented"))
    }

    fn create_modal_editor_part(&self, _options: Option<ModalEditorPartOptions>) -> Promise<Rc<dyn IModalEditorPart>> {
        throw(Error::new("not implemented"))
    }

    fn active_modal_editor_part(&self) -> Option<Rc<dyn IModalEditorPart>> {
        throw(Error::new("not implemented"))
    }
}

fn editor_groups_service() -> Rc<dyn IEditorGroupsService> {
    Rc::new(PartialEditorGroupsService)
}

/// `environmentService(isSessionsWindow = false)`.
fn environment_service(is_sessions_window: bool) -> Rc<dyn IWorkbenchEnvironmentService> {
    TestEnvironmentService::with_overrides(EnvironmentOverrides { is_sessions_window, ..EnvironmentOverrides::default() })
}

fn reference(provider_id: &str, session: &str, chat: &str, canvas: &str) -> ICanvasReference {
    ICanvasReference {
        owner: ICanvasOwner { provider_id: provider_id.to_string(), session: uri(session), chat: uri(chat) },
        canvas: uri(canvas),
    }
}

#[test]
fn keeps_transient_sources_out_of_editor_identity_and_does_not_restore() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let source = "https://secret.example/canvas?token=sensitive";
    let canvas = |title: &str| ICanvas {
        resource: uri("agent-host-canvas:/preview"),
        instance_id: Some("preview".to_string()),
        title: title.to_string(),
        status: None,
        source: Some(uri(source)),
    };
    let input = store.add_rc(CanvasInput::new(
        reference("local-agent-host", "agent-host-session:/session", "agent-host-chat:/session/main", "agent-host-canvas:/preview"),
        Some(canvas("Preview")),
        editor_groups_service(),
        environment_service(false),
    ));
    let labels: Rc<std::cell::RefCell<Vec<String>>> = Rc::default();
    let labels_listener = labels.clone();
    let input_listener = input.clone();
    store.add(input.on_did_change_label().listen(move |_| labels_listener.borrow_mut().push(input_listener.get_name())));

    input.set_canvas(Some(canvas("Updated Preview")), false);

    let resource = input.resource().expect("o canvas tem recurso");
    assert_eq!(
        (input.get_name(), labels.borrow().clone(), input.can_reopen(), resource.to_string(false).to_string_lossy().contains("secret.example")),
        ("Updated Preview".to_string(), vec!["Updated Preview".to_string()], false, false)
    );
}

#[test]
fn allows_main_window_movement_but_rejects_auxiliary_windows() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let input = store.add_rc(CanvasInput::new(
        reference("local", "session:/owner", "chat:/owner", "canvas:/preview"),
        Some(ICanvas {
            resource: uri("canvas:/preview"),
            instance_id: Some("preview".to_string()),
            title: "Preview".to_string(),
            status: None,
            source: Some(uri("https://example.test")),
        }),
        editor_groups_service(),
        environment_service(false),
    ));

    // `{ main: input.canMove(1, 1), auxiliary: typeof input.canMove(1, 2) }` contra `{ main: true, auxiliary: 'string' }`.
    assert_eq!(input.can_move(1, 1), CanMove::Yes);
    assert!(matches!(input.can_move(1, 2), CanMove::No(_)));
}

#[test]
fn describes_only_the_reopen_affordances_of_its_window() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let descriptions: Vec<Option<String>> = [false, true]
        .iter()
        .map(|&is_sessions_window| {
            store
                .add_rc(CanvasInput::new(
                    reference("local", "session:/owner", "chat:/owner", "canvas:/preview"),
                    None,
                    editor_groups_service(),
                    environment_service(is_sessions_window),
                ))
                .get_description(None)
        })
        .collect();

    assert_eq!(
        descriptions,
        vec![
            Some("Closing this tab hides the canvas. Reopen it from its canvas pill or ask the agent to open it again.".to_string()),
            Some(
                "Closing this tab hides the canvas. Reopen it from the Canvas submenu in Add Tab, use its canvas pill, or ask the agent to open it again."
                    .to_string()
            ),
        ]
    );
}
