//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/editKind.ts`: a classificação de uma edição
//! sugerida (inserção, remoção ou troca) por forma do texto e lugar da edição.
//!
//! Notas de tradução:
//! - Texto é UTF-16 (`&[u16]`), como no JavaScript: `length` e colunas contam unidades UTF-16.
//! - As regexes `/[a-zA-Z0-9_]/`, `/[ \t]+/g`, `/\s+/`, `/^[a-zA-Z_][a-zA-Z0-9_]*$/` e `/\r\n|\r|\n/` viram código
//!   manual; `\s` usa `is_js_whitespace`, o mesmo conjunto da ECMAScript.
//! - As uniões do TypeScript viram enums: [`TextShape`] (`kind`) e [`EditProperties`] (`properties`, que no
//!   original é `InsertProperties | DeleteProperties | ReplaceProperties`).
//! - Campos `T | undefined` viram `Option<T>`; no `toString` (`JSON.stringify`) eles são omitidos, como o
//!   `JSON.stringify` omite `undefined`.
//! - `computeEditKind(edit, textModel, cursorPosition?)` é [`compute_edit_kind`], com `Option<Position>`.

use je_base::js_string::{self, is_js_whitespace};

use crate::edits::edit::BaseEdit;
use crate::edits::string_edit::{StringEdit, StringReplacement};
use crate::position::Position;
use crate::text_model::TextModel;

/// `isSyntacticalChar`.
fn is_syntactical_char(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(|c| {
        matches!(
            c,
            ';' | ','
                | '='
                | '+'
                | '-'
                | '*'
                | '/'
                | '{'
                | '}'
                | '('
                | ')'
                | '['
                | ']'
                | '<'
                | '>'
                | ':'
                | '.'
                | '!'
                | '?'
                | '&'
                | '|'
                | '^'
                | '%'
                | '@'
                | '#'
                | '~'
                | '`'
                | '\\'
                | '\''
                | '"'
                | '$'
        )
    })
}

/// `/[a-zA-Z0-9_]/.test(char)` para um caractere de uma unidade.
fn is_identifier_char(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `isWhitespaceChar`.
fn is_whitespace_char(unit: u16) -> bool {
    unit == 0x20 || unit == 0x09
}

/// `SingleCharacterKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleCharacterKind {
    Syntactical,
    Identifier,
    Whitespace,
}

impl SingleCharacterKind {
    fn as_str(self) -> &'static str {
        match self {
            SingleCharacterKind::Syntactical => "syntactical",
            SingleCharacterKind::Identifier => "identifier",
            SingleCharacterKind::Whitespace => "whitespace",
        }
    }
}

/// `SingleLineTextShape` (`kind: 'singleLine'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingleLineTextShape {
    pub is_single_character: bool,
    pub single_character_kind: Option<SingleCharacterKind>,
    pub is_word: bool,
    pub is_multiple_words: bool,
    pub is_multiple_whitespace: bool,
    pub has_duplicated_whitespace: bool,
}

/// `TextShape`: `SingleLineTextShape | MultiLineTextShape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextShape {
    /// `kind: 'singleLine'`.
    SingleLine(SingleLineTextShape),
    /// `kind: 'multiLine'`.
    MultiLine { line_count: i32 },
}

impl TextShape {
    /// O campo `kind` do original.
    pub fn kind(&self) -> &'static str {
        match self {
            TextShape::SingleLine(_) => "singleLine",
            TextShape::MultiLine { .. } => "multiLine",
        }
    }

    /// O `SingleLineTextShape`, quando `kind === 'singleLine'`.
    pub fn as_single_line(&self) -> Option<&SingleLineTextShape> {
        match self {
            TextShape::SingleLine(shape) => Some(shape),
            TextShape::MultiLine { .. } => None,
        }
    }
}

