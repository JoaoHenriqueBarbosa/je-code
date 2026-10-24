//! Porte de `src/vs/workbench/contrib/issue/common/issueReporterUtil.ts`.

use je_base::JsString;
use je_base::strings::rtrim;

/// `rtrim(haystack, needle)` do `strings.ts` sobre texto Rust: tira `needle` repetidas vezes do fim.
fn rtrim_text(haystack: &str, needle: &str) -> String {
    let haystack = JsString::from(haystack);
    let needle = JsString::from(needle);
    JsString::from(rtrim(haystack.units(), needle.units())).to_string_lossy()
}

/// `normalizeGitHubUrl`: tira o sufixo `.git`, a barra final e os sufixos `/new` e `/issues`.
pub fn normalize_github_url(url: &str) -> String {
    // Se a URL tem o sufixo .git, remove
    let mut url = url.strip_suffix(".git").unwrap_or(url).to_string();

    // Remove a barra final
    url = rtrim_text(&url, "/");

    if url.ends_with("/new") {
        url = rtrim_text(&url, "/new");
    }

    if url.ends_with("/issues") {
        url = rtrim_text(&url, "/issues");
    }

    url
}
