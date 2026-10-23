//! Porte de `src/vs/workbench/contrib/codeEditor/common/languageConfigurationExtensionPoint.ts`: lê os
//! `language-configuration.json` que as extensões declaram e os registra no `ILanguageConfigurationService`
//! com prioridade 50, na primeira vez que uma linguagem pede os recursos básicos.
//!
//! Divergências:
//! - O original lê os arquivos de forma assíncrona (`IExtensionResourceLoaderService`) e espera
//!   `whenInstalledExtensionsRegistered`; aqui a leitura é síncrona do disco e o registro acontece na hora.
//! - O aviso de `console.warn` vira `eprintln!`; o JSON schema `vscode://schemas/language-configuration`
//!   (validação no editor de configurações) não foi portado.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::uri::URI;
use je_base::js_regexp::JsRegExp;
use je_editor::language_configuration::{
    AutoClosingPair, AutoClosingPairConditional, CharacterPair, CommentRule, EnterAction, FoldingMarkers, FoldingRules,
    IndentAction, IndentationRule, LanguageConfiguration, LineComment, LineCommentConfig, OnEnterRule,
};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use serde_json::{Map, Value};

use crate::jsonc;

/// `ILanguageConfiguration` do `package.json` das extensões, ainda sem validação (`serde_json::Value`).
pub type RawLanguageConfiguration = Value;

fn is_string_arr(something: &Value) -> bool {
    something.as_array().is_some_and(|a| a.iter().all(Value::is_string))
}

fn is_character_pair(something: &Value) -> bool {
    is_string_arr(something) && something.as_array().is_some_and(|a| a.len() == 2)
}

fn pair_of(value: &Value) -> CharacterPair {
    (value[0].as_str().unwrap_or_default().to_owned(), value[1].as_str().unwrap_or_default().to_owned())
}

/// `_parseRegex(languageId, confPath, value)`.
fn parse_regex(language_id: &str, conf_path: &str, value: &Value) -> Option<Rc<JsRegExp>> {
    let compile = |pattern: &str, flags: &str| match JsRegExp::from_str(pattern, flags) {
        Ok(regex) => Some(Rc::new(regex)),
        Err(err) => {
            eprintln!("[{language_id}]: Invalid regular expression in `{conf_path}`: {err:?}");
            None
        }
    };
    match value {
        Value::String(pattern) => compile(pattern, ""),
        Value::Object(object) => {
            let Some(pattern) = object.get("pattern").and_then(Value::as_str) else {
                eprintln!("[{language_id}]: language configuration: expected `{conf_path}.pattern` to be a string.");
                return None;
            };
            let flags = match object.get("flags") {
                None | Some(Value::Null) => "",
                Some(Value::String(flags)) => flags,
                Some(_) => {
                    eprintln!("[{language_id}]: language configuration: expected `{conf_path}.flags` to be a string.");
                    return None;
                }
            };
            compile(pattern, flags)
        }
        _ => {
            eprintln!("[{language_id}]: language configuration: expected `{conf_path}` to be a string or an object.");
            None
        }
    }
}

fn extract_valid_comment_rule(language_id: &str, configuration: &Value) -> Option<CommentRule> {
    let source = configuration.get("comments")?;
    let Some(source) = source.as_object() else {
        eprintln!("[{language_id}]: language configuration: expected `comments` to be an object.");
        return None;
    };
    let mut result: Option<CommentRule> = None;
    if let Some(line_comment) = source.get("lineComment") {
        match line_comment {
            Value::String(token) => result.get_or_insert_with(CommentRule::default).line_comment = Some(LineComment::Token(token.clone())),
            Value::Object(object) => {
                if let Some(comment) = object.get("comment").and_then(Value::as_str) {
                    result.get_or_insert_with(CommentRule::default).line_comment = Some(LineComment::Config(LineCommentConfig {
                        comment: comment.to_owned(),
                        no_indent: object.get("noIndent").and_then(Value::as_bool),
                    }));
                } else {
                    eprintln!("[{language_id}]: language configuration: expected `comments.lineComment.comment` to be a string.");
                }
            }
            _ => eprintln!(
                "[{language_id}]: language configuration: expected `comments.lineComment` to be a string or an object with comment property."
            ),
        }
    }
    if let Some(block_comment) = source.get("blockComment") {
        if !is_character_pair(block_comment) {
            eprintln!("[{language_id}]: language configuration: expected `comments.blockComment` to be an array of two strings.");
        } else {
            result.get_or_insert_with(CommentRule::default).block_comment = Some(pair_of(block_comment));
        }
    }
    result
}

