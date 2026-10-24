//! O relato de exceção não capturada, como o bun escreve no stderr: o trecho `N | linha` do fonte (até cinco linhas
//! antes da do erro, numeração alinhada à direita; regras de aparo, corte e `^` em `append_source_excerpt`), a linha do `^`, `error: msg` (ou `TypeError: msg` para as subclasses
//! nativas), os frames `at`, a linha em branco e o rodapé `Bun v1.4.2 (Linux x64)`. Um `Error` com `cause` que também
//! é `Error` encadeia os blocos separados por linha em branco; um `AggregateError` com `errors` relata cada elemento no
//! lugar de si mesmo, sem linha em branco entre eles. Um valor que não é `Error` sai como `error: texto` e o próprio
//! valor numa segunda linha. Várias exceções da mesma rodada dividem um só rodapé. Medido por
//! `scripts/gen-uncaught-golden.js` (`tests/golden/uncaught_bun.tsv`). A posição vem do `ErrorInstance` (`line` e
//! `column`, base um, gravados por `materialize_stack`).

use crate::runtime::console_format::Formatter;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PropertyNameMode};
use crate::runtime::exception_helpers::calculated_class_name;
use crate::runtime::object_constructor::own_property_keys;
use crate::runtime::error_instance::{ErrorInstance, ErrorInstanceRef};
use crate::runtime::error_type::ErrorType;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_dom_exception;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_value_conversions::number_to_string_radix10;
use crate::runtime::literal_parser::{wtf_string_to_units, JsonHost, JsonKey};
use crate::runtime::property_name::PropertyName;
use crate::runtime::source_highlight::highlight_javascript;
use crate::runtime::stack_frame::StackFrame;
use crate::wtf::text::wtf_string::String as WtfString;

/// O rodapé que o bun escreve depois de toda exceção não capturada.
const FOOTER: &str = "Bun v1.4.2 (Linux x64)\n";

/// Quantas linhas de fonte o trecho mostra antes da linha do erro (medido: cinco, seis ao todo).
const CONTEXT_LINES: usize = 5;

/// O bun corta cada linha do trecho em 1024 bytes, no meio de um caractere UTF-8 se for o caso (o byte parcial sai).
const MAX_LINE_BYTES: usize = 1024;

/// A linha do `^` sai vazia quando a linha do erro tem ao menos tantos bytes e a coluna é menor que a seguinte.
const CARET_MIN_LINE_BYTES: usize = 512;
const CARET_MIN_COLUMN: usize = 514;

thread_local! {
    /// O fonte do script principal, de onde sai o trecho que o relato mostra.
    static MAIN_SOURCE: std::cell::RefCell<Option<std::rc::Rc<str>>> = const { std::cell::RefCell::new(None) };
}

/// Guarda o fonte do script principal para os relatos que saem fora da exceção fatal (`report_handler_error`).
pub(crate) fn remember_main_source(source: std::rc::Rc<str>) {
    MAIN_SOURCE.with(|slot| *slot.borrow_mut() = Some(source));
}

/// O erro de um handler de `Bun.serve` que ninguém tratou: o bloco do erro como numa exceção fatal (trecho do fonte,
/// `error: msg`, frames), logo a linha `label` (`GET - /caminho failed`), no stderr, sem rodapé e sem terminar o
/// programa (medido no bun 1.4.2).
pub(crate) fn report_handler_error(global_object: &JSGlobalObject, error: JSValue, label: &str) {
    let source = MAIN_SOURCE.with(|slot| slot.borrow().clone()).unwrap_or_else(|| std::rc::Rc::from(""));
    let mut report = Vec::new();
    append_exception_report(&mut report, global_object, &source, error, false);
    report.extend_from_slice(label.as_bytes());
    report.push(b'\n');
    if let Some(console) = global_object.console_host() {
        console.write_stderr(&report);
    }
}

/// Escreve no console do hospedeiro de `global_object` o relato da exceção `exception` lançada por `source` e devolve o
/// código de saída do processo (1). Sem console instalado a saída é descartada.
pub fn report_uncaught_exception(global_object: &JSGlobalObject, source: &str, exception: JSValue) -> i32 {
    let mut report = Vec::new();
    append_exception_report(&mut report, global_object, source, exception, false);
    finish_report(global_object, &report)
}

/// Escreve `report` (os blocos acumulados, bytes crus) seguido da linha em branco e do rodapé; `0` sem nada a
/// relatar, senão `1`.
pub fn finish_report(global_object: &JSGlobalObject, report: &[u8]) -> i32 {
    if report.is_empty() {
        return 0;
    }
    if let Some(console) = global_object.console_host() {
        console.write_stderr(&[report, b"\n", FOOTER.as_bytes()].concat());
    }
    1
}

