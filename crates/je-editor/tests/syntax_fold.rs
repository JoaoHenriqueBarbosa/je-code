//! Porte de `src/vs/editor/contrib/folding/test/browser/syntaxFold.test.ts` (suíte `Syntax folding`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `async` + `await` do caso são o corpo `async` de [`run_with_faked_timers`], que o executa no event loop do porte
//!   e reprova se ele não terminar.
//! - `IndentRange` é [`IndentRange`]; o `FoldingRange` que o provedor devolve (`{ start, end }`, sem `kind`) é montado
//!   a partir dele.
//! - `foldingRangesLimit` (`{ limit, update }`) é [`LimitReporter`]: `reported` guarda o `limited` do último `update`
//!   (`number | false`, aqui `Option<usize>`).
//! - O terceiro argumento do `SyntaxRangeProvider` (`() => { }`) é um `Rc<dyn Fn()>` vazio e o último (`undefined`) é
//!   `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_editor::folding::syntax_range_provider::{FoldingContext, FoldingRange, FoldingRangeProvider, SyntaxRangeProvider};
use je_editor::folding::{FoldingLimitReporter, RangeProvider};
use je_editor::language_providers::ProviderResult;
use je_editor::text_model::TextModel;
use support::test_text_model::create_text_model;

#[derive(Clone, Debug, PartialEq, Eq)]
struct IndentRange {
    start: u32,
    end: u32,
}

struct TestFoldingRangeProvider {
    model: Rc<TextModel>,
    ranges: Vec<IndentRange>,
}

impl FoldingRangeProvider for TestFoldingRangeProvider {
    fn provide_folding_ranges(
        &self,
        model: &Rc<TextModel>,
        _context: &FoldingContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Vec<FoldingRange>> {
        if Rc::ptr_eq(model, &self.model) {
            let ranges = self
                .ranges
                .iter()
                .map(|range| FoldingRange { start: range.start as i32, end: range.end as i32, kind: None })
                .collect();
            return Promise::resolved(Some(ranges));
        }
        Promise::resolved(None)
    }
}

/// O objeto `{ limit: maxEntries, update: (computed, limited) => reported = limited }`.
struct LimitReporter {
    limit: usize,
    reported: Rc<Cell<Option<usize>>>,
}

impl FoldingLimitReporter for LimitReporter {
    fn limit(&self) -> usize {
        self.limit
    }

    fn update(&self, _computed: usize, limited: Option<usize>) {
        self.reported.set(limited);
    }
}

fn r(start: u32, end: u32) -> IndentRange {
    IndentRange { start, end }
}

// suite('Syntax folding')

#[test]
fn limit_by_nesting_level() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let lines = [
            /* 1*/ "{",
            /* 2*/ "  A",
            /* 3*/ "  {",
            /* 4*/ "    {",
            /* 5*/ "      B",
            /* 6*/ "    }",
            /* 7*/ "    {",
            /* 8*/ "      A",
            /* 9*/ "      {",
            /* 10*/ "         A",
            /* 11*/ "      }",
            /* 12*/ "      {",
            /* 13*/ "        {",
            /* 14*/ "          {",
            /* 15*/ "             A",
            /* 16*/ "          }",
            /* 17*/ "        }",
            /* 18*/ "      }",
            /* 19*/ "    }",
            /* 20*/ "  }",
            /* 21*/ "}",
            /* 22*/ "{",
            /* 23*/ "  A",
            /* 24*/ "}",
        ];

        let r1 = r(1, 20); //0
        let r2 = r(3, 19); //1
        let r3 = r(4, 5); //2
        let r4 = r(7, 18); //2
        let r5 = r(9, 10); //3
        let r6 = r(12, 17); //4
        let r7 = r(13, 16); //5
        let r8 = r(14, 15); //6
        let r9 = r(22, 23); //0

        let model = create_text_model(&lines.join("\n"));
        let ranges = vec![
            r1.clone(),
            r2.clone(),
            r3.clone(),
            r4.clone(),
            r5.clone(),
            r6.clone(),
            r7.clone(),
            r8.clone(),
            r9.clone(),
        ];
        let providers: Vec<Rc<dyn FoldingRangeProvider>> =
            vec![Rc::new(TestFoldingRangeProvider { model: model.clone(), ranges })];

        async fn assert_limit(
            model: &Rc<TextModel>,
            providers: &[Rc<dyn FoldingRangeProvider>],
            max_entries: usize,
            expected_ranges: Vec<IndentRange>,
            message: &str,
        ) {
            let reported: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
            let folding_ranges_limit = Rc::new(LimitReporter { limit: max_entries, reported: reported.clone() });
            let syntax_range_provider = SyntaxRangeProvider::new(
                model.clone(),
                providers.to_vec(),
                Rc::new(|| {}),
                folding_ranges_limit,
                None,
            );
            let indent_ranges = syntax_range_provider.compute(&CancellationToken::None).await.expect("compute cumprido");
            let mut actual: Vec<IndentRange> = Vec::new();
            if let Some(indent_ranges) = indent_ranges {
                for i in 0..indent_ranges.length() {
                    actual.push(IndentRange {
                        start: indent_ranges.get_start_line_number(i),
                        end: indent_ranges.get_end_line_number(i),
                    });
                }
                let expected_reported = if 9 <= max_entries { None } else { Some(max_entries) };
                assert_eq!(reported.get(), expected_reported, "limited");
            }
            assert_eq!(actual, expected_ranges, "{message}");
            syntax_range_provider.dispose();
        }

        let c = |ranges: &[&IndentRange]| -> Vec<IndentRange> { ranges.iter().map(|range| (*range).clone()).collect() };

        assert_limit(&model, &providers, 1000, c(&[&r1, &r2, &r3, &r4, &r5, &r6, &r7, &r8, &r9]), "1000").await;
        assert_limit(&model, &providers, 9, c(&[&r1, &r2, &r3, &r4, &r5, &r6, &r7, &r8, &r9]), "9").await;
        assert_limit(&model, &providers, 8, c(&[&r1, &r2, &r3, &r4, &r5, &r6, &r7, &r9]), "8").await;
        assert_limit(&model, &providers, 7, c(&[&r1, &r2, &r3, &r4, &r5, &r6, &r9]), "7").await;
        assert_limit(&model, &providers, 6, c(&[&r1, &r2, &r3, &r4, &r5, &r9]), "6").await;
        assert_limit(&model, &providers, 5, c(&[&r1, &r2, &r3, &r4, &r9]), "5").await;
        assert_limit(&model, &providers, 4, c(&[&r1, &r2, &r3, &r9]), "4").await;
        assert_limit(&model, &providers, 3, c(&[&r1, &r2, &r9]), "3").await;
        assert_limit(&model, &providers, 2, c(&[&r1, &r9]), "2").await;
        assert_limit(&model, &providers, 1, c(&[&r1]), "1").await;
        assert_limit(&model, &providers, 0, c(&[]), "0").await;

        model.dispose();
    });
}
