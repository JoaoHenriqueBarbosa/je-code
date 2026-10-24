//! Porte de `src/vs/editor/contrib/snippet/browser/snippetSession.ts`: a sessão de um snippet no editor
//! (`SnippetSession`), o snippet inserido em cada cursor (`OneSnippet`) e as decorações dos placeholders.
//!
//! Decisões de tradução:
//! - `IActiveCodeEditor` é o `Rc<CodeEditorWidget>` (o editor de teste entrega o seu por
//!   `TestCodeEditor::widget`). O `hasModel()` que o original confere antes de usar o modelo vira o
//!   `get_model()` com `Option`.
//! - `editor.invokeWithinContext(accessor => accessor.get(...))` e o `@ILanguageConfigurationService` do construtor
//!   são os [`SnippetServices`]: o `ILabelService`, o `IWorkspaceContextService` (opcional) e o
//!   `ILanguageConfigurationService`. As funções estáticas os recebem por argumento.
//! - Os cinco argumentos soltos de `createEditsAndSnippetsFromSelections`/`FromEdits` (`overwriteBefore`,
//!   `overwriteAfter`, `adjustWhitespace`, `clipboardText`, `overtypingCapturer`) são o
//!   [`SnippetSessionInsertOptions`] que a sessão já tem (as funções `FromEdits` não leem os dois `overwrite`).
//!   `ISnippetSessionInsertOptions` com `_defaultOptions` é o `Default`; `options = _defaultOptions` é `None`.
//! - `string | ISnippetEdit[]` é o [`SnippetTemplate`] (com `From` das três formas), e `ISnippetEdit` o
//!   [`SnippetEdit`].
//! - `Placeholder.index` do porte do parser é `u64` e imutável, e o `OneSnippet#merge` do original reescreve o
//!   índice com frações (`index + nested.index / nestingLevel`). Cada `OneSnippet` guarda então o seu próprio
//!   `Number` por placeholder (a tabela `indices`, semeada com o índice do parser), e é ela que faz o papel de
//!   `isFinalTabstop` (índice zero), de `Placeholder.compareByIndex` e das chaves de `computePossibleSelections`.
//!   O comportamento é o do original, inclusive o `_renormalizePlaceholderIndices` do #279349.
//! - Os `Map<Placeholder, string>` e `Set<Placeholder>` são tabelas por identidade ([`marker_id`]): a identidade
//!   do original (`===`) é o ponteiro do `Rc`. O `Map<number, Range[]>` de `computePossibleSelections` é uma lista
//!   de pares, que preserva a ordem de inserção como o `Map` do JavaScript.
//! - `OneSnippet#move` chama-se [`OneSnippet::move_to`] (`move` é palavra reservada). O `_move(fwd)` da sessão é
//!   o `move_selections`.
//! - Os `snippets` são `Rc<OneSnippet>`: a sessão os guarda e o callback do `executeEdits` os inicializa.
//! - `console.assert` vira `debug_assert!`; `throw new Error` e o `!` do TypeScript viram pânico, como no resto do
//!   porte.
//! - `EditSources.snippet()` ainda não existe em `text_model_edit_source.rs`: a origem da edição é montada aqui
//!   (`TextModelEditSource { source: "snippet", .. }`).
//! - `ModelDecorationOptions.register` das quatro decorações de placeholder fica em `thread_local`.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::{CompareResult, group_by};
use je_base::char_code as cc;
use je_base::dynamic::number_to_js_string;
use je_base::lifecycle::IDisposable;
use je_base::strings::{get_leading_whitespace, split_lines};
use je_platform::workspace::IWorkspaceContextService;

use super::snippet_parser::{Marker, MarkerKind, MarkerRef, SnippetParser, VariableResolver};
use super::snippet_variables::{
    ClipboardBasedVariableResolver, CommentBasedVariableResolver, CompositeSnippetVariableResolver, ILabelService,
    IOvertypingCapturer, ModelBasedVariableResolver, RandomBasedVariableResolver, ReadClipboardText,
    SelectionBasedVariableResolver, TimeBasedVariableResolver, WorkspaceBasedVariableResolver,
};
use crate::code_editor_widget::{CodeEditorWidget, EndCursorState, ExecuteEditsSource, ICodeEditor};
use crate::config::editor_options::registry::MULTI_CURSOR_PASTE;
use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::model::{ModelDecorationOptionsInit, SingleEditOperationIdentifier, TrackedRangeStickiness, ValidEditOperation};
use crate::position::Position;
use crate::range::Range;
use crate::selection::{Selection, SelectionDirection};
use crate::text_change::TextChange;
use crate::text_model::{ModelDecorationOptions, TextModel};
use crate::text_model_edit_source::TextModelEditSource;

/// A identidade de um marcador (o `===` do original): o endereço do `Rc`.
pub fn marker_id(marker: &MarkerRef) -> usize {
    Rc::as_ptr(marker) as usize
}

/// `Range.compareRangesUsingStarts` como `Ordering`, para o `sort`.
fn compare_starts(a: &Range, b: &Range) -> Ordering {
    Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0)
}

/// `lines.join(separator)`.
fn join_lines(lines: &[JsString], separator: &[u16]) -> JsString {
    let mut out = JsString::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_units(separator);
        }
        out.push_units(line.units());
    }
    out
}

/// `text.split(/\r\n|\r|\n/)`.
fn split_into_lines(text: &[u16]) -> Vec<JsString> {
    split_lines(text).into_iter().map(JsString::from).collect()
}

/// A origem de edição `EditSources.snippet()`.
fn snippet_edit_source() -> TextModelEditSource {
    TextModelEditSource { source: "snippet".to_owned(), name: None }
}

/// As quatro `ModelDecorationOptions` de um placeholder (`OneSnippet._decor`).
struct PlaceholderDecorationOptions {
    active: Rc<ModelDecorationOptions>,
    inactive: Rc<ModelDecorationOptions>,
    active_final: Rc<ModelDecorationOptions>,
    inactive_final: Rc<ModelDecorationOptions>,
}

fn register_placeholder_decoration(
    description: &str,
    stickiness: TrackedRangeStickiness,
    class_name: &str,
) -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: description.to_owned(),
        stickiness: Some(stickiness),
        class_name: Some(class_name.to_owned()),
        ..Default::default()
    })
}

