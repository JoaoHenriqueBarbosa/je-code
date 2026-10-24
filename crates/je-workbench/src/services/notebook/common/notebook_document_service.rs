//! Porte parcial de `src/vs/workbench/services/notebook/common/notebookDocumentService.ts`: as funções que montam e
//! leem as URIs de célula, de metadados e de saída de célula (`parse`, `generate`, `parseMetadataUri`,
//! `generateMetadataUri`, `extractCellOutputDetails`, usadas por `CellUri` e `NotebookMetadataUri` de
//! `notebookCommon.ts`) e os contratos `INotebookDocument` e `INotebookDocumentService`. A
//! `NotebookDocumentWorkbenchService` (o `ResourceMap` de documentos e o `registerSingleton` atrasado) fica para a
//! fatia dos serviços de notebook.
//!
//! Divergências:
//! - `handle` é `i64` (o `number` do original, sempre inteiro). `handle.toString(7)` e `parseInt(texto, 7)` são
//!   [`to_radix_string`] e [`parse_int_radix`]; um `parseInt` que dá `NaN` (nenhum dígito válido) é `None`.
//! - Em `extractCellOutputDetails`, o `parseInt(..., 10)` de `outputIndex` e de `cellIndex` que dá `NaN` vira `None`
//!   em vez de um `number` `NaN`.
//! - `new URLSearchParams(uri.query)` é o [`UrlSearchParams`] do je-base. Os componentes da URI (`JsString`) são
//!   lidos como texto com perda: o fragmento que `generate` produz e o `query` dos `URLSearchParams` são ASCII.
//! - `getCellIndex(cellUri): number | undefined` é `Option<usize>`.

use std::rc::Rc;

use je_base::buffer::{VSBuffer, decode_base64, encode_base64};
use je_base::network::schemas;
use je_base::oauth::UrlSearchParams;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::service_id;

/// `INotebookDocument`.
pub trait INotebookDocument {
    fn uri(&self) -> URI;
    fn get_cell_index(&self, cell_uri: &URI) -> Option<usize>;
}

const LENGTHS: [char; 10] = ['W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e', 'f'];
const RADIX: u32 = 7;

/// O resultado de [`parse`]: o notebook e o handle da célula (`{ notebook: URI; handle: number }`).
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedCellUri {
    pub notebook: URI,
    pub handle: i64,
}

