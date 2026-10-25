//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/inlineSuggestionItem.ts`: os itens de sugestão em
//! linha (`InlineCompletionItem` e `InlineEditItem`), a identidade (`InlineSuggestionIdentity`), a dica
//! (`InlineSuggestHint`) e o rastreio das edições do usuário sobre a sugestão (`SingleUpdatedNextEdit`).
//!
//! Notas de tradução:
//! - `InlineSuggestionItem = InlineEditItem | InlineCompletionItem` é o enum [`InlineSuggestionItem`]; a classe
//!   abstrata `InlineSuggestionItemBase` é o trait [`InlineSuggestionItemBase`], cujos métodos abstratos são `base`,
//!   `action` e `compute_edit_kind` e cujos demais métodos têm corpo padrão (o `hash` é sobrescrito pelo
//!   `InlineCompletionItem`);
//! - `withEdit`, `withIdentity` e `canBeReused` são métodos próprios de cada item e do enum, porque o retorno do
//!   original é o tipo concreto de cada classe;
//! - `ITextModel` é o `Rc<TextModel>`; a referência ao modelo alvo é `Rc<TextModelValueReference>`;
//! - o texto é UTF-16 ([`JsString`]); `String.prototype.substring` é `je_base::js_string::substring`;
//! - o `hashInlineSuggestionAction` serializa a ação com `JSON.stringify`; a ordem das chaves segue a do objeto
//!   criado por `InlineCompletionItem.action` (`kind`, `textReplacement`, `snippetInfo`, `stringEdit`,
//!   `alternativeAction`, `target`), a mesma do objeto de `withEdit`; o objeto de `InlineEditItem.create` do original
//!   usa `snippetInfo, stringEdit, textReplacement`, e essa diferença de ordem de chaves não é reproduzida;
//! - `Date.now()` e os tempos de exibição moram em [`InlineSuggestData`].

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::js_string::substring;
use je_base::observable::base::{ITransaction, Observable};
use je_base::observable::observables::observable_signal::ObservableSignal;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, SettableExt, observable_signal, observable_value};
use je_base::strings::{common_prefix_length, common_suffix_length, split_lines};
use je_platform::commands::commands::ICommandService;

use crate::code_action::languages::Command;
use crate::diff::default_lines_diff_computer::default_lines_diff_computer::DefaultLinesDiffComputer;
use crate::diff::lines_diff_computer::{ILinesDiffComputer, LinesDiffComputerOptions};
use crate::edit_operation::SingleEditOperation;
use crate::edits::edit::{BaseEdit, BaseReplacement};
use crate::edits::string_edit::{BaseStringReplacement, StringEdit, StringReplacement, apply_edits_to_ranges};
use crate::edits::text_edit::{TextEdit, TextReplacement};
use crate::inline_completions::edit_kind::{InlineSuggestionEditKind, compute_edit_kind};
use crate::inline_completions::inline_completion_is_visible::inline_completion_is_visible;
use crate::inline_completions::inline_completion_view_data::{InlineCompletionViewData, InlineCompletionViewKind};
use crate::inline_completions::inline_suggest_alternative_action::InlineSuggestAlternativeAction;
use crate::inline_completions::provide_inline_completions::{
    IInlineSuggestDataAction, IInlineSuggestDataActionEdit, InlineSuggestData, InlineSuggestionList, PartialAcceptance,
    RenameInfo, SnippetInfo,
};
use crate::inline_completions::text_model_value_reference::TextModelValueReference;
use crate::languages::{
    IInlineCompletionHint, InlineCompletion, InlineCompletionEndOfLifeReason, InlineCompletionHintStyle,
    InlineCompletionTriggerKind, InlineCompletionWarning, PartialAcceptInfo,
};
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::position_offset_transformer_with_text_model::get_position_offset_transformer_from_text_model;
use crate::position_to_offset::{PositionOffsetTransformerBase, TextLength};
use crate::range::Range;
use crate::text::abstract_text::{AbstractText, StringText};
use crate::text_model::TextModel;
use crate::text_model_text::TextModelText;

/// `InlineSuggestionItem = InlineEditItem | InlineCompletionItem`.
#[derive(Clone)]
pub enum InlineSuggestionItem {
    Edit(InlineEditItem),
    Completion(InlineCompletionItem),
}

impl InlineSuggestionItem {
    /// `InlineSuggestionItem.create(data, textModel, shouldDiffEdit = true)`.
    pub fn create(data: &Rc<InlineSuggestData>, text_model: &Rc<TextModelValueReference>, should_diff_edit: bool) -> InlineSuggestionItem {
        match data.action() {
            Some(IInlineSuggestDataAction::Edit(action)) if !data.is_inline_edit && action.uri.is_none() => {
                InlineSuggestionItem::Completion(InlineCompletionItem::create(data, text_model, &action))
            }
            _ => InlineSuggestionItem::Edit(InlineEditItem::create(data, text_model, should_diff_edit)),
        }
    }

    /// `isInlineEdit`.
    pub fn is_inline_edit(&self) -> bool {
        matches!(self, InlineSuggestionItem::Edit(_))
    }

    /// `withEdit(userEdit, textModel)`.
    pub fn with_edit(&self, user_edit: &StringEdit, text_model: &Rc<TextModel>) -> Option<InlineSuggestionItem> {
        match self {
            InlineSuggestionItem::Edit(item) => item.with_edit(user_edit, text_model).map(InlineSuggestionItem::Edit),
            InlineSuggestionItem::Completion(item) => item.with_edit(user_edit, text_model).map(InlineSuggestionItem::Completion),
        }
    }

