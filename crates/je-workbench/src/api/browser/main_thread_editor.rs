//! Porte de `src/vs/workbench/api/browser/mainThreadEditor.ts`: o `MainThreadTextEditorProperties` (seleções, opções e
//! intervalos visíveis lidos do editor) e o [`MainThreadTextEditor`], o editor de texto da API ligado para sempre ao
//! mesmo modelo e, opcionalmente, a um editor de código.
//!
//! Divergências:
//! - **O editor de código** (`ICodeEditor`) é o [`CodeEditorWidget`] concreto (`Rc<CodeEditorWidget>`): o porte não tem outra
//!   implementação do `ICodeEditor`. `IActiveCodeEditor` do workbench (o recorte de `ICodeEditor` que o
//!   `IEditorService` devolve) é implementado para o widget aqui, o que permite a [`MainThreadTextEditor::matches`]
//!   comparar o controle do painel pela identidade do objeto, como o `===` do original.
//! - **`IModelService`** é o [`ModelService`] concreto (só `getCreationOptions` é usado, do trait `IModelService`).
//! - **`setDecorationsByType`, `setDecorationsByTypeFast` e `removeDecorationsByType`** são métodos do `CodeEditorWidget`
//!   no original (com `_decorationTypeKeysToIds` e `_decorationTypeSubtypes`); o widget do porte não os tem e seus DTOs
//!   (`IDecorationOptions`) vivem no protocolo do workbench. A mesma lógica está em [`DecorationsByType`], guardada no
//!   [`MainThreadTextEditor`], que fala com o `ICodeEditorService` do widget
//!   ([`CodeEditorWidget::code_editor_service`]). O `hash(renderOptions)` que dá o sufixo do subtipo é o `stringHash`
//!   do texto de depuração das opções (o sufixo só precisa ser repetível e único por conteúdo).
//!   [`MainThreadTextEditor::dispose`] chama `removeDecorationsByType` das chaves usadas, como o descarte do widget faz com
//!   `_removeDecorationTypes`.
//! - `revealRange` com um `revealType` desconhecido (o `console.warn` do `default`) não existe: o enum é fechado.
//! - O `IEditorPane` de `matches` é o `dyn IEditorPane` do workbench; `editor.getControl() === this._codeEditor` é a
//!   comparação do controle de texto de código do painel com o editor.
//! - `insertSnippet` é [`async_fn`]; `this._clipboardService.readText()` é o `read_text` do serviço de área de
//!   transferência.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::hash::string_hash;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_editor::code_editor_widget::{CodeEditorWidget, EndCursorState, ExecuteEditsSource, ICodeEditor};
use je_editor::config::editor_options::cursor_style::{cursor_style_to_string, TextEditorCursorStyle};
use je_editor::config::editor_options::line_numbers::{RenderLineNumbersType, LINE_NUMBERS};
use je_editor::config::editor_options::registry::CURSOR_STYLE;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::editor_common::ScrollType;
use je_editor::editor_state::{code_editor_state_flag, EditorState};
use je_editor::model::{IndentSize, ModelDeltaDecoration, TextModelUpdateOptions};
use je_editor::model_service::ModelService;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::services::clipboard_service::IClipboardService;
use je_editor::services_model::IModelService;
use je_editor::snippet::snippet_controller2::{PartialSnippetInsertOptions, SnippetController2};
use je_editor::snippet::snippet_parser::SnippetParser;
use je_editor::snippet::snippet_session::SnippetEdit;
use je_editor::text_model::{ModelDecorationOptions, TextModel};

use crate::api::browser::main_thread_documents::MainThreadDocuments;
use crate::api::common::ext_host_protocol::{
    ApplyEditsOptions, DecorationInstanceRenderOptionsDto, DecorationOptionsDto, EditorPropertiesChangeData, IndentSizeValue, InsertSpacesValue,
    MarkdownStringOrArray, ResolvedTextEditorConfiguration, SelectionChangeEvent, SnippetOptions, TabSizeValue, TextEditorConfigurationUpdate,
};
use crate::api::common::ext_host_types::TextEditorRevealType;
use crate::common::editor::IEditorPane;
use crate::services::editor::editor_service::{ActiveTextEditorControl, EditorControl, IActiveCodeEditor};

use je_editor::model::MarkdownString;
use je_editor::services::decoration_render_options::{DecorationRenderOptions, ThemeDecorationRenderOptions};

