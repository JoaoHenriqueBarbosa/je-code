//! Porte do que `JSFunction.cpp` e `JSFunctionInlines.h` definem sobre as propriedades preguiçosas da
//! função: `getOwnPropertySlot`, `deleteProperty`, `defineOwnProperty`, `reifyLazyPropertyIfNeeded` e os
//! `reifyLazy{Prototype,Length,Name,BoundName}IfNeeded`, `reifyLength`, `reifyName`, `originalLength`,
//! `originalName`, `canAssumeNameAndLengthAreOriginal`, `mayHaveNonReifiedPrototype`,
//! `hasReifiedLength`/`hasReifiedName`, `isHostOrBuiltinFunction` e `constructPrototypeObject`.
//!
//! `JSObject` do porte não despacha `getOwnPropertySlot`/`deleteProperty`/`defineOwnProperty` por
//! tipo (os métodos são livres de `OverridesGetOwnPropertySlot`): a sobrescrita é o método de mesmo
//! nome de `JSFunction`, e quem consulta uma função (`ObjectRef` em `host_function_support.rs`) chama
//! esse, não o de `JSObject`.
//!
//! `put` e `getOwnSpecialPropertyNames` também moram aqui. LACUNAS: o `init` do `JSGlobalObject` ainda não
//! preenche `generatorPrototype()` e `asyncGeneratorPrototype()` (o `expect` do acessor acusa), então o
//! `prototype` preguiçoso de função geradora só funciona depois disso.

use crate::runtime::delete_property_slot::DeletePropertySlot;
use crate::runtime::exception_helpers::throw_out_of_memory_error;
use crate::runtime::executable::ExecutableBaseRef;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObject, JSObjectRef, PutError};
use crate::runtime::js_string::{js_empty_string, js_string, JSStringRef};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::operations::js_string_concat;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::is_valid_offset;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::put_property_slot::PutPropertySlot;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::enumeration_mode::DontEnumPropertiesMode;
use crate::runtime::throw_scope::ThrowScope;
use crate::runtime::vm::VM;
use crate::parser::parser_modes::{is_async_generator_wrapper_parse_mode, is_generator_wrapper_parse_mode};
use crate::wtf::text::wtf_string::String as WtfString;

/// `static constexpr unsigned prototypeAttributesForNonClass`.
const PROTOTYPE_ATTRIBUTES_FOR_NON_CLASS: u32 = DONT_ENUM | DONT_DELETE;

/// `enum class JSFunction::PropertyStatus { Eager, Lazy, Reified }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyStatus {
    Eager,
    Lazy,
    Reified,
}

impl PropertyStatus {
    /// `isLazy(property)`.
    pub fn is_lazy(self) -> bool {
        self == PropertyStatus::Lazy || self == PropertyStatus::Reified
    }

    /// `isReified(property)`.
    pub fn is_reified(self) -> bool {
        self == PropertyStatus::Reified
    }
}

/// `constructPrototypeObject(globalObject, thisObject)`.
fn construct_prototype_object(global_object: &JSGlobalObject, this_object: &JSFunction) -> JSObjectRef {
    let vm = global_object.vm();
    let scope_global_object = this_object.realm();
    let parse_mode = this_object.js_executable().borrow().parse_mode();
    // Unlike Function instances, the prototype object of GeneratorFunction instances lacks own "constructor" property.
    // https://tc39.es/ecma262/#sec-runtime-semantics-instantiategeneratorfunctionobject (step 6)
    let is_generator = is_generator_wrapper_parse_mode(parse_mode);
    // Unlike Function instances, the prototype object of AsyncGeneratorFunction instances lacks own "constructor" property.
    // https://tc39.es/ecma262/#sec-runtime-semantics-instantiateasyncgeneratorfunctionobject (step 6)
    let is_async_generator = is_async_generator_wrapper_parse_mode(parse_mode);

    // `constructEmptyObject(globalObject, scopeGlobalObject->generatorPrototype() / asyncGeneratorPrototype() /
    // objectPrototype())`.
    let object_prototype = if is_generator {
        scope_global_object.generator_prototype()
    } else if is_async_generator {
        scope_global_object.async_generator_prototype()
    } else {
        scope_global_object.object_prototype()
    };
    let structure = global_object.structure_cache().empty_object_structure_for_prototype(
        global_object,
        &object_prototype,
        JSFinalObject::DEFAULT_INLINE_CAPACITY,
        false,
    );
    let prototype = JSFinalObject::create(vm, &structure);
    if is_generator || is_async_generator {
        return prototype;
    }
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), this_object.as_value(), DONT_ENUM);
    prototype
}

