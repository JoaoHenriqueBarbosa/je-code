//! Porte de `tokenizeToString` e `_tokenizeToString` de `src/vs/editor/common/languages/textToHtmlTokenizer.ts`: o texto
//! tokenizado como HTML (`<div class="monaco-tokenized-source">` com um `<span class="...">` por token).
//!
//! Divergências:
//! - O `languageService.languageIdCodec` é o argumento `codec` (o `ILanguageService` o entrega com `language_id_codec()`).
//! - `tokenizeToStringSync` e `tokenizeLineToHTML` não estão aqui: nenhum chamador do porte os usa.
//! - O `fallback` do original (`getInitialState: () => NullState`, `tokenizeEncoded` nulo) é a ausência de suporte
//!   (`None`): o estado inicial é o [`null_state`] e a linha sai do [`null_tokenize_encoded`] com o idioma nulo.

use std::rc::Rc;

use je_base::JsString;
use je_base::promise::Promise;
use je_base::strings::{escape, split_lines};

use crate::language_service::ILanguageService;
use crate::languages::{ILanguageIdCodec, ITokenizationSupport};
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::null_tokenize::{null_state, null_tokenize_encoded};
use crate::tokenization_registry::{SupportRef, tokenization_registry};

/// `LanguageId.Null`.
const LANGUAGE_ID_NULL: u32 = 0;

fn push_str(out: &mut JsString, text: &str) {
    out.push_units(&text.encode_utf16().collect::<Vec<u16>>());
}

/// `tokenizeToString(languageService, text, languageId)`.
pub fn tokenize_to_string(language_service: &dyn ILanguageService, text: &JsString, language_id: Option<&str>) -> Promise<JsString> {
    let codec = language_service.language_id_codec();
    let text = text.clone();
    let Some(language_id) = language_id.filter(|language_id| !language_id.is_empty()) else {
        return Promise::resolved(tokenize_to_string_with_support(&text, &codec, None));
    };
    tokenization_registry().get_or_create(language_id).then(move |support| tokenize_to_string_with_support(&text, &codec, support.as_ref()))
}

/// `_tokenizeToString(text, languageIdCodec, tokenizationSupport)`; `None` é o `fallback`.
pub fn tokenize_to_string_with_support(text: &JsString, codec: &Rc<dyn ILanguageIdCodec>, support: Option<&SupportRef>) -> JsString {
    let mut result = JsString::new();
    push_str(&mut result, "<div class=\"monaco-tokenized-source\">");
    let lines = split_lines(text.units());
    let mut current_state = support.map_or_else(null_state, |support| support.get_initial_state());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            push_str(&mut result, "<br/>");
        }

        let line_text = JsString::from(*line);
        let mut tokenization_result = match support {
            Some(support) => support.tokenize_encoded(&line_text, true, &current_state),
            None => null_tokenize_encoded(LANGUAGE_ID_NULL, Some(current_state.clone())),
        };
        LineTokens::convert_to_end_offset(&mut tokenization_result.tokens, line.len());
        let line_tokens = LineTokens::new(tokenization_result.tokens.clone(), line_text, Rc::clone(codec));
        let view_line_tokens = line_tokens.inflate();

        let mut start_offset = 0usize;
        for j in 0..view_line_tokens.get_count() {
            let class_name = view_line_tokens.get_class_name(j);
            let end_index = view_line_tokens.get_end_offset(j) as usize;
            push_str(&mut result, &format!("<span class=\"{class_name}\">"));
            result.push_units(escape(&line[start_offset..end_index]).units());
            push_str(&mut result, "</span>");
            start_offset = end_index;
        }

        current_state = tokenization_result.end_state;
    }
    push_str(&mut result, "</div>");
    result
}
