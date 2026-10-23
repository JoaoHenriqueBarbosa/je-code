//! As linguagens do editor: as contribuições `languages` e `grammars` das extensões embutidas, a tokenização
//! TextMate (`ferriki-textmate`) ligada ao `TokenizationRegistry` do `je-editor`, o tema de cores de token e o
//! carregamento das configurações de linguagem.
//!
//! Por que um crate separado do `je-editor`: o `je-editor` porta `vs/editor/common` (puro, sem E/S e sem
//! dependência de gramática); este crate porta o que no VS Code mora em `vs/workbench/services` (leitura dos
//! `package.json`, arquivos de gramática, `TextMateTokenizationFeature`) e arrasta o `ferriki-textmate` e o
//! `ferroni`. Quem só quer o modelo de texto não paga por isso.

pub mod extensions;
pub mod jsonc;
pub mod language_configuration_file;
pub mod text_mate_tokenization_feature;
pub mod text_mate_tokenization_support;
pub mod token_theme;