thread_local! {
    static PLACEHOLDER_DECORATIONS: PlaceholderDecorationOptions = PlaceholderDecorationOptions {
        active: register_placeholder_decoration(
            "snippet-placeholder-1",
            TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges,
            "snippet-placeholder",
        ),
        inactive: register_placeholder_decoration(
            "snippet-placeholder-2",
            TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges,
            "snippet-placeholder",
        ),
        active_final: register_placeholder_decoration(
            "snippet-placeholder-3",
            TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges,
            "finish-snippet-placeholder",
        ),
        inactive_final: register_placeholder_decoration(
            "snippet-placeholder-4",
            TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges,
            "finish-snippet-placeholder",
        ),
    };
}

/// `OneSnippet._decor.active | inactive | activeFinal | inactiveFinal`.
fn placeholder_decoration(active: bool, is_final: bool) -> Rc<ModelDecorationOptions> {
    PLACEHOLDER_DECORATIONS.with(|options| {
        match (active, is_final) {
            (true, false) => &options.active,
            (false, false) => &options.inactive,
            (true, true) => &options.active_final,
            (false, true) => &options.inactive_final,
        }
        .clone()
    })
}

/// O resultado de `activeChoice`: `{ choice: Choice; range: Range }`.
#[derive(Clone, Debug)]
pub struct ActiveChoice {
    pub choice: MarkerRef,
    pub range: Range,
}

/// O snippet inserido em um cursor.
pub struct OneSnippet {
    editor: Rc<CodeEditorWidget>,
    snippet: MarkerRef,
    snippet_line_leading_whitespace: JsString,
    /// `_placeholderDecorations`: por identidade do placeholder, o placeholder (que mantém o endereço vivo) e o
    /// id da decoração.
    placeholder_decorations: RefCell<Option<HashMap<usize, (MarkerRef, String)>>>,
    placeholder_groups: RefCell<Vec<Vec<MarkerRef>>>,
    /// O `Placeholder.index` de cada placeholder (ver o topo do módulo).
    indices: RefCell<HashMap<usize, f64>>,
    /// `_offset`: `-1` até o `initialize`.
    offset: Cell<i64>,
    /// `_placeholderGroupsIdx`.
    placeholder_groups_idx: Cell<i32>,
    /// `_nestingLevel`.
    nesting_level: Cell<f64>,
}

impl OneSnippet {
    pub fn new(editor: Rc<CodeEditorWidget>, snippet: MarkerRef, snippet_line_leading_whitespace: JsString) -> Self {
        let indices = snippet
            .placeholders()
            .iter()
            .map(|placeholder| (marker_id(placeholder), placeholder.as_placeholder().expect("Placeholder").index as f64))
            .collect();
        let one = Self {
            editor,
            snippet,
            snippet_line_leading_whitespace,
            placeholder_decorations: RefCell::new(None),
            placeholder_groups: RefCell::new(Vec::new()),
            indices: RefCell::new(indices),
            offset: Cell::new(-1),
            placeholder_groups_idx: Cell::new(-1),
            nesting_level: Cell::new(1.0),
        };
        one.regroup_placeholders();
        one
    }

    /// O `TextmateSnippet` deste cursor (o `_snippet` do original, que o teste lê).
    pub fn snippet(&self) -> &MarkerRef {
        &self.snippet
    }

    /// `Placeholder.index`.
    fn index_of(&self, placeholder: &MarkerRef) -> f64 {
        self.indices
            .borrow()
            .get(&marker_id(placeholder))
            .copied()
            .unwrap_or_else(|| placeholder.as_placeholder().expect("Placeholder").index as f64)
    }

    /// `Placeholder#isFinalTabstop`.
    fn is_final_tabstop(&self, placeholder: &MarkerRef) -> bool {
        self.index_of(placeholder) == 0.0
    }

    /// `Placeholder.compareByIndex`.
    fn compare_by_index(&self, a: &MarkerRef, b: &MarkerRef) -> CompareResult {
        let (index_a, index_b) = (self.index_of(a), self.index_of(b));
        if index_a == index_b {
            0.0
        } else if index_a == 0.0 {
            1.0
        } else if index_b == 0.0 {
            -1.0
        } else if index_a < index_b {
            -1.0
        } else {
            1.0
        }
    }

    /// `groupBy(_snippet.placeholders, Placeholder.compareByIndex)`.
    fn regroup_placeholders(&self) {
        let groups = group_by(&self.snippet.placeholders(), |a, b| self.compare_by_index(a, b));
        *self.placeholder_groups.borrow_mut() = groups;
    }

    /// `_placeholderDecorations.get(placeholder)`.
    fn decoration_id(&self, placeholder: &MarkerRef) -> Option<String> {
        self.placeholder_decorations
            .borrow()
            .as_ref()?
            .get(&marker_id(placeholder))
            .map(|(_, id)| id.clone())
    }

    /// `id ? model.getDecorationRange(id) : null`.
    fn decoration_range(&self, model: &TextModel, placeholder: &MarkerRef) -> Option<Range> {
        self.decoration_id(placeholder).and_then(|id| model.get_decoration_range(&id))
    }

    /// O grupo de placeholders ativo (`_placeholderGroups[_placeholderGroupsIdx]`).
    fn active_group(&self) -> Vec<MarkerRef> {
        let idx = usize::try_from(self.placeholder_groups_idx.get()).expect("o snippet tem um grupo de placeholders ativo");
        self.placeholder_groups.borrow()[idx].clone()
    }

    pub fn initialize(&self, text_change: &TextChange) {
        self.offset.set(text_change.new_position as i64);
    }

    /// O intervalo de um placeholder no modelo, a partir do deslocamento do snippet e do próprio snippet `owner`.
    fn placeholder_range(model: &TextModel, snippet_offset: i64, owner: &MarkerRef, placeholder: &MarkerRef) -> Range {
        let placeholder_offset = owner.offset(placeholder).expect("o placeholder pertence ao snippet") as i64;
        let placeholder_len = owner.full_len(placeholder) as i64;
        Range::from_positions(
            model.get_position_at(snippet_offset + placeholder_offset),
            model.get_position_at(snippet_offset + placeholder_offset + placeholder_len),
        )
    }

    fn init_decorations(&self) {
        if self.offset.get() == -1 {
            panic!("Snippet not initialized!");
        }

        if self.placeholder_decorations.borrow().is_some() {
            // already initialized
            return;
        }

        let Some(model) = self.editor.get_model() else {
            return;
        };
        let mut decorations: HashMap<usize, (MarkerRef, String)> = HashMap::new();

        self.editor.change_decorations(|accessor| {
            // create a decoration for each placeholder
            for placeholder in self.snippet.placeholders() {
                let range = Self::placeholder_range(&model, self.offset.get(), &self.snippet, &placeholder);
                let options = placeholder_decoration(false, self.is_final_tabstop(&placeholder));
                let handle = accessor.add_decoration(&range, options);
                decorations.insert(marker_id(&placeholder), (placeholder.clone(), handle));
            }
        });
        *self.placeholder_decorations.borrow_mut() = Some(decorations);
    }