/// Acrescenta a `out` os blocos da exceção `exception` (sem a linha em branco final nem o rodapé). `with_trace` diz que
/// a exceção saiu de um callback (timer, microtarefa): um valor que não é `Error` ganha então o trecho, o `^` e o frame
/// do `throw` (lidos da última exceção do VM) no lugar da segunda linha com o valor.
pub fn append_exception_report(out: &mut Vec<u8>, global_object: &JSGlobalObject, source: &str, exception: JSValue, with_trace: bool) {
    // O `BuildMessage` (o arquivo que o transpilador recusa) e o `ResolveMessage` (o import que não resolve) saem como o
    // log do bun, não como objeto; o `AggregateError` de `BuildMessage` os relata um depois do outro, separados por linha em branco.
    if let Some(block) = crate::runtime::js_module_loader::message_block(exception) {
        out.extend_from_slice(&block);
        return;
    }
    if let JSValue::Cell(cell_id) = &exception {
        if let Some(error) = ErrorInstance::from_cell_id(*cell_id).filter(|error| error.error_type() == Some(ErrorType::AggregateError)) {
            let vm = global_object.vm();
            if let Some(list) = JSObject::from_value(&own_property(global_object, &error, &vm.property_names.errors)) {
                let length = list.get(vm, &PropertyName::from_identifier(&vm.property_names.length));
                let blocks: Vec<Vec<u8>> = (0..if length.is_number() { length.as_number() as u32 } else { 0 })
                    .map_while(|index| crate::runtime::js_module_loader::message_block(list.get_by_index(vm, index)))
                    .collect();
                if !blocks.is_empty() {
                    out.extend_from_slice(&blocks.join(&b'\n'));
                    return;
                }
            }
        }
    }
    let (text, inspected) = match &exception {
        JSValue::Cell(cell_id) if ErrorInstance::from_cell_id(*cell_id).is_some() => {
            // Saída de um callback, o bun relata a `JSC::Exception` e não o valor: `JSC__JSValue__toZigException` passa
            // `&jscException->stack()` a `fromErrorInstance`, que usa essa pilha (a do `throw`, capturada por
            // `Interpreter::unwind`) no lugar da do erro quando ela não é vazia. O frame de cima mostra então o ponto
            // do `throw` (o fim da expressão lançada) e os frames são os de quando ele lançou, não os da construção.
            let thrown = with_trace.then(|| thrown_stack(global_object, &exception)).flatten();
            let error = ErrorInstance::from_cell_id(*cell_id).expect("conferido acima");
            append_error_chain(out, global_object, source, error, &mut Vec::new(), thrown.as_deref(), false);
            return;
        }
        value if value.is_string() => (rust_string(&value.as_js_string().value()), None),
        value if value.is_number() => {
            let number = value.as_number();
            let shown = rust_string(&number_to_string_radix10(global_object.vm(), number).value());
            let inspected = if number == 0.0 && number.is_sign_negative() { "-0".to_string() } else { shown.clone() };
            (shown, Some(inspected))
        }
        // Objeto lançado num callback: o bun escreve só `error` e o frame (a posição é a do divot do `throw`, o fim da
        // expressão, que `ThrowNode::emit_bytecode` já registra), sem o bloco do objeto.
        _ if with_trace => (String::new(), None),
        // Objeto lançado no topo (ou rejeitado): cabeçalho com `name` e `message` strings do objeto e, na linha seguinte,
        // o objeto formatado (`print_error_instance_body`, ramo `error_instance != .zero` que não é `Error`).
        // Função lançada: o `name` dela é o cabeçalho (`f: `) e o valor sai como `[Function: f]`.
        other if other.is_object() => {
            let mut formatter = Formatter::new();
            let mut shown = Vec::new();
            let is_dom = js_dom_exception::has_dom_exception_prototype(other);
            let printed = if is_dom {
                dom_exception_body(global_object, other, &mut shown)
            } else {
                formatter.push_object(global_object, &mut shown, other.clone())
            };
            if printed {
                let object = other.as_object();
                let vm = global_object.vm();
                let field = |identifier: &crate::runtime::identifier::Identifier| {
                    let value = object.get(global_object, &PropertyName::from_identifier(identifier));
                    // O getter que lança (`name` de `DOMException.prototype` sobre objeto sem a marca) vale como campo
                    // ausente: o cabeçalho sai só `error`, e a exceção não fica pendente para a leitura seguinte.
                    if vm.exception().is_some() {
                        vm.clear_exception();
                        return String::new();
                    }
                    if value.is_string() { rust_string(&value.as_js_string().value()) } else { String::new() }
                };
                let (name, message) = (field(&vm.property_names.name), field(&vm.property_names.message));
                let head = error_header(&name, &message, None, false);
                // O frame não vem de pilha nenhuma: o valor lançado não é `Exception` nem `ErrorInstance`, então o bun cai em
                // `exceptionFromString` (src/jsc/bindings/ZigException.cpp), que, com `frames_len == 0`, lê as propriedades
                // `sourceURL` (string; sem ela não há frame) e `line` (número) do próprio objeto e monta `frames_ptr[0]`
                // com `frames_len = 1`. A coluna (`column`) e o nome da função nunca são lidos, por isso o frame sai sem
                // os dois. `print_stack_trace` (src/jsc/VirtualMachine.rs) imprime `      at <url>:<line>` quando a
                // linha é válida (>= 1), só `      at <url>` quando não, e pula o frame se a URL é vazia. Um `DOMException`
                // lançado por função nativa (`atob("*")`, `structuredClone(() => {})`) nasce com essas propriedades; um criado
                // por `new DOMException` não, e por isso não ganha frame. Vale para qualquer objeto, não só `DOMException`.
                // (`originalLine` substitui `line` quando ambos são número; `originalColumn` nunca é lido.)
                let frame = {
                    let url = object.get(global_object, &PropertyName::from_identifier(&vm.property_names.source_url));
                    let url = if url.is_string() { rust_string(&url.as_js_string().value()) } else { String::new() };
                    let line = object.get(global_object, &PropertyName::from_identifier(&vm.property_names.line));
                    let line = if line.is_number() {
                        // `originalLine` só é lido quando `line` é número, e o substitui se for número.
                        let original = object.get(global_object, &PropertyName::from_identifier(&Identifier::from_string(vm, &WtfString::from_latin1(b"originalLine"))));
                        let chosen = if original.is_number() { original } else { line };
                        chosen.as_number() as i64 as i32
                    } else {
                        0
                    };
                    if url.is_empty() {
                        None
                    } else if line >= 1 {
                        Some(format!("      at {url}:{line}\n"))
                    } else {
                        Some(format!("      at {url}\n"))
                    }
                };
                out.extend_from_slice(format!("{head}\n{}\n{}", String::from_utf16_lossy(&shown), frame.unwrap_or_default()).as_bytes());
                return;
            }
            (crate::api::eval::describe_exception(other), None)
        }
        other => (crate::api::eval::describe_exception(other), None),
    };
    let frame = with_trace
        .then(|| thrown_stack(global_object, &exception))
        .flatten()
        .and_then(|stack| stack.into_iter().next())
        .filter(|frame| frame.has_line_and_column_info());
    match frame {
        Some(frame) => {
            let stack = format!("\n{}", frame.to_stack_line());
            let position = Some((frame.line() as usize, frame.column() as usize));
            out.extend_from_slice(&format_error(source, &text, "", &stack, position, None, &[], None, false));
        }
        None => out.extend_from_slice(format_primitive(&text, inspected).as_bytes()),
    }
}