    /// `withIdentity(identity)`.
    pub fn with_identity(&self, identity: Rc<InlineSuggestionIdentity>) -> InlineSuggestionItem {
        match self {
            InlineSuggestionItem::Edit(item) => InlineSuggestionItem::Edit(item.with_identity(identity)),
            InlineSuggestionItem::Completion(item) => InlineSuggestionItem::Completion(item.with_identity(identity)),
        }
    }

    /// `canBeReused(model, position)`.
    pub fn can_be_reused(&self, model: &TextModel, position: Position) -> bool {
        match self {
            InlineSuggestionItem::Edit(item) => item.can_be_reused(model, position),
            InlineSuggestionItem::Completion(item) => item.can_be_reused(model, position),
        }
    }
}

impl InlineSuggestionItemBase for InlineSuggestionItem {
    fn base(&self) -> &ItemBase {
        match self {
            InlineSuggestionItem::Edit(item) => item.base(),
            InlineSuggestionItem::Completion(item) => item.base(),
        }
    }

    fn action(&self) -> Option<InlineSuggestionAction> {
        match self {
            InlineSuggestionItem::Edit(item) => item.action(),
            InlineSuggestionItem::Completion(item) => item.action(),
        }
    }

    fn hash(&self) -> String {
        match self {
            InlineSuggestionItem::Edit(item) => item.hash(),
            InlineSuggestionItem::Completion(item) => item.hash(),
        }
    }

    fn compute_edit_kind(&self, model: &TextModel) -> Option<InlineSuggestionEditKind> {
        match self {
            InlineSuggestionItem::Edit(item) => item.compute_edit_kind(model),
            InlineSuggestionItem::Completion(item) => item.compute_edit_kind(model),
        }
    }
}

/// `InlineSuggestionAction`.
#[derive(Clone)]
pub enum InlineSuggestionAction {
    Edit(IInlineSuggestionActionEdit),
    JumpTo(IInlineSuggestionActionJumpTo),
}

/// `IInlineSuggestionActionEdit`.
#[derive(Clone)]
pub struct IInlineSuggestionActionEdit {
    pub text_replacement: TextReplacement,
    pub snippet_info: Option<SnippetInfo>,
    pub string_edit: StringEdit,
    pub target: Rc<TextModelValueReference>,
    pub alternative_action: Option<InlineSuggestAlternativeAction>,
}

/// `IInlineSuggestionActionJumpTo`.
#[derive(Clone)]
pub struct IInlineSuggestionActionJumpTo {
    pub position: Position,
    pub offset: i32,
    pub target: Rc<TextModelValueReference>,
}

/// `JSON.stringify(text)` de um texto UTF-16 (surrogates isolados saem como `\udxxx`).
fn json_stringify(units: &[u16]) -> String {
    let mut out = String::from("\"");
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok('\t') => out.push_str("\\t"),
            Ok(c) if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            Ok(c) => out.push(c),
            Err(lone) => out.push_str(&format!("\\u{:04x}", lone.unpaired_surrogate())),
        }
    }
    out.push('"');
    out
}

/// O `JSON.stringify` de um `Range`.
fn range_to_json(range: &Range) -> String {
    format!(
        "{{\"startLineNumber\":{},\"startColumn\":{},\"endLineNumber\":{},\"endColumn\":{}}}",
        range.start_line_number, range.start_column, range.end_line_number, range.end_column
    )
}

/// O `JSON.stringify` de uma `StringReplacement` (os campos do objeto: `replaceRange` e `newText`).
fn string_replacement_to_json(replacement: &StringReplacement) -> String {
    format!(
        "{{\"replaceRange\":{{\"start\":{},\"endExclusive\":{}}},\"newText\":{}}}",
        replacement.replace_range.start,
        replacement.replace_range.end_exclusive,
        json_stringify(replacement.new_text.units())
    )
}

/// `hashInlineSuggestionAction(action)`.
fn hash_inline_suggestion_action(action: Option<&InlineSuggestionAction>) -> String {
    match action {
        Some(InlineSuggestionAction::Edit(edit)) => {
            let mut fields = vec![
                "\"kind\":\"edit\"".to_string(),
                format!(
                    "\"textReplacement\":{{\"range\":{},\"text\":{}}}",
                    range_to_json(&edit.text_replacement.range),
                    json_stringify(edit.text_replacement.text.units())
                ),
            ];
            if let Some(snippet_info) = &edit.snippet_info {
                fields.push(format!(
                    "\"snippetInfo\":{{\"snippet\":{},\"range\":{}}}",
                    json_stringify(snippet_info.snippet.units()),
                    range_to_json(&snippet_info.range)
                ));
            }
            let replacements: Vec<String> = edit.string_edit.replacements().iter().map(string_replacement_to_json).collect();
            fields.push(format!("\"stringEdit\":{{\"replacements\":[{}]}}", replacements.join(",")));
            if let Some(alternative) = InlineSuggestAlternativeAction::to_display_string(edit.alternative_action.as_ref()) {
                fields.push(format!("\"alternativeAction\":{}", json_stringify(&alternative.encode_utf16().collect::<Vec<u16>>())));
            }
            fields.push(format!("\"target\":{}", json_stringify(edit.target.uri().to_string(false).units())));
            format!("{{{}}}", fields.join(","))
        }
        Some(InlineSuggestionAction::JumpTo(jump_to)) => format!(
            "{{\"kind\":\"jumpTo\",\"position\":{{\"lineNumber\":{},\"column\":{}}},\"offset\":{},\"target\":{}}}",
            jump_to.position.line_number,
            jump_to.position.column,
            jump_to.offset,
            json_stringify(jump_to.target.uri().to_string(false).units())
        ),
        None => "{}".to_string(),
    }
}

