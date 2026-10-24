//! Gerado por `scripts/gen-keyword-lookup.py` a partir de `parser/Keywords.table`. Não editar à
//! mão. É o `KeywordLookup.h` do C++ (o `Lexer<T>::parseKeyword`) sem a árvore de `if`: ver o
//! comentário do gerador.

use crate::parser::parser_tokens::{AWAIT, BREAK, CASE, CATCH, CLASSTOKEN, CONSTTOKEN, CONTINUE, DEBUGGER, DEFAULT, DELETETOKEN, DO, ELSE, EXPORT_, EXTENDS, FALSETOKEN, FINALLY, FOR, FUNCTION, IF, IMPORT, INSTANCEOF, INTOKEN, LET, NEW, NULLTOKEN, RESERVED, RESERVED_IF_STRICT, RETURN, SUPER, SWITCH, THISTOKEN, THROW, TRUETOKEN, TRY, TYPEOF, VAR, VOIDTOKEN, WHILE, WITH, YIELD, JSTokenType};

/// `maxTokenLength`: o chamador só consulta a tabela quando restam pelo menos tantos caracteres.
pub const MAX_TOKEN_LENGTH: usize = 10;

/// Uma linha do `Keywords.table`, com o nome do `propertyNames->xxxKeyword` que o C++ devolve em
/// `data->ident`.
pub struct Keyword {
    pub word: &'static [u8],
    pub token: JSTokenType,
    pub property_name: &'static str,
}

/// As linhas do `Keywords.table`, na ordem do arquivo.
pub static KEYWORDS: &[Keyword] = &[
    Keyword { word: b"null", token: NULLTOKEN, property_name: "nullKeyword" },
    Keyword { word: b"true", token: TRUETOKEN, property_name: "trueKeyword" },
    Keyword { word: b"false", token: FALSETOKEN, property_name: "falseKeyword" },
    Keyword { word: b"await", token: AWAIT, property_name: "awaitKeyword" },
    Keyword { word: b"break", token: BREAK, property_name: "breakKeyword" },
    Keyword { word: b"case", token: CASE, property_name: "caseKeyword" },
    Keyword { word: b"catch", token: CATCH, property_name: "catchKeyword" },
    Keyword { word: b"class", token: CLASSTOKEN, property_name: "classKeyword" },
    Keyword { word: b"const", token: CONSTTOKEN, property_name: "constKeyword" },
    Keyword { word: b"default", token: DEFAULT, property_name: "defaultKeyword" },
    Keyword { word: b"extends", token: EXTENDS, property_name: "extendsKeyword" },
    Keyword { word: b"finally", token: FINALLY, property_name: "finallyKeyword" },
    Keyword { word: b"for", token: FOR, property_name: "forKeyword" },
    Keyword { word: b"instanceof", token: INSTANCEOF, property_name: "instanceofKeyword" },
    Keyword { word: b"new", token: NEW, property_name: "newKeyword" },
    Keyword { word: b"var", token: VAR, property_name: "varKeyword" },
    Keyword { word: b"let", token: LET, property_name: "letKeyword" },
    Keyword { word: b"continue", token: CONTINUE, property_name: "continueKeyword" },
    Keyword { word: b"function", token: FUNCTION, property_name: "functionKeyword" },
    Keyword { word: b"return", token: RETURN, property_name: "returnKeyword" },
    Keyword { word: b"void", token: VOIDTOKEN, property_name: "voidKeyword" },
    Keyword { word: b"delete", token: DELETETOKEN, property_name: "deleteKeyword" },
    Keyword { word: b"if", token: IF, property_name: "ifKeyword" },
    Keyword { word: b"this", token: THISTOKEN, property_name: "thisKeyword" },
    Keyword { word: b"do", token: DO, property_name: "doKeyword" },
    Keyword { word: b"while", token: WHILE, property_name: "whileKeyword" },
    Keyword { word: b"else", token: ELSE, property_name: "elseKeyword" },
    Keyword { word: b"in", token: INTOKEN, property_name: "inKeyword" },
    Keyword { word: b"super", token: SUPER, property_name: "superKeyword" },
    Keyword { word: b"switch", token: SWITCH, property_name: "switchKeyword" },
    Keyword { word: b"throw", token: THROW, property_name: "throwKeyword" },
    Keyword { word: b"try", token: TRY, property_name: "tryKeyword" },
    Keyword { word: b"typeof", token: TYPEOF, property_name: "typeofKeyword" },
    Keyword { word: b"with", token: WITH, property_name: "withKeyword" },
    Keyword { word: b"debugger", token: DEBUGGER, property_name: "debuggerKeyword" },
    Keyword { word: b"yield", token: YIELD, property_name: "yieldKeyword" },
    Keyword { word: b"enum", token: RESERVED, property_name: "enumKeyword" },
    Keyword { word: b"export", token: EXPORT_, property_name: "exportKeyword" },
    Keyword { word: b"import", token: IMPORT, property_name: "importKeyword" },
    Keyword { word: b"implements", token: RESERVED_IF_STRICT, property_name: "implementsKeyword" },
    Keyword { word: b"interface", token: RESERVED_IF_STRICT, property_name: "interfaceKeyword" },
    Keyword { word: b"package", token: RESERVED_IF_STRICT, property_name: "packageKeyword" },
    Keyword { word: b"private", token: RESERVED_IF_STRICT, property_name: "privateKeyword" },
    Keyword { word: b"protected", token: RESERVED_IF_STRICT, property_name: "protectedKeyword" },
    Keyword { word: b"public", token: RESERVED_IF_STRICT, property_name: "publicKeyword" },
    Keyword { word: b"static", token: RESERVED_IF_STRICT, property_name: "staticKeyword" },
];

/// O corpo do `parseKeyword`: a palavra-chave no começo de `code`, se houver, com `next_ends` dizendo
/// se o caractere logo depois dela é um `cannotBeIdentPartOrEscapeStart`. O chamador faz o
/// `internalShift` do tamanho da palavra e preenche `data->ident`.
pub fn match_keyword<C: Copy + Into<u32>>(
    code: &[C],
    next_ends: impl Fn(C) -> bool,
) -> Option<&'static Keyword> {
    debug_assert!(code.len() >= MAX_TOKEN_LENGTH);
    KEYWORDS.iter().find(|k| {
        // O C++ lê `code[len]` mesmo quando a palavra termina exatamente no fim do buffer (palavra de
        // `maxTokenLength` letras, como `instanceof`); o bun acusa a palavra-chave, então o byte
        // depois do fim conta como quem encerra um identificador.
        code.len() >= k.word.len()
            && k.word.iter().zip(code).all(|(&w, &c)| c.into() == w as u32)
            && (code.len() == k.word.len() || next_ends(code[k.word.len()]))
    })
}
