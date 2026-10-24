//! Porte de `VM::getHostFunction` (`VM.cpp:905-957`, o ramo sem JIT) e de `jitCodeForCallTrampoline`/
//! `jitCodeForConstructTrampoline`.
//!
//! DIVERGÊNCIAS: o C++ aponta o `NativeJITCode` para os rótulos de assembly `llint_native_call_trampoline`
//! e `llint_native_construct_trampoline` (`JITType::HostCallThunk`); aqui não há assembly, então o
//! `CodePtr` carrega o número do rótulo (como `LLIntEntry`, `llint/llint_jit_code.rs`). Os dois números
//! daqui ficam fora da faixa do `LLIntEntry` (1 a 13); quando o laço de despacho do LLInt portar o
//! `llint_native_call_trampoline`, ele reconhece estes valores (`HOST_CALL_TRAMPOLINE` e
//! `HOST_CONSTRUCT_TRAMPOLINE`) ou os rótulos migram para o `LLIntEntry`. O ramo `ENABLE(JIT)`
//! (`jitStubs->hostFunctionStub`) e o `WasmFunctionIntrinsic` não existem. O `DOMJIT::Signature` não existe.
//! O `JITCode` não é compartilhado por intrínseco como o `static LazyNeverDestroyed` do C++: é um valor
//! barato de duas palavras.

use std::rc::Rc;

use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::executable::JITCode;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::native_executable::{NativeExecutable, NativeExecutableRef};
use crate::runtime::native_function::{to_tagged, NativeFunction};
use crate::runtime::vm::VM;
use crate::wtf::code_ptr::CodePtr;
use crate::wtf::ptr_tag::JSEntryPtrTag;
use crate::wtf::text::wtf_string::String as WtfString;

/// O rótulo `llint_native_call_trampoline`.
pub const HOST_CALL_TRAMPOLINE: usize = 0x100;
/// O rótulo `llint_native_construct_trampoline`.
pub const HOST_CONSTRUCT_TRAMPOLINE: usize = 0x101;

/// `NativeJITCode(LLInt::getCodeRef<JSEntryPtrTag>(llint_native_*_trampoline), JITType::HostCallThunk, intrinsic)`.
#[derive(Clone, Copy, Debug)]
struct HostCallJITCode {
    entry: usize,
    intrinsic: Intrinsic,
}

impl JITCode for HostCallJITCode {
    fn address_for_call(&self, _arity_check: ArityCheckMode) -> CodePtr<JSEntryPtrTag> {
        CodePtr::new(self.entry)
    }

    fn intrinsic(&self) -> Intrinsic {
        self.intrinsic
    }
}

impl VM {
    /// `getHostFunction(function, implementationVisibility, constructor, length, name)`.
    pub fn get_host_function(
        &self,
        function: NativeFunction,
        implementation_visibility: ImplementationVisibility,
        constructor: NativeFunction,
        length: u32,
        name: &WtfString,
    ) -> NativeExecutableRef {
        self.get_host_function_with_intrinsic(function, implementation_visibility, Intrinsic::NoIntrinsic, constructor, length, name)
    }

    /// `getHostFunction(function, implementationVisibility, intrinsic, constructor, signature, length, name)`
    /// sem o `signature` do DOMJIT.
    pub fn get_host_function_with_intrinsic(
        &self,
        function: NativeFunction,
        implementation_visibility: ImplementationVisibility,
        intrinsic: Intrinsic,
        constructor: NativeFunction,
        length: u32,
        name: &WtfString,
    ) -> NativeExecutableRef {
        NativeExecutable::create(
            self,
            Rc::new(HostCallJITCode { entry: HOST_CALL_TRAMPOLINE, intrinsic }),
            to_tagged(function),
            Rc::new(HostCallJITCode { entry: HOST_CONSTRUCT_TRAMPOLINE, intrinsic: Intrinsic::NoIntrinsic }),
            to_tagged(constructor),
            implementation_visibility,
            length,
            name,
        )
    }
}