/// Os pares `[abre, fecha]` válidos de uma lista (`brackets` ou `colorizedBracketPairs`).
fn extract_character_pairs(language_id: &str, configuration: &Value, key: &str) -> Option<Vec<CharacterPair>> {
    let source = configuration.get(key)?;
    let Some(source) = source.as_array() else {
        eprintln!("[{language_id}]: language configuration: expected `{key}` to be an array.");
        return None;
    };
    let mut result: Option<Vec<CharacterPair>> = None;
    for (i, pair) in source.iter().enumerate() {
        if !is_character_pair(pair) {
            eprintln!("[{language_id}]: language configuration: expected `{key}[{i}]` to be an array of two strings.");
            continue;
        }
        result.get_or_insert_with(Vec::new).push(pair_of(pair));
    }
    // `colorizedBracketPairs` devolve `[]` mesmo vazio; `brackets` devolve `undefined`. A diferença é
    // invisível para o registro, que trata os dois como "sem pares".
    if key == "colorizedBracketPairs" { Some(result.unwrap_or_default()) } else { result }
}

fn extract_valid_auto_closing_pairs(language_id: &str, configuration: &Value) -> Option<Vec<AutoClosingPairConditional>> {
    let source = configuration.get("autoClosingPairs")?;
    let Some(source) = source.as_array() else {
        eprintln!("[{language_id}]: language configuration: expected `autoClosingPairs` to be an array.");
        return None;
    };
    let mut result: Option<Vec<AutoClosingPairConditional>> = None;
    for (i, pair) in source.iter().enumerate() {
        if pair.is_array() {
            if !is_character_pair(pair) {
                eprintln!("[{language_id}]: language configuration: expected `autoClosingPairs[{i}]` to be an array of two strings or an object.");
                continue;
            }
            let (open, close) = pair_of(pair);
            result.get_or_insert_with(Vec::new).push(AutoClosingPairConditional::new(&open, &close));
        } else {
            let Some(object) = pair.as_object() else {
                eprintln!("[{language_id}]: language configuration: expected `autoClosingPairs[{i}]` to be an array of two strings or an object.");
                continue;
            };
            let Some(open) = object.get("open").and_then(Value::as_str) else {
                eprintln!("[{language_id}]: language configuration: expected `autoClosingPairs[{i}].open` to be a string.");
                continue;
            };
            let Some(close) = object.get("close").and_then(Value::as_str) else {
                eprintln!("[{language_id}]: language configuration: expected `autoClosingPairs[{i}].close` to be a string.");
                continue;
            };
            let mut not_in = None;
            if let Some(value) = object.get("notIn").filter(|v| !v.is_null()) {
                if !is_string_arr(value) {
                    eprintln!("[{language_id}]: language configuration: expected `autoClosingPairs[{i}].notIn` to be a string array.");
                    continue;
                }
                not_in = Some(value.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()).unwrap_or_default());
            }
            let mut before_text = None;
            if let Some(value) = object.get("beforeText").filter(|v| !v.is_null()) {
                before_text = parse_regex(language_id, &format!("autoClosingPairs[{i}].beforeText"), value);
                if before_text.is_none() {
                    continue;
                }
            }
            result.get_or_insert_with(Vec::new).push(AutoClosingPairConditional {
                open: open.to_owned(),
                close: close.to_owned(),
                not_in,
                before_text,
            });
        }
    }
    result
}

fn extract_valid_surrounding_pairs(language_id: &str, configuration: &Value) -> Option<Vec<AutoClosingPair>> {
    let source = configuration.get("surroundingPairs")?;
    let Some(source) = source.as_array() else {
        eprintln!("[{language_id}]: language configuration: expected `surroundingPairs` to be an array.");
        return None;
    };
    let mut result: Option<Vec<AutoClosingPair>> = None;
    for (i, pair) in source.iter().enumerate() {
        if pair.is_array() {
            if !is_character_pair(pair) {
                eprintln!("[{language_id}]: language configuration: expected `surroundingPairs[{i}]` to be an array of two strings or an object.");
                continue;
            }
            let (open, close) = pair_of(pair);
            result.get_or_insert_with(Vec::new).push(AutoClosingPair::new(&open, &close));
        } else {
            let Some(object) = pair.as_object() else {
                eprintln!("[{language_id}]: language configuration: expected `surroundingPairs[{i}]` to be an array of two strings or an object.");
                continue;
            };
            let Some(open) = object.get("open").and_then(Value::as_str) else {
                eprintln!("[{language_id}]: language configuration: expected `surroundingPairs[{i}].open` to be a string.");
                continue;
            };
            let Some(close) = object.get("close").and_then(Value::as_str) else {
                eprintln!("[{language_id}]: language configuration: expected `surroundingPairs[{i}].close` to be a string.");
                continue;
            };
            result.get_or_insert_with(Vec::new).push(AutoClosingPair::new(open, close));
        }
    }
    result
}

