//! O diff de Myers que o `node:assert` do bun 1.4.2 usa para a mensagem do `AssertionError` (`myers_diff.rs`,
//! `node_assert.rs` e `node_assert_binding.rs` do bun, derivados do `internal/assert/myers_diff.js` do Node).
//!
//! O bun diferencia Latin-1 e UTF-16; aqui as duas entradas são sempre unidades UTF-16 (o resultado no JS é o mesmo
//! texto). As três funções nativas (`myersDiff`, `printSimpleMyersDiff` e `printMyersDiff`) chegam ao `assert` pelo
//! objeto `host`, como o `$rust("node_assert_binding.rs", "generate")` do bun.

use crate::host_function;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_class_support::{property_key, throw_coded_type_error};
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object};
use crate::runtime::web_iterable::{string_value, units_of, Units};

/// Os comprimentos cabem em `u32` no grafo de edição; `2 * (actual + expected)` tem de ficar abaixo de 4 Gi.
const MAXLEN: u64 = u32::MAX as u64;
/// O teto do traço (as cópias do grafo por nível), como no bun.
const MAX_TRACE_BYTES: usize = 64 * 1024 * 1024;

/// O que o diff compara: uma unidade (diff por caractere) ou uma linha.
pub(crate) trait Line: Copy {
    const IS_POINTER: bool;
    fn line_eq(a: Self, b: Self, check_comma_disparity: bool) -> bool;
    fn ends_with_comma(self) -> bool;
}

impl Line for u16 {
    const IS_POINTER: bool = false;
    fn line_eq(a: Self, b: Self, _check_comma_disparity: bool) -> bool {
        a == b
    }
    fn ends_with_comma(self) -> bool {
        false
    }
}

impl Line for &[u16] {
    const IS_POINTER: bool = true;
    fn line_eq(a: Self, b: Self, check_comma_disparity: bool) -> bool {
        are_str_lines_equal(a, b, check_comma_disparity)
    }
    fn ends_with_comma(self) -> bool {
        self.last() == Some(&u16::from(b','))
    }
}