/// `text.split(/\r\n|\r|\n/).length`: o número de linhas do texto (sempre pelo menos 1).
fn split_line_count(text: &[u16]) -> i32 {
    let mut count = 1;
    let mut i = 0;
    while i < text.len() {
        if text[i] == 13 {
            count += 1;
            if text.get(i + 1) == Some(&10) {
                i += 1;
            }
        } else if text[i] == 10 {
            count += 1;
        }
        i += 1;
    }
    count
}

/// `/[ \t]+/g`: os comprimentos das sequências de espaços e tabs.
fn whitespace_runs(text: &[u16]) -> Vec<&[u16]> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if is_whitespace_char(text[i]) {
            let start = i;
            while i < text.len() && is_whitespace_char(text[i]) {
                i += 1;
            }
            runs.push(&text[start..i]);
        } else {
            i += 1;
        }
    }
    runs
}

/// `text.split(/\s+/).filter(w => w.length > 0)`.
fn split_words(text: &[u16]) -> Vec<&[u16]> {
    let mut words = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if is_js_whitespace(text[i]) {
            i += 1;
        } else {
            let start = i;
            while i < text.len() && !is_js_whitespace(text[i]) {
                i += 1;
            }
            words.push(&text[start..i]);
        }
    }
    words
}

/// `/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(word)`.
fn is_identifier_word(word: &[u16]) -> bool {
    let Some((&first, rest)) = word.split_first() else {
        return false;
    };
    let is_start = char::from_u32(u32::from(first)).is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    is_start && rest.iter().all(|&unit| is_identifier_char(unit))
}

/// `analyzeTextShape`.
fn analyze_text_shape(text: &[u16]) -> TextShape {
    let line_count = split_line_count(text);
    if line_count > 1 {
        return TextShape::MultiLine { line_count };
    }

    let is_single_char = text.len() == 1;
    let mut single_char_kind = None;
    if is_single_char {
        let unit = text[0];
        if is_syntactical_char(unit) {
            single_char_kind = Some(SingleCharacterKind::Syntactical);
        } else if is_identifier_char(unit) {
            single_char_kind = Some(SingleCharacterKind::Identifier);
        } else if is_whitespace_char(unit) {
            single_char_kind = Some(SingleCharacterKind::Whitespace);
        }
    }

    // Analyze whitespace patterns
    let whitespace_matches = whitespace_runs(text);
    let is_multiple_whitespace = whitespace_matches.iter().any(|ws| ws.len() > 1);
    let has_duplicated_whitespace = whitespace_matches
        .iter()
        .any(|ws| ws.windows(2).any(|pair| pair[0] == pair[1]));

    // Analyze word patterns
    let words = split_words(text);
    let is_word = words.len() == 1 && is_identifier_word(words[0]);
    let is_multiple_words = words.len() > 1;

    TextShape::SingleLine(SingleLineTextShape {
        is_single_character: is_single_char,
        single_character_kind: single_char_kind,
        is_word,
        is_multiple_words,
        is_multiple_whitespace,
        has_duplicated_whitespace,
    })
}

/// `InsertLocationShape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertLocationShape {
    EndOfLine,
    EmptyLine,
    StartOfLine,
    MiddleOfLine,
}

impl InsertLocationShape {
    fn as_str(self) -> &'static str {
        match self {
            InsertLocationShape::EndOfLine => "endOfLine",
            InsertLocationShape::EmptyLine => "emptyLine",
            InsertLocationShape::StartOfLine => "startOfLine",
            InsertLocationShape::MiddleOfLine => "middleOfLine",
        }
    }
}

/// `InsertLocationRelativeToCursor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertLocationRelativeToCursor {
    pub at_cursor: bool,
    pub before_cursor_on_same_line: bool,
    pub after_cursor_on_same_line: bool,
    pub lines_above: Option<i32>,
    pub lines_below: Option<i32>,
}

/// `InsertProperties`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertProperties {
    pub text_shape: TextShape,
    pub location_shape: InsertLocationShape,
    pub relative_to_cursor: Option<InsertLocationRelativeToCursor>,
}

/// `DeleteProperties`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteProperties {
    pub text_shape: TextShape,
    pub is_at_end_of_line: bool,
    pub deletes_entire_line_content: bool,
}