fn extract_valid_on_enter_rules(language_id: &str, configuration: &Value) -> Option<Vec<OnEnterRule>> {
    let source = configuration.get("onEnterRules")?;
    let Some(source) = source.as_array() else {
        eprintln!("[{language_id}]: language configuration: expected `onEnterRules` to be an array.");
        return None;
    };
    let mut result: Option<Vec<OnEnterRule>> = None;
    for (i, on_enter_rule) in source.iter().enumerate() {
        let Some(rule) = on_enter_rule.as_object() else {
            eprintln!("[{language_id}]: language configuration: expected `onEnterRules[{i}]` to be an object.");
            continue;
        };
        let Some(raw_action) = rule.get("action").and_then(Value::as_object) else {
            eprintln!("[{language_id}]: language configuration: expected `onEnterRules[{i}].action` to be an object.");
            continue;
        };
        let indent_action = match raw_action.get("indent").and_then(Value::as_str) {
            Some("none") => IndentAction::None,
            Some("indent") => IndentAction::Indent,
            Some("indentOutdent") => IndentAction::IndentOutdent,
            Some("outdent") => IndentAction::Outdent,
            _ => {
                eprintln!(
                    "[{language_id}]: language configuration: expected `onEnterRules[{i}].action.indent` to be 'none', 'indent', 'indentOutdent' or 'outdent'."
                );
                continue;
            }
        };
        let mut action = EnterAction::new(indent_action);
        if let Some(append_text) = raw_action.get("appendText").filter(|v| is_truthy(v)) {
            match append_text.as_str() {
                Some(text) => action.append_text = Some(text.to_owned()),
                None => eprintln!(
                    "[{language_id}]: language configuration: expected `onEnterRules[{i}].action.appendText` to be undefined or a string."
                ),
            }
        }
        if let Some(remove_text) = raw_action.get("removeText").filter(|v| is_truthy(v)) {
            match remove_text.as_f64() {
                Some(n) => action.remove_text = Some(n as i32),
                None => eprintln!(
                    "[{language_id}]: language configuration: expected `onEnterRules[{i}].action.removeText` to be undefined or a number."
                ),
            }
        }
        let Some(before_text) = parse_regex(language_id, &format!("onEnterRules[{i}].beforeText"), rule.get("beforeText").unwrap_or(&Value::Null)) else {
            continue;
        };
        let mut resulting = OnEnterRule { before_text, after_text: None, previous_line_text: None, action };
        if let Some(after_text) = rule.get("afterText").filter(|v| is_truthy(v)) {
            resulting.after_text = parse_regex(language_id, &format!("onEnterRules[{i}].afterText"), after_text);
        }
        if let Some(previous) = rule.get("previousLineText").filter(|v| is_truthy(v)) {
            resulting.previous_line_text = parse_regex(language_id, &format!("onEnterRules[{i}].previousLineText"), previous);
        }
        result.get_or_insert_with(Vec::new).push(resulting);
    }
    result
}

/// A verdade de um valor JavaScript (`if (value)`).
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn map_indentation_rules(language_id: &str, rules: &Map<String, Value>) -> Option<IndentationRule> {
    let increase = parse_regex(language_id, "indentationRules.increaseIndentPattern", rules.get("increaseIndentPattern").unwrap_or(&Value::Null))?;
    let decrease = parse_regex(language_id, "indentationRules.decreaseIndentPattern", rules.get("decreaseIndentPattern").unwrap_or(&Value::Null))?;
    let mut result = IndentationRule {
        increase_indent_pattern: increase,
        decrease_indent_pattern: decrease,
        indent_next_line_pattern: None,
        un_indented_line_pattern: None,
    };
    if let Some(value) = rules.get("indentNextLinePattern").filter(|v| is_truthy(v)) {
        result.indent_next_line_pattern = parse_regex(language_id, "indentationRules.indentNextLinePattern", value);
    }
    if let Some(value) = rules.get("unIndentedLinePattern").filter(|v| is_truthy(v)) {
        result.un_indented_line_pattern = parse_regex(language_id, "indentationRules.unIndentedLinePattern", value);
    }
    Some(result)
}

