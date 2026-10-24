//! `vscode.l10n.t`: a mensagem com os argumentos `{0}`, `{1}` (posicionais) ou `{nome}` (objeto), sem pacote de
//! tradução carregado (o `l10n.t` do `extHost.api.impl.ts` devolve a própria mensagem formatada nesse caso).

use std::rc::Rc;

use zjsc::api::embed_node::JsData;

use crate::convert::field;

fn display(value: &JsData) -> String {
    match value {
        JsData::Str(text) => text.clone(),
        JsData::Num(number) => {
            if number.fract() == 0.0 && number.abs() < 1e15 { format!("{}", *number as i64) } else { number.to_string() }
        }
        JsData::Bool(flag) => flag.to_string(),
        JsData::Null => "null".to_owned(),
        JsData::Undefined => "undefined".to_owned(),
        JsData::Array(_) | JsData::Object(_) | JsData::Handle(_) => "[object Object]".to_owned(),
    }
}

/// `format(message, args)` do `l10n`: troca `{chave}` pelo argumento correspondente; chave sem argumento fica como está.
pub fn format_message(message: &str, positional: &[JsData], named: Option<&JsData>) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if after[..close].chars().all(|c| c.is_alphanumeric() || c == '_') && close > 0 => {
                let key = &after[..close];
                let value = match key.parse::<usize>() {
                    Ok(index) => positional.get(index),
                    Err(_) => named.and_then(|object| field(object, key)),
                };
                match value {
                    Some(value) => out.push_str(&display(value)),
                    None => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `l10n.t(message, ...args)`, `l10n.t(message, record)` e `l10n.t({ message, args, comment })`.
pub fn t(args: &[JsData]) -> Result<JsData, String> {
    match args.first() {
        Some(JsData::Str(message)) => match args.get(1) {
            Some(record @ JsData::Object(_)) if args.len() == 2 => Ok(JsData::Str(format_message(message, &[], Some(record)))),
            _ => Ok(JsData::Str(format_message(message, &args[1..], None))),
        },
        Some(options @ JsData::Object(_)) => {
            let Some(JsData::Str(message)) = field(options, "message") else { return Err("l10n.t: falta a mensagem".to_owned()) };
            match field(options, "args") {
                Some(JsData::Array(list)) => Ok(JsData::Str(format_message(message, list, None))),
                Some(record @ JsData::Object(_)) => Ok(JsData::Str(format_message(message, &[], Some(record)))),
                _ => Ok(JsData::Str(message.clone())),
            }
        }
        _ => Err("l10n.t: o primeiro argumento precisa ser texto ou { message }".to_owned()),
    }
}

/// O namespace `l10n` (`t`).
pub fn namespace(runner: &zjsc::api::embed_node::NodeRunner) -> JsData {
    let function = runner.make_function("t", 1, Rc::new(t));
    JsData::Object(vec![("t".to_owned(), JsData::Handle(Rc::new(function)))])
}
