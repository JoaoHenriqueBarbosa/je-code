//! Porte do `minimist` 1.2.8, o analisador de argv que o `parseArgs` de `platform/environment/node/argv.ts`
//! usa. Só as opções que o VS Code passa (`string`, `boolean` e `alias`); `unknown`, `default`, `stopEarly` e
//! `'--'` do original não são usadas pelo VS Code e não foram portadas.
//!
//! O resultado é um objeto dinâmico com a ordem de propriedades do JavaScript: as chaves aparecem na ordem
//! em que o `minimist` as cria (os booleanos declarados primeiro, `_` depois), e `a.b` vira um objeto
//! aninhado. Valores que parecem número viram número, a não ser que a chave esteja em `string`.

use std::collections::{HashMap, HashSet};

use je_base::JsString;
use je_base::dynamic::{ArrayRef, Dynamic, ObjectRef};

/// As opções do `minimist` que o VS Code usa.
#[derive(Clone, Debug, Default)]
pub struct MinimistOptions {
    pub string: Vec<String>,
    pub boolean: Vec<String>,
    /// `alias: { chave: 'apelido' }`, na ordem de declaração.
    pub alias: Vec<(String, String)>,
}

/// `isNumber(x)` para um texto.
fn is_number(x: &str) -> bool {
    // /^0x[0-9a-f]+$/i
    if let Some(hex) = x.strip_prefix("0x").or_else(|| x.strip_prefix("0X"))
        && !hex.is_empty()
        && hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return true;
    }
    matches_number(x)
}

