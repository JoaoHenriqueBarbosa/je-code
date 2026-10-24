//! Tradução de `createErrorForDuplicateGlobalVariableDeclaration`,
//! `createErrorForInvalidGlobalFunctionDeclaration` e `createErrorForInvalidGlobalVarDeclaration`
//! de `runtime/ExceptionHelpers.{h,cpp}`.
//!
//! e de `createUndefinedVariableError`, `createTDZError`, `createNotAFunctionError`,
//! `createNotAConstructorError`, `errorDescriptionForValue` e `createError(globalObject, value, message, appender)`.
//! `createStackOverflowError` e `createOutOfMemoryError` são reexportados de `error.rs`, onde
//! já estão.
//!
//! `createInvalidFunctionApplyParameterError` (o `TypeError` de `sizeOfVarargs`) também está aqui.
//!
//! Fora desta fatia: os demais `createInvalid*ParameterError`,
//! `InterruptedExecutionError`/`TerminatedExecutionError`...
//!
//! DIVERGÊNCIAS:
//!
//! - `tryMakeString(...)` falha por estouro de tamanho, o que `try_make_string_dyn`
//!   reproduz com `None`; o texto de reserva é o do C++.
//! - Vale `USE(BUN_JSC_ADDITIONS)` ligado: `createUndefinedVariableError` diz `x is not defined`.
//! - `ErrorInstance` não guarda `m_sourceAppender` (veja `error.rs`): `createNotAFunctionError` e
//!   `createNotAConstructorError` recebem o [`ErrorSite`] (o `CodeBlock` e o `BytecodeIndex` que o
//!   `ErrorInstance::finishCreation` tira do `topCallFrame`) e aplicam o appender na criação
//!   (`appendSourceToErrorMessage`), dando ` (evaluating '...')` / `(In 'foo.bar(...)', 'foo.bar' is ...)`.
//!   Sem `site` a mensagem fica pura. Só `defaultSourceAppender` e `notAFunctionSourceAppender` foram
//!   portados; os `invalidParameter*` entram com `createInvalid*ParameterError`.
//! - `errorDescriptionForValue` de objeto usa `JSObject::calculatedClassName` ([`calculated_class_name`]);
//!   o `BigInt` cai no `toString` do valor, como no C++.

use std::rc::Rc;

pub use crate::runtime::error::{create_out_of_memory_error, create_stack_overflow_error};

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::code_block::{CodeBlock, CodeBlockRef};
use crate::runtime::parse_int::is_str_white_space;
use crate::wtf::text::string_impl::deprecated_is_space_or_newline;
use crate::runtime::runtime_type::RuntimeType;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::internal_function::InternalFunction;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::wtf::text::atom_string::AtomString;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::Exception;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectHandle;
use crate::wtf::text::string_concatenate::{try_make_string_dyn, StringTypeAdapter};
use crate::wtf::text::string_impl::UniquedKey;
use crate::wtf::text::wtf_string::String as WtfString;

/// `createErrorForDuplicateGlobalVariableDeclaration(JSGlobalObject*, UniquedStringImpl*)`.
pub fn create_error_for_duplicate_global_variable_declaration(global_object: &JSGlobalObject, key: &UniquedKey) -> JSObjectHandle {
    let message = try_make_string_dyn(&[&"Can't create duplicate variable: '", &WtfString::from(std::rc::Rc::clone(&key.0)), &'\''])
        .unwrap_or_else(|| WtfString::from_latin1(b"Can't create duplicate variable"));
    crate::runtime::error::create_syntax_error(global_object, &message)
}

/// `createErrorForInvalidGlobalFunctionDeclaration(JSGlobalObject*, const Identifier&)`.
pub fn create_error_for_invalid_global_function_declaration(global_object: &JSGlobalObject, ident: &Identifier) -> JSObjectHandle {
    let message = try_make_string_dyn(&[
        &"Can't declare global function '",
        &ident.string().string(),
        &"': property must be either configurable or both writable and enumerable",
    ])
    .unwrap_or_else(|| {
        WtfString::from_latin1(b"Can't declare global function: property must be either configurable or both writable and enumerable")
    });
    crate::runtime::error::create_type_error(global_object, &message)
}

/// `createErrorForInvalidGlobalVarDeclaration(JSGlobalObject*, const Identifier&)`.
pub fn create_error_for_invalid_global_var_declaration(global_object: &JSGlobalObject, ident: &Identifier) -> JSObjectHandle {
    let message = try_make_string_dyn(&[&"Can't declare global variable '", &ident.string().string(), &"': global object must be extensible"])
        .unwrap_or_else(|| WtfString::from_latin1(b"Can't declare global variable: global object must be extensible"));
    crate::runtime::error::create_type_error(global_object, &message)
}

/// `createUndefinedVariableError(JSGlobalObject*, const Identifier&)` (ramo `USE(BUN_JSC_ADDITIONS)`).
pub fn create_undefined_variable_error(global_object: &JSGlobalObject, ident: &Identifier) -> JSObjectHandle {
    let message = if ident.is_private_name() {
        try_make_string_dyn(&[&"Can't find private variable: PrivateSymbol.", &ident.string().string()])
            .unwrap_or_else(|| WtfString::from_latin1(b"Can't find private variable"))
    } else {
        try_make_string_dyn(&[&ident.string().string(), &" is not defined"])
            .unwrap_or_else(|| WtfString::from_latin1(b"Variable is not defined"))
    };
    crate::runtime::error::create_reference_error(global_object, &message)
}

