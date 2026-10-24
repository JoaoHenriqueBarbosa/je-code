//! Porte de `runtime/JSArrayBufferPrototype.h`, `JSArrayBufferPrototypeInlines.h` e
//! `JSArrayBufferPrototype.cpp`: o `ArrayBuffer.prototype` e o `SharedArrayBuffer.prototype` (um
//! `JSNonFinalObject` com o `ClassInfo` `"ArrayBuffer"`, o modo de compartilhamento escolhe as funções):
//! `slice`, `resize`, `transfer`, `transferToFixedLength` e os acessores `byteLength`, `resizable`,
//! `maxByteLength`, `detached` (`ArrayBuffer`), e `slice`, `grow`, `byteLength`, `growable`,
//! `maxByteLength` (`SharedArrayBuffer`).
//!
//! DIVERGÊNCIAS:
//!
//! - Sem `speciesWatchpointIsValid` (os watchpoints não existem): o `slice` sempre decide a espécie pelo
//!   `arrayBufferSpeciesConstructorSlow`, que dá o mesmo resultado do atalho (o `constructor` e o
//!   `@@species` não alterados devolvem o construtor do realm, isto é, `std::nullopt`).
//! - O `constructorObject->realm()` do `arrayBufferSpeciesConstructorSlow` é o `realm()` da `Structure` do
//!   construtor candidato.
//! - O ramo `associatedWasmMemoryWrapper` do `resize` é do WebAssembly, que não existe.
//! - `byteLength`, `resizable`, `maxByteLength`, `detached` e `growable` são acessores (um `JSFunction`
//!   `"get X"` dentro de um `GetterSetter`), o que o `JSC_NATIVE_GETTER_WITHOUT_TRANSITION` cria.

use crate::host_function;
use crate::runtime::array_buffer::{ArrayBuffer, ArrayBufferContents, ArrayBufferSharingMode};
use crate::runtime::call_data::{construct_with_error_message, get_construct_data};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::{get_value_property, thrown_from_llint};
use crate::runtime::js_array_buffer::{JSArrayBuffer, JSArrayBufferRef};
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{ACCESSOR, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo JSArrayBufferPrototype::s_info`.
pub static JS_ARRAY_BUFFER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "ArrayBuffer", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSC_NATIVE_INTRINSIC_GETTER_WITHOUT_TRANSITION(name, getter, attributes, intrinsic)`
/// (`putDirectNativeIntrinsicGetterWithoutTransition`): o `JSFunction` `"get <name>"` num `GetterSetter`
/// sob `name`, com `Accessor` somado aos `attributes`.
pub fn put_native_getter(
    vm: &VM,
    global_object: &JSGlobalObject,
    object: &JSObject,
    name: &str,
    getter: NativeFunction,
    intrinsic: Intrinsic,
    attributes: u32,
) {
    let function = crate::runtime::js_custom_accessor_function::create_host_getter_function(vm, global_object, name, getter, intrinsic);
    let accessor = GetterSetter::create_from_values(vm, function.as_value(), js_undefined());
    object.put_direct_non_index_accessor_without_transition(
        vm,
        &PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes())),
        &accessor,
        attributes | ACCESSOR,
    );
}

/// O `this` como `JSArrayBuffer` do modo (`dynamicDowncast<JSArrayBuffer>` mais a conferência do
/// `sharingMode`): `"Receiver must be ArrayBuffer"` ou `"... SharedArrayBuffer"`.
fn this_array_buffer(this_value: JSValue, mode: ArrayBufferSharingMode) -> Result<JSArrayBufferRef, Thrown> {
    JSArrayBuffer::from_value(&this_value)
        .filter(|this_object| this_object.impl_().sharing_mode() == mode)
        .ok_or_else(|| Thrown::type_error(&format!("Receiver must be {}", mode.name())))
}

