//! Porte de `src/vs/base/test/common/console.test.ts` (suíte `Console`), caso a caso e na mesma ordem.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::console::get_first_frame as first_frame;
use je_base::path::{FixedEnvironment, PathModule, Platform};

fn fs_path(stack: &str) -> (String, f64, f64) {
    let frame = first_frame(Some(stack), Platform::Posix).expect("deve achar o primeiro frame");
    (frame.uri.fs_path(Platform::Posix).to_string_lossy(), frame.line, frame.column)
}

#[test]
fn get_first_frame() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let env = FixedEnvironment::new(Platform::Posix, "/");
    let normalize = |p: &str| PathModule::native(&env).normalize(p);

    let mut stack = "at vscode.commands.registerCommand (/Users/someone/Desktop/test-ts/out/src/extension.js:18:17)";
    assert_eq!(fs_path(stack), (normalize("/Users/someone/Desktop/test-ts/out/src/extension.js"), 18.0, 17.0));

    stack = "at /Users/someone/Desktop/test-ts/out/src/extension.js:18:17";
    assert_eq!(fs_path(stack), (normalize("/Users/someone/Desktop/test-ts/out/src/extension.js"), 18.0, 17.0));

    stack = "at c:\\Users\\someone\\Desktop\\end-js\\extension.js:18:17";
    assert_eq!(fs_path(stack), ("c:\\Users\\someone\\Desktop\\end-js\\extension.js".to_owned(), 18.0, 17.0));

    stack = "at e.$executeContributedCommand(c:\\Users\\someone\\Desktop\\end-js\\extension.js:18:17)";
    assert_eq!(fs_path(stack), ("c:\\Users\\someone\\Desktop\\end-js\\extension.js".to_owned(), 18.0, 17.0));

    stack = "at /Users/someone/Desktop/test-ts/out/src/extension.js:18:17\nat /Users/someone/Desktop/test-ts/out/src/other.js:28:27\nat /Users/someone/Desktop/test-ts/out/src/more.js:38:37";
    assert_eq!(fs_path(stack), (normalize("/Users/someone/Desktop/test-ts/out/src/extension.js"), 18.0, 17.0));
}
