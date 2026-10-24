//! Porte de `src/vs/editor/contrib/suggest/browser/wordDistance.ts`: a distância de uma sugestão até o cursor
//! (`WordDistance`), que dá o bônus de localidade da ordenação: as palavras do mesmo bloco de código que o cursor
//! vêm antes.
//!
//! Representação:
//! - `abstract class WordDistance` com `static None` e `static create` é o trait [`WordDistance`] com as funções
//!   associadas de `dyn WordDistance`: `<dyn WordDistance>::none()` e `<dyn WordDistance>::create(service, editor)`.
//! - `IEditorWorkerService` é o trait do porte (só com as duas operações de intervalos de palavras) e o `editor` é o
//!   `Rc<CodeEditorWidget>`: a distância relê a posição do editor a cada chamada. `ICodeEditor` não é objeto de trait
//!   (tem métodos genéricos), então o argumento é o widget.
//! - `create` é `async`: [`async_fn`] roda o corpo já, de forma síncrona, até o primeiro `await`.
//! - `const [ranges] = await provideSelectionRanges(model, [position])` com resultado ausente lança no original (não
//!   se desestrutura `undefined`): vira a rejeição com `TypeError`.
//! - `isFalsyOrEmpty(wordLines)` é `None` ou lista vazia; `binarySearch` devolve `-(n + 1)` onde o original devolve
//!   `~n`.
//! - `2 << 20` é a distância de uma palavra que não está no código visto pelo worker (2 097 152).

use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::binary_search;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::promise::Promise;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::suggest::SUGGEST;
use crate::editor_worker::{IEditorWorkerService, WordRanges};
use crate::language_providers::{SelectionRange, SelectionRangeProvider};
use crate::languages::{self, CompletionItemKind};
use crate::position::Position;
use crate::range::Range;
use crate::smart_select::bracket_selections::BracketSelectionRangeProvider;
use crate::suggest::suggest::label_text;

/// A distância de uma palavra ausente do worker (`2 << 20`).
const FAR_AWAY: i32 = 2 << 20;

/// `WordDistance`.
pub trait WordDistance {
    /// `distance(anchor, suggestion)`: quanto menor, mais perto do cursor.
    fn distance(&self, anchor: Position, suggestion: &languages::CompletionItem) -> i32;
}

/// `WordDistance.None`: toda sugestão está à mesma distância.
struct NoWordDistance;

impl WordDistance for NoWordDistance {
    fn distance(&self, _anchor: Position, _suggestion: &languages::CompletionItem) -> i32 {
        0
    }
}

/// A distância com bônus de localidade, que [`WordDistance::create`] monta.
struct LocalityWordDistance {
    editor: Rc<CodeEditorWidget>,
    position: Position,
    ranges: Vec<SelectionRange>,
    word_ranges: WordRanges,
}

impl WordDistance for LocalityWordDistance {
    fn distance(&self, anchor: Position, item: &languages::CompletionItem) -> i32 {
        if Some(self.position) != self.editor.get_position() {
            return 0;
        }
        if item.kind == CompletionItemKind::Keyword {
            return FAR_AWAY;
        }
        let word = label_text(&item.label);
        let Some(word_lines) = self.word_ranges.get(&JsString::from(word)).filter(|lines| !lines.is_empty()) else {
            return FAR_AWAY;
        };
        let idx = binary_search(word_lines, &Range::from_positions(anchor, anchor), |a, b| {
            f64::from(Range::compare_ranges_using_starts(Some(a), Some(b)))
        });
        // `idx >= 0 ? idx : Math.max(0, ~idx - 1)`, com `~idx == -idx - 1`
        let best_index = if idx >= 0 { idx } else { (-idx - 2).max(0) };
        let best_word_range = word_lines[best_index as usize];
        let mut block_distance = self.ranges.len() as i32;
        for range in &self.ranges {
            if !range.range.contains_range(&best_word_range) {
                break;
            }
            block_distance -= 1;
        }
        block_distance
    }
}

impl dyn WordDistance {
    /// `WordDistance.None`.
    pub fn none() -> Rc<dyn WordDistance> {
        Rc::new(NoWordDistance)
    }

    /// `WordDistance.create(service, editor)`: sem bônus de localidade ligado, sem modelo, ou sem como calcular as
    /// palavras do modelo, a distância é a `None`.
    pub fn create(service: &Rc<dyn IEditorWorkerService>, editor: &Rc<CodeEditorWidget>) -> Promise<Rc<dyn WordDistance>> {
        let service = Rc::clone(service);
        let editor = Rc::clone(editor);
        async_fn(async move {
            if !editor.get_option(SUGGEST).locality_bonus {
                return Ok(<dyn WordDistance>::none());
            }

            if !editor.has_model() {
                return Ok(<dyn WordDistance>::none());
            }

            let model = editor.get_model().expect("o editor tem modelo");
            let position = editor.get_position().expect("o editor com modelo tem posição");

            if !service.can_compute_word_ranges(&model.uri()) {
                return Ok(<dyn WordDistance>::none());
            }

            let selection_ranges = BracketSelectionRangeProvider
                .provide_selection_ranges(&model, &[position], &CancellationToken::None)
                .await?;
            let Some(ranges) = selection_ranges.and_then(|all| all.into_iter().next()) else {
                return Err(Error::type_error("undefined is not iterable"));
            };
            if ranges.is_empty() {
                return Ok(<dyn WordDistance>::none());
            }

            let Some(mut word_ranges) = service.compute_word_ranges(&model.uri(), ranges[0].range).await? else {
                return Ok(<dyn WordDistance>::none());
            };

            // remove current word
            let word_until_pos = model.get_word_until_position(position);
            word_ranges.remove(&word_until_pos.word);

            let distance: Rc<dyn WordDistance> = Rc::new(LocalityWordDistance { editor, position, ranges, word_ranges });
            Ok(distance)
        })
    }
}
