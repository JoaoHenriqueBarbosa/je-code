//! Porte de `extractRangeFromFilter` de `src/vs/workbench/contrib/search/common/search.ts`: separa de um filtro
//! de arquivo (`/caminho/arquivo.txt:20:3-40:5`) o caminho e o intervalo de linha e coluna.
//!
//! Notas de tradução:
//! - O resto de `search.ts` (`getOutOfWorkspaceEditorResources`, `SearchUIState`, `SearchStateKey` e as
//!   consultas `ISearchConfiguration`) depende do editor, do contexto e dos serviços de arquivo e entra quando
//!   o painel de busca for portado. Aqui fica o que o `extractRange.test.ts` exercita.
//! - `IRange` é o [`Range`] do je-editor.
//! - O filtro entra como `&str` e sai como `String`; os índices da regex são em unidades UTF-16, como no
//!   original, e o corte do `substr(0, index)` cai sempre antes de um caractere ASCII (`#`, `:`, `(` ou espaço),
//!   então nunca divide um par substituto.
//! - `parseInt(group, 10)` sobre `\d*`: vazio é `NaN` (aqui `None`); um número maior que `i32::MAX` satura,
//!   porque o [`Range`] guarda `i32`.
//! - `LINE_COLON_PATTERN` roda no `zyarr`, em `thread_local!` dentro de um [`Lazy`] (o `Lazy` não é `Sync`).

use std::rc::Rc;

use je_base::js_string::{index_of, substring};
use je_base::lazy::Lazy;
use je_editor::range::Range;
use zyarr::RegExp;

thread_local! {
    /// Padrões aceitos: `<caminho><#|:|(><linha><#|:|,><coluna?>>`, opcionalmente seguidos de um intervalo
    /// `<-<linhaFinal><#|:|,><colunaFinal?>>`.
    static LINE_COLON_PATTERN: Lazy<Rc<RegExp>> = Lazy::new(|| {
        Rc::new(
            RegExp::from_str(r"\s?[#:\(](?:line )?(\d*)(?:[#:,](\d*))?(?:-(\d*)(?:[#:,](\d*))?)?\)?:?\s*$", "")
                .expect("a regex do extractRangeFromFilter é válida"),
        )
    });
}

/// `IFilterAndRange`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterAndRange {
    pub filter: String,
    pub range: Range,
}

/// `parseInt(text, 10)` de um grupo `\d*`: `None` é o `NaN` de um grupo vazio ou ausente.
fn parse_digits(group: Option<&[u16]>) -> Option<i32> {
    let digits = group.filter(|digits| !digits.is_empty())?;
    let value = digits.iter().fold(0.0_f64, |acc, &unit| acc * 10.0 + f64::from(unit - u16::from(b'0')));
    Some(value.min(f64::from(i32::MAX)) as i32)
}

pub fn extract_range_from_filter(filter: &str, unless: Option<&[&str]>) -> Option<FilterAndRange> {
    let units: Vec<u16> = filter.encode_utf16().collect();
    let line_colon_pattern = LINE_COLON_PATTERN.with(Lazy::value);

    // Ignore when the unless character not the first character or is before the line colon pattern
    if units.is_empty()
        || unless.is_some_and(|unless| {
            unless.iter().any(|value| {
                let value_units: Vec<u16> = value.encode_utf16().collect();
                match index_of(&units, &value_units, 0) {
                    Some(0) => true,
                    Some(unless_char_pos) => {
                        let after = substring(&units, unless_char_pos as i64 + 1, units.len() as i64);
                        line_colon_pattern.exec(after, 0).is_none()
                    }
                    None => false,
                }
            })
        })
    {
        return None;
    }

    // Find Line/Column number from search value using RegExp
    let pattern_match = line_colon_pattern.exec(&units, 0)?;
    let group = |n: usize| pattern_match.captures.get(n).and_then(Option::as_ref).map(|range| &units[range.clone()]);

    let range = if let Some(start_line_number) = parse_digits(group(1)) {
        // Line Number
        let mut range = Range::new(start_line_number, 1, start_line_number, 1);

        // Column Number
        if let Some(start_column) = parse_digits(group(2)) {
            range = Range::new(range.start_line_number, start_column, range.end_line_number, start_column);
        }

        // End Line Number (range selection, e.g. "20-40")
        if let Some(end_line_number) = parse_digits(group(3)) {
            // End Column Number (e.g. "20:3-40:5"), defaults to the start of the end line
            let end_column = parse_digits(group(4)).unwrap_or(1);
            range = Range::new(range.start_line_number, range.start_column, end_line_number, end_column);
        }
        range
    } else if group(1).is_some_and(<[u16]>::is_empty) {
        // User has typed "something:" or "something#" without a line number, in this case treat as start of file
        Range::new(1, 1, 1, 1)
    } else {
        return None;
    };

    Some(FilterAndRange {
        // clear range suffix from search value
        filter: String::from_utf16_lossy(&units[..pattern_match.start().min(units.len())]),
        range,
    })
}
