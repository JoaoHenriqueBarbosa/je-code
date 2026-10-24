//! Cola entre a `NativeFunction` do porte e o corpo das funções nativas do tempo de execução
//! (`ObjectConstructor`, `SymbolConstructor`, `SymbolPrototype`...).
//!
//! No C++ o corpo é `JSC_DEFINE_HOST_FUNCTION(name, (JSGlobalObject*, CallFrame*))`, que lê
//! `callFrame->thisValue()`/`argument(n)` e, em erro, faz `throwVMTypeError(...)` e devolve o
//! `EncodedJSValue` nulo. Aqui o corpo é uma função comum que recebe os argumentos já lidos
//! (`HostCall`) e devolve `Result<JSValue, Thrown>`; `finish` lança o `Thrown` pendente e codifica o
//! resultado. A leitura da pilha mora só em `HostCall::read`, então qualquer mudança na assinatura da
//! `NativeFunction` toca este módulo e mais nada.
//!
//! DIVERGÊNCIA: `Thrown::Unported` é o caminho do C++ que depende de código ainda não portado (o mesmo
//! papel de `PutError::Unported`): vira `panic!` na hora de lançar, e a mensagem diz qual.

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::error::{create_range_error, create_type_error};
use crate::runtime::error_natives::capture_frames;
use crate::runtime::stack_frame::StackFrame;
use crate::runtime::exception_helpers::{throw_out_of_memory_error, throw_stack_overflow_error};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_array::ArrayError;
use crate::runtime::js_object::PutError;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::wtf_string::String as WtfString;

/// O que uma função nativa lança.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Thrown {
    /// `throwTypeError(globalObject, scope, message)`.
    TypeError(String),
    /// `throwRangeError(globalObject, scope, message)`.
    RangeError(String),
    /// `throwStackOverflowError`.
    StackOverflow,
    /// `throwOutOfMemoryError`.
    OutOfMemory,
    /// Caminho do C++ que depende de código ainda não portado.
    Unported(&'static str),
    /// `throwException(globalObject, scope, JSWebAssembly*Error::create(...))`: `WebAssembly.CompileError`,
    /// `LinkError` ou `RuntimeError` com a mensagem do C++.
    WebAssembly(crate::runtime::wasm_errors::WasmErrorKind, String),
    /// `RETURN_IF_EXCEPTION`: a exceção já está pendente no `VM` (lançada por `toObject`,
    /// `toPropertyKey`...), não há nada a lançar.
    Pending,
}

impl Thrown {
    /// `throwTypeError(globalObject, scope, message)`.
    pub fn type_error(message: &str) -> Thrown {
        Thrown::TypeError(message.to_string())
    }

    /// `throwRangeError(globalObject, scope, message)`.
    pub fn range_error(message: &str) -> Thrown {
        Thrown::RangeError(message.to_string())
    }
}

impl From<PutError> for Thrown {
    fn from(error: PutError) -> Thrown {
        match error {
            PutError::TypeError(message) => Thrown::type_error(message),
            PutError::StackOverflow => Thrown::StackOverflow,
            PutError::OutOfMemory => Thrown::OutOfMemory,
            PutError::RangeError(message) => Thrown::range_error(message),
            PutError::Pending => Thrown::Pending,
            PutError::Unported(what) => Thrown::Unported(what),
        }
    }
}

impl From<ArrayError> for Thrown {
    fn from(error: ArrayError) -> Thrown {
        match error {
            ArrayError::Put(error) => error.into(),
            ArrayError::RangeError(message) => Thrown::range_error(message),
        }
    }
}

/// `RETURN_IF_EXCEPTION(scope, ...)`: `Err(Pending)` se há exceção pendente no `VM` (as conversões de
/// `JSValue` devolvem um valor sentinela e a deixam lá), senão o valor.
pub fn pending_or<T>(global_object: &JSGlobalObject, value: T) -> Result<T, Thrown> {
    if global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(value)
}

/// O resultado de um corpo de função nativa.
pub type HostResult = Result<JSValue, Thrown>;

/// `callFrame->thisValue()`, `argument(n)`, `newTarget()` e `jsCallee()`, lidos da pilha.
#[derive(Clone, Debug)]
pub struct HostCall {
    this_value: JSValue,
    arguments: Vec<JSValue>,
    new_target: JSValue,
    callee: usize,
    /// O `CodeBlock` do frame chamador (`getCallerCodeBlock`); `None` quando quem chamou é nativo.
    caller_code_block: Option<crate::interpreter::register::CodeBlockId>,
    /// O próprio frame nativo, para `callerSourceOrigin`; `None` num `HostCall` montado à mão.
    native_frame: Option<crate::interpreter::call_frame::CallFrame>,
}

