//! Porte de `src/vs/workbench/contrib/terminalContrib/quickFix/browser/terminalQuickFixBuiltinActions.ts`: os quick fixes
//! internos (git, porta em uso e as sugestões do PowerShell).
//!
//! Decisões de tradução:
//! - As constantes `RegExp` exportadas (`GitSimilarOutputRegex` e as demais) são funções que devolvem o mesmo `RegExp` compilado
//!   uma vez por thread (o `JsRegExp` guarda `lastIndex`, então não é `Sync`).
//! - `String#replace(regex, () => ...)` sem `g` troca o primeiro casamento ([`replace_first`]); `trim()` é o do JavaScript.
//! - `lines[i + 1].match(...)` do `pwshGeneralError` lança `TypeError` no original quando a linha `Suggestion [General]:` é a
//!   última; aqui o caso não gera sugestões.

use std::rc::Rc;

use je_base::js_regexp::JsRegExp;
use je_base::js_string::{self, JsString};
use je_base::nls;
use je_base::uri::URI;
use je_platform::terminal::terminal::{ITerminalOutputMatcher, LineMatcher, OutputMatcherAnchor, RegExpMatchArray};

use super::quick_fix::{
    CommandExitResult, ITerminalCommandMatchResult, ITerminalQuickFixOpenerAction, ITerminalQuickFixPortAction,
    ITerminalQuickFixTerminalCommandAction, QuickFixHandler, QuickFixOption, TerminalQuickFixActionInternal, match_capture, match_group,
};

/// Constrói a função de uma constante `RegExp` do original.
macro_rules! regex_const {
    ($name:ident, $pattern:expr) => {
        pub fn $name() -> Rc<JsRegExp> {
            thread_local! {
                static REGEX: Rc<JsRegExp> = Rc::new(JsRegExp::from_str($pattern, "").expect("padrão de quick fix válido"));
            }
            REGEX.with(Rc::clone)
        }
    };
}

regex_const!(git_command_line_regex, r"git");
regex_const!(git_fast_forward_pull_output_regex, r"and can be fast-forwarded");
regex_const!(git_push_command_line_regex, r"git\s+push");
regex_const!(git_two_dashes_regex, r"error: did you mean `--(.+)` \(with two dashes\)\?");
regex_const!(git_similar_output_regex, r"(?:(most similar commands? (is|are)))");
regex_const!(
    free_port_output_regex,
    r"(?:address already in use (?:0\.0\.0\.0|127\.0\.0\.1|localhost|::):|Unable to bind [^ ]*:|can't listen on port |listen EADDRINUSE [^ ]*:)(?<portNumber>\d{4,5})"
);
regex_const!(git_push_output_regex, r"git push --set-upstream origin (?<branchName>[^\s]+)");
// The previous line starts with "Create a pull request for \'([^\s]+)\' on GitHub by visiting:\s*"
// it's safe to assume it's a github pull request if the URL includes `/pull/`
regex_const!(git_create_pr_output_regex, r"remote:\s*(?<link>https:\/\/github\.com\/.+\/.+\/pull\/new\/.+)");
regex_const!(pwsh_general_error_output_regex, r"Suggestion \[General\]:");
regex_const!(pwsh_unix_command_not_found_error_output_regex, r"Suggestion \[cmd-not-found\]:");

/// `QuickFixSource.Builtin`.
pub const QUICK_FIX_SOURCE_BUILTIN: &str = "builtin";

/// O `commandLineMatcher: /.+/` do `freePort` e dos quick fixes do PowerShell.
fn any_command_line_matcher() -> LineMatcher {
    thread_local! {
        static REGEX: Rc<JsRegExp> = Rc::new(JsRegExp::from_str(".+", "").expect("padrão de quick fix válido"));
    }
    LineMatcher::RegExp(REGEX.with(Rc::clone))
}

fn output_matcher(line_matcher: Rc<JsRegExp>, offset: f64, length: f64) -> ITerminalOutputMatcher {
    ITerminalOutputMatcher {
        line_matcher: LineMatcher::RegExp(line_matcher),
        anchor: OutputMatcherAnchor::Bottom,
        offset,
        length,
        multiple_matches: None,
    }
}

