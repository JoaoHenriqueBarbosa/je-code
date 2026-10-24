//! Tradução de `runtime/ExceptionScope.h` e `runtime/ThrowScope.h`, na configuração sem
//! `ENABLE(EXCEPTION_SCOPE_VERIFICATION)` (o caso de release: o escopo só guarda o `VM&`).
//!
//! DIVERGÊNCIAS:
//!
//! - `ThrowScope::throwException(JSGlobalObject*, JSValue)` constrói o `Exception` com
//!   `Exception::create` (sem a pilha capturada, que depende do `Interpreter`). As sobrecargas de
//!   `Exception*`, `JSValue` e `JSObject*` são o trait `IntoException`; `VM::throw_exception` guarda
//!   a exceção pendente (o aviso ao depurador e os traps ficam para o `Interpreter` e o `VMTraps`).
//! - `RETURN_IF_EXCEPTION` e `TRY_CLEAR_EXCEPTION` são macros do C++; o chamador escreve o
//!   `if scope.exception().is_some() { return ... }` e `try_clear_exception`.
//! - `EncodedJSValue` nulo de `throwVMException` é o `JSValue` vazio.

use std::rc::Rc;

use crate::runtime::error_instance::ErrorInstanceRef;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObjectHandle, JSObjectRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::vm::{Exception, VM};

/// `class ExceptionScope`.
pub struct ExceptionScope<'vm> {
    vm: &'vm VM,
}

impl<'vm> ExceptionScope<'vm> {
    fn new(vm: &'vm VM) -> ExceptionScope<'vm> {
        ExceptionScope { vm }
    }

    /// `vm()`.
    pub fn vm(&self) -> &'vm VM {
        self.vm
    }

    /// `exception()`.
    pub fn exception(&self) -> Option<Rc<Exception>> {
        self.vm.exception()
    }

    /// `assertNoException()`.
    pub fn assert_no_exception(&self) {
        debug_assert!(self.exception().is_none());
    }

    /// `releaseAssertNoException()`.
    pub fn release_assert_no_exception(&self) {
        assert!(self.exception().is_none());
    }

    /// `assertNoExceptionExceptTermination()`.
    pub fn assert_no_exception_except_termination(&self) {
        debug_assert!(self.exception().is_none() || self.vm.has_pending_termination_exception());
    }

    /// `releaseAssertNoExceptionExceptTermination()`.
    pub fn release_assert_no_exception_except_termination(&self) {
        assert!(self.exception().is_none() || self.vm.has_pending_termination_exception());
    }

    /// `[[nodiscard]] tryClearException()`: falso quando a pendente é a de terminação.
    #[must_use]
    pub fn try_clear_exception(&self) -> bool {
        if let Some(exception) = self.exception() {
            if self.vm.is_termination_exception(&exception) {
                return false;
            }
        }
        self.vm.clear_exception();
        true
    }
}

/// `class ThrowScope : public ExceptionScope`.
pub struct ThrowScope<'vm> {
    base: ExceptionScope<'vm>,
}

impl<'vm> std::ops::Deref for ThrowScope<'vm> {
    type Target = ExceptionScope<'vm>;

    fn deref(&self) -> &ExceptionScope<'vm> {
        &self.base
    }
}

impl<'vm> ThrowScope<'vm> {
    /// `ThrowScope(VM&)` (`DECLARE_THROW_SCOPE`).
    pub fn new(vm: &'vm VM) -> ThrowScope<'vm> {
        ThrowScope { base: ExceptionScope::new(vm) }
    }

    /// `throwException(JSGlobalObject*, Exception*)`, `throwException(JSGlobalObject*, JSValue)` e
    /// `throwException(JSGlobalObject*, JSObject*)`: o `Rc<Exception>`, o `JSValue` e o `JSObjectRef`
    /// implementam `IntoException`.
    pub fn throw_exception(&mut self, global_object: &JSGlobalObject, thrown: impl IntoException) -> Rc<Exception> {
        let exception = thrown.into_exception(self.base.vm);
        self.base.vm.throw_exception(global_object, exception)
    }

    /// `release()`.
    pub fn release(&mut self) {}
}

/// O que `ThrowScope::throwException` e `VM::throwException` aceitam como argumento.
pub trait IntoException {
    /// `VM::throwException(JSGlobalObject*, JSValue)`: o `Exception::create` quando não é uma `Exception`.
    fn into_exception(self, vm: &VM) -> Rc<Exception>;
}

impl IntoException for Rc<Exception> {
    fn into_exception(self, _vm: &VM) -> Rc<Exception> {
        self
    }
}

/// `dynamicDowncast<Exception>(thrownValue)` do C++: o handler `finally` guarda a célula `Exception`
/// (`store_caught_value`) e o `op_throw` do fim do `finally` a relança como `JSValue`. Sem reaproveitar a
/// `Exception`, cada nível criava uma nova cujo valor era a célula `Exception` da anterior, e o `catch` de
/// cima recebia esse embrulho em vez do `RangeError`.
impl IntoException for JSValue {
    fn into_exception(self, vm: &VM) -> Rc<Exception> {
        if self.is_cell() {
            if let Some(exception) = Exception::from_cell_id(self.as_cell()) {
                return exception;
            }
        }
        Exception::create(vm, self)
    }
}

impl IntoException for JSObjectRef {
    fn into_exception(self, vm: &VM) -> Rc<Exception> {
        Exception::create(vm, self.as_value())
    }
}

impl IntoException for JSObjectHandle {
    fn into_exception(self, vm: &VM) -> Rc<Exception> {
        Exception::create(vm, self.as_value())
    }
}

impl IntoException for ErrorInstanceRef {
    fn into_exception(self, vm: &VM) -> Rc<Exception> {
        Exception::create(vm, self.as_value())
    }
}

/// `throwException(JSGlobalObject*, ThrowScope&, Exception*)` e as sobrecargas de `JSValue`/`JSObject*`.
pub fn throw_exception(global_object: &JSGlobalObject, scope: &mut ThrowScope<'_>, thrown: impl IntoException) -> Rc<Exception> {
    scope.throw_exception(global_object, thrown)
}

/// `throwVMException(JSGlobalObject*, ThrowScope&, Exception*)`: devolve o `EncodedJSValue` nulo.
pub fn throw_vm_exception(global_object: &JSGlobalObject, scope: &mut ThrowScope<'_>, thrown: impl IntoException) -> JSValue {
    throw_exception(global_object, scope, thrown);
    JSValue::empty()
}

/// `throwVMError(JSGlobalObject*, ThrowScope&, JSValue)` (Error.h). O C++ devolve a célula do `Exception`
/// codificada; como a exceção pendente é o que o chamador lê, aqui vale o `JSValue` vazio, o mesmo de
/// `throw_vm_exception` (reexportação, não função de repasse).
pub use throw_vm_exception as throw_vm_error;