/// Os campos da classe abstrata `InlineSuggestionItemBase`.
#[derive(Clone)]
pub struct ItemBase {
    data: Rc<InlineSuggestData>,
    pub identity: Rc<InlineSuggestionIdentity>,
    pub hint: Option<InlineSuggestHint>,
    /// Reference to the text model this item targets.
    /// For cross-file edits, this may differ from the current editor's model.
    pub original_text_ref: Rc<TextModelValueReference>,
}

/// `InlineSuggestionItemBase`.
pub trait InlineSuggestionItemBase {
    /// Os campos comuns.
    fn base(&self) -> &ItemBase;

    /// `action`.
    fn action(&self) -> Option<InlineSuggestionAction>;

    /// `computeEditKind(model)`.
    fn compute_edit_kind(&self, model: &TextModel) -> Option<InlineSuggestionEditKind>;

    /// `hash`.
    fn hash(&self) -> String {
        hash_inline_suggestion_action(self.action().as_ref())
    }

    /// A reference to the original inline completion list this inline completion has been constructed from.
    /// Used for event data to ensure referential equality.
    fn source(&self) -> Rc<InlineSuggestionList> {
        Rc::clone(&self.base().data.source)
    }

    fn is_from_explicit_request(&self) -> bool {
        self.base().data.context.trigger_kind == InlineCompletionTriggerKind::Explicit
    }

    fn forward_stable(&self) -> bool {
        self.base().data.source.inline_suggestions.enable_forward_stability.unwrap_or(false)
    }

    /// `targetRange`.
    fn target_range(&self) -> Range {
        if let Some(hint) = &self.base().hint {
            return hint.range;
        }
        match self.action() {
            Some(InlineSuggestionAction::Edit(edit)) => edit.text_replacement.range,
            Some(InlineSuggestionAction::JumpTo(jump_to)) => Range::from_positions(jump_to.position, jump_to.position),
            None => errors::throw(Error::bug_indicating(Some("InlineSuggestionItem: Either hint or action must be set"))),
        }
    }

    fn semantic_id(&self) -> String {
        self.hash()
    }

    fn gutter_menu_link_action(&self) -> Option<Command> {
        self.base().data.source_inline_completion.gutter_menu_link_action.clone()
    }

    fn command(&self) -> Option<Command> {
        self.base().data.source_inline_completion.command.clone()
    }

    fn supports_rename(&self) -> bool {
        self.base().data.supports_rename
    }

    fn warning(&self) -> Option<InlineCompletionWarning> {
        self.base().data.source_inline_completion.warning.clone()
    }

    fn show_inline_edit_menu(&self) -> bool {
        self.base().data.source_inline_completion.show_inline_edit_menu.unwrap_or(false)
    }

    /// `shownCommand` (obsoleto).
    fn shown_command(&self) -> Option<Command> {
        self.base().data.source_inline_completion.shown_command.clone()
    }

    fn request_uuid(&self) -> String {
        self.base().data.context.request_uuid.clone()
    }

    fn partial_accepts(&self) -> PartialAcceptance {
        self.base().data.partial_accepts()
    }

    fn add_ref(&self) {
        self.base().identity.add_ref();
        self.base().data.source.add_ref();
    }

    fn remove_ref(&self) {
        self.base().identity.remove_ref();
        self.base().data.source.remove_ref(None);
    }

    fn report_inline_edit_shown(
        &self,
        command_service: &dyn ICommandService,
        view_kind: InlineCompletionViewKind,
        view_data: InlineCompletionViewData,
        model: &TextModel,
        time_when_shown: f64,
    ) -> je_base::promise::Promise<()> {
        let insert_text = match self.action() {
            Some(InlineSuggestionAction::Edit(edit)) => edit.text_replacement.text,
            _ => JsString::new(), // TODO@hediet support insertText === undefined
        };
        self.base().data.report_inline_edit_shown(
            command_service,
            &insert_text,
            view_kind,
            view_data,
            self.compute_edit_kind(model),
            time_when_shown,
        )
    }

    fn report_partial_accept(&self, accepted_characters: i32, info: PartialAcceptInfo, partial_acceptance: PartialAcceptance) {
        self.base().data.report_partial_accept(accepted_characters, info, partial_acceptance);
    }

    fn report_end_of_life(&self, reason: InlineCompletionEndOfLifeReason) {
        self.base().data.report_end_of_life(Some(reason));
    }

    fn set_end_of_life_reason(&self, reason: InlineCompletionEndOfLifeReason) {
        self.base().data.set_end_of_life_reason(reason);
    }

    fn set_is_preceeded(&self, item: &InlineSuggestionItem) {
        self.base().data.set_is_preceeded(item.partial_accepts());
    }

    fn set_not_shown_reason_if_not_set(&self, reason: &str) {
        self.base().data.set_not_shown_reason(reason);
    }

    /// Avoid using this method. Instead introduce getters for the needed properties.
    fn get_source_completion(&self) -> Rc<InlineCompletion> {
        Rc::clone(&self.base().data.source_inline_completion)
    }

    fn set_rename_processing_info(&self, info: RenameInfo) {
        self.base().data.set_rename_processing_info(info);
    }

    fn with_action(&self, action: IInlineSuggestDataAction) -> Rc<InlineSuggestData> {
        self.base().data.with_action(action)
    }

    fn add_performance_marker(&self, marker: &str) {
        self.base().data.add_performance_marker(marker);
    }
}