/// A pilha do `throw` de `exception` (a `m_stack` da última `Exception` do VM, se ela leva esse valor), não vazia.
fn thrown_stack(global_object: &JSGlobalObject, exception: &JSValue) -> Option<Vec<StackFrame>> {
    global_object.vm().last_exception().filter(|last| last.value() == *exception).map(|last| last.stack()).filter(|stack| !stack.is_empty())
}


/// `error: texto` (ou só `error` quando vazio) e o valor inspecionado.
fn format_primitive(text: &str, inspected: Option<String>) -> String {
    let head = if text.is_empty() { "error".to_string() } else { format!("error: {text}") };
    format!("{head}\n{}\n", inspected.as_deref().unwrap_or(text))
}

/// Quantos objetos da cadeia de protótipos o `forEachProperty` do bun percorre (`prototypeCount++ < 5`).
const MAX_PROTOTYPE_LEVELS: usize = 5;

/// O corpo de um `DOMException` lançado (ou de um objeto com protótipo `DOMException`): `Classe {` e as propriedades
/// como o `JSC__JSValue__forEachPropertyImpl` do bun as lista no caminho lento (as de um `DOMException` são acessores,
/// o que desliga o caminho rápido): para cada objeto da cadeia, até cinco e parando em `Object.prototype` ou
/// `Function.prototype`, todos os nomes de string próprios, enumeráveis ou não (`DontEnumPropertiesMode::Include`),
/// menos `constructor` e `__proto__`, sem repetir um nome já visto. É por isso que o `toString` não enumerável de
/// `Error.prototype` aparece no fim. O valor de cada um é lido no próprio objeto (os getters do protótipo rodam com a
/// instância como `this`); uma exceção do getter vira `undefined`. Cada valor sai com `depth + 1` e `max_depth` 1.
/// `false` com exceção pendente.
fn dom_exception_body(global_object: &JSGlobalObject, exception: &JSValue, out: &mut Vec<u16>) -> bool {
    let vm = global_object.vm();
    let mut names: Vec<(Vec<u16>, PropertyName)> = Vec::new();
    let mut current = exception.clone();
    let (object_prototype, function_prototype) = (global_object.object_prototype().as_value(), global_object.function_prototype().as_value());
    let mut levels = 0;
    while current.is_object() && current != object_prototype && current != function_prototype && levels < MAX_PROTOTYPE_LEVELS {
        levels += 1;
        let Ok(keys) = own_property_keys(global_object, &current.as_object(), PropertyNameMode::Strings, DontEnumPropertiesMode::Include) else {
            return false;
        };
        for index in 0..keys.length() {
            let key = keys.get_by_index(vm, index);
            if !key.is_string() {
                continue;
            }
            let name = key.as_js_string().value();
            let units = wtf_string_to_units(&name);
            let is = |text: &str| units.iter().copied().eq(text.bytes().map(u16::from));
            if is("constructor") || is("__proto__") || names.iter().any(|(known, _)| *known == units) {
                continue;
            }
            names.push((units.clone(), PropertyName::from_identifier(&Identifier::from_string(vm, &name))));
        }
        current = current.as_object().get_prototype_direct();
    }
    let object = exception.as_object();
    let class_name = String::from_utf16_lossy(&wtf_string_to_units(&calculated_class_name(&object)));
    out.extend(format!("{class_name} {{\n").encode_utf16());
    let mut formatter = Formatter::with_depth(1, 1);
    for (units, name) in &names {
        let mut value = object.get(global_object, name);
        if vm.exception().is_some() {
            vm.clear_exception();
            value = JSValue::undefined();
        }
        let mut shown = Vec::new();
        if !formatter.push_object(global_object, &mut shown, value) {
            return false;
        }
        out.extend("  ".encode_utf16());
        out.extend_from_slice(units);
        out.extend(": ".encode_utf16());
        out.extend(shown);
        out.extend(",\n".encode_utf16());
    }
    out.extend("}".encode_utf16());
    true
}