/// `text.trim()` do JavaScript.
fn js_trim(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(js_string::trim(&units))
}

/// `text.replace(regex, () => replacement)` sem a flag `g`: troca o primeiro casamento.
fn replace_first(regex: &JsRegExp, text: &str, replacement: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    regex.set_last_index(0);
    match regex.exec(&units) {
        Some(found) => {
            let mut out = JsString::from_units(units[..found.start()].to_vec());
            let replacement_units: Vec<u16> = replacement.encode_utf16().collect();
            out.push_units(&replacement_units);
            out.push_units(&units[found.end()..]);
            out.to_string_lossy()
        }
        None => text.to_owned(),
    }
}

fn terminal_command_action(id: &str, terminal_command: String, should_execute: Option<bool>) -> TerminalQuickFixActionInternal {
    TerminalQuickFixActionInternal::TerminalCommand(ITerminalQuickFixTerminalCommandAction {
        id: id.to_owned(),
        source: QUICK_FIX_SOURCE_BUILTIN.to_owned(),
        terminal_command,
        should_execute,
    })
}

fn internal_options(
    id: &str,
    command_line_matcher: LineMatcher,
    output_matcher: ITerminalOutputMatcher,
    command_exit_result: CommandExitResult,
    get_quick_fixes: impl Fn(&ITerminalCommandMatchResult) -> Vec<TerminalQuickFixActionInternal> + 'static,
) -> QuickFixOption {
    QuickFixOption::new(id, command_line_matcher, Some(output_matcher), command_exit_result, None, QuickFixHandler::Internal(Rc::new(get_quick_fixes)))
}

pub fn git_similar() -> QuickFixOption {
    internal_options(
        "Git Similar",
        LineMatcher::RegExp(git_command_line_regex()),
        output_matcher(git_similar_output_regex(), 0.0, 10.0),
        CommandExitResult::Error,
        |match_result| {
            let Some(output_match) = &match_result.output_match else {
                return Vec::new();
            };
            let Some(regex_match) = match_capture(&output_match.regex_match, 0).filter(|text| !text.is_empty()) else {
                return Vec::new();
            };
            let mut actions = Vec::new();
            let start_index = output_match.output_lines.iter().position(|line| line.contains(&regex_match)).map_or(0, |index| index + 1);
            let results: Vec<String> = output_match.output_lines.iter().map(|line| js_trim(line)).collect();
            let command_line_regex = JsRegExp::from_str(r"git\s+[^\s]+", "").expect("padrão de quick fix válido");
            for fixed_command in results.iter().skip(start_index) {
                if !fixed_command.is_empty() {
                    actions.push(terminal_command_action(
                        "Git Similar",
                        replace_first(&command_line_regex, &match_result.command_line, &format!("git {fixed_command}")),
                        Some(true),
                    ));
                }
            }
            actions
        },
    )
}

pub fn git_fast_forward_pull() -> QuickFixOption {
    internal_options(
        "Git Fast Forward Pull",
        LineMatcher::RegExp(git_command_line_regex()),
        output_matcher(git_fast_forward_pull_output_regex(), 0.0, 8.0),
        CommandExitResult::Success,
        |_match_result| vec![terminal_command_action("Git Fast Forward Pull", "git pull".to_owned(), Some(true))],
    )
}

pub fn git_two_dashes() -> QuickFixOption {
    internal_options(
        "Git Two Dashes",
        LineMatcher::RegExp(git_command_line_regex()),
        output_matcher(git_two_dashes_regex(), 0.0, 2.0),
        CommandExitResult::Error,
        |match_result| {
            let Some(problem_arg) = match_result.output_match.as_ref().and_then(|output_match| match_capture(&output_match.regex_match, 1)).filter(|arg| !arg.is_empty())
            else {
                return Vec::new();
            };
            vec![terminal_command_action(
                "Git Two Dashes",
                match_result.command_line.replacen(&format!(" -{problem_arg}"), &format!(" --{problem_arg}"), 1),
                Some(true),
            )]
        },
    )
}

