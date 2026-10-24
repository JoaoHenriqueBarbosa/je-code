//! A infraestrutura de teste que `editorGroupModel.test.ts` e `filteredEditorGroupModel.test.ts` repetem: o
//! `inst()`, o `createEditorGroupModel`, o `closeAllEditors`, o `groupListener`, as entradas `TestEditorInput`,
//! `NonSerializableTestEditorInput` e `TestFileEditorInput` e o `TestEditorInputSerializer`, mais os auxiliares que
//! os casos usam. Está num módulo só porque os dois arquivos do original têm o mesmo texto.
//!
//! Notas de tradução:
//! - **`inst()`.** O `TestInstantiationService` com `stub(IConfigurationService, ...)` é um `InstantiationService` sobre um
//!   `ServiceCollection` com o `TestConfigurationService` (`setUserConfiguration('workbench', { editor: ... })`).
//!   `IStorageService`, `ILifecycleService`, `IWorkspaceContextService` e `ITelemetryService`, que o original também
//!   registra, não são lidos pelo `EditorGroupModel` e não entram.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] do [`Suite`]; o `disposables` e o `teardown` do
//!   original são o próprio guarda, e o `setup` (registrar o serializador) é o [`Suite::new`].
//! - **`class TestEditorInput extends EditorInput`** (e as outras duas) são structs que guardam o `EditorInputBase`.
//!   `matches(other: TestEditorInput)` compara o `id` e a classe (`instanceof`), que é o `downcast_ref`.
//! - **`TestEditorInputSerializer`** é um [`Injectable`] sem dependências; `static disableSerialize` e
//!   `static disableDeserialize` são variáveis da thread. O `disposables.add(new TestEditorInput(...))` do `deserialize`
//!   usa o store do guarda, guardado numa variável da thread.
//! - **`index`** (o contador de `input()`) é o campo `index` do [`Suite`], que começa em 1 (o original zera para 1 no
//!   `teardown`).
//! - **`IGroupEditorOpenEvent` e as outras formas** são o [`GroupModelChangeEvent`] (ver o porte do `EditorGroupModel`).
//! - **Opções.** `{ pinned: true, active: true }` é `open!(group, input, pinned: true, active: true)`.
//! - **`createInstance(DiffEditorInput, ...)` e `createInstance(SideBySideEditorInput, ...)`** são os `new` das entradas,
//!   com o `IEditorService` de teste (`workbenchInstantiationService` só serve para isso).
#![allow(dead_code)]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable, id_of, to_disposable};
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::resources::ext_uri;
use je_workbench::common::editor::diff_editor_input::DiffEditorInput;
use je_workbench::common::editor::editor_factory_registry::{IEditorSerializer, editor_factory_registry};
use je_workbench::common::editor::editor_group_model::{
    EditorGroupModel, EditorOrIndex, GetEditorsOptions, GroupModelChangeEvent, IReadonlyEditorGroupModel, SerializedEditorGroupModel,
    is_group_editor_change_event, is_group_editor_close_event, is_group_editor_move_event, is_group_editor_open_event,
};
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase, matches_by_default};
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{
    CloseDirection, EditorInputOrUntyped, EditorInputRef, EditorsOrder, GroupModelChangeKind, MatchEditorOptions, SideBySideEditor, UntypedEditorInput,
};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::test::browser::workbench_test_services::TestEditorService;

/// `{ pinned: true, active: true }`: as opções de `openEditor`, com os campos que o caso dá.
macro_rules! open {
    ($group:expr, $editor:expr $(, $field:ident : $value:expr)* $(,)?) => {
        $group.open_editor(
            &$editor,
            Some(&je_workbench::common::editor::editor_group_model::EditorOpenOptions {
                $($field: Some($value),)*
                ..je_workbench::common::editor::editor_group_model::EditorOpenOptions::default()
            }),
        )
    };
}