/// `createTDZError(JSGlobalObject*)`.
pub fn create_tdz_error_without_name(global_object: &JSGlobalObject) -> JSObjectHandle {
    crate::runtime::error::create_reference_error(global_object, &WtfString::from_latin1(b"Cannot access uninitialized variable."))
}

/// `createTDZError(JSGlobalObject*, StringView)`.
pub fn create_tdz_error(global_object: &JSGlobalObject, ident: &AtomString) -> JSObjectHandle {
    tdz_error_for_text(global_object, ident.string())
}

/// A mensagem `Cannot access '<text>' before initialization.` comum a `createTDZError(StringView)` e ao
/// nome tirado do trecho do fonte; sem memória para montá-la, `createTDZError` sem nome.
fn tdz_error_for_text(global_object: &JSGlobalObject, text: &dyn StringTypeAdapter) -> JSObjectHandle {
    match try_make_string_dyn(&[&"Cannot access '", text, &"' before initialization."]) {
        Some(message) => crate::runtime::error::create_reference_error(global_object, &message),
        None => create_tdz_error_without_name(global_object),
    }
}

/// `slow_path_check_tdz` (CommonSlowPaths.cpp:318): o nome do `ReferenceError` não vem do operando da
/// instrução, e sim do trecho do fonte `[divot - startOffset, divot + endOffset)` da expressão em
/// `getBytecodeIndex(vm, callFrame)` (por isso o bun diz `'globalThis'` ou `'}'` quando a expressão que lê
/// a variável é outra, e `''` quando o frame é privado, como o inicializador de campo de classe: a expression
/// info vem do primeiro frame visível acima dele, e o trecho é lido do fonte do `codeBlock` corrente). Sem
/// expression info, o trecho é vazio (`Cannot access '' before initialization.`).
pub fn create_tdz_error_from_source_range(global_object: &JSGlobalObject, site: &BlockErrorSite<'_>) -> JSObjectHandle {
    let (info, source_block) = match &site.visible_caller {
        Some((caller, index)) => {
            let caller = caller.borrow();
            if !caller.unlinked_code_block().borrow().has_expression_info() {
                // O C++ não checa: a entrada vazia tem divot 0 e dá o trecho vazio, `Cannot access '' ...`.
                return tdz_error_for_text(global_object, &"");
            }
            (caller.expression_info_for_bytecode_index(*index), site.code_block)
        }
        None => {
            if !site.code_block.unlinked_code_block().borrow().has_expression_info() {
                return tdz_error_for_text(global_object, &"");
            }
            (site.code_block.expression_info_for_bytecode_index(site.bytecode_index), site.code_block)
        }
    };
    let start = info.divot as i64 - info.start_offset as i64;
    let stop = info.divot as i64 + info.end_offset as i64;
    let source = source_block.source();
    let Some(provider) = source.provider() else {
        return create_tdz_error_without_name(global_object);
    };
    let text = provider.get_range(start as i32, stop as i32);
    tdz_error_for_text(global_object, &text)
}

/// O ramo comum de `JSObject::calculatedClassName` que lê `constructor` de um slot: o
/// `calculatedDisplayName` do `JSFunction` ou do `InternalFunction`, nulo para qualquer outro valor.
fn constructor_display_name(global_object: &JSGlobalObject, constructor: JSValue) -> WtfString {
    if !constructor.is_object() {
        return WtfString::default();
    }
    if let Some(function) = constructor.as_js_function() {
        return function.calculated_display_name(global_object.vm(), global_object);
    }
    if let Some(function) = InternalFunction::from_cell_id(constructor.as_cell()) {
        return function.calculated_display_name(global_object.vm());
    }
    WtfString::default()
}

/// `JSObject::calculatedClassName(object)`: o nome de `obj.constructor`, ou de
/// `obj.__proto__.constructor`, ou o `@@toStringTag` string, ou o `className` da `ClassInfo`. As consultas
/// são `VMInquiry` (não chamam getter); a exceção que o `getOwnPropertySlot` deixe é descartada.
pub fn calculated_class_name(object: &ObjectRef) -> WtfString {
    let Some(global_object) = object.structure().realm() else {
        return WtfString::from_latin1(object.class_info().class_name.as_bytes());
    };
    let vm = global_object.vm();
    let had_exception = vm.exception().is_some();
    let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);
    let mut constructor_function_name = WtfString::default();

    // Check for a display name of obj.constructor.
    // This is useful to get `Foo` for the `(class Foo).prototype` object.
    let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::VMInquiry);
    if object.get_own_property_slot(&global_object, &constructor_name, &mut slot) && slot.is_value() {
        constructor_function_name = constructor_display_name(&global_object, slot.get_value_for(&constructor_name));
    }
    if !had_exception && vm.exception().is_some() {
        vm.clear_exception();
        constructor_function_name = WtfString::default();
    }

    // Get the display name of obj.__proto__.constructor.
    // This is useful to get `Foo` for a `new Foo` object.
    if constructor_function_name.is_null() && !object.structure().type_info().overrides_get_prototype() {
        let proto_value = object.get_prototype_direct();
        if let Some(proto_object) = ObjectRef::from_value(&proto_value).filter(|_| proto_value.is_object()) {
            let mut slot = PropertySlot::new(proto_value, InternalMethodType::VMInquiry);
            if proto_object.get_property_slot(&global_object, &constructor_name, &mut slot) && slot.is_value() {
                constructor_function_name = constructor_display_name(&global_object, slot.get_value_for(&constructor_name));
            }
        }
    }
    if !had_exception && vm.exception().is_some() {
        vm.clear_exception();
        constructor_function_name = WtfString::default();
    }

    if constructor_function_name.is_null() || constructor_function_name == WtfString::from_latin1(b"Object") {
        let tag_name = PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol);
        let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::VMInquiry);
        if object.get_property_slot(&global_object, &tag_name, &mut slot) && slot.is_value() {
            let value = slot.get_value_for(&tag_name);
            if value.is_string() {
                return value.as_js_string().value();
            }
        }
        if !had_exception && vm.exception().is_some() {
            vm.clear_exception();
        }

        let class_info_name = object.class_info().class_name;
        if !class_info_name.is_empty() {
            return WtfString::from_latin1(class_info_name.as_bytes());
        }

        if constructor_function_name.is_null() {
            return WtfString::from_latin1(b"Object");
        }
    }

    constructor_function_name
}