/// `LanguageConfigurationFileHandler.extractValidConfig(languageId, configuration)`.
pub fn extract_valid_config(language_id: &str, configuration: &RawLanguageConfiguration) -> LanguageConfiguration {
    let word_pattern = configuration.get("wordPattern").filter(|v| is_truthy(v)).and_then(|v| parse_regex(language_id, "wordPattern", v));
    let indentation_rules = configuration
        .get("indentationRules")
        .filter(|v| is_truthy(v))
        .and_then(|v| v.as_object())
        .and_then(|rules| map_indentation_rules(language_id, rules));
    let mut folding = None;
    if let Some(raw_folding) = configuration.get("folding").filter(|v| is_truthy(v)) {
        let raw_markers = raw_folding.get("markers");
        let marker = |key: &str| {
            raw_markers.and_then(|m| m.get(key)).filter(|v| is_truthy(v)).and_then(|v| parse_regex(language_id, &format!("folding.markers.{key}"), v))
        };
        let (start, end) = (marker("start"), marker("end"));
        let markers = match (start, end) {
            (Some(start), Some(end)) => Some(FoldingMarkers { start, end }),
            _ => None,
        };
        folding = Some(FoldingRules { off_side: raw_folding.get("offSide").and_then(Value::as_bool), markers });
    }
    LanguageConfiguration {
        comments: extract_valid_comment_rule(language_id, configuration),
        brackets: extract_character_pairs(language_id, configuration, "brackets"),
        word_pattern,
        indentation_rules,
        on_enter_rules: extract_valid_on_enter_rules(language_id, configuration),
        auto_closing_pairs: extract_valid_auto_closing_pairs(language_id, configuration),
        surrounding_pairs: extract_valid_surrounding_pairs(language_id, configuration),
        colorized_bracket_pairs: extract_character_pairs(language_id, configuration, "colorizedBracketPairs"),
        auto_close_before: configuration.get("autoCloseBefore").and_then(Value::as_str).map(str::to_owned),
        folding,
        electric_character_support: None,
    }
}

/// `_readConfigFile(configFileLocation)`: um arquivo ilegível ou que não é objeto vira `{}`.
fn read_config_file(location: &URI) -> RawLanguageConfiguration {
    let path = location.fs_path(je_base::path::Platform::Posix).to_string_lossy();
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) => {
            eprintln!("{path}: {err}");
            return Value::Object(Map::new());
        }
    };
    match jsonc::parse(&contents) {
        Ok(configuration) if configuration.is_object() => configuration,
        Ok(_) => {
            eprintln!("{path}: Invalid format, JSON object expected.");
            Value::Object(Map::new())
        }
        Err(err) => {
            eprintln!("Errors parsing {path}: {err}");
            Value::Object(Map::new())
        }
    }
}

/// `LanguageConfigurationFileHandler`.
pub struct LanguageConfigurationFileHandler {
    base: Rc<Disposable>,
    language_service: Rc<dyn ILanguageService>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    /// Para cada linguagem, os arquivos já carregados (o `hash` das URIs do original).
    done: RefCell<HashMap<String, Vec<String>>>,
    registrations: Rc<DisposableStore>,
}

impl LanguageConfigurationFileHandler {
    pub fn new(
        language_service: Rc<dyn ILanguageService>,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let registrations = DisposableStore::new();
        base.register(registrations.clone());
        let handler = Rc::new(Self {
            base,
            language_service,
            language_configuration_service,
            done: RefCell::new(HashMap::new()),
            registrations,
        });

        let weak = Rc::downgrade(&handler);
        handler.base.register(handler.language_service.on_did_request_basic_language_features().listen(move |language_id| {
            if let Some(handler) = weak.upgrade() {
                handler.load_configurations_for_mode(language_id);
            }
        }));
        let weak = Rc::downgrade(&handler);
        handler.base.register(handler.language_service.on_did_change().listen(move |_| {
            let Some(handler) = weak.upgrade() else { return };
            let language_ids: Vec<String> = handler.done.borrow().keys().cloned().collect();
            for language_id in language_ids {
                handler.load_configurations_for_mode(&language_id);
            }
        }));
        handler
    }

    /// `_loadConfigurationsForMode(languageId)`.
    pub fn load_configurations_for_mode(&self, language_id: &str) {
        let configuration_files = self.language_service.get_configuration_files(language_id);
        let configuration_hash: Vec<String> = configuration_files.iter().map(|uri| uri.to_string(false).to_string_lossy()).collect();

        if self.done.borrow().get(language_id) == Some(&configuration_hash) {
            return;
        }
        self.done.borrow_mut().insert(language_id.to_owned(), configuration_hash);

        let configs: Vec<RawLanguageConfiguration> = configuration_files.iter().map(read_config_file).collect();
        for config in configs {
            self.handle_config(language_id, &config);
        }
    }

    /// `_handleConfig(languageId, configuration)`.
    fn handle_config(&self, language_id: &str, configuration: &RawLanguageConfiguration) {
        let rich_edit_config = extract_valid_config(language_id, configuration);
        let registration = self.language_configuration_service.register(language_id, rich_edit_config, Some(50));
        self.registrations.add(registration);
    }
}

impl IDisposable for LanguageConfigurationFileHandler {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}