thread_local! {
    /// `TestEditorInputSerializer.disableSerialize`.
    static DISABLE_SERIALIZE: Cell<bool> = const { Cell::new(false) };
    /// `TestEditorInputSerializer.disableDeserialize`.
    static DISABLE_DESERIALIZE: Cell<bool> = const { Cell::new(false) };
    /// O `disposables` do original, que o `deserialize` do serializador usa.
    static SUITE_DISPOSABLES: RefCell<Option<Rc<DisposableStore>>> = const { RefCell::new(None) };
}

/// `TestEditorInputSerializer.disableSerialize = value`.
pub fn set_disable_serialize(value: bool) {
    DISABLE_SERIALIZE.with(|flag| flag.set(value));
}

/// `TestEditorInputSerializer.disableDeserialize = value`.
pub fn set_disable_deserialize(value: bool) {
    DISABLE_DESERIALIZE.with(|flag| flag.set(value));
}

/// `class TestEditorInput extends EditorInput`.
pub struct TestEditorInput {
    base: EditorInputBase,
    pub id: String,
}

impl TestEditorInput {
    pub fn new(id: &str) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), id: id.to_string() })
    }

    pub fn set_dirty(&self) {
        self.base.fire_did_change_dirty();
    }

    pub fn set_label(&self) {
        self.base.fire_did_change_label();
    }
}

impl IDisposable for TestEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for TestEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        "testEditorInputForGroups".to_string()
    }

    fn resource(&self) -> Option<URI> {
        None
    }

    fn matches(&self, other: &EditorInputOrUntyped) -> bool {
        match other {
            EditorInputOrUntyped::Typed(other) => other.as_any().downcast_ref::<TestEditorInput>().is_some_and(|other| other.id == self.id),
            EditorInputOrUntyped::Untyped(_) => false,
        }
    }
}

/// `class NonSerializableTestEditorInput extends EditorInput`.
pub struct NonSerializableTestEditorInput {
    base: EditorInputBase,
    pub id: String,
}

impl NonSerializableTestEditorInput {
    pub fn new(id: &str) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), id: id.to_string() })
    }
}

impl IDisposable for NonSerializableTestEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for NonSerializableTestEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        "testEditorInputForGroups-nonSerializable".to_string()
    }

    fn resource(&self) -> Option<URI> {
        None
    }

    fn matches(&self, other: &EditorInputOrUntyped) -> bool {
        match other {
            EditorInputOrUntyped::Typed(other) => other.as_any().downcast_ref::<NonSerializableTestEditorInput>().is_some_and(|other| other.id == self.id),
            EditorInputOrUntyped::Untyped(_) => false,
        }
    }
}

/// `class TestFileEditorInput extends EditorInput implements IFileEditorInput`.
pub struct TestFileEditorInput {
    base: EditorInputBase,
    pub id: String,
    pub resource: URI,
    preferred_resource: URI,
}

impl TestFileEditorInput {
    pub fn new(id: &str, resource: URI) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), id: id.to_string(), preferred_resource: resource.clone(), resource })
    }
}

impl IDisposable for TestFileEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for TestFileEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        "testFileEditorInputForGroups".to_string()
    }

    fn editor_id(&self) -> Option<String> {
        Some(self.id.clone())
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn preferred_resource(&self) -> Option<URI> {
        Some(self.preferred_resource.clone())
    }

    fn matches(&self, other: &EditorInputOrUntyped) -> bool {
        if matches_by_default(self, other) {
            return true;
        }

        match other {
            EditorInputOrUntyped::Typed(other) => other
                .as_any()
                .downcast_ref::<TestFileEditorInput>()
                .is_some_and(|other| ext_uri().is_equal(Some(&other.resource), Some(&self.resource), false)),
            EditorInputOrUntyped::Untyped(_) => false,
        }
    }
}

/// `class TestEditorInputSerializer implements IEditorSerializer`.
pub struct TestEditorInputSerializer;