/// `IFocusTracker`.
pub trait IFocusTracker {
    fn on_gained_focus(&self);
    fn on_lost_focus(&self);
}

/// O `CodeEditorWidget` como o `IActiveCodeEditor` que o `IEditorService` devolve.
impl IActiveCodeEditor for CodeEditorWidget {
    fn get_selection(&self) -> Option<Selection> {
        ICodeEditor::get_selection(self)
    }

    fn get_value_in_range(&self, selection: &Selection) -> Option<JsString> {
        ICodeEditor::get_model(self).map(|model| model.get_value_in_range(selection))
    }

    fn has_widget_focus(&self) -> bool {
        ICodeEditor::has_widget_focus(self)
    }

    fn get_language_id(&self) -> Option<String> {
        ICodeEditor::get_model(self).map(|model| model.get_language_id())
    }
}

/// `MainThreadTextEditorProperties`.
#[derive(Clone, Debug, PartialEq)]
pub struct MainThreadTextEditorProperties {
    pub selections: Vec<Selection>,
    pub options: ResolvedTextEditorConfiguration,
    pub visible_ranges: Vec<Range>,
}

impl MainThreadTextEditorProperties {
    /// `readFromEditor(previousProperties, model, codeEditor)`.
    pub fn read_from_editor(previous_properties: Option<&MainThreadTextEditorProperties>, model: &TextModel, code_editor: Option<&Rc<CodeEditorWidget>>) -> Self {
        let selections = Self::read_selections_from_code_editor(previous_properties, code_editor);
        let options = Self::read_options_from_code_editor(previous_properties, model, code_editor);
        let visible_ranges = Self::read_visible_ranges_from_code_editor(code_editor);
        Self { selections, options, visible_ranges }
    }

    fn read_selections_from_code_editor(previous_properties: Option<&MainThreadTextEditorProperties>, code_editor: Option<&Rc<CodeEditorWidget>>) -> Vec<Selection> {
        let mut result: Option<Vec<Selection>> = None;
        if let Some(code_editor) = code_editor {
            result = code_editor.get_selections();
        }
        if result.is_none() {
            if let Some(previous_properties) = previous_properties {
                result = Some(previous_properties.selections.clone());
            }
        }
        result.unwrap_or_else(|| vec![Selection::new(1, 1, 1, 1)])
    }

    fn read_options_from_code_editor(
        previous_properties: Option<&MainThreadTextEditorProperties>,
        model: &TextModel,
        code_editor: Option<&Rc<CodeEditorWidget>>,
    ) -> ResolvedTextEditorConfiguration {
        if model.is_disposed() {
            return match previous_properties {
                // shutdown time
                Some(previous_properties) => previous_properties.options,
                None => je_base::errors::throw(Error::new("No valid properties")),
            };
        }

        let cursor_style: TextEditorCursorStyle;
        let line_numbers: RenderLineNumbersType;
        if let Some(code_editor) = code_editor {
            let options = code_editor.get_options();
            let line_numbers_opts = options.get(LINE_NUMBERS);
            cursor_style = *options.get(CURSOR_STYLE);
            line_numbers = line_numbers_opts.render_type;
        } else if let Some(previous_properties) = previous_properties {
            cursor_style = previous_properties.options.cursor_style;
            line_numbers = previous_properties.options.line_numbers;
        } else {
            cursor_style = TextEditorCursorStyle::Line;
            line_numbers = RenderLineNumbersType::On;
        }

        let model_options = model.get_options();
        ResolvedTextEditorConfiguration {
            insert_spaces: model_options.insert_spaces,
            tab_size: f64::from(model_options.tab_size),
            indent_size: f64::from(model_options.indent_size),
            original_indent_size: match model_options.original_indent_size() {
                IndentSize::TabSize => IndentSizeValue::TabSize,
                IndentSize::Size(size) => IndentSizeValue::Size(f64::from(size)),
            },
            cursor_style,
            line_numbers,
        }
    }

    fn read_visible_ranges_from_code_editor(code_editor: Option<&Rc<CodeEditorWidget>>) -> Vec<Range> {
        match code_editor {
            Some(code_editor) => code_editor.get_visible_ranges(),
            None => Vec::new(),
        }
    }

