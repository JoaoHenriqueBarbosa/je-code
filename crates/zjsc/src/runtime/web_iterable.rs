//! O esqueleto comum de `URLSearchParams`, `FormData` e `Headers` (as classes do bun/WebCore com uma lista de pares
//! nome/valor, iterador próprio, `forEach` e `Symbol.iterator` igual a `entries`):
//!
//! - o estado das instâncias e dos iteradores, em `thread_local` único (zerado em [`reset_for_program`]), com a classe
//!   gravada junto: a marca que o brand check confere;
//! - o acesso à lista de `this` ([`with_entries`], erro `Can only call <Classe>.<método> on instances of <Classe>`);
//! - o objeto iterador com protótipo próprio (`next` e `@@toStringTag` `<Classe> Iterator`), vivo nas três classes:
//!   relê a lista (na ordem da classe) a cada `next` mantendo o índice, e esgotado fica esgotado (medido no bun);
//! - `entries`/`keys`/`values`/`forEach`/`next` como funções nativas ([`web_iterable_functions!`]);
//! - a leitura do argumento do construtor: registro e sequência de pares;
//! - os auxiliares de texto (`Units`) e de método (`require_arguments`, `put_enumerable_methods`, `toJSON`).
//!
//! O que difere entre as classes é o [`WebIterable`] da classe: o nome, a ordem de iteração, a validação do par
//! gravado e as mensagens medidas da sequência.

use std::any::Any;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PropertyNameMode};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::{call_checked, for_each_in_iterable, for_each_in_iterable_with_method, get_value_property};
use crate::runtime::js_array::construct_array;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSNonFinalObject, JSObject};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{instance_structure, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::{identifier_to_js_value, own_names};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::create_iterator_result_object;
use crate::runtime::structure::{Structure, StructureRef};
use crate::wtf::text::wtf_string::String as WtfString;

pub(crate) type Units = Vec<u16>;
/// A lista de pares nome/valor; o tipo do valor é o da classe (`Units` para `URLSearchParams` e `Headers`).
pub(crate) type Pairs<V = Units> = Vec<(Units, V)>;

/// O que difere entre as classes que compartilham o esqueleto.
pub(crate) trait WebIterable: 'static {
    /// O tipo do valor de um par (texto, ou texto e arquivo no `FormData`).
    type Value: Clone + 'static;
    /// O nome da classe: nas mensagens, no `@@toStringTag` e como marca do brand check.
    const NAME: &'static str;
    /// Item de uma sequência de pares que não é iterável: `true` lança `Type error` antes de contar os elementos.
    const ITEM_NEEDS_ITERATOR: bool = false;
    /// A mensagem de um par da sequência que não tem exatamente dois elementos.
    const SUB_SEQUENCE_ERROR: &'static str = "Type error";
    /// Os textos do construtor são `USVString` do WebIDL (o Symbol tem a mensagem dos bindings, ver
    /// [`usv_units_of`]); `false` é a conversão do JSC.
    const USV_STRINGS: bool = false;

    /// Um texto do construtor (nome ou valor de um par), com a conversão da classe.
    fn units(global_object: &JSGlobalObject, value: JSValue) -> Result<Units, Thrown> {
        if Self::USV_STRINGS { usv_units_of(global_object, value) } else { units_of(global_object, value) }
    }

    /// A ordem em que a iteração enxerga a lista.
    fn ordered(entries: &Pairs<Self::Value>) -> Cow<'_, Pairs<Self::Value>> {
        Cow::Borrowed(entries)
    }

    /// Grava um par vindo do construtor (a validação da classe, se houver).
    fn push(entries: &mut Pairs<Self::Value>, name: Units, value: Self::Value) -> Result<(), Thrown> {
        entries.push((name, value));
        Ok(())
    }

    /// O valor de um texto que o construtor leu (registro ou sequência).
    fn text(units: Units) -> Self::Value;

    /// O valor JS de um valor guardado.
    fn to_js(global_object: &JSGlobalObject, value: &Self::Value) -> JSValue;
}