impl Injectable for TestEditorInputSerializer {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

impl IEditorSerializer for TestEditorInputSerializer {
    fn can_serialize(&self, _editor: &EditorInputRef) -> bool {
        true
    }

    fn serialize(&self, editor: &EditorInputRef) -> Option<String> {
        if DISABLE_SERIALIZE.with(Cell::get) {
            return None;
        }

        let test_editor_input = test_input(editor);

        Some(serde_json::json!({ "id": test_editor_input.id }).to_string())
    }

    fn deserialize(&self, _instantiation_service: &InstantiationService, serialized_editor_input: &str) -> Option<EditorInputRef> {
        if DISABLE_DESERIALIZE.with(Cell::get) {
            return None;
        }

        let test_input: serde_json::Value = serde_json::from_str(serialized_editor_input).expect("o serializado é um JSON");
        let id = test_input["id"].as_str().expect("o serializado tem um id");

        let editor = SUITE_DISPOSABLES.with(|disposables| {
            disposables.borrow().as_ref().expect("o guarda do caso está ativo").add_rc(TestEditorInput::new(id))
        });
        let editor: EditorInputRef = editor;

        Some(editor)
    }
}

/// `(<TestEditorInput>editor)`.
pub fn test_input(editor: &EditorInputRef) -> &TestEditorInput {
    editor.as_any().downcast_ref::<TestEditorInput>().expect("a entrada é um TestEditorInput")
}

/// Os valores de `workbench.editor.*` que o `inst()` do original configura.
pub fn default_editor_settings() -> Vec<(&'static str, Dynamic)> {
    vec![("openPositioning", Dynamic::string("right")), ("focusRecentEditorAfterClose", Dynamic::Bool(true))]
}

/// `new TestInstantiationService()` com `stub(IConfigurationService, config)` e `config.setUserConfiguration('workbench', {
/// editor: editorSettings })`.
pub fn instantiation_service(disposables: &LeakGuard, editor_settings: Vec<(&str, Dynamic)>) -> Rc<InstantiationService> {
    let configuration = TestConfigurationService::new(None);
    let _ = configuration.set_user_configuration("workbench", Dynamic::object([("editor", Dynamic::object(editor_settings))]), None);
    let configuration: Rc<dyn IConfigurationService> = configuration;

    disposables.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration)))
}

macro_rules! group_events {
    ($($name:ident : $ty:ty),* $(,)?) => {
        /// `GroupEvents`: o que `groupListener` coletou, cada lista lida por um método.
        #[derive(Default)]
        pub struct GroupEvents {
            $($name: RefCell<Vec<$ty>>,)*
        }

        impl GroupEvents {
            $(pub fn $name(&self) -> Vec<$ty> {
                self.$name.borrow().clone()
            })*
        }
    };
}

group_events! {
    locked: i64,
    active: i64,
    index: i64,
    label: i64,
    opened: GroupModelChangeEvent,
    activated: GroupModelChangeEvent,
    closed: GroupModelChangeEvent,
    pinned: GroupModelChangeEvent,
    unpinned: GroupModelChangeEvent,
    sticky: GroupModelChangeEvent,
    unsticky: GroupModelChangeEvent,
    transient: GroupModelChangeEvent,
    moved: GroupModelChangeEvent,
    disposed: GroupModelChangeEvent,
}

/// O ambiente de um caso: o guarda de vazamento, o `inst()` e os criadores de entrada.
pub struct Suite {
    pub disposables: LeakGuard,
    pub instantiation_service: Rc<InstantiationService>,
    editor_service: Rc<dyn IEditorService>,
    index: Cell<u32>,
}