thread_local! {
    /// `InlineSuggestionIdentity.idCounter`.
    static IDENTITY_COUNTER: Cell<u32> = const { Cell::new(0) };
}

/// `InlineSuggestionIdentity`.
pub struct InlineSuggestionIdentity {
    on_dispose_signal: Rc<ObservableSignal<()>>,
    jumped_to_value: Rc<ObservableValue<bool, ()>>,
    ref_count: Cell<i32>,
    pub id: String,
}

impl InlineSuggestionIdentity {
    pub fn new() -> Rc<Self> {
        let counter = IDENTITY_COUNTER.with(|counter| {
            let current = counter.get();
            counter.set(current + 1);
            current
        });
        Rc::new(Self {
            on_dispose_signal: observable_signal("onDispose"),
            jumped_to_value: observable_value("jumpedTo", false),
            ref_count: Cell::new(0),
            id: format!("InlineCompletionIdentity{counter}"),
        })
    }

    /// `onDispose`.
    pub fn on_dispose(&self) -> IObservable<()> {
        self.on_dispose_signal.to_observable()
    }

    /// `jumpedTo`.
    pub fn jumped_to(&self) -> IObservable<bool> {
        self.jumped_to_value.to_observable()
    }

    pub fn add_ref(&self) {
        self.ref_count.set(self.ref_count.get() + 1);
    }

    pub fn remove_ref(&self) {
        self.ref_count.set(self.ref_count.get() - 1);
        if self.ref_count.get() == 0 {
            self.on_dispose_signal.trigger(None, ());
        }
    }

    /// `setJumpTo(tx)`.
    pub fn set_jump_to(&self, tx: Option<&dyn ITransaction>) {
        self.jumped_to_value.set(true, tx);
    }
}

/// `InlineSuggestHint`.
#[derive(Clone, Debug)]
pub struct InlineSuggestHint {
    pub range: Range,
    pub content: String,
    pub style: InlineCompletionHintStyle,
}

impl InlineSuggestHint {
    pub fn create(hint: &IInlineCompletionHint) -> InlineSuggestHint {
        InlineSuggestHint { range: hint.range, content: hint.content.clone(), style: hint.style }
    }

    /// `withEdit(edit, positionOffsetTransformer)`.
    pub fn with_edit(&self, edit: &StringEdit, position_offset_transformer: &dyn PositionOffsetTransformerBase) -> Option<InlineSuggestHint> {
        let offset_range = OffsetRange::new(
            position_offset_transformer.get_offset(self.range.get_start_position()),
            position_offset_transformer.get_offset(self.range.get_end_position()),
        );

        let new_offset_range = apply_edits_to_ranges(&[offset_range], edit).first().copied()?;
        let new_range = position_offset_transformer.get_range(new_offset_range);

        Some(InlineSuggestHint { range: new_range, content: self.content.clone(), style: self.style })
    }
}

/// `string.replace(/\r\n|\r|\n/g, eol)`.
fn replace_line_breaks(text: &[u16], eol: &[u16]) -> JsString {
    let mut out: Vec<u16> = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            13 if text.get(i + 1) == Some(&10) => {
                out.extend_from_slice(eol);
                i += 2;
            }
            10 | 13 => {
                out.extend_from_slice(eol);
                i += 1;
            }
            unit => {
                out.push(unit);
                i += 1;
            }
        }
    }
    JsString::from_units(out)
}

/// `haystack.includes(needle)` sobre unidades UTF-16.
fn includes_units(haystack: &[u16], needle: &[u16]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|window| window == needle)
}

/// `InlineCompletionItem`.
#[derive(Clone)]
pub struct InlineCompletionItem {
    base: ItemBase,
    edit: StringReplacement,
    trimmed_edit: StringReplacement,
    text_edit: TextReplacement,
    original_range: Range,
    pub snippet_info: Option<SnippetInfo>,
    pub additional_text_edits: Vec<SingleEditOperation>,
}

impl InlineCompletionItem {
    pub fn create(data: &Rc<InlineSuggestData>, text_model: &Rc<TextModelValueReference>, action: &IInlineSuggestDataActionEdit) -> InlineCompletionItem {
        let identity = InlineSuggestionIdentity::new();
        let transformer = text_model.get_transformer();

        let insert_text = replace_line_breaks(action.insert_text.units(), text_model.get_eol().as_units());

        let edit = reshape_inline_completion(StringReplacement::new(transformer.get_offset_range(&action.range), insert_text), text_model);
        let trimmed_edit = edit.remove_common_suffix_and_prefix(text_model.get_value().units());
        let text_edit = transformer.get_text_replacement(&edit);

        let display_location = data.hint.as_ref().map(InlineSuggestHint::create);

        let original_range = text_edit.range;
        InlineCompletionItem {
            base: ItemBase { data: Rc::clone(data), identity, hint: display_location, original_text_ref: Rc::clone(text_model) },
            edit,
            trimmed_edit,
            text_edit,
            original_range,
            snippet_info: action.snippet_info.clone(),
            additional_text_edits: data.additional_text_edits.clone(),
        }
    }

    pub const IS_INLINE_EDIT: bool = false;

    pub fn get_single_text_edit(&self) -> TextReplacement {
        self.text_edit.clone()
    }

    /// `withIdentity(identity)`.
    pub fn with_identity(&self, identity: Rc<InlineSuggestionIdentity>) -> InlineCompletionItem {
        let mut item = self.clone();
        item.base.identity = identity;
        item
    }