    /// `generateDelta(oldProps, selectionChangeSource)`.
    pub fn generate_delta(&self, old_props: Option<&MainThreadTextEditorProperties>, selection_change_source: Option<&str>) -> Option<EditorPropertiesChangeData> {
        let mut delta = EditorPropertiesChangeData { options: None, selections: None, visible_ranges: None };

        if old_props.is_none_or(|old_props| !Self::selections_equal(&old_props.selections, &self.selections)) {
            delta.selections = Some(SelectionChangeEvent { selections: self.selections.clone(), source: selection_change_source.map(str::to_string) });
        }

        if old_props.is_none_or(|old_props| !Self::options_equal(&old_props.options, &self.options)) {
            delta.options = Some(self.options);
        }

        if old_props.is_none_or(|old_props| !Self::ranges_equal(&old_props.visible_ranges, &self.visible_ranges)) {
            delta.visible_ranges = Some(self.visible_ranges.clone());
        }

        if delta.selections.is_some() || delta.options.is_some() || delta.visible_ranges.is_some() {
            // something changed
            return Some(delta);
        }
        // nothing changed
        None
    }

    fn selections_equal(a: &[Selection], b: &[Selection]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a_value, b_value)| a_value == b_value)
    }

    fn ranges_equal(a: &[Range], b: &[Range]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a_value, b_value)| a_value == b_value)
    }

    fn options_equal(a: &ResolvedTextEditorConfiguration, b: &ResolvedTextEditorConfiguration) -> bool {
        a.tab_size == b.tab_size
            && a.indent_size == b.indent_size
            && a.insert_spaces == b.insert_spaces
            && a.cursor_style == b.cursor_style
            && a.line_numbers == b.line_numbers
    }
}

/// `_decorationTypeKeysToIds` e `_decorationTypeSubtypes` do `CodeEditorWidget` e os três métodos que os usam (ver as
/// divergências do módulo).
#[derive(Default)]
struct DecorationsByType {
    /// `_decorationTypeKeysToIds`.
    key_to_ids: HashMap<String, Vec<String>>,
    /// `_decorationTypeSubtypes`: o conjunto de subtipos (na ordem de inserção) por chave.
    subtypes: HashMap<String, Vec<String>>,
}

/// `IMarkdownString` do DTO como o `MarkdownString` do modelo de decorações (que guarda só o conteúdo, ver `model.rs`).
fn to_markdown_strings(hover_message: &MarkdownStringOrArray) -> Vec<MarkdownString> {
    let convert = |markdown: &je_base::html_content::IMarkdownString| MarkdownString { value: markdown.value.to_string_lossy() };
    match hover_message {
        MarkdownStringOrArray::One(markdown) => vec![convert(markdown)],
        MarkdownStringOrArray::Many(markdowns) => markdowns.iter().map(convert).collect(),
    }
}

/// `IDecorationInstanceRenderOptions` como o `IDecorationRenderOptions` com que o tipo de decoração é registrado.
fn to_render_options(render_options: &DecorationInstanceRenderOptionsDto) -> DecorationRenderOptions {
    let theme = |options: &crate::api::common::ext_host_protocol::ThemeDecorationInstanceRenderOptionsDto| ThemeDecorationRenderOptions {
        before: options.before.clone(),
        after: options.after.clone(),
        ..ThemeDecorationRenderOptions::default()
    };
    DecorationRenderOptions {
        base: theme(&render_options.base),
        light: render_options.light.as_ref().map(theme),
        dark: render_options.dark.as_ref().map(theme),
        ..DecorationRenderOptions::default()
    }
}

