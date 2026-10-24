//! `structuredClone(value, options)` do global. O JavaScriptCore não o define como função de global: quem o instala
//! é o bun (WebCore, `SerializedScriptValue`), como propriedade de dados comum (`writable`, `enumerable`,
//! `configurable`), `length` 2, `name` "structuredClone". Na ordem de chaves do bun vem logo depois de `setTimeout`.
//!
//! Portado até aqui: primitivos (inclusive `-0`, `NaN`, BigInt, string), `Object` comum e `Array`, com a identidade
//! preservada nos ciclos e nas referências repetidas. Do objeto saem só as propriedades próprias enumeráveis de
//! chave string, lidas com `[[Get]]` (getters rodam), e o protótipo do resultado é o de `Object`/`Array`
//! (instância de classe vira objeto simples). Função e `Symbol` lançam `DataCloneError`. Buraco de array
//! continua buraco; propriedade não índice de array é copiada.
//!
//! Também: Date, invólucros Number/String/Boolean/BigInt, `ArrayBuffer` (inclusive redimensionável),
//! `SharedArrayBuffer` (compartilha a memória), typed arrays e `DataView` (o buffer clonado uma vez só, identidade
//! preservada) e a opção `transfer` (lista validada como no WebCore; os buffers são destacados depois do clone com
//! sucesso). `RegExp` (mesmo padrão e flags, `lastIndex` 0, propriedades extras perdidas), `Map` e `Set` (chaves e
//! valores clonados, ciclos preservados, propriedades próprias copiadas) e `Error`: só `name` (um dos sete tipos
//! padrão, senão `Error`), `message` (propriedade própria de dados), e, com `stack` string, `line`, `column`,
//! `sourceURL` e `stack`; `cause`, `errors` e extras se perdem.
//!
//! `Blob` e `File` clonam com bytes, `type`, nome e `lastModified`. Headers, URL, URLSearchParams, FormData,
//! AbortController, Request, Response, Event e EventTarget lançam `DataCloneError`, como no bun.
//!
//! Nada mais pendente. Ver `wip-notes/global-names-gap.md`.
//!
//! Fiel ao `CloneSerializer::serialize` do bun (`SerializedScriptValue.cpp`): `Object.prototype` clona como objeto
//! comum e `Array.prototype` como array; o array lê o `length` uma vez e visita `0..length` (elemento não enumerável
//! também é copiado, getter roda, buraco continua buraco); os nomes de objeto são o retrato de enumeráveis tirado antes
//! dos getters. `arguments`, `Proxy`, `Math`, `JSON`, `globalThis`, `Promise`, iteradores e demais exóticos lançam.
//!
//! DIVERGÊNCIAS: o `DataCloneError` é um `DOMException` real (`js_dom_exception.rs`). A profundidade da recursão não
//! tem o limite de pilha do bun.

use crate::host_function;
use crate::runtime::js_dom_exception::throw_dom_exception_from_host;
use crate::runtime::blob::{make_blob, state_of};
use crate::runtime::message_channel::{is_port, push_transfer_port};
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PropertyNameMode};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::{construct_array, JSArray};
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::object_constructor::{construct_empty_object, own_descriptor, own_names};
use crate::runtime::property_name::PropertyName;
use crate::runtime::proxy_object::{object_get, object_set};
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
use crate::runtime::array_buffer::{ArrayBuffer, ArrayBufferContents, ArrayBufferRef};
use crate::runtime::boolean_object::{construct_boolean_from_immediate_boolean, BooleanObject};
use crate::runtime::date_instance::DateInstance;
use crate::runtime::bigint_object::BigIntObject;
use crate::runtime::node_error::{throw_coded_type_error, throw_native_type_error};
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::error_natives::put_message_property;
use crate::runtime::error_type::{error_type_name, ErrorType};
use crate::runtime::js_map::JSMap;
use crate::runtime::js_set::JSSet;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::reg_exp_object::RegExpObject;
use std::ops::ControlFlow;
use crate::runtime::iterator_operations::{for_each_in_iterable_with_method, get_value_property};
use crate::runtime::js_array_buffer::{to_js_array_buffer, JSArrayBuffer};
use crate::runtime::js_data_view::JSDataView;
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::number_object::{construct_number, NumberObject};
use crate::runtime::string_object::{construct_string, StringObject};
use crate::wtf::text::wtf_string::String as WtfString;
use std::rc::Rc;

const DATA_CLONE_MESSAGE: &str = "The object can not be cloned.";

/// A mensagem do `DataCloneError` de uma `MessagePort` no dado que não está na lista de transferência.
pub(crate) const UNLISTED_PORT_MESSAGE: &str = "Object that needs transfer was found in message but not listed in transferList";

fn data_clone_error(global_object: &JSGlobalObject, call: &HostCall) -> Thrown {
    throw_dom_exception_from_host(global_object, call, "DataCloneError", DATA_CLONE_MESSAGE)
}

/// Acima deste comprimento o array é percorrido pelo retrato das chaves de índice, não índice a índice.
const LIVE_INDEX_LIMIT: u32 = 1 << 16;

/// O elemento `index` de `source`, se ainda existe (enumerável ou não, como o `getDirectIndex` do WebCore), lido com
/// `[[Get]]` (getter roda) e clonado para `target`.
fn clone_index(global_object: &JSGlobalObject, call: &HostCall, source: &ObjectRef, target: &ObjectRef, index: u32, memory: &mut Vec<(usize, JSValue)>) -> Result<(), Thrown> {
    let vm = global_object.vm();
    let name = PropertyName::from_identifier(&Identifier::from_u32(vm, index));
    if own_descriptor(global_object, source, &name)?.is_none() {
        return Ok(());
    }
    let value = object_get(global_object, source, &name, source.as_value())?;
    let cloned = clone_value(global_object, call, value, memory)?;
    target.put_direct_index(vm, index, cloned, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect)?;
    Ok(())
}

/// Os elementos de `0..length` de um array, com o `length` lido uma vez antes de qualquer getter (o laço do
/// `ArrayStartVisitMember`: buraco continua buraco e o elemento removido por um getter some). DIVERGÊNCIA: acima de
/// `LIVE_INDEX_LIMIT` o conjunto de índices é o retrato do início, então um getter que cria elemento novo em array
/// enorme não é visto (o bun percorre os `length` índices).
fn clone_array_elements(global_object: &JSGlobalObject, call: &HostCall, source: &ObjectRef, target: &ObjectRef, length: u32, memory: &mut Vec<(usize, JSValue)>) -> Result<(), Thrown> {
    if length <= LIVE_INDEX_LIMIT {
        for index in 0..length {
            clone_index(global_object, call, source, target, index, memory)?;
        }
        return Ok(());
    }
    let names = own_names(global_object, source, PropertyNameMode::Strings, DontEnumPropertiesMode::Include)?;
    for identifier in &names {
        if let Some(index) = PropertyName::from_identifier(identifier).parse_index().filter(|index| *index < length) {
            clone_index(global_object, call, source, target, index, memory)?;
        }
    }
    Ok(())
}

/// As propriedades próprias enumeráveis de chave string de `source` (o retrato dos nomes tirado antes de qualquer
/// getter; nome removido no meio some) copiadas (clonadas) para `target`. `skip_indices` pula as chaves de índice
/// (o array as copia à parte, em `clone_array_elements`).
fn clone_own_properties(
    global_object: &JSGlobalObject,
    call: &HostCall,
    source: &ObjectRef,
    target: &ObjectRef,
    memory: &mut Vec<(usize, JSValue)>,
    skip_indices: bool,
) -> Result<(), Thrown> {
    let vm = global_object.vm();
    let names = own_names(global_object, source, PropertyNameMode::Strings, DontEnumPropertiesMode::Exclude)?;
    for identifier in &names {
        let name = PropertyName::from_identifier(identifier);
        if skip_indices && name.parse_index().is_some() {
            continue;
        }
        if own_descriptor(global_object, source, &name)?.is_none() {
            continue;
        }
        let value = object_get(global_object, source, &name, source.as_value())?;
        let cloned = clone_value(global_object, call,value, memory)?;
        match name.parse_index() {
            Some(index) => {
                target.put_direct_index(vm, index, cloned, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect)?;
            }
            None => {
                target.put_direct(vm, &name, cloned, 0);
            }
        }
    }
    Ok(())
}

/// O `ArrayBuffer` de `source` clonado (os bytes copiados; o `SharedArrayBuffer` compartilha a memória), com a
/// identidade preservada: o mesmo buffer origem dá o mesmo buffer clonado.
fn clone_buffer(global_object: &JSGlobalObject, call: &HostCall, source: &ArrayBufferRef, memory: &mut Vec<(usize, JSValue)>) -> Result<ArrayBufferRef, Thrown> {
    let wrapper = to_js_array_buffer(global_object, source).as_value();
    let key = wrapper.as_cell();
    if let Some((_, existing)) = memory.iter().find(|(seen, _)| *seen == key) {
        let existing = JSArrayBuffer::from_value(existing).ok_or(Thrown::Pending)?;
        return Ok(Rc::clone(existing.impl_()));
    }
    if source.is_detached() {
        return Err(data_clone_error(global_object, call));
    }
    let created = if source.is_shared() {
        let mut contents = ArrayBufferContents::default();
        if !source.share_with(&mut contents) {
            return Err(data_clone_error(global_object, call));
        }
        ArrayBuffer::new(contents)
    } else {
        let bytes = source.with_bytes(|bytes| bytes.to_vec());
        match source.max_byte_length() {
            Some(max) => {
                let created = ArrayBuffer::try_create(bytes.len(), 1, Some(max)).ok_or(Thrown::OutOfMemory)?;
                created.with_bytes_mut(|target| target.copy_from_slice(&bytes));
                created
            }
            None => ArrayBuffer::create_from_bytes(bytes),
        }
    };
    let created_wrapper = to_js_array_buffer(global_object, &created).as_value();
    memory.push((key, created_wrapper));
    Ok(created)
}

