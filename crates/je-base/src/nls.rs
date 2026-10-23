//! Porte de `src/vs/nls.ts`: mensagens localizáveis.
//!
//! No VS Code, `localize(key, message, ...args)` devolve a tradução do language pack ativo ou a
//! mensagem padrão, e troca `{0}`, `{1}`... pelos argumentos. O carregamento de language packs entra
//! com a plataforma; até lá vale a mensagem padrão, que é exatamente o que o VS Code mostra em inglês.

/// Formata `{n}` como o `_format` do `nls.ts`: índice fora dos argumentos deixa o marcador intacto.
pub fn format(message: &str, args: &[&str]) -> String {
    if args.is_empty() {
        return message.to_owned();
    }
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let closes = digits > 0 && after.as_bytes().get(digits) == Some(&b'}');
        match after[..digits].parse::<usize>().ok().filter(|_| closes).and_then(|i| args.get(i)) {
            Some(arg) => {
                out.push_str(arg);
                rest = &after[digits + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `localize(key, message, ...args)`.
pub fn localize(_key: &str, message: &str, args: &[&str]) -> String {
    format(message, args)
}