impl DecorationsByType {
    /// `setDecorationsByType(description, decorationTypeKey, decorationOptions)`.
    fn set_decorations_by_type(&mut self, editor: &Rc<CodeEditorWidget>, description: &str, decoration_type_key: &str, decoration_options: &[DecorationOptionsDto]) {
        let Some(service) = editor.code_editor_service() else {
            return;
        };
        let decoration_service = service.decoration_service();

        let mut new_decorations_sub_types: Vec<String> = Vec::new();
        let old_decorations_sub_types = self.subtypes.get(decoration_type_key).cloned().unwrap_or_default();

        let mut new_model_decorations: Vec<ModelDeltaDecoration> = Vec::new();

        for decoration_option in decoration_options {
            let mut type_key = decoration_type_key.to_string();
            if let Some(render_options) = &decoration_option.render_options {
                // identify custom render options by a hash code over all keys and values
                // For custom render options register a decoration type if necessary
                let units: Vec<u16> = format!("{render_options:?}").encode_utf16().collect();
                let sub_type = format!("{:x}", string_hash(&units, 0));
                // The fact that `decorationTypeKey` appears in the typeKey has no influence
                // it is just a mechanism to get predictable and unique keys (repeatable for the same options and unique across clients)
                type_key = format!("{decoration_type_key}-{sub_type}");
                if !old_decorations_sub_types.contains(&sub_type) && !new_decorations_sub_types.contains(&sub_type) {
                    // decoration type did not exist before, register new one
                    decoration_service.register_decoration_type(description, &type_key, to_render_options(render_options), Some(decoration_type_key));
                }
                if !new_decorations_sub_types.contains(&sub_type) {
                    new_decorations_sub_types.push(sub_type);
                }
            }
            let mut opts = decoration_service.resolve_decoration_options(&type_key, decoration_option.hover_message.is_some());
            if let Some(hover_message) = &decoration_option.hover_message {
                opts.hover_message = Some(to_markdown_strings(hover_message));
            }
            new_model_decorations.push(ModelDeltaDecoration { range: decoration_option.range, options: ModelDecorationOptions::create_dynamic(&opts) });
        }

        // remove decoration sub types that are no longer used, deregister decoration type if necessary
        for sub_type in &old_decorations_sub_types {
            if !new_decorations_sub_types.contains(sub_type) {
                decoration_service.remove_decoration_type(&format!("{decoration_type_key}-{sub_type}"));
            }
        }
        self.subtypes.insert(decoration_type_key.to_string(), new_decorations_sub_types);

        // update all decorations
        let old_decorations_ids = self.key_to_ids.get(decoration_type_key).cloned().unwrap_or_default();
        let new_ids = editor.delta_decorations(&old_decorations_ids, &new_model_decorations);
        self.key_to_ids.insert(decoration_type_key.to_string(), new_ids);
    }

    /// `setDecorationsByTypeFast(decorationTypeKey, ranges)`.
    fn set_decorations_by_type_fast(&mut self, editor: &Rc<CodeEditorWidget>, decoration_type_key: &str, ranges: &[Range]) {
        let Some(service) = editor.code_editor_service() else {
            return;
        };
        let decoration_service = service.decoration_service();

        // remove decoration sub types that are no longer used, deregister decoration type if necessary
        let old_decorations_sub_types = self.subtypes.get(decoration_type_key).cloned().unwrap_or_default();
        for sub_type in &old_decorations_sub_types {
            decoration_service.remove_decoration_type(&format!("{decoration_type_key}-{sub_type}"));
        }
        self.subtypes.insert(decoration_type_key.to_string(), Vec::new());

        let opts = ModelDecorationOptions::create_dynamic(&decoration_service.resolve_decoration_options(decoration_type_key, false));
        let new_model_decorations: Vec<ModelDeltaDecoration> = ranges.iter().map(|range| ModelDeltaDecoration { range: *range, options: opts.clone() }).collect();

        // update all decorations
        let old_decorations_ids = self.key_to_ids.get(decoration_type_key).cloned().unwrap_or_default();
        let new_ids = editor.delta_decorations(&old_decorations_ids, &new_model_decorations);
        self.key_to_ids.insert(decoration_type_key.to_string(), new_ids);
    }

    /// `removeDecorationsByType(decorationTypeKey)`.
    fn remove_decorations_by_type(&mut self, editor: &Rc<CodeEditorWidget>, decoration_type_key: &str) {
        // remove decorations for type and sub type
        if let Some(old_decorations_ids) = self.key_to_ids.remove(decoration_type_key) {
            editor.remove_decorations(&old_decorations_ids);
        }
        if let Some(items) = self.subtypes.remove(decoration_type_key) {
            if let Some(service) = editor.code_editor_service() {
                let decoration_service = service.decoration_service();
                for sub_type in items {
                    decoration_service.remove_decoration_type(&format!("{decoration_type_key}-{sub_type}"));
                }
            }
        }
    }
}