/// A propriedade própria `name` do erro (`cause`, `errors`), `undefined` se não existe.
fn own_property(global_object: &JSGlobalObject, error: &ErrorInstanceRef, name: &crate::runtime::identifier::Identifier) -> JSValue {
    match JSObject::from_value(&error.as_value()) {
        Some(object) => object.get_direct_by_name(global_object.vm(), &PropertyName::from_identifier(name)),
        None => JSValue::undefined(),
    }
}

/// Acrescenta a `out` o relato de `error` (a cadeia de `cause` ou os elementos de um `AggregateError` incluídos), o que o
/// relato de exceção não capturada e o `console.log` de um `Error` (`print_errorlike_object`) escrevem igual. Com `source`
/// vazio o trecho sai do fonte guardado no próprio erro (o do frame de cima, como o bun lê o do `SourceProvider`).
/// `colors` liga as sequências ANSI (o `allow_ansi_color` do bun: fonte destacado, `^` vermelho, nome e frames).
pub fn append_error_report(out: &mut Vec<u8>, global_object: &JSGlobalObject, source: &str, error: ErrorInstanceRef, colors: bool) {
    append_error_chain(out, global_object, source, error, &mut Vec::new(), None, colors);
}

/// O bloco de `error` e, depois de uma linha em branco, o da sua `cause` quando ela também é `Error`. Um
/// `AggregateError` com elementos relata os elementos no lugar de si. `seen` corta ciclos de `cause`. `thrown` é a
/// pilha da `Exception` que trouxe a raiz, que substitui a do próprio erro só no bloco dela.
fn append_error_chain(out: &mut Vec<u8>, global_object: &JSGlobalObject, source: &str, error: ErrorInstanceRef, seen: &mut Vec<usize>, thrown: Option<&[StackFrame]>, colors: bool) {
    if seen.contains(&error.cell_id()) {
        return;
    }
    seen.push(error.cell_id());
    // `materializeErrorInfoIfNeeded`: sem isto o texto de `stack` e a posição ainda não existem.
    let _ = error.materialize_stack(global_object);
    let vm = global_object.vm();
    if error.error_type() == Some(ErrorType::AggregateError) {
        let errors = own_property(global_object, &error, &vm.property_names.errors);
        if let Some(list) = JSObject::from_value(&errors) {
            let length = list.get(vm, &PropertyName::from_identifier(&vm.property_names.length));
            let count = if length.is_number() { length.as_number() as u32 } else { 0 };
            if count > 0 {
                for index in 0..count {
                    let element = list.get_by_index(vm, index);
                    if let JSValue::Cell(cell_id) = element {
                        if let Some(inner) = ErrorInstance::from_cell_id(cell_id) {
                            append_error_chain(out, global_object, source, inner, seen, None, colors);
                        }
                    }
                }
                return;
            }
        }
    }
    // Ciclo de `cause` (medido contra o bun, com `cause` atribuída, portanto enumerável): a raiz sai sempre como bloco
    // simples; um erro seguinte cuja cadeia de `cause` volta a um já relatado (ou a si) é inspecionado com a `cause`
    // aninhada (` cause: ...,`), e o relato fecha com o bloco do erro repetido (`circular_tail`).
    let root = seen.len() == 1;
    let cause = cause_of(global_object, &error);
    let Some(cause) = cause else {
        out.extend_from_slice(&error_block(global_object, source, &error, None, thrown, colors));
        return;
    };
    if cause.cell_id() == error.cell_id() {
        if root {
            out.extend_from_slice(&error_block(global_object, source, &error, None, thrown, colors));
            out.push(b'\n');
        }
        out.extend_from_slice(&circular_tail(global_object, source, &error, colors));
        return;
    }
    let earlier: Vec<usize> = seen.iter().copied().filter(|id| *id != error.cell_id()).collect();
    if !root && is_cyclic(global_object, &error, &earlier) {
        out.extend_from_slice(&cyclic_block(global_object, source, &error, &earlier, colors));
    } else {
        out.extend_from_slice(&error_block(global_object, source, &error, None, thrown, colors));
    }
    out.push(b'\n');
    if seen.contains(&cause.cell_id()) {
        out.extend_from_slice(&circular_tail(global_object, source, &cause, colors));
        return;
    }
    append_error_chain(out, global_object, source, cause, seen, None, colors);
}