/// Os dois itens de [`WebIterable`] das classes cujo valor é só texto (`URLSearchParams` e `Headers`).
#[macro_export]
macro_rules! text_web_iterable_values {
    () => {
        type Value = $crate::runtime::web_iterable::Units;

        fn text(units: $crate::runtime::web_iterable::Units) -> Self::Value {
            units
        }

        fn to_js(global_object: &$crate::runtime::js_global_object::JSGlobalObject, value: &Self::Value) -> $crate::runtime::js_value::JSValue {
            $crate::runtime::web_iterable::string_value(global_object, value)
        }
    };
}

/// Iterador vivo: a classe, a lista dona (o valor codificado), o que ele devolve (0 chaves, 1 valores, 2 pares) e o
/// índice; a lista (na ordem da classe) é relida a cada `next`.
struct IteratorState {
    class: &'static str,
    owner: EncodedJSValue,
    kind: u8,
    index: usize,
}

thread_local! {
    /// As instâncias do programa (o valor codificado da célula) com a classe e a lista de pares.
    static INSTANCES: RefCell<HashMap<EncodedJSValue, (&'static str, Box<dyn Any>)>> = RefCell::new(HashMap::new());
    /// Os iteradores do programa.
    static ITERATORS: RefCell<HashMap<EncodedJSValue, IteratorState>> = RefCell::new(HashMap::new());
    /// A `Structure` dos objetos iteradores de cada classe, criada na instalação.
    static ITERATOR_STRUCTURES: RefCell<HashMap<&'static str, StructureRef>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): o que foi guardado é do programa.
/// As listas de pares (só texto e bytes, sem valores do programa) são campo da instância e saem com ela; o iterador
/// segura a instância dona, como o `m_iterable` do iterador do WebCore.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    let instances = crate::runtime::gc::side_table(&INSTANCES, marker, phase, |_, _| {});
    let iterators = crate::runtime::gc::side_table(&ITERATORS, marker, phase, |iterator, out| out.push(JSValue::decode(iterator.owner)));
    instances | iterators
}

pub(crate) fn reset_for_program() {
    let _ = INSTANCES.try_with(|instances| instances.borrow_mut().clear());
    let _ = ITERATORS.try_with(|iterators| iterators.borrow_mut().clear());
}

pub(crate) fn units_of(global_object: &JSGlobalObject, value: JSValue) -> Result<Units, Thrown> {
    let text = pending_or(global_object, value.to_wtf_string())?;
    Ok((0..text.length()).map(|index| text.code_unit_at(index)).collect())
}

/// A conversão `IDLUSVString` dos bindings do WebCore (`URL`, `URLSearchParams`, `FormData`): o Symbol é recusado com
/// a mensagem do WebIDL, não a do `toString` do JSC que `Headers`, `Blob` e `TextEncoder` mostram (medido no bun 1.4.2).
pub(crate) fn usv_units_of(global_object: &JSGlobalObject, value: JSValue) -> Result<Units, Thrown> {
    if value.is_symbol() {
        return Err(Thrown::type_error("Cannot convert a Symbol value to a string"));
    }
    units_of(global_object, value)
}

pub(crate) fn string_value(global_object: &JSGlobalObject, units: &[u16]) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(units)))
}

pub(crate) fn require_arguments(global_object: &JSGlobalObject, call: &HostCall, count: usize) -> Result<(), Thrown> {
    if call.argument_count() < count {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    Ok(())
}

pub(crate) fn not_a_sequence(global_object: &JSGlobalObject) -> Thrown {
    throw_coded_type_error(global_object, "Value is not a sequence", "ERR_INVALID_ARG_TYPE")
}

/// Registra a instância recém-criada da classe com a lista inicial.
pub(crate) fn register_instance<C: WebIterable>(instance: JSValue, pairs: Pairs<C::Value>) {
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), (C::NAME, Box::new(pairs))));
}

/// O nome da classe (`Headers`, `URLSearchParams`, `FormData`) de `value`, se for instância de uma delas.
pub(crate) fn class_name_of(value: JSValue) -> Option<&'static str> {
    INSTANCES.with(|instances| instances.borrow().get(&value.encode()).map(|entry| entry.0))
}