    /// `move(fwd)`.
    pub fn move_to(&self, fwd: Option<bool>) -> Vec<Selection> {
        let Some(model) = self.editor.get_model() else {
            return Vec::new();
        };

        self.init_decorations();

        // Transform placeholder text if necessary
        if self.placeholder_groups_idx.get() >= 0 {
            let mut operations: Vec<SingleEditOperation> = Vec::new();

            for placeholder in &self.active_group() {
                // Check if the placeholder has a transformation
                let Some(transform) = placeholder.transform() else {
                    continue;
                };
                if let Some(range) = self.decoration_range(&model, placeholder) {
                    let current_value = model.get_value_in_range(&range);
                    let transformed_value = transform.resolve_transform(current_value.units());
                    let mut transformed_value_lines = split_into_lines(transformed_value.units());
                    // fix indentation for transformed lines
                    for line in transformed_value_lines.iter_mut().skip(1) {
                        *line = model.normalize_indentation(prefixed(&self.snippet_line_leading_whitespace, line).units());
                    }
                    operations.push(EditOperation::replace(
                        range,
                        Some(join_lines(&transformed_value_lines, model.get_eol().as_units())),
                    ));
                }
            }
            if !operations.is_empty() {
                self.editor.execute_edits(ExecuteEditsSource::Name(Some("snippet.placeholderTransform")), operations, None);
            }
        }

        let mut could_skip_this_placeholder = false;
        let groups_len = self.placeholder_groups.borrow().len() as i32;
        let groups_idx = self.placeholder_groups_idx.get();
        if fwd == Some(true) && groups_idx < groups_len - 1 {
            self.placeholder_groups_idx.set(groups_idx + 1);
            could_skip_this_placeholder = true;
        } else if fwd == Some(false) && groups_idx > 0 {
            self.placeholder_groups_idx.set(groups_idx - 1);
            could_skip_this_placeholder = true;
        } else {
            // the selection of the current placeholder might
            // not acurate any more -> simply restore it
        }

        let new_selections = model.change_decorations(
            |accessor| {
                let mut active_placeholders: HashSet<usize> = HashSet::new();

                // change stickiness to always grow when typing at its edges
                // because these decorations represent the currently active
                // tabstop.
                // Special case #1: reaching the final tabstop
                // Special case #2: placeholders enclosing active placeholders
                let mut selections: Vec<Selection> = Vec::new();
                for placeholder in &self.active_group() {
                    let id = self.decoration_id(placeholder);
                    let range = id.as_deref().and_then(|id| model.get_decoration_range(id));

                    // consider to skip this placeholder index when the decoration
                    // range is empty but when the placeholder wasn't. that's a strong
                    // hint that the placeholder has been deleted. (all placeholder must match this)
                    could_skip_this_placeholder =
                        could_skip_this_placeholder && self.has_placeholder_been_collapsed(&model, placeholder);

                    let (Some(id), Some(range)) = (id, range) else {
                        continue;
                    };
                    selections.push(Selection::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column));

                    accessor.change_decoration_options(&id, placeholder_decoration(true, self.is_final_tabstop(placeholder)));
                    active_placeholders.insert(marker_id(placeholder));

                    for enclosing_placeholder in self.snippet.enclosing_placeholders(placeholder) {
                        if let Some(id) = self.decoration_id(&enclosing_placeholder) {
                            accessor.change_decoration_options(
                                &id,
                                placeholder_decoration(true, self.is_final_tabstop(&enclosing_placeholder)),
                            );
                            active_placeholders.insert(marker_id(&enclosing_placeholder));
                        }
                    }
                }

                // change stickness to never grow when typing at its edges
                // so that in-active tabstops never grow
                let inactive: Vec<(MarkerRef, String)> = self
                    .placeholder_decorations
                    .borrow()
                    .as_ref()
                    .expect("as decorações foram criadas")
                    .iter()
                    .filter(|(key, _)| !active_placeholders.contains(key))
                    .map(|(_, entry)| entry.clone())
                    .collect();
                for (placeholder, id) in inactive {
                    accessor.change_decoration_options(&id, placeholder_decoration(false, self.is_final_tabstop(&placeholder)));
                }

                selections
            },
            0,
        );