/// O `ErrorType` que o `name` herdado do erro seleciona na serialização (`errorNameToSerializableErrorType`): os sete
/// nomes padrão; qualquer outro valor (inclusive `AggregateError` e não string) vira `Error`.
fn serializable_error_type(name: &JSValue) -> ErrorType {
    if !name.is_string() {
        return ErrorType::Error;
    }
    let text = name.to_wtf_string();
    [ErrorType::EvalError, ErrorType::RangeError, ErrorType::ReferenceError, ErrorType::SyntaxError, ErrorType::TypeError, ErrorType::URIError]
        .into_iter()
        .find(|candidate| text.equals_latin1(Some(error_type_name(*candidate).as_bytes())))
        .unwrap_or(ErrorType::Error)
}

/// `[[Get]]` de `name` (ASCII) em `source`.
fn get_named(global_object: &JSGlobalObject, source: &ObjectRef, name: &[u8]) -> Result<JSValue, Thrown> {
    let property = PropertyName::from_identifier(&Identifier::from_span(global_object.vm(), name));
    object_get(global_object, source, &property, source.as_value())
}

/// O erro clonado como o `ErrorTag` do WebCore: só `name` (pelo tipo), `message` (propriedade própria de dados,
/// convertida em string), e, se `stack` é string, `line`, `column`, `sourceURL` e `stack`. `cause`, `errors` e as
/// propriedades extras se perdem; a classe é a do tipo, não a da subclasse.
fn clone_error(global_object: &JSGlobalObject, source: &ObjectRef, cell: usize, memory: &mut Vec<(usize, JSValue)>) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let error_type = serializable_error_type(&get_named(global_object, source, b"name")?);
    let message_name = PropertyName::from_identifier(&vm.property_names.message);
    let message = match own_descriptor(global_object, source, &message_name)? {
        Some(descriptor) if descriptor.is_data_descriptor() && !descriptor.value().is_symbol() => descriptor.value().to_wtf_string(),
        _ => WtfString::default(),
    };
    let line = get_named(global_object, source, b"line")?;
    let column = get_named(global_object, source, b"column")?;
    let source_url = get_named(global_object, source, b"sourceURL")?;
    let stack = get_named(global_object, source, b"stack")?;
    let instance = ErrorInstance::create(vm, global_object.error_structure_for(error_type), message.clone(), error_type);
    let created = instance.as_value();
    memory.push((cell, created));
    if !message.is_empty() {
        put_message_property(vm, &instance, &message);
    }
    if stack.is_string() {
        let names = &vm.property_names;
        if line.is_number() {
            instance.put_direct(vm, &PropertyName::from_identifier(&names.line), js_number(line.as_number()), DONT_ENUM);
            instance.set_line(line.as_number() as i32);
        }
        if column.is_number() {
            instance.put_direct(vm, &PropertyName::from_identifier(&names.column), js_number(column.as_number()), DONT_ENUM);
            instance.set_column(column.as_number() as i32);
        }
        if source_url.is_string() {
            instance.put_direct(vm, &PropertyName::from_identifier(&names.source_url), source_url, DONT_ENUM);
            instance.set_source_url(source_url.to_wtf_string());
        }
        instance.set_stack_value(vm, stack);
    }
    Ok(created)
}