pub fn free_port(run_callback: Rc<dyn Fn(String, String)>) -> QuickFixOption {
    internal_options(
        "Free Port",
        any_command_line_matcher(),
        output_matcher(free_port_output_regex(), 0.0, 30.0),
        CommandExitResult::Error,
        move |match_result| {
            let Some(port) = match_result.output_match.as_ref().and_then(|output_match| match_group(&output_match.regex_match, "portNumber")).filter(|port| !port.is_empty())
            else {
                return Vec::new();
            };
            let label = nls::localize("terminal.freePort", "Free port {0}", &[port.as_str()]);
            let run_callback = run_callback.clone();
            let command_line = match_result.command_line.clone();
            let run_port = port.clone();
            vec![TerminalQuickFixActionInternal::Port(ITerminalQuickFixPortAction {
                id: "Free Port".to_owned(),
                source: QUICK_FIX_SOURCE_BUILTIN.to_owned(),
                tooltip: label.clone(),
                label,
                class: None,
                enabled: true,
                run: Rc::new(move || run_callback(run_port.clone(), command_line.clone())),
            })]
        },
    )
}

pub fn git_push_set_upstream() -> QuickFixOption {
    internal_options(
        "Git Push Set Upstream",
        LineMatcher::RegExp(git_push_command_line_regex()),
        /*
           Example output on Windows:
           8: PS C:\Users\merogge\repos\xterm.js> git push
           7: fatal: The current branch sdjfskdjfdslkjf has no upstream branch.
           6: To push the current branch and set the remote as upstream, use
           5:
           4:	git push --set-upstream origin sdjfskdjfdslkjf
           3:
           2: To have this happen automatically for branches without a tracking
           1: upstream, see 'push.autoSetupRemote' in 'git help config'.
           0:

           Example output on macOS:
           5: meganrogge@Megans-MacBook-Pro xterm.js % git push
           4: fatal: The current branch merogge/asjdkfsjdkfsdjf has no upstream branch.
           3: To push the current branch and set the remote as upstream, use
           2:
           1:	git push --set-upstream origin merogge/asjdkfsjdkfsdjf
           0:
        */
        output_matcher(git_push_output_regex(), 0.0, 8.0),
        CommandExitResult::Error,
        |match_result| {
            let command_to_run = "git push --set-upstream origin ${group:branchName}";
            let Some(output_match) = &match_result.output_match else {
                return Vec::new();
            };
            let groups = &output_match.regex_match.matched.groups;
            if groups.is_empty() {
                return Vec::new();
            }
            let mut fixed_command = command_to_run.to_owned();
            for (key, _) in groups {
                let var_to_resolve = format!("${{group:{key}}}");
                if !command_to_run.contains(&var_to_resolve) {
                    return Vec::new();
                }
                // O valor de um grupo que não participou é `undefined`, que o `replaceAll` escreve como texto.
                let value = match_group(&output_match.regex_match, key).unwrap_or_else(|| "undefined".to_owned());
                fixed_command = fixed_command.replace(&var_to_resolve, &value);
            }
            if fixed_command.is_empty() {
                return Vec::new();
            }
            vec![terminal_command_action("Git Push Set Upstream", fixed_command, Some(true))]
        },
    )
}

pub fn git_create_pr() -> QuickFixOption {
    internal_options(
        "Git Create Pr",
        LineMatcher::RegExp(git_push_command_line_regex()),
        // Example output:
        // ...
        // 10: remote:
        // 9:  remote: Create a pull request for 'my_branch' on GitHub by visiting:
        // 8:  remote:      https://github.com/microsoft/vscode/pull/new/my_branch
        // 7:  remote:
        // 6:  remote: GitHub found x vulnerabilities on microsoft/vscode's default branch (...). To find out more, visit:
        // 5:  remote:      https://github.com/microsoft/vscode/security/dependabot
        // 4:  remote:
        // 3:  To https://github.com/microsoft/vscode
        // 2:  * [new branch]              my_branch -> my_branch
        // 1:  Branch 'my_branch' set up to track remote branch 'my_branch' from 'origin'.
        // 0:
        //
        // ~6 should only be needed here for security alerts, but the git provider can customize
        // the text, so use 12 to be safe.
        output_matcher(git_create_pr_output_regex(), 4.0, 12.0),
        CommandExitResult::Success,
        |match_result| {
            let Some(link) = match_result
                .output_match
                .as_ref()
                .and_then(|output_match| match_group(&output_match.regex_match, "link"))
                .map(|link| String::from_utf16_lossy(js_string::trim_end(&link.encode_utf16().collect::<Vec<u16>>())))
                .filter(|link| !link.is_empty())
            else {
                return Vec::new();
            };
            vec![TerminalQuickFixActionInternal::Opener(ITerminalQuickFixOpenerAction {
                id: "Git Create Pr".to_owned(),
                source: QUICK_FIX_SOURCE_BUILTIN.to_owned(),
                uri: URI::parse(link.as_str(), false),
            })]
        },
    )
}