/// `ReplaceProperties`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplaceProperties {
    pub is_word_to_word_replacement: bool,
    pub is_additive: bool,
    pub is_subtractive: bool,
    pub is_single_line_to_single_line: bool,
    pub is_single_line_to_multi_line: bool,
    pub is_multi_line_to_single_line: bool,
}

/// `EditOperation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOperation {
    Insert,
    Delete,
    Replace,
}

impl EditOperation {
    fn as_str(self) -> &'static str {
        match self {
            EditOperation::Insert => "insert",
            EditOperation::Delete => "delete",
            EditOperation::Replace => "replace",
        }
    }
}

/// `InsertProperties | DeleteProperties | ReplaceProperties`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditProperties {
    Insert(InsertProperties),
    Delete(DeleteProperties),
    Replace(ReplaceProperties),
}

/// `IInlineSuggestionEditKindEdit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineSuggestionEditKindEdit {
    pub operation: EditOperation,
    pub properties: EditProperties,
    pub characters_inserted: i32,
    pub characters_deleted: i32,
    pub lines_inserted: i32,
    pub lines_deleted: i32,
}

/// `InlineSuggestionEditKind`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineSuggestionEditKind {
    pub edits: Vec<InlineSuggestionEditKindEdit>,
}

impl InlineSuggestionEditKind {
    pub fn new(edits: Vec<InlineSuggestionEditKindEdit>) -> Self {
        Self { edits }
    }

    /// `toString()`: `JSON.stringify({ edits: this.edits })`.
    pub fn to_json_string(&self) -> String {
        let edits: Vec<String> = self.edits.iter().map(edit_to_json).collect();
        format!("{{\"edits\":[{}]}}", edits.join(","))
    }
}

/// Um objeto JSON a partir dos pares já serializados, na ordem de inserção, sem os `undefined`.
fn json_object(fields: Vec<Option<(&str, String)>>) -> String {
    let body: Vec<String> = fields.into_iter().flatten().map(|(key, value)| format!("\"{key}\":{value}")).collect();
    format!("{{{}}}", body.join(","))
}

fn text_shape_to_json(shape: &TextShape) -> String {
    match shape {
        TextShape::SingleLine(s) => json_object(vec![
            Some(("kind", "\"singleLine\"".to_string())),
            Some(("isSingleCharacter", s.is_single_character.to_string())),
            s.single_character_kind.map(|k| ("singleCharacterKind", format!("\"{}\"", k.as_str()))),
            Some(("isWord", s.is_word.to_string())),
            Some(("isMultipleWords", s.is_multiple_words.to_string())),
            Some(("isMultipleWhitespace", s.is_multiple_whitespace.to_string())),
            Some(("hasDuplicatedWhitespace", s.has_duplicated_whitespace.to_string())),
        ]),
        TextShape::MultiLine { line_count } => json_object(vec![
            Some(("kind", "\"multiLine\"".to_string())),
            Some(("lineCount", line_count.to_string())),
        ]),
    }
}

fn relative_to_cursor_to_json(r: &InsertLocationRelativeToCursor) -> String {
    json_object(vec![
        Some(("atCursor", r.at_cursor.to_string())),
        Some(("beforeCursorOnSameLine", r.before_cursor_on_same_line.to_string())),
        Some(("afterCursorOnSameLine", r.after_cursor_on_same_line.to_string())),
        r.lines_above.map(|n| ("linesAbove", n.to_string())),
        r.lines_below.map(|n| ("linesBelow", n.to_string())),
    ])
}