/// `errorDescriptionForValue(JSGlobalObject*, JSValue)`, sem exceção possível.
pub fn error_description_for_value(value: JSValue) -> WtfString {
    if value.is_string() {
        return try_make_string_dyn(&[&'"', &value.to_wtf_string(), &'"']).unwrap_or_default();
    }
    if value.is_symbol() {
        if let Some(CellEntry::Symbol(symbol)) = cell_registry::get(value.as_cell()) {
            return symbol.try_get_descriptive_string().unwrap_or_else(|_| WtfString::from_latin1(b"Symbol"));
        }
    }
    if value.is_object() {
        if let Some(object) = ObjectRef::from_value(&value) {
            if value.is_callable() {
                return WtfString::from_latin1(b"function");
            }
            return calculated_class_name(&object);
        }
        return WtfString::from_latin1(b"Object");
    }
    value.to_wtf_string()
}


/// `ErrorInstance::SourceTextWhereErrorOccurred`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceTextWhereErrorOccurred {
    FoundApproximateSource,
    FoundExactSource,
}

/// `ErrorInstance::SourceAppender`: `(originalMessage, sourceText, type, occurrence)` para a mensagem
/// com o trecho do fonte. Os textos são sequências de unidades UTF-16 (o `StringView` do C++).
pub type SourceAppender = fn(&[u16], &[u16], RuntimeType, SourceTextWhereErrorOccurred) -> Vec<u16>;

/// Onde o erro nasce: o `CodeBlock` do frame que executa a instrução e o `BytecodeIndex` dela. É o
/// `getBytecodeIndex(vm, vm.topCallFrame)` que `ErrorInstance::finishCreation` consulta; o laço do LLInt
/// o conhece sem passar pelo `topCallFrame` do `VM`.
pub struct ErrorSite<'a> {
    pub code_block: &'a CodeBlockRef,
    pub bytecode_index: BytecodeIndex,
    /// O frame que executa a instrução e o `VM`, para `visible_location` achar o frame visível acima dele
    /// quando o corrente é privado (o cálculo é sob demanda: só o caminho de erro paga a caminhada).
    pub vm: &'a crate::runtime::vm::VM,
    pub call_frame: crate::interpreter::call_frame::CallFrame,
}

impl ErrorSite<'_> {
    /// `getBytecodeIndex(vm, topCallFrame)` (`Error.cpp`): o frame visível que o erro cita, ou o corrente
    /// quando ele já é visível.
    pub fn visible_location(&self) -> (CodeBlockRef, BytecodeIndex) {
        visible_caller_site(self.vm, self.call_frame).unwrap_or_else(|| (self.code_block.clone(), self.bytecode_index))
    }
}

/// `getBytecodeIndex(vm, topCallFrame)` (`Error.cpp`): percorre a pilha de `call_frame` para fora e devolve o
/// primeiro frame que tem `CodeBlock`, não é nativo e não é de visibilidade privada (`FindFirstCallerFrameWith
/// CodeblockFunctor`). `None` se esse for o próprio `call_frame` (vale o `CodeBlock` já em mãos).
pub fn visible_caller_site(vm: &crate::runtime::vm::VM, call_frame: crate::interpreter::call_frame::CallFrame) -> Option<(CodeBlockRef, BytecodeIndex)> {
    use crate::interpreter::stack_visitor::{IterationStatus, StackVisitor};
    let interpreter = vm.interpreter();
    let mut found = None;
    StackVisitor::visit(&interpreter, Some(call_frame), false, |frame| {
        if frame.is_native_callee_frame() {
            return IterationStatus::Continue;
        }
        let Some(code_block) = frame.code_block() else {
            return IterationStatus::Continue;
        };
        // `if (!isBuiltinFunction()) m_bytecodeIndex = visitor->bytecodeIndex()`: num builtin o índice fica em 0
        // (o `BytecodeIndex { 0 }` do functor), então o trecho do usuário não é citado, mesmo no frame do topo.
        let is_builtin = code_block.borrow().unlinked_code_block().borrow().is_builtin_function();
        // O bun 1.4.2 não pula o builtin privado (`performIteration` do spread, `Iterator.from`, `Object.fromEntries`):
        // `[...X]` com `next` não chamável dá `1 is not a function` sem `(near '...')` do spread do usuário. Só o
        // frame privado que não é builtin (o inicializador de campo de classe sintetizado) é pulado.
        if frame.is_implementation_visibility_private() && !is_builtin {
            return IterationStatus::Continue;
        }
        if is_builtin {
            found = Some((code_block.clone(), BytecodeIndex::from_offset(0)));
        } else if frame.index() != 0 {
            found = Some((code_block.clone(), frame.bytecode_index()));
        }
        IterationStatus::Done
    });
    found
}