    /// `withEdit(textModelEdit, textModel)`.
    pub fn with_edit(&self, text_model_edit: &StringEdit, text_model: &Rc<TextModel>) -> Option<InlineCompletionItem> {
        // If the edit is to a different model than our target, it's a noop
        if !self.base.original_text_ref.targets(text_model) {
            return Some(self.clone()); // unchanged
        }

        let new_edit_range = apply_edits_to_ranges(&[self.edit.replace_range], text_model_edit);
        let new_edit = StringReplacement::new(*new_edit_range.first()?, self.text_edit.text.clone());
        let position_offset_transformer = get_position_offset_transformer_from_text_model(text_model);
        let new_text_edit = position_offset_transformer.get_text_replacement(&new_edit);

        let mut new_display_location = self.base.hint.clone();
        if let Some(display_location) = &new_display_location {
            new_display_location = Some(display_location.with_edit(text_model_edit, &position_offset_transformer)?);
        }

        let trimmed_edit = new_edit.remove_common_suffix_and_prefix(text_model.get_value().units());

        let mut item = self.clone();
        item.edit = new_edit;
        item.trimmed_edit = trimmed_edit;
        item.text_edit = new_text_edit;
        item.base.hint = new_display_location;
        Some(item)
    }

    /// `canBeReused(model, position)`.
    pub fn can_be_reused(&self, model: &TextModel, position: Position) -> bool {
        // TODO@hediet I believe this can be simplified to `return true;`, as applying an edit should kick out this suggestion.
        let updated_range = self.text_edit.range;
        updated_range.contains_position(&position)
            && self.is_visible(model, position)
            && TextLength::of_range(&updated_range).is_greater_than_or_equal_to(&TextLength::of_range(&self.original_range))
    }

    pub fn is_visible(&self, model: &TextModel, cursor_position: Position) -> bool {
        let single_text_edit = self.get_single_text_edit();
        inline_completion_is_visible(&single_text_edit, Some(&self.original_range), model, cursor_position)
    }

    pub fn edit_range(&self) -> Range {
        self.text_edit.range
    }

    pub fn insert_text(&self) -> JsString {
        self.text_edit.text.clone()
    }
}

impl InlineSuggestionItemBase for InlineCompletionItem {
    fn base(&self) -> &ItemBase {
        &self.base
    }

    fn action(&self) -> Option<InlineSuggestionAction> {
        Some(InlineSuggestionAction::Edit(IInlineSuggestionActionEdit {
            text_replacement: self.get_single_text_edit(),
            snippet_info: self.snippet_info.clone(),
            string_edit: StringEdit::new(vec![self.trimmed_edit.clone()]),
            alternative_action: None,
            target: Rc::clone(&self.base.original_text_ref),
        }))
    }

    /// `JSON.stringify(this._trimmedEdit.toJson())`.
    fn hash(&self) -> String {
        let json = self.trimmed_edit.to_json();
        format!("{{\"txt\":{},\"pos\":{},\"len\":{}}}", json_stringify(json.txt.units()), json.pos, json.len)
    }

    fn compute_edit_kind(&self, model: &TextModel) -> Option<InlineSuggestionEditKind> {
        compute_edit_kind(&StringEdit::new(vec![self.edit.clone()]), model, None)
    }
}

/// `InlineEditItem`.
#[derive(Clone)]
pub struct InlineEditItem {
    base: ItemBase,
    action: Option<InlineSuggestionAction>,
    edits: Vec<SingleUpdatedNextEdit>,
    last_change_part_of_inline_edit: bool,
    inline_edit_model_version: i64,
}

impl InlineEditItem {
    pub const IS_INLINE_EDIT: bool = true;

    pub fn create_for_test(text_model: &Rc<TextModelValueReference>, range: Range, new_text: &str) -> InlineEditItem {
        let action = IInlineSuggestDataAction::Edit(IInlineSuggestDataActionEdit {
            snippet_info: None,
            insert_text: JsString::from(new_text),
            range,
            uri: Some(text_model.uri()),
            alternative_action: None,
        });

        InlineEditItem::create(&InlineSuggestData::create_for_test(Some(action), &text_model.uri()), text_model, true)
    }

    pub fn create(data: &Rc<InlineSuggestData>, text_model: &Rc<TextModelValueReference>, should_diff_edit: bool) -> InlineEditItem {
        let mut edits: Vec<SingleUpdatedNextEdit> = Vec::new();
        let action = match data.action() {
            Some(IInlineSuggestDataAction::Edit(edit_action)) => {
                // TODO compute async
                let offset_edit = if should_diff_edit {
                    get_diffed_string_edit(text_model, &edit_action.range, &edit_action.insert_text)
                } else {
                    get_string_edit(text_model, &edit_action.range, &edit_action.insert_text)
                };
                let text_edit = TextEdit::from_string_edit(&offset_edit, &**text_model);
                // FIXME: .toReplacement() can throw because offsetEdit is empty because we get an empty diff in getStringEdit after diffing
                let single_text_edit = if offset_edit.is_empty() {
                    TextReplacement::new(Range::new(1, 1, 1, 1), "")
                } else {
                    text_edit.to_replacement(&**text_model)
                };

                edits = offset_edit
                    .replacements()
                    .iter()
                    .map(|edit| {
                        let replaced_range = Range::from_positions(
                            text_model.get_position_at(i64::from(edit.replace_range.start)),
                            text_model.get_transformer().get_position(edit.replace_range.end_exclusive),
                        );
                        let replaced_text = text_model.get_value_in_range(&replaced_range);
                        SingleUpdatedNextEdit::create(edit.clone(), &replaced_text)
                    })
                    .collect();

                Some(InlineSuggestionAction::Edit(IInlineSuggestionActionEdit {
                    snippet_info: edit_action.snippet_info.clone(),
                    string_edit: offset_edit,
                    text_replacement: single_text_edit,
                    alternative_action: edit_action.alternative_action.clone(),
                    target: Rc::clone(text_model),
                }))
            }
            Some(IInlineSuggestDataAction::JumpTo(jump_to)) => Some(InlineSuggestionAction::JumpTo(IInlineSuggestionActionJumpTo {
                position: jump_to.position,
                offset: text_model.get_transformer().get_offset(jump_to.position),
                target: Rc::clone(text_model),
            })),
            None => {
                if data.hint.is_none() {
                    errors::throw(Error::bug_indicating(Some("InlineEditItem: action is undefined and no hint is provided")));
                }
                None
            }
        };

        let identity = InlineSuggestionIdentity::new();

        let hint = data.hint.as_ref().map(InlineSuggestHint::create);
        InlineEditItem {
            base: ItemBase { data: Rc::clone(data), identity, hint, original_text_ref: Rc::clone(text_model) },
            action,
            edits,
            last_change_part_of_inline_edit: false,
            inline_edit_model_version: text_model.get_version_id(),
        }
    }