fn properties_to_json(properties: &EditProperties) -> String {
    match properties {
        EditProperties::Insert(p) => json_object(vec![
            Some(("textShape", text_shape_to_json(&p.text_shape))),
            Some(("locationShape", format!("\"{}\"", p.location_shape.as_str()))),
            p.relative_to_cursor.as_ref().map(|r| ("relativeToCursor", relative_to_cursor_to_json(r))),
        ]),
        EditProperties::Delete(p) => json_object(vec![
            Some(("textShape", text_shape_to_json(&p.text_shape))),
            Some(("isAtEndOfLine", p.is_at_end_of_line.to_string())),
            Some(("deletesEntireLineContent", p.deletes_entire_line_content.to_string())),
        ]),
        EditProperties::Replace(p) => json_object(vec![
            Some(("isWordToWordReplacement", p.is_word_to_word_replacement.to_string())),
            Some(("isAdditive", p.is_additive.to_string())),
            Some(("isSubtractive", p.is_subtractive.to_string())),
            Some(("isSingleLineToSingleLine", p.is_single_line_to_single_line.to_string())),
            Some(("isSingleLineToMultiLine", p.is_single_line_to_multi_line.to_string())),
            Some(("isMultiLineToSingleLine", p.is_multi_line_to_single_line.to_string())),
        ]),
    }
}

fn edit_to_json(edit: &InlineSuggestionEditKindEdit) -> String {
    json_object(vec![
        Some(("operation", format!("\"{}\"", edit.operation.as_str()))),
        Some(("properties", properties_to_json(&edit.properties))),
        Some(("charactersInserted", edit.characters_inserted.to_string())),
        Some(("charactersDeleted", edit.characters_deleted.to_string())),
        Some(("linesInserted", edit.lines_inserted.to_string())),
        Some(("linesDeleted", edit.lines_deleted.to_string())),
    ])
}

/// `computeEditKind`: `None` para uma edição vazia (não há o que classificar).
pub fn compute_edit_kind(
    edit: &StringEdit,
    text_model: &TextModel,
    cursor_position: Option<Position>,
) -> Option<InlineSuggestionEditKind> {
    if edit.replacements().is_empty() {
        // Empty edit - return undefined as there's no edit to classify
        return None;
    }

    Some(InlineSuggestionEditKind::new(
        edit.replacements().iter().map(|rep| compute_single_edit_kind(rep, text_model, cursor_position)).collect(),
    ))
}

/// `countLines`.
fn count_lines(text: &[u16]) -> i32 {
    if text.is_empty() {
        return 0;
    }
    split_line_count(text) - 1
}

/// `textModel.getValue().substring(start, endExclusive)`.
fn text_in_model(text_model: &TextModel, start: i32, end_exclusive: i32) -> Vec<u16> {
    let value = text_model.get_value();
    js_string::substring(value.units(), i64::from(start), i64::from(end_exclusive)).to_vec()
}

/// `computeSingleEditKind`.
fn compute_single_edit_kind(
    replacement: &StringReplacement,
    text_model: &TextModel,
    cursor_position: Option<Position>,
) -> InlineSuggestionEditKindEdit {
    let replace_range = replacement.replace_range;
    let new_text = replacement.new_text.units();
    let deleted_length = replace_range.length();
    let inserted_length = new_text.len() as i32;
    let lines_inserted = count_lines(new_text);

    if replace_range.is_empty() {
        InlineSuggestionEditKindEdit {
            operation: EditOperation::Insert,
            properties: EditProperties::Insert(compute_insert_properties(
                replace_range.start,
                new_text,
                text_model,
                cursor_position,
            )),
            characters_inserted: inserted_length,
            characters_deleted: 0,
            lines_inserted,
            lines_deleted: 0,
        }
    } else if new_text.is_empty() {
        let deleted_text = text_in_model(text_model, replace_range.start, replace_range.end_exclusive);
        InlineSuggestionEditKindEdit {
            operation: EditOperation::Delete,
            properties: EditProperties::Delete(compute_delete_properties(
                replace_range.start,
                replace_range.end_exclusive,
                text_model,
            )),
            characters_inserted: 0,
            characters_deleted: deleted_length,
            lines_inserted: 0,
            lines_deleted: count_lines(&deleted_text),
        }
    } else {
        let old_text = text_in_model(text_model, replace_range.start, replace_range.end_exclusive);
        InlineSuggestionEditKindEdit {
            operation: EditOperation::Replace,
            properties: EditProperties::Replace(compute_replace_properties(&old_text, new_text)),
            characters_inserted: inserted_length,
            characters_deleted: deleted_length,
            lines_inserted,
            lines_deleted: count_lines(&old_text),
        }
    }
}