        if could_skip_this_placeholder { self.move_to(fwd) } else { new_selections.unwrap_or_default() }
    }

    fn has_placeholder_been_collapsed(&self, model: &TextModel, placeholder: &MarkerRef) -> bool {
        // A placeholder is empty when it wasn't empty when authored but
        // when its tracking decoration is empty. This also applies to all
        // potential parent placeholders
        let mut marker = Some(placeholder.clone());
        while let Some(current) = marker {
            if current.as_placeholder().is_some() {
                let range = self.decoration_range(model, &current);
                if range.is_none_or(|range| range.is_empty()) && !current.to_js_string().is_empty() {
                    return true;
                }
            }
            marker = current.parent();
        }
        false
    }

    pub fn is_at_first_placeholder(&self) -> bool {
        self.placeholder_groups_idx.get() <= 0 || self.placeholder_groups.borrow().is_empty()
    }

    pub fn is_at_last_placeholder(&self) -> bool {
        i64::from(self.placeholder_groups_idx.get()) == self.placeholder_groups.borrow().len() as i64 - 1
    }

    pub fn has_placeholder(&self) -> bool {
        !self.snippet.placeholders().is_empty()
    }

    /// A snippet is trivial when it has no placeholder or only a final placeholder at
    /// its very end
    pub fn is_trivial_snippet(&self) -> bool {
        let placeholders = self.snippet.placeholders();
        match placeholders.as_slice() {
            [] => true,
            [placeholder] => {
                self.is_final_tabstop(placeholder) && Rc::ptr_eq(&self.snippet.right_most_descendant(), placeholder)
            }
            _ => false,
        }
    }

    /// `computePossibleSelections()`: para cada índice de placeholder, os intervalos de todas as suas ocorrências
    /// (na ordem em que o índice apareceu).
    pub fn compute_possible_selections(&self) -> Vec<(f64, Vec<Range>)> {
        let mut result: Vec<(f64, Vec<Range>)> = Vec::new();
        let Some(model) = self.editor.get_model() else {
            return result;
        };
        for placeholders_with_equal_index in self.placeholder_groups.borrow().iter() {
            let mut ranges: Option<(f64, Vec<Range>)> = None;

            for placeholder in placeholders_with_equal_index {
                if self.is_final_tabstop(placeholder) {
                    // ignore those
                    break;
                }

                let Some(range) = self.decoration_range(&model, placeholder) else {
                    // one of the placeholder lost its decoration and
                    // therefore we bail out and pretend the placeholder
                    // (with its mirrors) doesn't exist anymore.
                    ranges = None;
                    break;
                };

                ranges.get_or_insert_with(|| (self.index_of(placeholder), Vec::new())).1.push(range);
            }
            result.extend(ranges);
        }
        result
    }

    pub fn active_choice(&self) -> Option<ActiveChoice> {
        self.placeholder_decorations.borrow().as_ref()?;
        let idx = usize::try_from(self.placeholder_groups_idx.get()).ok()?;
        let placeholder = self.placeholder_groups.borrow().get(idx)?.first()?.clone();
        let choice = placeholder.choice()?;
        let range = self.decoration_range(&self.editor.get_model()?, &placeholder)?;
        Some(ActiveChoice { range, choice })
    }

    pub fn has_choice(&self) -> bool {
        let mut result = false;
        self.snippet.walk(|marker| {
            result = marker.kind() == MarkerKind::Choice;
            !result
        });
        result
    }

    pub fn active_placeholder_count(&self) -> usize {
        match usize::try_from(self.placeholder_groups_idx.get()) {
            Ok(idx) => self.placeholder_groups.borrow()[idx].len(),
            Err(_) => 0,
        }
    }

    /// `merge(others)`: troca cada placeholder ativo por um dos snippets de `others` (tirados do começo).
    pub fn merge(&self, others: &mut Vec<Rc<OneSnippet>>) {
        let Some(model) = self.editor.get_model() else {
            return;
        };
        self.nesting_level.set(self.nesting_level.get() * 10.0);

        self.editor.change_decorations(|accessor| {
            // For each active placeholder take one snippet and merge it
            // in that the placeholder (can be many for `$1foo$1foo`). Because
            // everything is sorted by editor selection we can simply remove
            // elements from the beginning of the array
            for placeholder in &self.active_group() {
                let nested = others.remove(0);
                debug_assert!(nested.offset.get() != -1);
                debug_assert!(nested.placeholder_decorations.borrow().is_none());

                // Massage placeholder-indicies of the nested snippet to be
                // sorted right after the insertion point. This ensures we move
                // through the placeholders in the correct order
                let nested_info = nested.snippet.placeholder_info();
                let index_last_placeholder =
                    nested.index_of(nested_info.last.as_ref().expect("o snippet aninhado tem placeholders"));
                let nesting_level = self.nesting_level.get();
                let placeholder_index = self.index_of(placeholder);

                for nested_placeholder in &nested_info.all {
                    let new_index = if nested.is_final_tabstop(nested_placeholder) {
                        placeholder_index + (index_last_placeholder + 1.0) / nesting_level
                    } else {
                        placeholder_index + nested.index_of(nested_placeholder) / nesting_level
                    };
                    self.indices.borrow_mut().insert(marker_id(nested_placeholder), new_index);
                }
                self.snippet.replace(placeholder, nested.snippet.children());

                // Remove the placeholder at which position are inserting
                // the snippet and also remove its decoration.
                let removed = self
                    .placeholder_decorations
                    .borrow_mut()
                    .as_mut()
                    .expect("as decorações foram criadas")
                    .remove(&marker_id(placeholder));
                if let Some((_, id)) = removed {
                    accessor.remove_decoration(&id);
                }

                // For each *new* placeholder we create decoration to monitor
                // how and if it grows/shrinks.
                for nested_placeholder in nested.snippet.placeholders() {
                    let range = Self::placeholder_range(&model, nested.offset.get(), &nested.snippet, &nested_placeholder);
                    let handle = accessor.add_decoration(&range, placeholder_decoration(false, false));
                    self.placeholder_decorations
                        .borrow_mut()
                        .as_mut()
                        .expect("as decorações foram criadas")
                        .insert(marker_id(&nested_placeholder), (nested_placeholder.clone(), handle));
                }
            }

            // Renormalize fractional placeholder indicies back to small integers.
            // Without this, deeply nested merges (~16+ levels) lose floating-point
            // precision so distinct placeholders collapse onto the same index and
            // produce phantom cursors. #279349
            self.renormalize_placeholder_indices();

            // Last, re-create the placeholder groups by sorting placeholders by their index.
            self.regroup_placeholders();
        });
    }

    fn renormalize_placeholder_indices(&self) {
        let placeholders = self.snippet.placeholders();
        let mut unique_indices: Vec<f64> = Vec::new();
        for placeholder in &placeholders {
            if !self.is_final_tabstop(placeholder) {
                let index = self.index_of(placeholder);
                if !unique_indices.contains(&index) {
                    unique_indices.push(index);
                }
            }
        }
        unique_indices.sort_by(|a, b| a.total_cmp(b));
        for placeholder in &placeholders {
            if !self.is_final_tabstop(placeholder) {
                let old_index = self.index_of(placeholder);
                let position = unique_indices
                    .iter()
                    .position(|index| *index == old_index)
                    .expect("o índice veio da lista dos únicos");
                self.indices.borrow_mut().insert(marker_id(placeholder), (position + 1) as f64);
            }
        }
        self.nesting_level.set(1.0);
    }

    pub fn get_enclosing_range(&self) -> Option<Range> {
        let model = self.editor.get_model()?;
        let ids: Vec<String> = self
            .placeholder_decorations
            .borrow()
            .as_ref()?
            .values()
            .map(|(_, id)| id.clone())
            .collect();
        let mut result: Option<Range> = None;
        for decoration_id in ids {
            let placeholder_range = model.get_decoration_range(&decoration_id);
            result = match (result, placeholder_range) {
                (None, range) => range,
                (Some(current), Some(range)) => Some(current.plus_range(&range)),
                (Some(current), None) => Some(current),
            };
        }
        result
    }
}

impl IDisposable for OneSnippet {
    fn dispose(&self) {
        if let Some(decorations) = self.placeholder_decorations.borrow().as_ref() {
            let ids: Vec<String> = decorations.values().map(|(_, id)| id.clone()).collect();
            self.editor.remove_decorations(&ids);
        }
        self.placeholder_groups.borrow_mut().clear();
    }
}

