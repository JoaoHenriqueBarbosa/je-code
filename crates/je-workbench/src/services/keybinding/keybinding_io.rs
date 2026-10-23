//! Porte de `src/vs/workbench/services/keybinding/common/keybindingIO.ts`: a leitura e a escrita de um item do
//! `keybindings.json` do usuário.
//!
//! Notas de tradução:
//! - O `Object` que `readUserKeybindingItem` recebe é um [`Dynamic`] (o `JSON.parse` do arquivo do usuário), e
//!   `'key' in input` é [`Dynamic::has_property`].
//! - `JSON.stringify` de um valor qualquer (os `args` do comando) é [`json_stringify`], com a semântica do
//!   JavaScript: `undefined` e função somem de objeto e viram `null` em array, número não finito vira `null`.
//! - `rightPaddedString` conta unidades UTF-16 (o `length` do JavaScript).

use je_base::dynamic::json_stringify;
use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy, json_quote};
use je_base::keybinding_parser::KeybindingParser;
use je_base::keybindings::Keybinding;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;

/// `IUserKeybindingItem`.
pub struct UserKeybindingItem {
    pub keybinding: Option<Keybinding>,
    pub command: Option<String>,
    pub command_args: Dynamic,
    pub when: Option<ContextKeyExpression>,
    pub system_wide: bool,
    /// Captura o campo `key` do `keybindings.json`: `keybinding` presente implica `source_key` presente.
    pub source_key: Option<String>,
}

/// `KeybindingIO`.
pub struct KeybindingIO;

impl KeybindingIO {
    pub fn write_keybinding_item(out: &mut OutputBuilder, item: &ResolvedKeybindingItem) {
        let Some(resolved_keybinding) = &item.resolved_keybinding else {
            return;
        };
        let quoted_serialized_keybinding = quote_optional(resolved_keybinding.get_user_settings_label());
        out.write(&format!(
            "{{ \"key\": {} \"command\": ",
            right_padded_string(&format!("{quoted_serialized_keybinding},"), 25)
        ));

        let quoted_serialized_when = item.when.as_ref().map(|when| quote(&when.serialize()));
        let quoted_serialize_command = quote_optional(item.command.clone());
        match quoted_serialized_when {
            Some(quoted_serialized_when) => {
                out.write(&format!("{quoted_serialize_command},"));
                out.write_line("");
                out.write(&format!("                                     \"when\": {quoted_serialized_when}"));
            }
            None => out.write(&quoted_serialize_command),
        }
        if item.command_args.is_truthy() {
            out.write(",");
            out.write_line("");
            let args = json_stringify(&item.command_args).unwrap_or_else(|| "undefined".to_string());
            out.write(&format!("                                     \"args\": {args}"));
        }
        if item.system_wide {
            out.write(",");
            out.write_line("");
            out.write("                                     \"systemWide\": true");
        }
        out.write(" }");
    }

    pub fn read_user_keybinding_item(input: &Dynamic) -> UserKeybindingItem {
        let key = string_property(input, "key");
        let keybinding = key.as_deref().and_then(KeybindingParser::parse_keybinding);
        let when = string_property(input, "when").and_then(|when| ContextKeyExpr::deserialize(Some(&when)));
        let command = string_property(input, "command");
        let command_args = if has_property(input, "args") && !input.get_str("args").is_undefined() {
            input.get_str("args")
        } else {
            Dynamic::Undefined
        };
        let system_wide = has_property(input, "systemWide") && matches!(input.get_str("systemWide"), Dynamic::Bool(true));
        UserKeybindingItem { keybinding, command, command_args, when, system_wide, source_key: key }
    }
}

fn has_property(input: &Dynamic, name: &str) -> bool {
    input.has_property(&JsString::from(name))
}

/// `'name' in input && typeof input.name === 'string' ? input.name : undefined`.
fn string_property(input: &Dynamic, name: &str) -> Option<String> {
    if !has_property(input, name) {
        return None;
    }
    match input.get_str(name) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `JSON.stringify(text)`.
fn quote(text: &str) -> String {
    json_quote(&JsString::from(text))
}

/// `JSON.stringify(value)` de um `string | null`.
fn quote_optional(text: Option<String>) -> String {
    text.map_or_else(|| "null".to_string(), |text| quote(&text))
}

fn right_padded_string(text: &str, min_chars: usize) -> String {
    let length = text.encode_utf16().count();
    if length < min_chars {
        // `new Array(n).join(' ')` tem `n - 1` espaços.
        return format!("{text}{}", " ".repeat(min_chars - length - 1));
    }
    text.to_string()
}

/// `OutputBuilder`.
#[derive(Default)]
pub struct OutputBuilder {
    lines: Vec<String>,
    current_line: String,
}

impl OutputBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write(&mut self, text: &str) {
        self.current_line.push_str(text);
    }

    pub fn write_line(&mut self, text: &str) {
        self.lines.push(format!("{}{text}", self.current_line));
        self.current_line.clear();
    }

    /// `toString`: fecha a linha corrente e junta as linhas com `\n`.
    pub fn to_string(&mut self) -> String {
        self.write_line("");
        self.lines.join("\n")
    }
}
