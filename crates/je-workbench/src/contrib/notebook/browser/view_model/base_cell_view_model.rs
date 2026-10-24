//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/baseCellViewModel.ts`: o que os view models de célula de código
//! e de markdown têm em comum (`BaseCellViewModel`).
//!
//! Divergências:
//! - `abstract class` com subclasses é a composição: [`super::code_cell_view_model::CodeCellViewModel`] e
//!   [`super::markup_cell_view_model::MarkupCellViewModel`] guardam um `Rc<BaseCellViewModel>` e o expõem por `Deref`. Os três
//!   membros abstratos (`getHeight`, `onDeselect`, `layoutChange`) e `onDidChangeTextModelContent` ficam nas subclasses; os dois
//!   lugares em que o corpo da base chamava `this.layoutChange(...)` (o ouvinte de `onDidChangeInternalMetadata` e o setter de
//!   `commentHeight`) são registrados/expostos nas subclasses.
//! - O editor de código da célula (`ICodeEditor`, `attachTextEditor`, `detachTextEditor`, `ICodeEditorService`, o estado
//!   transitório de quebra de linha e `IInlineChatSessionService`) é Fase 6 (a UI nativa do editor), então a célula só existe no
//!   ramo "sem editor anexado" do original: `editorAttached` é falso, `assertTextModelAttached` é falso, `cursorAtBoundary` e
//!   `cursorAtLineBoundary` são `None`, `getLineScrollTopOffset` e `getPositionScrollTopOffset` são `0`, `revealRangeInCenter` e
//!   `setSelection` não fazem nada, `getSelections` e `getSelectionsStartPosition` leem só o estado de view guardado (que o
//!   porte guarda como [`Dynamic`] opaco, vazio), e `setSelections` só tem efeito quando há estado guardado. As decorações de
//!   modelo ficam todas `_lazy_` (o ramo sem editor de `addModelDecoration`).
//! - `resolveTextModel` (que abre uma referência pelo `ITextModelService`) e `onDidChangeTextModelContent` (o ouvinte que
//!   depende dele) são do editor de texto e não entram; `textModel` é o `textModel` que o `NotebookCellTextModel` já guarda.
//! - `lineNumbers: 'on' | 'off' | 'inherit'` é [`LineNumbers`]; `commentOptions` (`IEditorCommentsOptions`) é um [`Dynamic`].
//! - `toJSON` devolve o `handle`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::mime::mimes;
use je_base::uri::URI;
use je_editor::model::{FindMatch, ModelDeltaDecoration};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_editor::text_model_search::SearchParams;
use je_editor::undo_redo_service::IUndoRedoService;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;

use super::cell_view_model_types::{CellFocusMode, CursorAtBoundary, CursorAtLineBoundary};
use super::view_context::ViewContext;
use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::notebook_options::NotebookOptionsChangeEvent;
use crate::contrib::notebook::browser::notebook_view_events::CellViewModelStateChangeEvent;
use crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::notebook_common::{
    CellKind, ICell, INotebookCellStatusBarItem, INotebookFindOptions, NotebookCellInternalMetadata, NotebookCellMetadata, TextBufferRef,
};

/// `'on' | 'off' | 'inherit'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineNumbers {
    On,
    Off,
    Inherit,
}

impl LineNumbers {
    pub fn as_str(self) -> &'static str {
        match self {
            LineNumbers::On => "on",
            LineNumbers::Off => "off",
            LineNumbers::Inherit => "inherit",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "on" => Some(LineNumbers::On),
            "off" => Some(LineNumbers::Off),
            "inherit" => Some(LineNumbers::Inherit),
            _ => None,
        }
    }
}

/// `INotebookCellDecorationOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct INotebookCellDecorationOptions {
    pub class_name: Option<String>,
    pub gutter_class_name: Option<String>,
    pub output_class_name: Option<String>,
    pub top_class_name: Option<String>,
}

/// `{ added, removed }` de `onCellDecorationsChanged`.
#[derive(Clone, Debug)]
pub struct CellDecorationsChange {
    pub added: Vec<INotebookCellDecorationOptions>,
    pub removed: Vec<INotebookCellDecorationOptions>,
}

/// O valor do `_resolvedDecorations`: `{ id?, options }`.
struct ResolvedDecoration {
    #[allow(dead_code)]
    id: Option<String>,
    #[allow(dead_code)]
    options: ModelDeltaDecoration,
}

