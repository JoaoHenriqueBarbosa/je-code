//! `console.table` do bun: o `TablePrinter` de `ConsoleObject.rs`. Monta o texto da tabela (bordas `┌─┬┐│├┼┤└┴┘`, a
//! primeira coluna `" "` com o índice alinhado à direita, as colunas criadas na ordem em que as propriedades aparecem,
//! `Key` para `Map` e `Values` por último para o que não é objeto) a partir dos dados tabulares e das colunas pedidas.
//!
//! - Cada célula é formatada uma vez: string vem pura, o resto pelo formatador de uma linha com profundidade 5.
//! - A largura é a de exibição (CJK e emoji ocupam 2, combinantes e `U+200B` 0), sem as sequências `ESC [ ... m`.
//! - Com `properties`, as colunas são as pedidas (repetidas valem, inexistentes ficam vazias) e linha que não é objeto
//!   fica em branco; sem ele, linha que não é objeto vai para `Values`.
//! - Devolve `None` com exceção pendente (getter, iterador ou formatação que lançou); nada é escrito nesse caso.

use crate::runtime::call_data::get_call_data;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::console_format::Formatter;
use crate::runtime::host_call::{throw_thrown, Thrown as HostThrown};
use crate::runtime::js_promise_host::rethrow;
use crate::runtime::identifier::Identifier;
use crate::runtime::iterator_operations::{for_each_in_iterable, get_value_property};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::literal_parser::{wtf_string_to_units, JsonHost, JsonKey};
use crate::runtime::property_name::PropertyName;

const PADDING: usize = 1;

struct Cell {
    text: Vec<u16>,
    width: usize,
}

struct Column {
    name: Vec<u16>,
    width: usize,
}

enum RowKey {
    /// O nome da propriedade (dados tabulares que são objeto comum).
    Str(Vec<u16>, usize),
    /// O índice da linha (array e iteráveis).
    Num(u32),
}

impl RowKey {
    fn width(&self) -> usize {
        match self {
            RowKey::Str(_, width) => *width,
            RowKey::Num(value) => value.to_string().len(),
        }
    }
}

struct Row {
    key: RowKey,
    /// Indexada por `coluna - 1`; mais curta que as colunas se colunas novas apareceram depois.
    cells: Vec<Option<Cell>>,
    values_cell: Option<Cell>,
}

/// A largura de exibição de `units` sem sequências ANSI (`Bun__visibleWidthExcludeANSI_utf16`).
fn visible_width(units: &[u16]) -> usize {
    super::string_width::visible_width(units, false, false)
}

fn has_exception(global_object: &JSGlobalObject) -> bool {
    global_object.vm().exception().is_some()
}

/// A exceção pendente (que `collect_row` deixou no `VM`) como o `Thrown::Pending` que interrompe
/// `for_each_in_iterable`; o laço fecha o iterador e a exceção segue pendente para o chamador.
fn take_pending(_global_object: &JSGlobalObject) -> HostThrown {
    HostThrown::Pending
}

fn property_name(global_object: &JSGlobalObject, key: &JsonKey) -> PropertyName {
    let vm = global_object.vm();
    PropertyName::from_identifier(&match key {
        JsonKey::Index(index) => Identifier::from_u32(vm, *index),
        JsonKey::Name(name) => Identifier::from_string(vm, name),
    })
}

/// `value[index]`; `None` com exceção pendente.
fn index_of(global_object: &JSGlobalObject, value: JSValue, index: u32) -> Option<JSValue> {
    let name = PropertyName::from_identifier(&Identifier::from_u32(global_object.vm(), index));
    let element = value.as_object().get(global_object, &name);
    if has_exception(global_object) { None } else { Some(element) }
}

struct Printer<'a> {
    global_object: &'a JSGlobalObject,
    properties: JSValue,
    is_map: bool,
    indent: usize,
    columns: Vec<Column>,
    values_col_width: Option<usize>,
}

