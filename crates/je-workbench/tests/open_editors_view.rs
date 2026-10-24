//! Porte de `src/vs/workbench/contrib/files/test/browser/openEditorsView.test.ts` (suíte `Files - OpenEditorsView`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é o `store` do original: o que o teste registra nele
//!   (as entradas e as listas) é descartado ao fim do caso, e só então vem a conferência.
//! - **`TestDirtyEditorInput`.** Subclasse de `TestEditorInput` que devolve `true` em `isDirty`: a struct de mesmo nome daqui
//!   implementa o contrato `EditorInput` com o mesmo corpo do `TestEditorInput` do porte e o `isDirty` trocado.
//! - **`delegate`, `renderer` e `createList`.** A `List` é a do `je-widgets` sobre um nó solto da árvore do `je-ui` (o
//!   `document.createElement('div')`). O `WeakMap<object, number>` do `identityProvider` é um mapa pelo endereço da entrada
//!   (o `Rc` vivo durante o caso garante que o endereço não é reaproveitado).
//! - **`assert.deepStrictEqual({ editor, group }, ...)`.** Compara as identidades (`===`) do editor e do grupo, pelo endereço.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_ui::{NodeId, Role, Ui};
use je_widgets::list::list_types::{IIdentityProvider, IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use je_widgets::list::list_widget::{List, ListOptions};
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::EditorInputRef;
use je_workbench::contrib::files::browser::views::open_editors_view::find_first_dirty_editor;
use je_workbench::contrib::files::common_files::OpenEditor;
use je_workbench::services::editor::editor_groups_service::IEditorGroup;
use je_workbench::test::browser::workbench_test_services::TestEditorInput;
use support::test_editor_group_view::TestEditorGroupView;

/// `TestDirtyEditorInput extends TestEditorInput`: `isDirty()` devolve `true`.
struct TestDirtyEditorInput {
    base: EditorInputBase,
    resource: URI,
    type_id: String,
}

impl TestDirtyEditorInput {
    fn new(resource: URI, type_id: &str) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), resource, type_id: type_id.to_string() })
    }
}

impl IDisposable for TestDirtyEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for TestDirtyEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        self.type_id.clone()
    }

    fn editor_id(&self) -> Option<String> {
        Some(self.type_id.clone())
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        Promise::resolved(None)
    }

    fn is_dirty(&self) -> bool {
        true
    }
}

/// `delegate`: `getHeight: () => 20`, `getTemplateId: () => 'openEditor'`.
struct Delegate;

impl IListVirtualDelegate<OpenEditor> for Delegate {
    fn get_height(&self, _element: &OpenEditor) -> f64 {
        20.0
    }

    fn get_template_id(&self, _element: &OpenEditor) -> String {
        "openEditor".to_owned()
    }
}

/// `renderer`: `templateId: 'openEditor'` e o resto sem efeito.
struct Renderer;

impl IListRenderer<OpenEditor> for Renderer {
    fn template_id(&self) -> String {
        "openEditor".to_owned()
    }

    fn render_template(&self, _container: NodeId) -> TemplateData {
        Rc::new(()) as Rc<dyn Any>
    }

    fn render_element(&self, _element: &OpenEditor, _index: usize, _template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {}

    fn dispose_template(&self, _template_data: &TemplateData) {}
}

/// `identityProvider`: `openeditor:${groupId}:${editorId}`, com `editorId` numerado por editor (o `WeakMap` do original).
#[derive(Default)]
struct EditorIdentityProvider {
    editor_ids: RefCell<HashMap<usize, i64>>,
    editor_id_pool: Cell<i64>,
}

impl IIdentityProvider<OpenEditor> for EditorIdentityProvider {
    fn get_id(&self, open_editor: &OpenEditor) -> String {
        let key = Rc::as_ptr(open_editor.editor()) as *const () as usize;
        let editor_id = *self.editor_ids.borrow_mut().entry(key).or_insert_with(|| {
            let editor_id = self.editor_id_pool.get();
            self.editor_id_pool.set(editor_id + 1);
            editor_id
        });

        format!("openeditor:{}:{}", open_editor.group_id(), editor_id)
    }
}

/// `createList`.
fn create_list(guard: &common::LeakGuard) -> Rc<List<OpenEditor>> {
    let ui = Ui::new();
    let container = ui.tree_mut().create_detached(Role::Generic);
    let options = ListOptions { identity_provider: Some(Rc::new(EditorIdentityProvider::default())), ..ListOptions::default() };

    guard.add_rc(List::new("OpenEditors", &ui, container, Rc::new(Delegate), vec![Rc::new(Renderer)], options))
}

/// A mesma instância (`===`), pelo endereço.
fn same<T: ?Sized, U: ?Sized>(left: &Rc<T>, right: &Rc<U>) -> bool {
    Rc::as_ptr(left) as *const () == Rc::as_ptr(right) as *const ()
}

#[test]
fn preserves_multi_selection_when_an_editor_is_refreshed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let group: Rc<dyn IEditorGroup> = TestEditorGroupView::new(1);
    let first_editor = guard.add_rc(TestEditorInput::new(URI::parse("test:/first", false), "testEditor"));
    let second_editor = guard.add_rc(TestEditorInput::new(URI::parse("test:/second", false), "testEditor"));
    let first_editor_ref: EditorInputRef = first_editor;
    let second_editor_ref: EditorInputRef = second_editor;
    let list = create_list(&guard);