/// `arrayBufferSpeciesConstructorSlow(globalObject, thisObject, mode)`: `None` é `std::nullopt` (o
/// construtor padrão).
fn array_buffer_species_constructor(
    global_object: &JSGlobalObject,
    this_object: &JSArrayBuffer,
    mode: ArrayBufferSharingMode,
) -> Result<Option<JSValue>, Thrown> {
    let names = &global_object.vm().property_names;
    let constructor = get_value_property(global_object, this_object.as_value(), &PropertyName::from_identifier(&names.constructor))?;
    if !get_construct_data(constructor).is_none() {
        let realm = constructor.as_object().structure().realm();
        let is_any_array_buffer_constructor = realm
            .is_some_and(|realm| constructor == realm.array_buffer_realm.array_buffer_constructor(mode).as_value());
        if is_any_array_buffer_constructor {
            return Ok(None);
        }
    }

    if constructor.is_undefined() {
        return Ok(None);
    }

    if !constructor.is_object() {
        return Err(Thrown::type_error("constructor property should not be null"));
    }

    let species = get_value_property(global_object, constructor, &PropertyName::from_identifier(&names.species_symbol))?;
    Ok(if species.is_undefined_or_null() { None } else { Some(species) })
}

/// `speciesConstructArrayBuffer(globalObject, thisObject, length, mode)`: `None` é o `FastPath`, `Some` o
/// `CreatedObject`.
fn species_construct_array_buffer(
    global_object: &JSGlobalObject,
    this_object: &JSArrayBufferRef,
    length: usize,
    mode: ArrayBufferSharingMode,
) -> Result<Option<JSArrayBufferRef>, Thrown> {
    // This is optimized way of SpeciesConstruct invoked from {ArrayBuffer,SharedArrayBuffer}.prototype.slice.
    // https://tc39.es/ecma262/#sec-arraybuffer.prototype.slice
    // https://tc39.es/ecma262/#sec-sharedarraybuffer.prototype.slice
    let Some(species) = array_buffer_species_constructor(global_object, this_object, mode)? else {
        return Ok(None);
    };

    // 16. Let new be ? Construct(ctor, « 𝔽(newLen) »).
    let new_object = construct_with_error_message(
        global_object,
        species,
        &[js_number(length as f64)],
        "Species construction did not get a valid constructor",
    )
    .map_err(thrown_from_llint)?;

    // 17. Perform ? RequireInternalSlot(new, [[ArrayBufferData]]).
    let Some(result) = JSArrayBuffer::from_value(&new_object) else {
        return Err(Thrown::type_error("Species construction does not create ArrayBuffer"));
    };

    if mode == ArrayBufferSharingMode::Default {
        // 18. If IsSharedArrayBuffer(new) is true, throw a TypeError exception.
        if result.impl_().is_shared() {
            return Err(Thrown::type_error("ArrayBuffer.prototype.slice creates SharedArrayBuffer"));
        }
        // 19. If IsDetachedBuffer(new) is true, throw a TypeError exception.
        if result.impl_().is_detached() {
            return Err(Thrown::type_error("Created ArrayBuffer is detached"));
        }
    } else if !result.impl_().is_shared() {
        // 17. If IsSharedArrayBuffer(new) is false, throw a TypeError exception.
        return Err(Thrown::type_error("SharedArrayBuffer.prototype.slice creates non-shared ArrayBuffer"));
    }

    // 20. If SameValue(new, O) is true, throw a TypeError exception.
    if std::rc::Rc::ptr_eq(&result, this_object) {
        return Err(Thrown::type_error("Species construction returns same ArrayBuffer to a receiver"));
    }

    // 21. If new.[[ArrayBufferByteLength]] < newLen, throw a TypeError exception.
    if result.impl_().byte_length() < length {
        return Err(Thrown::type_error("Species construction returns ArrayBuffer which byteLength is less than requested"));
    }

    Ok(Some(result))
}

