//! Porte de `src/vs/editor/test/common/modes/supports/indentationRules.ts`: as regras de indentação de
//! linguagens de exemplo.

use je_editor::language_configuration::IndentationRule;

use super::re;

pub fn javascript_indentation_rules() -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: re(r##"^((?!.*?\/\*).*\*\/)?\s*[\}\]\)].*$"##, ""),
        increase_indent_pattern: re(r##"^((?!\/\/).)*(\{([^}"'`]*|(\t|[ ])*\/\/.*)|\([^)"'`]*|\[[^\]"'`]*)$"##, ""),
        indent_next_line_pattern: Some(re(r##"^((.*=>\s*)|((.*[^\w]+|\s*)(if|while|for)\s*\(.*\)\s*))$"##, "")),
        un_indented_line_pattern: Some(re(r##"^(\t|[ ])*[ ]\*[^/]*\*\/\s*$|^(\t|[ ])*[ ]\*\/\s*$|^(\t|[ ])*[ ]\*([ ]([^\*]|\*(?!\/))*)?$"##, "")),
    }
}

pub fn ruby_indentation_rules() -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: re(r##"^\s*([}\]]([,)]?\s*(#|$)|\.[a-zA-Z_]\w*\b)|(end|rescue|ensure|else|elsif)\b|(in|when)\s)"##, ""),
        increase_indent_pattern: re(r##"^\s*((begin|class|(private|protected)\s+def|def|else|elsif|ensure|for|if|module|rescue|unless|until|when|in|while|case)|([^#]*\sdo\b)|([^#]*=\s*(case|if|unless)))\b([^#\{;]|(\"|'|\/).*\4)*(#.*)?$"##, ""),
        indent_next_line_pattern: None,
        un_indented_line_pattern: None,
    }
}

pub fn php_indentation_rules() -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: re(r##"^(.*\*\/)?\s*((\})|(\)+[;,])|(\]\)*[;,])|\b(else:)|\b((end(if|for(each)?|while|switch));))"##, ""),
        increase_indent_pattern: re(r##"({(?!.*}).*|\(|\[|((else(\s)?)?if|else|for(each)?|while|switch|case).*:)\s*((\/[/*].*|)?$|\?>)"##, ""),
        indent_next_line_pattern: None,
        un_indented_line_pattern: None,
    }
}

pub fn go_indentation_rules() -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: re(r##"^\s*(\bcase\b.*:|\bdefault\b:|}[)}]*[),]?|\)[,]?)$"##, ""),
        increase_indent_pattern: re(r##"^.*(\bcase\b.*:|\bdefault\b:|(\b(func|if|else|switch|select|for|struct)\b.*)?{[^}"'`]*|\([^)"'`]*)$"##, ""),
        indent_next_line_pattern: None,
        un_indented_line_pattern: None,
    }
}

pub fn html_indentation_rules() -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: re(r##"^\s*(<\/(?!html)[-_\.A-Za-z0-9]+\b[^>]*>|-->|\})"##, ""),
        increase_indent_pattern: re(r##"<(?!\?|(?:area|base|br|col|frame|hr|html|img|input|keygen|link|menuitem|meta|param|source|track|wbr)\b|[^>]*\/>)([-_\.A-Za-z0-9]+)(?=\s|>)\b[^>]*>(?!.*<\/\1>)|<!--(?!.*-->)|\{[^}"']*$"##, ""),
        indent_next_line_pattern: None,
        un_indented_line_pattern: None,
    }
}

pub fn latex_indentation_rules() -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: re(r##"^\s*\\end{(?!document)"##, ""),
        increase_indent_pattern: re(r##"\\begin{(?!document)([^}]*)}(?!.*\\end{\1})"##, ""),
        indent_next_line_pattern: None,
        un_indented_line_pattern: None,
    }
}

pub fn lua_indentation_rules() -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: re(r##"^\s*((\b(elseif|else|end|until)\b)|(\})|(\)))"##, ""),
        increase_indent_pattern: re(r##"^((?!(\-\-)).)*((\b(else|function|then|do|repeat)\b((?!\b(end|until)\b).)*)|(\{\s*))$"##, ""),
        indent_next_line_pattern: None,
        un_indented_line_pattern: None,
    }
}

pub fn vb_indentation_rules() -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: re(r##"^\s*((End\s+(If|Sub|Function|Class|Module|Enum|Structure|Interface|Namespace|With|Select|Try|While|For|Property|Get|Set|SyncLock|Using|AddHandler|RaiseEvent|RemoveHandler|Event|Operator))|Else|ElseIf|Case|Catch|Finally|Loop|Next|Wend|Until)\b"##, "i"),
        increase_indent_pattern: re(r##"^\s*((If|ElseIf).*Then(?!.*End\s+If)\s*(('|REM).*)?|(Else|While|For|Do|Select\s+Case|Case|Sub|Function|Class|Module|Enum|Structure|Interface|Namespace|With|Try|Catch|Finally|SyncLock|Using|Property|Get|Set|AddHandler|RaiseEvent|RemoveHandler|Event|Operator)\b(?!.*\bEnd\s+(If|Sub|Function|Class|Module|Enum|Structure|Interface|Namespace|With|Select|Try|While|For|Property|Get|Set|SyncLock|Using|AddHandler|RaiseEvent|RemoveHandler|Event|Operator)\b).*(('|REM).*)?)$"##, "i"),
        indent_next_line_pattern: None,
        un_indented_line_pattern: None,
    }
}