impl Suite {
    /// `ensureNoDisposablesAreLeakedInTestSuite()` e o `setup` da suíte.
    pub fn new() -> Self {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        SUITE_DISPOSABLES.with(|store| *store.borrow_mut() = Some(disposables.store()));

        set_disable_serialize(false);
        set_disable_deserialize(false);

        disposables.add(editor_factory_registry().register_editor_serializer::<TestEditorInputSerializer>("testEditorInputForGroups"));

        let instantiation_service = instantiation_service(&disposables, default_editor_settings());
        let editor_service: Rc<dyn IEditorService> = Rc::new(TestEditorService);

        Self { disposables, instantiation_service, editor_service, index: Cell::new(1) }
    }

    /// `createEditorGroupModel(serialized?)`.
    pub fn create_editor_group_model(&self, serialized: Option<SerializedEditorGroupModel>) -> Rc<EditorGroupModel> {
        let group = self.disposables.add_rc(self.instantiation_service.create_instance::<EditorGroupModel>(serialized));

        let editors_owner = group.clone();
        self.disposables.add(to_disposable(move || {
            for editor in editors_owner.get_editors(EditorsOrder::MostRecentlyActive, None) {
                editors_owner.close_editor(&editor, None, None);
            }
        }));

        group
    }

    /// `createEditorGroupModel()`.
    pub fn group(&self) -> Rc<EditorGroupModel> {
        self.create_editor_group_model(None)
    }

    /// O `String(index++)` do parâmetro `id` de `input()`.
    pub fn next_id(&self) -> String {
        let id = self.index.get();
        self.index.set(id + 1);

        id.to_string()
    }

    /// `input()`.
    pub fn input(&self) -> EditorInputRef {
        self.named(&self.next_id())
    }

    /// `input(id)`.
    pub fn named(&self, id: &str) -> EditorInputRef {
        let editor = self.disposables.add_rc(TestEditorInput::new(id));

        editor
    }

    /// `input(id, true)`.
    pub fn non_serializable(&self, id: &str) -> EditorInputRef {
        let editor = self.disposables.add_rc(NonSerializableTestEditorInput::new(id));

        editor
    }

    /// `input(id, nonSerializable, resource)`: com o recurso é sempre um `TestFileEditorInput`.
    pub fn file(&self, id: &str, resource: URI) -> EditorInputRef {
        let editor = self.disposables.add_rc(TestFileEditorInput::new(id, resource));

        editor
    }

    /// `instantiationService.createInstance(SideBySideEditorInput, name, undefined, secondary, primary)`.
    pub fn side_by_side(&self, name: Option<&str>, secondary: &EditorInputRef, primary: &EditorInputRef) -> EditorInputRef {
        let editor = self.disposables.add_rc(SideBySideEditorInput::new(name, None, secondary.clone(), primary.clone(), self.editor_service.clone()));

        editor
    }

    /// `instantiationService.createInstance(DiffEditorInput, name, description, original, modified, undefined)`.
    pub fn diff(&self, original: &EditorInputRef, modified: &EditorInputRef) -> EditorInputRef {
        let editor =
            self.disposables.add_rc(DiffEditorInput::new(Some("name"), Some("description"), original.clone(), modified.clone(), None, self.editor_service.clone()));

        editor
    }