/// `makeNameWithOutOfMemoryCheck(globalObject, scope, messagePrefix, prefix, name)`: `None` com o
/// `OutOfMemoryError` pendente.
fn make_prefixed_name(global_object: &JSGlobalObject, prefix: &str, name: &WtfString) -> Option<WtfString> {
    let vm = global_object.vm();
    match js_string_concat(vm, &WtfString::from_latin1(prefix.as_bytes()), name) {
        Some(string) => Some(string.value()),
        None => {
            let mut scope = ThrowScope::new(vm);
            throw_out_of_memory_error(global_object, &mut scope);
            None
        }
    }
}

impl JSFunction {
    /// `hasReifiedLength()`.
    pub fn has_reified_length(&self) -> bool {
        self.rare_data().is_some_and(|rare_data| rare_data.has_reified_length())
    }

    /// `hasReifiedName()`.
    pub fn has_reified_name(&self) -> bool {
        self.rare_data().is_some_and(|rare_data| rare_data.has_reified_name())
    }

    /// `isBuiltinFunction()`.
    pub fn is_builtin_function(&self) -> bool {
        !self.is_host_function() && self.js_executable().borrow().is_builtin_function()
    }

    /// `isHostOrBuiltinFunction()`.
    pub fn is_host_or_builtin_function(&self) -> bool {
        self.is_host_function() || self.is_builtin_function()
    }

    /// `isNonBoundHostFunction()`.
    pub fn is_non_bound_host_function(&self) -> bool {
        self.is_host_function() && self.as_bound_function().is_none()
    }

    /// `mayHaveNonReifiedPrototype()`.
    pub fn may_have_non_reified_prototype(&self) -> bool {
        !self.is_host_or_builtin_function() && self.js_executable().borrow().has_prototype_property()
    }

    /// `canAssumeNameAndLengthAreOriginal(vm)`.
    pub fn can_assume_name_and_length_are_original(&self) -> bool {
        // Uma bound function sem `name`/`length` redefinidos também vale: `bind` adia o cálculo e `name_slow`
        // percorre a cadeia (`nesting_count`). Materializar `"bound " + nome` a cada `bind` encadeado (sem ropes
        // aqui) custaria memória quadrática: 1e5 `f = f.bind()` seriam dezenas de GB.
        // A bound function não registra a redefinição no rare data: enquanto a estrutura não transitou, nenhum
        // `name`/`length` próprio foi materializado nem redefinido.
        if self.as_bound_function().is_some() {
            return !self.structure().did_transition();
        }
        match self.rare_data() {
            None => true,
            Some(rare_data) => {
                !rare_data.has_modified_name_for_bound_or_non_host_function()
                    && !rare_data.has_modified_length_for_bound_or_non_host_function()
            }
        }
    }

    /// `originalLength(vm)`.
    pub fn original_length(&self, vm: &VM) -> f64 {
        if let Some(bound) = self.as_bound_function() {
            return bound.length(vm);
        }
        if let Some(remote) = self.as_remote_function() {
            return remote.length();
        }
        if self.is_host_function() {
            // The original length is captured in NativeExecutable at creation time.
            let ExecutableBaseRef::Native(native) = self.executable() else {
                unreachable!("isHostFunction com executável que não é NativeExecutable");
            };
            let length = native.borrow().length();
            return f64::from(length);
        }
        f64::from(self.js_executable().borrow().parameter_count())
    }

    /// `originalName(globalObject)`: `None` com a exceção pendente.
    pub fn original_name(&self, global_object: &JSGlobalObject) -> Option<JSStringRef> {
        let vm = global_object.vm();

        if let Some(bound) = self.as_bound_function() {
            return match bound.name_may_be_null() {
                Some(name) => {
                    let prefixed = make_prefixed_name(global_object, "bound ", &name.value())?;
                    Some(js_string(vm, &prefixed))
                }
                None => Some(js_empty_string(vm)),
            };
        }

        if let Some(remote) = self.as_remote_function() {
            return Some(remote.name_may_be_null().unwrap_or_else(|| js_empty_string(vm)));
        }

        if self.is_host_function() {
            // Build a fresh JSString from the original name stored on NativeExecutable.
            let ExecutableBaseRef::Native(native) = self.executable() else {
                unreachable!("isHostFunction com executável que não é NativeExecutable");
            };
            let name = native.borrow().name_js_string(vm);
            return Some(name);
        }

        let name = self.js_executable_name(vm);
        Some(js_string(vm, &self.decorate_name_for_accessor(global_object, name)?))
    }