/// `'foo,'` é igual a `'foo'` quando `check_comma_disparity`.
fn are_str_lines_equal(a: &[u16], b: &[u16], check_comma_disparity: bool) -> bool {
    if !check_comma_disparity {
        return a == b;
    }
    let (largest, smallest) = if a.len() > b.len() { (a, b) } else { (b, a) };
    match largest.len() - smallest.len() {
        0 => a == b,
        1 => largest[largest.len() - 1] == u16::from(b',') && largest[..smallest.len()] == *smallest,
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DiffKind {
    Insert = 0,
    Delete = 1,
    Equal = 2,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Diff<T> {
    pub(crate) kind: DiffKind,
    pub(crate) value: T,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DiffError {
    DiffTooLarge,
    InputsTooLarge,
}

/// O caminho de edição mais curto de `actual` para `expected`, do fim para o começo (como o `backtrack` devolve).
pub(crate) fn diff<L: Line>(actual: &[L], expected: &[L], check_comma_disparity: bool) -> Result<Vec<Diff<L>>, DiffError> {
    let max = actual.len() + expected.len();
    let graph_size = 2 * max + 1;
    if max as u64 > MAXLEN {
        return Err(DiffError::InputsTooLarge);
    }
    if graph_size as u64 > MAXLEN {
        return Err(DiffError::DiffTooLarge);
    }
    let mut graph: Vec<u32> = vec![0; graph_size];
    let mut trace: Vec<Box<[u32]>> = Vec::with_capacity(max + 1);
    for diff_level in 0..=max {
        let trace_bytes = (diff_level + 1).saturating_mul(graph_size).saturating_mul(std::mem::size_of::<u32>());
        if trace_bytes > MAX_TRACE_BYTES {
            return Err(DiffError::DiffTooLarge);
        }
        trace.push(graph.clone().into_boxed_slice());
        let level = diff_level as i64;
        let mut diag = -level;
        while diag <= level {
            let k = (diag + max as i64) as usize;
            let mut x = if diag == -level || (diag != level && graph[k - 1] < graph[k + 1]) { graph[k + 1] } else { graph[k - 1] + 1 };
            let mut y = (i64::from(x) - diag) as usize;
            while (x as usize) < actual.len() && y < expected.len() && L::line_eq(actual[x as usize], expected[y], check_comma_disparity) {
                x += 1;
                y += 1;
            }
            graph[k] = x;
            if (x as usize) >= actual.len() && y >= expected.len() {
                return Ok(backtrack(&trace, actual, expected, check_comma_disparity));
            }
            diag += 2;
        }
    }
    unreachable!("o diff sempre chega ao fim de `actual` ou de `expected`")
}

fn backtrack<L: Line>(trace: &[Box<[u32]>], actual: &[L], expected: &[L], check_comma_disparity: bool) -> Vec<Diff<L>> {
    let max = (actual.len() + expected.len()) as i64;
    let (mut x, mut y) = (actual.len() as i64, expected.len() as i64);
    let mut result = Vec::new();
    for diff_level in (0..trace.len()).rev() {
        let graph = &trace[diff_level];
        let diagonal = x - y;
        let offset = (diagonal + max) as usize;
        let level = diff_level as i64;
        let previous_diagonal = if diagonal == -level || (diagonal != level && graph[offset - 1] < graph[offset + 1]) { diagonal + 1 } else { diagonal - 1 };
        let previous_x = i64::from(graph[(previous_diagonal + max) as usize]);
        let previous_y = previous_x - previous_diagonal;
        while x > previous_x && y > previous_y {
            let actual_line = actual[x as usize - 1];
            let value = if L::IS_POINTER && check_comma_disparity && !actual_line.ends_with_comma() { expected[y as usize - 1] } else { actual_line };
            result.push(Diff { kind: DiffKind::Equal, value });
            x -= 1;
            y -= 1;
        }
        if diff_level > 0 {
            if x > previous_x {
                result.push(Diff { kind: DiffKind::Insert, value: actual[x as usize - 1] });
                x -= 1;
            } else {
                result.push(Diff { kind: DiffKind::Delete, value: expected[y as usize - 1] });
                y -= 1;
            }
        }
    }
    result
}

/// As linhas (`split('\n')`).
fn split(text: &[u16]) -> Vec<&[u16]> {
    text.split(|&unit| unit == u16::from(b'\n')).collect()
}

/// As sequências ANSI do `internal/util/colors` (vazias sem cor).
#[derive(Default)]
pub(crate) struct Colors {
    pub(crate) green: Units,
    pub(crate) red: Units,
    pub(crate) white: Units,
    pub(crate) blue: Units,
}

/// Um valor do diff que se acrescenta à saída: a unidade ou a linha.
trait DiffText: Copy {
    fn append_to(self, out: &mut Units);
}

impl DiffText for u16 {
    fn append_to(self, out: &mut Units) {
        out.push(self);
    }
}

impl DiffText for &[u16] {
    fn append_to(self, out: &mut Units) {
        out.extend_from_slice(self);
    }
}

fn ascii(out: &mut Units, bytes: &[u8]) {
    out.extend(bytes.iter().map(|&byte| u16::from(byte)));
}

/// `printSimpleMyersDiff`: o diff por caractere em linha, com a cor de cada operação.
fn render_simple<T: DiffText>(diff: &[Diff<T>], colors: &Colors) -> Units {
    let mut out = Units::with_capacity(diff.len() + 1);
    ascii(&mut out, b"\n");
    for entry in diff.iter().rev() {
        let color = match entry.kind {
            DiffKind::Insert => &colors.green,
            DiffKind::Delete => &colors.red,
            DiffKind::Equal => &colors.white,
        };
        out.extend_from_slice(color);
        entry.value.append_to(&mut out);
        out.extend_from_slice(&colors.white);
    }
    out
}

/// `String.prototype.trimEnd`: espaço e fim de linha do JS.
fn is_js_whitespace(unit: u16) -> bool {
    matches!(unit, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

/// `printMyersDiff`: uma linha por entrada com `+`, `-` ou dois espaços, e as sequências de mais de cinco linhas iguais
/// viram `...`.
fn render_lines<T: DiffText>(diff: &[Diff<T>], colors: &Colors) -> (Units, bool) {
    const NOP_LINES_TO_COLLAPSE: usize = 5;
    let mut out = Units::new();
    ascii(&mut out, b"\n");
    let mut skipped = false;
    let mut nop_count = 0usize;
    let equal_line = |out: &mut Units, value: T| {
        out.extend_from_slice(&colors.white);
        ascii(out, b"  ");
        value.append_to(out);
        ascii(out, b"\n");
    };
    for index in (0..diff.len()).rev() {
        let Diff { kind, value } = diff[index];
        let previous_kind = diff.get(index + 1).map(|entry| entry.kind);
        if previous_kind == Some(DiffKind::Equal) && kind != DiffKind::Equal {
            // Uma ou duas linhas a mais não valem o `...`.
            if nop_count == NOP_LINES_TO_COLLAPSE + 1 {
                equal_line(&mut out, diff[index + 1].value);
            } else if nop_count == NOP_LINES_TO_COLLAPSE + 2 {
                equal_line(&mut out, diff[index + 2].value);
                equal_line(&mut out, diff[index + 1].value);
            }
            if nop_count >= NOP_LINES_TO_COLLAPSE + 3 {
                out.extend_from_slice(&colors.blue);
                ascii(&mut out, b"...");
                out.extend_from_slice(&colors.white);
                ascii(&mut out, b"\n");
                equal_line(&mut out, diff[index + 1].value);
                skipped = true;
            }
            nop_count = 0;
        }
        match kind {
            DiffKind::Insert | DiffKind::Delete => {
                let (color, sign) = if kind == DiffKind::Insert { (&colors.green, b"+") } else { (&colors.red, b"-") };
                out.extend_from_slice(color);
                ascii(&mut out, sign);
                out.extend_from_slice(&colors.white);
                ascii(&mut out, b" ");
                value.append_to(&mut out);
                ascii(&mut out, b"\n");
            }
            DiffKind::Equal => {
                if nop_count < NOP_LINES_TO_COLLAPSE {
                    equal_line(&mut out, value);
                }
                nop_count += 1;
            }
        }
    }
    // O `trimEnd()` da mensagem (o `\n` do começo fica).
    while out.len() > 1 && is_js_whitespace(out[out.len() - 1]) {
        out.pop();
    }
    (out, skipped)
}

// -- As funções nativas --

/// Os dois primeiros argumentos, que têm de ser strings.
fn string_arguments(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<(Units, Units), Thrown> {
    if call.argument_count() < 2 {
        return Err(throw_coded_type_error(global_object, &format!("Not enough arguments to '{name}'. Expected 2, got {}.", call.argument_count()), "ERR_MISSING_ARGS"));
    }
    for (index, label) in [(0, "actual"), (1, "expected")] {
        if !call.argument(index).is_string() {
            return Err(throw_coded_type_error(global_object, &format!("The \"{label}\" argument must be of type string."), "ERR_INVALID_ARG_TYPE"));
        }
    }
    Ok((units_of(global_object, call.argument(0))?, units_of(global_object, call.argument(1))?))
}

fn diff_error(global_object: &JSGlobalObject, error: DiffError) -> Thrown {
    let message = match error {
        DiffError::DiffTooLarge => "Diffing these two values would create a string that is too large. If this was intentional, please open a bug report on GitHub.",
        DiffError::InputsTooLarge => "Input strings are too large to diff. Please open a bug report on GitHub.",
    };
    throw_coded_type_error(global_object, message, "ERR_INVALID_ARG_TYPE")
}

/// `{ green, red, white, blue }` do `internal/util/colors`.
fn colors_of(global_object: &JSGlobalObject, value: JSValue) -> Result<Colors, Thrown> {
    let Some(object) = JSObject::from_value(&value).filter(|_| value.is_object()) else { return Ok(Colors::default()) };
    let vm = global_object.vm();
    let get = |name: &str| -> Result<Units, Thrown> {
        let field = pending_or(global_object, object.get(vm, &property_key(vm, name)))?;
        if field.is_string() { units_of(global_object, field) } else { Ok(Units::new()) }
    };
    Ok(Colors { green: get("green")?, red: get("red")?, white: get("white")?, blue: get("blue")? })
}

/// `{ kind, value }` com protótipo nulo, para cada entrada da lista.
fn diff_list<T: Copy>(global_object: &JSGlobalObject, diff: &[Diff<T>], to_js: impl Fn(T) -> JSValue) -> JSValue {
    let vm = global_object.vm();
    let entries: Vec<JSValue> = diff
        .iter()
        .map(|entry| {
            let object = construct_empty_object(global_object);
            object.set_prototype_direct(vm, JSValue::null());
            object.put_direct(vm, &property_key(vm, "kind"), js_number(entry.kind as i32), 0);
            object.put_direct(vm, &property_key(vm, "value"), to_js(entry.value), 0);
            object.as_value()
        })
        .collect();
    construct_array_of(global_object, &entries).as_value()
}

/// `myersDiff(actual, expected, checkCommaDisparity, lines)`.
fn myers_diff_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (actual, expected) = string_arguments(global_object, call, "myersDiff")?;
    let check_comma_disparity = call.argument(2).to_boolean();
    if actual.is_empty() && expected.is_empty() {
        return Ok(construct_array_of(global_object, &[]).as_value());
    }
    if call.argument(3).to_boolean() {
        let (a, e) = (split(&actual), split(&expected));
        let diff = diff(&a, &e, check_comma_disparity).map_err(|error| diff_error(global_object, error))?;
        return Ok(diff_list(global_object, &diff, |line| string_value(global_object, line)));
    }
    let diff = diff(&actual, &expected, false).map_err(|error| diff_error(global_object, error))?;
    Ok(diff_list(global_object, &diff, |unit| js_number(i32::from(unit))))
}

/// `printSimpleMyersDiff(actual, expected, colors)`.
fn print_simple_myers_diff_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let colors = colors_of(global_object, call.argument(2))?;
    let (actual, expected) = string_arguments(global_object, call, "printSimpleMyersDiff")?;
    if actual.is_empty() && expected.is_empty() {
        return Ok(string_value(global_object, &render_simple::<u16>(&[], &colors)));
    }
    let diff = diff(&actual, &expected, false).map_err(|error| diff_error(global_object, error))?;
    Ok(string_value(global_object, &render_simple(&diff, &colors)))
}

/// `printMyersDiff(actual, expected, checkCommaDisparity, colors)`: `{ message, skipped }`.
fn print_myers_diff_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let check_comma_disparity = call.argument(2).to_boolean();
    let colors = colors_of(global_object, call.argument(3))?;
    let (actual, expected) = string_arguments(global_object, call, "printMyersDiff")?;
    let (message, skipped) = if actual.is_empty() && expected.is_empty() {
        render_lines::<u16>(&[], &colors)
    } else {
        let (a, e) = (split(&actual), split(&expected));
        let diff = diff(&a, &e, check_comma_disparity).map_err(|error| diff_error(global_object, error))?;
        render_lines(&diff, &colors)
    };
    let vm = global_object.vm();
    let result = construct_empty_object(global_object);
    result.put_direct(vm, &property_key(vm, "message"), string_value(global_object, &message), 0);
    result.put_direct(vm, &property_key(vm, "skipped"), JSValue::Bool(skipped), 0);
    Ok(result.as_value())
}

host_function!(pub myers_diff, myers_diff_body);
host_function!(pub print_simple_myers_diff, print_simple_myers_diff_body);
host_function!(pub print_myers_diff, print_myers_diff_body);

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Units {
        text.encode_utf16().collect()
    }

    #[test]
    fn lines_compare_with_and_without_the_trailing_comma() {
        let eq = |a: &str, b: &str, comma| are_str_lines_equal(&units(a), &units(b), comma);
        assert!(eq("", "", false) && eq("Bun", "Bun", false) && !eq("bun", "Bun", false) && !eq("", " ", false));
        assert!(eq("", ",", true) && eq(" ", " ,", true) && eq("I am speed,", "I am speed", true) && eq("I am speed", "I am speed,", true));
        assert!(!eq(",Bun", "Bun", true) && !eq("", " ,", true) && !eq(" ", " , ", true) && !eq("I, am speed", "I am speed", true));
        assert!(!eq("a", "a,", false));
    }

    /// O diff refaz `expected` a partir de `actual`: as linhas iguais e removidas dão `expected`, as iguais e
    /// inseridas dão `actual`.
    #[test]
    fn the_diff_rebuilds_both_sides() {
        let cases = [
            ("foo", "foo"),
            ("foo", "bar"),
            ("[\n  1,\n  2,\n  3,\n  4,\n  5,\n  6,\n  7\n]", "[\n  1,\n  2,\n  3,\n  4,\n  5,\n  9,\n  7\n]"),
            ("foo\nbar\nbaz", "foo\nbarrr\nbaz"),
            ("foooo\nbar\nbaz", "foo\nbar\nbaz"),
            ("a\nb\nc\nd", "x\na\nc\ny"),
            ("", "a\nb"),
        ];
        for (actual, expected) in cases {
            let (a, e) = (units(actual), units(expected));
            let (a_lines, e_lines) = (split(&a), split(&e));
            let diff = diff(&a_lines, &e_lines, false).unwrap();
            let side = |keep: DiffKind| -> Vec<&[u16]> { diff.iter().rev().filter(|d| d.kind == DiffKind::Equal || d.kind == keep).map(|d| d.value).collect() };
            assert_eq!(side(DiffKind::Insert), a_lines, "{actual:?} contra {expected:?}");
            assert_eq!(side(DiffKind::Delete), e_lines, "{actual:?} contra {expected:?}");
        }
    }

    #[test]
    fn the_rendered_lines_collapse_long_equal_runs() {
        let a = units("1\n2\n3\n4\n5\n6\n7\n8\n9\nx");
        let e = units("1\n2\n3\n4\n5\n6\n7\n8\n9\ny");
        let (a_lines, e_lines) = (split(&a), split(&e));
        let diff = diff(&a_lines, &e_lines, false).unwrap();
        let (text, skipped) = render_lines(&diff, &Colors::default());
        assert!(skipped);
        assert_eq!(String::from_utf16(&text).unwrap(), "\n  1\n  2\n  3\n  4\n  5\n...\n  9\n+ x\n- y");
    }
}
