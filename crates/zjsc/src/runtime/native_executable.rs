//! Tradução de `runtime/NativeExecutable.h` e `NativeExecutable.cpp`.
//!
//! DIVERGÊNCIAS (ver `executable.rs`):
//!
//! - `m_function`/`m_constructor` são `TaggedNativeFunction` (`native_function.rs`).
//! - `m_asString` é `WriteBarrier<JSString>`: `Option<JSStringRef>`, sem barreira de escrita.
//! - `signatureFor` devolve o `DOMJIT::Signature` do `JITCode`; `DOMJIT` é camada de JIT
//!   (CONVENTIONS) e não existe. `vm.forEachDebugger(didCreateNativeExecutable)` fica fora porque o
//!   `Debugger` do porte só tem o `DebuggerParseData` por enquanto; quando o `Debugger` entrar, o
//!   `create` o chama.
//! - Sem GC: `destroy`, `createStructure`, `subspaceFor`, `visitChildren`, `offsetOf...`,
//!   `DECLARE_INFO` e `asStringConcurrently` não existem; `StructureFlags` (`StructureIsImmortal`)
//!   é flag de coleta.
//! - `NativeExecutableRef` é `Rc<RefCell<NativeExecutable>>`. O `executable.rs` guarda um trait
//!   `NativeExecutable` provisório para enquanto este módulo não existia; os métodos
//!   (`hash_for`, `intrinsic`, `implementation_visibility`, `function`, `constructor`) têm aqui os
//!   mesmos nomes, então o trait provisório é substituído pela importação desta struct.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use crate::bytecode::code_block_hash::CodeBlockHash;
use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::executable::{ExecutableBase, JITCode};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::{js_empty_string, js_string, JSStringRef};
use crate::runtime::js_string_builder::js_make_nontrivial_string;
use crate::runtime::js_type::JSType;
use crate::runtime::native_function::TaggedNativeFunction;
use crate::runtime::throw_scope::ThrowScope;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `NativeExecutable*`.
pub type NativeExecutableRef = Rc<RefCell<NativeExecutable>>;

/// `class NativeExecutable`.
pub struct NativeExecutable {
    base: ExecutableBase,
    function: TaggedNativeFunction,
    constructor: TaggedNativeFunction,
    implementation_visibility: ImplementationVisibility,
    length: u32,
    name: WtfString,
    as_string: Option<JSStringRef>,
}

crate::parser::nodes::inherit!(NativeExecutable => ExecutableBase);

impl NativeExecutable {
    /// `create(VM&, Ref<JITCode>&& callThunk, TaggedNativeFunction, Ref<JITCode>&& constructThunk,
    /// TaggedNativeFunction constructor, ImplementationVisibility, unsigned length, const String& name)`
    /// mais o construtor privado e o `finishCreation`.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        _vm: &VM,
        call_thunk: Rc<dyn JITCode>,
        function: TaggedNativeFunction,
        construct_thunk: Rc<dyn JITCode>,
        constructor: TaggedNativeFunction,
        implementation_visibility: ImplementationVisibility,
        length: u32,
        name: &WtfString,
    ) -> NativeExecutableRef {
        let mut base = ExecutableBase::new(JSType::NativeExecutableType);
        base.jit_code_for_call_with_arity_check = Some(call_thunk.address_for_call(ArityCheckMode::MustCheckArity));
        base.jit_code_for_construct_with_arity_check =
            Some(construct_thunk.address_for_call(ArityCheckMode::MustCheckArity));
        base.jit_code_for_call = Some(call_thunk);
        base.jit_code_for_construct = Some(construct_thunk);
        Rc::new(RefCell::new(NativeExecutable {
            base,
            function,
            constructor,
            implementation_visibility,
            length,
            name: name.clone(),
            as_string: None,
        }))
    }

    /// `hashFor(CodeSpecializationKind)`: `CodeBlockHash(bit_cast<uintptr_t>(m_function))`, o
    /// construtor `CodeBlockHash(unsigned)` trunca o endereço para 32 bits.
    pub fn hash_for(&self, kind: CodeSpecializationKind) -> CodeBlockHash {
        if kind == CodeSpecializationKind::CodeForCall {
            return CodeBlockHash::from_hash(self.function.tagged_ptr() as u32);
        }

        assert!(kind == CodeSpecializationKind::CodeForConstruct);
        CodeBlockHash::from_hash(self.constructor.tagged_ptr() as u32)
    }

    /// `function()`.
    pub fn function(&self) -> TaggedNativeFunction {
        self.function
    }

    /// `constructor()`.
    pub fn constructor(&self) -> TaggedNativeFunction {
        self.constructor
    }

    /// `nativeFunctionFor(CodeSpecializationKind)`.
    pub fn native_function_for(&self, kind: CodeSpecializationKind) -> TaggedNativeFunction {
        if kind == CodeSpecializationKind::CodeForCall {
            return self.function();
        }
        debug_assert!(kind == CodeSpecializationKind::CodeForConstruct);
        self.constructor()
    }

    /// `name()`.
    pub fn name(&self) -> &WtfString {
        &self.name
    }

    /// `nameJSString(VM&)`.
    pub fn name_js_string(&self, vm: &VM) -> JSStringRef {
        if self.name.is_null() {
            js_empty_string(vm)
        } else {
            js_string(vm, &self.name)
        }
    }

    /// `length()`.
    pub fn length(&self) -> u32 {
        self.length
    }

    /// `implementationVisibility()`.
    pub fn implementation_visibility(&self) -> ImplementationVisibility {
        self.implementation_visibility
    }

    /// `intrinsic()`: o `JITCode` de chamada carrega o intrínseco.
    pub fn intrinsic(&self) -> Intrinsic {
        self.base.generated_jit_code_for(CodeSpecializationKind::CodeForCall).intrinsic()
    }

    /// `toString(JSGlobalObject*)`.
    pub fn to_string(&mut self, global_object: &JSGlobalObject) -> Option<JSStringRef> {
        if let Some(as_string) = &self.as_string {
            return Some(Rc::clone(as_string));
        }
        self.to_string_slow(global_object)
    }

    /// `toStringSlow(JSGlobalObject*)` (privado).
    fn to_string_slow(&mut self, global_object: &JSGlobalObject) -> Option<JSStringRef> {
        let vm = global_object.vm();
        let throw_scope = ThrowScope::new(vm);

        let value = js_make_nontrivial_string(global_object, &[&"function ", &self.name, &"() { [native code] }"]);

        if throw_scope.exception().is_some() {
            return None;
        }

        let as_string = value.expect("jsMakeNontrivialString sem exceção devolveu nulo");
        self.as_string = Some(Rc::clone(&as_string));
        Some(as_string)
    }
}