    pub fn snippet_info(&self) -> Option<SnippetInfo> {
        None
    }

    pub fn additional_text_edits(&self) -> Vec<SingleEditOperation> {
        Vec::new()
    }

    pub fn updated_edit_model_version(&self) -> i64 {
        self.inline_edit_model_version
    }

    /// `withIdentity(identity)`.
    pub fn with_identity(&self, identity: Rc<InlineSuggestionIdentity>) -> InlineEditItem {
        let mut item = self.clone();
        item.base.identity = identity;
        item
    }

    /// `canBeReused(model, position)`.
    pub fn can_be_reused(&self, model: &TextModel, _position: Position) -> bool {
        // TODO@hediet I believe this can be simplified to `return true;`, as applying an edit should kick out this suggestion.
        self.last_change_part_of_inline_edit && self.updated_edit_model_version() == model.get_version_id()
    }

    /// `withEdit(textModelChanges, textModel)`.
    pub fn with_edit(&self, text_model_changes: &StringEdit, text_model: &Rc<TextModel>) -> Option<InlineEditItem> {
        // If the edit is to a different model than our target, it's a noop
        if !self.base.original_text_ref.targets(text_model) {
            return Some(self.clone()); // unchanged
        }

        self.apply_text_model_changes(text_model_changes, &self.edits, text_model)
    }

    fn apply_text_model_changes(&self, text_model_changes: &StringEdit, edits: &[SingleUpdatedNextEdit], text_model: &Rc<TextModel>) -> Option<InlineEditItem> {
        let position_offset_transformer = get_position_offset_transformer_from_text_model(text_model);

        let mut edits: Vec<SingleUpdatedNextEdit> = edits.to_vec();
        let mut last_change_part_of_inline_edit = false;
        let mut inline_edit_model_version = self.inline_edit_model_version;

        let updated_target = TextModelValueReference::snapshot(text_model);

        let new_action = match &self.action {
            Some(InlineSuggestionAction::Edit(edit_action)) => {
                // TODO What about rename?
                edits = edits.iter().map(|inner_edit| inner_edit.apply_text_model_changes(text_model_changes)).collect();

                if edits.iter().any(|edit| edit.edit().is_none()) {
                    return None; // change is invalid, so we will have to drop the completion
                }

                let new_text_model_version = text_model.get_version_id();
                last_change_part_of_inline_edit = edits.iter().any(SingleUpdatedNextEdit::last_change_updated_edit);
                if last_change_part_of_inline_edit {
                    inline_edit_model_version = new_text_model_version;
                }

                if inline_edit_model_version + 20 < new_text_model_version {
                    return None; // the completion has been ignored for a while, remove it
                }

                edits.retain(|inner_edit| inner_edit.edit().is_some_and(|edit| !edit.is_empty()));
                if edits.is_empty() {
                    return None; // the completion has been typed by the user
                }

                let new_edit = StringEdit::new(edits.iter().filter_map(|edit| edit.edit().cloned()).collect());

                let new_text_edit =
                    position_offset_transformer.get_text_edit(&new_edit).to_replacement(&TextModelText::new(Rc::clone(text_model)));

                Some(InlineSuggestionAction::Edit(IInlineSuggestionActionEdit {
                    text_replacement: new_text_edit,
                    snippet_info: None,
                    string_edit: new_edit,
                    alternative_action: edit_action.alternative_action.clone(),
                    target: Rc::clone(&updated_target),
                }))
            }
            Some(InlineSuggestionAction::JumpTo(jump_to)) => {
                let new_jump_to_offset = text_model_changes.apply_to_offset_or_undefined(jump_to.offset)?;
                let new_jump_to_position = position_offset_transformer.get_position(new_jump_to_offset);

                Some(InlineSuggestionAction::JumpTo(IInlineSuggestionActionJumpTo {
                    position: new_jump_to_position,
                    offset: new_jump_to_offset,
                    target: Rc::clone(&updated_target),
                }))
            }
            None => None,
        };

        let mut new_display_location = self.base.hint.clone();
        if let Some(display_location) = &new_display_location {
            new_display_location = Some(display_location.with_edit(text_model_changes, &position_offset_transformer)?);
        }

        Some(InlineEditItem {
            base: ItemBase {
                data: Rc::clone(&self.base.data),
                identity: Rc::clone(&self.base.identity),
                hint: new_display_location,
                original_text_ref: updated_target,
            },
            action: new_action,
            edits,
            last_change_part_of_inline_edit,
            inline_edit_model_version,
        })
    }
}

