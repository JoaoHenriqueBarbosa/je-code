//! Porte de `src/vs/code/node/cliArgs.ts`.

/// Rewrites `--folder-uri <uri>` / `--file-uri <uri>` pairs into a single
/// `--flag=value` token so the URI is not a standalone argv entry. Used on
/// Windows to avoid Chromium filtering URL-like tokens before main.js runs.
/// See https://github.com/microsoft/vscode/issues/209072.
pub fn combine_uri_flags(args: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            // end-of-options marker: copy the rest verbatim
            result.extend(args[i..].iter().cloned());
            break;
        }
        if (arg == "--folder-uri" || arg == "--file-uri") && i + 1 < args.len() && !args[i + 1].starts_with('-') {
            result.push(format!("{arg}={}", args[i + 1]));
            i += 1; // skip the value, it's now part of the flag
        } else {
            result.push(arg.clone());
        }
        i += 1;
    }
    result
}
