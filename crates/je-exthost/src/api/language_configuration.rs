//! `vscode.languages.setLanguageConfiguration(language, configuration)`: a leitura do `vscode.LanguageConfiguration` que a extensão
//! passou (`comments`, `brackets`, `wordPattern`, `indentationRules` e `onEnterRules`) como o [`ApiLanguageConfiguration`] do
//! `ExtHostLanguageFeatures` portado, que serializa as regras (`_serializeIndentationRule`, `_serializeOnEnterRules`...) e envia o
//! `$setLanguageConfiguration` ao `MainThreadLanguageFeatures`.
//!
//! A configuração e as `RegExp` dela não atravessam o motor como cópia (a `RegExp` chega como referência), então tudo é lido por
//! referência, propriedade a propriedade, com o [`JsObjects`]; cada `RegExp` vira o `JsRegExp` do host pelo `source` e pelas `flags`.
//!
//! Divergências:
//! - Os membros obsoletos `__electricCharacterSupport` e `__characterPairSupport` e o `autoClosingPairs` (que não está no
//!   `vscode.d.ts` estável) não são lidos.
//! - `IndentAction` é o número do enum (`None` 0, `Indent` 1, `IndentOutdent` 2, `Outdent` 3); um valor fora dele lança.

use std::rc::Rc;

use je_base::errors::Error;
use je_base::js_regexp::JsRegExp;
use je_editor::language_configuration::{CharacterPair, CommentRule, EnterAction, IndentAction, IndentationRule, LineComment, LineCommentConfig, OnEnterRule};
use je_workbench::api::common::ext_host_language_features_api::ApiLanguageConfiguration;
use zjsc::api::embed_node::JsData;

use crate::api::js_object::JsObjects;
use crate::api::language_provider_types::regexp_parts;
use crate::convert::{is_nullish, opt_bool, opt_number, opt_string};

fn error(message: impl Into<String>) -> Error {
    Error::new(message.into())
}

/// `target[name]`; `None` para `undefined` e `null`.
fn property(js: &JsObjects, target: &JsData, name: &str) -> Option<JsData> {
    let value = js.property(target, name);
    (!is_nullish(Some(&value))).then_some(value)
}

fn text_property(js: &JsObjects, target: &JsData, name: &str) -> Option<String> {
    opt_string(Some(&js.property(target, name)))
}

/// Os elementos de um array lido por referência.
fn items(js: &JsObjects, list: &JsData) -> Vec<JsData> {
    match js.property(list, "length") {
        JsData::Num(length) => (0..length as usize).map(|index| js.property(list, &index.to_string())).collect(),
        _ => Vec::new(),
    }
}

/// Cada elemento do array `list` lido por `read`.
fn list_of<T>(js: &JsObjects, list: &JsData, read: impl Fn(&JsData) -> Result<T, Error>) -> Result<Vec<T>, Error> {
    items(js, list).iter().map(read).collect()
}

fn reg_exp(js: &JsObjects, value: &JsData, what: &str) -> Result<Rc<JsRegExp>, Error> {
    let (source, flags) = regexp_parts(js, value, what)?;
    JsRegExp::from_str(&source, &flags).map(Rc::new).map_err(|reason| error(format!("{what} is not a valid regular expression: {reason}")))
}

fn optional_reg_exp(js: &JsObjects, target: &JsData, name: &str) -> Result<Option<Rc<JsRegExp>>, Error> {
    property(js, target, name).map(|value| reg_exp(js, &value, name)).transpose()
}

fn required_reg_exp(js: &JsObjects, target: &JsData, name: &str) -> Result<Rc<JsRegExp>, Error> {
    optional_reg_exp(js, target, name)?.ok_or_else(|| error(format!("{name} must be a RegExp")))
}

/// `CharacterPair`: `[string, string]`.
fn character_pair(js: &JsObjects, value: &JsData) -> Result<CharacterPair, Error> {
    let parts = items(js, value);
    match (opt_string(parts.first()), opt_string(parts.get(1))) {
        (Some(open), Some(close)) => Ok((open, close)),
        _ => Err(error("a CharacterPair must be [string, string]")),
    }
}

/// `CommentRule`: `lineComment` (texto, ou `{ comment, noIndent }`) e `blockComment`.
fn comment_rule(js: &JsObjects, comments: &JsData) -> Result<CommentRule, Error> {
    let line_comment = match js.property(comments, "lineComment") {
        JsData::Str(token) => Some(LineComment::Token(token)),
        config @ JsData::Handle(_) => Some(LineComment::Config(LineCommentConfig {
            comment: text_property(js, &config, "comment").unwrap_or_default(),
            no_indent: opt_bool(Some(&js.property(&config, "noIndent"))),
        })),
        _ => None,
    };
    let block_comment = property(js, comments, "blockComment").map(|pair| character_pair(js, &pair)).transpose()?;
    Ok(CommentRule { line_comment, block_comment })
}

fn indentation_rule(js: &JsObjects, rules: &JsData) -> Result<IndentationRule, Error> {
    Ok(IndentationRule {
        decrease_indent_pattern: required_reg_exp(js, rules, "decreaseIndentPattern")?,
        increase_indent_pattern: required_reg_exp(js, rules, "increaseIndentPattern")?,
        indent_next_line_pattern: optional_reg_exp(js, rules, "indentNextLinePattern")?,
        un_indented_line_pattern: optional_reg_exp(js, rules, "unIndentedLinePattern")?,
    })
}

fn indent_action_of(value: i32) -> Option<IndentAction> {
    [IndentAction::None, IndentAction::Indent, IndentAction::IndentOutdent, IndentAction::Outdent].into_iter().find(|action| *action as i32 == value)
}

fn on_enter_rule(js: &JsObjects, rule: &JsData) -> Result<OnEnterRule, Error> {
    let action = property(js, rule, "action").ok_or_else(|| error("OnEnterRule.action must be an EnterAction"))?;
    let indent_action = opt_number(Some(&js.property(&action, "indentAction")))
        .and_then(|value| indent_action_of(value as i32))
        .ok_or_else(|| error("EnterAction.indentAction must be an IndentAction"))?;
    Ok(OnEnterRule {
        before_text: required_reg_exp(js, rule, "beforeText")?,
        after_text: optional_reg_exp(js, rule, "afterText")?,
        previous_line_text: optional_reg_exp(js, rule, "previousLineText")?,
        action: EnterAction {
            indent_action,
            append_text: text_property(js, &action, "appendText"),
            remove_text: opt_number(Some(&js.property(&action, "removeText"))).map(|value| value as i32),
        },
    })
}

/// O `vscode.LanguageConfiguration` (um objeto da extensão, por referência) como [`ApiLanguageConfiguration`].
pub fn language_configuration_from_js(js: &JsObjects, configuration: &JsData) -> Result<ApiLanguageConfiguration, Error> {
    Ok(ApiLanguageConfiguration {
        comments: property(js, configuration, "comments").map(|comments| comment_rule(js, &comments)).transpose()?,
        brackets: property(js, configuration, "brackets").map(|brackets| list_of(js, &brackets, |pair| character_pair(js, pair))).transpose()?,
        word_pattern: optional_reg_exp(js, configuration, "wordPattern")?,
        indentation_rules: property(js, configuration, "indentationRules").map(|rules| indentation_rule(js, &rules)).transpose()?,
        on_enter_rules: property(js, configuration, "onEnterRules").map(|rules| list_of(js, &rules, |rule| on_enter_rule(js, rule))).transpose()?,
        ..ApiLanguageConfiguration::default()
    })
}
