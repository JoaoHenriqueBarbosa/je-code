//! `parser/LexerUnicodeProperties.{h,cpp}` com a tabela gerada `LexerUnicodePropertyTables.h`
//! (`generateLexerUnicodePropertyTables.py` sobre o `ucd/` 17.0 do JavaScriptCore). O
//! `isNonLatin1WhiteSpace` mora no `ul_common::js_number`, junto do `jsToNumber` que também o usa.

pub use ul_common::js_number::is_non_latin1_white_space;
