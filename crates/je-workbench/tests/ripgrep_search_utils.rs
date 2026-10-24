//! Porte de `src/vs/workbench/services/search/test/node/ripgrepSearchUtils.test.ts` (suíte
//! `RipgrepSymlinkLoopDetector`), caso a caso e na mesma ordem.
//!
//! Nota de tradução: `ensureNoDisposablesAreLeakedInTestSuite()` não tem o que conferir (o detector não é
//! descartável) e fica de fora.

use je_workbench::services::search::ripgrep_search_utils::{MAX_RIPGREP_SYMLINK_LOOPS, RipgrepSymlinkLoopDetector};

#[test]
fn reports_once_more_loops_than_allowed_were_found_including_loops_split_across_chunks() {
    let mut detector = RipgrepSymlinkLoopDetector::new();
    let loop_line = "rg: File system loop found: a/link points to an ancestor a\n";

    let actual = [
        detector.handle_stderr(&(loop_line.repeat(MAX_RIPGREP_SYMLINK_LOOPS - 1) + "rg: ./unreadable: Permission denied (os error 13)\n")),
        detector.handle_stderr(&(loop_line.to_owned() + &loop_line[..20])),
        detector.handle_stderr(&loop_line[20..]),
    ];
    assert_eq!(actual, [false, false, true]);
}
