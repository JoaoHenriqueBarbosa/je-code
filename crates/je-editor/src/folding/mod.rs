//! Porte do que o dobramento de código (`src/vs/editor/contrib/folding/browser`) precisa de
//! `folding.ts` para os provedores de intervalo: `RangeProvider`, `FoldingLimitReporter` e
//! `SelectedLines`. O controlador, as ações e o restante de `folding.ts` não fazem parte desta fatia.
//!
//! Os módulos filhos são `foldingRanges.ts` ([`folding_ranges`]), `indentRangeProvider.ts`
//! ([`indent_range_provider`]), `foldingModel.ts` ([`folding_model`]), `hiddenRangeModel.ts`
//! ([`hidden_range_model`]) e `syntaxRangeProvider.ts` ([`syntax_range_provider`]). `foldingDecorations.ts` não foi portado: depende de `ICodeEditor`, do registro
//! de cores e de ícones (`registerColor`, `registerIcon`) e de `Codicon`, que não existem.
//!
//! Decisões de tradução:
//! - `number | false` de `FoldingLimitReporter.update` é `Option<usize>` (`None` é `false`).
//! - `RangeProvider.compute` devolve `Promise<FoldingRegions | null>`; como `Promise<T>` exige `T: Clone`
//!   e o original compartilha o objeto por referência, o valor é `Option<Rc<FoldingRegions>>`.
//! - `toSelectedLines(selections)` é [`to_selected_lines`], que devolve [`SelectionsAsLines`].

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;

use crate::selection::Selection;

pub mod folding_ranges;
pub mod indent_range_provider;
pub mod folding_model;
pub mod hidden_range_model;
pub mod syntax_range_provider;

use folding_ranges::FoldingRegions;

/// `RangeProvider`.
pub trait RangeProvider {
    fn id(&self) -> &str;

    fn compute(&self, cancellation_token: &CancellationToken) -> Promise<Option<Rc<FoldingRegions>>>;

    fn dispose(&self);
}

/// `FoldingLimitReporter`.
pub trait FoldingLimitReporter {
    /// O máximo de regiões de dobra.
    fn limit(&self) -> usize;

    /// `computed` é quantas regiões foram calculadas; `limited` é o limite aplicado, ou `None` (o
    /// `false` do original) se o limite não foi atingido.
    fn update(&self, computed: usize, limited: Option<usize>);
}

/// `SelectedLines`.
pub trait SelectedLines {
    fn starts_inside(&self, start_line: u32, end_line: u32) -> bool;
}

/// O objeto devolvido por `toSelectedLines(selections)`. Sem seleções (`null` ou lista vazia) o laço não
/// acha nenhuma e `startsInside` é sempre falso, como o ramo `() => false` do original.
pub struct SelectionsAsLines {
    selections: Vec<Selection>,
}

impl SelectedLines for SelectionsAsLines {
    fn starts_inside(&self, start_line: u32, end_line: u32) -> bool {
        self.selections.iter().any(|selection| {
            let line = i64::from(selection.start_line_number);
            line >= i64::from(start_line) && line <= i64::from(end_line)
        })
    }
}

/// `toSelectedLines(selections)`.
pub fn to_selected_lines(selections: Option<&[Selection]>) -> SelectionsAsLines {
    SelectionsAsLines {
        selections: selections.map(<[Selection]>::to_vec).unwrap_or_default(),
    }
}