impl HostCall {
    /// Lê o quadro nativo (a pilha emprestada no `NativeCallFrame`).
    pub fn read(call_frame: &NativeCallFrame<'_>) -> HostCall {
        HostCall {
            this_value: call_frame.this_value(),
            arguments: call_frame.arguments_span(),
            new_target: call_frame.this_value(),
            callee: call_frame.js_callee(),
            caller_code_block: call_frame
                .call_frame()
                .caller_frame(call_frame.stack())
                .and_then(|caller| caller.code_block(call_frame.stack())),
            native_frame: Some(call_frame.call_frame()),
        }
    }

    /// Um `HostCall` montado à mão, para testes e para quem chama o corpo sem passar por um quadro.
    pub fn new(this_value: JSValue, arguments: Vec<JSValue>) -> HostCall {
        HostCall { this_value, arguments, new_target: JSValue::empty(), callee: 0, caller_code_block: None, native_frame: None }
    }

    /// O próprio frame nativo; `None` num `HostCall` montado à mão.
    pub fn native_frame(&self) -> Option<crate::interpreter::call_frame::CallFrame> {
        self.native_frame
    }

    /// O índice do primeiro registro do próprio frame nativo (`CallFrame::registers`); `None` num `HostCall`
    /// montado à mão. É a âncora dos quadros Wasm da função exportada (`wasm_call_stack`).
    pub fn native_frame_registers(&self) -> Option<usize> {
        self.native_frame.map(|frame| frame.registers())
    }

    /// `callFrame->callerSourceOrigin(vm)`: a origem do código que chamou a função nativa; a nula num
    /// `HostCall` montado à mão (sem frame).
    pub fn caller_source_origin(&self, global_object: &JSGlobalObject) -> crate::runtime::source_origin::SourceOrigin {
        match self.native_frame {
            Some(frame) => crate::interpreter::caller_source_origin::caller_source_origin(&global_object.vm().interpreter(), frame),
            None => crate::runtime::source_origin::SourceOrigin::default(),
        }
    }

    /// `getStackTrace(vm, obj, ...)` de `Error.cpp` a partir do quadro nativo: os frames JS de quem
    /// chamou, até `Error.stackTraceLimit`. `None` quando não há `stackTraceLimit` (o erro não guarda
    /// pilha) ou quando o `HostCall` foi montado à mão (sem frame).
    pub fn capture_stack_frames(&self, global_object: &JSGlobalObject, caller: Option<usize>) -> Option<Vec<StackFrame>> {
        capture_frames(global_object, self.native_frame?, caller, false)
    }

    /// Como `capture_stack_frames`, mas com o próprio frame nativo no topo: o erro que a função de host cria sem
    /// passar por um construtor de erro (o `AggregateError` de `Promise.any`) mostra `at any (unknown)` no bun.
    pub fn capture_stack_frames_with_native(&self, global_object: &JSGlobalObject) -> Option<Vec<StackFrame>> {
        capture_frames(global_object, self.native_frame?, None, true)
    }

    /// `thisValue()`.
    pub fn this_value(&self) -> JSValue {
        self.this_value
    }

    /// `argument(i)`: `undefined` além do último.
    pub fn argument(&self, index: usize) -> JSValue {
        self.arguments.get(index).copied().unwrap_or_else(JSValue::undefined)
    }

    /// `argumentCount()`.
    pub fn argument_count(&self) -> usize {
        self.arguments.len()
    }

    /// `ArgList(callFrame)`: todos os argumentos.
    pub fn arguments(&self) -> &[JSValue] {
        &self.arguments
    }

    /// `newTarget()`.
    pub fn new_target(&self) -> JSValue {
        self.new_target
    }

    /// `getCallerCodeBlock(callFrame)` (JSGlobalObjectFunctions.cpp): o `CodeBlock` de quem chamou, ou
    /// `None` se o chamador é um frame nativo. Sem DFG não há `InlineCallFrame` para resolver.
    pub fn caller_code_block(&self, global_object: &JSGlobalObject) -> Option<crate::bytecode::code_block::CodeBlockRef> {
        self.caller_code_block.and_then(|id| global_object.vm().interpreter().code_block(id))
    }

    /// `jsCallee()`: o `cell_id` do objeto chamado.
    pub fn callee(&self) -> usize {
        self.callee
    }
}

/// `throwException(globalObject, scope, error)` para cada espécie de `Thrown`.
pub fn throw_thrown(global_object: &JSGlobalObject, thrown: Thrown) {
    let mut scope = ThrowScope::new(global_object.vm());
    match thrown {
        Thrown::TypeError(message) => {
            let error = create_type_error(global_object, &WtfString::from_utf8(message.as_bytes()));
            throw_exception(global_object, &mut scope, error);
        }
        Thrown::RangeError(message) => {
            let error = create_range_error(global_object, &WtfString::from_utf8(message.as_bytes()));
            throw_exception(global_object, &mut scope, error);
        }
        Thrown::StackOverflow => {
            throw_stack_overflow_error(global_object, &mut scope);
        }
        Thrown::OutOfMemory => {
            throw_out_of_memory_error(global_object, &mut scope);
        }
        Thrown::WebAssembly(kind, message) => {
            let error = crate::runtime::wasm_errors::create_wasm_error(global_object, kind, &WtfString::from_utf8(message.as_bytes()));
            throw_exception(global_object, &mut scope, error.as_object());
        }
        Thrown::Unported(what) => panic!("caminho ainda não portado: {what}"),
        Thrown::Pending => {}
    }
}