/// `ISnippetSessionInsertOptions`: o `Default` é o `_defaultOptions`.
#[derive(Clone)]
pub struct SnippetSessionInsertOptions {
    pub overwrite_before: i32,
    pub overwrite_after: i32,
    pub adjust_whitespace: bool,
    pub clipboard_text: Option<JsString>,
    pub overtyping_capturer: Option<Rc<dyn IOvertypingCapturer>>,
}

impl Default for SnippetSessionInsertOptions {
    fn default() -> Self {
        Self {
            overwrite_before: 0,
            overwrite_after: 0,
            adjust_whitespace: true,
            clipboard_text: None,
            overtyping_capturer: None,
        }
    }
}

/// `ISnippetEdit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnippetEdit {
    pub range: Range,
    pub template: JsString,
    pub keep_whitespace: Option<bool>,
}

/// `string | ISnippetEdit[]`.
#[derive(Clone, Debug)]
pub enum SnippetTemplate {
    Text(JsString),
    Edits(Vec<SnippetEdit>),
}

impl From<&str> for SnippetTemplate {
    fn from(template: &str) -> Self {
        SnippetTemplate::Text(JsString::from(template))
    }
}

impl From<JsString> for SnippetTemplate {
    fn from(template: JsString) -> Self {
        SnippetTemplate::Text(template)
    }
}

impl From<Vec<SnippetEdit>> for SnippetTemplate {
    fn from(edits: Vec<SnippetEdit>) -> Self {
        SnippetTemplate::Edits(edits)
    }
}

/// Os serviços que o original busca por `invokeWithinContext` e por `@ILanguageConfigurationService`.
pub struct SnippetServices {
    pub language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    pub label_service: Rc<dyn ILabelService>,
    pub workspace_service: Option<Rc<dyn IWorkspaceContextService>>,
}

/// O que `createEditsAndSnippetsFromSelections` e `createEditsAndSnippetsFromEdits` devolvem.
#[derive(Default)]
pub struct EditsAndSnippets {
    pub edits: Vec<SingleEditOperation>,
    pub snippets: Vec<Rc<OneSnippet>>,
}

/// `() => clipboardText`.
fn clipboard_reader(clipboard_text: &Option<JsString>) -> ReadClipboardText {
    let clipboard_text = clipboard_text.clone();
    Rc::new(move || clipboard_text.clone())
}

/// O edit de uma inserção, marcado para só os nossos edits de desfazer gerarem os cursores finais.
fn tracked_edit(range: Range, text: JsString, major: usize) -> SingleEditOperation {
    let mut edit = EditOperation::replace(range, Some(text));
    edit.identifier = Some(SingleEditOperationIdentifier { major: major as i32, minor: 0 });
    edit.is_tracked = true;
    edit
}

/// As posições finais dos edits de desfazer, como cursores sem seleção.
fn end_positions_as_selections(undo_edits: &[&ValidEditOperation]) -> Vec<Selection> {
    undo_edits
        .iter()
        .map(|edit| {
            let end = edit.range.get_end_position();
            Selection::from_positions(end, end)
        })
        .collect()
}

/// Os edits de desfazer que são nossos. Às vezes o buffer de texto remove o espaço em branco automático ao
/// fazer qualquer edição, então só interessam os edits de desfazer que são nossos: os nossos têm um
/// `identifier`, e é assim que se distinguem dos outros.
fn own_undo_edits(undo_edits: &[ValidEditOperation]) -> Vec<&ValidEditOperation> {
    undo_edits.iter().filter(|edit| edit.identifier.is_some()).collect()
}

/// `SnippetSession`.
pub struct SnippetSession {
    editor: Rc<CodeEditorWidget>,
    template: SnippetTemplate,
    options: SnippetSessionInsertOptions,
    services: Rc<SnippetServices>,
    /// `_templateMerges`: o nível de aninhamento, o índice do grupo e o template de cada `merge`.
    template_merges: RefCell<Vec<(f64, i32, JsString)>>,
    snippets: RefCell<Vec<Rc<OneSnippet>>>,
}

impl SnippetSession {
    pub fn adjust_whitespace(
        model: &TextModel,
        position: Position,
        adjust_indentation: bool,
        snippet: &MarkerRef,
        filter: Option<&HashSet<usize>>,
    ) -> JsString {
        let line = model.get_line_content(position.line_number);
        let line_leading_whitespace =
            JsString::from(get_leading_whitespace(line.units(), 0, (position.column - 1).max(0) as usize));

        // the snippet as inserted
        let mut snippet_text_string: Option<JsString> = None;

        snippet.walk(|marker| {
            // all text elements that are not inside choice
            let Some(text) = marker.as_text() else {
                return true;
            };
            if marker.parent().is_some_and(|parent| parent.kind() == MarkerKind::Choice) {
                return true;
            }

            // check with filter (iff provided)
            if filter.is_some_and(|filter| !filter.contains(&marker_id(marker))) {
                return true;
            }

            let value = text.value.borrow().clone();
            let mut lines = split_into_lines(value.units());

            if adjust_indentation {
                // adjust indentation of snippet test
                // -the snippet-start doesn't get extra-indented (lineLeadingWhitespace), only normalized
                // -all N+1 lines get extra-indented and normalized
                // -the text start get extra-indented and normalized when following a linebreak
                match snippet.offset(marker) {
                    Some(0) => {
                        // snippet start
                        lines[0] = model.normalize_indentation(lines[0].units());
                    }
                    Some(offset) => {
                        // check if text start is after a linebreak
                        let snippet_text = snippet_text_string.get_or_insert_with(|| snippet.to_js_string());
                        let prev_char = snippet_text.units().get(offset - 1).copied();
                        if prev_char == Some(cc::LineFeed) || prev_char == Some(cc::CarriageReturn) {
                            lines[0] = model.normalize_indentation(prefixed(&line_leading_whitespace, &lines[0]).units());
                        }
                    }
                    None => {}
                }
                for line in lines.iter_mut().skip(1) {
                    *line = model.normalize_indentation(prefixed(&line_leading_whitespace, line).units());
                }
            }

            let new_value = join_lines(&lines, model.get_eol().as_units());
            if new_value != value {
                let parent = marker.parent().expect("o Text a trocar tem pai");
                parent.replace(marker, vec![Marker::new_text(new_value)]);
                snippet_text_string = None;
            }
            true
        });

        line_leading_whitespace
    }