/// A `cause` própria de `error` quando é um `Error`.
fn cause_of(global_object: &JSGlobalObject, error: &ErrorInstanceRef) -> Option<ErrorInstanceRef> {
    match own_property(global_object, error, &global_object.vm().property_names.cause) {
        JSValue::Cell(cell_id) => ErrorInstance::from_cell_id(cell_id),
        _ => None,
    }
}

/// Se seguir as `cause` a partir de `error` volta a `error`, a um nó repetido ou a um de `earlier`.
fn is_cyclic(global_object: &JSGlobalObject, error: &ErrorInstanceRef, earlier: &[usize]) -> bool {
    let mut walked = vec![error.cell_id()];
    let mut current = cause_of(global_object, error);
    while let Some(next) = current {
        if walked.contains(&next.cell_id()) || earlier.contains(&next.cell_id()) {
            return true;
        }
        walked.push(next.cell_id());
        current = cause_of(global_object, &next);
    }
    false
}

/// O bloco do erro repetido que fecha um ciclo: ele com ` cause: [Circular],` e, depois, a linha `[Circular]`.
fn circular_tail(global_object: &JSGlobalObject, source: &str, error: &ErrorInstanceRef, colors: bool) -> Vec<u8> {
    let _ = error.materialize_stack(global_object);
    let mut block = error_block(global_object, source, error, Some(b"[Circular]"), None, colors);
    block.extend_from_slice(b"\n[Circular]");
    block
}

/// O bloco de `error` com a `cause` inspecionada por dentro: a `cause` já relatada vira `circular_tail`, a ainda não
/// relatada vira o próprio bloco aninhado seguido de `\n[Circular]`.
fn cyclic_block(global_object: &JSGlobalObject, source: &str, error: &ErrorInstanceRef, earlier: &[usize], colors: bool) -> Vec<u8> {
    let _ = error.materialize_stack(global_object);
    let Some(cause) = cause_of(global_object, error) else { return error_block(global_object, source, error, None, None, colors) };
    let mut walked = earlier.to_vec();
    walked.push(error.cell_id());
    let inner = if walked.contains(&cause.cell_id()) {
        circular_tail(global_object, source, &cause, colors)
    } else {
        let mut nested = cyclic_block(global_object, source, &cause, &walked, colors);
        nested.extend_from_slice(b"\n[Circular]");
        nested
    };
    error_block(global_object, source, error, Some(&inner), None, colors)
}

/// O bloco de `error` (trecho, cabeçalho, propriedades próprias e frames); `cause` acrescenta a linha ` cause: texto,`
/// depois do cabeçalho. `thrown`, não vazia, dá os frames e a posição no lugar dos do erro.
fn error_block(global_object: &JSGlobalObject, source: &str, error: &ErrorInstanceRef, cause: Option<&[u8]>, thrown: Option<&[StackFrame]>, colors: bool) -> Vec<u8> {
    let (stack, position) = match thrown {
        Some(frames) => {
            // O bun não escreve o frame de função de host sem fonte (`dispatchEvent`); medido no bun 1.4.2.
            let shown = frames.iter().filter(|frame| !frame.is_native_function());
            let lines: String = shown.map(|frame| format!("\n{}", frame.to_stack_line())).collect();
            let top = frames.first().filter(|frame| frame.has_line_and_column_info());
            (lines, top.map(|frame| (frame.line() as usize, frame.column() as usize)))
        }
        None => (
            error.stack_string().map(|stack| rust_string(&stack)).unwrap_or_default(),
            (error.line() >= 1 && error.column() >= 1).then(|| (error.line() as usize, error.column() as usize)),
        ),
    };
    let (properties, code) = error_properties(global_object, error, colors);
    let own_source = error.source_text();
    let source = if source.is_empty() { own_source.as_deref().unwrap_or("") } else { source };
    format_error(source, &rust_string(&error.message()), &report_name(global_object, error), &stack, position, cause, &properties, code.as_deref(), colors)
}

