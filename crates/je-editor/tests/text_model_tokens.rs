//! Porte de `src/vs/editor/test/common/model/textModelTokens.test.ts` (suíte `RangePriorityQueueImpl`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `assert.deepStrictEqual(ranges.map(r => r.toString()), [...])` vira `assert_eq!` sobre `Vec<String>`.
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::offset_range::OffsetRange;
use je_editor::text_model_tokens::RangePriorityQueueImpl;

fn strings(ranges: &[OffsetRange]) -> Vec<String> {
    ranges.iter().map(ToString::to_string).collect()
}

mod range_priority_queue_impl {
    use super::*;

    #[test]
    fn add_range() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut ranges: Vec<OffsetRange> = Vec::new();

        OffsetRange::add_range(OffsetRange::new(0, 2), &mut ranges);
        OffsetRange::add_range(OffsetRange::new(10, 13), &mut ranges);
        OffsetRange::add_range(OffsetRange::new(20, 24), &mut ranges);

        assert_eq!(strings(&ranges), ["[0, 2)", "[10, 13)", "[20, 24)"]);

        OffsetRange::add_range(OffsetRange::new(2, 10), &mut ranges);

        assert_eq!(strings(&ranges), ["[0, 13)", "[20, 24)"]);

        OffsetRange::add_range(OffsetRange::new(14, 19), &mut ranges);

        assert_eq!(strings(&ranges), ["[0, 13)", "[14, 19)", "[20, 24)"]);

        OffsetRange::add_range(OffsetRange::new(10, 22), &mut ranges);

        assert_eq!(strings(&ranges), ["[0, 24)"]);

        OffsetRange::add_range(OffsetRange::new(-1, 29), &mut ranges);

        assert_eq!(strings(&ranges), ["[-1, 29)"]);

        OffsetRange::add_range(OffsetRange::new(-10, -5), &mut ranges);

        assert_eq!(strings(&ranges), ["[-10, -5)", "[-1, 29)"]);
    }

    #[test]
    fn add_range_and_resize() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut queue = RangePriorityQueueImpl::new();

        queue.add_range(OffsetRange::new(0, 20));
        queue.add_range(OffsetRange::new(100, 120));
        queue.add_range(OffsetRange::new(200, 220));

        // disjoint
        queue.add_range_and_resize(OffsetRange::new(25, 27), 0);

        assert_eq!(strings(queue.get_ranges()), ["[0, 20)", "[98, 118)", "[198, 218)"]);

        queue.add_range_and_resize(OffsetRange::new(19, 20), 0);

        assert_eq!(strings(queue.get_ranges()), ["[0, 19)", "[97, 117)", "[197, 217)"]);

        queue.add_range_and_resize(OffsetRange::new(19, 97), 0);

        assert_eq!(strings(queue.get_ranges()), ["[0, 39)", "[119, 139)"]);

        queue.add_range_and_resize(OffsetRange::new(-1000, 1000), 0);

        assert_eq!(strings(queue.get_ranges()), Vec::<String>::new());
    }
}