/// O objeto de `toJSON` de `value` quando é instância da classe `C` (o que o `console.log` do bun imprime).
pub(crate) fn json_object_of<C: WebIterable>(global_object: &JSGlobalObject, value: JSValue) -> Option<JSValue> {
    let pairs = with_instance::<C, _>(value, |pairs| pairs.clone())?;
    Some(pairs_to_json_object::<C>(global_object, pairs, None))
}

/// A lista de `value`, se for instância da classe.
pub(crate) fn with_instance<C: WebIterable, R>(value: JSValue, body: impl FnOnce(&mut Pairs<C::Value>) -> R) -> Option<R> {
    INSTANCES.with(|instances| {
        let mut instances = instances.borrow_mut();
        let entry = instances.get_mut(&value.encode()).filter(|entry| entry.0 == C::NAME)?;
        entry.1.downcast_mut::<Pairs<C::Value>>().map(body)
    })
}

/// A lista de `this`, se for instância; senão o `TypeError` de `this` inválido.
pub(crate) fn with_entries<C: WebIterable, R>(global_object: &JSGlobalObject, call: &HostCall, method: &str, body: impl FnOnce(&mut Pairs<C::Value>) -> R) -> Result<R, Thrown> {
    with_instance::<C, R>(call.this_value(), body)
        .ok_or_else(|| throw_coded_type_error(global_object, &format!("Can only call {name}.{method} on instances of {name}", name = C::NAME), "ERR_INVALID_THIS"))
}

/// Item `index` da lista do dono, na ordem da classe, se ainda existir.
fn item_at<C: WebIterable>(owner: EncodedJSValue, index: usize) -> Option<(Units, C::Value)> {
    INSTANCES.with(|instances| {
        let instances = instances.borrow();
        let pairs = instances.get(&owner)?.1.downcast_ref::<Pairs<C::Value>>()?;
        C::ordered(pairs).get(index).cloned()
    })
}

/// `entries`/`keys`/`values`: o iterador sobre `this`.
pub(crate) fn make_iterator<C: WebIterable>(global_object: &JSGlobalObject, call: &HostCall, method: &str, kind: u8) -> HostResult {
    with_entries::<C, _>(global_object, call, method, |_| ())?;
    let structure = ITERATOR_STRUCTURES.with(|structures| structures.borrow().get(C::NAME).cloned()).expect("iterador sem estrutura (instalação)");
    let iterator = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let state = IteratorState { class: C::NAME, owner: call.this_value().encode(), kind, index: 0 };
    ITERATORS.with(|iterators| iterators.borrow_mut().insert(iterator.encode(), state));
    Ok(iterator)
}

/// `next` do iterador. Esgotado uma vez, fica esgotado (o índice vira `usize::MAX`).
pub(crate) fn iterator_next<C: WebIterable>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let step = ITERATORS.with(|iterators| {
        let mut iterators = iterators.borrow_mut();
        let state = iterators.get_mut(&call.this_value().encode()).filter(|state| state.class == C::NAME)?;
        let item = item_at::<C>(state.owner, state.index);
        state.index = if item.is_some() { state.index + 1 } else { usize::MAX };
        Some((state.kind, item))
    });
    let Some((kind, item)) = step else {
        return Err(throw_coded_type_error(global_object, "Cannot call next() on a non-Iterator object", "ERR_INVALID_THIS"));
    };
    let Some((name, value)) = item else {
        return Ok(create_iterator_result_object(global_object, JSValue::undefined(), true));
    };
    let result = match kind {
        0 => string_value(global_object, &name),
        1 => C::to_js(global_object, &value),
        _ => construct_array(global_object.vm(), &global_object.array_structure(), &[string_value(global_object, &name), C::to_js(global_object, &value)]).as_value(),
    };
    Ok(create_iterator_result_object(global_object, result, false))
}