/// `CopyDataBlockBytes(toBuf, 0, fromBuf, first, count)`: copia por um vetor intermediário, para o caso de
/// os dois buffers dividirem o mesmo armazenamento.
fn copy_data_block_bytes(source: &ArrayBuffer, first: usize, destination: &ArrayBuffer, count: usize) {
    let bytes = source.with_bytes(|bytes| bytes[first..first + count].to_vec());
    destination.with_bytes_mut(|destination| destination[..count].copy_from_slice(&bytes));
}

/// O índice relativo de `slice` (`relativeStart`/`relativeEnd`) já limitado a `[0, byteLength]`.
fn clamp_relative_index(relative: f64, byte_length: usize) -> usize {
    if relative < 0.0 {
        return (byte_length as f64 + relative).max(0.0) as usize;
    }
    relative.min(byte_length as f64) as usize
}

fn array_buffer_slice(
    global_object: &JSGlobalObject,
    array_buffer_value: JSValue,
    start_value: JSValue,
    end_value: JSValue,
    mode: ArrayBufferSharingMode,
) -> HostResult {
    // https://tc39.es/ecma262/#sec-arraybuffer.prototype.slice
    // https://tc39.es/ecma262/#sec-sharedarraybuffer.prototype.slice
    let vm = global_object.vm();

    // 2. Perform ? RequireInternalSlot(O, [[ArrayBufferData]]).
    // 3. If IsSharedArrayBuffer(O) is true, throw a TypeError exception.
    let this_object = this_array_buffer(array_buffer_value, mode)?;
    let buffer = this_object.impl_();

    // 4. If IsDetachedBuffer(O) is true, throw a TypeError exception.
    if mode == ArrayBufferSharingMode::Default && buffer.is_detached() {
        return Err(Thrown::type_error("Receiver is detached"));
    }

    // 5. Let len be O.[[ArrayBufferByteLength]].
    // https://tc39.es/proposal-resizablearraybuffer/#sec-sharedarraybuffer.prototype.slice
    let byte_length = buffer.byte_length();

    let first_index = clamp_relative_index(start_value.to_integer_or_infinity_checked()?, byte_length);
    debug_assert!(first_index <= byte_length);

    let final_index = if !end_value.is_undefined() {
        clamp_relative_index(end_value.to_integer_or_infinity_checked()?, byte_length)
    } else {
        byte_length
    };
    debug_assert!(final_index <= byte_length);

    // 14. Let newLen be max(final - first, 0).
    let new_length = final_index.saturating_sub(first_index);

    // 15. Let ctor be ? SpeciesConstructor(O, %ArrayBuffer%).
    let species_result = species_construct_array_buffer(global_object, &this_object, new_length, mode)?;

    // 23. If IsDetachedBuffer(O) is true, throw a TypeError exception.
    if mode == ArrayBufferSharingMode::Default && buffer.is_detached() {
        return Err(Thrown::type_error("Receiver is detached"));
    }

    let Some(new_object) = species_result else {
        debug_assert!(!buffer.is_detached());
        let mut new_buffer = None;
        if mode == ArrayBufferSharingMode::Default && this_object.impl_().is_resizable_or_growable_shared() {
            let created = ArrayBuffer::try_create(new_length, 1, None).ok_or(Thrown::OutOfMemory)?;
            created.set_sharing_mode(buffer.sharing_mode());
            if first_index < buffer.byte_length() {
                copy_data_block_bytes(buffer, first_index, &created, new_length.min(buffer.byte_length() - first_index));
            }
            new_buffer = Some(created);
        }

        let new_buffer = match new_buffer {
            Some(new_buffer) => new_buffer,
            None => buffer.slice_with_clamped_index(first_index, final_index).ok_or(Thrown::OutOfMemory)?,
        };

        let structure = global_object.array_buffer_realm.array_buffer_structure(new_buffer.sharing_mode());
        return Ok(JSArrayBuffer::create(vm, &structure, new_buffer).as_value());
    };

    // 24. Let fromBuf be O.[[ArrayBufferData]].
    // 25. Let toBuf be new.[[ArrayBufferData]].
    // 26. Perform CopyDataBlockBytes(toBuf, 0, fromBuf, first, newLen).
    debug_assert!(!buffer.is_detached());
    debug_assert!(!new_object.impl_().is_detached());
    debug_assert!(new_object.impl_().byte_length() >= new_length);
    if mode == ArrayBufferSharingMode::Default {
        if first_index < buffer.byte_length() {
            copy_data_block_bytes(buffer, first_index, new_object.impl_(), new_length.min(buffer.byte_length() - first_index));
        }
    } else {
        copy_data_block_bytes(buffer, first_index, new_object.impl_(), new_length);
    }
    Ok(new_object.as_value())
}