/// `computeInsertProperties`.
fn compute_insert_properties(
    offset: i32,
    new_text: &[u16],
    text_model: &TextModel,
    cursor_position: Option<Position>,
) -> InsertProperties {
    let text_shape = analyze_text_shape(new_text);
    let insert_position = text_model.get_position_at(i64::from(offset));
    let line_content = text_model.get_line_content(insert_position.line_number);
    let line_length = line_content.len() as i32;

    // Determine location shape
    let is_line_empty = js_string::trim(line_content.units()).is_empty();
    let is_at_end_of_line = insert_position.column > line_length;
    let is_at_start_of_line = insert_position.column == 1;

    let location_shape = if is_line_empty {
        InsertLocationShape::EmptyLine
    } else if is_at_end_of_line {
        InsertLocationShape::EndOfLine
    } else if is_at_start_of_line {
        InsertLocationShape::StartOfLine
    } else {
        InsertLocationShape::MiddleOfLine
    };

    // Compute relative to cursor if cursor position is provided
    let relative_to_cursor = cursor_position.map(|cursor| {
        let cursor_line = cursor.line_number;
        let insert_line = insert_position.line_number;
        let cursor_column = cursor.column;
        let insert_column = insert_position.column;

        InsertLocationRelativeToCursor {
            at_cursor: cursor_line == insert_line && cursor_column == insert_column,
            before_cursor_on_same_line: cursor_line == insert_line && insert_column < cursor_column,
            after_cursor_on_same_line: cursor_line == insert_line && insert_column > cursor_column,
            lines_above: (insert_line < cursor_line).then(|| cursor_line - insert_line),
            lines_below: (insert_line > cursor_line).then(|| insert_line - cursor_line),
        }
    });

    InsertProperties { text_shape, location_shape, relative_to_cursor }
}

/// `computeDeleteProperties`.
fn compute_delete_properties(start_offset: i32, end_offset: i32, text_model: &TextModel) -> DeleteProperties {
    let deleted_text = text_in_model(text_model, start_offset, end_offset);
    let text_shape = analyze_text_shape(&deleted_text);

    let start_position = text_model.get_position_at(i64::from(start_offset));
    let end_position = text_model.get_position_at(i64::from(end_offset));

    // Check if delete is at end of line
    let line_length = text_model.get_line_content(end_position.line_number).len() as i32;
    let is_at_end_of_line = end_position.column > line_length;

    // Check if entire line content is deleted
    let deletes_entire_line_content = start_position.line_number == end_position.line_number
        && start_position.column == 1
        && end_position.column > line_length;

    DeleteProperties { text_shape, is_at_end_of_line, deletes_entire_line_content }
}

/// `computeReplaceProperties`.
fn compute_replace_properties(old_text: &[u16], new_text: &[u16]) -> ReplaceProperties {
    let old_shape = analyze_text_shape(old_text);
    let new_shape = analyze_text_shape(new_text);

    let old_is_word = old_shape.as_single_line().is_some_and(|s| s.is_word);
    let new_is_word = new_shape.as_single_line().is_some_and(|s| s.is_word);
    let is_word_to_word_replacement = old_is_word && new_is_word;

    let is_additive = new_text.len() > old_text.len();
    let is_subtractive = new_text.len() < old_text.len();

    let old_single = old_shape.as_single_line().is_some();
    let new_single = new_shape.as_single_line().is_some();

    ReplaceProperties {
        is_word_to_word_replacement,
        is_additive,
        is_subtractive,
        is_single_line_to_single_line: old_single && new_single,
        is_single_line_to_multi_line: old_single && !new_single,
        is_multi_line_to_single_line: !old_single && new_single,
    }
}
