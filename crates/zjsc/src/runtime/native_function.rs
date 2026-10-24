//! Tradução de `runtime/NativeFunction.h`.
//!
//! DIVERGÊNCIAS:
//!
//! - `FunctionPtr<CFunctionPtrTag, EncodedJSValue(JSGlobalObject*, CallFrame*), JSCHostCall>` é o
//!   ponteiro de função do Rust, como o item 5 do `CONVENTIONS.md` fixa (`JSC_DEFINE_HOST_FUNCTION`).
//!   O `JSGlobalObject*` é `&JSGlobalObject`: o objeto é compartilhado (o realm vem de um `Rc`) e o que
//!   o C++ muta por ele é mutabilidade interior no porte.
//! - `TaggedNativeFunction` (a mesma função com a tag `HostFunctionPtrTag`) é um newtype sobre o
//!   `NativeFunction`: a tag é só validação de PAC e CFI, sem efeito aqui, então `toTagged`/`retagged`
//!   é a troca do tipo e `tagged_ptr()` é o endereço da função.
//! - `add(Hasher&, NativeFunction)` é o `Hash` do newtype pelo `tagged_ptr()`.
//! - O `CallFrame*` do C++ é um ponteiro para dentro da `CLoopStack`; o `CallFrame` do porte é só o
//!   índice do frame. A função nativa recebe então o [`NativeCallFrame`]: o mesmo frame com a pilha de
//!   registradores emprestada (`thisValue`, `argument`, `argumentCount` e `jsCallee` leem por ele).

use std::hash::{Hash, Hasher};

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::EncodedJSValue;

/// `NativeFunction`: `EncodedJSValue(JSGlobalObject*, CallFrame*)`.
pub type NativeFunction = fn(&JSGlobalObject, &mut NativeCallFrame<'_>) -> EncodedJSValue;

/// `TaggedNativeFunction`.
#[derive(Clone, Copy, Debug)]
pub struct TaggedNativeFunction {
    function: NativeFunction,
}

impl TaggedNativeFunction {
    /// `FunctionPtr::function()`.
    pub fn function(&self) -> NativeFunction {
        self.function
    }

    /// `FunctionPtr::taggedPtr()`.
    pub fn tagged_ptr(&self) -> usize {
        self.function as usize
    }

    /// `FunctionPtr::call(...)`: chama a função nativa.
    pub fn call(&self, global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
        (self.function)(global_object, call_frame)
    }
}

impl PartialEq for TaggedNativeFunction {
    fn eq(&self, other: &TaggedNativeFunction) -> bool {
        self.tagged_ptr() == other.tagged_ptr()
    }
}

impl Eq for TaggedNativeFunction {}

impl Hash for TaggedNativeFunction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tagged_ptr().hash(state);
    }
}

/// `toTagged(NativeFunction)`.
pub fn to_tagged(function: NativeFunction) -> TaggedNativeFunction {
    TaggedNativeFunction { function }
}