fn array_buffer_byte_length(array_buffer_value: JSValue, mode: ArrayBufferSharingMode) -> HostResult {
    let this_object = this_array_buffer(array_buffer_value, mode)?;

    if mode == ArrayBufferSharingMode::Default && this_object.impl_().is_detached() {
        return Ok(js_number(0.0));
    }

    Ok(js_number(this_object.impl_().byte_length() as f64))
}

/// `if (!std::isfinite(newLength) || newLength < 0) throwVMRangeError(...)` e o `static_cast<size_t>`: o
/// comprimento novo de `resize`/`grow`, já convertido por `toIntegerOrInfinity`.
fn validated_new_byte_length(new_length: f64) -> Result<usize, Thrown> {
    if !new_length.is_finite() || new_length < 0.0 {
        return Err(Thrown::range_error("new length is out of range"));
    }
    Ok(new_length as usize)
}

// https://tc39.es/proposal-resizablearraybuffer/#sec-arraybuffer.prototype.resize
fn array_buffer_proto_func_resize_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = this_array_buffer(call.this_value(), ArrayBufferSharingMode::Default)?;

    if !this_object.impl_().is_resizable_or_growable_shared() {
        return Err(Thrown::type_error("ArrayBuffer is not resizable"));
    }

    let new_length = call.argument(0).to_integer_or_infinity_checked()?;

    if this_object.impl_().is_detached() {
        return Err(Thrown::type_error("Receiver is detached"));
    }

    let new_byte_length = validated_new_byte_length(new_length)?;

    // O buffer redimensionável de `WebAssembly.Memory.toResizableBuffer` cresce a própria memória (bun 1.4.2).
    if this_object.impl_().is_wasm_memory() {
        let current = this_object.impl_().byte_length();
        if new_byte_length < current {
            return Err(Thrown::range_error("Cannot shrink WebAssembly memory"));
        }
        if this_object.impl_().max_byte_length().is_some_and(|max| new_byte_length > max) {
            return Err(Thrown::range_error(&format!("ArrayBuffer resize failed with new byte length {new_byte_length}")));
        }
        if new_byte_length % 65536 != 0 {
            return Err(Thrown::range_error(&format!(
                "WebAssembly memory cannot be resized to new byte length {new_byte_length} because it is not a multiple of 65536"
            )));
        }
        if this_object.impl_().resize_wasm_memory(new_byte_length).is_err() {
            return Err(Thrown::range_error(&format!("ArrayBuffer resize failed with new byte length {new_byte_length}")));
        }
        return Ok(js_undefined());
    }

    if this_object.impl_().resize(new_byte_length).is_err() {
        return Err(Thrown::range_error(&format!("ArrayBuffer resize failed with new byte length {new_byte_length}")));
    }

    Ok(js_undefined())
}

// https://tc39.es/proposal-arraybuffer-transfer/#sec-arraybuffercopyanddetach
#[derive(Clone, Copy, PartialEq, Eq)]
enum CopyAndDetachMode {
    PreserveResizability,
    FixedLength,
}