/// Date, invólucros, `ArrayBuffer`, typed arrays, `DataView`, RegExp, Map, Set e Error; `None` se `value` não é de
/// nenhum desses.
fn clone_special(global_object: &JSGlobalObject, call: &HostCall, value: &JSValue, memory: &mut Vec<(usize, JSValue)>) -> Result<Option<JSValue>, Thrown> {
    let vm = global_object.vm();
    let cell = value.as_cell();
    if ErrorInstance::from_cell_id(cell).is_some() {
        let source = ObjectRef::from_value(value).ok_or(Thrown::Pending)?;
        return clone_error(global_object, &source, cell, memory).map(Some);
    }
    if let Some(created) = crate::runtime::crypto::clone_crypto_key(global_object, value) {
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(state) = state_of(*value) {
        // `Blob` e `File` (`BlobTag` do WebCore): o clone leva os bytes, o `type`, o nome e o `lastModified`; propriedades
        // extras e o protótipo de subclasse se perdem.
        let created = make_blob(global_object, state);
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(reg_exp) = RegExpObject::from_cell_id(cell) {
        // O clone recebe o mesmo padrão e as mesmas flags; `lastIndex` volta a 0 e propriedades extras se perdem.
        let created = RegExpObject::create(vm, global_object.reg_exp_structure(), reg_exp.reg_exp(), true).as_value();
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(map) = JSMap::from_value(value) {
        let created = JSMap::create(vm, &global_object.map_structure());
        memory.push((cell, created.as_value()));
        let cursor = map.table().borrow_mut().new_cursor();
        loop {
            let next = map.table().borrow().next_entry(&cursor);
            let Some((key, entry)) = next else { break };
            let key = clone_value(global_object, call,key, memory)?;
            let entry = clone_value(global_object, call,entry, memory)?;
            created.set(key, entry);
        }
        let source = ObjectRef::from_value(value).ok_or(Thrown::Pending)?;
        let target = ObjectRef::from_value(&created.as_value()).ok_or(Thrown::Pending)?;
        clone_own_properties(global_object, call, &source,&target, memory, false)?;
        return Ok(Some(created.as_value()));
    }
    if let Some(set) = JSSet::from_value(value) {
        let created = JSSet::create(vm, &global_object.set_structure());
        memory.push((cell, created.as_value()));
        set.for_each_key(|key| {
            let key = clone_value(global_object, call,key, memory)?;
            created.add(key);
            Ok::<_, Thrown>(ControlFlow::Continue(()))
        })?;
        let source = ObjectRef::from_value(value).ok_or(Thrown::Pending)?;
        let target = ObjectRef::from_value(&created.as_value()).ok_or(Thrown::Pending)?;
        clone_own_properties(global_object, call, &source,&target, memory, false)?;
        return Ok(Some(created.as_value()));
    }
    if let Some(big_int) = BigIntObject::from_value(value) {
        let created = BigIntObject::create(vm, global_object, big_int.internal_value()).as_value();
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(date) = DateInstance::from_value(value) {
        let created = DateInstance::create(vm, global_object.date_structure(), date.internal_number()).as_value();
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(number) = NumberObject::from_value(value) {
        let created = construct_number(vm, global_object, number.internal_value()).as_value();
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(boolean) = BooleanObject::from_value(value) {
        let created = construct_boolean_from_immediate_boolean(vm, global_object, boolean.internal_value()).as_value();
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(string) = StringObject::from_cell_id(cell) {
        let created = construct_string(vm, global_object, string.internal_value()).as_value();
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(buffer) = JSArrayBuffer::from_value(value) {
        let created = clone_buffer(global_object, call,buffer.impl_(), memory)?;
        return Ok(Some(to_js_array_buffer(global_object, &created).as_value()));
    }
    if let Some(view) = JSGenericTypedArrayView::from_value(value) {
        let source = view.possibly_shared_buffer();
        let buffer = clone_buffer(global_object, call,&source, memory)?;
        let structure = global_object.array_buffer_realm.typed_arrays.structure(view.typed_array_type(), buffer.is_resizable_or_growable_shared());
        let length = if view.is_auto_length() { None } else { Some(view.length_raw()) };
        let created = JSGenericTypedArrayView::create_with_buffer(global_object, &structure, buffer, view.byte_offset_raw(), length)?.as_value();
        memory.push((cell, created));
        return Ok(Some(created));
    }
    if let Some(view) = JSDataView::from_value(value) {
        let buffer = clone_buffer(global_object, call,view.possibly_shared_buffer(), memory)?;
        let structure = global_object.array_buffer_realm.data_view_structure(buffer.is_resizable_or_growable_shared());
        let length = if view.is_auto_length() { None } else { Some(view.byte_length_raw()) };
        let created = JSDataView::create(global_object, &structure, buffer, view.byte_offset_raw(), length)?.as_value();
        memory.push((cell, created));
        return Ok(Some(created));
    }
    Ok(None)
}

/// `true` se `value` é uma célula que não é string, `Symbol` nem BigInt (um objeto de qualquer espécie).
pub(crate) fn is_object_cell(value: &JSValue) -> bool {
    value.is_cell() && !value.is_string() && !value.is_symbol() && !value.is_big_int()
}

/// A lista `transfer` validada como o `SerializedScriptValue::create` do WebCore: `ArrayBuffer` destacável ou
/// `MessagePort` aberta, sem repetição; primitivo é `TypeError`, outro objeto é `DataCloneError`.
fn collect_transfer_list(global_object: &JSGlobalObject, call: &HostCall, list: JSValue) -> Result<(Vec<ArrayBufferRef>, Vec<JSValue>), Thrown> {
    let items = transfer_items(global_object, list, "Optional options.transfer argument must be an iterable")?;
    let mut buffers: Vec<ArrayBufferRef> = Vec::new();
    let mut ports: Vec<JSValue> = Vec::new();
    for item in items {
        if !is_object_cell(&item) {
            return Err(throw_native_type_error(global_object, "Type error"));
        }
        if is_port(item) {
            push_transfer_port(global_object, call, item, &mut ports)?;
        } else {
            push_transfer_buffer(global_object, call, &item, &mut buffers)?;
        }
    }
    Ok((buffers, ports))
}

/// Os elementos da lista `transfer` (um iterável); `invalid_message` é o `TypeError` `ERR_INVALID_ARG_TYPE` de quem não é.
pub(crate) fn transfer_items(global_object: &JSGlobalObject, list: JSValue, invalid_message: &str) -> Result<Vec<JSValue>, Thrown> {
    let vm = global_object.vm();
    let invalid = || throw_coded_type_error(global_object, invalid_message, "ERR_INVALID_ARG_TYPE");
    if !is_object_cell(&list) {
        return Err(invalid());
    }
    let method = get_value_property(global_object, list, &PropertyName::from_identifier(&vm.property_names.iterator_symbol))?;
    if !method.is_callable() {
        return Err(invalid());
    }
    let mut items = Vec::new();
    for_each_in_iterable_with_method(global_object, list, method, |item| {
        items.push(item);
        Ok(())
    })?;
    Ok(items)
}

/// Valida `item` como `ArrayBuffer` a transferir (destacável, sem repetição) e o acrescenta a `buffers`.
pub(crate) fn push_transfer_buffer(global_object: &JSGlobalObject, call: &HostCall, item: &JSValue, buffers: &mut Vec<ArrayBufferRef>) -> Result<(), Thrown> {
    let Some(wrapper) = JSArrayBuffer::from_value(item) else { return Err(data_clone_error(global_object, call)) };
    let buffer = wrapper.impl_();
    if buffers.iter().any(|seen| Rc::ptr_eq(seen, buffer)) {
        return Err(throw_dom_exception_from_host(global_object, call, "DataCloneError", "Transfer list contains duplicate ArrayBuffer"));
    }
    if buffer.is_detached() || !buffer.is_detachable() {
        return Err(data_clone_error(global_object, call));
    }
    buffers.push(Rc::clone(buffer));
    Ok(())
}

/// O algoritmo `StructuredSerialize` seguido do `StructuredDeserialize`, numa passada só.
fn clone_value(global_object: &JSGlobalObject, call: &HostCall, value: JSValue, memory: &mut Vec<(usize, JSValue)>) -> Result<JSValue, Thrown> {
    if value.is_symbol() {
        return Err(data_clone_error(global_object, call));
    }
    if value.is_string() || value.is_big_int() {
        return Ok(value);
    }
    if value.is_cell() && !value.is_callable() {
        let cell = value.as_cell();
        if let Some((_, existing)) = memory.iter().find(|(seen, _)| *seen == cell) {
            return Ok(*existing);
        }
        // `MessagePortReferenceTag` do WebCore: a porta só atravessa o clone se foi listada em `transfer` (e então já
        // está na memória, em lugar da porta nova); qualquer outra é `DataCloneError`, em qualquer profundidade.
        if is_port(value) {
            return Err(throw_dom_exception_from_host(global_object, call, "DataCloneError", UNLISTED_PORT_MESSAGE));
        }
        if let Some(created) = clone_special(global_object, call, &value, memory)? {
            return Ok(created);
        }
    }
    let Some(source) = ObjectRef::from_value(&value) else { return Ok(value) };
    if value.is_callable() {
        return Err(data_clone_error(global_object, call));
    }
    let cell = value.as_cell();
    if let Some((_, existing)) = memory.iter().find(|(seen, _)| *seen == cell) {
        return Ok(*existing);
    }
    let vm = global_object.vm();
    // `inherits<JSArray>()` do WebCore: inclui o `Array.prototype`, que é um `JSArray` (tipo derivado).
    if let Some(array) = JSArray::from_value_by_class(&value) {
        let source_length = array.length();
        let created = construct_array(vm, &global_object.array_structure(), &[]).as_value();
        memory.push((cell, created));
        let target = ObjectRef::from_value(&created).ok_or(Thrown::Pending)?;
        clone_array_elements(global_object, call, &source, &target, source_length, memory)?;
        clone_own_properties(global_object, call, &source, &target, memory, true)?;
        let length = PropertyName::from_identifier(&Identifier::from_span(vm, b"length"));
        object_set(global_object, &target, &length, js_number(source_length), created, true)?;
        return Ok(created);
    }
    // Só `Object` comum e o `Object.prototype` (`ObjectPrototype::info()` no WebCore); o resto é `DataCloneError`.
    let is_object_prototype = cell == global_object.object_prototype().as_value().as_cell();
    if source.type_() == JSType::FinalObjectType || is_object_prototype {
        let created = construct_empty_object(global_object).as_value();
        memory.push((cell, created));
        let target = ObjectRef::from_value(&created).ok_or(Thrown::Pending)?;
        clone_own_properties(global_object, call, &source,&target, memory, false)?;
        return Ok(created);
    }
    Err(data_clone_error(global_object, call))
}

fn structured_clone_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_native_type_error(global_object, "structuredClone requires 1 argument"));
    }
    let mut transfer_list = Vec::new();
    let mut ports: Vec<JSValue> = Vec::new();
    let options = call.argument(1);
    if !options.is_undefined_or_null() {
        let Some(options) = ObjectRef::from_value(&options) else { return Err(throw_native_type_error(global_object, "Type error")) };
        let vm = global_object.vm();
        let transfer = PropertyName::from_identifier(&Identifier::from_span(vm, b"transfer"));
        let list = object_get(global_object, &options, &transfer, options.as_value())?;
        if !list.is_undefined() {
            (transfer_list, ports) = collect_transfer_list(global_object, call, list)?;
        }
    }
    // No `structuredClone` as portas listadas não se movem (no bun o resultado é a própria porta, que segue ligada ao
    // par): cada uma entra na memória de identidades como ela mesma, e as não listadas continuam `DataCloneError`.
    let pairs: Vec<(JSValue, JSValue)> = ports.iter().map(|port| (*port, *port)).collect();
    let cloned = clone_value_with_ports(global_object, call, call.argument(0), &pairs)?;
    for buffer in &transfer_list {
        buffer.detach();
    }
    Ok(cloned)
}

/// O clone estruturado de `value` para quem o pede de dentro de uma função nativa (o `detail` de
/// `performance.mark`): uma memória de identidades nova, sem opções de transferência.
pub(crate) fn clone_value_for_host(global_object: &JSGlobalObject, call: &HostCall, value: JSValue) -> Result<JSValue, Thrown> {
    clone_value_with_ports(global_object, call, value, &[])
}

/// Como `clone_value_for_host`, com as portas transferidas: cada par `(porta original, porta nova)` entra na memória de
/// identidades, então toda ocorrência da original no dado (aninhada, repetida, em `Map`, `Set`, array) sai como a nova,
/// a mesma para todas, como a `MessagePortReferenceTag` serializada pelo índice na lista de transferência.
pub(crate) fn clone_value_with_ports(global_object: &JSGlobalObject, call: &HostCall, value: JSValue, ports: &[(JSValue, JSValue)]) -> Result<JSValue, Thrown> {
    let mut memory: Vec<(usize, JSValue)> = ports.iter().map(|(old, new)| (old.as_cell(), *new)).collect();
    clone_value(global_object, call, value, &mut memory)
}

/// Camada de bytes do formato de fio do `serialize`/`deserialize` (Worker): só `std`, sem `JSValue`, para que os bytes
/// cruzem threads. Inteiros em varint (7 bits por byte), `f64` em 8 bytes little-endian, cadeias em unidades UTF-16.
/// O percorredor de valores (tags por tipo, tabela de ids para ciclos) é o módulo `value_wire`.
#[allow(dead_code)]
pub(crate) mod wire {
    /// O dado terminou no meio de um item, ou um varint não cabe em 64 bits: bytes que o `serialize` não produziu.
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct Truncated;

    #[derive(Default)]
    pub(crate) struct Writer {
        out: Vec<u8>,
    }

    impl Writer {
        pub(crate) fn tag(&mut self, tag: u8) {
            self.out.push(tag);
        }

        pub(crate) fn varint(&mut self, mut value: u64) {
            while value >= 0x80 {
                self.out.push((value as u8 & 0x7f) | 0x80);
                value >>= 7;
            }
            self.out.push(value as u8);
        }

        pub(crate) fn f64(&mut self, value: f64) {
            self.out.extend_from_slice(&value.to_bits().to_le_bytes());
        }

        pub(crate) fn bytes(&mut self, bytes: &[u8]) {
            self.varint(bytes.len() as u64);
            self.out.extend_from_slice(bytes);
        }

        pub(crate) fn utf16(&mut self, units: &[u16]) {
            self.varint(units.len() as u64);
            for unit in units {
                self.out.extend_from_slice(&unit.to_le_bytes());
            }
        }

        pub(crate) fn finish(self) -> Vec<u8> {
            self.out
        }
    }

    pub(crate) struct Reader<'a> {
        data: &'a [u8],
        position: usize,
    }

    impl<'a> Reader<'a> {
        pub(crate) fn new(data: &'a [u8]) -> Self {
            Reader { data, position: 0 }
        }

        fn take(&mut self, count: usize) -> Result<&'a [u8], Truncated> {
            let end = self.position.checked_add(count).filter(|end| *end <= self.data.len()).ok_or(Truncated)?;
            let slice = &self.data[self.position..end];
            self.position = end;
            Ok(slice)
        }

        pub(crate) fn tag(&mut self) -> Result<u8, Truncated> {
            Ok(self.take(1)?[0])
        }

        /// `true` quando todos os bytes foram consumidos.
        pub(crate) fn is_at_end(&self) -> bool {
            self.position == self.data.len()
        }

        pub(crate) fn varint(&mut self) -> Result<u64, Truncated> {
            let mut value = 0u64;
            for shift in (0..64).step_by(7) {
                let byte = self.tag()?;
                value |= u64::from(byte & 0x7f) << shift;
                if byte & 0x80 == 0 {
                    return Ok(value);
                }
            }
            Err(Truncated)
        }

        pub(crate) fn f64(&mut self) -> Result<f64, Truncated> {
            let raw = self.take(8)?;
            let mut array = [0u8; 8];
            array.copy_from_slice(raw);
            Ok(f64::from_bits(u64::from_le_bytes(array)))
        }

        pub(crate) fn bytes(&mut self) -> Result<&'a [u8], Truncated> {
            let length = usize::try_from(self.varint()?).map_err(|_| Truncated)?;
            self.take(length)
        }

        pub(crate) fn utf16(&mut self) -> Result<Vec<u16>, Truncated> {
            let length = usize::try_from(self.varint()?).map_err(|_| Truncated)?;
            let raw = self.take(length.checked_mul(2).ok_or(Truncated)?)?;
            Ok(raw.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trips_every_primitive() {
            let mut writer = Writer::default();
            writer.tag(7);
            writer.varint(0);
            writer.varint(300);
            writer.varint(u64::MAX);
            writer.f64(-0.0);
            writer.f64(f64::NAN);
            writer.bytes(b"abc");
            writer.utf16(&[0x61, 0xd83d, 0xde00]);
            let data = writer.finish();
            let mut reader = Reader::new(&data);
            assert_eq!(reader.tag(), Ok(7));
            assert_eq!(reader.varint(), Ok(0));
            assert_eq!(reader.varint(), Ok(300));
            assert_eq!(reader.varint(), Ok(u64::MAX));
            assert_eq!(reader.f64().map(f64::to_bits), Ok((-0.0f64).to_bits()));
            assert!(reader.f64().is_ok_and(f64::is_nan));
            assert_eq!(reader.bytes(), Ok(&b"abc"[..]));
            assert_eq!(reader.utf16(), Ok(vec![0x61, 0xd83d, 0xde00]));
            assert_eq!(reader.tag(), Err(Truncated));
        }

        #[test]
        fn rejects_truncated_input() {
            let mut writer = Writer::default();
            writer.bytes(b"abcdef");
            let data = writer.finish();
            assert_eq!(Reader::new(&data[..4]).bytes(), Err(Truncated));
            assert_eq!(Reader::new(&[0x80; 11]).varint(), Err(Truncated));
        }
    }
}

/// `serialize`/`deserialize`: o clone estruturado em bytes (Worker, `postMessage`). O percorredor escreve uma tag por
/// tipo sobre o `wire`; objetos e buffers ganham um id na ordem em que aparecem (célula -> id no escritor, id -> valor
/// no leitor), então ciclos e referências repetidas voltam como a mesma identidade. Cobre o que o clone atual cobre,
/// inclusive `Blob`/`File` por valor e a lista `transfer`: os `ArrayBuffer` listados são copiados e só destacados
/// depois do sucesso; as `MessagePort` listadas viram handle lateral (`Serialized::ports`, referenciado por índice),
/// sem o cruzamento de thread de portas (fatia própria). O `SharedArrayBuffer` não é copiado: o `serialize` o registra
/// em `Serialized::shared` e escreve só o índice; a troca de `Rc` por `Arc` no conteúdo, para a memória cruzar threads
/// de verdade, é uma fatia própria. O `deserialize` recusa bytes que sobrem depois do valor.
#[allow(dead_code)]
pub(crate) mod value_wire {
    use super::wire::{Reader, Truncated, Writer};
    use super::{
        get_named, is_port, serializable_error_type, state_of, DATA_CLONE_MESSAGE, LIVE_INDEX_LIMIT, UNLISTED_PORT_MESSAGE,
    };
    use crate::runtime::array_buffer::{ArrayBuffer, ArrayBufferContents, ArrayBufferRef};
    use crate::runtime::bigint_object::BigIntObject;
    use crate::runtime::blob::{make_blob, BlobState};
    use crate::runtime::boolean_object::{construct_boolean_from_immediate_boolean, BooleanObject};
    use crate::runtime::date_instance::DateInstance;
    use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PropertyNameMode};
    use crate::runtime::error_instance::ErrorInstance;
    use crate::runtime::error_natives::put_message_property;
    use crate::runtime::error_type::ErrorType;
    use crate::runtime::host_call::{HostCall, Thrown};
    use crate::runtime::host_function_support::ObjectRef;
    use crate::runtime::identifier::Identifier;
    use crate::runtime::js_array::{construct_array, JSArray};
    use crate::runtime::js_array_buffer::{to_js_array_buffer, JSArrayBuffer};
    use crate::runtime::js_big_int::JSBigInt;
    use crate::runtime::js_big_int_ops::big_int_of;
    use crate::runtime::js_big_int::ImplResult;
    use crate::runtime::js_data_view::JSDataView;
    use crate::runtime::js_dom_exception::{throw_dom_exception, throw_dom_exception_from_host};
    use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
    use crate::runtime::js_global_object::JSGlobalObject;
    use crate::runtime::js_map::JSMap;
    use crate::runtime::js_set::JSSet;
    use crate::runtime::js_string::js_string;
    use crate::runtime::js_type::JSType;
    use crate::runtime::js_value::{js_boolean, js_null, js_number, js_undefined, JSValue};
    use crate::runtime::node_error::throw_native_type_error;
    use crate::runtime::number_object::{construct_number, NumberObject};
    use crate::runtime::object_constructor::{construct_empty_object, own_descriptor, own_names};
    use crate::runtime::property_attribute::DONT_ENUM;
    use crate::runtime::property_name::PropertyName;
    use crate::runtime::proxy_object::{object_get, object_set};
    use crate::runtime::reg_exp::RegExp;
    use crate::runtime::reg_exp_object::RegExpObject;
    use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
    use crate::runtime::string_object::{construct_string, StringObject};
    use crate::runtime::typed_array_type::TypedArrayType;
    use crate::wtf::text::wtf_string::String as WtfString;
    use crate::yarr::yarr_flags::{flags_string, parse_flags};
    use std::collections::HashMap;
    use std::ops::ControlFlow;
    use std::rc::Rc;

    const UNDEFINED: u8 = 0;
    const NULL: u8 = 1;
    const TRUE: u8 = 2;
    const FALSE: u8 = 3;
    const NUMBER: u8 = 4;
    const STRING: u8 = 5;
    const BIG_INT: u8 = 6;
    const OBJECT: u8 = 7;
    const ARRAY: u8 = 8;
    const DATE: u8 = 9;
    const REG_EXP: u8 = 10;
    const MAP: u8 = 11;
    const SET: u8 = 12;
    const ERROR: u8 = 13;
    const ARRAY_BUFFER: u8 = 14;
    const TYPED_ARRAY: u8 = 15;
    const DATA_VIEW: u8 = 16;
    const BOOLEAN_OBJECT: u8 = 17;
    const NUMBER_OBJECT: u8 = 18;
    const STRING_OBJECT: u8 = 19;
    const BIG_INT_OBJECT: u8 = 20;
    const REFERENCE: u8 = 21;
    const END: u8 = 22;
    const PROPERTY: u8 = 23;
    const ELEMENT: u8 = 24;
    const ENTRY: u8 = 25;
    const BUFFER_OWNED: u8 = 26;
    const BUFFER_SHARED: u8 = 27;
    const BLOB: u8 = 28;
    const PORT: u8 = 29;

    const TYPED_ARRAY_TYPES: [TypedArrayType; 14] = [
        TypedArrayType::NotTypedArray,
        TypedArrayType::Int8,
        TypedArrayType::Uint8,
        TypedArrayType::Uint8Clamped,
        TypedArrayType::Int16,
        TypedArrayType::Uint16,
        TypedArrayType::Int32,
        TypedArrayType::Uint32,
        TypedArrayType::Float16,
        TypedArrayType::Float32,
        TypedArrayType::Float64,
        TypedArrayType::BigInt64,
        TypedArrayType::BigUint64,
        TypedArrayType::DataView,
    ];

    const ERROR_TYPES: [ErrorType; 7] = [
        ErrorType::Error,
        ErrorType::EvalError,
        ErrorType::RangeError,
        ErrorType::ReferenceError,
        ErrorType::SyntaxError,
        ErrorType::TypeError,
        ErrorType::URIError,
    ];

    fn error_type_code(error_type: ErrorType) -> u8 {
        match error_type {
            ErrorType::EvalError => 1,
            ErrorType::RangeError => 2,
            ErrorType::ReferenceError => 3,
            ErrorType::SyntaxError => 4,
            ErrorType::TypeError => 5,
            ErrorType::URIError => 6,
            _ => 0,
        }
    }

    /// Os bytes do valor, os `SharedArrayBuffer` que ele referencia (por índice, sem cópia) e as `MessagePort`
    /// transferidas (handle lateral, por índice; o cruzamento de thread de portas não existe nesta fatia).
    pub(crate) struct Serialized {
        pub(crate) bytes: Vec<u8>,
        pub(crate) shared: Vec<ArrayBufferRef>,
        pub(crate) ports: Vec<JSValue>,
    }

    fn units_of(text: &WtfString) -> Vec<u16> {
        if text.is_8bit() {
            text.span8().iter().map(|byte| u16::from(*byte)).collect()
        } else {
            text.span16().to_vec()
        }
    }

    fn string_value(global_object: &JSGlobalObject, units: &[u16]) -> JSValue {
        JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(units)))
    }

    struct Serializer<'a> {
        global_object: &'a JSGlobalObject,
        call: Option<&'a HostCall>,
        writer: Writer,
        ids: HashMap<usize, u64>,
        shared: Vec<ArrayBufferRef>,
        ports: Vec<JSValue>,
    }

    impl<'a> Serializer<'a> {
        fn fail(&self, message: &str) -> Thrown {
            match self.call {
                Some(call) => throw_dom_exception_from_host(self.global_object, call, "DataCloneError", message),
                None => throw_dom_exception(self.global_object, "DataCloneError", message),
            }
        }

        fn alloc_id(&mut self, cell: usize) {
            let id = self.ids.len() as u64;
            self.ids.insert(cell, id);
        }

        fn write_string(&mut self, text: &WtfString) {
            let units = units_of(text);
            self.writer.utf16(&units);
        }

        fn write_big_int(&mut self, value: JSValue) -> Result<(), Thrown> {
            let big_int = big_int_of(value).ok_or(Thrown::Pending)?;
            self.writer.tag(u8::from(big_int.sign()));
            let digits = big_int.digits();
            self.writer.varint(digits.len() as u64);
            for digit in digits {
                self.writer.varint(*digit);
            }
            Ok(())
        }

        fn write_buffer(&mut self, source: &ArrayBufferRef) -> Result<(), Thrown> {
            let wrapper = to_js_array_buffer(self.global_object, source).as_value();
            let key = wrapper.as_cell();
            if let Some(id) = self.ids.get(&key) {
                self.writer.tag(REFERENCE);
                self.writer.varint(*id);
                return Ok(());
            }
            if source.is_detached() {
                return Err(self.fail(DATA_CLONE_MESSAGE));
            }
            self.alloc_id(key);
            if source.is_shared() {
                self.writer.tag(BUFFER_SHARED);
                self.writer.varint(self.shared.len() as u64);
                self.shared.push(Rc::clone(source));
                return Ok(());
            }
            self.writer.tag(BUFFER_OWNED);
            self.writer.varint(source.max_byte_length().map_or(0, |max| max as u64 + 1));
            let bytes = source.with_bytes(|bytes| bytes.to_vec());
            self.writer.bytes(&bytes);
            Ok(())
        }

        fn write_index(&mut self, source: &ObjectRef, index: u32) -> Result<(), Thrown> {
            let vm = self.global_object.vm();
            let name = PropertyName::from_identifier(&Identifier::from_u32(vm, index));
            if own_descriptor(self.global_object, source, &name)?.is_none() {
                return Ok(());
            }
            let value = object_get(self.global_object, source, &name, source.as_value())?;
            self.writer.tag(ELEMENT);
            self.writer.varint(u64::from(index));
            self.write_value(value)
        }

        fn write_members(&mut self, source: &ObjectRef, skip_indices: bool) -> Result<(), Thrown> {
            let names = own_names(self.global_object, source, PropertyNameMode::Strings, DontEnumPropertiesMode::Exclude)?;
            for identifier in &names {
                let name = PropertyName::from_identifier(identifier);
                if skip_indices && name.parse_index().is_some() {
                    continue;
                }
                if own_descriptor(self.global_object, source, &name)?.is_none() {
                    continue;
                }
                let value = object_get(self.global_object, source, &name, source.as_value())?;
                let key: WtfString = identifier.string().string().clone();
                self.writer.tag(PROPERTY);
                self.write_string(&key);
                self.write_value(value)?;
            }
            self.writer.tag(END);
            Ok(())
        }

        fn write_error(&mut self, source: &ObjectRef, cell: usize) -> Result<(), Thrown> {
            let vm = self.global_object.vm();
            let error_type = serializable_error_type(&get_named(self.global_object, source, b"name")?);
            let message_name = PropertyName::from_identifier(&vm.property_names.message);
            let message = match own_descriptor(self.global_object, source, &message_name)? {
                Some(descriptor) if descriptor.is_data_descriptor() && !descriptor.value().is_symbol() => descriptor.value().to_wtf_string(),
                _ => WtfString::default(),
            };
            let line = get_named(self.global_object, source, b"line")?;
            let column = get_named(self.global_object, source, b"column")?;
            let source_url = get_named(self.global_object, source, b"sourceURL")?;
            let stack = get_named(self.global_object, source, b"stack")?;
            self.alloc_id(cell);
            self.writer.tag(ERROR);
            self.writer.tag(error_type_code(error_type));
            self.write_string(&message);
            let has_stack = stack.is_string();
            self.writer.tag(u8::from(has_stack));
            if has_stack {
                let flags = u8::from(line.is_number()) | (u8::from(column.is_number()) << 1) | (u8::from(source_url.is_string()) << 2);
                self.writer.tag(flags);
                if line.is_number() {
                    self.writer.f64(line.as_number());
                }
                if column.is_number() {
                    self.writer.f64(column.as_number());
                }
                if source_url.is_string() {
                    self.write_string(&source_url.to_wtf_string());
                }
                self.write_string(&stack.to_wtf_string());
            }
            Ok(())
        }

        /// Date, invólucros, buffers, views, RegExp, Map, Set e Error; `false` se `value` não é de nenhum desses.
        fn write_special(&mut self, value: &JSValue, cell: usize) -> Result<bool, Thrown> {
            if ErrorInstance::from_cell_id(cell).is_some() {
                let source = ObjectRef::from_value(value).ok_or(Thrown::Pending)?;
                self.write_error(&source, cell)?;
                return Ok(true);
            }
            if let Some(state) = state_of(*value) {
                // Blob e File por valor: bytes, tipo, nome, indicador de arquivo e `lastModified`.
                self.alloc_id(cell);
                self.writer.tag(BLOB);
                self.writer.bytes(&state.bytes);
                self.writer.bytes(&state.content_type);
                self.writer.tag(u8::from(state.name.is_some()));
                if let Some(name) = &state.name {
                    self.writer.utf16(name);
                }
                self.writer.tag(u8::from(state.is_file));
                self.writer.tag(u8::from(state.last_modified.is_some()));
                if let Some(modified) = state.last_modified {
                    self.writer.f64(modified);
                }
                return Ok(true);
            }
            if let Some(reg_exp) = RegExpObject::from_cell_id(cell) {
                self.alloc_id(cell);
                let inner = reg_exp.reg_exp();
                self.writer.tag(REG_EXP);
                self.write_string(inner.pattern());
                let flags = flags_string(inner.flags());
                let length = flags.iter().position(|byte| *byte == 0).unwrap_or(flags.len());
                self.writer.bytes(&flags[..length]);
                return Ok(true);
            }
            if let Some(map) = JSMap::from_value(value) {
                self.alloc_id(cell);
                self.writer.tag(MAP);
                let cursor = map.table().borrow_mut().new_cursor();
                loop {
                    let next = map.table().borrow().next_entry(&cursor);
                    let Some((key, entry)) = next else { break };
                    self.writer.tag(ENTRY);
                    self.write_value(key)?;
                    self.write_value(entry)?;
                }
                self.writer.tag(END);
                let source = ObjectRef::from_value(value).ok_or(Thrown::Pending)?;
                self.write_members(&source, false)?;
                return Ok(true);
            }
            if let Some(set) = JSSet::from_value(value) {
                self.alloc_id(cell);
                self.writer.tag(SET);
                set.for_each_key(|key| {
                    self.writer.tag(ENTRY);
                    self.write_value(key)?;
                    Ok::<_, Thrown>(ControlFlow::Continue(()))
                })?;
                self.writer.tag(END);
                let source = ObjectRef::from_value(value).ok_or(Thrown::Pending)?;
                self.write_members(&source, false)?;
                return Ok(true);
            }
            if let Some(big_int) = BigIntObject::from_value(value) {
                self.alloc_id(cell);
                self.writer.tag(BIG_INT_OBJECT);
                self.write_big_int(big_int.internal_value())?;
                return Ok(true);
            }
            if let Some(date) = DateInstance::from_value(value) {
                self.alloc_id(cell);
                self.writer.tag(DATE);
                self.writer.f64(date.internal_number());
                return Ok(true);
            }
            if let Some(number) = NumberObject::from_value(value) {
                self.alloc_id(cell);
                self.writer.tag(NUMBER_OBJECT);
                self.writer.f64(number.internal_value().as_number());
                return Ok(true);
            }
            if let Some(boolean) = BooleanObject::from_value(value) {
                self.alloc_id(cell);
                self.writer.tag(BOOLEAN_OBJECT);
                self.writer.tag(u8::from(boolean.internal_value().as_boolean()));
                return Ok(true);
            }
            if let Some(string) = StringObject::from_cell_id(cell) {
                self.alloc_id(cell);
                self.writer.tag(STRING_OBJECT);
                self.write_string(&JSValue::from_js_string(string.internal_value()).to_wtf_string());
                return Ok(true);
            }
            if let Some(buffer) = JSArrayBuffer::from_value(value) {
                self.writer.tag(ARRAY_BUFFER);
                self.write_buffer(buffer.impl_())?;
                return Ok(true);
            }
            if let Some(view) = JSGenericTypedArrayView::from_value(value) {
                self.writer.tag(TYPED_ARRAY);
                self.writer.tag(view.typed_array_type() as u8);
                self.writer.tag(u8::from(view.is_auto_length()));
                self.write_buffer(&view.possibly_shared_buffer())?;
                self.writer.varint(view.byte_offset_raw() as u64);
                self.writer.varint(if view.is_auto_length() { 0 } else { view.length_raw() as u64 });
                self.alloc_id(cell);
                return Ok(true);
            }
            if let Some(view) = JSDataView::from_value(value) {
                self.writer.tag(DATA_VIEW);
                self.writer.tag(u8::from(view.is_auto_length()));
                self.write_buffer(view.possibly_shared_buffer())?;
                self.writer.varint(view.byte_offset_raw() as u64);
                self.writer.varint(if view.is_auto_length() { 0 } else { view.byte_length_raw() as u64 });
                self.alloc_id(cell);
                return Ok(true);
            }
            Ok(false)
        }

        fn write_value(&mut self, value: JSValue) -> Result<(), Thrown> {
            if value.is_symbol() {
                return Err(self.fail(DATA_CLONE_MESSAGE));
            }
            if value.is_undefined() {
                self.writer.tag(UNDEFINED);
                return Ok(());
            }
            if value.is_null() {
                self.writer.tag(NULL);
                return Ok(());
            }
            if value.is_boolean() {
                self.writer.tag(if value.as_boolean() { TRUE } else { FALSE });
                return Ok(());
            }
            if value.is_number() {
                self.writer.tag(NUMBER);
                self.writer.f64(value.as_number());
                return Ok(());
            }
            if value.is_string() {
                self.writer.tag(STRING);
                self.write_string(&value.to_wtf_string());
                return Ok(());
            }
            if value.is_big_int() {
                self.writer.tag(BIG_INT);
                return self.write_big_int(value);
            }
            if !value.is_cell() || value.is_callable() {
                return Err(self.fail(DATA_CLONE_MESSAGE));
            }
            let cell = value.as_cell();
            if let Some(id) = self.ids.get(&cell) {
                self.writer.tag(REFERENCE);
                self.writer.varint(*id);
                return Ok(());
            }
            if is_port(value) {
                let Some(index) = self.ports.iter().position(|port| port.encode() == value.encode()) else {
                    return Err(self.fail(UNLISTED_PORT_MESSAGE));
                };
                self.writer.tag(PORT);
                self.writer.varint(index as u64);
                return Ok(());
            }
            if self.write_special(&value, cell)? {
                return Ok(());
            }
            let Some(source) = ObjectRef::from_value(&value) else { return Err(self.fail(DATA_CLONE_MESSAGE)) };
            if let Some(array) = JSArray::from_value_by_class(&value) {
                let length = array.length();
                self.alloc_id(cell);
                self.writer.tag(ARRAY);
                self.writer.varint(u64::from(length));
                if length <= LIVE_INDEX_LIMIT {
                    for index in 0..length {
                        self.write_index(&source, index)?;
                    }
                } else {
                    let names = own_names(self.global_object, &source, PropertyNameMode::Strings, DontEnumPropertiesMode::Include)?;
                    for identifier in &names {
                        if let Some(index) = PropertyName::from_identifier(identifier).parse_index().filter(|index| *index < length) {
                            self.write_index(&source, index)?;
                        }
                    }
                }
                return self.write_members(&source, true);
            }
            let is_object_prototype = cell == self.global_object.object_prototype().as_value().as_cell();
            if source.type_() == JSType::FinalObjectType || is_object_prototype {
                self.alloc_id(cell);
                self.writer.tag(OBJECT);
                return self.write_members(&source, false);
            }
            Err(self.fail(DATA_CLONE_MESSAGE))
        }
    }

    /// Serializa `value`. `call` escolhe como o `DataCloneError` é lançado (`Some`: de dentro de uma função nativa).
    /// `transfer` é a lista de transferíveis, validada como a do `structuredClone` (`ArrayBuffer` destacável ou
    /// `MessagePort`, sem repetição; outro objeto é `DataCloneError`, primitivo é `TypeError`). Os buffers são copiados
    /// para os bytes e só destacados depois do sucesso; as portas vão para `Serialized::ports`.
    pub(crate) fn serialize(global_object: &JSGlobalObject, call: Option<&HostCall>, value: JSValue, transfer: &[JSValue]) -> Result<Serialized, Thrown> {
        let mut serializer = Serializer { global_object, call, writer: Writer::default(), ids: HashMap::new(), shared: Vec::new(), ports: Vec::new() };
        let mut buffers: Vec<ArrayBufferRef> = Vec::new();
        for item in transfer {
            if !super::is_object_cell(item) {
                return Err(throw_native_type_error(global_object, "Type error"));
            }
            if is_port(*item) {
                if serializer.ports.iter().any(|seen| seen.encode() == item.encode()) {
                    return Err(serializer.fail("Transfer list contains duplicate MessagePort"));
                }
                serializer.ports.push(*item);
                continue;
            }
            let Some(wrapper) = JSArrayBuffer::from_value(item) else { return Err(serializer.fail(DATA_CLONE_MESSAGE)) };
            let buffer = wrapper.impl_();
            if buffers.iter().any(|seen| Rc::ptr_eq(seen, buffer)) {
                return Err(serializer.fail("Transfer list contains duplicate ArrayBuffer"));
            }
            if buffer.is_detached() || !buffer.is_detachable() {
                return Err(serializer.fail(DATA_CLONE_MESSAGE));
            }
            buffers.push(Rc::clone(buffer));
        }
        serializer.write_value(value)?;
        for buffer in &buffers {
            buffer.detach();
        }
        Ok(Serialized { bytes: serializer.writer.finish(), shared: serializer.shared, ports: serializer.ports })
    }

    struct Deserializer<'a> {
        global_object: &'a JSGlobalObject,
        reader: Reader<'a>,
        shared: &'a [ArrayBufferRef],
        ports: &'a [JSValue],
        table: Vec<JSValue>,
    }

    impl<'a> Deserializer<'a> {
        fn corrupt(&self, _: Truncated) -> Thrown {
            throw_native_type_error(self.global_object, "Invalid serialized data")
        }

        fn invalid(&self) -> Thrown {
            throw_native_type_error(self.global_object, "Invalid serialized data")
        }

        fn tag(&mut self) -> Result<u8, Thrown> {
            self.reader.tag().map_err(|error| self.corrupt(error))
        }

        fn varint(&mut self) -> Result<u64, Thrown> {
            self.reader.varint().map_err(|error| self.corrupt(error))
        }

        fn size(&mut self) -> Result<usize, Thrown> {
            usize::try_from(self.varint()?).map_err(|_| self.invalid())
        }

        fn f64(&mut self) -> Result<f64, Thrown> {
            self.reader.f64().map_err(|error| self.corrupt(error))
        }

        fn units(&mut self) -> Result<Vec<u16>, Thrown> {
            self.reader.utf16().map_err(|error| self.corrupt(error))
        }

        fn text(&mut self) -> Result<WtfString, Thrown> {
            Ok(WtfString::from_utf16(&self.units()?))
        }

        fn register(&mut self, value: JSValue) -> JSValue {
            self.table.push(value);
            value
        }

        fn read_big_int(&mut self) -> Result<JSValue, Thrown> {
            let sign = self.tag()? != 0;
            let count = self.size()?;
            let mut words = Vec::new();
            for _ in 0..count {
                words.push(self.varint()?);
            }
            let big_int = JSBigInt::create_from_words(&words, sign).map_err(|_| Thrown::OutOfMemory)?;
            Ok(ImplResult::Heap(big_int).into_js_value(self.global_object.vm()))
        }

        /// O buffer escrito por `write_buffer`: criado (e registrado) ou a referência a um já visto.
        fn read_buffer(&mut self) -> Result<ArrayBufferRef, Thrown> {
            let tag = self.tag()?;
            let created = match tag {
                REFERENCE => {
                    let id = self.size()?;
                    let existing = self.table.get(id).ok_or_else(|| self.invalid())?;
                    let wrapper = JSArrayBuffer::from_value(existing).ok_or_else(|| self.invalid())?;
                    return Ok(Rc::clone(wrapper.impl_()));
                }
                BUFFER_SHARED => {
                    let index = self.size()?;
                    let source = self.shared.get(index).ok_or_else(|| self.invalid())?;
                    let mut contents = ArrayBufferContents::default();
                    if !source.share_with(&mut contents) {
                        return Err(throw_dom_exception(self.global_object, "DataCloneError", DATA_CLONE_MESSAGE));
                    }
                    ArrayBuffer::new(contents)
                }
                BUFFER_OWNED => {
                    let max = self.size()?;
                    let bytes = self.reader.bytes().map_err(|error| self.corrupt(error))?.to_vec();
                    if max == 0 {
                        ArrayBuffer::create_from_bytes(bytes)
                    } else {
                        let created = ArrayBuffer::try_create(bytes.len(), 1, Some(max - 1)).ok_or(Thrown::OutOfMemory)?;
                        created.with_bytes_mut(|target| target.copy_from_slice(&bytes));
                        created
                    }
                }
                _ => return Err(self.invalid()),
            };
            let wrapper = to_js_array_buffer(self.global_object, &created).as_value();
            self.register(wrapper);
            Ok(created)
        }

        fn read_members(&mut self, target: &ObjectRef) -> Result<(), Thrown> {
            let vm = self.global_object.vm();
            loop {
                match self.tag()? {
                    END => return Ok(()),
                    PROPERTY => {
                        let key = self.text()?;
                        let value = self.read_value()?;
                        let name = PropertyName::from_identifier(&Identifier::from_string(vm, &key));
                        match name.parse_index() {
                            Some(index) => {
                                target.put_direct_index(vm, index, value, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect)?;
                            }
                            None => {
                                target.put_direct(vm, &name, value, 0);
                            }
                        }
                    }
                    ELEMENT => {
                        let index = u32::try_from(self.varint()?).map_err(|_| self.invalid())?;
                        let value = self.read_value()?;
                        target.put_direct_index(vm, index, value, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect)?;
                    }
                    _ => return Err(self.invalid()),
                }
            }
        }

        fn read_error(&mut self) -> Result<JSValue, Thrown> {
            let global_object = self.global_object;
            let vm = global_object.vm();
            let code = usize::from(self.tag()?);
            let error_type = *ERROR_TYPES.get(code).ok_or_else(|| self.invalid())?;
            let message = self.text()?;
            let has_stack = self.tag()? != 0;
            let instance = ErrorInstance::create(vm, global_object.error_structure_for(error_type), message.clone(), error_type);
            let created = self.register(instance.as_value());
            if !message.is_empty() {
                put_message_property(vm, &instance, &message);
            }
            if has_stack {
                let flags = self.tag()?;
                let names = &vm.property_names;
                if flags & 1 != 0 {
                    let line = self.f64()?;
                    instance.put_direct(vm, &PropertyName::from_identifier(&names.line), js_number(line), DONT_ENUM);
                    instance.set_line(line as i32);
                }
                if flags & 2 != 0 {
                    let column = self.f64()?;
                    instance.put_direct(vm, &PropertyName::from_identifier(&names.column), js_number(column), DONT_ENUM);
                    instance.set_column(column as i32);
                }
                if flags & 4 != 0 {
                    let source_url = string_value(global_object, &self.units()?);
                    instance.put_direct(vm, &PropertyName::from_identifier(&names.source_url), source_url, DONT_ENUM);
                    instance.set_source_url(source_url.to_wtf_string());
                }
                let stack = string_value(global_object, &self.units()?);
                instance.set_stack_value(vm, stack);
            }
            Ok(created)
        }

        fn read_value(&mut self) -> Result<JSValue, Thrown> {
            let global_object = self.global_object;
            let vm = global_object.vm();
            match self.tag()? {
                UNDEFINED => Ok(js_undefined()),
                NULL => Ok(js_null()),
                TRUE => Ok(js_boolean(true)),
                FALSE => Ok(js_boolean(false)),
                NUMBER => Ok(js_number(self.f64()?)),
                STRING => Ok(string_value(global_object, &self.units()?)),
                BIG_INT => self.read_big_int(),
                REFERENCE => {
                    let id = self.size()?;
                    self.table.get(id).copied().ok_or_else(|| self.invalid())
                }
                OBJECT => {
                    let created = self.register(construct_empty_object(global_object).as_value());
                    let target = ObjectRef::from_value(&created).ok_or(Thrown::Pending)?;
                    self.read_members(&target)?;
                    Ok(created)
                }
                ARRAY => {
                    let length = u32::try_from(self.varint()?).map_err(|_| self.invalid())?;
                    let created = self.register(construct_array(vm, &global_object.array_structure(), &[]).as_value());
                    let target = ObjectRef::from_value(&created).ok_or(Thrown::Pending)?;
                    self.read_members(&target)?;
                    let name = PropertyName::from_identifier(&Identifier::from_span(vm, b"length"));
                    object_set(global_object, &target, &name, js_number(length), created, true)?;
                    Ok(created)
                }
                DATE => {
                    let time = self.f64()?;
                    let created = DateInstance::create(vm, global_object.date_structure(), time).as_value();
                    Ok(self.register(created))
                }
                REG_EXP => {
                    let pattern = self.text()?;
                    let raw = self.reader.bytes().map_err(|error| self.corrupt(error))?;
                    let flags = parse_flags(raw).ok_or_else(|| self.invalid())?;
                    let reg_exp = RegExp::create(vm, &pattern, flags);
                    let created = RegExpObject::create(vm, global_object.reg_exp_structure(), reg_exp, true).as_value();
                    Ok(self.register(created))
                }
                MAP => {
                    let created = JSMap::create(vm, &global_object.map_structure());
                    self.register(created.as_value());
                    while self.tag()? == ENTRY {
                        let key = self.read_value()?;
                        let entry = self.read_value()?;
                        created.set(key, entry);
                    }
                    let target = ObjectRef::from_value(&created.as_value()).ok_or(Thrown::Pending)?;
                    self.read_members(&target)?;
                    Ok(created.as_value())
                }
                SET => {
                    let created = JSSet::create(vm, &global_object.set_structure());
                    self.register(created.as_value());
                    while self.tag()? == ENTRY {
                        let key = self.read_value()?;
                        created.add(key);
                    }
                    let target = ObjectRef::from_value(&created.as_value()).ok_or(Thrown::Pending)?;
                    self.read_members(&target)?;
                    Ok(created.as_value())
                }
                ERROR => self.read_error(),
                BLOB => {
                    let bytes = self.reader.bytes().map_err(|error| self.corrupt(error))?.to_vec();
                    let content_type = self.reader.bytes().map_err(|error| self.corrupt(error))?.to_vec();
                    let name = if self.tag()? != 0 { Some(self.units()?) } else { None };
                    let is_file = self.tag()? != 0;
                    let last_modified = if self.tag()? != 0 { Some(self.f64()?) } else { None };
                    let created = make_blob(global_object, BlobState { bytes, content_type, name, is_file, last_modified });
                    Ok(self.register(created))
                }
                PORT => {
                    let index = self.size()?;
                    self.ports.get(index).copied().ok_or_else(|| self.invalid())
                }
                BOOLEAN_OBJECT => {
                    let flag = self.tag()? != 0;
                    let created = construct_boolean_from_immediate_boolean(vm, global_object, js_boolean(flag)).as_value();
                    Ok(self.register(created))
                }
                NUMBER_OBJECT => {
                    let number = self.f64()?;
                    let created = construct_number(vm, global_object, js_number(number)).as_value();
                    Ok(self.register(created))
                }
                STRING_OBJECT => {
                    let units = self.units()?;
                    let string = js_string(vm, &WtfString::from_utf16(&units));
                    let created = construct_string(vm, global_object, string).as_value();
                    Ok(self.register(created))
                }
                BIG_INT_OBJECT => {
                    let inner = self.read_big_int()?;
                    let created = BigIntObject::create(vm, global_object, inner).as_value();
                    Ok(self.register(created))
                }
                ARRAY_BUFFER => {
                    let buffer = self.read_buffer()?;
                    Ok(to_js_array_buffer(global_object, &buffer).as_value())
                }
                TYPED_ARRAY => {
                    let code = usize::from(self.tag()?);
                    let kind = *TYPED_ARRAY_TYPES.get(code).ok_or_else(|| self.invalid())?;
                    if matches!(kind, TypedArrayType::NotTypedArray | TypedArrayType::DataView) {
                        return Err(self.invalid());
                    }
                    let auto_length = self.tag()? != 0;
                    let buffer = self.read_buffer()?;
                    let offset = self.size()?;
                    let length = self.size()?;
                    let structure = global_object.array_buffer_realm.typed_arrays.structure(kind, buffer.is_resizable_or_growable_shared());
                    let length = if auto_length { None } else { Some(length) };
                    let created = JSGenericTypedArrayView::create_with_buffer(global_object, &structure, buffer, offset, length)?.as_value();
                    Ok(self.register(created))
                }
                DATA_VIEW => {
                    let auto_length = self.tag()? != 0;
                    let buffer = self.read_buffer()?;
                    let offset = self.size()?;
                    let length = self.size()?;
                    let structure = global_object.array_buffer_realm.data_view_structure(buffer.is_resizable_or_growable_shared());
                    let length = if auto_length { None } else { Some(length) };
                    let created = JSDataView::create(global_object, &structure, buffer, offset, length)?.as_value();
                    Ok(self.register(created))
                }
                _ => Err(self.invalid()),
            }
        }
    }

    /// O valor escrito por `serialize`; `shared` é a lista que ele devolveu junto com os bytes, e `ports` as portas que
    /// o chamador criou no lugar das transferidas (mesmo índice). Bytes que sobram depois do valor são recusados.
    pub(crate) fn deserialize(global_object: &JSGlobalObject, bytes: &[u8], shared: &[ArrayBufferRef], ports: &[JSValue]) -> Result<JSValue, Thrown> {
        let mut deserializer = Deserializer { global_object, reader: Reader::new(bytes), shared, ports, table: Vec::new() };
        let value = deserializer.read_value()?;
        if !deserializer.reader.is_at_end() {
            return Err(deserializer.invalid());
        }
        Ok(value)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::api::eval::{evaluate, new_global_object, program_source, read_global_result, VmFinalizer};
        use crate::runtime::cell_registry::run_program;

        /// Roda `setup` (grava `A`), serializa `A`, desserializa em `B` e devolve `String(check)` avaliado depois.
        fn round_trip(setup: &str, check: &str) -> String {
            round_trip_transfer(setup, "", check)
        }

        /// Como `round_trip`, com a variável global `T` (se `transfer_var` não é vazio) como único transferível.
        fn round_trip_transfer(setup: &str, transfer_var: &str, check: &str) -> String {
            run_program(|| {
                let (vm, global_object) = new_global_object();
                let _finalizer = VmFinalizer::new(&vm);
                assert!(evaluate(&global_object, &program_source(setup)).is_ok(), "setup falhou: {setup}");
                let original = read_global_result(&global_object, "A").expect("A");
                let transfer: Vec<JSValue> = if transfer_var.is_empty() { Vec::new() } else { vec![read_global_result(&global_object, transfer_var).expect("T")] };
                let serialized = serialize(&global_object, None, original, &transfer).unwrap_or_else(|_| panic!("serialize falhou: {setup}"));
                let copy = deserialize(&global_object, &serialized.bytes, &serialized.shared, &serialized.ports).unwrap_or_else(|_| panic!("deserialize falhou: {setup}"));
                // `B` entra como global de verdade (no `JSGlobalObject`, não no `JSGlobalProxy`), para o programa de
                // checagem enxergá-lo como variável.
                crate::runtime::native_class_support::install_global(&global_object, "B", copy);
                assert!(evaluate(&global_object, &program_source(&format!("var R = String({check});"))).is_ok(), "check falhou: {check}");
                let result = read_global_result(&global_object, "R").expect("R");
                result.to_wtf_string().latin1().iter().map(|byte| *byte as char).collect()
            })
        }

        #[test]
        fn primitives_round_trip() {
            assert_eq!(round_trip("var A = [undefined, null, true, false, 1.5, -0, 'olá', 123456789012345678901234567890n];", "[B[0], B[1], B[2], B[3], B[4], Object.is(B[5], -0), B[6], B[7]].join()"), ",,true,false,1.5,true,olá,123456789012345678901234567890");
        }

        #[test]
        fn cycles_and_repeats_keep_identity() {
            assert_eq!(round_trip("var o = {n: 1}; o.self = o; var A = {a: o, b: o};", "[B.a === B.b, B.a.self === B.a, B.a.n].join()"), "true,true,1");
        }

        #[test]
        fn holes_and_extra_array_properties() {
            assert_eq!(round_trip("var A = [1, , 3]; A.extra = 'x';", "[B.length, 1 in B, B.extra, B[2]].join()"), "3,false,x,3");
        }

        #[test]
        fn collections_dates_regexps_wrappers() {
            assert_eq!(
                round_trip(
                    "var A = {m: new Map([[1, {k: 2}]]), s: new Set(['a', 'b']), d: new Date(86400000), r: /a+b/gi, n: new Number(7), t: new String('hi'), l: new Boolean(false), g: Object(10n)};",
                    "[B.m.get(1).k, B.s.has('b'), B.d.getTime(), B.r.source, B.r.flags, typeof B.n, B.n.valueOf(), B.t.valueOf(), B.l.valueOf(), typeof B.g].join()",
                ),
                "2,true,86400000,a+b,gi,object,7,hi,false,object"
            );
        }

        #[test]
        fn buffers_views_share_one_buffer() {
            assert_eq!(
                round_trip(
                    "var b = new ArrayBuffer(8); var A = {u: new Uint8Array(b, 2, 4), v: new DataView(b), b: b}; A.u[0] = 9;",
                    "[B.u.buffer === B.b, B.v.buffer === B.b, B.u.byteOffset, B.u.length, B.v.getUint8(2)].join()",
                ),
                "true,true,2,4,9"
            );
        }

        #[test]
        fn errors_keep_name_and_message() {
            assert_eq!(round_trip("var A = new RangeError('boom');", "[B instanceof RangeError, B.message, B.name].join()"), "true,boom,RangeError");
        }

        #[test]
        fn function_and_symbol_throw() {
            run_program(|| {
                let (vm, global_object) = new_global_object();
                let _finalizer = VmFinalizer::new(&vm);
                assert!(evaluate(&global_object, &program_source("var A = {f() {}}; var S = Symbol('s');")).is_ok());
                let object = read_global_result(&global_object, "A").expect("A");
                let symbol = read_global_result(&global_object, "S").expect("S");
                assert!(serialize(&global_object, None, object, &[]).is_err());
                assert!(serialize(&global_object, None, symbol, &[]).is_err());
            });
        }

        #[test]
        fn truncated_input_is_rejected() {
            run_program(|| {
                let (vm, global_object) = new_global_object();
                let _finalizer = VmFinalizer::new(&vm);
                assert!(deserialize(&global_object, &[STRING, 5, 0], &[], &[]).is_err());
            });
        }

        #[test]
        fn trailing_bytes_are_rejected() {
            run_program(|| {
                let (vm, global_object) = new_global_object();
                let _finalizer = VmFinalizer::new(&vm);
                assert!(deserialize(&global_object, &[NULL], &[], &[]).is_ok());
                assert!(deserialize(&global_object, &[NULL, NULL], &[], &[]).is_err());
            });
        }

        #[test]
        fn blob_and_file_round_trip_with_identity() {
            assert_eq!(
                round_trip(
                    "var bl = new Blob(['abc'], {type: 'text/plain'}); var A = {a: bl, b: bl, f: new File(['xy'], 'n.txt', {type: 'text/x', lastModified: 5})};",
                    "[B.a === B.b, B.a.size, B.a.type, B.a instanceof Blob, B.f instanceof File, B.f.name, B.f.size, B.f.lastModified].join()",
                ),
                // O tipo é a constante canônica (medido no bun 1.4.2: `structuredClone` do Blob `text/plain` dá
                // `text/plain;charset=utf-8`).
                "true,3,text/plain;charset=utf-8,true,true,n.txt,2,5"
            );
        }

        #[test]
        fn transfer_copies_then_detaches_the_original() {
            assert_eq!(
                round_trip_transfer("var T = new ArrayBuffer(4); new Uint8Array(T)[1] = 7; var A = {u: new Uint8Array(T)};", "T", "[B.u[1], B.u.length, T.byteLength].join()"),
                "7,4,0"
            );
        }

        #[test]
        fn transfer_list_rejects_non_transferable() {
            run_program(|| {
                let (vm, global_object) = new_global_object();
                let _finalizer = VmFinalizer::new(&vm);
                assert!(evaluate(&global_object, &program_source("var A = {}; var T = {};")).is_ok());
                let original = read_global_result(&global_object, "A").expect("A");
                let item = read_global_result(&global_object, "T").expect("T");
                assert!(serialize(&global_object, None, original, &[item]).is_err());
                assert!(serialize(&global_object, None, original, &[js_number(1.0)]).is_err());
            });
        }
    }
}

host_function!(global_func_structured_clone, structured_clone_body);

/// Instala `structuredClone` no global como propriedade de dados comum.
pub fn add_structured_clone(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let name = Identifier::from_span(vm, b"structuredClone");
    let function = JSFunction::create_native(
        vm,
        global_object,
        2,
        name.string().string(),
        global_func_structured_clone,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    global_object.put_direct(vm, &PropertyName::from_identifier(&name), function.as_value(), 0);
}
