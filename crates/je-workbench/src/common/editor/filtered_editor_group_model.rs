//! Porte de `src/vs/workbench/common/editor/filteredEditorGroupModel.ts`: a visão somente leitura de um modelo de grupo de
//! editores que mostra só os editores aderentes (`StickyEditorGroupModel`) ou só os não aderentes
//! (`UnstickyEditorGroupModel`).
//!
//! Divergências:
//! - **Classe abstrata.** `FilteredEditorGroupModel` (com o `filter` abstrato e as sobrescritas das duas subclasses) é a
//!   struct genérica [`FilteredEditorGroupModel`] sobre uma estratégia [`EditorGroupModelFilter`]: o `filter` e os métodos
//!   que cada subclasse sobrescreve (`count`, `stickyCount`, `isSticky`, `getEditors`, `getEditorByIndex`, `indexOf` e
//!   `contains`) são funções associadas da estratégia, e o que a classe base implementa é o `impl` único de
//!   [`IReadonlyEditorGroupModel`]. As subclasses são as estratégias [`StickyFilter`] e [`UnstickyFilter`], e
//!   [`StickyEditorGroupModel`] e [`UnstickyEditorGroupModel`] são os apelidos de tipo que dão o `new(model)` do original.
//! - **`super.getEditors(order, options)`** é [`filtered_editors`], o que a base faz (o método padrão da estratégia).
//! - **Ciclo.** O listener que repassa os eventos do modelo guarda o modelo por `Weak`, porque o emissor do modelo o
//!   guardaria e o modelo guardaria a visão.

use std::marker::PhantomData;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::common::editor::editor_group_model::{EditorOrIndex, GetEditorsOptions, GroupModelChangeEvent, IReadonlyEditorGroupModel};
use crate::common::editor::{EditorInputOrUntyped, EditorInputRef, EditorsOrder, GroupIdentifier, MatchEditorOptions};

/// O que `StickyEditorGroupModel` e `UnstickyEditorGroupModel` acrescentam à `FilteredEditorGroupModel`: o `filter` abstrato,
/// os membros abstratos e as sobrescritas. Cada função recebe o modelo filtrado (`this.model`).
pub trait EditorGroupModelFilter: Sized + 'static {
    /// `filter(editorOrIndex)` (`protected abstract`).
    fn filter(model: &dyn IReadonlyEditorGroupModel, candidate_or_index: &EditorOrIndex) -> bool;

    /// `get count()` (`abstract`).
    fn count(model: &dyn IReadonlyEditorGroupModel) -> usize;

    /// `getEditorByIndex(index)` (`abstract`).
    fn get_editor_by_index(model: &dyn IReadonlyEditorGroupModel, index: i64) -> Option<EditorInputRef>;

    /// `indexOf(editor, editors?, options?)` (`abstract`).
    fn index_of(
        model: &dyn IReadonlyEditorGroupModel,
        editor: Option<&EditorInputOrUntyped>,
        editors: Option<&[EditorInputRef]>,
        options: Option<MatchEditorOptions>,
    ) -> i64;

    /// `contains(editor, options?)` (`abstract`).
    fn contains(model: &dyn IReadonlyEditorGroupModel, candidate: &EditorInputOrUntyped, options: Option<MatchEditorOptions>) -> bool;

    /// `get stickyCount()`.
    fn sticky_count(model: &dyn IReadonlyEditorGroupModel) -> usize {
        model.sticky_count()
    }

    /// `isSticky(editorOrIndex)`.
    fn is_sticky(model: &dyn IReadonlyEditorGroupModel, candidate_or_index: &EditorOrIndex) -> bool {
        model.is_sticky(candidate_or_index)
    }

    /// `getEditors(order, options?)`.
    fn get_editors(model: &dyn IReadonlyEditorGroupModel, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorInputRef> {
        filtered_editors::<Self>(model, order, options)
    }
}

