//! Porte de `src/vs/editor/contrib/snippet` do VS Code: o que o editor sabe fazer com snippets.
//!
//! Esta fatia traz o analisador (`browser/snippetParser.ts`), os resolvedores de variáveis
//! (`browser/snippetVariables.ts`) e a sessão de snippet no editor (`browser/snippetSession.ts`).

pub mod snippet_parser;

pub mod snippet_session;
pub mod snippet_variables;
