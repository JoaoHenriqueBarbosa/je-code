//! `createSuggestItem` de `src/vs/editor/contrib/suggest/test/browser/completionModel.test.ts`: o item de
//! sugestão dos testes da memória e do modelo de sugestões.
//!
//! Os parâmetros opcionais do original (`kind`, `incomplete`, `position`, `sortText`, `filterText`) viram os
//! campos de [`SuggestItemOptions`], com os mesmos padrões; [`create_suggest_item`] é a chamada com só
//! `(label, overwriteBefore)`.

use std::rc::Rc;

use je_editor::languages::{
    CompletionItem as LanguageCompletionItem, CompletionItemKind, CompletionItemProvider, CompletionItemRange,
    CompletionLabel, CompletionList,
};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::suggest::suggest::CompletionItem;

/// O provedor `{ _debugDisplayName: 'test', provideCompletionItems() { return; } }`.
struct TestProvider;

impl CompletionItemProvider for TestProvider {
    fn debug_display_name(&self) -> &str {
        "test"
    }
}

/// Os parâmetros opcionais de `createSuggestItem`.
pub struct SuggestItemOptions {
    pub kind: CompletionItemKind,
    pub incomplete: bool,
    pub position: Position,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
}

impl Default for SuggestItemOptions {
    fn default() -> Self {
        Self {
            kind: CompletionItemKind::Property,
            incomplete: false,
            position: Position::new(1, 1),
            sort_text: None,
            filter_text: None,
        }
    }
}

/// `createSuggestItem(label, overwriteBefore)`.
pub fn create_suggest_item(label: &str, overwrite_before: i32) -> CompletionItem {
    create_suggest_item_with(CompletionLabel::Text(label.to_string()), overwrite_before, SuggestItemOptions::default())
}

/// `createSuggestItem(label, overwriteBefore, kind, incomplete, position, sortText, filterText)`.
pub fn create_suggest_item_with(label: CompletionLabel, overwrite_before: i32, options: SuggestItemOptions) -> CompletionItem {
    let SuggestItemOptions { kind, incomplete, position, sort_text, filter_text } = options;
    let insert_text = match &label {
        CompletionLabel::Text(text) => text.clone(),
        CompletionLabel::Item(item) => item.label.clone(),
    };
    let suggestion = LanguageCompletionItem {
        label,
        kind,
        range: CompletionItemRange::Single(Range {
            start_line_number: position.line_number,
            start_column: position.column - overwrite_before,
            end_line_number: position.line_number,
            end_column: position.column,
        }),
        tags: None,
        detail: None,
        documentation: None,
        sort_text,
        filter_text,
        preselect: None,
        insert_text,
        insert_text_rules: None,
        commit_characters: None,
        additional_text_edits: None,
        command: None,
        action: None,
        extension_id: None,
    };
    let container = Rc::new(CompletionList {
        suggestions: vec![suggestion.clone()],
        incomplete: Some(incomplete),
        dispose: None,
        duration: None,
    });
    CompletionItem::new(position, suggestion, container, Rc::new(TestProvider))
}