    list.splice(0, 0, &[OpenEditor::new(first_editor_ref, group.clone()), OpenEditor::new(second_editor_ref.clone(), group.clone())]);
    list.set_selection(vec![0, 1], None);
    list.splice(1, 1, &[OpenEditor::new(second_editor_ref, group)]);

    assert_eq!(list.get_selection(), vec![0, 1]);
}

#[test]
fn preserves_multi_selection_when_editors_are_resorted() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let group: Rc<dyn IEditorGroup> = TestEditorGroupView::new(1);
    let first_editor = guard.add_rc(TestEditorInput::new(URI::parse("test:/first", false), "testEditor"));
    let second_editor = guard.add_rc(TestEditorInput::new(URI::parse("test:/second", false), "testEditor"));
    let third_editor = guard.add_rc(TestEditorInput::new(URI::parse("test:/third", false), "testEditor"));
    let first_editor_ref: EditorInputRef = first_editor;
    let second_editor_ref: EditorInputRef = second_editor;
    let third_editor_ref: EditorInputRef = third_editor;
    let list = create_list(&guard);

    list.splice(
        0,
        0,
        &[
            OpenEditor::new(first_editor_ref.clone(), group.clone()),
            OpenEditor::new(second_editor_ref.clone(), group.clone()),
            OpenEditor::new(third_editor_ref.clone(), group.clone()),
        ],
    );
    list.set_selection(vec![0, 1], None);
    list.splice(
        0,
        list.length(),
        &[OpenEditor::new(third_editor_ref, group.clone()), OpenEditor::new(first_editor_ref, group.clone()), OpenEditor::new(second_editor_ref, group)],
    );

    assert_eq!(list.get_selection(), vec![1, 2]);
}

#[test]
fn finds_the_first_unsaved_editor_in_index_order() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let first_group = TestEditorGroupView::new(1);
    let second_group = TestEditorGroupView::new(2);
    let saved_editor = guard.add_rc(TestEditorInput::new(URI::parse("test:/saved", false), "testEditor"));
    let first_dirty_editor = guard.add_rc(TestDirtyEditorInput::new(URI::parse("test:/firstDirty", false), "testEditor"));
    let second_dirty_editor = guard.add_rc(TestDirtyEditorInput::new(URI::parse("test:/secondDirty", false), "testEditor"));
    let saved_editor_ref: EditorInputRef = saved_editor;
    let first_dirty_editor_ref: EditorInputRef = first_dirty_editor;
    let second_dirty_editor_ref: EditorInputRef = second_dirty_editor;

    first_group.set_editors(vec![saved_editor_ref.clone()]);
    second_group.set_editors(vec![saved_editor_ref, first_dirty_editor_ref.clone(), second_dirty_editor_ref]);

    let groups: Vec<Rc<dyn IEditorGroup>> = vec![first_group, second_group.clone()];
    let first_dirty = find_first_dirty_editor(&groups).expect("há um editor com alterações não salvas");

    assert!(same(first_dirty.editor(), &first_dirty_editor_ref));
    assert!(same(first_dirty.group(), &second_group));
}

#[test]
fn finds_no_unsaved_editor_when_all_editors_are_saved() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let group = TestEditorGroupView::new(1);
    let saved_editor = guard.add_rc(TestEditorInput::new(URI::parse("test:/saved", false), "testEditor"));
    let saved_editor_ref: EditorInputRef = saved_editor;

    group.set_editors(vec![saved_editor_ref]);

    let groups: Vec<Rc<dyn IEditorGroup>> = vec![group];
    assert!(find_first_dirty_editor(&groups).is_none());
}