/// O nome do cabeçalho: a primeira propriedade de dados `name` na cadeia do erro (o acessor é pulado, sem chamar o
/// getter), convertida em texto quando é string, número, booleano ou `null`; `undefined`, símbolo, objeto ou nenhuma
/// deixam o nome vazio (o cabeçalho escreve `error`). Medido no bun 1.4.2: `e.name = 'Foo'` num `TypeError` sai `Foo: m`,
/// `e.name = 5` sai `5: m`, um getter próprio cai no `TypeError.prototype.name` e `e.name = undefined` sai `error: m`.
fn report_name(global_object: &JSGlobalObject, error: &ErrorInstanceRef) -> String {
    let vm = global_object.vm();
    let name = PropertyName::from_identifier(&vm.property_names.name);
    let mut level = error.as_value();
    while level.is_object() {
        let object = level.as_object();
        let mut descriptor = crate::runtime::property_descriptor::PropertyDescriptor::default();
        if object.get_own_property_descriptor(vm, &name, &mut descriptor) && !descriptor.is_accessor_descriptor() {
            let value = descriptor.value();
            return if value.is_string() || value.is_number() || value.is_boolean() || value.is_null() { rust_string(&value.to_wtf_string()) } else { String::new() };
        }
        level = object.get_prototype_direct();
    }
    String::new()
}

/// As propriedades próprias enumeráveis do erro, como `print_error_instance_body` (VirtualMachine.rs) as escreve: cada
/// uma ` nome: valor,` alinhada à direita pelo maior nome (no máximo 10), valor formatado com aspas nas strings; `message`,
/// `name` e `stack` são pulados, um `Error` como valor fica para a cadeia de `cause`, e um `code` string de até 8 bits sai
/// por último, entre aspas e sem vírgula. Cada linha termina em `\n`. Devolve também o `code` string de até 8 bits, que
/// o cabeçalho usa (`print_error_name_and_message`).
fn error_properties(global_object: &JSGlobalObject, error: &ErrorInstanceRef, colors: bool) -> (Vec<u8>, Option<String>) {
    let vm = global_object.vm();
    let value = error.as_value();
    let Ok(keys) = global_object.own_enumerable_string_keys(value) else { return (Vec::new(), None) };
    let entries: Vec<(Vec<u16>, PropertyName)> = keys
        .into_iter()
        .filter_map(|key| match key {
            JsonKey::Name(name) => Some((wtf_string_to_units(&name), PropertyName::from_identifier(&Identifier::from_string(vm, &name)))),
            JsonKey::Index(_) => None,
        })
        .filter(|(units, _)| !units.is_empty())
        .collect();
    let longest = entries.iter().map(|(units, _)| units.len()).max().unwrap_or(0).min(10);
    let object = value.as_object();
    let is = |units: &[u16], text: &str| units.iter().copied().eq(text.bytes().map(u16::from));
    let code = entries.iter().find(|(units, _)| is(units, "code")).and_then(|(_, name)| {
        let code = object.get(global_object, name);
        (code.is_string() && !global_object.vm().exception().is_some()).then(|| wtf_string_to_units(&code.to_wtf_string())).filter(|units| units.iter().all(|unit| *unit < 256))
    });
    let mut formatter = Formatter::with_depth(1, 1).with_colors(colors).inside_error_report();
    let mut out = Vec::new();
    for (units, name) in &entries {
        if is(units, "message") || is(units, "name") || is(units, "stack") || (code.is_some() && is(units, "code")) {
            continue;
        }
        let property = object.get(global_object, name);
        if vm.exception().is_some() {
            return (out, None);
        }
        if matches!(&property, JSValue::Cell(cell_id) if ErrorInstance::from_cell_id(*cell_id).is_some()) {
            continue;
        }
        let mut shown = Vec::new();
        if !formatter.push_object(global_object, &mut shown, property) {
            return (out, None);
        }
        out.extend(std::iter::repeat(b' ').take(longest.saturating_sub(units.len())));
        let (colon, comma) = if colors { ("\x1b[0m\x1b[2m:\x1b[0m", "\x1b[0m\x1b[2m,\x1b[0m") } else { (":", ",") };
        out.extend_from_slice(format!(" {}{colon} {}{comma}\n", String::from_utf16_lossy(units), String::from_utf16_lossy(&shown)).as_bytes());
    }
    let code = code.map(|code| String::from_utf16_lossy(&code));
    if let Some(code) = &code {
        out.extend(std::iter::repeat(b' ').take(longest.saturating_sub("code".len())));
        let line = if colors { format!(" code\x1b[0m\x1b[2m:\x1b[0m \x1b[32m\"{code}\"\x1b[0m\n") } else { format!(" code: \"{code}\"\n") };
        out.extend_from_slice(line.as_bytes());
    }
    (out, code)
}