/// `FilteredEditorGroupModel.getEditors(order, options)`: os editores do modelo que passam no filtro.
pub fn filtered_editors<K: EditorGroupModelFilter>(model: &dyn IReadonlyEditorGroupModel, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorInputRef> {
    model.get_editors(order, options).into_iter().filter(|editor| K::filter(model, &EditorOrIndex::Editor(editor.clone()))).collect()
}

/// `FilteredEditorGroupModel`.
pub struct FilteredEditorGroupModel<K: EditorGroupModelFilter> {
    base: Rc<Disposable>,
    model: Rc<dyn IReadonlyEditorGroupModel>,
    on_did_model_change: Rc<Emitter<GroupModelChangeEvent>>,
    _filter: PhantomData<fn() -> K>,
}

impl<K: EditorGroupModelFilter> FilteredEditorGroupModel<K> {
    /// `constructor(model)`.
    pub fn new(model: Rc<dyn IReadonlyEditorGroupModel>) -> Rc<Self> {
        let base = Disposable::new();

        let on_did_model_change: Rc<Emitter<GroupModelChangeEvent>> = Emitter::new();
        base.register(on_did_model_change.clone());

        let (weak_model, emitter) = (Rc::downgrade(&model), on_did_model_change.clone());
        base.register(model.on_did_model_change().listen(move |e| {
            let Some(model) = weak_model.upgrade() else { return };

            let candidate_or_index = match (e.editor_index, &e.editor) {
                (Some(index), _) => Some(EditorOrIndex::Index(index)),
                (None, Some(editor)) => Some(EditorOrIndex::Editor(editor.clone())),
                (None, None) => None,
            };
            if let Some(candidate_or_index) = candidate_or_index
                && !K::filter(&*model, &candidate_or_index)
            {
                return; // exclude events for excluded items
            }

            emitter.fire(e.clone());
        }));

        Rc::new(Self { base, model, on_did_model_change, _filter: PhantomData })
    }

    /// `this.filter(editor)`.
    fn passes(&self, editor: &EditorInputRef) -> bool {
        K::filter(&*self.model, &EditorOrIndex::Editor(editor.clone()))
    }
}

impl<K: EditorGroupModelFilter> IReadonlyEditorGroupModel for FilteredEditorGroupModel<K> {
    fn on_did_model_change(&self) -> Event<GroupModelChangeEvent> {
        self.on_did_model_change.event()
    }

    fn id(&self) -> GroupIdentifier {
        self.model.id()
    }

    fn count(&self) -> usize {
        K::count(&*self.model)
    }

    fn sticky_count(&self) -> usize {
        K::sticky_count(&*self.model)
    }

    fn is_locked(&self) -> bool {
        self.model.is_locked()
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        self.model.active_editor().filter(|editor| self.passes(editor))
    }

    fn preview_editor(&self) -> Option<EditorInputRef> {
        self.model.preview_editor().filter(|editor| self.passes(editor))
    }

    fn selected_editors(&self) -> Vec<EditorInputRef> {
        self.model.selected_editors().into_iter().filter(|editor| self.passes(editor)).collect()
    }

    fn get_editors(&self, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorInputRef> {
        K::get_editors(&*self.model, order, options)
    }

    fn get_editor_by_index(&self, index: i64) -> Option<EditorInputRef> {
        K::get_editor_by_index(&*self.model, index)
    }

    fn index_of(&self, editor: Option<&EditorInputOrUntyped>, editors: Option<&[EditorInputRef]>, options: Option<MatchEditorOptions>) -> i64 {
        K::index_of(&*self.model, editor, editors, options)
    }

    fn is_active(&self, editor: &EditorInputOrUntyped) -> bool {
        self.model.is_active(editor)
    }

    fn is_pinned(&self, editor_or_index: &EditorOrIndex) -> bool {
        self.model.is_pinned(editor_or_index)
    }

    fn is_sticky(&self, editor_or_index: &EditorOrIndex) -> bool {
        K::is_sticky(&*self.model, editor_or_index)
    }

    fn is_selected(&self, editor_or_index: &EditorOrIndex) -> bool {
        self.model.is_selected(editor_or_index)
    }

    fn is_transient(&self, editor_or_index: &EditorOrIndex) -> bool {
        self.model.is_transient(editor_or_index)
    }

    fn is_first(&self, editor: &EditorInputRef, _editors: Option<&[EditorInputRef]>) -> bool {
        self.model.is_first(editor, Some(self.get_editors(EditorsOrder::Sequential, None).as_slice()))
    }

    fn is_last(&self, editor: &EditorInputRef, _editors: Option<&[EditorInputRef]>) -> bool {
        self.model.is_last(editor, Some(self.get_editors(EditorsOrder::Sequential, None).as_slice()))
    }

    fn find_editor(&self, candidate: Option<&EditorInputRef>, options: Option<MatchEditorOptions>) -> Option<(EditorInputRef, i64)> {
        let result = self.model.find_editor(candidate, options)?;

        K::filter(&*self.model, &EditorOrIndex::Index(result.1)).then_some(result)
    }

    fn contains(&self, candidate: &EditorInputOrUntyped, options: Option<MatchEditorOptions>) -> bool {
        K::contains(&*self.model, candidate, options)
    }
}

impl<K: EditorGroupModelFilter> IDisposable for FilteredEditorGroupModel<K> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `StickyEditorGroupModel`: os editores aderentes do modelo.
pub struct StickyFilter;

impl EditorGroupModelFilter for StickyFilter {
    fn filter(model: &dyn IReadonlyEditorGroupModel, candidate_or_index: &EditorOrIndex) -> bool {
        model.is_sticky(candidate_or_index)
    }

    fn count(model: &dyn IReadonlyEditorGroupModel) -> usize {
        model.sticky_count()
    }

    fn get_editors(model: &dyn IReadonlyEditorGroupModel, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorInputRef> {
        if options.is_some_and(|options| options.exclude_sticky == Some(true)) {
            return Vec::new();
        }

        if order == EditorsOrder::Sequential {
            return model.get_editors(EditorsOrder::Sequential, None).into_iter().take(model.sticky_count()).collect();
        }

        filtered_editors::<Self>(model, order, options)
    }

    fn is_sticky(_model: &dyn IReadonlyEditorGroupModel, _candidate_or_index: &EditorOrIndex) -> bool {
        true
    }

    fn get_editor_by_index(model: &dyn IReadonlyEditorGroupModel, index: i64) -> Option<EditorInputRef> {
        if index < Self::count(model) as i64 { model.get_editor_by_index(index) } else { None }
    }

    fn index_of(
        model: &dyn IReadonlyEditorGroupModel,
        editor: Option<&EditorInputOrUntyped>,
        editors: Option<&[EditorInputRef]>,
        options: Option<MatchEditorOptions>,
    ) -> i64 {
        let editor_index = model.index_of(editor, editors, options);
        if editor_index < 0 || editor_index >= model.sticky_count() as i64 {
            return -1;
        }

        editor_index
    }

    fn contains(model: &dyn IReadonlyEditorGroupModel, candidate: &EditorInputOrUntyped, options: Option<MatchEditorOptions>) -> bool {
        let editor_index = model.index_of(Some(candidate), None, options);

        editor_index >= 0 && editor_index < model.sticky_count() as i64
    }
}

/// `UnstickyEditorGroupModel`: os editores não aderentes do modelo.
pub struct UnstickyFilter;

impl EditorGroupModelFilter for UnstickyFilter {
    fn filter(model: &dyn IReadonlyEditorGroupModel, candidate_or_index: &EditorOrIndex) -> bool {
        !model.is_sticky(candidate_or_index)
    }

    fn count(model: &dyn IReadonlyEditorGroupModel) -> usize {
        model.count().saturating_sub(model.sticky_count())
    }

    fn sticky_count(_model: &dyn IReadonlyEditorGroupModel) -> usize {
        0
    }

    fn is_sticky(_model: &dyn IReadonlyEditorGroupModel, _candidate_or_index: &EditorOrIndex) -> bool {
        false
    }

    fn get_editors(model: &dyn IReadonlyEditorGroupModel, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorInputRef> {
        if order == EditorsOrder::Sequential {
            return model.get_editors(EditorsOrder::Sequential, None).into_iter().skip(model.sticky_count()).collect();
        }

        filtered_editors::<Self>(model, order, options)
    }

    fn get_editor_by_index(model: &dyn IReadonlyEditorGroupModel, index: i64) -> Option<EditorInputRef> {
        if index >= 0 { model.get_editor_by_index(index + model.sticky_count() as i64) } else { None }
    }

    fn index_of(
        model: &dyn IReadonlyEditorGroupModel,
        editor: Option<&EditorInputOrUntyped>,
        editors: Option<&[EditorInputRef]>,
        options: Option<MatchEditorOptions>,
    ) -> i64 {
        let editor_index = model.index_of(editor, editors, options);
        if editor_index < model.sticky_count() as i64 || editor_index >= model.count() as i64 {
            return -1;
        }

        editor_index - model.sticky_count() as i64
    }

    fn contains(model: &dyn IReadonlyEditorGroupModel, candidate: &EditorInputOrUntyped, options: Option<MatchEditorOptions>) -> bool {
        let editor_index = model.index_of(Some(candidate), None, options);

        editor_index >= model.sticky_count() as i64 && editor_index < model.count() as i64
    }
}

/// `StickyEditorGroupModel`.
pub type StickyEditorGroupModel = FilteredEditorGroupModel<StickyFilter>;

/// `UnstickyEditorGroupModel`.
pub type UnstickyEditorGroupModel = FilteredEditorGroupModel<UnstickyFilter>;