/// `ArrayBuffer::transferTo` seguido do `ArrayBuffer::create(contents)`: o conteúdo de `buffer` num buffer
/// novo, com `"ArrayBuffer transfer failed"` se não havia o que passar.
fn transfer_to_new_buffer(buffer: &ArrayBuffer) -> Result<std::rc::Rc<ArrayBuffer>, Thrown> {
    let mut contents = ArrayBufferContents::default();
    if !buffer.transfer_to(&mut contents) {
        return Err(Thrown::range_error("ArrayBuffer transfer failed"));
    }
    Ok(ArrayBuffer::new(contents))
}

fn array_buffer_copy_and_detach(
    global_object: &JSGlobalObject,
    array_buffer: &JSArrayBuffer,
    new_byte_length: usize,
    mode: CopyAndDetachMode,
) -> Result<JSArrayBufferRef, Thrown> {
    let vm = global_object.vm();
    let buffer = array_buffer.impl_();

    debug_assert_eq!(buffer.sharing_mode(), ArrayBufferSharingMode::Default);
    let is_resizable = buffer.is_resizable_or_growable_shared();

    if buffer.is_detached() {
        return Err(Thrown::type_error("Receiver is detached"));
    }

    let structure = global_object.array_buffer_realm.array_buffer_structure(ArrayBufferSharingMode::Default);

    if !is_resizable && new_byte_length == buffer.byte_length() {
        // We should just transfer!
        return Ok(JSArrayBuffer::create(vm, &structure, transfer_to_new_buffer(buffer)?));
    }

    if mode == CopyAndDetachMode::PreserveResizability && is_resizable {
        if buffer.max_byte_length().is_none_or(|max_byte_length| new_byte_length > max_byte_length) {
            return Err(Thrown::range_error(&format!("ArrayBuffer transfer failed with new byte length {new_byte_length}")));
        }

        let new_buffer = transfer_to_new_buffer(buffer)?;
        if new_buffer.resize(new_byte_length).is_err() {
            return Err(Thrown::range_error(&format!("ArrayBuffer resize failed with new byte length {new_byte_length}")));
        }
        return Ok(JSArrayBuffer::create(vm, &structure, new_buffer));
    }

    // We should create a new ArrayBuffer and copy them since underlying ArrayBuffer characteristics are different.
    let new_buffer = ArrayBuffer::try_create(new_byte_length, 1, None).ok_or(Thrown::OutOfMemory)?;
    copy_data_block_bytes(buffer, 0, &new_buffer, new_byte_length.min(buffer.byte_length()));

    transfer_to_new_buffer(buffer)?;

    Ok(JSArrayBuffer::create(vm, &structure, new_buffer))
}

fn array_buffer_proto_func_transfer_impl(
    global_object: &JSGlobalObject,
    array_buffer_value: JSValue,
    new_length_value: JSValue,
    mode: CopyAndDetachMode,
) -> HostResult {
    let this_object = this_array_buffer(array_buffer_value, ArrayBufferSharingMode::Default)?;

    // WebAssembly.Memory's buffer cannot be detached.
    if this_object.impl_().is_wasm_memory() {
        return Err(Thrown::type_error("Receiver cannot be detached because it is WebAssembly.Memory"));
    }

    let new_byte_length = if new_length_value.is_undefined() {
        if this_object.impl_().is_detached() { 0 } else { this_object.impl_().byte_length() }
    } else {
        new_length_value.to_index("newLength")? as usize
    };

    Ok(array_buffer_copy_and_detach(global_object, &this_object, new_byte_length, mode)?.as_value())
}

fn array_buffer_proto_func_slice_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    array_buffer_slice(global_object, call.this_value(), call.argument(0), call.argument(1), ArrayBufferSharingMode::Default)
}

// https://tc39.es/proposal-arraybuffer-transfer/#sec-arraybuffer.prototype.transfer
fn array_buffer_proto_func_transfer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    array_buffer_proto_func_transfer_impl(global_object, call.this_value(), call.argument(0), CopyAndDetachMode::PreserveResizability)
}