/// `forEach` sobre a lista viva de `this`.
pub(crate) fn for_each<C: WebIterable>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entries::<C, _>(global_object, call, "forEach", |_| ())?;
    let callback = call.argument(0);
    let not_a_function = "Cannot call callback on a non-function";
    if !callback.is_callable() {
        return Err(throw_coded_type_error(global_object, not_a_function, "ERR_INVALID_ARG_TYPE"));
    }
    let this_key = call.this_value().encode();
    let mut index = 0;
    // A lista é relida (e reordenada) a cada passo: o que o callback acrescenta ainda é visitado (medido).
    while let Some((name, value)) = item_at::<C>(this_key, index) {
        let arguments = [C::to_js(global_object, &value), string_value(global_object, &name), call.this_value()];
        call_checked(global_object, callback, call.argument(1), &arguments, not_a_function)?;
        index += 1;
    }
    Ok(JSValue::undefined())
}

/// Define `entries_fn`, `keys_fn`, `values_fn`, `for_each_fn` e `iterator_next_fn` da classe `$class`.
#[macro_export]
macro_rules! web_iterable_functions {
    ($class:ty) => {
        fn entries_body(global_object: &$crate::runtime::js_global_object::JSGlobalObject, call: &$crate::runtime::host_call::HostCall) -> $crate::runtime::host_call::HostResult {
            $crate::runtime::web_iterable::make_iterator::<$class>(global_object, call, "entries", 2)
        }
        fn keys_body(global_object: &$crate::runtime::js_global_object::JSGlobalObject, call: &$crate::runtime::host_call::HostCall) -> $crate::runtime::host_call::HostResult {
            $crate::runtime::web_iterable::make_iterator::<$class>(global_object, call, "keys", 0)
        }
        fn values_body(global_object: &$crate::runtime::js_global_object::JSGlobalObject, call: &$crate::runtime::host_call::HostCall) -> $crate::runtime::host_call::HostResult {
            $crate::runtime::web_iterable::make_iterator::<$class>(global_object, call, "values", 1)
        }
        $crate::host_function!(entries_fn, entries_body);
        $crate::host_function!(keys_fn, keys_body);
        $crate::host_function!(values_fn, values_body);
        $crate::host_function!(for_each_fn, $crate::runtime::web_iterable::for_each::<$class>);
        $crate::host_function!(iterator_next_fn, $crate::runtime::web_iterable::iterator_next::<$class>);
    };
}

/// `@@iterator` é a própria função `entries`, não enumerável.
pub(crate) fn put_iterator_alias(global_object: &JSGlobalObject, prototype: &JSObject) {
    let vm = global_object.vm();
    let entries = prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"entries")));
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.iterator_symbol), entries, DONT_ENUM);
}

/// O fim da instalação do protótipo: `@@toStringTag` da classe e o protótipo do iterador (`next`, `@@toStringTag`
/// `<Classe> Iterator`, herdando de `%IteratorPrototype%`) com a `Structure` dos iteradores.
pub(crate) fn install_iteration<C: WebIterable>(global_object: &JSGlobalObject, prototype: &JSObject, iterator_info: &'static ClassInfo, next: NativeFunction) {
    let vm = global_object.vm();
    put_to_string_tag(vm, prototype, C::NAME);

    let iterator_prototype_structure =
        Structure::create(vm, Some(global_object), global_object.iterator_prototype().as_value(), TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS), iterator_info);
    let iterator_prototype = JSObject::allocate(vm, &iterator_prototype_structure);
    iterator_prototype.did_become_prototype(vm);
    put_direct_native_function_without_transition(vm, global_object, &iterator_prototype, &vm.property_names.next, 0, next, ImplementationVisibility::Public, Intrinsic::NoIntrinsic, 0);
    put_to_string_tag(vm, &iterator_prototype, &format!("{} Iterator", C::NAME));
    let structure = instance_structure(vm, Some(global_object), iterator_prototype.as_value());
    ITERATOR_STRUCTURES.with(|structures| structures.borrow_mut().insert(C::NAME, structure));
}

/// Os métodos públicos enumeráveis (ao contrário da maioria dos protótipos, medido) das classes.
pub(crate) fn put_enumerable_methods(global_object: &JSGlobalObject, prototype: &JSObject, methods: &[(&str, u32, NativeFunction)]) {
    let vm = global_object.vm();
    for &(name, length, function) in methods {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            prototype,
            &Identifier::from_span(vm, name.as_bytes()),
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            0,
        );
    }
}