/// `Number#toString(radix)` para um inteiro.
fn to_radix_string(value: i64, radix: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut remaining = value.unsigned_abs();
    let mut digits: Vec<char> = Vec::new();
    while remaining > 0 {
        let digit = u32::try_from(remaining % u64::from(radix)).unwrap_or(0);
        digits.push(char::from_digit(digit, radix).unwrap_or('0'));
        remaining /= u64::from(radix);
    }
    if value < 0 {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

/// `parseInt(text, radix)`: lê o sinal e os dígitos válidos do começo; sem nenhum dígito, `None` (o `NaN`).
fn parse_int_radix(text: &str, radix: u32) -> Option<i64> {
    let trimmed = text.trim_start();
    let (negative, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let mut value = 0f64;
    let mut any = false;
    for digit in unsigned.chars().map_while(|c| c.to_digit(radix)) {
        value = value * f64::from(radix) + f64::from(digit);
        any = true;
    }
    if !any {
        return None;
    }
    let value = if negative { -value } else { value };
    Some(value as i64)
}

/// `parse`: lê a URI de célula (`vscode-notebook-cell`) e devolve o notebook e o handle dela.
pub fn parse(cell: &URI) -> Option<ParsedCellUri> {
    if cell.scheme() != schemas::VSCODE_NOTEBOOK_CELL {
        return None;
    }

    let fragment = cell.fragment().to_string_lossy();
    let idx = fragment.find('s')?;

    // `.replace(_padRegexp, '')`, com `_padRegexp = /^[WXYZabcdef]+/`.
    let handle = parse_int_radix(fragment[..idx].trim_start_matches(|c: char| LENGTHS.contains(&c)), RADIX);
    let scheme = decode_base64(&fragment[idx + 1..]).to_string();

    let handle = handle?;
    Some(ParsedCellUri {
        handle,
        notebook: cell.with(&UriChange { scheme: Field::from(scheme.as_str()), fragment: Field::Clear, ..UriChange::default() }),
    })
}

/// `generate`: a URI de célula do notebook e do handle.
pub fn generate(notebook: &URI, handle: i64) -> URI {
    let s = to_radix_string(handle, RADIX);
    let p = if s.len() < LENGTHS.len() { LENGTHS[s.len() - 1] } else { 'z' };

    let encoded_scheme = encode_base64(&VSBuffer::from_string(&notebook.scheme().to_string_lossy()), true, true);
    let fragment = format!("{p}{s}s{encoded_scheme}");
    notebook.with(&UriChange {
        scheme: Field::from(schemas::VSCODE_NOTEBOOK_CELL),
        fragment: Field::from(fragment.as_str()),
        ..UriChange::default()
    })
}

/// `parseMetadataUri`.
pub fn parse_metadata_uri(metadata: &URI) -> Option<URI> {
    if metadata.scheme() != schemas::VSCODE_NOTEBOOK_METADATA {
        return None;
    }

    let scheme = decode_base64(&metadata.fragment().to_string_lossy()).to_string();

    Some(metadata.with(&UriChange { scheme: Field::from(scheme.as_str()), fragment: Field::Clear, ..UriChange::default() }))
}

/// `generateMetadataUri`.
pub fn generate_metadata_uri(notebook: &URI) -> URI {
    let fragment = encode_base64(&VSBuffer::from_string(&notebook.scheme().to_string_lossy()), true, true);
    notebook.with(&UriChange {
        scheme: Field::from(schemas::VSCODE_NOTEBOOK_METADATA),
        fragment: Field::from(fragment.as_str()),
        ..UriChange::default()
    })
}

/// O resultado de [`extract_cell_output_details`].
#[derive(Clone, Debug, PartialEq)]
pub struct CellOutputUriDetails {
    pub notebook: URI,
    pub open_in: String,
    pub output_id: Option<String>,
    pub cell_fragment: Option<String>,
    pub output_index: Option<i64>,
    pub cell_handle: Option<i64>,
    pub cell_index: Option<i64>,
}

/// `extractCellOutputDetails`.
pub fn extract_cell_output_details(uri: &URI) -> Option<CellOutputUriDetails> {
    if uri.scheme() != schemas::VSCODE_NOTEBOOK_CELL_OUTPUT {
        return None;
    }

    let params = UrlSearchParams::parse(&uri.query().to_string_lossy());
    let open_in = params.get("openIn").filter(|open_in| !open_in.is_empty())?;
    let output_id = params.get("outputId").map(str::to_owned);
    let parsed_cell = parse(&uri.with(&UriChange {
        scheme: Field::from(schemas::VSCODE_NOTEBOOK_CELL),
        query: Field::Clear,
        ..UriChange::default()
    }));
    let output_index = params.get("outputIndex").filter(|value| !value.is_empty()).and_then(|value| parse_int_radix(value, 10));
    let notebook_uri = match &parsed_cell {
        Some(parsed_cell) => parsed_cell.notebook.clone(),
        None => uri.with(&UriChange {
            scheme: Field::from(params.get("notebookScheme").filter(|scheme| !scheme.is_empty()).unwrap_or(schemas::FILE)),
            fragment: Field::Clear,
            query: Field::Clear,
            ..UriChange::default()
        }),
    };
    let cell_index = params.get("cellIndex").filter(|value| !value.is_empty()).and_then(|value| parse_int_radix(value, 10));

    Some(CellOutputUriDetails {
        notebook: notebook_uri,
        open_in: open_in.to_owned(),
        output_id,
        output_index,
        cell_handle: parsed_cell.map(|parsed_cell| parsed_cell.handle),
        cell_fragment: Some(uri.fragment().to_string_lossy()),
        cell_index,
    })
}

/// `INotebookDocumentService`.
pub trait INotebookDocumentService {
    fn get_notebook(&self, uri: &URI) -> Option<Rc<dyn INotebookDocument>>;
    fn add_notebook_document(&self, document: Rc<dyn INotebookDocument>);
    fn remove_notebook_document(&self, document: Rc<dyn INotebookDocument>);
}

service_id!(pub I_NOTEBOOK_DOCUMENT_SERVICE: dyn INotebookDocumentService = "notebookDocumentService");