// https://tc39.es/proposal-arraybuffer-transfer/#sec-arraybuffer.prototype.transfertofixedlength
fn array_buffer_proto_func_transfer_to_fixed_length_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    array_buffer_proto_func_transfer_impl(global_object, call.this_value(), call.argument(0), CopyAndDetachMode::FixedLength)
}

// http://tc39.github.io/ecmascript_sharedmem/shmem.html#sec-get-arraybuffer.prototype.bytelength
fn array_buffer_proto_getter_func_byte_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    array_buffer_byte_length(call.this_value(), ArrayBufferSharingMode::Default)
}

// https://tc39.es/proposal-resizablearraybuffer/#sec-get-arraybuffer.prototype.resizable
fn array_buffer_proto_getter_func_resizable_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = this_array_buffer(call.this_value(), ArrayBufferSharingMode::Default)?;
    Ok(js_boolean(this_object.impl_().is_resizable_non_shared()))
}

/// O `maxByteLength` do acessor: o máximo, ou o próprio comprimento de um buffer de comprimento fixo.
fn max_byte_length_value(this_object: &JSArrayBuffer) -> JSValue {
    let buffer = this_object.impl_();
    js_number(buffer.max_byte_length().unwrap_or_else(|| buffer.byte_length()) as f64)
}

// https://tc39.es/proposal-resizablearraybuffer/#sec-get-arraybuffer.prototype.maxbytelength
fn array_buffer_proto_getter_func_max_byte_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = this_array_buffer(call.this_value(), ArrayBufferSharingMode::Default)?;
    debug_assert!(this_object.impl_().max_byte_length().is_none() || this_object.impl_().is_resizable_non_shared());
    Ok(max_byte_length_value(&this_object))
}

// https://tc39.es/proposal-arraybuffer-transfer/#sec-get-arraybuffer.prototype.detached
fn array_buffer_proto_getter_func_detached_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = this_array_buffer(call.this_value(), ArrayBufferSharingMode::Default)?;
    Ok(js_boolean(this_object.impl_().is_detached()))
}

fn shared_array_buffer_proto_func_slice_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    array_buffer_slice(global_object, call.this_value(), call.argument(0), call.argument(1), ArrayBufferSharingMode::Shared)
}

fn shared_array_buffer_proto_func_grow_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/proposal-resizablearraybuffer/#sec-sharedarraybuffer.prototype.grow
    let this_object = this_array_buffer(call.this_value(), ArrayBufferSharingMode::Shared)?;

    if !this_object.impl_().is_resizable_or_growable_shared() {
        return Err(Thrown::type_error("SharedArrayBuffer is not growable"));
    }

    let new_byte_length = validated_new_byte_length(call.argument(0).to_integer_or_infinity_checked()?)?;
    if this_object.impl_().grow(new_byte_length).is_err() {
        return Err(Thrown::range_error(&format!("grow failed with new byte length {new_byte_length}")));
    }

    Ok(js_undefined())
}

// http://tc39.github.io/ecmascript_sharedmem/shmem.html#StructuredData.SharedArrayBuffer.prototype.get_byteLength
fn shared_array_buffer_proto_getter_func_byte_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    array_buffer_byte_length(call.this_value(), ArrayBufferSharingMode::Shared)
}

// https://tc39.es/proposal-resizablearraybuffer/#sec-get-sharedarraybuffer.prototype.growable
fn shared_array_buffer_proto_getter_func_growable_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = this_array_buffer(call.this_value(), ArrayBufferSharingMode::Shared)?;
    Ok(js_boolean(this_object.impl_().is_growable_shared()))
}

// https://tc39.es/proposal-resizablearraybuffer/#sec-get-sharedarraybuffer.prototype.maxbytelength
fn shared_array_buffer_proto_getter_func_max_byte_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = this_array_buffer(call.this_value(), ArrayBufferSharingMode::Shared)?;
    debug_assert!(this_object.impl_().max_byte_length().is_none() || this_object.impl_().is_growable_shared());
    Ok(max_byte_length_value(&this_object))
}

