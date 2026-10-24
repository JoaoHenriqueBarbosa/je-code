//! Porte de `src/vs/editor/contrib/folding/test/browser/indentFold.test.ts` (suíte `Indentation Folding`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `{ limit, update: (computed, limited) => reported = limited }` vira um [`FoldingLimitReporter`] com
//!   `Cell`: o `number | false` de `reported` é `Option<usize>` (`None` é o `false` do original).
//! - `assert.ok(indentRanges.length <= maxEntries, ...)` e os `deepStrictEqual` levam a mensagem do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::folding::FoldingLimitReporter;
use je_editor::folding::indent_range_provider::compute_ranges;
use support::test_text_model::create_text_model;

#[derive(Debug, PartialEq, Eq)]
struct IndentRange {
    start: u32,
    end: u32,
}

fn r(start: u32, end: u32) -> IndentRange {
    IndentRange { start, end }
}

/// O `{ limit: maxEntries, update }` do original.
struct LimitReporter {
    limit: usize,
    reported: Cell<Option<usize>>,
}

impl FoldingLimitReporter for LimitReporter {
    fn limit(&self) -> usize {
        self.limit
    }

    fn update(&self, _computed: usize, limited: Option<usize>) {
        self.reported.set(limited);
    }
}

#[test]
fn limit_by_indent() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let lines = [
        /* 1*/ "A",
        /* 2*/ "  A",
        /* 3*/ "  A",
        /* 4*/ "    A",
        /* 5*/ "      A",
        /* 6*/ "    A",
        /* 7*/ "      A",
        /* 8*/ "      A",
        /* 9*/ "         A",
        /* 10*/ "      A",
        /* 11*/ "         A",
        /* 12*/ "  A",
        /* 13*/ "              A",
        /* 14*/ "                 A",
        /* 15*/ "A",
        /* 16*/ "  A",
    ];
    let r1 = || r(1, 14); //0
    let r2 = || r(3, 11); //1
    let r3 = || r(4, 5); //2
    let r4 = || r(6, 11); //2
    let r5 = || r(8, 9); //3
    let r6 = || r(10, 11); //3
    let r7 = || r(12, 14); //1
    let r8 = || r(13, 14); //4
    let r9 = || r(15, 16); //0

    let model = create_text_model(&lines.join("\n"));

    let assert_limit = |max_entries: usize, expected_ranges: Vec<IndentRange>, message: &str| {
        let reporter = LimitReporter { limit: max_entries, reported: Cell::new(None) };
        let indent_ranges = compute_ranges(&model, true, None, Some(&reporter));
        assert!(indent_ranges.length() <= max_entries, "max {message}");
        let mut actual: Vec<IndentRange> = Vec::new();
        for i in 0..indent_ranges.length() {
            actual.push(IndentRange { start: indent_ranges.get_start_line_number(i), end: indent_ranges.get_end_line_number(i) });
        }
        assert_eq!(actual, expected_ranges, "{message}");
        let expected_reported = if 9 <= max_entries { None } else { Some(max_entries) };
        assert_eq!(reporter.reported.get(), expected_reported, "limited");
    };

    assert_limit(1000, vec![r1(), r2(), r3(), r4(), r5(), r6(), r7(), r8(), r9()], "1000");
    assert_limit(9, vec![r1(), r2(), r3(), r4(), r5(), r6(), r7(), r8(), r9()], "9");
    assert_limit(8, vec![r1(), r2(), r3(), r4(), r5(), r6(), r7(), r9()], "8");
    assert_limit(7, vec![r1(), r2(), r3(), r4(), r5(), r7(), r9()], "7");
    assert_limit(6, vec![r1(), r2(), r3(), r4(), r7(), r9()], "6");
    assert_limit(5, vec![r1(), r2(), r3(), r7(), r9()], "5");
    assert_limit(4, vec![r1(), r2(), r7(), r9()], "4");
    assert_limit(3, vec![r1(), r2(), r9()], "3");
    assert_limit(2, vec![r1(), r9()], "2");
    assert_limit(1, vec![r1()], "1");
    assert_limit(0, vec![], "0");

    model.dispose();
}