/// `MainThreadTextEditor`: text editor that is permanently bound to the same model. It can be bound or not to a
/// CodeEditor.
pub struct MainThreadTextEditor {
    me: Weak<MainThreadTextEditor>,
    id: String,
    model: Rc<TextModel>,
    main_thread_documents: Rc<MainThreadDocuments>,
    model_service: Rc<ModelService>,
    clipboard_service: Rc<dyn IClipboardService>,
    model_listeners: Rc<DisposableStore>,
    code_editor: RefCell<Option<Rc<CodeEditorWidget>>>,
    focus_tracker: Rc<dyn IFocusTracker>,
    code_editor_listeners: Rc<DisposableStore>,
    properties: RefCell<Option<MainThreadTextEditorProperties>>,
    on_properties_changed: Rc<Emitter<EditorPropertiesChangeData>>,
    decorations_by_type: RefCell<DecorationsByType>,
    disposed: Cell<bool>,
}

impl MainThreadTextEditor {
    pub fn new(
        id: String,
        model: Rc<TextModel>,
        code_editor: Rc<CodeEditorWidget>,
        focus_tracker: Rc<dyn IFocusTracker>,
        main_thread_documents: Rc<MainThreadDocuments>,
        model_service: Rc<ModelService>,
        clipboard_service: Rc<dyn IClipboardService>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            id,
            model,
            main_thread_documents,
            model_service,
            clipboard_service,
            model_listeners: DisposableStore::new(),
            code_editor: RefCell::new(None),
            focus_tracker,
            code_editor_listeners: DisposableStore::new(),
            properties: RefCell::new(None),
            on_properties_changed: Emitter::new(),
            decorations_by_type: RefCell::new(DecorationsByType::default()),
            disposed: Cell::new(false),
        });

        let weak = this.me.clone();
        this.model_listeners.add(this.model.on_did_change_options().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_properties_now(None);
            }
        }));

        this.set_code_editor(Some(code_editor));
        this.update_properties_now(None);
        this
    }

    fn update_properties_now(&self, selection_change_source: Option<&str>) {
        let code_editor = self.code_editor.borrow().clone();
        let new_properties = {
            let properties = self.properties.borrow();
            MainThreadTextEditorProperties::read_from_editor(properties.as_ref(), &self.model, code_editor.as_ref())
        };
        self.set_properties(new_properties, selection_change_source);
    }

    fn set_properties(&self, new_properties: MainThreadTextEditorProperties, selection_change_source: Option<&str>) {
        let delta = new_properties.generate_delta(self.properties.borrow().as_ref(), selection_change_source);
        *self.properties.borrow_mut() = Some(new_properties);
        if let Some(delta) = delta {
            self.on_properties_changed.fire(delta);
        }
    }

    /// `getId()`.
    pub fn get_id(&self) -> &str {
        &self.id
    }

    /// `getModel()`.
    pub fn get_model(&self) -> &Rc<TextModel> {
        &self.model
    }

    /// `getCodeEditor()`.
    pub fn get_code_editor(&self) -> Option<Rc<CodeEditorWidget>> {
        self.code_editor.borrow().clone()
    }

    /// `hasCodeEditor(codeEditor)`.
    pub fn has_code_editor(&self, code_editor: Option<&Rc<CodeEditorWidget>>) -> bool {
        match (self.code_editor.borrow().as_ref(), code_editor) {
            (None, None) => true,
            (Some(current), Some(candidate)) => Rc::ptr_eq(current, candidate),
            _ => false,
        }
    }

    /// `setCodeEditor(codeEditor)`.
    pub fn set_code_editor(&self, code_editor: Option<Rc<CodeEditorWidget>>) {
        if self.has_code_editor(code_editor.as_ref()) {
            // Nothing to do...
            return;
        }
        self.code_editor_listeners.clear();

        *self.code_editor.borrow_mut() = code_editor.clone();
        let Some(code_editor) = code_editor else {
            return;
        };

        // Catch early the case that this code editor gets a different model set and disassociate from this model
        let weak = self.me.clone();
        self.code_editor_listeners.add(code_editor.on_did_change_model().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.set_code_editor(None);
                }
            }
        }));

        self.code_editor_listeners.add(code_editor.on_did_focus_editor_widget().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.focus_tracker.on_gained_focus();
                }
            }
        }));
        self.code_editor_listeners.add(code_editor.on_did_blur_editor_widget().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.focus_tracker.on_lost_focus();
                }
            }
        }));

        let next_selection_change_source: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        self.code_editor_listeners.add(self.main_thread_documents.on_is_caught_up_with_content_changes().listen({
            let weak = weak.clone();
            let next_selection_change_source = next_selection_change_source.clone();
            move |uri| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if uri.to_string(false) == this.model.uri().to_string(false) {
                    let selection_change_source = next_selection_change_source.borrow_mut().take();
                    this.update_properties_now(selection_change_source.as_deref());
                }
            }
        }));

        // Due to event timings, it is possible that there is a model change event not yet delivered to us.
        // > e.g. a model change event is emitted to a listener which then decides to update editor options
        // > In this case the editor configuration change event reaches us first.
        // So simply check that the model is still attached to this code editor
        fn is_valid_code_editor(this: &MainThreadTextEditor) -> bool {
            match this.code_editor.borrow().as_ref() {
                Some(code_editor) => code_editor.get_model().is_some_and(|model| Rc::ptr_eq(&model, &this.model)),
                None => false,
            }
        }

        // Some editor events get delivered faster than model content changes. This is
        // problematic, as this leads to editor properties reaching the extension host
        // too soon, before the model content change that was the root cause.
        //
        // If this case is identified, then let's update editor properties on the next model
        // content change instead.
        let update_properties = {
            let next_selection_change_source = next_selection_change_source.clone();
            move |this: &MainThreadTextEditor, selection_change_source: Option<&str>| {
                if this.main_thread_documents.is_caught_up_with_content_changes(&this.model.uri()) {
                    *next_selection_change_source.borrow_mut() = None;
                    this.update_properties_now(selection_change_source);
                } else {
                    // update editor properties on the next model content change
                    *next_selection_change_source.borrow_mut() = selection_change_source.map(str::to_string);
                }
            }
        };
        let update_properties = Rc::new(update_properties);

        self.code_editor_listeners.add(code_editor.on_did_change_cursor_selection().listen({
            let weak = weak.clone();
            let update_properties = update_properties.clone();
            move |e| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                // selection
                if !is_valid_code_editor(&this) {
                    return;
                }
                update_properties(&this, Some(e.source.as_str()));
            }
        }));
        self.code_editor_listeners.add(code_editor.on_did_change_configuration().listen({
            let weak = weak.clone();
            let update_properties = update_properties.clone();
            move |_| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                // options
                if !is_valid_code_editor(&this) {
                    return;
                }
                update_properties(&this, None);
            }
        }));
        self.code_editor_listeners.add(code_editor.on_did_layout_change().listen({
            let weak = weak.clone();
            let update_properties = update_properties.clone();
            move |_| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                // visibleRanges
                if !is_valid_code_editor(&this) {
                    return;
                }
                update_properties(&this, None);
            }
        }));
        self.code_editor_listeners.add(code_editor.on_did_scroll_change().listen({
            let weak = weak.clone();
            let update_properties = update_properties.clone();
            move |_| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                // visibleRanges
                if !is_valid_code_editor(&this) {
                    return;
                }
                update_properties(&this, None);
            }
        }));
        self.update_properties_now(None);
    }

    /// `isVisible()`.
    pub fn is_visible(&self) -> bool {
        self.code_editor.borrow().is_some()
    }

    /// `getProperties()`.
    pub fn get_properties(&self) -> MainThreadTextEditorProperties {
        self.properties.borrow().clone().expect("propriedades do editor lidas no construtor")
    }

    /// `onPropertiesChanged`.
    pub fn on_properties_changed(&self) -> Event<EditorPropertiesChangeData> {
        self.on_properties_changed.event()
    }

    /// `setSelections(selections)`.
    pub fn set_selections(&self, selections: &[Selection]) {
        let code_editor = self.code_editor.borrow().clone();
        if let Some(code_editor) = code_editor {
            code_editor.set_selections(selections, None, None);
            return;
        }

        let new_selections: Vec<Selection> = selections.to_vec();
        let current = self.get_properties();
        self.set_properties(MainThreadTextEditorProperties { selections: new_selections, options: current.options, visible_ranges: current.visible_ranges }, None);
    }

    /// `_setIndentConfiguration(newConfiguration)`.
    fn set_indent_configuration(&self, new_configuration: &TextEditorConfigurationUpdate) {
        let creation_opts = self.model_service.get_creation_options(&self.model.get_language_id(), Some(&self.model.uri()), self.model.is_for_simple_widget());

        if new_configuration.tab_size == Some(TabSizeValue::Auto) || new_configuration.insert_spaces == Some(InsertSpacesValue::Auto) {
            // one of the options was set to 'auto' => detect indentation
            let mut insert_spaces = creation_opts.insert_spaces;
            let mut tab_size = creation_opts.tab_size;

            if let Some(InsertSpacesValue::Bool(value)) = new_configuration.insert_spaces {
                insert_spaces = value;
            }

            if let Some(TabSizeValue::Size(value)) = new_configuration.tab_size {
                tab_size = value as i32;
            }

            self.model.detect_indentation(insert_spaces, tab_size);
            return;
        }

        let mut new_opts = TextModelUpdateOptions::default();
        if let Some(InsertSpacesValue::Bool(value)) = new_configuration.insert_spaces {
            new_opts.insert_spaces = Some(value);
        }
        if let Some(TabSizeValue::Size(value)) = new_configuration.tab_size {
            new_opts.tab_size = Some(value as i32);
        }
        if let Some(indent_size) = new_configuration.indent_size {
            new_opts.indent_size = Some(match indent_size {
                IndentSizeValue::TabSize => IndentSize::TabSize,
                IndentSizeValue::Size(size) => IndentSize::Size(size as i32),
            });
        }
        self.model.update_options(new_opts);
    }

    /// `setConfiguration(newConfiguration)`.
    pub fn set_configuration(&self, new_configuration: TextEditorConfigurationUpdate) {
        self.set_indent_configuration(&new_configuration);

        let Some(code_editor) = self.code_editor.borrow().clone() else {
            return;
        };

        if let Some(cursor_style) = new_configuration.cursor_style {
            let new_cursor_style = cursor_style_to_string(cursor_style);
            code_editor.update_options(Some(&Dynamic::object([("cursorStyle", Dynamic::String(JsString::from(new_cursor_style)))])));
        }

        if let Some(new_line_numbers) = new_configuration.line_numbers {
            let line_numbers = match new_line_numbers {
                RenderLineNumbersType::On => "on",
                RenderLineNumbersType::Relative => "relative",
                RenderLineNumbersType::Interval => "interval",
                _ => "off",
            };
            code_editor.update_options(Some(&Dynamic::object([("lineNumbers", Dynamic::String(JsString::from(line_numbers)))])));
        }
    }

    /// `setDecorations(key, ranges)`.
    pub fn set_decorations(&self, key: &str, ranges: &[DecorationOptionsDto]) {
        let Some(code_editor) = self.code_editor.borrow().clone() else {
            return;
        };
        self.decorations_by_type.borrow_mut().set_decorations_by_type(&code_editor, "exthost-api", key, ranges);
    }

    /// `setDecorationsFast(key, _ranges)`: `_ranges` são quartetos `startLine, startColumn, endLine, endColumn`.
    pub fn set_decorations_fast(&self, key: &str, ranges: &[f64]) {
        let Some(code_editor) = self.code_editor.borrow().clone() else {
            return;
        };
        let ranges: Vec<Range> = (0..ranges.len() / 4)
            .map(|i| Range::new(ranges[4 * i] as i32, ranges[4 * i + 1] as i32, ranges[4 * i + 2] as i32, ranges[4 * i + 3] as i32))
            .collect();
        self.decorations_by_type.borrow_mut().set_decorations_by_type_fast(&code_editor, key, &ranges);
    }

    /// `revealRange(range, revealType)`.
    pub fn reveal_range(&self, range: Range, reveal_type: TextEditorRevealType) {
        let Some(code_editor) = self.code_editor.borrow().clone() else {
            return;
        };
        match reveal_type {
            TextEditorRevealType::Default => code_editor.reveal_range(range, Some(ScrollType::Smooth), None, None),
            TextEditorRevealType::InCenter => code_editor.reveal_range_in_center(range, Some(ScrollType::Smooth)),
            TextEditorRevealType::InCenterIfOutsideViewport => code_editor.reveal_range_in_center_if_outside_viewport(range, Some(ScrollType::Smooth)),
            TextEditorRevealType::AtTop => code_editor.reveal_range_at_top(range, Some(ScrollType::Smooth)),
        }
    }

    /// `isFocused()`.
    pub fn is_focused(&self) -> bool {
        match self.code_editor.borrow().as_ref() {
            Some(code_editor) => ICodeEditor::has_text_focus(&**code_editor),
            None => false,
        }
    }

    /// `matches(editor)`: o controle do painel é o editor de código deste editor.
    pub fn matches(&self, editor: &dyn IEditorPane) -> bool {
        let code_editor = self.code_editor.borrow();
        let Some(code_editor) = code_editor.as_ref() else {
            // `getControl()` sem controle é `undefined`, e `undefined === null` é falso.
            return false;
        };
        match editor.get_control() {
            Some(EditorControl::Text(ActiveTextEditorControl::Code(control))) => std::ptr::eq(Rc::as_ptr(&control) as *const (), Rc::as_ptr(code_editor) as *const ()),
            _ => false,
        }
    }

    /// `applyEdits(versionIdCheck, edits, opts)`.
    pub fn apply_edits(&self, version_id_check: f64, edits: Vec<SingleEditOperation>, opts: ApplyEditsOptions) -> bool {
        if self.model.get_version_id() as f64 != version_id_check {
            // throw new Error('Model has changed in the meantime!');
            // model changed in the meantime
            return false;
        }

        let Some(code_editor) = self.code_editor.borrow().clone() else {
            // console.warn('applyEdits on invisible editor');
            return false;
        };

        if let Some(end_of_line) = opts.set_end_of_line {
            self.model.push_eol(end_of_line);
        }

        let transformed_edits: Vec<SingleEditOperation> = edits.into_iter().map(|edit| SingleEditOperation::new(edit.range, edit.text, edit.force_move_markers)).collect();

        if opts.undo_stop_before {
            code_editor.push_undo_stop();
        }
        code_editor.execute_edits(ExecuteEditsSource::Name(Some("MainThreadTextEditor")), transformed_edits, None::<EndCursorState<'_>>);
        if opts.undo_stop_after {
            code_editor.push_undo_stop();
        }
        true
    }

    /// `insertSnippet(modelVersionId, template, ranges, opts)`.
    pub fn insert_snippet(&self, model_version_id: f64, template: &str, ranges: &[Range], opts: SnippetOptions) -> Promise<bool> {
        let code_editor = self.code_editor.borrow().clone();
        let Some(code_editor) = code_editor.filter(|code_editor| code_editor.has_model()) else {
            return Promise::resolved(false);
        };
        let clipboard_service = self.clipboard_service.clone();
        let template = template.to_string();
        let ranges = ranges.to_vec();

        async_fn(async move {
            // check if clipboard is required and only iff read it (async)
            let mut clipboard_text: Option<JsString> = None;
            let template_units: Vec<u16> = template.encode_utf16().collect();
            let needs_template = SnippetParser::guess_needs_clipboard(&template_units);
            if needs_template {
                let state = EditorState::new(&*code_editor, code_editor_state_flag::VALUE | code_editor_state_flag::POSITION);
                clipboard_text = Some(JsString::from(clipboard_service.read_text().await?));
                if !state.validate(&*code_editor) {
                    return Ok(false);
                }
            }

            if code_editor.get_model().map(|model| model.get_version_id() as f64) != Some(model_version_id) {
                return Ok(false);
            }

            let Some(snippet_controller) = SnippetController2::get(&code_editor) else {
                return Ok(false);
            };

            code_editor.focus();

            // make modifications as snippet edit
            let edits: Vec<SnippetEdit> = ranges.iter().map(|range| SnippetEdit { range: *range, template: JsString::from(template.clone()), keep_whitespace: None }).collect();
            snippet_controller.apply(
                edits,
                Some(PartialSnippetInsertOptions {
                    overwrite_before: Some(0),
                    overwrite_after: Some(0),
                    undo_stop_before: Some(opts.undo_stop_before),
                    undo_stop_after: Some(opts.undo_stop_after),
                    adjust_whitespace: Some(!opts.keep_whitespace.unwrap_or(false)),
                    clipboard_text,
                    ..PartialSnippetInsertOptions::default()
                }),
            );

            Ok(true)
        })
    }
}

impl IDisposable for MainThreadTextEditor {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.model_listeners.dispose();
        self.on_properties_changed.dispose();
        let code_editor = self.code_editor.borrow_mut().take();
        if let Some(code_editor) = code_editor {
            let keys: Vec<String> = self.decorations_by_type.borrow().key_to_ids.keys().chain(self.decorations_by_type.borrow().subtypes.keys()).cloned().collect();
            for key in keys {
                self.decorations_by_type.borrow_mut().remove_decorations_by_type(&code_editor, &key);
            }
        }
        self.code_editor_listeners.dispose();
    }
}