/// Onde o erro nasce, quando o chamador (um handler de slow path) tem o `CodeBlock` emprestado em vez do
/// `CodeBlockRef`: o mesmo `getBytecodeIndex(vm, vm.topCallFrame)` do `ErrorSite`.
pub struct BlockErrorSite<'a> {
    pub code_block: &'a CodeBlock,
    pub bytecode_index: BytecodeIndex,
    /// `FindFirstCallerFrameWithCodeblockFunctor` (`Error.cpp`): quando o frame em execução tem visibilidade
    /// privada (o inicializador de campos de classe sintetizado, os builtins), o erro cita o primeiro frame
    /// acima dele que não é privado (o `super(...args)` do construtor que o chamou). `None` quando o próprio
    /// frame é visível.
    pub visible_caller: Option<(CodeBlockRef, BytecodeIndex)>,
}

/// O que `ErrorInstance::finishCreation` faz com o frame do topo: acrescenta à mensagem o trecho do fonte
/// da instrução. As duas formas de apontar o `CodeBlock` (`ErrorSite`, `BlockErrorSite`) o implementam.
pub trait SourceSite {
    fn append_source(&self, message: &WtfString, type_: RuntimeType, appender: SourceAppender) -> WtfString;
}

impl SourceSite for ErrorSite<'_> {
    fn append_source(&self, message: &WtfString, type_: RuntimeType, appender: SourceAppender) -> WtfString {
        let (code_block, bytecode_index) = self.visible_location();
        append_source_to_error_message(&code_block, bytecode_index, message, type_, appender)
    }
}

/// O site de `createNotAFunctionError` lançado pelo `handleHostCall`: o bun cita a chamada no próprio frame que a
/// executa, mesmo quando ele é o inicializador de campo de classe sintetizado (visibilidade privada), ao contrário
/// do erro de `get_by_id` e afins, que sobe até o construtor que o chamou (`getBytecodeIndex`). Só o frame de
/// builtin segue o caminho comum (`visible_location`), que o cita com índice 0.
pub struct CallErrorSite<'a, 'b>(pub &'a ErrorSite<'b>);

impl SourceSite for CallErrorSite<'_, '_> {
    fn append_source(&self, message: &WtfString, type_: RuntimeType, appender: SourceAppender) -> WtfString {
        let site = self.0;
        if site.code_block.borrow().unlinked_code_block().borrow().is_builtin_function() {
            return site.append_source(message, type_, appender);
        }
        append_source_to_error_message(site.code_block, site.bytecode_index, message, type_, appender)
    }
}

impl SourceSite for BlockErrorSite<'_> {
    fn append_source(&self, message: &WtfString, type_: RuntimeType, appender: SourceAppender) -> WtfString {
        match &self.visible_caller {
            Some((code_block, bytecode_index)) => append_source_to_error_message(code_block, *bytecode_index, message, type_, appender),
            None => append_source_to_error_message_in_block(self.code_block, self.bytecode_index, message, type_, appender),
        }
    }
}

fn units(string: &WtfString) -> Vec<u16> {
    (0..string.length()).map(|index| string.code_unit_at(index)).collect()
}

fn concat_units(parts: &[&[u16]]) -> Vec<u16> {
    parts.concat()
}

fn ascii_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `reverseFind(needle)`: o índice da última ocorrência, `None` no `notFound`.
fn reverse_find_units(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).rev().find(|&index| haystack[index..index + needle.len()] == *needle)
}

/// `clampErrorMessage`: no máximo 2 KB de unidades.
fn clamp_error_message(original_message: &[u16]) -> &[u16] {
    &original_message[..original_message.len().min(2 * 1024)]
}

/// `defaultApproximateSourceError`.
fn default_approximate_source_error(original_message: &[u16], source_text: &[u16]) -> Vec<u16> {
    concat_units(&[clamp_error_message(original_message), &ascii_units(" (near '..."), source_text, &ascii_units("...')")])
}

/// `defaultSourceAppender`.
pub fn default_source_appender(
    original_message: &[u16],
    source_text: &[u16],
    _type: RuntimeType,
    occurrence: SourceTextWhereErrorOccurred,
) -> Vec<u16> {
    if occurrence == SourceTextWhereErrorOccurred::FoundApproximateSource {
        return default_approximate_source_error(original_message, source_text);
    }
    concat_units(&[clamp_error_message(original_message), &ascii_units(" (evaluating '"), source_text, &ascii_units("')")])
}