/// `/^[-+]?(?:\d+(?:\.\d*)?|\.\d+)(e[-+]?\d+)?$/`.
fn matches_number(x: &str) -> bool {
    let b = x.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let has_int = i > int_start;
    if has_int {
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    } else {
        if i >= b.len() || b[i] != b'.' {
            return false;
        }
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac_start {
            return false;
        }
    }
    if i < b.len() && b[i] == b'e' {
        i += 1;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == b.len()
}

/// `Number(text)` de um texto que passou em [`is_number`].
fn to_number(x: &str) -> f64 {
    if let Some(hex) = x.strip_prefix("0x").or_else(|| x.strip_prefix("0X")) {
        return u128::from_str_radix(hex, 16).map_or(f64::NAN, |n| n as f64);
    }
    x.parse().unwrap_or(f64::NAN)
}

/// `/-?\d+(\.\d*)?(e-?\d+)?$/.test(text)`: o texto termina em algo com a forma de um número.
fn ends_with_number(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len()).any(|start| {
        let tail: String = chars[start..].iter().collect();
        let b = tail.as_bytes();
        let mut i = 0;
        if i < b.len() && b[i] == b'-' {
            i += 1;
        }
        let digits = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits {
            return false;
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i < b.len() && b[i] == b'e' {
            let mut j = i + 1;
            if j < b.len() && b[j] == b'-' {
                j += 1;
            }
            let exp = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > exp {
                i = j;
            }
        }
        i == b.len()
    })
}

/// `/^(-|--)[^-]/.test(arg)`: começa com uma opção.
fn looks_like_option(arg: &str) -> bool {
    let chars: Vec<char> = arg.chars().take(3).collect();
    match chars.as_slice() {
        ['-', second, ..] if *second != '-' => true,
        ['-', '-', third, ..] => *third != '-',
        _ => false,
    }
}

fn is_true_or_false(arg: &str) -> bool {
    arg == "true" || arg == "false"
}

fn key(name: &str) -> JsString {
    JsString::from(name)
}

struct Flags {
    bools: HashSet<String>,
    strings: HashSet<String>,
    aliases: HashMap<String, Vec<String>>,
}

impl Flags {
    fn alias_is_boolean(&self, name: &str) -> bool {
        self.aliases.get(name).is_some_and(|list| list.iter().any(|x| self.bools.contains(x)))
    }

    /// `aliases[key] ? !aliasIsBoolean(key) : true`.
    fn alias_allows_value(&self, name: &str) -> bool {
        !self.aliases.contains_key(name) || !self.alias_is_boolean(name)
    }
}

/// `setKey(obj, keys, value)`.
fn set_key(flags: &Flags, obj: &ObjectRef, keys: &[&str], value: Dynamic) {
    let mut o = obj.clone();
    for name in &keys[..keys.len() - 1] {
        if is_constructor_or_proto(name) {
            return;
        }
        let existing = o.get(&key(name));
        let next = match existing {
            None | Some(Dynamic::Undefined) => {
                let created = ObjectRef::new();
                o.set(&key(name), Dynamic::Object(created.clone()));
                created
            }
            Some(Dynamic::Object(existing)) => existing,
            Some(_) => return,
        };
        o = next;
    }

    let last_key = keys[keys.len() - 1];
    if is_constructor_or_proto(last_key) {
        return;
    }
    let current = o.get(&key(last_key)).unwrap_or(Dynamic::Undefined);
    if current.is_undefined() || flags.bools.contains(last_key) || matches!(current, Dynamic::Bool(_)) {
        o.set(&key(last_key), value);
    } else if let Dynamic::Array(array) = &current {
        array.push(value);
    } else {
        o.set(&key(last_key), Dynamic::Array(ArrayRef::new(vec![current, value])));
    }
}

fn is_constructor_or_proto(name: &str) -> bool {
    name == "constructor" || name == "__proto__"
}

fn set_arg(flags: &Flags, argv: &ObjectRef, name: &str, value: Dynamic) {
    let value = match &value {
        Dynamic::String(text) if !flags.strings.contains(name) && is_number(&text.to_string_lossy()) => Dynamic::Number(to_number(&text.to_string_lossy())),
        _ => value,
    };
    let parts: Vec<&str> = name.split('.').collect();
    set_key(flags, argv, &parts, value.clone());

    for alias in flags.aliases.get(name).into_iter().flatten() {
        let parts: Vec<&str> = alias.split('.').collect();
        set_key(flags, argv, &parts, value.clone());
    }
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

/// `minimist(args, opts)`.
pub fn minimist(args: &[String], opts: &MinimistOptions) -> ObjectRef {
    let mut flags = Flags { bools: HashSet::new(), strings: HashSet::new(), aliases: HashMap::new() };
    let mut bool_order: Vec<String> = Vec::new();
    for name in &opts.boolean {
        if flags.bools.insert(name.clone()) {
            bool_order.push(name.clone());
        }
    }

    for (name, alias) in &opts.alias {
        let list = vec![alias.clone()];
        flags.aliases.insert(name.clone(), list.clone());
        for x in &list {
            let others: Vec<String> = std::iter::once(name.clone()).chain(list.iter().filter(|y| *y != x).cloned()).collect();
            flags.aliases.insert(x.clone(), others);
        }
    }

    for name in &opts.string {
        flags.strings.insert(name.clone());
        if let Some(aliases) = flags.aliases.get(name).cloned() {
            for alias in aliases {
                flags.strings.insert(alias);
            }
        }
    }

    let argv = ObjectRef::new();
    argv.set(&key("_"), Dynamic::Array(ArrayRef::new(Vec::new())));
    let positional = || match argv.get(&key("_")) {
        Some(Dynamic::Array(array)) => array,
        _ => unreachable!("`_` é sempre um array"),
    };

    for name in &bool_order {
        set_arg(&flags, &argv, name, Dynamic::Bool(false));
    }

    let mut not_flags: Vec<String> = Vec::new();
    let mut args: Vec<String> = args.to_vec();
    if let Some(index) = args.iter().position(|a| a == "--") {
        not_flags = args.split_off(index + 1);
        args.pop();
    }

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        let next = args.get(i + 1).cloned();

        if arg.starts_with("--") && arg.char_indices().any(|(index, c)| c == '=' && index >= 3) {
            // /^--.+=/
            let rest = &arg[2..];
            let eq = rest.find('=').expect("há um `=`");
            let name = &rest[..eq];
            let mut value = text(&rest[eq + 1..]);
            if flags.bools.contains(name) {
                value = Dynamic::Bool(&rest[eq + 1..] != "false");
            }
            set_arg(&flags, &argv, name, value);
        } else if arg.starts_with("--no-") && arg.len() > 5 {
            set_arg(&flags, &argv, &arg[5..], Dynamic::Bool(false));
        } else if arg.starts_with("--") && arg.len() > 2 {
            let name = &arg[2..];
            if let Some(next) = next.as_deref().filter(|next| !looks_like_option(next) && !flags.bools.contains(name) && flags.alias_allows_value(name)) {
                set_arg(&flags, &argv, name, text(next));
                i += 1;
            } else if let Some(next) = next.as_deref().filter(|next| is_true_or_false(next)) {
                set_arg(&flags, &argv, name, Dynamic::Bool(next == "true"));
                i += 1;
            } else {
                set_arg(&flags, &argv, name, if flags.strings.contains(name) { text("") } else { Dynamic::Bool(true) });
            }
        } else if arg.starts_with('-') && arg.chars().nth(1).is_some_and(|c| c != '-') {
            let chars: Vec<char> = arg.chars().collect();
            let letters: &[char] = &chars[1..chars.len() - 1];

            let mut broken = false;
            for j in 0..letters.len() {
                let next_text: String = chars[j + 2..].iter().collect();
                let letter = letters[j].to_string();

                if next_text == "-" {
                    set_arg(&flags, &argv, &letter, text(&next_text));
                    continue;
                }

                if letters[j].is_ascii_alphabetic() && next_text.starts_with('=') {
                    set_arg(&flags, &argv, &letter, text(&next_text[1..]));
                    broken = true;
                    break;
                }

                if letters[j].is_ascii_alphabetic() && ends_with_number(&next_text) {
                    set_arg(&flags, &argv, &letter, text(&next_text));
                    broken = true;
                    break;
                }

                if letters.get(j + 1).is_some_and(|c| !(c.is_ascii_alphanumeric() || *c == '_')) {
                    let rest: String = chars[j + 2..].iter().collect();
                    set_arg(&flags, &argv, &letter, text(&rest));
                    broken = true;
                    break;
                }

                set_arg(&flags, &argv, &letter, if flags.strings.contains(&letter) { text("") } else { Dynamic::Bool(true) });
            }

            let last = chars[chars.len() - 1].to_string();
            if !broken && last != "-" {
                let following = args.get(i + 1).filter(|a| !a.is_empty());
                if let Some(following) = following.filter(|a| !looks_like_option(a) && !flags.bools.contains(&last) && flags.alias_allows_value(&last)) {
                    set_arg(&flags, &argv, &last, text(following));
                    i += 1;
                } else if let Some(following) = following.filter(|a| is_true_or_false(a)) {
                    set_arg(&flags, &argv, &last, Dynamic::Bool(following == "true"));
                    i += 1;
                } else {
                    set_arg(&flags, &argv, &last, if flags.strings.contains(&last) { text("") } else { Dynamic::Bool(true) });
                }
            }
        } else {
            let value = if flags.strings.contains("_") || !is_number(&arg) { text(&arg) } else { Dynamic::Number(to_number(&arg)) };
            positional().push(value);
        }
        i += 1;
    }

    for not_flag in not_flags {
        positional().push(text(&not_flag));
    }

    argv
}
