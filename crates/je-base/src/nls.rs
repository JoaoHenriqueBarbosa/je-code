//! Porte de `src/vs/nls.ts`: mensagens localizáveis.
//!
//! No VS Code, `localize(key, message, ...args)` devolve a tradução do language pack ativo ou a
//! mensagem padrão, e troca `{0}`, `{1}`... pelos argumentos. O carregamento de language packs entra
//! com a plataforma; até lá vale a mensagem padrão, que é exatamente o que o VS Code mostra em inglês.
//!
//! Decisões de tradução:
//! - os argumentos do original são `string | number | boolean | undefined | null`: [`NlsArg`]. Um valor de
//!   outro tipo (um objeto, por exemplo, que o chamador força com `as unknown as string`) é
//!   [`NlsArg::Unreplaceable`], e o marcador dele fica intacto. [`localize`] e [`format`] recebem só
//!   textos (`&[&str]`), o caso de quase todo chamador; [`localize_args`] e [`format_args`] recebem
//!   [`NlsArg`];
//! - um índice sem argumento (`{3}` com dois argumentos) vira `undefined`, como o `String(args[index])` do
//!   original, e não deixa o marcador;
//! - o modo pseudo (`_VSCODE_NLS_LANGUAGE === 'pseudo'` ou `#pseudo=true` na URL do documento) e a busca
//!   por índice na tabela global de mensagens (`getNLSMessages`, `lookupMessage`, só no VS Code compilado)
//!   não existem aqui, pois o porte não tem essas globais; a chave de [`localize_args`] é o ponto onde a
//!   busca de language pack entra.

use crate::dynamic::number_to_js_string;

/// Um argumento de `localize`: `string | number | boolean | undefined | null`.
#[derive(Clone, Debug, PartialEq)]
pub enum NlsArg {
    Str(String),
    Number(f64),
    Bool(bool),
    Undefined,
    Null,
    /// Qualquer outro valor: o marcador não é substituído.
    Unreplaceable,
}

impl NlsArg {
    /// O texto que entra no lugar do marcador, ou `None` quando o marcador fica.
    fn replacement(&self) -> Option<String> {
        match self {
            NlsArg::Str(text) => Some(text.clone()),
            NlsArg::Number(n) => Some(number_to_js_string(*n)),
            NlsArg::Bool(b) => Some(b.to_string()),
            NlsArg::Undefined => Some("undefined".to_owned()),
            NlsArg::Null => Some("null".to_owned()),
            NlsArg::Unreplaceable => None,
        }
    }
}

/// `ILocalizedString`.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalizedString {
    pub original: String,
    pub value: String,
}

/// Formata `{n}` como o `_format` do `nls.ts`: sem argumentos a mensagem passa como está; com argumentos,
/// cada `{n}` vira o texto do n-ésimo (`undefined` se não existe) e o marcador de um argumento que não é
/// texto, número, booleano, `undefined` nem `null` fica intacto.
pub fn format_args(message: &str, args: &[NlsArg]) -> String {
    if args.is_empty() {
        return message.to_owned();
    }
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && after.as_bytes().get(digits) == Some(&b'}') {
            // um índice que não cabe em `usize` também não existe nos argumentos
            let replacement = match after[..digits].parse::<usize>().ok().and_then(|index| args.get(index)) {
                Some(arg) => arg.replacement(),
                None => NlsArg::Undefined.replacement(),
            };
            match replacement {
                Some(text) => out.push_str(&text),
                None => {
                    out.push('{');
                    out.push_str(&after[..=digits]);
                }
            }
            rest = &after[digits + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// [`format_args`] com os argumentos em texto.
pub fn format(message: &str, args: &[&str]) -> String {
    format_args(message, &to_nls_args(args))
}

fn to_nls_args(args: &[&str]) -> Vec<NlsArg> {
    args.iter().map(|arg| NlsArg::Str((*arg).to_owned())).collect()
}

/// `localize(key, message, ...args)`.
pub fn localize_args(_key: &str, message: &str, args: &[NlsArg]) -> String {
    format_args(message, args)
}

/// `localize(key, message, ...args)` com os argumentos em texto.
pub fn localize(key: &str, message: &str, args: &[&str]) -> String {
    localize_args(key, message, &to_nls_args(args))
}

/// `localize2(key, message, ...args)`: o valor localizado e o texto original (formatado com os mesmos
/// argumentos). Sem tradução ativa os dois são iguais, que é o ramo `originalMessage === message` do original.
pub fn localize2(_key: &str, original_message: &str, args: &[NlsArg]) -> LocalizedString {
    let value = format_args(original_message, args);
    LocalizedString { original: value.clone(), value }
}
