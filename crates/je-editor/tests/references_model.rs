//! Porte de `src/vs/editor/contrib/gotoSymbol/test/browser/referencesModel.test.ts` (suíte `references`), caso a caso.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de `ensure_no_disposables_are_leaked_in_test_suite`.
//! - Os literais `{ uri, range }` passados ao construtor (um `Location`, lido como `LocationLink` pelo original) são
//!   `LocationLink` sem os campos de origem e de alvo.
//! - `URI.file(path)` é `URI::file(path, host_platform())`, e `ref.uri.path` é `uri().path()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::uri::URI;
use je_editor::goto_symbol::references_model::ReferencesModel;
use je_editor::language_features_service::LocationLink;
use je_editor::position::Position;
use je_editor::range::Range;
use je_platform::resources::host_platform;

fn link(path: &str) -> LocationLink {
    LocationLink { origin_selection_range: None, uri: URI::file(path, host_platform()), range: Range::new(1, 1, 1, 1), target_selection_range: None }
}

// suite('references')

#[test]
fn nearest_reference() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let model = ReferencesModel::new(vec![link("/out/obj/can"), link("/out/obj/can2"), link("/src/can")], "FOO");

    let mut reference = model.nearest_reference(&URI::file("/src/can", host_platform()), &Position::new(1, 1));
    assert_eq!(reference.unwrap().uri().path().to_string_lossy(), "/src/can");

    reference = model.nearest_reference(&URI::file("/src/someOtherFileInSrc", host_platform()), &Position::new(1, 1));
    assert_eq!(reference.unwrap().uri().path().to_string_lossy(), "/src/can");

    reference = model.nearest_reference(&URI::file("/out/someOtherFile", host_platform()), &Position::new(1, 1));
    assert_eq!(reference.unwrap().uri().path().to_string_lossy(), "/out/obj/can");

    reference = model.nearest_reference(&URI::file("/out/obj/can2222", host_platform()), &Position::new(1, 1));
    assert_eq!(reference.unwrap().uri().path().to_string_lossy(), "/out/obj/can2");
}