    /// `groupListener(group)`.
    pub fn group_listener(&self, group: &Rc<EditorGroupModel>) -> Rc<GroupEvents> {
        let events = Rc::new(GroupEvents::default());

        let (weak_group, listener_events) = (Rc::downgrade(group), events.clone());
        self.disposables.add(group.on_did_model_change().listen(move |e| {
            let Some(group) = weak_group.upgrade() else { return };

            match e.kind {
                GroupModelChangeKind::GroupLocked => {
                    listener_events.locked.borrow_mut().push(group.id());
                    return;
                }
                GroupModelChangeKind::GroupActive => {
                    listener_events.active.borrow_mut().push(group.id());
                    return;
                }
                GroupModelChangeKind::GroupIndex => {
                    listener_events.index.borrow_mut().push(group.id());
                    return;
                }
                GroupModelChangeKind::GroupLabel => {
                    listener_events.label.borrow_mut().push(group.id());
                    return;
                }
                _ => {}
            }

            let Some(editor) = e.editor.clone() else { return };

            match e.kind {
                GroupModelChangeKind::EditorOpen if is_group_editor_open_event(e) => listener_events.opened.borrow_mut().push(e.clone()),
                GroupModelChangeKind::EditorClose if is_group_editor_close_event(e) => listener_events.closed.borrow_mut().push(e.clone()),
                GroupModelChangeKind::EditorActive if is_group_editor_change_event(e) => listener_events.activated.borrow_mut().push(e.clone()),
                GroupModelChangeKind::EditorPin if is_group_editor_change_event(e) => {
                    if group.is_pinned(&EditorOrIndex::Editor(editor)) {
                        listener_events.pinned.borrow_mut().push(e.clone());
                    } else {
                        listener_events.unpinned.borrow_mut().push(e.clone());
                    }
                }
                GroupModelChangeKind::EditorSticky if is_group_editor_change_event(e) => {
                    if group.is_sticky(&EditorOrIndex::Editor(editor)) {
                        listener_events.sticky.borrow_mut().push(e.clone());
                    } else {
                        listener_events.unsticky.borrow_mut().push(e.clone());
                    }
                }
                GroupModelChangeKind::EditorTransient if is_group_editor_change_event(e) => listener_events.transient.borrow_mut().push(e.clone()),
                GroupModelChangeKind::EditorMove if is_group_editor_move_event(e) => listener_events.moved.borrow_mut().push(e.clone()),
                GroupModelChangeKind::EditorWillDispose if is_group_editor_change_event(e) => listener_events.disposed.borrow_mut().push(e.clone()),
                _ => {}
            }
        }));

        events
    }

    /// Um contador dos eventos de um tipo (`let dirty1Counter = 0; disposables.add(group.onDidModelChange(e => { if (e.kind
    /// === ...) dirty1Counter++; }))`).
    pub fn count_events(&self, model: &dyn IReadonlyEditorGroupModel, kind: GroupModelChangeKind) -> Rc<Cell<u32>> {
        let counter = Rc::new(Cell::new(0));

        let listener_counter = counter.clone();
        self.disposables.add(model.on_did_model_change().listen(move |e| {
            if e.kind == kind {
                listener_counter.set(listener_counter.get() + 1);
            }
        }));

        counter
    }
}

/// `closeAllEditors(group)`.
pub fn close_all_editors(group: &EditorGroupModel) {
    for editor in group.get_editors(EditorsOrder::Sequential, None) {
        group.close_editor(&editor, None, Some(false));
    }
}

/// `closeEditors(group, except, direction?)`.
pub fn close_editors(group: &EditorGroupModel, except: &EditorInputRef, direction: Option<CloseDirection>) {
    let index = group.index_of(Some(&typed(except)), None, None);
    if index == -1 {
        return; // not found
    }

    match direction {
        // Close to the left
        Some(CloseDirection::Left) => {
            for i in (0..index).rev() {
                let editor = group.get_editor_by_index(i).expect("há um editor à esquerda");
                group.close_editor(&editor, None, None);
            }
        }

        // Close to the right
        Some(CloseDirection::Right) => {
            for i in ((index + 1)..group.get_editors(EditorsOrder::Sequential, None).len() as i64).rev() {
                let editor = group.get_editor_by_index(i).expect("há um editor à direita");
                group.close_editor(&editor, None, None);
            }
        }

        // Both directions
        None => {
            for editor in group.get_editors(EditorsOrder::MostRecentlyActive, None) {
                if !editor.matches(&typed(except)) {
                    group.close_editor(&editor, None, None);
                }
            }
        }
    }
}

/// `group.closeEditor(editor)`.
pub fn close(group: &EditorGroupModel, editor: &EditorInputRef) -> Option<je_workbench::common::editor::editor_group_model::EditorCloseResult> {
    group.close_editor(editor, None, None)
}

