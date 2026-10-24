//! Mensagens de erro compartilhadas de `runtime/JSObject.cpp` (`ReadonlyPropertyWriteError` etc.).

/// `JSObject.cpp:70`: `const ASCIILiteral ReadonlyPropertyWriteError`.
pub const READONLY_PROPERTY_WRITE_ERROR: &str = "Attempted to assign to readonly property.";

/// `JSGlobalObjectFunctions.h`: `inline constexpr ASCIILiteral RestrictedPropertyAccessError`.
pub const RESTRICTED_PROPERTY_ACCESS_ERROR: &str = "'arguments', 'callee', and 'caller' cannot be accessed in this context.";

/// `ExceptionHelpers.cpp`: `createInvalidPrivateNameError`.
pub const INVALID_PRIVATE_NAME_ERROR: &str = "Cannot access invalid private field";

/// `ExceptionHelpers.cpp`: `createRedefinedPrivateNameError`.
pub const REDEFINED_PRIVATE_NAME_ERROR: &str = "Cannot redefine existing private field";

/// `ExceptionHelpers.cpp`: `createPrivateMethodAccessError` (a grafia "acessor" é a do C++).
pub const PRIVATE_METHOD_ACCESS_ERROR: &str = "Cannot access private method or acessor";

/// `ExceptionHelpers.cpp`: `createReinstallPrivateMethodError`.
pub const REINSTALL_PRIVATE_METHOD_ERROR: &str = "Cannot install same private methods on object more than once";

/// `JSObjectInlines.h`: `JSObject::setPrivateBrand` num `WebAssemblyGCObjectType`.
pub const PRIVATE_METHOD_ON_WEB_ASSEMBLY_GC_OBJECT_ERROR: &str = "Cannot add private method to a WebAssembly GC object";

/// `JSObject.cpp:69`: `const ASCIILiteral NonExtensibleObjectPropertyDefineError`.
pub const NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR: &str =
    "Attempting to define property on object that is not extensible.";

/// `JSObject.cpp:71`: `const ASCIILiteral ReadonlyPropertyChangeError`.
pub const READONLY_PROPERTY_CHANGE_ERROR: &str = "Attempting to change value of a readonly property.";

/// `JSObject.cpp:72`: `const ASCIILiteral UnableToDeletePropertyError`.
pub const UNABLE_TO_DELETE_PROPERTY_ERROR: &str = "Unable to delete property.";

/// `JSObject.cpp:73`: `const ASCIILiteral UnconfigurablePropertyChangeAccessMechanismError`.
pub const UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR: &str =
    "Attempting to change access mechanism for an unconfigurable property.";

/// `JSObject.cpp:74`: `const ASCIILiteral UnconfigurablePropertyChangeConfigurabilityError`.
pub const UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR: &str =
    "Attempting to change configurable attribute of unconfigurable property.";

/// `JSObject.cpp:75`: `const ASCIILiteral UnconfigurablePropertyChangeEnumerabilityError`.
pub const UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR: &str =
    "Attempting to change enumerable attribute of unconfigurable property.";

/// `JSObject.cpp:76`: `const ASCIILiteral UnconfigurablePropertyChangeWritabilityError`.
pub const UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR: &str =
    "Attempting to change writable attribute of unconfigurable property.";

/// `JSObject.cpp`, `validateAndApplyPropertyDescriptor` (passo 8): a mensagem literal do setter.
pub const UNCONFIGURABLE_PROPERTY_CHANGE_SETTER_ERROR: &str =
    "Attempting to change the setter of an unconfigurable property.";

/// `JSObject.cpp`, `validateAndApplyPropertyDescriptor` (passo 8): a mensagem literal do getter.
pub const UNCONFIGURABLE_PROPERTY_CHANGE_GETTER_ERROR: &str =
    "Attempting to change the getter of an unconfigurable property.";