host_function!(array_buffer_proto_func_slice, array_buffer_proto_func_slice_body);
host_function!(array_buffer_proto_func_resize, array_buffer_proto_func_resize_body);
host_function!(array_buffer_proto_func_transfer, array_buffer_proto_func_transfer_body);
host_function!(array_buffer_proto_func_transfer_to_fixed_length, array_buffer_proto_func_transfer_to_fixed_length_body);
host_function!(array_buffer_proto_getter_func_byte_length, array_buffer_proto_getter_func_byte_length_body);
host_function!(array_buffer_proto_getter_func_resizable, array_buffer_proto_getter_func_resizable_body);
host_function!(array_buffer_proto_getter_func_max_byte_length, array_buffer_proto_getter_func_max_byte_length_body);
host_function!(array_buffer_proto_getter_func_detached, array_buffer_proto_getter_func_detached_body);
host_function!(shared_array_buffer_proto_func_slice, shared_array_buffer_proto_func_slice_body);
host_function!(shared_array_buffer_proto_func_grow, shared_array_buffer_proto_func_grow_body);
host_function!(shared_array_buffer_proto_getter_func_byte_length, shared_array_buffer_proto_getter_func_byte_length_body);
host_function!(shared_array_buffer_proto_getter_func_growable, shared_array_buffer_proto_getter_func_growable_body);
host_function!(shared_array_buffer_proto_getter_func_max_byte_length, shared_array_buffer_proto_getter_func_max_byte_length_body);

/// `class JSArrayBufferPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct ArrayBufferPrototype;

impl ArrayBufferPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, ArrayBufferPrototype::STRUCTURE_FLAGS),
            &JS_ARRAY_BUFFER_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure, sharingMode)`: `JSArrayBufferPrototype(vm, structure)` e o
    /// `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef, sharing_mode: ArrayBufferSharingMode) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        ArrayBufferPrototype::finish_creation(&prototype, vm, global_object, sharing_mode);
        prototype
    }

    /// `finishCreation(vm, globalObject, sharingMode)`.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject, sharing_mode: ArrayBufferSharingMode) {
        prototype.finish_creation(vm);

        put_to_string_tag(vm, prototype, sharing_mode.name());

        let names = &vm.property_names;
        let getter_attributes = DONT_ENUM | READ_ONLY;
        let function = |name: &Identifier, length: u32, function: NativeFunction| {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                prototype,
                name,
                length,
                function,
                ImplementationVisibility::Public,
                Intrinsic::NoIntrinsic,
                DONT_ENUM,
            );
        };
        let getter = |name: &str, getter: NativeFunction| {
            put_native_getter(vm, global_object, prototype, name, getter, Intrinsic::NoIntrinsic, getter_attributes);
        };

        if sharing_mode == ArrayBufferSharingMode::Default {
            function(&names.slice, 2, array_buffer_proto_func_slice);
            getter("byteLength", array_buffer_proto_getter_func_byte_length);
            function(&names.resize, 1, array_buffer_proto_func_resize);
            function(&names.transfer, 0, array_buffer_proto_func_transfer);
            function(&names.transfer_to_fixed_length, 0, array_buffer_proto_func_transfer_to_fixed_length);
            getter("resizable", array_buffer_proto_getter_func_resizable);
            getter("maxByteLength", array_buffer_proto_getter_func_max_byte_length);
            getter("detached", array_buffer_proto_getter_func_detached);
        } else {
            function(&names.slice, 2, shared_array_buffer_proto_func_slice);
            getter("byteLength", shared_array_buffer_proto_getter_func_byte_length);
            function(&names.grow, 1, shared_array_buffer_proto_func_grow);
            getter("growable", shared_array_buffer_proto_getter_func_growable);
            getter("maxByteLength", shared_array_buffer_proto_getter_func_max_byte_length);
        }
    }
}