/// `group.getEditors(EditorsOrder.SEQUENTIAL)`.
pub fn seq(group: &EditorGroupModel) -> Vec<EditorInputRef> {
    group.get_editors(EditorsOrder::Sequential, None)
}

/// `group.getEditors(EditorsOrder.MOST_RECENTLY_ACTIVE)`.
pub fn mru(group: &EditorGroupModel) -> Vec<EditorInputRef> {
    group.get_editors(EditorsOrder::MostRecentlyActive, None)
}

/// `group.indexOf(editor)`.
pub fn index_of(group: &EditorGroupModel, editor: &EditorInputRef) -> i64 {
    group.index_of(Some(&typed(editor)), None, None)
}

/// `EditorInput` como `EditorInput | IUntypedEditorInput`.
pub fn typed(editor: &EditorInputRef) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(editor.clone())
}

/// Uma entrada sem tipo como `EditorInput | IUntypedEditorInput`.
pub fn untyped(editor: &UntypedEditorInput) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Untyped(editor.clone())
}

/// O editor como `EditorInput | number`.
pub fn ed(editor: &EditorInputRef) -> EditorOrIndex {
    EditorOrIndex::Editor(editor.clone())
}

/// O índice como `EditorInput | number`.
pub fn at(index: i64) -> EditorOrIndex {
    EditorOrIndex::Index(index)
}

/// `a === b`.
pub fn same(a: &EditorInputRef, b: &EditorInputRef) -> bool {
    id_of(a) == id_of(b)
}

/// `assert.strictEqual(actual, expected)` entre entradas.
pub fn assert_same(actual: &EditorInputRef, expected: &EditorInputRef) {
    assert!(same(actual, expected), "a entrada não é a esperada");
}

/// `assert.strictEqual(actual, expected)` com `actual` possivelmente ausente.
pub fn assert_is(actual: Option<EditorInputRef>, expected: &EditorInputRef) {
    assert!(actual.as_ref().is_some_and(|actual| same(actual, expected)), "a entrada não é a esperada");
}

/// `assert.strictEqual(actual, null)` (ou `undefined`).
pub fn assert_none(actual: Option<EditorInputRef>) {
    assert!(actual.is_none(), "esperava nenhuma entrada");
}

/// `e.editor` de um evento de editor.
pub fn editor_of(event: &GroupModelChangeEvent) -> EditorInputRef {
    event.editor.clone().expect("o evento tem um editor")
}

/// `{ supportSideBySide: SideBySideEditor.ANY }`.
pub fn any() -> Option<MatchEditorOptions> {
    Some(MatchEditorOptions { support_side_by_side: Some(SideBySideEditor::Any), strict_equals: None })
}

/// `{ supportSideBySide: SideBySideEditor.BOTH }`.
pub fn both() -> Option<MatchEditorOptions> {
    Some(MatchEditorOptions { support_side_by_side: Some(SideBySideEditor::Both), strict_equals: None })
}

/// `{ strictEquals: true }`.
pub fn strict() -> Option<MatchEditorOptions> {
    Some(MatchEditorOptions { support_side_by_side: None, strict_equals: Some(true) })
}

/// `{ supportSideBySide: SideBySideEditor.ANY, strictEquals: true }`.
pub fn any_strict() -> Option<MatchEditorOptions> {
    Some(MatchEditorOptions { support_side_by_side: Some(SideBySideEditor::Any), strict_equals: Some(true) })
}

/// `{ supportSideBySide: SideBySideEditor.BOTH, strictEquals: true }`.
pub fn both_strict() -> Option<MatchEditorOptions> {
    Some(MatchEditorOptions { support_side_by_side: Some(SideBySideEditor::Both), strict_equals: Some(true) })
}

/// `{ excludeSticky }`.
pub fn exclude_sticky(exclude_sticky: bool) -> Option<GetEditorsOptions> {
    Some(GetEditorsOptions { exclude_sticky: Some(exclude_sticky) })
}