/// `functionCallBase`: o `foo.bar` de `foo.bar(baz)`, varrendo da direita para a esquerda; vazio quando o
/// texto não termina em `)` ou o balanceamento de parênteses falha.
fn function_call_base(source_text: &[u16]) -> &[u16] {
    let source_length = source_text.len();
    if source_length < 2 || source_text[source_length - 1] != u16::from(b')') {
        return &[];
    }
    let (open, close, slash, star, dot, question) =
        (u16::from(b'('), u16::from(b')'), u16::from(b'/'), u16::from(b'*'), u16::from(b'.'), u16::from(b'?'));
    let mut idx = source_length - 1;
    let mut paren_stack = 1u32;
    let mut is_in_multi_line_comment = false;
    idx -= 1;
    while paren_stack != 0 && idx != 0 {
        let cur_char = source_text[idx];
        if is_in_multi_line_comment {
            if cur_char == star && source_text[idx - 1] == slash {
                is_in_multi_line_comment = false;
                idx -= 1;
            }
        } else if cur_char == open {
            paren_stack -= 1;
        } else if cur_char == close {
            paren_stack += 1;
        } else if cur_char == slash && source_text[idx - 1] == star {
            is_in_multi_line_comment = true;
            idx -= 1;
        }
        if idx != 0 {
            idx -= 1;
        }
    }
    if paren_stack != 0 {
        return &[];
    }
    // Don't display the ?. of an optional call.
    if idx > 1 && source_text[idx] == dot && source_text[idx - 1] == question {
        idx -= 2;
    }
    &source_text[..idx + 1]
}

/// `notAFunctionSourceAppender`: `foo.bar is not a function. (In 'foo.bar(baz)', 'foo.bar' is undefined)`.
pub fn not_a_function_source_appender(
    original_message: &[u16],
    source_text: &[u16],
    type_: RuntimeType,
    occurrence: SourceTextWhereErrorOccurred,
) -> Vec<u16> {
    debug_assert!(type_ != RuntimeType::TypeFunction);
    if occurrence == SourceTextWhereErrorOccurred::FoundApproximateSource {
        return default_approximate_source_error(original_message, source_text);
    }
    let not_a_function_index =
        reverse_find_units(original_message, &ascii_units("is not a function")).expect("mensagem sem 'is not a function'");
    let display_value = &original_message[..not_a_function_index.saturating_sub(1)];

    let base = function_call_base(source_text);
    if base.is_empty() {
        return default_approximate_source_error(original_message, source_text);
    }
    let description: Vec<u16> = match type_ {
        RuntimeType::TypeSymbol => ascii_units("a Symbol"),
        RuntimeType::TypeObject => concat_units(&[&ascii_units("an instance of "), display_value]),
        _ => display_value.to_vec(),
    };
    concat_units(&[
        base,
        &ascii_units(" is not a function. (In '"),
        source_text,
        &ascii_units("', '"),
        base,
        &ascii_units("' is "),
        &description,
        &ascii_units(")"),
    ])
}

/// `runtimeTypeForValue(JSValue)`. `isAnyInt` aqui é só o `int32`: o resto dos números é `TypeNumber`,
/// diferença que nenhum appender observa.
fn runtime_type_for_value(value: JSValue) -> RuntimeType {
    if value.is_undefined() {
        return RuntimeType::TypeUndefined;
    }
    if value.is_null() {
        return RuntimeType::TypeNull;
    }
    if value.is_int32() {
        return RuntimeType::TypeAnyInt;
    }
    if value.is_number() {
        return RuntimeType::TypeNumber;
    }
    if value.is_string() {
        return RuntimeType::TypeString;
    }
    if value.is_cell() {
        if let Some(entry) = cell_registry::get(value.as_cell()) {
            match &entry {
                CellEntry::Symbol(_) => return RuntimeType::TypeSymbol,
                CellEntry::BigInt(_) => return RuntimeType::TypeBigInt,
                _ => {}
            }
            if entry.as_js_object().is_some() {
                return if matches!(
                    entry.js_type(),
                    JSType::JSFunctionType | JSType::InternalFunctionType | JSType::NullSetterFunctionType
                ) {
                    RuntimeType::TypeFunction
                } else {
                    RuntimeType::TypeObject
                };
            }
        }
    }
    if value.is_boolean() {
        return RuntimeType::TypeBoolean;
    }
    RuntimeType::TypeNothing
}

/// `appendSourceToErrorMessage(CodeBlock*, BytecodeIndex, message, type, appender)` (`ErrorInstance.cpp`):
/// o trecho do fonte da expressão da instrução (`ExpressionInfo`), ou até 20 caracteres de contexto de cada
/// lado do `divot` (sem as brancas das pontas, na linha) quando a expressão não tem extensão.
pub fn append_source_to_error_message(
    code_block: &CodeBlockRef,
    bytecode_index: BytecodeIndex,
    message: &WtfString,
    type_: RuntimeType,
    appender: SourceAppender,
) -> WtfString {
    append_source_to_error_message_in_block(&code_block.borrow(), bytecode_index, message, type_, appender)
}