/// Registro: as chaves próprias enumeráveis (texto e símbolo) com os valores lidos.
fn pairs_of_record<C: WebIterable>(global_object: &JSGlobalObject, init: JSValue) -> Result<Pairs<C::Value>, Thrown> {
    let vm = global_object.vm();
    let keys = own_names(global_object, &*init.as_object(), PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Exclude)?;
    let mut pairs = Pairs::<C::Value>::new();
    for key in keys {
        if key.is_symbol() {
            return Err(Thrown::type_error("Cannot convert a symbol to a string"));
        }
        let name = C::units(global_object, identifier_to_js_value(vm, &key))?;
        let value = get_value_property(global_object, init, &PropertyName::from_identifier(&key))?;
        let value = C::text(C::units(global_object, value)?);
        C::push(&mut pairs, name, value)?;
    }
    Ok(pairs)
}

/// Iterável de pares: cada item é um objeto iterável com exatamente dois elementos.
fn pairs_of_sequence<C: WebIterable>(global_object: &JSGlobalObject, init: JSValue, iterator_method: JSValue) -> Result<Pairs<C::Value>, Thrown> {
    let mut pairs = Pairs::<C::Value>::new();
    for_each_in_iterable_with_method(global_object, init, iterator_method, |item| -> Result<(), Thrown> {
        if !item.is_object() {
            return Err(not_a_sequence(global_object));
        }
        if C::ITEM_NEEDS_ITERATOR {
            let item_method = get_value_property(global_object, item, &PropertyName::from_identifier(&global_object.vm().property_names.iterator_symbol))?;
            if item_method.is_undefined_or_null() {
                return Err(Thrown::type_error("Type error"));
            }
        }
        let mut parts = Vec::new();
        for_each_in_iterable(global_object, item, |part| -> Result<(), Thrown> {
            parts.push(part);
            Ok(())
        })?;
        if parts.len() != 2 {
            return Err(Thrown::type_error(C::SUB_SEQUENCE_ERROR));
        }
        let (name, value) = (C::units(global_object, parts[0])?, C::text(C::units(global_object, parts[1])?));
        C::push(&mut pairs, name, value)
    })?;
    Ok(pairs)
}

/// O argumento objeto do construtor: sequência de pares se tem `Symbol.iterator`, senão registro.
pub(crate) fn pairs_of_object<C: WebIterable>(global_object: &JSGlobalObject, init: JSValue) -> Result<Pairs<C::Value>, Thrown> {
    let iterator_method = get_value_property(global_object, init, &PropertyName::from_identifier(&global_object.vm().property_names.iterator_symbol))?;
    if iterator_method.is_undefined_or_null() {
        return pairs_of_record::<C>(global_object, init);
    }
    if !iterator_method.is_callable() {
        return Err(Thrown::type_error("Symbol.iterator property should be callable"));
    }
    pairs_of_sequence::<C>(global_object, init, iterator_method)
}

/// O objeto de `toJSON`: nome repetido vira array de valores; `tag` é o `@@toStringTag` próprio, quando medido.
pub(crate) fn pairs_to_json_object<C: WebIterable>(global_object: &JSGlobalObject, pairs: Pairs<C::Value>, tag: Option<&str>) -> JSValue {
    let vm = global_object.vm();
    let mut groups: Vec<(Units, Vec<C::Value>)> = Vec::new();
    for (name, value) in pairs {
        match groups.iter_mut().find(|group| group.0 == name) {
            Some(group) => group.1.push(value),
            None => groups.push((name, vec![value])),
        }
    }
    let object = JSFinalObject::create(vm, &global_object.object_structure_for_object_constructor());
    for (name, values) in groups {
        let key = PropertyName::from_identifier(&Identifier::from_string(vm, &WtfString::from_utf16(&name)));
        let value = match values.as_slice() {
            [single] => C::to_js(global_object, single),
            _ => {
                let items: Vec<JSValue> = values.iter().map(|value| C::to_js(global_object, value)).collect();
                construct_array(vm, &global_object.array_structure(), &items).as_value()
            }
        };
        let _ = object.define_own_property(vm, &key, &PropertyDescriptor::new(value, 0), false);
    }
    if let Some(tag) = tag {
        put_to_string_tag(vm, &object, tag);
    }
    object.as_value()
}