    pub fn adjust_selection(model: &TextModel, selection: &Selection, overwrite_before: i32, overwrite_after: i32) -> Selection {
        if overwrite_before != 0 || overwrite_after != 0 {
            // overwrite[Before|After] is compute using the position, not the whole
            // selection. therefore we adjust the selection around that position
            let (position_line_number, position_column) = (selection.position_line_number, selection.position_column);
            let position_column_before = position_column - overwrite_before;
            let position_column_after = position_column + overwrite_after;

            let range = model.validate_range(&Range::new(
                position_line_number,
                position_column_before,
                position_line_number,
                position_column_after,
            ));

            return Selection::create_with_direction(
                range.start_line_number,
                range.start_column,
                range.end_line_number,
                range.end_column,
                selection.get_direction(),
            );
        }
        *selection
    }

    pub fn create_edits_and_snippets_from_selections(
        editor: &Rc<CodeEditorWidget>,
        template: &JsString,
        enforce_final_tabstop: bool,
        options: &SnippetSessionInsertOptions,
        services: &SnippetServices,
    ) -> EditsAndSnippets {
        let Some(model) = editor.get_model() else {
            return EditsAndSnippets::default();
        };

        let model_based_variable_resolver: Rc<dyn VariableResolver> =
            Rc::new(ModelBasedVariableResolver::new(services.label_service.clone(), model.clone()));
        let read_clipboard_text = clipboard_reader(&options.clipboard_text);
        let clipboard_spread = editor.get_option(MULTI_CURSOR_PASTE).as_str() == "spread";

        // know what text the overwrite[Before|After] extensions
        // of the primary cursor have selected because only when
        // secondary selections extend to the same text we can grow them
        let primary_selection = editor.get_selection().expect("o editor tem o modelo");
        let first_before_text = model.get_value_in_range(&Self::adjust_selection(
            &model,
            &primary_selection,
            options.overwrite_before,
            0,
        ));
        let first_after_text =
            model.get_value_in_range(&Self::adjust_selection(&model, &primary_selection, 0, options.overwrite_after));

        // remember the first non-whitespace column to decide if
        // `keepWhitespace` should be overruled for secondary selections
        let first_line_first_non_whitespace = model.get_line_first_non_whitespace_column(primary_selection.position_line_number);

        // sort selections by their start position but remeber
        // the original index. that allows you to create correct
        // offset-based selection logic without changing the
        // primary selection
        let mut indexed_selections: Vec<(usize, Selection)> =
            editor.get_selections().expect("o editor tem o modelo").into_iter().enumerate().collect();
        indexed_selections.sort_by(|a, b| compare_starts(&a.1, &b.1));

        let mut edits: Vec<Option<SingleEditOperation>> = vec![None; indexed_selections.len()];
        let mut snippets: Vec<Option<Rc<OneSnippet>>> = vec![None; indexed_selections.len()];

        for &(idx, selection) in &indexed_selections {
            // extend selection with the `overwriteBefore` and `overwriteAfter` and then
            // compare if this matches the extensions of the primary selection
            let mut extension_before = Self::adjust_selection(&model, &selection, options.overwrite_before, 0);
            let mut extension_after = Self::adjust_selection(&model, &selection, 0, options.overwrite_after);
            if first_before_text != model.get_value_in_range(&extension_before) {
                extension_before = selection;
            }
            if first_after_text != model.get_value_in_range(&extension_after) {
                extension_after = selection;
            }

            // merge the before and after selection into one
            let snippet_selection = selection
                .set_start_position(extension_before.start_line_number, extension_before.start_column)
                .set_end_position(extension_after.end_line_number, extension_after.end_column);

            let snippet = SnippetParser::new().parse(template.units(), true, enforce_final_tabstop);

            // adjust the template string to match the indentation and
            // whitespace rules of this insert location (can be different for each cursor)
            // happens when being asked for (default) or when this is a secondary
            // cursor and the leading whitespace is different
            let start = snippet_selection.get_start_position();
            let snippet_line_leading_whitespace = Self::adjust_whitespace(
                &model,
                start,
                options.adjust_whitespace
                    || (idx > 0
                        && first_line_first_non_whitespace != model.get_line_first_non_whitespace_column(selection.position_line_number)),
                &snippet,
                None,
            );

            let resolvers: Vec<Rc<dyn VariableResolver>> = vec![
                model_based_variable_resolver.clone(),
                Rc::new(ClipboardBasedVariableResolver::new(
                    read_clipboard_text.clone(),
                    idx,
                    indexed_selections.len(),
                    clipboard_spread,
                )),
                Rc::new(SelectionBasedVariableResolver::new(model.clone(), selection, idx, options.overtyping_capturer.clone())),
                Rc::new(CommentBasedVariableResolver::new(model.clone(), selection, services.language_configuration_service.clone())),
                Rc::new(TimeBasedVariableResolver::default()),
                Rc::new(WorkspaceBasedVariableResolver::new(services.workspace_service.clone())),
                Rc::new(RandomBasedVariableResolver),
            ];
            snippet.resolve_variables(&CompositeSnippetVariableResolver::new(resolvers));

            // store snippets with the index of their originating selection.
            // that ensures the primary cursor stays primary despite not being
            // the one with lowest start position
            edits[idx] = Some(tracked_edit(*snippet_selection, snippet.to_js_string(), idx));
            snippets[idx] = Some(Rc::new(OneSnippet::new(editor.clone(), snippet, snippet_line_leading_whitespace)));
        }

        EditsAndSnippets {
            edits: edits.into_iter().map(|edit| edit.expect("todo cursor gera um edit")).collect(),
            snippets: snippets.into_iter().map(|snippet| snippet.expect("todo cursor gera um snippet")).collect(),
        }
    }