impl InlineSuggestionItemBase for InlineEditItem {
    fn base(&self) -> &ItemBase {
        &self.base
    }

    fn action(&self) -> Option<InlineSuggestionAction> {
        self.action.clone()
    }

    fn compute_edit_kind(&self, model: &TextModel) -> Option<InlineSuggestionEditKind> {
        match &self.action {
            Some(InlineSuggestionAction::Edit(edit)) => compute_edit_kind(&edit.string_edit, model, None),
            _ => None,
        }
    }
}

fn get_diffed_string_edit(text_model: &Rc<TextModelValueReference>, edit_range: &Range, replace_text: &JsString) -> StringEdit {
    let eol = text_model.get_eol().as_units();
    let edit_original_text = text_model.get_value_of_range(edit_range);
    let edit_replace_text = replace_line_breaks(replace_text.units(), eol);

    let to_lines = |text: &JsString| -> Vec<JsString> { split_lines(text.units()).into_iter().map(JsString::from).collect() };
    let line_diffs = DefaultLinesDiffComputer::new().compute_diff(
        &to_lines(&edit_original_text),
        &to_lines(&edit_replace_text),
        &LinesDiffComputerOptions {
            ignore_trim_whitespace: false,
            compute_moves: false,
            extend_to_subwords: Some(true),
            max_computation_time_ms: 50.0,
        },
    );

    let inner_changes: Vec<_> = line_diffs.changes.iter().flat_map(|c| c.inner_changes.clone().unwrap_or_default()).collect();

    fn add_range_to_pos(pos: Position, range: &Range) -> Range {
        let start = TextLength::from_position(range.get_start_position());
        TextLength::of_range(range).create_range(start.add_to_position(pos))
    }

    let modified_text = StringText::new(edit_replace_text);

    StringEdit::new(
        inner_changes
            .iter()
            .map(|c| {
                let range_in_model = add_range_to_pos(edit_range.get_start_position(), &c.original_range);
                let original_range = text_model.get_transformer().get_offset_range(&range_in_model);

                let replace_text = modified_text.get_value_of_range(&c.modified_range);
                let edit = StringReplacement::new(original_range, replace_text);

                let original_text = text_model.get_value_of_range(&range_in_model);
                reshape_inline_edit(edit, &original_text, inner_changes.len(), text_model)
            })
            .collect(),
    )
}

fn get_string_edit(text_model: &Rc<TextModelValueReference>, edit_range: &Range, replace_text: &JsString) -> StringEdit {
    StringEdit::new(vec![StringReplacement::new(text_model.get_transformer().get_offset_range(edit_range), replace_text.clone())])
}

/// `SingleUpdatedNextEdit`.
#[derive(Clone)]
struct SingleUpdatedNextEdit {
    edit: Option<StringReplacement>,
    trimmed_new_text: JsString,
    prefix_length: i32,
    suffix_length: i32,
    last_change_updated_edit: bool,
}

impl SingleUpdatedNextEdit {
    fn create(edit: StringReplacement, replaced_text: &JsString) -> SingleUpdatedNextEdit {
        let prefix_length = common_prefix_length(edit.new_text.units(), replaced_text.units()) as i32;
        let suffix_length = common_suffix_length(edit.new_text.units(), replaced_text.units()) as i32;
        let trimmed_new_text = JsString::from(substring(
            edit.new_text.units(),
            i64::from(prefix_length),
            edit.new_text.len() as i64 - i64::from(suffix_length),
        ));
        SingleUpdatedNextEdit { edit: Some(edit), trimmed_new_text, prefix_length, suffix_length, last_change_updated_edit: false }
    }

    fn edit(&self) -> Option<&StringReplacement> {
        self.edit.as_ref()
    }

    fn last_change_updated_edit(&self) -> bool {
        self.last_change_updated_edit
    }

    fn apply_text_model_changes(&self, text_model_changes: &StringEdit) -> SingleUpdatedNextEdit {
        let mut clone = self.clone();
        clone.apply_changes_in_place(text_model_changes);
        clone
    }

    fn apply_changes_in_place(&mut self, text_model_changes: &StringEdit) {
        self.last_change_updated_edit = false; // TODO @benibenj make immutable

        let Some(edit) = self.edit.clone() else {
            errors::throw(Error::bug_indicating(Some("UpdatedInnerEdits: No edit to apply changes to")));
        };

        match self.apply_changes(&edit, text_model_changes) {
            None => self.edit = None,
            Some((new_edit, edit_has_changed)) => {
                self.edit = Some(new_edit);
                self.last_change_updated_edit = edit_has_changed;
            }
        }
    }