/// O deslocamento em bytes de `source` da posição (`line`, `column`), base um, onde a linha termina em `\n`, `\r`,
/// `\r\n`, U+2028 ou U+2029 (como o JSC conta) e a coluna conta unidades UTF-16.
fn byte_offset(source: &str, line: usize, column: usize) -> Option<usize> {
    let mut chars = source.char_indices().peekable();
    let mut line_start = 0;
    for _ in 1..line {
        loop {
            let (_, c) = chars.next()?;
            if c == '\r' && matches!(chars.peek(), Some(&(_, '\n'))) {
                chars.next();
            }
            if matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                break;
            }
        }
        line_start = chars.peek().map_or(source.len(), |&(index, _)| index);
    }
    let mut units = 0;
    let mut offset = line_start;
    for c in source[line_start..].chars() {
        if units + 1 >= column || matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            break;
        }
        units += c.len_utf16();
        offset += c.len_utf8();
    }
    Some(offset)
}

/// O texto de uma linha como o bun o mostra: sem o espaço e o tab do fim (e sem o `\r`, salvo na primeira linha do
/// arquivo, onde o `\r` fica e impede a poda do que vem antes dele), cortado em [`MAX_LINE_BYTES`] bytes.
fn trimmed_line(piece: &str, at_file_start: bool) -> &str {
    if at_file_start { piece.trim_end_matches([' ', '\t']) } else { piece.trim_end_matches([' ', '\t', '\r']) }
}

/// Acrescenta a `out` o trecho do fonte (até [`CONTEXT_LINES`] linhas antes da do erro, partindo o fonte só em `\n`) e a
/// linha do `^`. O número de cada linha vem do contador do JSC (`line` menos a distância até a linha do erro). Quando a
/// linha do erro é a última, sem `\n` no fim, e não é a primeira, o bun não mostra o texto dela e numera as anteriores
/// uma unidade acima. A linha do `^` sai vazia se a linha do erro passa de 1024 bytes, ou se ela tem ao menos 512 bytes e
/// a coluna é menor que 514.
fn append_source_excerpt(out: &mut Vec<u8>, source: &str, line: usize, column: usize, colors: bool) {
    let Some(offset) = byte_offset(source, line, column) else { return };
    let pieces: Vec<&str> = source.split('\n').collect();
    let (mut error_index, mut start) = (0, 0);
    for (index, piece) in pieces.iter().enumerate() {
        if start > offset {
            break;
        }
        error_index = index;
        start += piece.len() + 1;
    }
    let unterminated = error_index > 0 && error_index + 1 == pieces.len();
    let last = if unterminated { error_index - 1 } else { error_index };
    let width = line.to_string().len();
    for index in error_index.saturating_sub(CONTEXT_LINES)..=last {
        let number = (line + usize::from(unterminated)).saturating_sub(error_index - index);
        let trimmed = trimmed_line(pieces[index], index == 0).as_bytes();
        let shown = &trimmed[..trimmed.len().min(MAX_LINE_BYTES)];
        if colors {
            // `<r><b>{n} |<r> {fonte}`, e `<r><d> | ... truncated <r>` quando a linha foi cortada; o erro, se cortado, ganha `\n` a mais.
            let digits = number.to_string();
            out.extend(std::iter::repeat(b' ').take(width.saturating_sub(digits.len())));
            out.extend_from_slice(format!("\x1b[0m\x1b[1m{digits} |\x1b[0m ").as_bytes());
            highlight_javascript(out, shown, true);
            if shown.len() != trimmed.len() {
                out.extend_from_slice(b"\x1b[0m\x1b[2m | ... truncated \x1b[0m\n");
                if index == error_index {
                    out.push(b'\n');
                    return;
                }
            } else {
                out.push(b'\n');
            }
        } else {
            out.extend_from_slice(format!("{number:>width$} | ").as_bytes());
            out.extend_from_slice(shown);
            out.push(b'\n');
        }
    }
    let length = trimmed_line(pieces[error_index], error_index == 0).len();
    if length > MAX_LINE_BYTES || (length >= CARET_MIN_LINE_BYTES && column < CARET_MIN_COLUMN) {
        out.push(b'\n');
    } else {
        out.extend_from_slice(" ".repeat(width + 3 + column - 1).as_bytes());
        out.extend_from_slice(if colors { b"\x1b[31m\x1b[1m^\x1b[0m\n".as_slice() } else { b"^\n".as_slice() });
    }
}