impl Printer<'_> {
    /// A célula de `value`: a string pura, o resto pelo formatador; `None` com exceção pendente.
    fn format_cell(&self, value: JSValue) -> Option<Cell> {
        let text = if value.is_string() {
            let units = wtf_string_to_units(&value.to_wtf_string());
            if has_exception(self.global_object) {
                return None;
            }
            units
        } else {
            let mut out = Vec::new();
            if !Formatter::table_cell(self.indent).push_object(self.global_object, &mut out, value) {
                return None;
            }
            out
        };
        let width = visible_width(&text);
        Some(Cell { text, width })
    }

    /// `getOwn(name)`: o valor da propriedade própria, `Ok(None)` se não existe; `Err` com exceção pendente.
    fn own_value(&self, object: JSValue, name: &[u16]) -> Result<Option<JSValue>, ()> {
        let key = JsonKey::from_units(name);
        let vm = self.global_object.vm();
        let target = object.as_object();
        let present = match &key {
            JsonKey::Index(index) => target.has_own_property_by_index(vm, *index),
            JsonKey::Name(_) => target.has_own_property(self.global_object, &property_name(self.global_object, &key)),
        };
        if !present {
            return Ok(None);
        }
        let value = target.get(self.global_object, &property_name(self.global_object, &key));
        if has_exception(self.global_object) { Err(()) } else { Ok(Some(value)) }
    }

    fn collect_row(&mut self, key: RowKey, row_value: JSValue) -> Option<Row> {
        self.columns[0].width = self.columns[0].width.max(key.width());
        let mut row = Row { key, cells: Vec::new(), values_cell: None };

        // `Map`: a coluna 1 é `Key`, e o valor vai para `Values`.
        if self.is_map {
            let key_cell = self.format_cell(index_of(self.global_object, row_value, 0)?)?;
            let value_cell = self.format_cell(index_of(self.global_object, row_value, 1)?)?;
            self.columns[1].width = self.columns[1].width.max(key_cell.width);
            self.values_col_width = Some(self.values_col_width.unwrap_or(0).max(value_cell.width));
            row.cells.push(Some(key_cell));
            row.values_cell = Some(value_cell);
            return Some(row);
        }

        if row_value.is_object() {
            if !self.properties.is_undefined() {
                for column in 1..self.columns.len() {
                    let name = self.columns[column].name.clone();
                    match self.own_value(row_value, &name) {
                        Err(()) => return None,
                        Ok(Some(value)) => {
                            let cell = self.format_cell(value)?;
                            self.columns[column].width = self.columns[column].width.max(cell.width);
                            row.cells.push(Some(cell));
                        }
                        Ok(None) => row.cells.push(None),
                    }
                }
            } else {
                let keys = match self.global_object.own_enumerable_string_keys(row_value) {
                    Ok(keys) => keys,
                    Err(thrown) => {
                        throw_thrown(self.global_object, rethrow(self.global_object, thrown));
                        return None;
                    }
                };
                for key in keys {
                    let name = wtf_string_to_units(&key.to_wtf_string());
                    let value = row_value.as_object().get(self.global_object, &property_name(self.global_object, &key));
                    if has_exception(self.global_object) {
                        return None;
                    }
                    let column = match self.columns[1..].iter().position(|column| column.name == name) {
                        Some(position) => position + 1,
                        None => {
                            self.columns.push(Column { name, width: 1 });
                            self.columns.len() - 1
                        }
                    };
                    let cell = self.format_cell(value)?;
                    self.columns[column].width = self.columns[column].width.max(cell.width);
                    if row.cells.len() < column {
                        row.cells.resize_with(column, || None);
                    }
                    row.cells[column - 1] = Some(cell);
                }
            }
        } else if self.properties.is_undefined() {
            // Não objeto: o valor vai para a coluna especial `Values`.
            let cell = self.format_cell(row_value)?;
            self.values_col_width = Some(self.values_col_width.unwrap_or(1).max(cell.width));
            row.values_cell = Some(cell);
        }
        Some(row)
    }
}

fn push_units(out: &mut String, units: &[u16]) {
    out.push_str(&String::from_utf16_lossy(units));
}

fn push_spaces(out: &mut String, count: usize) {
    out.extend(std::iter::repeat(' ').take(count));
}

fn push_rule(out: &mut String, width: usize) {
    out.extend(std::iter::repeat('─').take(width + PADDING * 2));
}