/// `appendSourceToErrorMessage` sobre o `CodeBlock` já emprestado (o handler de slow path o tem em mãos).
pub fn append_source_to_error_message_in_block(
    block: &CodeBlock,
    bytecode_index: BytecodeIndex,
    message: &WtfString,
    type_: RuntimeType,
    appender: SourceAppender,
) -> WtfString {
    let unlinked = block.unlinked_code_block().borrow();
    if !unlinked.has_expression_info() || message.is_null() {
        return message.clone();
    }
    drop(unlinked);
    // `CodeBlock::expressionRangeForBytecodeIndex`: o divot guardado é relativo ao início do fonte do bloco.
    let info = block.expression_info_for_bytecode_index(bytecode_index);
    let expression_start = info.divot as i64 - info.start_offset as i64;
    let expression_stop = info.divot as i64 + info.end_offset as i64;

    let source = block.source();
    let Some(provider) = source.provider() else {
        return message.clone();
    };
    let source_string = units(&provider.source());
    if expression_stop == 0 || expression_start > source_string.len() as i64 {
        return message.clone();
    }
    let original = units(message);

    if expression_start < expression_stop {
        let source_text = units(&provider.get_range(expression_start as i32, expression_stop as i32));
        return WtfString::from_utf16(&appender(&original, &source_text, type_, SourceTextWhereErrorOccurred::FoundExactSource));
    }

    // No range information, so give a few characters of context.
    let data_length = source_string.len() as i64;
    let expression_start = expression_start.max(0);
    let newline = u16::from(b'\n');
    let (mut start, mut stop) = (expression_start, expression_start);
    // Get up to 20 characters of context to the left and right of the divot, clamping to the line.
    // Then strip whitespace.
    while start > 0 && expression_start - start < 20 && source_string[(start - 1) as usize] != newline {
        start -= 1;
    }
    while start < expression_start - 1 && is_str_white_space(source_string[start as usize]) {
        start += 1;
    }
    while stop < data_length && stop - expression_start < 20 && source_string[stop as usize] != newline {
        stop += 1;
    }
    while stop > expression_start && is_str_white_space(source_string[(stop - 1) as usize]) {
        stop -= 1;
    }
    let source_text = units(&provider.get_range(start as i32, stop as i32));
    WtfString::from_utf16(&appender(&original, &source_text, type_, SourceTextWhereErrorOccurred::FoundApproximateSource))
}

/// `createError(JSGlobalObject*, JSValue, const String&, SourceAppender)`: um `TypeError` com
/// `"<descrição> <message>"`, que ganha o trecho do fonte do `site` (o que `ErrorInstance::finishCreation`
/// faz com o `topCallFrame`); sem `site` fica a mensagem pura. Descrição ou concatenação que falham dão
/// `createOutOfMemoryError`.
fn create_error_for_value(
    global_object: &JSGlobalObject,
    value: JSValue,
    message: &str,
    appender: SourceAppender,
    site: Option<&dyn SourceSite>,
) -> JSObjectHandle {
    let description = error_description_for_value(value);
    let full = match try_make_string_dyn(&[&description, &' ', &message]) {
        Some(full) if !description.is_empty() => full,
        _ => return create_out_of_memory_error(global_object),
    };
    let full = match site {
        _ if !error_captures_stack_trace(global_object) => full,
        Some(site) => site.append_source(&full, runtime_type_for_value(value), appender),
        None => append_native_call_site(global_object, &full, runtime_type_for_value(value), appender),
    };
    crate::runtime::error::create_type_error(global_object, &full)
}

/// A condição `m_stackTrace && !m_stackTrace->isEmpty()` de `ErrorInstance::finishCreation`: com
/// `Error.stackTraceLimit` 0, negativo, `NaN`, não numérico ou apagado nenhum frame é capturado (o mesmo
/// critério de `capture_frames_for_error`, em `error_natives.rs`), e a mensagem não ganha o trecho do fonte.
fn error_captures_stack_trace(global_object: &JSGlobalObject) -> bool {
    global_object.stack_trace_limit.get().is_some_and(|limit| limit != 0)
}

/// `ErrorInstance::finishCreation` sem `site` explícito: o erro nasce dentro de uma função nativa, e o frame
/// do topo é a instrução JS que a chamou (`Object.keys(null)`); sem chamada JS em curso a mensagem fica pura.
fn append_native_call_site(global_object: &JSGlobalObject, message: &WtfString, type_: RuntimeType, appender: SourceAppender) -> WtfString {
    match global_object.vm().native_call_site() {
        Some((code_block, bytecode_index)) => append_source_to_error_message(&code_block, bytecode_index, message, type_, appender),
        None => message.clone(),
    }
}

/// `createInvalidFunctionApplyParameterError(JSGlobalObject*, JSValue)`: o `TypeError` de argumentos que não
/// são array-like, com o `site` da instrução (`defaultSourceAppender`, `runtimeTypeForValue(value)`).
pub fn create_invalid_function_apply_parameter_error(
    global_object: &JSGlobalObject,
    value: JSValue,
    site: Option<&ErrorSite<'_>>,
) -> JSObjectHandle {
    let message = WtfString::from_latin1(b"second argument to Function.prototype.apply must be an Array-like object");
    let message = match site {
        Some(site) if error_captures_stack_trace(global_object) => site.append_source(&message, runtime_type_for_value(value), default_source_appender),
        _ => message,
    };
    crate::runtime::error::create_type_error(global_object, &message)
}

/// `createNotAConstructorError(JSGlobalObject*, JSValue)` com o `site` da instrução (`defaultSourceAppender`).
pub fn create_not_a_constructor_error_at(global_object: &JSGlobalObject, value: JSValue, site: Option<&dyn SourceSite>) -> JSObjectHandle {
    create_error_for_value(global_object, value, "is not a constructor", default_source_appender, site)
}

/// `createNotAConstructorError(JSGlobalObject*, JSValue)` sem instrução de origem.
pub fn create_not_a_constructor_error(global_object: &JSGlobalObject, value: JSValue) -> JSObjectHandle {
    create_not_a_constructor_error_at(global_object, value, None)
}

/// `createNotAFunctionError(JSGlobalObject*, JSValue)` (`notAFunctionSourceAppender`).
pub fn create_not_a_function_error(global_object: &JSGlobalObject, value: JSValue, site: Option<&dyn SourceSite>) -> JSObjectHandle {
    create_error_for_value(global_object, value, "is not a function", not_a_function_source_appender, site)
}

/// `createNotAnObjectError(JSGlobalObject*, JSValue)`.
pub fn create_not_an_object_error(global_object: &JSGlobalObject, value: JSValue) -> JSObjectHandle {
    create_not_an_object_error_at(global_object, value, None)
}