/// `BaseCellViewModel`.
pub struct BaseCellViewModel {
    base: Rc<Disposable>,
    pub view_type: String,
    pub model: Rc<NotebookCellTextModel>,
    pub id: String,
    view_context: Rc<ViewContext>,
    configuration_service: Rc<dyn IConfigurationService>,
    undo_redo_service: Rc<dyn IUndoRedoService>,

    on_did_change_editor_attach_state: Rc<Emitter<()>>,
    /// Do not merge this event with `onDidChangeState` as we are using `Event.once(onDidChangeEditorAttachState)` elsewhere.
    pub(crate) on_did_change_state: Rc<Emitter<CellViewModelStateChangeEvent>>,
    cell_decorations_changed: Rc<Emitter<CellDecorationsChange>>,
    on_did_change_cell_status_bar_items: Rc<Emitter<()>>,

    edit_state: Cell<CellEditState>,
    edit_state_source: RefCell<String>,
    line_numbers: Cell<LineNumbers>,
    comment_options: RefCell<Dynamic>,
    focus_mode: Cell<CellFocusMode>,
    editor_view_states: RefCell<Option<Dynamic>>,
    resolved_cell_decorations: RefCell<Vec<(String, INotebookCellDecorationOptions)>>,
    resolved_decorations: RefCell<HashMap<String, ResolvedDecoration>>,
    last_decoration_id: Cell<i64>,
    cell_status_bar_items: RefCell<Vec<(String, INotebookCellStatusBarItem)>>,
    last_status_bar_id: Cell<i64>,
    dragging: Cell<bool>,
    input_collapsed: Cell<bool>,
    output_collapsed: Cell<bool>,
    pub(crate) comment_height: Cell<f64>,
    is_disposed: Cell<bool>,
    is_readonly: Cell<bool>,
}