/// `JSValue::encode(result)`, lançando o erro pendente e devolvendo o valor nulo quando houve erro.
pub fn finish(global_object: &JSGlobalObject, result: HostResult) -> EncodedJSValue {
    match result {
        Ok(value) => value.encode(),
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            JSValue::empty().encode()
        }
    }
}

/// Define o `JSC_DEFINE_HOST_FUNCTION(name, ...)` que lê o `HostCall` e chama `body(global_object,
/// &call)`: `host_function!(object_constructor_keys, object_keys)`.
#[macro_export]
macro_rules! host_function {
    (pub $name:ident, $body:path) => {
        $crate::host_function!(@define [pub] $name, $body);
    };
    ($name:ident, $body:path) => {
        $crate::host_function!(@define [] $name, $body);
    };
    (@define [$($visibility:tt)*] $name:ident, $body:path) => {
        $($visibility)* fn $name(
            global_object: &$crate::runtime::js_global_object::JSGlobalObject,
            call_frame: &mut $crate::interpreter::call_frame::NativeCallFrame<'_>,
        ) -> $crate::runtime::js_value::EncodedJSValue {
            let call = $crate::runtime::host_call::HostCall::read(call_frame);
            let _realm = $crate::runtime::current_realm::CurrentRealmScope::enter(global_object);
            let result = $body(global_object, &call);
            $crate::runtime::host_call::finish(global_object, result)
        }
    };
}

/// Define o `JSC_DEFINE_CUSTOM_GETTER(name, (globalObject, thisValue, propertyName))`, o `GetValueFunc` de
/// um `CustomGetterSetter`: `custom_getter!(symbol_proto_getter_description, symbol_proto_description)`.
/// O `body` recebe `(&JSGlobalObject, JSValue this, &PropertyName)` e devolve um [`HostResult`].
#[macro_export]
macro_rules! custom_getter {
    (pub $name:ident, $body:path) => {
        $crate::custom_getter!(@define [pub] $name, $body);
    };
    ($name:ident, $body:path) => {
        $crate::custom_getter!(@define [] $name, $body);
    };
    (@define [$($visibility:tt)*] $name:ident, $body:path) => {
        $($visibility)* fn $name(
            global_object: &$crate::runtime::js_global_object::JSGlobalObject,
            this_value: $crate::runtime::js_value::EncodedJSValue,
            property_name: &$crate::runtime::property_name::PropertyName,
        ) -> $crate::runtime::js_value::EncodedJSValue {
            let _realm = $crate::runtime::current_realm::CurrentRealmScope::enter(global_object);
            let result = $body(global_object, $crate::runtime::js_value::JSValue::decode(this_value), property_name);
            $crate::runtime::host_call::finish(global_object, result)
        }
    };
}

/// Define o `JSC_DEFINE_CUSTOM_SETTER(name, (globalObject, thisValue, value, propertyName))`, o
/// `PutValueFunc` de um `CustomGetterSetter`. O `body` recebe `(&JSGlobalObject, JSValue this, JSValue
/// value, &PropertyName)` e devolve `Result<bool, Thrown>` (o `bool` do C++; o `Err` lança e dá `false`).
#[macro_export]
macro_rules! custom_setter {
    (pub $name:ident, $body:path) => {
        $crate::custom_setter!(@define [pub] $name, $body);
    };
    ($name:ident, $body:path) => {
        $crate::custom_setter!(@define [] $name, $body);
    };
    (@define [$($visibility:tt)*] $name:ident, $body:path) => {
        $($visibility)* fn $name(
            global_object: &$crate::runtime::js_global_object::JSGlobalObject,
            this_value: $crate::runtime::js_value::EncodedJSValue,
            value: $crate::runtime::js_value::EncodedJSValue,
            property_name: &$crate::runtime::property_name::PropertyName,
        ) -> bool {
            let _realm = $crate::runtime::current_realm::CurrentRealmScope::enter(global_object);
            let result = $body(
                global_object,
                $crate::runtime::js_value::JSValue::decode(this_value),
                $crate::runtime::js_value::JSValue::decode(value),
                property_name,
            );
            match result {
                Ok(stored) => stored,
                Err(thrown) => {
                    $crate::runtime::host_call::throw_thrown(global_object, thrown);
                    false
                }
            }
        }
    };
}
