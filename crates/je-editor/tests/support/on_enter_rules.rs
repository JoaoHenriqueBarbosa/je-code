//! Porte de `src/vs/editor/test/common/modes/supports/onEnterRules.ts`: as regras de Enter de linguagens
//! de exemplo (JavaScript, PHP, C++, HTML e Visual Basic).

use je_editor::language_configuration::{EnterAction, IndentAction, OnEnterRule};

use super::re;

pub fn javascript_on_enter_rules() -> Vec<OnEnterRule> {
    vec![
        // e.g. /** | */
        OnEnterRule {
            before_text: re(r##"^\s*\/\*\*(?!\/)([^\*]|\*(?!\/))*$"##, ""),
            after_text: Some(re(r##"^\s*\*\/$"##, "")),
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::IndentOutdent, append_text: Some(" * ".to_owned()), remove_text: None },
        },
        // e.g. /** ...|
        OnEnterRule {
            before_text: re(r##"^\s*\/\*\*(?!\/)([^\*]|\*(?!\/))*$"##, ""),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::None, append_text: Some(" * ".to_owned()), remove_text: None },
        },
        // e.g.  * ...|
        OnEnterRule {
            before_text: re(r##"^(\t|[ ])*[ ]\*([ ]([^\*]|\*(?!\/))*)?$"##, ""),
            after_text: None,
            previous_line_text: Some(re(r##"(?=^(\s*(\/\*\*|\*)).*)(?=(?!(\s*\*\/)))"##, "")),
            action: EnterAction { indent_action: IndentAction::None, append_text: Some("* ".to_owned()), remove_text: None },
        },
        // e.g.  */|
        OnEnterRule {
            before_text: re(r##"^(\t|[ ])*[ ]\*\/\s*$"##, ""),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::None, append_text: None, remove_text: Some(1) },
        },
        // e.g.  *-----*/|
        OnEnterRule {
            before_text: re(r##"^(\t|[ ])*[ ]\*[^/]*\*\/\s*$"##, ""),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::None, append_text: None, remove_text: Some(1) },
        },
        OnEnterRule {
            before_text: re(r##"^\s*(\bcase\s.+:|\bdefault:)$"##, ""),
            after_text: Some(re(r##"^(?!\s*(\bcase\b|\bdefault\b))"##, "")),
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::Indent, append_text: None, remove_text: None },
        },
        OnEnterRule {
            before_text: re(r##"^\s+([^{i\s]|i(?!f\b))"##, ""),
            after_text: None,
            previous_line_text: Some(re(r##"^\s*(((else ?)?if|for|while)\s*\(.*\)\s*|else\s*)$"##, "")),
            action: EnterAction { indent_action: IndentAction::Outdent, append_text: None, remove_text: None },
        },
        OnEnterRule {
            before_text: re(r##"^.*\([^\)]*$"##, ""),
            after_text: Some(re(r##"^\s*\).*$"##, "")),
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::IndentOutdent, append_text: Some("\t".to_owned()), remove_text: None },
        },
        OnEnterRule {
            before_text: re(r##"^.*\{[^\}]*$"##, ""),
            after_text: Some(re(r##"^\s*\}.*$"##, "")),
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::IndentOutdent, append_text: Some("\t".to_owned()), remove_text: None },
        },
        OnEnterRule {
            before_text: re(r##"^.*\[[^\]]*$"##, ""),
            after_text: Some(re(r##"^\s*\].*$"##, "")),
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::IndentOutdent, append_text: Some("\t".to_owned()), remove_text: None },
        },
    ]
}

pub fn php_on_enter_rules() -> Vec<OnEnterRule> {
    vec![
        OnEnterRule {
            before_text: re(r##"^\s*\/\*\*(?!\/)([^\*]|\*(?!\/))*$"##, ""),
            after_text: Some(re(r##"^\s*\*\/$"##, "")),
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::IndentOutdent, append_text: Some(" * ".to_owned()), remove_text: None },
        },
        OnEnterRule {
            before_text: re(r##"^\s*\/\*\*(?!\/)([^\*]|\*(?!\/))*$"##, ""),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::None, append_text: Some(" * ".to_owned()), remove_text: None },
        },
        OnEnterRule {
            before_text: re(r##"^(\t|(\ \ ))*\ \*(\ ([^\*]|\*(?!\/))*)?$"##, ""),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::None, append_text: Some("* ".to_owned()), remove_text: None },
        },
        OnEnterRule {
            before_text: re(r##"^(\t|(\ \ ))*\ \*\/\s*$"##, ""),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::None, append_text: None, remove_text: Some(1) },
        },
        OnEnterRule {
            before_text: re(r##"^(\t|(\ \ ))*\ \*[^/]*\*\/\s*$"##, ""),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::None, append_text: None, remove_text: Some(1) },
        },
        OnEnterRule {
            before_text: re(r##"^\s+([^{i\s]|i(?!f\b))"##, ""),
            after_text: None,
            previous_line_text: Some(re(r##"^\s*(((else ?)?if|for(each)?|while)\s*\(.*\)\s*|else\s*)$"##, "")),
            action: EnterAction { indent_action: IndentAction::Outdent, append_text: None, remove_text: None },
        },
    ]
}

pub fn cpp_on_enter_rules() -> Vec<OnEnterRule> {
    vec![
        OnEnterRule {
            before_text: re(r##"^\s+([^{i\s]|i(?!f\b))"##, ""),
            after_text: None,
            previous_line_text: Some(re(r##"^\s*(((else ?)?if|for|while)\s*\(.*\)\s*|else\s*)$"##, "")),
            action: EnterAction { indent_action: IndentAction::Outdent, append_text: None, remove_text: None },
        },
    ]
}

pub fn html_on_enter_rules() -> Vec<OnEnterRule> {
    vec![
        OnEnterRule {
            before_text: re(r##"<(?!(?:area|base|br|col|embed|hr|img|input|keygen|link|menuitem|meta|param|source|track|wbr))([_:\w][_:\w\-.\d]*)(?:(?:[^'"/>]|"[^"]*"|'[^']*')*?(?!\/)>)[^<]*$"##, "i"),
            after_text: Some(re(r##"^<\/([_:\w][_:\w\-.\d]*)\s*>"##, "i")),
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::IndentOutdent, append_text: None, remove_text: None },
        },
        OnEnterRule {
            before_text: re(r##"<(?!(?:area|base|br|col|embed|hr|img|input|keygen|link|menuitem|meta|param|source|track|wbr))([_:\w][_:\w\-.\d]*)(?:(?:[^'"/>]|"[^"]*"|'[^']*')*?(?!\/)>)[^<]*$"##, "i"),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::Indent, append_text: None, remove_text: None },
        },
    ]
}

pub fn vb_on_enter_rules() -> Vec<OnEnterRule> {
    vec![
        OnEnterRule {
            before_text: re(r##"^\s*((End\s+(If|Sub|Function|Class|Module|Enum|Structure|Interface|Namespace|With|Select|Try|While|For|Property|Get|Set|SyncLock|Using|AddHandler|RaiseEvent|RemoveHandler|Event|Operator))|Loop|Next|Wend|Until)\b.*$"##, "i"),
            after_text: None,
            previous_line_text: None,
            action: EnterAction { indent_action: IndentAction::None, append_text: None, remove_text: None },
        },
    ]
}