/// O cabeçalho do bloco, como `print_error_name_and_message`: com nome e mensagem, `nome: mensagem` (um `Error` mostra
/// `error`, ou o `code` quando a mensagem começa por `code: `, que então sai do texto); só com nome, `nome: `; só com
/// mensagem, `error: mensagem`; sem nenhum dos dois, `error`. Com `colors`, o `ErrorDisplayLevelFormatter` do bun
/// (`<r>nome<r><d>:<r> ` e a mensagem em `<b>...<r>`).
fn error_header(name: &str, message: &str, code: Option<&str>, colors: bool) -> String {
    let (shown_name, shown_message) = match (name.is_empty(), message.is_empty()) {
        (false, false) if name != "Error" => (name.to_string(), Some(message.to_string())),
        (false, false) => {
            let units: Vec<u16> = message.encode_utf16().collect();
            let stripped = code.filter(|code| code.is_ascii()).filter(|code| {
                let prefix: Vec<u16> = code.encode_utf16().collect();
                units.len() > prefix.len() + 3 && units.starts_with(&prefix) && units[prefix.len()] == u16::from(b':') && units[prefix.len() + 1] == u16::from(b' ')
            });
            match stripped {
                Some(code) => (code.to_string(), Some(String::from_utf16_lossy(&units[code.len() + 2..]))),
                None => (String::new(), Some(message.to_string())),
            }
        }
        (false, true) => (name.to_string(), None),
        (true, false) => (String::new(), Some(message.to_string())),
        (true, true) => (String::new(), None),
    };
    let label = if shown_name.is_empty() { "error" } else { shown_name.as_str() };
    match (&shown_message, name.is_empty() && message.is_empty()) {
        (_, true) if colors => format!("\x1b[0m{label}\x1b[0m"),
        (_, true) => label.to_string(),
        (Some(text), _) if colors => format!("\x1b[0m{label}\x1b[0m\x1b[2m:\x1b[0m \x1b[1m{text}\x1b[0m"),
        (Some(text), _) => format!("{label}: {text}"),
        (None, _) if colors => format!("\x1b[0m{label}\x1b[0m\x1b[2m:\x1b[0m "),
        (None, _) => format!("{label}: "),
    }
}

/// Um frame `at ...` do texto de `Error.stack` como `print_stack_trace` o escreve com cores: nome em negrito itálico,
/// o `SourceURLFormatter` (arquivo ciano, linha e coluna amarelas, separadores apagados) e o `at` apagado.
fn colored_frame(frame: &str) -> String {
    let text = frame.strip_prefix("at ").unwrap_or(frame);
    let (name, location) = match text.strip_suffix(')').and_then(|inner| inner.split_once(" (")) {
        Some((name, location)) => (Some(name), location),
        None => (None, text),
    };
    let numeric = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let mut parts = location.rsplitn(3, ':');
    let (last, middle, rest) = (parts.next(), parts.next(), parts.next());
    let source = match (last, middle, rest) {
        (Some(column), Some(line), Some(file)) if numeric(column) && numeric(line) => {
            format!("\x1b[0m\x1b[36m{file}\x1b[0m\x1b[2m:\x1b[0m\x1b[33m{line}\x1b[0m\x1b[2m:\x1b[33m{column}\x1b[0m")
        }
        (Some(line), Some(_), _) if numeric(line) => {
            let file = &location[..location.len() - line.len() - 1];
            format!("\x1b[0m\x1b[36m{file}\x1b[0m\x1b[2m:\x1b[0m\x1b[33m{line}\x1b[0m")
        }
        _ => format!("\x1b[0m\x1b[36m{location}\x1b[0m"),
    };
    match name {
        Some(name) => {
            let shown = match name.strip_prefix("new ") {
                Some(_) => name.to_string(),
                None => format!("\x1b[0m\x1b[1m\x1b[3m{name}\x1b[0m"),
            };
            format!("\x1b[0m      \x1b[2mat \x1b[0m{shown}\x1b[2m (\x1b[0m{source}\x1b[2m)\x1b[0m\n")
        }
        None => format!("\x1b[0m      \x1b[2mat \x1b[0m{source}\n"),
    }
}

/// O bloco de um `Error`: o trecho do fonte até a linha do erro (se há posição), o cabeçalho e os frames.
fn format_error(source: &str, message: &str, name: &str, stack: &str, position: Option<(usize, usize)>, cause: Option<&[u8]>, properties: &[u8], code: Option<&str>, colors: bool) -> Vec<u8> {
    let frames: Vec<&str> = stack.lines().skip(1).map(str::trim).filter(|line| line.starts_with("at ")).collect();
    let mut out = Vec::new();
    if let Some((line, column)) = position {
        append_source_excerpt(&mut out, source, line, column, colors);
    }
    out.extend_from_slice(error_header(name, message, code, colors).as_bytes());
    out.push(b'\n');
    if let Some(cause) = cause {
        out.extend_from_slice(b" cause: ");
        out.extend_from_slice(cause);
        out.extend_from_slice(b",\n");
    }
    out.extend_from_slice(properties);
    if cause.is_some() || !properties.is_empty() {
        out.push(b'\n');
    }
    for frame in &frames {
        let frame = without_builtin_url(frame);
        if colors {
            out.extend_from_slice(colored_frame(&frame).as_bytes());
        } else {
            out.extend_from_slice(format!("      {frame}\n").as_bytes());
        }
    }
    out
}

/// O relato do bun monta os frames a partir do `ZigStackFrame`, em que a URL de um builtin em JS é vazia (o `native`
/// só entra no texto de `Error.stack`, `FormatStackTraceForJS.cpp`): `new Promise (1:11)`, não `new Promise (native:1:11)`.
fn without_builtin_url(frame: &str) -> String {
    match frame.rfind(" (native:") {
        Some(at) if frame.ends_with(')') => format!("{} ({}", &frame[..at], &frame[at + " (native:".len()..]),
        _ => frame.to_string(),
    }
}