    pub fn create_edits_and_snippets_from_edits(
        editor: &Rc<CodeEditorWidget>,
        snippet_edits: &[SnippetEdit],
        enforce_final_tabstop: bool,
        options: &SnippetSessionInsertOptions,
        services: &SnippetServices,
    ) -> EditsAndSnippets {
        let Some(model) = editor.get_model() else {
            return EditsAndSnippets::default();
        };
        if snippet_edits.is_empty() {
            return EditsAndSnippets::default();
        }

        let mut edits: Vec<SingleEditOperation> = Vec::new();

        let mut parser = SnippetParser::new();
        let snippet = Marker::new_textmate_snippet();

        let model_based_variable_resolver: Rc<dyn VariableResolver> =
            Rc::new(ModelBasedVariableResolver::new(services.label_service.clone(), model.clone()));
        let time_based_variable_resolver: Rc<dyn VariableResolver> = Rc::new(TimeBasedVariableResolver::default());
        let workspace_based_variable_resolver: Rc<dyn VariableResolver> =
            Rc::new(WorkspaceBasedVariableResolver::new(services.workspace_service.clone()));
        let random_based_variable_resolver: Rc<dyn VariableResolver> = Rc::new(RandomBasedVariableResolver);
        let read_clipboard_text = clipboard_reader(&options.clipboard_text);
        let clipboard_spread = editor.get_option(MULTI_CURSOR_PASTE).as_str() == "spread";

        // keep caller's original index so $CURSOR_INDEX/$CURSOR_NUMBER reflect input order, not range-sorted order
        let mut indexed_snippet_edits: Vec<(usize, &SnippetEdit)> = snippet_edits.iter().enumerate().collect();
        indexed_snippet_edits.sort_by(|a, b| compare_starts(&a.1.range, &b.1.range));

        let mut offset = 0usize;
        for (i, &(idx, edit)) in indexed_snippet_edits.iter().enumerate() {
            // gaps between snippet edits are appended as text nodes. this
            // ensures placeholder-offsets are later correct
            if i > 0 {
                let last_range = indexed_snippet_edits[i - 1].1.range;
                let text_range = Range::from_positions(last_range.get_end_position(), edit.range.get_start_position());
                let text_node = Marker::new_text(model.get_value_in_range(&text_range));
                offset += text_node.len();
                snippet.append_child(text_node);
            }

            // snapshot already-resolved variables so this edit's resolver only touches
            // (a) variables in the newly parsed fragment and (b) clones backfilled by
            // parseFragment into earlier placeholders sharing the same index (#206121)
            let mut pre_existing_variables: HashSet<usize> = HashSet::new();
            snippet.walk(|marker| {
                if marker.as_variable().is_some() {
                    pre_existing_variables.insert(marker_id(marker));
                }
                true
            });

            let new_nodes = parser.parse_fragment(edit.template.units(), &snippet);
            let new_nodes_ids: HashSet<usize> = new_nodes.iter().map(marker_id).collect();
            Self::adjust_whitespace(
                &model,
                edit.range.get_start_position(),
                edit.keep_whitespace.map_or(options.adjust_whitespace, |keep_whitespace| !keep_whitespace),
                &snippet,
                Some(&new_nodes_ids),
            );

            let edit_selection = Selection::from_range(&edit.range, SelectionDirection::Ltr);
            let resolvers: Vec<Rc<dyn VariableResolver>> = vec![
                model_based_variable_resolver.clone(),
                Rc::new(ClipboardBasedVariableResolver::new(
                    read_clipboard_text.clone(),
                    idx,
                    indexed_snippet_edits.len(),
                    clipboard_spread,
                )),
                Rc::new(SelectionBasedVariableResolver::new(model.clone(), edit_selection, idx, options.overtyping_capturer.clone())),
                Rc::new(CommentBasedVariableResolver::new(
                    model.clone(),
                    edit_selection,
                    services.language_configuration_service.clone(),
                )),
                time_based_variable_resolver.clone(),
                workspace_based_variable_resolver.clone(),
                random_based_variable_resolver.clone(),
            ];
            let edit_resolver = CompositeSnippetVariableResolver::new(resolvers);

            snippet.walk(|marker| {
                if marker.as_variable().is_some() && !pre_existing_variables.contains(&marker_id(marker)) {
                    marker.resolve_variable(&edit_resolver);
                }
                true
            });

            let snippet_text = snippet.to_js_string();
            let snippet_fragment_text = JsString::from(snippet_text.units().get(offset..).unwrap_or_default());
            offset = snippet_text.len();

            // make edit
            edits.push(tracked_edit(edit.range, snippet_fragment_text, i));
        }

        parser.ensure_final_tabstop(&snippet, enforce_final_tabstop, true);

        EditsAndSnippets {
            edits,
            snippets: vec![Rc::new(OneSnippet::new(editor.clone(), snippet, JsString::new()))],
        }
    }

    pub fn new(
        editor: Rc<CodeEditorWidget>,
        template: impl Into<SnippetTemplate>,
        options: Option<SnippetSessionInsertOptions>,
        services: Rc<SnippetServices>,
    ) -> Self {
        Self {
            editor,
            template: template.into(),
            options: options.unwrap_or_default(),
            services,
            template_merges: RefCell::new(Vec::new()),
            snippets: RefCell::new(Vec::new()),
        }
    }

    /// `_logInfo()`: o template e os `merge`s, no texto que a interpolação do JavaScript produz.
    pub fn log_info(&self) -> String {
        let template = match &self.template {
            SnippetTemplate::Text(template) => template.to_string_lossy(),
            SnippetTemplate::Edits(edits) => vec!["[object Object]"; edits.len()].join(","),
        };
        let merges: Vec<String> = self
            .template_merges
            .borrow()
            .iter()
            .map(|(nesting_level, groups_idx, template)| {
                format!("{},{},{}", number_to_js_string(*nesting_level), groups_idx, template.to_string_lossy())
            })
            .collect();
        format!("template=\"{}\", merged_templates=\"{}\"", template, merges.join(" -> "))
    }

    /// O primeiro snippet (`this._snippets[0]`).
    fn primary(&self) -> Rc<OneSnippet> {
        self.snippets.borrow()[0].clone()
    }

    pub fn insert(&self, edit_reason: Option<TextModelEditSource>) {
        if !self.editor.has_model() {
            return;
        }

        // make insert edit and start with first selections
        let EditsAndSnippets { edits, snippets } = match &self.template {
            SnippetTemplate::Text(template) => {
                Self::create_edits_and_snippets_from_selections(&self.editor, template, false, &self.options, &self.services)
            }
            SnippetTemplate::Edits(snippet_edits) => {
                Self::create_edits_and_snippets_from_edits(&self.editor, snippet_edits, false, &self.options, &self.services)
            }
        };

        *self.snippets.borrow_mut() = snippets.clone();

        let mut compute_end_cursor_state = |undo_edits: &[ValidEditOperation]| -> Option<Vec<Selection>> {
            let undo_edits = own_undo_edits(undo_edits);
            for (idx, snippet) in snippets.iter().enumerate() {
                snippet.initialize(&undo_edits[idx].text_change);
            }

            if self.primary().has_placeholder() {
                Some(self.move_selections(Some(true)))
            } else {
                Some(end_positions_as_selections(&undo_edits))
            }
        };
        self.editor.execute_edits(
            ExecuteEditsSource::Reason(edit_reason.unwrap_or_else(snippet_edit_source)),
            edits,
            Some(EndCursorState::Computer(&mut compute_end_cursor_state)),
        );
        if let Some(selections) = self.editor.get_selections() {
            self.editor.reveal_range(*selections[0], None, None, None);
        }
    }