/// As linhas do texto casado (`regexMatch.input?.split('\n')`).
fn input_lines(match_result: &ITerminalCommandMatchResult) -> Option<Vec<String>> {
    let output_match = match_result.output_match.as_ref()?;
    Some(output_match.regex_match.input.to_string_lossy().split('\n').map(str::to_owned).collect())
}

pub fn pwsh_general_error() -> QuickFixOption {
    internal_options(
        "Pwsh General Error",
        any_command_line_matcher(),
        output_matcher(pwsh_general_error_output_regex(), 0.0, 10.0),
        CommandExitResult::Error,
        |match_result| {
            let Some(lines) = input_lines(match_result) else {
                return Vec::new();
            };

            // Find the start
            let general_error = pwsh_general_error_output_regex();
            let Some(start) = lines.iter().position(|line| line_matches(&general_error, line)) else {
                return Vec::new();
            };

            let Some(next_line) = lines.get(start + 1) else {
                return Vec::new();
            };
            let similar_commands = JsRegExp::from_str(r"The most similar commands are: (?<values>.+).", "").expect("padrão de quick fix válido");
            let Some(values) = exec_str(&similar_commands, next_line).and_then(|found| match_group(&found, "values")) else {
                return Vec::new();
            };
            values.split(", ").map(|suggestion| terminal_command_action("Pwsh General Error", suggestion.to_owned(), None)).collect()
        },
    )
}

pub fn pwsh_unix_command_not_found_error() -> QuickFixOption {
    internal_options(
        "Unix Command Not Found",
        any_command_line_matcher(),
        output_matcher(pwsh_unix_command_not_found_error_output_regex(), 0.0, 10.0),
        CommandExitResult::Error,
        |match_result| {
            let Some(lines) = input_lines(match_result) else {
                return Vec::new();
            };

            // Find the start
            let not_found = pwsh_unix_command_not_found_error_output_regex();
            let Some(start) = lines.iter().position(|line| line_matches(&not_found, line)) else {
                return Vec::new();
            };

            // Always remove the first element as it's the "Suggestion [cmd-not-found]"" line
            let mut result = Vec::new();
            let mut in_suggestions = false;
            let installed = JsRegExp::from_str(r"You also have .+ installed, you can run '(?<command>.+)' instead.", "").expect("padrão de quick fix válido");
            let can_be_installed = JsRegExp::from_str(r"Command '.+' not found, but can be installed with:", "").expect("padrão de quick fix válido");
            for line in &lines[start..] {
                let line = js_trim(line);
                if line.is_empty() {
                    break;
                }
                if let Some(install_command) = exec_str(&installed, &line).and_then(|found| match_group(&found, "command")) {
                    result.push(terminal_command_action("Pwsh Unix Command Not Found Error", install_command, None));
                    in_suggestions = false;
                    continue;
                }
                if line_matches(&can_be_installed, &line) {
                    in_suggestions = true;
                    continue;
                }
                if in_suggestions {
                    result.push(terminal_command_action("Pwsh Unix Command Not Found Error", js_trim(&line), None));
                }
            }
            result
        },
    )
}

/// `line.match(regex)` com um `RegExp` sem `g`.
fn exec_str(regex: &JsRegExp, text: &str) -> Option<RegExpMatchArray> {
    let input = JsString::from(text);
    regex.set_last_index(0);
    regex.exec(input.units()).map(|matched| RegExpMatchArray { input, matched })
}

/// `!!line.match(regex)`.
fn line_matches(regex: &JsRegExp, text: &str) -> bool {
    exec_str(regex, text).is_some()
}
