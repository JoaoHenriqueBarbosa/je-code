//! Porte de `src/vs/editor/contrib/inlayHints/browser/inlayHints.ts`: a âncora de uma dica (`InlayHintAnchor`), o item que a
//! carrega (`InlayHintItem`) e a coleta dos provedores (`InlayHintsFragments`).
//!
//! Decisões de tradução:
//! - `InlayHintItem.resolve` (com `_isResolved` e `_currentResolve`) não é lido por nenhum chamador do porte (só pelo controlador
//!   do editor, que não existe) e não tem porte; o item guarda a dica, a âncora e o provedor.
//! - `InlayHintsFragments.create` é [`InlayHintsFragments::create`] e devolve uma promessa que rejeita com o `CancellationError`
//!   quando o token foi cancelado ou o modelo foi descartado.
//! - `data: [InlayHintList, InlayHintsProvider][]` é um vetor compartilhado, preenchido na ordem em que as promessas dos
//!   provedores terminam; `registry.ordered(model).reverse()` e a ordenação estável por posição são os do original.
//! - `asCommandLink` (`createCommandUri`) não tem leitor no porte e não foi portado.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, on_unexpected_external_error};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;

use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{InlayHint, InlayHintList, InlayHintsProvider};
use crate::line_tokens::IViewLineTokens;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `'before' | 'after'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlayHintDirection {
    Before,
    After,
}

/// `InlayHintAnchor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlayHintAnchor {
    pub range: Range,
    pub direction: InlayHintDirection,
}

/// `InlayHintItem` (sem o `resolve`, ver a nota do módulo).
#[derive(Clone)]
pub struct InlayHintItem {
    pub hint: InlayHint,
    pub anchor: InlayHintAnchor,
    pub provider: Rc<dyn InlayHintsProvider>,
}

/// `InlayHintsFragments`.
pub struct InlayHintsFragments {
    disposables: Rc<DisposableStore>,
    pub items: Vec<InlayHintItem>,
    pub ranges: Vec<Range>,
    pub provider: Vec<Rc<dyn InlayHintsProvider>>,
}

impl InlayHintsFragments {
    /// `InlayHintsFragments.create(registry, model, ranges, token)`.
    pub fn create(
        registry: &LanguageFeatureRegistry<dyn InlayHintsProvider>,
        model: &Rc<TextModel>,
        ranges: Vec<Range>,
        token: &CancellationToken,
    ) -> Promise<Rc<InlayHintsFragments>> {
        let data: Rc<RefCell<Vec<(Rc<InlayHintList>, Rc<dyn InlayHintsProvider>)>>> = Rc::default();

        let mut ordered = registry.ordered(model, false);
        ordered.reverse();
        let (model, token) = (Rc::clone(model), token.clone());

        async_fn(async move {
            let mut promises: Vec<Promise<()>> = Vec::new();
            for provider in ordered {
                for range in &ranges {
                    let (provider, model, token, data, range) = (Rc::clone(&provider), Rc::clone(&model), token.clone(), Rc::clone(&data), *range);
                    promises.push(async_fn(async move {
                        match provider.provide_inlay_hints(&model, range, &token).await {
                            Ok(result) => {
                                let has_hints = result.as_ref().is_some_and(|list| !list.hints.is_empty());
                                if has_hints || provider.on_did_change_inlay_hints().is_some() {
                                    let list = result.unwrap_or_else(|| Rc::new(InlayHintList::new(Vec::new(), || {})));
                                    data.borrow_mut().push((list, provider));
                                }
                            }
                            Err(err) => on_unexpected_external_error(&err),
                        }
                        Ok(())
                    }));
                }
            }

            Promise::all(promises).await?;

            if token.is_cancellation_requested() || model.is_disposed() {
                return Err(Error::cancellation());
            }

            let data = data.borrow().clone();
            Ok(Rc::new(InlayHintsFragments::new(ranges, data, &model)))
        })
    }

    fn new(ranges: Vec<Range>, data: Vec<(Rc<InlayHintList>, Rc<dyn InlayHintsProvider>)>, model: &Rc<TextModel>) -> Self {
        let disposables = DisposableStore::new();
        let mut providers: Vec<Rc<dyn InlayHintsProvider>> = Vec::new();
        let mut items: Vec<InlayHintItem> = Vec::new();
        for (list, provider) in data {
            let hints = list.hints.clone();
            let disposable: DisposableRef = list;
            disposables.add(disposable);
            if !providers.iter().any(|known| Rc::ptr_eq(known, &provider)) {
                providers.push(Rc::clone(&provider));
            }

            for hint in hints {
                // compute the range to which the item should be attached to
                let position = model.validate_position(hint.position);

                let word_range = Self::get_range_at_position(model, position);
                let (range, direction) = if word_range.get_start_position().is_before(&position) {
                    (Range::from_positions(word_range.get_start_position(), position), InlayHintDirection::After)
                } else {
                    (Range::from_positions(position, word_range.get_end_position()), InlayHintDirection::Before)
                };

                items.push(InlayHintItem { hint, anchor: InlayHintAnchor { range, direction }, provider: Rc::clone(&provider) });
            }
        }
        items.sort_by(|a, b| Position::compare(&a.hint.position, &b.hint.position).cmp(&0));
        Self { disposables, items, ranges, provider: providers }
    }

    /// `_getRangeAtPosition(model, position)`.
    fn get_range_at_position(model: &Rc<TextModel>, position: Position) -> Range {
        let line = position.line_number;
        if let Some(word) = model.get_word_at_position(position) {
            // always prefer the word range
            return Range::new(line, word.start_column, line, word.end_column);
        }

        model.tokenization().tokenize_if_cheap(line);
        let tokens = model.tokenization().get_line_tokens(line);
        let offset = position.column - 1;
        let idx = tokens.find_token_index_at_offset(offset);

        let mut start = tokens.get_start_offset(idx);
        let mut end = tokens.get_end_offset(idx);

        if end - start == 1 {
            // single character token, when at its end try leading/trailing token instead
            if start == offset && idx > 1 {
                // leading token
                start = tokens.get_start_offset(idx - 1);
                end = tokens.get_end_offset(idx - 1);
            } else if end == offset && idx + 1 < tokens.get_count() {
                // trailing token
                start = tokens.get_start_offset(idx + 1);
                end = tokens.get_end_offset(idx + 1);
            }
        }

        Range::new(line, start + 1, line, end + 1)
    }
}

impl IDisposable for InlayHintsFragments {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}