/// O texto da tabela de `console.table(tabular_data, properties)`; `properties` é `undefined` ou um array.
/// `levels` é o recuo de `console.group`, que só entra no formatador das células. `None` com exceção pendente.
pub fn render(global_object: &JSGlobalObject, tabular_data: JSValue, properties: JSValue, levels: usize) -> Option<String> {
    let vm = global_object.vm();
    let iterator_method = match get_value_property(global_object, tabular_data, &PropertyName::from_identifier(&vm.property_names.iterator_symbol)) {
        Ok(method) => method,
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            return None;
        }
    };
    let is_iterable = !get_call_data(iterator_method).is_none();
    let is_map = matches!(cell_registry::get(tabular_data.as_cell()), Some(CellEntry::Map(_)));

    let mut printer = Printer { global_object, properties, is_map, indent: levels, columns: vec![Column { name: vec![u16::from(b' ')], width: 1 }], values_col_width: None };
    if is_map {
        printer.columns.push(Column { name: "Key".encode_utf16().collect(), width: 1 });
    }
    if !properties.is_undefined() {
        let length = global_object.length_of_array_like(properties).ok()?;
        for index in 0..length {
            let value = index_of(global_object, properties, u32::try_from(index).ok()?)?;
            let name = wtf_string_to_units(&value.to_wtf_string());
            if has_exception(global_object) {
                return None;
            }
            printer.columns.push(Column { name, width: 1 });
        }
    }

    let mut rows: Vec<Row> = Vec::new();
    if is_iterable {
        let mut index = 0u32;
        let result = for_each_in_iterable(global_object, tabular_data, |value| {
            let row = printer.collect_row(RowKey::Num(index), value).ok_or_else(|| take_pending(global_object))?;
            rows.push(row);
            index += 1;
            Ok(())
        });
        if let Err(thrown) = result {
            throw_thrown(global_object, thrown);
            return None;
        }
    } else {
        let keys = match global_object.own_enumerable_string_keys(tabular_data) {
            Ok(keys) => keys,
            Err(thrown) => {
                throw_thrown(global_object, rethrow(global_object, thrown));
                return None;
            }
        };
        for key in keys {
            let name = wtf_string_to_units(&key.to_wtf_string());
            let value = tabular_data.as_object().get(global_object, &property_name(global_object, &key));
            if has_exception(global_object) {
                return None;
            }
            let width = visible_width(&name);
            rows.push(printer.collect_row(RowKey::Str(name, width), value)?);
        }
    }

    // A coluna `Values` entra por último, se alguma linha a usou.
    let mut values_col_idx = usize::MAX;
    if let Some(width) = printer.values_col_width {
        values_col_idx = printer.columns.len();
        printer.columns.push(Column { name: "Values".encode_utf16().collect(), width });
    }
    let mut columns = printer.columns;
    for column in columns.iter_mut() {
        column.width = column.width.max(visible_width(&column.name));
    }

    let mut out = String::new();
    out.push('┌');
    for (i, column) in columns.iter().enumerate() {
        if i > 0 {
            out.push('┬');
        }
        push_rule(&mut out, column.width);
    }
    out.push_str("┐\n│");
    for (i, column) in columns.iter().enumerate() {
        if i > 0 {
            out.push('│');
        }
        let needed = column.width.saturating_sub(visible_width(&column.name));
        push_spaces(&mut out, 1);
        push_units(&mut out, &column.name);
        push_spaces(&mut out, needed + PADDING);
    }
    out.push_str("│\n├");
    for (i, column) in columns.iter().enumerate() {
        if i > 0 {
            out.push('┼');
        }
        push_rule(&mut out, column.width);
    }
    out.push_str("┤\n");

    for row in &rows {
        out.push('│');
        push_spaces(&mut out, columns[0].width.saturating_sub(row.key.width()) + PADDING);
        match &row.key {
            RowKey::Str(text, _) => push_units(&mut out, text),
            RowKey::Num(value) => out.push_str(&value.to_string()),
        }
        push_spaces(&mut out, PADDING);
        for (col_idx, column) in columns.iter().enumerate().skip(1) {
            out.push('│');
            let cell = if col_idx == values_col_idx { row.values_cell.as_ref() } else { row.cells.get(col_idx - 1).and_then(Option::as_ref) };
            match cell {
                None => push_spaces(&mut out, column.width + PADDING * 2),
                Some(cell) => {
                    push_spaces(&mut out, PADDING);
                    push_units(&mut out, &cell.text);
                    push_spaces(&mut out, column.width.saturating_sub(cell.width) + PADDING);
                }
            }
        }
        out.push_str("│\n");
    }

    out.push('└');
    push_rule(&mut out, columns[0].width);
    for column in &columns[1..] {
        out.push('┴');
        push_rule(&mut out, column.width);
    }
    out.push_str("┘\n");
    Some(out)
}