/// `createNotAnObjectError(JSGlobalObject*, JSValue)` com o `site` da instrução: o `ErrorInstance` acrescenta
/// o texto-fonte da expressão (`defaultSourceAppender`), como `undefined is not an object (evaluating 'a.b')`.
pub fn create_not_an_object_error_at(global_object: &JSGlobalObject, value: JSValue, site: Option<&dyn SourceSite>) -> JSObjectHandle {
    create_error_for_value(global_object, value, "is not an object", default_source_appender, site)
}

/// `sourceText.find(needle)`: o índice da primeira ocorrência.
fn find_units(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&index| haystack[index..index + needle.len()] == *needle)
}

/// `StringView::trim(deprecatedIsSpaceOrNewline)`.
fn trim_space_or_newline(text: &[u16]) -> &[u16] {
    let start = text.iter().position(|&unit| !deprecated_is_space_or_newline(unit)).unwrap_or(text.len());
    let end = text.iter().rposition(|&unit| !deprecated_is_space_or_newline(unit)).map_or(start, |index| index + 1);
    &text[start..end]
}

/// O miolo comum de `invalidParameterInSourceAppender` e `invalidParameterInstanceofSourceAppender`: a última
/// ocorrência de `keyword` no texto-fonte tem de ser a única, e o que vem depois dela (sem as brancas das
/// pontas) é o lado direito, que abre a mensagem. `None` quando a palavra não existe (o chamador devolve a
/// mensagem original); `Err` com o texto `(evaluating '...')` quando ela aparece mais de uma vez.
fn right_hand_side_after<'a>(
    original_message: &[u16],
    source_text: &'a [u16],
    keyword: &str,
) -> Option<Result<&'a [u16], Vec<u16>>> {
    let keyword = ascii_units(keyword);
    let index = reverse_find_units(source_text, &keyword)?;
    if find_units(source_text, &keyword) != Some(index) {
        return Some(Err(concat_units(&[original_message, &ascii_units(" (evaluating '"), source_text, &ascii_units("')")])));
    }
    Some(Ok(trim_space_or_newline(&source_text[index + keyword.len()..])))
}

/// `invalidParameterInSourceAppender`: `x is not an Object. (evaluating 'a in x')`.
pub fn invalid_parameter_in_source_appender(
    original_message: &[u16],
    source_text: &[u16],
    type_: RuntimeType,
    occurrence: SourceTextWhereErrorOccurred,
) -> Vec<u16> {
    debug_assert!(type_ != RuntimeType::TypeObject);
    if occurrence == SourceTextWhereErrorOccurred::FoundApproximateSource {
        return default_approximate_source_error(original_message, source_text);
    }
    match right_hand_side_after(original_message, source_text, "in") {
        // This should basically never happen, since JS code must use the literal text "in" for the `in`
        // operation. However, if we fail to find "in" for any reason, just fail gracefully.
        None => original_message.to_vec(),
        Some(Err(message)) => message,
        Some(Ok(right_hand_side)) => {
            concat_units(&[right_hand_side, &ascii_units(" is not an Object. (evaluating '"), source_text, &ascii_units("')")])
        }
    }
}

/// `invalidParameterInstanceofSourceAppender`: `content` fecha a frase depois do lado direito do `instanceof`.
fn invalid_parameter_instanceof_source_appender(
    content: &str,
    original_message: &[u16],
    source_text: &[u16],
    occurrence: SourceTextWhereErrorOccurred,
) -> Vec<u16> {
    if occurrence == SourceTextWhereErrorOccurred::FoundApproximateSource {
        return default_approximate_source_error(original_message, source_text);
    }
    match right_hand_side_after(original_message, source_text, "instanceof") {
        // This can happen when Symbol.hasInstance function is directly called.
        None => original_message.to_vec(),
        Some(Err(message)) => message,
        Some(Ok(right_hand_side)) => concat_units(&[
            right_hand_side,
            &ascii_units(content),
            &ascii_units(". (evaluating '"),
            source_text,
            &ascii_units("')"),
        ]),
    }
}

/// `invalidParameterInstanceofNotFunctionSourceAppender`.
fn invalid_parameter_instanceof_not_function_source_appender(
    original_message: &[u16],
    source_text: &[u16],
    _type: RuntimeType,
    occurrence: SourceTextWhereErrorOccurred,
) -> Vec<u16> {
    invalid_parameter_instanceof_source_appender(" is not a function", original_message, source_text, occurrence)
}

/// `invalidParameterInstanceofhasInstanceValueNotFunctionSourceAppender`.
fn invalid_parameter_instanceof_has_instance_value_not_function_source_appender(
    original_message: &[u16],
    source_text: &[u16],
    _type: RuntimeType,
    occurrence: SourceTextWhereErrorOccurred,
) -> Vec<u16> {
    invalid_parameter_instanceof_source_appender(
        "[Symbol.hasInstance] is not a function, undefined, or null",
        original_message,
        source_text,
        occurrence,
    )
}

