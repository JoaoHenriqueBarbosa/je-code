//! Os codicons do VS Code (`src/vs/base/common/codicons.ts`): a fonte e o code point de cada id.

mod table;

/// Os bytes de `codicon.ttf`.
pub static FONT: &[u8] = include_bytes!("../assets/codicon.ttf");

/// O code point do codicon `id` (`Codicon.<nome>.id` no original), ou `None` se o id não existe.
pub fn code_point(id: &str) -> Option<char> {
    table::CODICONS.iter().find(|(name, _)| *name == id).and_then(|(_, code)| char::from_u32(*code))
}

/// Todos os ids, na ordem de registro do original.
pub fn ids() -> impl Iterator<Item = &'static str> {
    table::CODICONS.iter().map(|(name, _)| *name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_conhecidos() {
        assert_eq!(super::code_point("add"), Some('\u{ea60}'));
        assert_eq!(super::code_point("files"), super::code_point("explorer-view-icon").or(super::code_point("files")));
        assert_eq!(super::code_point("dialog-error"), super::code_point("error"));
        assert_eq!(super::code_point("nao-existe"), None);
    }
}
