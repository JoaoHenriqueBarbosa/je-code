//! Porte de `src/vs/base/test/common/mime.test.ts` (suíte `Mime`), caso a caso e na mesma ordem.
//!
//! O `normalizeMimeType(mimeType, true)` sobrecarregado do original é `normalize_mime_type_strict`.
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque o `mime` não cria `IDisposable`.

use je_base::mime::{self, normalize_mime_type, normalize_mime_type_strict};
use je_base::path::{FixedEnvironment, Platform};

#[test]
fn normalize() {
    assert_eq!(normalize_mime_type("invalid"), "invalid");
    assert_eq!(normalize_mime_type_strict("invalid"), None);
    assert_eq!(normalize_mime_type("Text/plain"), "text/plain");
    assert_eq!(normalize_mime_type("Text/pläin"), "text/pläin");
    assert_eq!(normalize_mime_type("Text/plain;UPPER"), "text/plain;UPPER");
    assert_eq!(normalize_mime_type("Text/plain;lower"), "text/plain;lower");
}

#[test]
fn get_extension_for_mime_type() {
    // Note: for MIME types with multiple extensions (e.g., image/jpg -> .jpe, .jpeg, .jpg),
    // the function returns the first matching extension in iteration order
    assert!([".jpe", ".jpeg", ".jpg"].contains(&mime::get_extension_for_mime_type("image/jpg").unwrap()));
    // image/jpeg is an alias for image/jpg and should also return a valid extension
    assert!([".jpe", ".jpeg", ".jpg"].contains(&mime::get_extension_for_mime_type("image/jpeg").unwrap()));
    assert_eq!(mime::get_extension_for_mime_type("image/avif"), Some(".avif"));
    assert_eq!(mime::get_extension_for_mime_type("image/png"), Some(".png"));
    assert_eq!(mime::get_extension_for_mime_type("image/gif"), Some(".gif"));
    assert_eq!(mime::get_extension_for_mime_type("image/webp"), Some(".webp"));
    assert!([".mp2", ".mp2a", ".mp3", ".mpga", ".m2a", ".m3a"].contains(&mime::get_extension_for_mime_type("audio/mpeg").unwrap()));
    assert!([".mp4", ".mp4v", ".mpg4"].contains(&mime::get_extension_for_mime_type("video/mp4").unwrap()));
    assert_eq!(mime::get_extension_for_mime_type("text/plain"), Some(".txt"));
    assert_eq!(mime::get_extension_for_mime_type("unknown/type"), None);
}

#[test]
fn get_media_mime() {
    let env = FixedEnvironment::new(Platform::Posix, "/");
    assert_eq!(mime::get_media_mime("profile.avif", &env), Some("image/avif"));
}
