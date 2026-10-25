//! Porte de `src/vs/workbench/browser/quickaccess.ts`: o `PickerEditorState`, que guarda o editor ativo antes de um
//! seletor abrir editores transitórios (a pré-visualização ao navegar pela lista) e o restaura, fechando os transitórios, ao
//! sair dele.
//!
//! Notas de tradução:
//! - **`@IEditorService` e `@IEditorGroupsService`.** São as dependências do [`Injectable`], na ordem do construtor.
//! - **`IPickerEditorState`.** `set`, `restore` e `reset` são os métodos do trait que o provedor de "ir para arquivo" lê
//!   ([`IPickerEditorState`]); o `openTransientEditor` do trait não leva grupo nem devolve o painel, então a forma completa
//!   (`openTransientEditor(editor, group)` com o painel) é o método próprio [`PickerEditorState::open_transient_editor`], que o
//!   do trait chama sem grupo. O estado guarda um `Weak` de si para os métodos assíncronos.
//! - **`activeEditorPane.group`.** O painel do porte só conhece o `group_id`, então o grupo é o
//!   `IEditorGroupsService.getGroup(group_id)`.
//! - **`getIEditor(activeEditorPane.getControl())?.saveViewState() ?? undefined`.** É o `get_view_state()` do painel, que já
//!   devolve `None` quando o painel não tem controle.
//! - **`Set<EditorInput>`.** É uma lista sem repetição por identidade (`Rc::ptr_eq`), na ordem de inserção.
//! - **`options: { ...editor.options, transient: true }`.** A cópia das opções do `UntypedEditorInput` com o `transient`
//!   ligado; sem opções, só o `transient`.
//! - **Fora do porte.** `inQuickPickContextKey`, `defaultQuickAccessContext`, `getQuickNavigateHandler` e
//!   `IWorkbenchQuickAccessConfiguration` não são usados por nenhum caso portado.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_instantiation::Injectable;
use je_platform::opener::editor_types::EditorOptions;

use crate::common::editor::editor_group_model::EditorOrIndex;
use crate::common::editor::{EditorInputRef, IEditorPane, UntypedEditorInput};
use crate::contrib::search::browser::anything_quick_access::IPickerEditorState;
use crate::services::editor::editor_groups_service::{CloseEditorOptions, IEditorGroup, IEditorGroupsService, PreferredGroup};
use crate::services::editor::editor_service::IEditorService;

/// `editorViewState`.
struct EditorViewState {
    editor: EditorInputRef,
    group: Rc<dyn IEditorGroup>,
    state: Option<Dynamic>,
}

/// `PickerEditorState`.
pub struct PickerEditorState {
    this: Weak<PickerEditorState>,
    base: Rc<Disposable>,
    editor_service: Rc<dyn IEditorService>,
    editor_groups_service: Rc<dyn IEditorGroupsService>,
    editor_view_state: RefCell<Option<EditorViewState>>,
    /// Editors that were opened between set and restore.
    opened_transient_editors: RefCell<Vec<EditorInputRef>>,
}

impl Injectable for PickerEditorState {
    type Args = ();
    type Deps = (Rc<dyn IEditorService>, Rc<dyn IEditorGroupsService>);

    fn create(_: (), (editor_service, editor_groups_service): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            editor_service,
            editor_groups_service,
            editor_view_state: RefCell::new(None),
            opened_transient_editors: RefCell::new(Vec::new()),
        })
    }
}

impl PickerEditorState {
    fn rc(&self) -> Rc<PickerEditorState> {
        self.this.upgrade().expect("o estado do seletor está vivo enquanto é usado")
    }

    /// Open a transient editor such that it may be closed when the state is restored.
    /// Note that, when the state is restored, if the editor is no longer transient, it will not be closed.
    pub fn open_transient_editor(&self, mut editor: UntypedEditorInput, group: Option<PreferredGroup>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let mut options = editor.options.take().unwrap_or_default();
        options.base.transient = Some(true);
        editor.options = Some(options);

        let this = self.rc();
        self.editor_service.open_editor_in(editor, group).then(move |editor_pane| {
            if let Some(pane) = &editor_pane {
                if let (Some(input), Some(group)) = (pane.input(), this.editor_groups_service.get_group(pane.group_id())) {
                    let is_view_state_editor = this.editor_view_state.borrow().as_ref().is_some_and(|state| Rc::ptr_eq(&state.editor, &input));
                    if !is_view_state_editor && group.is_transient(&EditorOrIndex::Editor(input.clone())) {
                        let mut opened_transient_editors = this.opened_transient_editors.borrow_mut();
                        if !opened_transient_editors.iter().any(|opened| Rc::ptr_eq(opened, &input)) {
                            opened_transient_editors.push(input);
                        }
                    }
                }
            }

            editor_pane
        })
    }
}

impl IPickerEditorState for PickerEditorState {
    fn set(&self) {
        if self.editor_view_state.borrow().is_some() {
            return; // return early if already done
        }

        let Some(active_editor_pane) = self.editor_service.active_editor_pane() else {
            return;
        };
        if let (Some(group), Some(editor)) = (self.editor_groups_service.get_group(active_editor_pane.group_id()), active_editor_pane.input()) {
            *self.editor_view_state.borrow_mut() = Some(EditorViewState { group, editor, state: active_editor_pane.get_view_state() });
        }
    }

    fn restore(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let view_state = this.editor_view_state.borrow().as_ref().map(|state| (state.group.clone(), state.editor.clone(), state.state.clone()));
            let Some((view_group, view_editor, state)) = view_state else {
                return Ok(());
            };

            let opened_transient_editors = this.opened_transient_editors.borrow().clone();
            for editor in opened_transient_editors {
                if editor.is_dirty() {
                    continue;
                }

                for group in this.editor_groups_service.groups() {
                    if group.is_transient(&EditorOrIndex::Editor(editor.clone())) {
                        group.close_editor_with_options(Some(&editor), Some(CloseEditorOptions { preserve_focus: Some(true), force: None })).await?;
                    }
                }
            }

            view_group
                .open_editor(
                    &view_editor,
                    Some(EditorOptions {
                        view_state: state,
                        preserve_focus: Some(true), // important to not close the picker as a result
                        ..EditorOptions::default()
                    }),
                )
                .await?;

            this.reset();
            Ok(())
        })
    }

    fn reset(&self) {
        *self.editor_view_state.borrow_mut() = None;
        self.opened_transient_editors.borrow_mut().clear();
    }

    fn open_transient_editor(&self, editor: UntypedEditorInput) -> Promise<()> {
        PickerEditorState::open_transient_editor(self, editor, None).then(|_| ())
    }
}

impl IDisposable for PickerEditorState {
    fn dispose(&self) {
        self.base.dispose();

        self.reset();
    }
}