    pub fn merge(&self, template: impl Into<JsString>, options: Option<&SnippetSessionInsertOptions>) {
        if !self.editor.has_model() {
            return;
        }
        let template = template.into();
        let default_options = SnippetSessionInsertOptions::default();
        let options = options.unwrap_or(&default_options);

        let primary = self.primary();
        self.template_merges
            .borrow_mut()
            .push((primary.nesting_level.get(), primary.placeholder_groups_idx.get(), template.clone()));
        let EditsAndSnippets { edits, snippets: mut nested_snippets } =
            Self::create_edits_and_snippets_from_selections(&self.editor, &template, true, options, &self.services);

        let mut compute_end_cursor_state = |undo_edits: &[ValidEditOperation]| -> Option<Vec<Selection>> {
            let undo_edits = own_undo_edits(undo_edits);
            for (idx, snippet) in nested_snippets.iter().enumerate() {
                snippet.initialize(&undo_edits[idx].text_change);
            }

            // Trivial snippets have no placeholder or are just the final placeholder. That means they
            // are just text insertions and we don't need to merge the nested snippet into the existing
            // snippet
            let is_trivial_snippet = nested_snippets[0].is_trivial_snippet();
            // Only merge when each active placeholder occurrence has a matching nested snippet.
            // Cursor normalization or external selection changes can collapse selections, leaving
            // fewer nested snippets than placeholder occurrences and previously crashing the merge.
            let current_snippets = self.snippets.borrow().clone();
            let can_merge_snippets = nested_snippets.len()
                == current_snippets.iter().map(|snippet| snippet.active_placeholder_count()).sum::<usize>();
            if !is_trivial_snippet && can_merge_snippets {
                for snippet in &current_snippets {
                    snippet.merge(&mut nested_snippets);
                }
                debug_assert!(nested_snippets.is_empty());
            }

            if self.primary().has_placeholder() && !is_trivial_snippet && can_merge_snippets {
                Some(self.move_selections(None))
            } else {
                Some(end_positions_as_selections(&undo_edits))
            }
        };
        self.editor.execute_edits(
            ExecuteEditsSource::Name(Some("snippet")),
            edits,
            Some(EndCursorState::Computer(&mut compute_end_cursor_state)),
        );
    }

    /// `next()` e `prev()`: vai para o próximo (ou o anterior) grupo de placeholders e seleciona.
    fn go(&self, fwd: bool) {
        let new_selections = self.move_selections(Some(fwd));
        if !new_selections.is_empty() {
            self.editor.set_selections(&new_selections, None, None);
            self.editor.reveal_position_in_center_if_outside_viewport(new_selections[0].get_position(), None);
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&self) {
        self.go(true);
    }

    pub fn prev(&self) {
        self.go(false);
    }

    /// `_move(fwd)`.
    fn move_selections(&self, fwd: Option<bool>) -> Vec<Selection> {
        let snippets = self.snippets.borrow().clone();
        snippets.iter().flat_map(|snippet| snippet.move_to(fwd)).collect()
    }

    pub fn is_at_first_placeholder(&self) -> bool {
        self.primary().is_at_first_placeholder()
    }

    pub fn is_at_last_placeholder(&self) -> bool {
        self.primary().is_at_last_placeholder()
    }

    pub fn has_placeholder(&self) -> bool {
        self.primary().has_placeholder()
    }

    pub fn has_choice(&self) -> bool {
        self.primary().has_choice()
    }

    pub fn active_choice(&self) -> Option<ActiveChoice> {
        self.primary().active_choice()
    }

    pub fn is_selection_within_placeholders(&self) -> bool {
        if !self.has_placeholder() {
            return false;
        }

        let mut selections = self.editor.get_selections().unwrap_or_default();
        let snippets = self.snippets.borrow().clone();
        if selections.len() < snippets.len() {
            // this means we started snippet mode with N
            // selections and have M (N > M) selections.
            // So one snippet is without selection -> cancel
            return false;
        }

        let mut all_possible_selections: Vec<(f64, Vec<Range>)> = Vec::new();
        for snippet in &snippets {
            let possible_selections = snippet.compute_possible_selections();

            // for the first snippet find the placeholder (and its ranges)
            // that contain at least one selection. for all remaining snippets
            // the same placeholder (and their ranges) must be used.
            if all_possible_selections.is_empty() {
                for (index, ranges) in &possible_selections {
                    let mut ranges = ranges.clone();
                    ranges.sort_by(|a, b| compare_starts(a, b));
                    if let Some(first) = ranges.first()
                        && selections.iter().any(|selection| first.contains_range(selection))
                    {
                        all_possible_selections.push((*index, Vec::new()));
                    }
                }
            }

            if all_possible_selections.is_empty() {
                // return false if we couldn't associate a selection to
                // this (the first) snippet
                return false;
            }

            // add selections from 'this' snippet so that we know all
            // selections for this placeholder
            for (index, array) in all_possible_selections.iter_mut() {
                if let Some((_, ranges)) = possible_selections.iter().find(|(possible_index, _)| possible_index == index) {
                    array.extend(ranges.iter().copied());
                }
            }
        }

        // sort selections (and later placeholder-ranges). then walk both
        // arrays and make sure the placeholder-ranges contain the corresponding
        // selection
        selections.sort_by(|a, b| compare_starts(a, b));

        all_possible_selections.retain_mut(|(_, ranges)| {
            if ranges.len() != selections.len() {
                return false;
            }

            ranges.sort_by(|a, b| compare_starts(a, b));

            ranges.iter().zip(selections.iter()).all(|(range, selection)| range.contains_range(selection))
        });

        // from all possible selections we have deleted those
        // that don't match with the current selection. if we don't
        // have any left, we don't have a selection anymore
        !all_possible_selections.is_empty()
    }

    pub fn get_enclosing_range(&self) -> Option<Range> {
        let mut result: Option<Range> = None;
        for snippet in self.snippets.borrow().iter() {
            let snippet_range = snippet.get_enclosing_range();
            result = match (result, snippet_range) {
                (None, range) => range,
                (Some(current), Some(range)) => Some(current.plus_range(&range)),
                (Some(current), None) => Some(current),
            };
        }
        result
    }
}

impl IDisposable for SnippetSession {
    fn dispose(&self) {
        for snippet in self.snippets.borrow().iter() {
            snippet.dispose();
        }
    }
}

/// `prefix + text`.
fn prefixed(prefix: &JsString, text: &JsString) -> JsString {
    let mut out = prefix.clone();
    out.push_units(text.units());
    out
}