impl BaseCellViewModel {
    pub fn new(
        view_type: &str,
        model: Rc<NotebookCellTextModel>,
        id: String,
        view_context: Rc<ViewContext>,
        configuration_service: Rc<dyn IConfigurationService>,
        undo_redo_service: Rc<dyn IUndoRedoService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_editor_attach_state = Emitter::<()>::new();
        let on_did_change_state = Emitter::<CellViewModelStateChangeEvent>::new();
        let cell_decorations_changed = Emitter::<CellDecorationsChange>::new();
        let on_did_change_cell_status_bar_items = Emitter::<()>::new();
        base.register(on_did_change_editor_attach_state.clone());
        base.register(on_did_change_state.clone());
        base.register(cell_decorations_changed.clone());
        base.register(on_did_change_cell_status_bar_items.clone());

        let language = model.language();
        let comment_options = Self::read_comment_options(&configuration_service, &language);

        let this = Rc::new(Self {
            base,
            view_type: view_type.to_string(),
            model: model.clone(),
            id,
            view_context,
            configuration_service: configuration_service.clone(),
            undo_redo_service,
            on_did_change_editor_attach_state,
            on_did_change_state: on_did_change_state.clone(),
            cell_decorations_changed,
            on_did_change_cell_status_bar_items,
            edit_state: Cell::new(CellEditState::Preview),
            edit_state_source: RefCell::new(String::new()),
            line_numbers: Cell::new(LineNumbers::Inherit),
            comment_options: RefCell::new(comment_options),
            focus_mode: Cell::new(CellFocusMode::Container),
            editor_view_states: RefCell::new(None),
            resolved_cell_decorations: RefCell::new(Vec::new()),
            resolved_decorations: RefCell::new(HashMap::new()),
            last_decoration_id: Cell::new(0),
            cell_status_bar_items: RefCell::new(Vec::new()),
            last_status_bar_id: Cell::new(0),
            dragging: Cell::new(false),
            input_collapsed: Cell::new(false),
            output_collapsed: Cell::new(false),
            comment_height: Cell::new(0.0),
            is_disposed: Cell::new(false),
            is_readonly: Cell::new(false),
        });

        {
            let on_did_change_state = on_did_change_state.clone();
            this.base.register(model.on_did_change_metadata().listen(move |_| {
                on_did_change_state.fire(CellViewModelStateChangeEvent { metadata_changed: true, ..Default::default() });
            }));
        }

        {
            let weak = Rc::downgrade(&this);
            this.base.register(configuration_service.on_did_change_configuration().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                if e.affects_configuration("notebook.lineNumbers", None) {
                    this.set_line_numbers(LineNumbers::Inherit);
                }
                if e.affects_configuration("editor.comments", None) {
                    *this.comment_options.borrow_mut() = Self::read_comment_options(&this.configuration_service, &this.language());
                }
            }));
        }

        if let Some(collapse_state) = this.model.collapse_state {
            if collapse_state.input_collapsed == Some(true) {
                this.input_collapsed.set(true);
            }

            if collapse_state.output_collapsed == Some(true) {
                this.output_collapsed.set(true);
            }
        }

        this
    }

    fn read_comment_options(configuration_service: &Rc<dyn IConfigurationService>, language: &str) -> Dynamic {
        configuration_service.get_value(Some("editor.comments"), Some(&ConfigurationOverrides { override_identifier: Some(language.into()), resource: None }))
    }

    /// Registra um ouvinte que o `dispose` da célula desfaz (o `this._register` das subclasses).
    pub fn register(&self, disposable: je_base::lifecycle::DisposableRef) -> je_base::lifecycle::DisposableRef {
        self.base.register(disposable)
    }

    pub fn on_did_change_editor_attach_state(&self) -> Event<()> {
        self.on_did_change_editor_attach_state.event()
    }

    pub fn on_did_change_state(&self) -> Event<CellViewModelStateChangeEvent> {
        self.on_did_change_state.event()
    }

    pub fn fire_did_change_state(&self, event: CellViewModelStateChangeEvent) {
        self.on_did_change_state.fire(event);
    }

    pub fn on_cell_decorations_changed(&self) -> Event<CellDecorationsChange> {
        self.cell_decorations_changed.event()
    }

    pub fn on_did_change_cell_status_bar_items(&self) -> Event<()> {
        self.on_did_change_cell_status_bar_items.event()
    }

    pub fn view_context(&self) -> &Rc<ViewContext> {
        &self.view_context
    }

    pub fn handle(&self) -> i64 {
        self.model.handle()
    }

    pub fn uri(&self) -> URI {
        self.model.uri()
    }

    pub fn line_count(&self) -> usize {
        self.model.text_buffer().borrow().get_line_count()
    }

    pub fn metadata(&self) -> NotebookCellMetadata {
        self.model.metadata()
    }

    pub fn internal_metadata(&self) -> NotebookCellInternalMetadata {
        self.model.internal_metadata()
    }

    pub fn language(&self) -> String {
        self.model.language()
    }

    pub fn mime(&self) -> String {
        if let Some(mime) = self.model.mime() {
            return mime;
        }

        match self.language().as_str() {
            "markdown" => mimes::MARKDOWN.to_string(),
            _ => mimes::TEXT.to_string(),
        }
    }

    pub fn line_numbers(&self) -> LineNumbers {
        self.line_numbers.get()
    }

    pub fn set_line_numbers(&self, line_numbers: LineNumbers) {
        if line_numbers == self.line_numbers.get() {
            return;
        }

        self.line_numbers.set(line_numbers);
        self.on_did_change_state.fire(CellViewModelStateChangeEvent { cell_line_number_changed: true, ..Default::default() });
    }

    pub fn comment_options(&self) -> Dynamic {
        self.comment_options.borrow().clone()
    }

    pub fn set_comment_options(&self, new_options: Dynamic) {
        *self.comment_options.borrow_mut() = new_options;
    }

    pub fn focus_mode(&self) -> CellFocusMode {
        self.focus_mode.get()
    }

    pub fn set_focus_mode(&self, new_mode: CellFocusMode) {
        if self.focus_mode.get() != new_mode {
            self.focus_mode.set(new_mode);
            self.on_did_change_state.fire(CellViewModelStateChangeEvent { focus_mode_changed: true, ..Default::default() });
        }
    }

    /// Sem editor de código anexado (ver as divergências).
    pub fn editor_attached(&self) -> bool {
        false
    }

    pub fn text_model(&self) -> Option<Rc<TextModel>> {
        self.model.text_model()
    }

    pub fn has_model(&self) -> bool {
        self.text_model().is_some()
    }

    pub fn dragging(&self) -> bool {
        self.dragging.get()
    }

    pub fn set_dragging(&self, v: bool) {
        self.dragging.set(v);
        self.on_did_change_state.fire(CellViewModelStateChangeEvent { drag_state_changed: true, ..Default::default() });
    }

    pub fn is_input_collapsed(&self) -> bool {
        self.input_collapsed.get()
    }

    pub fn set_input_collapsed(&self, v: bool) {
        self.input_collapsed.set(v);
        self.on_did_change_state.fire(CellViewModelStateChangeEvent { input_collapsed_changed: true, ..Default::default() });
    }

    pub fn is_output_collapsed(&self) -> bool {
        self.output_collapsed.get()
    }

    pub fn set_output_collapsed(&self, v: bool) {
        self.output_collapsed.set(v);
        self.on_did_change_state.fire(CellViewModelStateChangeEvent { output_collapsed_changed: true, ..Default::default() });
    }

    pub fn update_options(&self, e: &NotebookOptionsChangeEvent) {
        if let Some(readonly) = e.readonly {
            self.is_readonly.set(readonly);
        }
    }

    pub fn get_text(&self) -> String {
        self.model.get_value().to_string_lossy()
    }

    pub fn get_alternative_id(&self) -> i64 {
        self.model.alternative_id()
    }

    pub fn get_text_length(&self) -> usize {
        self.model.get_text_length()
    }

    pub fn enable_auto_language_detection(&self) {
        self.model.enable_auto_language_detection();
    }

    pub fn save_editor_view_state(&self) -> Option<Dynamic> {
        self.editor_view_states.borrow().clone()
    }

    /// `restoreEditorViewState(editorViewStates, totalHeight?)` da base: só guarda o estado (o `totalHeight` é das subclasses).
    pub fn restore_editor_view_state(&self, editor_view_states: Option<Dynamic>) {
        *self.editor_view_states.borrow_mut() = editor_view_states;
    }

    pub fn add_model_decoration(&self, decoration: ModelDeltaDecoration) -> String {
        // `!this._textEditor`: sempre o ramo lazy.
        let id = self.last_decoration_id.get() + 1;
        self.last_decoration_id.set(id);
        let decoration_id = format!("_lazy_{};{}", self.id, id);
        self.resolved_decorations.borrow_mut().insert(decoration_id.clone(), ResolvedDecoration { id: None, options: decoration });
        decoration_id
    }

    pub fn remove_model_decoration(&self, decoration_id: &str) {
        // lastly, remove all the cache
        self.resolved_decorations.borrow_mut().remove(decoration_id);
    }

    pub fn delta_model_decorations(&self, old_decorations: &[String], new_decorations: Vec<ModelDeltaDecoration>) -> Vec<String> {
        for id in old_decorations {
            self.remove_model_decoration(id);
        }

        new_decorations.into_iter().map(|option| self.add_model_decoration(option)).collect()
    }

    fn remove_cell_decoration(&self, decoration_id: &str) {
        let options = {
            let mut resolved = self.resolved_cell_decorations.borrow_mut();
            resolved.iter().position(|(id, _)| id == decoration_id).map(|index| resolved.remove(index).1)
        };

        if let Some(mut options) = options {
            for (_, existing_options) in self.resolved_cell_decorations.borrow().iter() {
                // don't remove decorations that are applied from other entries
                if options.class_name == existing_options.class_name {
                    options.class_name = None;
                }
                if options.output_class_name == existing_options.output_class_name {
                    options.output_class_name = None;
                }
                if options.gutter_class_name == existing_options.gutter_class_name {
                    options.gutter_class_name = None;
                }
                if options.top_class_name == existing_options.top_class_name {
                    options.top_class_name = None;
                }
            }

            self.cell_decorations_changed.fire(CellDecorationsChange { added: Vec::new(), removed: vec![options] });
        }
    }

    fn add_cell_decoration(&self, options: INotebookCellDecorationOptions) -> String {
        let id = self.last_decoration_id.get() + 1;
        self.last_decoration_id.set(id);
        let decoration_id = format!("_cell_{};{}", self.id, id);
        self.resolved_cell_decorations.borrow_mut().push((decoration_id.clone(), options.clone()));
        self.cell_decorations_changed.fire(CellDecorationsChange { added: vec![options], removed: Vec::new() });
        decoration_id
    }

    pub fn get_cell_decorations(&self) -> Vec<INotebookCellDecorationOptions> {
        self.resolved_cell_decorations.borrow().iter().map(|(_, options)| options.clone()).collect()
    }

    /// Sem editor anexado o original devolve `null`.
    pub fn get_cell_decoration_range(&self, _decoration_id: &str) -> Option<Range> {
        None
    }

    pub fn delta_cell_decorations(&self, old_decorations: &[String], new_decorations: Vec<INotebookCellDecorationOptions>) -> Vec<String> {
        for id in old_decorations {
            self.remove_cell_decoration(id);
        }

        new_decorations.into_iter().map(|option| self.add_cell_decoration(option)).collect()
    }

    pub fn delta_cell_status_bar_items(&self, old_items: &[String], new_items: Vec<INotebookCellStatusBarItem>) -> Vec<String> {
        for id in old_items {
            self.cell_status_bar_items.borrow_mut().retain(|(item_id, _)| item_id != id);
        }

        let new_ids = new_items
            .into_iter()
            .map(|item| {
                let id = self.last_status_bar_id.get() + 1;
                self.last_status_bar_id.set(id);
                let item_id = format!("_cell_{};{}", self.id, id);
                self.cell_status_bar_items.borrow_mut().push((item_id.clone(), item));
                item_id
            })
            .collect();

        self.on_did_change_cell_status_bar_items.fire(());

        new_ids
    }

    pub fn get_cell_status_bar_items(&self) -> Vec<INotebookCellStatusBarItem> {
        self.cell_status_bar_items.borrow().iter().map(|(_, item)| item.clone()).collect()
    }

    /// Sem editor anexado, não faz nada.
    pub fn reveal_range_in_center(&self, _range: &Range) {}

    /// Sem editor anexado, não faz nada.
    pub fn set_selection(&self, _range: &Range) {}

    /// Sem editor anexado, o estado de view guardado do porte (opaco) não tem cursores para atualizar.
    pub fn set_selections(&self, _selections: &[Selection]) {}

    pub fn get_selections(&self) -> Vec<Selection> {
        Vec::new()
    }

    pub fn get_selections_start_position(&self) -> Option<Vec<Position>> {
        None
    }

    pub fn get_line_scroll_top_offset(&self, _line: i32) -> f64 {
        0.0
    }

    pub fn get_position_scroll_top_offset(&self, _range: &Range) -> f64 {
        0.0
    }

    pub fn cursor_at_line_boundary(&self) -> CursorAtLineBoundary {
        CursorAtLineBoundary::None
    }

    pub fn cursor_at_boundary(&self) -> CursorAtBoundary {
        CursorAtBoundary::None
    }

    pub fn edit_state_source(&self) -> String {
        self.edit_state_source.borrow().clone()
    }

    pub fn update_edit_state(&self, new_state: CellEditState, source: &str) {
        if new_state == self.edit_state.get() {
            return;
        }

        *self.edit_state_source.borrow_mut() = source.to_string();
        self.edit_state.set(new_state);
        self.on_did_change_state.fire(CellViewModelStateChangeEvent { edit_state_changed: true, ..Default::default() });
        if self.edit_state.get() == CellEditState::Preview {
            self.set_focus_mode(CellFocusMode::Container);
        }
    }

    pub fn get_edit_state(&self) -> CellEditState {
        self.edit_state.get()
    }

    pub fn text_buffer(&self) -> TextBufferRef {
        self.model.text_buffer()
    }

    pub fn cell_start_find(&self, value: &str, options: &INotebookFindOptions) -> Option<Vec<FindMatch>> {
        let line_count = self.text_buffer().borrow().get_line_count();
        let find_range: Vec<Range> = match options.find_scope.as_ref().and_then(|scope| scope.selected_text_ranges.clone()) {
            Some(ranges) => ranges,
            None => vec![Range::new(1, 1, line_count as i32, self.text_buffer().borrow().get_line_length(line_count as i32) as i32 + 1)],
        };

        // `assertTextModelAttached()` é sempre falso sem editor: a busca é linha a linha no buffer.
        let word_separators = if options.whole_word.unwrap_or(false) { options.word_separators.as_deref() } else { None };
        let search_params = SearchParams::new(value.into(), options.regex.unwrap_or(false), options.case_sensitive.unwrap_or(false), word_separators);
        let search_data = search_params.parse_search_request()?;

        let mut cell_matches = Vec::new();
        for range in &find_range {
            cell_matches.extend(self.text_buffer().borrow().find_matches_line_by_line(
                &Range::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column),
                &search_data,
                options.regex.unwrap_or(false),
                1000,
            ));
        }

        Some(cell_matches)
    }

    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    pub fn to_json(&self) -> Dynamic {
        let object = ObjectRef::new();
        object.set(&"handle".into(), Dynamic::Number(self.handle() as f64));
        Dynamic::Object(object)
    }

    pub fn cell_kind_of(&self) -> CellKind {
        self.model.cell_kind()
    }

    pub fn weak(self: &Rc<Self>) -> Weak<Self> {
        Rc::downgrade(self)
    }
}

impl IDisposable for BaseCellViewModel {
    fn dispose(&self) {
        self.is_disposed.set(true);
        self.base.dispose();

        // Only remove the undo redo stack if we map this cell uri to itself
        // If we are not in perCell mode, it will map to the full NotebookDocument and
        // we don't want to remove that entire document undo / redo stack when a cell is deleted
        if self.undo_redo_service.get_uri_comparison_key(&self.uri()) == self.uri().to_string(false).to_string_lossy() {
            self.undo_redo_service.remove_elements(&self.uri());
        }
    }

    fn constructor_name(&self) -> &'static str {
        "BaseCellViewModel"
    }
}