    fn apply_changes(&mut self, edit: &StringReplacement, text_model_changes: &StringEdit) -> Option<(StringReplacement, bool)> {
        let mut edit_start = edit.replace_range.start;
        let mut edit_end = edit.replace_range.end_exclusive;
        let mut edit_replace_text: Vec<u16> = edit.new_text.units().to_vec();
        let mut edit_has_changed = false;

        let should_preserve_edit_shape = self.prefix_length > 0 || self.suffix_length > 0;

        for change in text_model_changes.replacements().iter().rev() {
            let change_text = change.new_text.units();
            let change_len = change_text.len() as i32;

            // INSERTIONS (only support inserting at start of edit)
            let is_insertion = change_len > 0 && change.replace_range.is_empty();

            if is_insertion && !should_preserve_edit_shape && change.replace_range.start == edit_start && edit_replace_text.starts_with(change_text) {
                edit_start += change_len;
                edit_replace_text = edit_replace_text[change_text.len()..].to_vec();
                edit_end += change_len;
                edit_has_changed = true;
                continue;
            }

            if is_insertion
                && should_preserve_edit_shape
                && change.replace_range.start == edit_start + self.prefix_length
                && self.trimmed_new_text.units().starts_with(change_text)
            {
                edit_end += change_len;
                edit_has_changed = true;
                self.prefix_length += change_len;
                self.trimmed_new_text = JsString::from(&self.trimmed_new_text.units()[change_text.len()..]);
                continue;
            }

            // DELETIONS
            let is_deletion = change_len == 0 && change.replace_range.length() > 0;
            if is_deletion
                && change.replace_range.start >= edit_start + self.prefix_length
                && change.replace_range.end_exclusive <= edit_end - self.suffix_length
            {
                // user deleted text IN-BETWEEN the deletion range
                edit_end -= change.replace_range.length();
                edit_has_changed = true;
                continue;
            }

            // user did exactly the edit
            if change.equals(edit) {
                edit_has_changed = true;
                edit_start = change.replace_range.end_exclusive;
                edit_replace_text = Vec::new();
                continue;
            }

            // MOVE EDIT
            if change.replace_range.start > edit_end {
                // the change happens after the completion range
                continue;
            }
            if change.replace_range.end_exclusive < edit_start {
                // the change happens before the completion range
                edit_start += change_len - change.replace_range.length();
                edit_end += change_len - change.replace_range.length();
                continue;
            }

            // The change intersects the completion, so we will have to drop the completion
            return None;
        }

        // the resulting edit is a noop as the original and new text are the same
        if self.trimmed_new_text.len() == 0 && edit_start + self.prefix_length == edit_end - self.suffix_length {
            let offset = edit_start + self.prefix_length;
            return Some((StringReplacement::new(OffsetRange::new(offset, offset), ""), true));
        }

        Some((StringReplacement::new(OffsetRange::new(edit_start, edit_end), JsString::from_units(edit_replace_text)), edit_has_changed))
    }
}

fn reshape_inline_completion(mut edit: StringReplacement, text_model: &Rc<TextModelValueReference>) -> StringReplacement {
    // If the insertion is a multi line insertion starting on the next line
    // Move it forwards so that the multi line insertion starts on the current line
    let eol = text_model.get_eol().as_units();
    if edit.replace_range.is_empty() && includes_units(edit.new_text.units(), eol) {
        edit = reshape_multi_line_insertion(edit, text_model);
    }

    edit
}

fn reshape_inline_edit(
    mut edit: StringReplacement,
    original_text: &JsString,
    total_inner_edits: usize,
    text_model: &Rc<TextModelValueReference>,
) -> StringReplacement {
    // TODO: EOL are not properly trimmed by the diffAlgorithm #12680
    let eol = text_model.get_eol().as_units();
    if edit.new_text.units().ends_with(eol) && original_text.units().ends_with(eol) {
        let new_text_len = edit.new_text.len() - eol.len();
        edit = StringReplacement::new(edit.replace_range.delta_end(-(eol.len() as i32)), JsString::from(&edit.new_text.units()[..new_text_len]));
    }

    // INSERTION
    // If the insertion ends with a new line and is inserted at the start of a line which has text,
    // we move the insertion to the end of the previous line if possible
    if total_inner_edits == 1 && edit.replace_range.is_empty() && includes_units(edit.new_text.units(), eol) {
        let start_position = text_model.get_transformer().get_position(edit.replace_range.start);
        let has_text_on_insertion_line = text_model.get_line_length(start_position.line_number) != 0;
        if has_text_on_insertion_line {
            edit = reshape_multi_line_insertion(edit, text_model);
        }
    }

    // The diff algorithm extended a simple edit to the entire word
    // shrink it back to a simple edit if it is deletion/insertion only
    if total_inner_edits == 1 {
        let original = original_text.units();
        let new_text = edit.new_text.units();
        let prefix_length = common_prefix_length(original, new_text);
        let suffix_length = common_suffix_length(&original[prefix_length..], &new_text[prefix_length..]);
        let shrunk_range = edit.replace_range.delta_start(prefix_length as i32).delta_end(-(suffix_length as i32));

        // reshape it back to an insertion
        if prefix_length + suffix_length == original.len() {
            return StringReplacement::new(shrunk_range, JsString::from(&new_text[prefix_length..new_text.len() - suffix_length]));
        }

        // reshape it back to a deletion
        if prefix_length + suffix_length == new_text.len() {
            return StringReplacement::new(shrunk_range, "");
        }
    }

    edit
}

fn reshape_multi_line_insertion(edit: StringReplacement, text_model: &Rc<TextModelValueReference>) -> StringReplacement {
    if !edit.replace_range.is_empty() {
        errors::throw(Error::bug_indicating(Some("Unexpected original range")));
    }

    if edit.replace_range.start == 0 {
        return edit;
    }

    let eol = text_model.get_eol().as_units();
    let start_position = text_model.get_transformer().get_position(edit.replace_range.start);
    let start_column = start_position.column;
    let start_line_number = start_position.line_number;

    // If the insertion ends with a new line and is inserted at the start of a line which has text,
    // we move the insertion to the end of the previous line if possible
    let new_text = edit.new_text.units();
    if start_column == 1 && start_line_number > 1 && new_text.ends_with(eol) && !new_text.starts_with(eol) {
        let mut moved: Vec<u16> = eol.to_vec();
        moved.extend_from_slice(&new_text[..new_text.len() - eol.len()]);
        return StringReplacement::new(edit.replace_range.delta(-1), JsString::from_units(moved));
    }

    edit
}