/// `invalidPrototypeSourceAppender`.
fn invalid_prototype_source_appender(
    original_message: &[u16],
    source_text: &[u16],
    _type: RuntimeType,
    occurrence: SourceTextWhereErrorOccurred,
) -> Vec<u16> {
    if occurrence == SourceTextWhereErrorOccurred::FoundApproximateSource {
        return default_approximate_source_error(original_message, source_text);
    }
    let extends = ascii_units("extends");
    let found_once = reverse_find_units(source_text, &extends).is_some_and(|index| find_units(source_text, &extends) == Some(index));
    if !found_once {
        return concat_units(&[original_message, &ascii_units(" (evaluating '"), source_text, &ascii_units("')")]);
    }
    ascii_units("The value of the superclass's prototype property is not an object or null.")
}

/// `createInvalidInParameterError(JSGlobalObject*, JSValue)`: o lado direito de um `in` que não é objeto.
pub fn create_invalid_in_parameter_error(global_object: &JSGlobalObject, value: JSValue, site: Option<&dyn SourceSite>) -> JSObjectHandle {
    create_error_for_value(global_object, value, "is not an Object.", invalid_parameter_in_source_appender, site)
}

/// `createInvalidPrivateNameError(JSGlobalObject*)`: o `TypeError` de campo privado ausente, com o texto-fonte
/// da instrução (`defaultSourceAppender`), como `Cannot access invalid private field (evaluating 'o.#p')`.
pub fn create_invalid_private_name_error(global_object: &JSGlobalObject, site: Option<&dyn SourceSite>) -> JSObjectHandle {
    create_type_error_with_default_appender(global_object, crate::runtime::error_messages::INVALID_PRIVATE_NAME_ERROR, site)
}

/// `createTypeError(globalObject, message, defaultSourceAppender, TypeNothing)`: o `TypeError` de mensagem fixa
/// que ganha ` (evaluating '...')` com o texto-fonte do `site`. É o molde de `createInvalidPrivateNameError`,
/// `createRedefinedPrivateNameError`, `createPrivateMethodAccessError` e `createReinstallPrivateMethodError`
/// (`ExceptionHelpers.cpp`), e de todo `throwTypeError` cujo erro nasce com `defaultSourceAppender`.
pub fn create_type_error_with_default_appender(
    global_object: &JSGlobalObject,
    message: &str,
    site: Option<&dyn SourceSite>,
) -> JSObjectHandle {
    let message = WtfString::from_utf8(message.as_bytes());
    let full = match site {
        _ if !error_captures_stack_trace(global_object) => message,
        Some(site) => site.append_source(&message, RuntimeType::TypeNothing, default_source_appender),
        None => append_native_call_site(global_object, &message, RuntimeType::TypeNothing, default_source_appender),
    };
    crate::runtime::error::create_type_error(global_object, &full)
}
/// A mensagem de um erro criado dentro de uma função nativa com `defaultSourceAppender`: ganha o
/// ` (evaluating '...')` da instrução JS que chamou a função (sem chamada JS em curso fica pura).
pub fn append_default_source_to_native_message(global_object: &JSGlobalObject, message: &str) -> WtfString {
    let message = WtfString::from_utf8(message.as_bytes());
    append_native_call_site(global_object, &message, RuntimeType::TypeNothing, default_source_appender)
}

/// `createInvalidInstanceofParameterErrorNotFunction(JSGlobalObject*, JSValue)`.
pub fn create_invalid_instanceof_parameter_error_not_function(
    global_object: &JSGlobalObject,
    value: JSValue,
    site: Option<&dyn SourceSite>,
) -> JSObjectHandle {
    create_error_for_value(global_object, value, " is not a function", invalid_parameter_instanceof_not_function_source_appender, site)
}

/// `createInvalidInstanceofParameterErrorHasInstanceValueNotFunction(JSGlobalObject*, JSValue)`.
pub fn create_invalid_instanceof_parameter_error_has_instance_value_not_function(
    global_object: &JSGlobalObject,
    value: JSValue,
    site: Option<&dyn SourceSite>,
) -> JSObjectHandle {
    create_error_for_value(
        global_object,
        value,
        "[Symbol.hasInstance] is not a function, undefined, or null",
        invalid_parameter_instanceof_has_instance_value_not_function_source_appender,
        site,
    )
}

/// `createInvalidPrototypeError(JSGlobalObject*, JSValue)`.
pub fn create_invalid_prototype_error(global_object: &JSGlobalObject, value: JSValue, site: Option<&dyn SourceSite>) -> JSObjectHandle {
    create_error_for_value(global_object, value, "is not an object or null", invalid_prototype_source_appender, site)
}

/// `throwOutOfMemoryError(JSGlobalObject*, ThrowScope&)`.
pub fn throw_out_of_memory_error(global_object: &JSGlobalObject, scope: &mut ThrowScope<'_>) -> Rc<Exception> {
    throw_exception(global_object, scope, crate::runtime::error::create_out_of_memory_error(global_object))
}

/// `throwOutOfMemoryError(JSGlobalObject*, ThrowScope&, const String&)`.
pub fn throw_out_of_memory_error_with_message(global_object: &JSGlobalObject, scope: &mut ThrowScope<'_>, message: &WtfString) -> Rc<Exception> {
    throw_exception(global_object, scope, crate::runtime::error::create_out_of_memory_error_with_message(global_object, message))
}

/// `throwStackOverflowError(JSGlobalObject*, ThrowScope&)`. O `ErrorHandlingScope` só afeta o limite de
/// pilha da VM (veja `error_info.rs`) e não tem efeito no dado do erro.
pub fn throw_stack_overflow_error(global_object: &JSGlobalObject, scope: &mut ThrowScope<'_>) -> Rc<Exception> {
    throw_exception(global_object, scope, crate::runtime::error::create_stack_overflow_error(global_object))
}