    /// `JSFunction::name(vm)`: o nome original sem as propriedades reificadas (`bound ` na frente do da
    /// função ligada, o do `NativeExecutable` na de host, o do executável nas demais; `*default*` vira vazio).
    pub fn name(&self, vm: &VM, global_object: &JSGlobalObject) -> WtfString {
        if let Some(bound) = self.as_bound_function() {
            return bound.name(vm, global_object).value();
        }
        if self.is_host_function() {
            let ExecutableBaseRef::Native(native) = self.executable() else {
                unreachable!("isHostFunction com executável que não é NativeExecutable");
            };
            return native.borrow().name().clone();
        }
        let identifier = self.js_executable().borrow().name();
        if PropertyName::from_identifier(&identifier) == vm.property_names.star_default_private_name {
            return WtfString::default();
        }
        identifier.string().string().clone()
    }

    /// `JSFunction::displayName(vm)`: a propriedade própria `displayName` quando é uma string.
    pub fn display_name(&self, vm: &VM) -> WtfString {
        let display_name = self.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.display_name));
        if !display_name.is_empty() && display_name.is_string() {
            return display_name.as_js_string().try_get_value();
        }
        WtfString::default()
    }

    /// `JSFunction::calculatedDisplayName(vm)`.
    pub fn calculated_display_name(&self, vm: &VM, global_object: &JSGlobalObject) -> WtfString {
        let explicit_name = self.display_name(vm);
        if !explicit_name.is_empty() {
            return explicit_name;
        }

        let actual_name = self.name(vm, global_object);
        if !actual_name.is_empty() || self.is_host_or_builtin_function() {
            return actual_name;
        }

        self.js_executable().borrow().ecma_name().string().string().clone()
    }

    /// O nome do frame na pilha do `Bun` (`functionName(vm, globalObject, object)` de `ErrorStackTrace.cpp`):
    /// primeiro a propriedade própria `name` de dados (a reificada, que `defineProperty` e `setFunctionName`
    /// de chave computada ou símbolo mudam, ou a preguiçosa, com o `get `/`set ` dos acessores), se for
    /// string não vazia; depois `getCalculatedDisplayName` (`displayName`, nome do executável, `ecmaName`).
    pub fn stack_frame_name(&self, vm: &VM, global_object: &JSGlobalObject) -> WtfString {
        let reified = self.has_reified_name();
        let own_name = if reified {
            let value = self.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.name));
            (!value.is_empty() && value.is_string()).then(|| value.as_js_string().try_get_value())
        } else {
            self.original_name(global_object).map(|name| name.value())
        };
        let name = match own_name {
            Some(name) if !name.is_empty() => name,
            _ => self.calculated_display_name(vm, global_object),
        };
        // O bun imprime o acessor de chave literal sem o `get `/`set ` (`at g (...)`), mas o de chave computada,
        // cujo `name` foi reificado por `setFunctionName`, com ele (`at get [d]`, `at get dyn`), medido contra o bun 1.4.2.
        // O V8 sempre o mantém (`at get p`, medido no Chromium 154).
        let is_accessor = !reified && !crate::runtime::stack_frame::v8_stack_format() && {
            let executable = self.js_executable();
            let executable = executable.borrow();
            executable.is_getter() || executable.is_setter()
        };
        if is_accessor {
            for prefix in ["get ", "set "] {
                if name.starts_with(&WtfString::from_latin1(prefix.as_bytes())) {
                    return name.substring(4, u32::MAX);
                }
            }
        }
        name
    }

    /// `ecmaName`, com o `*default*` trocado por `default`.
    fn js_executable_name(&self, vm: &VM) -> WtfString {
        let ecma_name = self.js_executable().borrow().ecma_name();
        // https://tc39.github.io/ecma262/#sec-exports-runtime-semantics-evaluation
        // When the ident is "*default*", we need to set "default" for the ecma name.
        // This "*default*" name is never shown to users.
        if PropertyName::from_identifier(&ecma_name) == vm.property_names.star_default_private_name {
            vm.property_names.default_keyword.string().string().clone()
        } else {
            ecma_name.string().string().clone()
        }
    }

    /// O `"get "`/`"set "` que `reifyName` e `originalName` põem na frente do nome de acessor.
    fn decorate_name_for_accessor(&self, global_object: &JSGlobalObject, name: WtfString) -> Option<WtfString> {
        let (is_getter, is_setter) = {
            let executable = self.js_executable();
            let executable = executable.borrow();
            (executable.is_getter(), executable.is_setter())
        };
        if is_getter {
            make_prefixed_name(global_object, "get ", &name)
        } else if is_setter {
            make_prefixed_name(global_object, "set ", &name)
        } else {
            Some(name)
        }
    }

    /// `reifyLength(vm)`.
    fn reify_length(&self, vm: &VM) {
        let rare_data = self.ensure_rare_data(vm);

        debug_assert!(!self.has_reified_length());
        let length = self.original_length(vm);
        let initial_value = js_number(length);
        let initial_attributes = DONT_ENUM | READ_ONLY;
        rare_data.set_has_reified_length();
        self.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.length), initial_value, initial_attributes);
    }

    /// `reifyName(vm, globalObject)`.
    fn reify_name(&self, vm: &VM, global_object: &JSGlobalObject) -> PropertyStatus {
        let name = self.js_executable_name(vm);
        self.reify_name_with(vm, global_object, name)
    }

    /// `reifyName(vm, globalObject, name)`.
    pub(crate) fn reify_name_with(&self, vm: &VM, global_object: &JSGlobalObject, name: WtfString) -> PropertyStatus {
        let rare_data = self.ensure_rare_data(vm);

        debug_assert!(!self.has_reified_name());
        debug_assert!(!self.is_host_function());
        let initial_attributes = DONT_ENUM | READ_ONLY;

        let Some(name) = self.decorate_name_for_accessor(global_object, name) else {
            return PropertyStatus::Lazy;
        };

        rare_data.set_has_reified_name();
        self.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.name), JSValue::from_js_string(js_string(vm, &name)), initial_attributes);
        PropertyStatus::Reified
    }

    /// `reifyLazyPropertyIfNeeded<set>(vm, globalObject, propertyName)`: `set_has_modified` é o
    /// `SetHasModifiedLengthOrName::Yes`.
    pub fn reify_lazy_property_if_needed(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        set_has_modified: bool,
    ) -> PropertyStatus {
        let vm = global_object.vm();
        let status = if self.is_host_or_builtin_function() {
            self.reify_lazy_property_for_host_or_builtin_if_needed(global_object, property_name)
        } else {
            let lazy_prototype = self.reify_lazy_prototype_if_needed(global_object, property_name);
            if lazy_prototype.is_lazy() {
                lazy_prototype
            } else {
                let lazy_length = self.reify_lazy_length_if_needed(vm, property_name);
                if lazy_length.is_lazy() {
                    lazy_length
                } else {
                    let lazy_name = self.reify_lazy_name_if_needed(global_object, property_name);
                    if lazy_name.is_lazy() {
                        lazy_name
                    } else {
                        PropertyStatus::Eager
                    }
                }
            }
        };

        if set_has_modified {
            // Skip if length/name haven't been reified yet (no transition = no own length/name slot),
            // or if this is a JSBoundFunction (which tracks modifications differently).
            if !self.structure().did_transition() || self.as_bound_function().is_some() {
                return status;
            }
            let is_length_property = *property_name == vm.property_names.length;
            let is_name_property = *property_name == vm.property_names.name;
            if !is_length_property && !is_name_property {
                return status;
            }
            let rare_data = self.ensure_rare_data(vm);
            if is_length_property {
                rare_data.set_has_modified_length_for_bound_or_non_host_function();
            } else {
                rare_data.set_has_modified_name_for_bound_or_non_host_function();
            }
        }

        status
    }

    /// `reifyLazyPropertyForHostOrBuiltinIfNeeded`.
    fn reify_lazy_property_for_host_or_builtin_if_needed(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> PropertyStatus {
        debug_assert!(self.is_host_or_builtin_function());
        // length is lazy for everything in here (host, builtin, bound, remote).
        let lazy_length = self.reify_lazy_length_if_needed(global_object.vm(), property_name);
        if lazy_length.is_lazy() {
            return lazy_length;
        }
        self.reify_lazy_bound_name_if_needed(global_object, property_name)
    }

    /// `reifyLazyPrototypeIfNeeded`.
    fn reify_lazy_prototype_if_needed(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> PropertyStatus {
        let vm = global_object.vm();
        if *property_name == vm.property_names.prototype && self.may_have_non_reified_prototype() {
            if self.get_direct_by_name(vm, property_name).is_empty() {
                // For class constructors, prototype object is initialized from bytecode via defineOwnProperty().
                debug_assert!(!self.js_executable().borrow().is_class_constructor_function());
                let prototype = construct_prototype_object(global_object, self);
                self.put_direct(vm, property_name, prototype.as_value(), PROTOTYPE_ATTRIBUTES_FOR_NON_CLASS);
                return PropertyStatus::Reified;
            }
            return PropertyStatus::Lazy;
        }
        PropertyStatus::Eager
    }

    /// `reifyLazyLengthIfNeeded`.
    fn reify_lazy_length_if_needed(&self, vm: &VM, property_name: &PropertyName) -> PropertyStatus {
        if *property_name == vm.property_names.length {
            if !self.has_reified_length() {
                self.reify_length(vm);
                return PropertyStatus::Reified;
            }
            return PropertyStatus::Lazy;
        }
        PropertyStatus::Eager
    }

    /// `reifyLazyNameIfNeeded`.
    fn reify_lazy_name_if_needed(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> PropertyStatus {
        let vm = global_object.vm();
        if *property_name == vm.property_names.name {
            if !self.has_reified_name() {
                return self.reify_name(vm, global_object);
            }
            return PropertyStatus::Lazy;
        }
        PropertyStatus::Eager
    }

    /// `reifyLazyBoundNameIfNeeded`.
    pub(crate) fn reify_lazy_bound_name_if_needed(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> PropertyStatus {
        let vm = global_object.vm();
        let name_ident = &vm.property_names.name;
        if *property_name != *name_ident {
            return PropertyStatus::Eager;
        }

        if self.has_reified_name() {
            return PropertyStatus::Lazy;
        }

        let initial_attributes = DONT_ENUM | READ_ONLY;
        if self.is_builtin_function() {
            return self.reify_name(vm, global_object);
        } else if let Some(bound) = self.as_bound_function() {
            let rare_data = self.ensure_rare_data(vm);
            let name = bound.name(vm, global_object);
            let Some(prefixed) = make_prefixed_name(global_object, "bound ", &name.value()) else {
                return PropertyStatus::Lazy;
            };
            rare_data.set_has_reified_name();
            self.put_direct(vm, property_name, JSValue::from_js_string(js_string(vm, &prefixed)), initial_attributes);
        } else if let Some(remote) = self.as_remote_function() {
            let rare_data = self.ensure_rare_data(vm);
            let name = remote.name_may_be_null().unwrap_or_else(|| js_empty_string(vm));
            rare_data.set_has_reified_name();
            self.put_direct(vm, property_name, JSValue::from_js_string(name), initial_attributes);
        } else {
            debug_assert!(self.is_non_bound_host_function());
            let rare_data = self.ensure_rare_data(vm);
            let ExecutableBaseRef::Native(native) = self.executable() else {
                unreachable!("isNonBoundHostFunction com executável que não é NativeExecutable");
            };
            let name = native.borrow().name_js_string(vm);
            rare_data.set_has_reified_name();
            self.put_direct(vm, property_name, JSValue::from_js_string(name), initial_attributes);
        }
        PropertyStatus::Reified
    }

    /// `JSFunction::getOwnPropertySlot(object, globalObject, propertyName, slot)`: a exceção que a
    /// materialização lança fica pendente no `VM` (e o resultado é `false`).
    pub fn get_own_property_slot(&self, global_object: &JSGlobalObject, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        let vm = global_object.vm();

        if *property_name == vm.property_names.prototype && self.may_have_non_reified_prototype() {
            let (mut offset, mut attributes) = self.get_direct_offset_with_attributes(vm, property_name);
            if !is_valid_offset(offset) {
                // For class constructors, prototype object is initialized from bytecode via defineOwnProperty().
                debug_assert!(!self.js_executable().borrow().is_class_constructor_function());
                let prototype = construct_prototype_object(global_object, self);
                self.put_direct(vm, property_name, prototype.as_value(), PROTOTYPE_ATTRIBUTES_FOR_NON_CLASS);
                (offset, attributes) = self.get_direct_offset_with_attributes(vm, property_name);
                debug_assert!(is_valid_offset(offset));
            }
            slot.set_value_at_offset(self, attributes, self.get_direct(offset), offset);
            return true;
        }

        self.reify_lazy_property_if_needed(global_object, property_name, false);
        if vm.exception().is_some() {
            return false;
        }

        JSObject::get_own_property_slot(self, vm, property_name, slot)
    }

    /// `getPropertySlot(globalObject, propertyName, slot)` com a sobrescrita de `getOwnPropertySlot` no
    /// primeiro elo, e o resto da cadeia pelo `ObjectRef` (que alcança outras funções).
    pub fn get_property_slot(&self, global_object: &JSGlobalObject, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        if self.get_own_property_slot(global_object, property_name, slot) {
            return true;
        }
        if global_object.vm().exception().is_some() {
            return false;
        }
        let prototype = self.get_prototype_direct();
        match ObjectRef::from_value(&prototype) {
            Some(next) => next.get_property_slot(global_object, property_name, slot),
            None => false,
        }
    }

    /// `JSFunction::deleteProperty(cell, globalObject, propertyName, slot)`.
    pub fn delete_property(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        slot: &mut DeletePropertySlot,
    ) -> Result<bool, PutError> {
        let vm = global_object.vm();
        let property_type = self.reify_lazy_property_if_needed(global_object, property_name, true);
        if vm.exception().is_some() {
            return Ok(false);
        }
        if property_type.is_lazy() {
            slot.disable_caching();
        }
        JSObject::delete_property(self, vm, property_name, slot)
    }

    /// `JSFunction::defineOwnProperty(object, globalObject, propertyName, descriptor, throwException)`.
    pub fn define_own_property(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        throw_exception: bool,
    ) -> Result<bool, PutError> {
        let vm = global_object.vm();

        if *property_name == vm.property_names.prototype {
            if let Some(rare_data) = self.rare_data() {
                rare_data.clear(vm, "Store to prototype property of a function");
            }
        }

        if *property_name == vm.property_names.prototype && self.may_have_non_reified_prototype() {
            if !is_valid_offset(self.get_direct_offset(vm, property_name)) {
                if self.js_executable().borrow().is_class_constructor_function() {
                    // Fast path for prototype object initialization from bytecode that avoids calling into getOwnPropertySlot().
                    debug_assert!(descriptor.is_data_descriptor());
                    self.put_direct(vm, property_name, descriptor.value(), descriptor.attributes());
                    return Ok(true);
                }
                let prototype = construct_prototype_object(global_object, self);
                self.put_direct(vm, property_name, prototype.as_value(), PROTOTYPE_ATTRIBUTES_FOR_NON_CLASS);
            }
        } else {
            self.reify_lazy_property_if_needed(global_object, property_name, true);
            if vm.exception().is_some() {
                return Ok(false);
            }
        }

        JSObject::define_own_property(self, vm, property_name, descriptor, throw_exception)
    }

    /// `JSFunction::put(cell, globalObject, propertyName, value, slot)`.
    pub fn put(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Result<bool, PutError> {
        let vm = global_object.vm();

        if *property_name == vm.property_names.prototype {
            slot.disable_caching();
            if let Some(rare_data) = self.rare_data() {
                rare_data.clear(vm, "Store to prototype property of a function");
            }
            if self.may_have_non_reified_prototype() {
                if !is_valid_offset(self.get_direct_offset(vm, property_name)) {
                    // For class constructors, prototype object is initialized from bytecode via defineOwnProperty().
                    debug_assert!(!self.js_executable().borrow().is_class_constructor_function());
                    if slot.this_value() != self.as_value() {
                        return JSObject::define_property_on_receiver(self, vm, property_name, value, slot);
                    }
                    self.put_direct(vm, property_name, value, PROTOTYPE_ATTRIBUTES_FOR_NON_CLASS);
                    return Ok(true);
                }
                return JSObject::put(self, vm, property_name, value, slot);
            }
        }

        let property_type = self.reify_lazy_property_if_needed(global_object, property_name, false);
        if vm.exception().is_some() {
            return Ok(false);
        }
        if property_type.is_lazy() {
            slot.disable_caching();
        }
        JSObject::put(self, vm, property_name, value, slot)
    }

    /// `hasOwnProperty(globalObject, propertyName)` pelo `getOwnPropertySlot` de `JSFunction`.
    fn has_own_property_through_override(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> bool {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
        self.get_own_property_slot(global_object, property_name, &mut slot)
    }

    /// `getOwnPropertyDescriptor(globalObject, propertyName, descriptor)` pelo `getOwnPropertySlot` de `JSFunction`.
    fn own_property_descriptor_through_override(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> Option<PropertyDescriptor> {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
        if !self.get_own_property_slot(global_object, property_name, &mut slot) {
            return None;
        }
        let mut descriptor = PropertyDescriptor::default();
        descriptor.set_property_slot(&slot, property_name);
        Some(descriptor)
    }

    /// `JSFunction::getOwnSpecialPropertyNames(object, globalObject, propertyNames, mode)`: as exceções
    /// que a materialização lança são engolidas, como no C++ (`DECLARE_TOP_EXCEPTION_SCOPE`).
    pub fn get_own_special_property_names(
        &self,
        global_object: &JSGlobalObject,
        property_names: &mut PropertyNameArrayBuilder<'_>,
        mode: DontEnumPropertiesMode,
    ) {
        let vm = global_object.vm();
        let names = &vm.property_names;

        match mode {
            DontEnumPropertiesMode::Include => {
                let mut has_length = self.has_own_property_through_override(global_object, &PropertyName::from_identifier(&names.length));
                if vm.exception().is_some() {
                    has_length = false;
                    vm.clear_exception();
                }
                if !self.has_reified_length() || has_length {
                    property_names.add(&names.length);
                }
                let mut has_name = self.has_own_property_through_override(global_object, &PropertyName::from_identifier(&names.name));
                if vm.exception().is_some() {
                    has_name = false;
                    vm.clear_exception();
                }
                if !self.has_reified_name() || has_name {
                    property_names.add(&names.name);
                }
                if !self.is_host_or_builtin_function() && self.js_executable().borrow().has_prototype_property() {
                    property_names.add(&names.prototype);
                }
            }
            DontEnumPropertiesMode::Exclude => {
                for identifier in [&names.length, &names.name] {
                    let descriptor = self.own_property_descriptor_through_override(global_object, &PropertyName::from_identifier(identifier));
                    if vm.exception().is_some() {
                        vm.clear_exception();
                    } else if descriptor.is_some_and(|descriptor| descriptor.enumerable()) {
                        property_names.add(identifier);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::enumeration_mode::{PrivateSymbolMode, PropertyNameMode};
    use crate::runtime::identifier::Identifier;
    use crate::runtime::implementation_visibility::ImplementationVisibility;
    use crate::runtime::intrinsic::Intrinsic;
    use crate::runtime::js_function::{call_host_function_as_constructor, JSFunctionRef};
    use crate::runtime::js_global_object::JSGlobalObjectRef;
    use crate::runtime::js_value::js_null;

    fn global() -> JSGlobalObjectRef {
        let vm = std::rc::Rc::new(VM::new());
        let structure = JSGlobalObject::create_structure(&vm, js_null());
        JSGlobalObject::create(&vm, structure, js_null())
    }

    crate::host_function!(noop_host_function, noop_body);
    fn noop_body(_global_object: &JSGlobalObject, _call: &crate::runtime::host_call::HostCall) -> crate::runtime::host_call::HostResult {
        Ok(JSValue::undefined())
    }

    /// Uma função de host de nome `foo` e `length` 2.
    fn host_function(global: &JSGlobalObject) -> JSFunctionRef {
        JSFunction::create_native(
            global.vm(),
            global,
            2,
            &WtfString::from_latin1(b"foo"),
            noop_host_function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        )
    }

    fn own_slot(global: &JSGlobalObject, function: &JSFunction, name: &PropertyName) -> Option<(JSValue, u32)> {
        let mut slot = PropertySlot::new(function.as_value(), InternalMethodType::GetOwnProperty);
        function.get_own_property_slot(global, name, &mut slot).then(|| (slot.get_value_for(name), slot.attributes()))
    }

    #[test]
    fn host_function_reifies_name_and_length_on_demand() {
        let global = global();
        let vm = global.vm();
        let function = host_function(&global);
        let length = PropertyName::from_identifier(&vm.property_names.length);
        let name = PropertyName::from_identifier(&vm.property_names.name);

        assert!(!function.has_reified_length());
        assert!(!function.has_reified_name());

        let (value, attributes) = own_slot(&global, &function, &length).expect("length é própria");
        assert_eq!(value.as_number(), 2.0);
        assert_eq!(attributes, READ_ONLY | DONT_ENUM);
        assert!(function.has_reified_length());
        assert!(!function.has_reified_name());

        let (value, attributes) = own_slot(&global, &function, &name).expect("name é própria");
        assert_eq!(value.as_js_string().value(), WtfString::from_latin1(b"foo"));
        assert_eq!(attributes, READ_ONLY | DONT_ENUM);
        assert!(function.has_reified_name());
    }

    #[test]
    fn host_function_has_no_lazy_prototype() {
        let global = global();
        let function = host_function(&global);
        let prototype = PropertyName::from_identifier(&global.vm().property_names.prototype);
        assert!(!function.may_have_non_reified_prototype());
        assert!(own_slot(&global, &function, &prototype).is_none());
    }

    #[test]
    fn unrelated_property_is_not_reified_by_lookup() {
        let global = global();
        let function = host_function(&global);
        let other = PropertyName::from_identifier(&Identifier::from_string(global.vm(), &WtfString::from_latin1(b"zzz")));
        assert!(own_slot(&global, &function, &other).is_none());
        assert!(!function.has_reified_length());
        assert!(!function.has_reified_name());
    }

    #[test]
    fn delete_length_reifies_then_removes_it() {
        let global = global();
        let vm = global.vm();
        let function = host_function(&global);
        let length = PropertyName::from_identifier(&vm.property_names.length);

        let deleted = function.delete_property(&global, &length, &mut DeletePropertySlot::default()).expect("sem erro");
        assert!(deleted);
        assert!(vm.exception().is_none());
        assert!(function.has_reified_length());
        assert!(own_slot(&global, &function, &length).is_none());
        // `name` segue preguiçoso e intacto.
        assert!(!function.has_reified_name());
        assert!(own_slot(&global, &function, &PropertyName::from_identifier(&vm.property_names.name)).is_some());
    }

    #[test]
    fn define_own_property_of_name_keeps_the_new_value() {
        let global = global();
        let vm = global.vm();
        let function = host_function(&global);
        let name = PropertyName::from_identifier(&vm.property_names.name);

        let replacement = JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(b"bar")));
        let mut descriptor = PropertyDescriptor::default();
        descriptor.set_value(replacement);
        assert_eq!(function.define_own_property(&global, &name, &descriptor, false), Ok(true));

        let (value, _) = own_slot(&global, &function, &name).expect("name é própria");
        assert_eq!(value, replacement);
        assert!(function.has_reified_name());
    }

    #[test]
    fn get_property_slot_walks_to_function_prototype() {
        let global = global();
        let vm = global.vm();
        let function = host_function(&global);
        // `Function.prototype.length` é 0 e própria da função, então `get` devolve a da instância (2).
        let length = PropertyName::from_identifier(&vm.property_names.length);
        let value = ObjectRef::Function(function.clone()).get(&global, &length);
        assert_eq!(value.as_number(), 2.0);
        assert!(vm.exception().is_none());
    }

    #[test]
    fn own_special_property_names_list_length_and_name() {
        let global = global();
        let vm = global.vm();
        let function = host_function(&global);

        let mut builder = PropertyNameArrayBuilder::new(vm, PropertyNameMode::Strings, PrivateSymbolMode::Exclude);
        function.get_own_special_property_names(&global, &mut builder, DontEnumPropertiesMode::Include);
        assert_eq!(builder.len(), 2);
        assert!(builder.iter().any(|identifier| *identifier == vm.property_names.length));
        assert!(builder.iter().any(|identifier| *identifier == vm.property_names.name));

        // Ambas são não enumeráveis: `Exclude` não lista nenhuma.
        let mut builder = PropertyNameArrayBuilder::new(vm, PropertyNameMode::Strings, PrivateSymbolMode::Exclude);
        function.get_own_special_property_names(&global, &mut builder, DontEnumPropertiesMode::Exclude);
        assert!(builder.is_empty());
    }

    #[test]
    fn deleted_length_is_not_listed_but_stays_reified() {
        let global = global();
        let vm = global.vm();
        let function = host_function(&global);
        let length = PropertyName::from_identifier(&vm.property_names.length);
        assert_eq!(function.delete_property(&global, &length, &mut DeletePropertySlot::default()), Ok(true));

        let mut builder = PropertyNameArrayBuilder::new(vm, PropertyNameMode::Strings, PrivateSymbolMode::Exclude);
        function.get_own_special_property_names(&global, &mut builder, DontEnumPropertiesMode::Include);
        assert_eq!(builder.len(), 1);
        assert!(builder.iter().any(|identifier| *identifier == vm.property_names.name));
    }

    #[test]
    fn calculated_display_name_of_host_function() {
        let global = global();
        let vm = global.vm();
        let function = host_function(&global);
        assert_eq!(function.name(vm, &global), WtfString::from_latin1(b"foo"));
        assert!(function.display_name(vm).is_empty());
        assert_eq!(function.calculated_display_name(vm, &global), WtfString::from_latin1(b"foo"));

        // `displayName` próprio, quando é string, vence o nome.
        let display_name = PropertyName::from_identifier(&vm.property_names.display_name);
        let text = JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(b"shown")));
        function.put_direct(vm, &display_name, text, 0);
        assert_eq!(function.display_name(vm), WtfString::from_latin1(b"shown"));
        assert_eq!(function.calculated_display_name(vm, &global), WtfString::from_latin1(b"shown"));
    }
}
