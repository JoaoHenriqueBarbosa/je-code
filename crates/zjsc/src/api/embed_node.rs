//! Runner de programa no estilo Node que DEVOLVE o controle ao hospedeiro: o oposto de `evaluate_main_script`, que roda o
//! programa e o laço de eventos até o fim. É o que um host de extensões precisa: o hospedeiro avalia o programa principal
//! (um bootstrap CommonJS com `require`, `process` e os módulos `node:*`), guarda funções JavaScript, chama-as quando chega
//! um evento de fora e avança o laço de eventos em fatias que ele mesmo mede (o relógio do laço é virtual: alimentá-lo
//! com o tempo real que passou dá a semântica de tempo real).
//!
//! MUDANÇA LOCAL do je-code (README da crate): este arquivo não existe no pseudo-linus.
//!
//! A fronteira com o hospedeiro é só texto: uma função global `__je_call(nome, json)` que o programa chama e que o
//! hospedeiro atende com [`NodeRunner::set_bridge`] (devolve o JSON da resposta ou a mensagem de um `TypeError`), mais
//! [`NodeRunner::call`] para o caminho inverso. O JSON dispensa o hospedeiro de conhecer as células do motor.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::api::eval::{describe_exception, evaluate, install_cjs_fs, named_program_source, new_global_object, run_main_program, VmFinalizer};
use crate::api::module_probe::ModuleFs;
use crate::host_function;
use crate::runtime::cell_registry::run_program;
use crate::runtime::console_host::ConsoleHost;
use crate::runtime::gc::{Root, RootScope};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_string::js_string;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_array::{construct_array, is_js_array};
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::object_constructor::{construct_empty_object, enumerable_own_entries};
use crate::runtime::property_attribute::{ACCESSOR, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::{get_object_property, set_object_property};
use crate::runtime::native_class_support::install_global_function_with_visibility;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// O que atende `__je_call(nome, json)`: devolve o JSON da resposta, ou a mensagem do `TypeError` que o programa vê.
pub type Bridge = Rc<dyn Fn(&str, &str) -> Result<String, String>>;

thread_local! {
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
}

fn bridge_call_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = rust_string(&call.argument(0).to_wtf_string());
    let payload = rust_string(&call.argument(1).to_wtf_string());
    let Some(bridge) = BRIDGE.with(|slot| slot.borrow().clone()) else {
        return Err(Thrown::type_error("__je_call: o hospedeiro não instalou a ponte"));
    };
    match bridge(&name, &payload) {
        Ok(text) => Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes())))),
        Err(message) => Err(Thrown::TypeError(message)),
    }
}
host_function!(bridge_call, bridge_call_body);

/// O que atende uma função nativa criada por [`NodeRunner::make_function`]: recebe os argumentos já lidos e devolve o
/// resultado, ou a mensagem do `TypeError` que o JavaScript vê.
pub type NativeCallback = Rc<dyn Fn(&[JsData]) -> Result<JsData, String>>;

thread_local! {
    /// As funções nativas vivas, pelo id da célula da função.
    static NATIVES: RefCell<HashMap<usize, NativeCallback>> = RefCell::new(HashMap::new());
}

fn native_trampoline_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(callback) = NATIVES.with(|natives| natives.borrow().get(&call.callee()).cloned()) else {
        return Err(Thrown::type_error("função nativa sem implementação registrada"));
    };
    let Some(global) = global_object.to_ref() else { return Ok(JSValue::undefined()) };
    let arguments: Vec<JsData> = call.arguments().iter().map(|argument| read_value(&global, *argument, 0)).collect();
    match callback(&arguments) {
        Ok(data) => Ok(write_value(&global, &data)),
        Err(message) => Err(Thrown::TypeError(message)),
    }
}
host_function!(native_trampoline, native_trampoline_body);

thread_local! {
    /// As funções nativas que recebem os objetos por referência ([`NodeRunner::make_function_by_reference`]), pelo id da célula.
    static REFERENCE_NATIVES: RefCell<HashMap<usize, NativeCallback>> = RefCell::new(HashMap::new());
}

/// O argumento de uma função por referência: um objeto (comum, array, instância de classe de usuário) vira [`JsData::Handle`] e
/// mantém a identidade e o protótipo; o que não é objeto viaja por cópia, como em [`read_value`].
fn read_reference(global: &JSGlobalObjectRef, value: JSValue) -> JsData {
    if value.is_object() { JsData::Handle(Rc::new(JsHandle::new(value))) } else { read_value(global, value, 0) }
}

fn reference_trampoline_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(callback) = REFERENCE_NATIVES.with(|natives| natives.borrow().get(&call.callee()).cloned()) else {
        return Err(Thrown::type_error("função nativa sem implementação registrada"));
    };
    let Some(global) = global_object.to_ref() else { return Ok(JSValue::undefined()) };
    let arguments: Vec<JsData> = call.arguments().iter().map(|argument| read_reference(&global, *argument)).collect();
    match callback(&arguments) {
        Ok(data) => Ok(write_value(&global, &data)),
        Err(message) => Err(Thrown::TypeError(message)),
    }
}
host_function!(reference_trampoline, reference_trampoline_body);

/// A chave reservada que marca o objeto como instância de uma classe registrada: não enumerável no lado JavaScript, e no
/// lado do Rust um campo `("$$class", Str(nome))` de [`JsData::Object`] (o equivalente de `instanceof`).
pub const CLASS_KEY: &str = "$$class";

/// O corpo de um método de classe: recebe o `this` (copiado, com `$$class`) e os argumentos.
pub type MethodCallback = Rc<dyn Fn(&JsData, &[JsData]) -> Result<JsData, String>>;

/// A descrição de uma classe nativa para [`NodeRunner::define_class`].
pub struct ClassSpec {
    pub name: String,
    pub length: u32,
    /// O construtor: devolve os campos próprios da instância (um [`JsData::Object`]).
    pub construct: NativeCallback,
    pub methods: Vec<(String, u32, MethodCallback)>,
    /// Acessores somente leitura no protótipo (`get isEmpty()`): o corpo recebe o `this` copiado.
    pub getters: Vec<(String, MethodCallback)>,
    pub statics: Vec<(String, u32, NativeCallback)>,
    /// A classe base já definida (`Selection extends Range`): o protótipo e o construtor herdam dela.
    pub parent: Option<String>,
}

thread_local! {
    /// Os módulos do hospedeiro (`require('vscode')`), pelo nome.
    static HOST_MODULES: RefCell<HashMap<String, JsHandle>> = RefCell::new(HashMap::new());
}

/// O `exports` do módulo do hospedeiro `request`, que o `require` consulta antes dos embutidos do Node.
pub(crate) fn host_module(request: &str) -> Option<JSValue> {
    HOST_MODULES.with(|modules| modules.borrow().get(request).map(|handle| handle.0))
}

/// Esvazia o estado por programa quando o [`run_node`] termina, para o próximo programa da mesma thread não herdar nada.
struct ProgramState;

impl Drop for ProgramState {
    fn drop(&mut self) {
        let _ = BRIDGE.try_with(|slot| slot.borrow_mut().take());
        let _ = NATIVES.try_with(|natives| natives.borrow_mut().clear());
        let _ = REFERENCE_NATIVES.try_with(|natives| natives.borrow_mut().clear());
        let _ = CONSTRUCTORS.try_with(|constructors| constructors.borrow_mut().clear());
        let _ = METHODS.try_with(|methods| methods.borrow_mut().clear());
        let _ = CLASSES.try_with(|classes| classes.borrow_mut().clear());
        let _ = CLASS_CONSTRUCTORS.try_with(|constructors| constructors.borrow_mut().clear());
        let _ = HOST_MODULES.try_with(|modules| modules.borrow_mut().clear());
    }
}

struct ClassEntry {
    name: String,
    construct: NativeCallback,
}

thread_local! {
    /// O protótipo de cada classe registrada, pelo nome (a raiz do coletor é o [`JsHandle`] guardado junto).
    static CLASSES: RefCell<HashMap<String, JsHandle>> = RefCell::new(HashMap::new());
    /// Os construtores vivos, pelo id da célula da função.
    static CONSTRUCTORS: RefCell<HashMap<usize, ClassEntry>> = RefCell::new(HashMap::new());
    /// O construtor de cada classe registrada, pelo nome (para a herança).
    static CLASS_CONSTRUCTORS: RefCell<HashMap<String, JsHandle>> = RefCell::new(HashMap::new());
    /// Os métodos vivos, pelo id da célula da função.
    static METHODS: RefCell<HashMap<usize, MethodCallback>> = RefCell::new(HashMap::new());
}

fn class_call_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = CONSTRUCTORS.with(|constructors| constructors.borrow().get(&call.callee()).map(|entry| entry.name.clone())).unwrap_or_default();
    let _ = global_object;
    Err(Thrown::type_error(&format!("Class constructor {name} cannot be invoked without 'new'")))
}
host_function!(class_call, class_call_body);

fn class_construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some((name, construct)) = CONSTRUCTORS.with(|constructors| constructors.borrow().get(&call.callee()).map(|entry| (entry.name.clone(), entry.construct.clone()))) else {
        return Err(Thrown::type_error("construtor nativo sem implementação registrada"));
    };
    let Some(global) = global_object.to_ref() else { return Ok(JSValue::undefined()) };
    let vm = global.vm();
    let identifier = Identifier::from_span(vm, b"prototype");
    let prototype = get_object_property(global_object, call.new_target(), &identifier)?;
    let arguments: Vec<JsData> = call.arguments().iter().map(|argument| read_value(&global, *argument, 0)).collect();
    let fields = construct(&arguments).map_err(Thrown::TypeError)?;
    let instance = construct_empty_object(&global);
    if prototype.is_object() {
        instance.set_prototype_direct(vm, prototype);
    }
    mark_class(&global, &instance, &name);
    if let JsData::Object(fields) = fields {
        put_fields(&global, &instance, &fields);
    }
    Ok(instance.as_value())
}
host_function!(class_construct, class_construct_body);

fn native_method_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(callback) = METHODS.with(|methods| methods.borrow().get(&call.callee()).cloned()) else {
        return Err(Thrown::type_error("método nativo sem implementação registrada"));
    };
    let Some(global) = global_object.to_ref() else { return Ok(JSValue::undefined()) };
    let this = read_value(&global, call.this_value(), 0);
    let arguments: Vec<JsData> = call.arguments().iter().map(|argument| read_value(&global, *argument, 0)).collect();
    match callback(&this, &arguments) {
        Ok(data) => Ok(write_value(&global, &data)),
        Err(message) => Err(Thrown::TypeError(message)),
    }
}
host_function!(native_method, native_method_body);

/// Marca `object` com a classe `name` (propriedade não enumerável [`CLASS_KEY`]).
fn mark_class(global: &JSGlobalObjectRef, object: &JSObject, name: &str) {
    let vm = global.vm();
    let value = JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(name.as_bytes())));
    let key = PropertyName::from_identifier(&Identifier::from_span(vm, CLASS_KEY.as_bytes()));
    object.put_direct(vm, &key, value, DONT_ENUM);
}

/// Grava os campos próprios enumeráveis de `fields` em `object`.
fn put_fields(global: &JSGlobalObjectRef, object: &JSObject, fields: &[(String, JsData)]) {
    let vm = global.vm();
    let values: Vec<JSValue> = fields.iter().map(|(_, item)| write_value(global, item)).collect();
    let _rooted = RootScope::new(&values);
    for ((name, _), value) in fields.iter().zip(values) {
        if name == CLASS_KEY {
            continue;
        }
        let identifier = Identifier::from_span(vm, name.as_bytes());
        let property = PropertyName::from_identifier(&identifier);
        match property.parse_index() {
            Some(index) => {
                let _ = object.put_by_index(vm, index, value, false);
            }
            None => {
                object.put_direct(vm, &property, value, 0);
            }
        }
    }
}

/// Os valores que cruzam a fronteira: o que o JavaScript e o Rust trocam. Objetos e arrays viajam por cópia (só as
/// propriedades próprias enumeráveis, com profundidade limitada); função, promessa e qualquer outra célula viajam como
/// [`JsHandle`], que o coletor não solta.
#[derive(Clone)]
pub enum JsData {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<JsData>),
    Object(Vec<(String, JsData)>),
    Handle(Rc<JsHandle>),
}

/// A profundidade máxima de uma cópia (cíclico ou absurdamente fundo vira `undefined` além dela).
const MAX_COPY_DEPTH: usize = 32;

/// Só o objeto comum e o array viajam por cópia; promessa, `Map`, `Date`, erro, tipado, função e o resto ficam como
/// [`JsHandle`] para não perder a identidade nem o comportamento.
fn copies_by_value(value: &JSValue) -> bool {
    use crate::runtime::js_type::JSType;
    match value {
        JSValue::Cell(id) => matches!(crate::runtime::cell_registry::cell_type(*id), Some(JSType::FinalObjectType | JSType::ArrayType)),
        _ => false,
    }
}

/// Um erro (`Error` e subclasses) viaja por cópia como objeto com `name`, `message` e `stack` (e a causa, se houver): é o
/// que o código do hospedeiro lê de um `catch`.
fn read_error(global: &JSGlobalObjectRef, value: JSValue, depth: usize) -> Option<JsData> {
    use crate::runtime::js_type::JSType;
    let JSValue::Cell(id) = value else { return None };
    if !matches!(crate::runtime::cell_registry::cell_type(id), Some(JSType::ErrorInstanceType)) {
        return None;
    }
    let mut fields = vec![(CLASS_KEY.to_owned(), JsData::Str("Error".to_owned()))];
    for name in ["name", "message", "stack", "cause"] {
        if let Ok(item) = get_object_property(global, value, &Identifier::from_span(global.vm(), name.as_bytes())) {
            if !item.is_undefined() {
                fields.push((name.to_owned(), read_value(global, item, depth + 1)));
            }
        }
    }
    Some(JsData::Object(fields))
}

fn read_value(global: &JSGlobalObjectRef, value: JSValue, depth: usize) -> JsData {
    if let Some(error) = read_error(global, value, depth) {
        return error;
    }
    if value.is_undefined() {
        JsData::Undefined
    } else if value.is_null() {
        JsData::Null
    } else if value.is_boolean() {
        JsData::Bool(value.as_boolean())
    } else if value.is_number() {
        JsData::Num(value.as_number())
    } else if value.is_string() {
        JsData::Str(rust_string(&value.to_wtf_string()))
    } else if copies_by_value(&value) {
        if depth >= MAX_COPY_DEPTH {
            return JsData::Undefined;
        }
        let Ok(entries) = enumerable_own_entries(global, value) else { return JsData::Undefined };
        if is_js_array(&value) {
            let mut indexed: Vec<(u32, JsData)> = entries
                .iter()
                .filter_map(|(name, item)| String::from_utf8_lossy(&name.utf8()).parse::<u32>().ok().map(|index| (index, read_value(global, *item, depth + 1))))
                .collect();
            indexed.sort_by_key(|(index, _)| *index);
            JsData::Array(indexed.into_iter().map(|(_, item)| item).collect())
        } else {
            let mut fields: Vec<(String, JsData)> = Vec::new();
            let marker = get_object_property(global, value, &Identifier::from_span(global.vm(), CLASS_KEY.as_bytes())).ok().filter(JSValue::is_string);
            if let Some(marker) = marker {
                fields.push((CLASS_KEY.to_owned(), JsData::Str(rust_string(&marker.to_wtf_string()))));
            }
            fields.extend(entries.iter().map(|(name, item)| (String::from_utf8_lossy(&name.utf8()).into_owned(), read_value(global, *item, depth + 1))));
            JsData::Object(fields)
        }
    } else {
        JsData::Handle(Rc::new(JsHandle::new(value)))
    }
}

fn write_value(global: &JSGlobalObjectRef, data: &JsData) -> JSValue {
    let vm = global.vm();
    match data {
        JsData::Undefined => JSValue::undefined(),
        JsData::Null => JSValue::null(),
        JsData::Bool(flag) => js_boolean(*flag),
        JsData::Num(number) => js_number(*number),
        JsData::Str(text) => JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes()))),
        JsData::Handle(handle) => handle.0,
        JsData::Array(items) => {
            let values: Vec<JSValue> = items.iter().map(|item| write_value(global, item)).collect();
            let _rooted = RootScope::new(&values);
            construct_array(vm, &global.array_structure(), &values).as_value()
        }
        JsData::Object(fields) => {
            let object = construct_empty_object(global);
            let class = fields.iter().find_map(|(name, item)| match item {
                JsData::Str(class) if name == CLASS_KEY => Some(class.clone()),
                _ => None,
            });
            if let Some(class) = class {
                if let Some(prototype) = CLASSES.with(|classes| classes.borrow().get(&class).map(|handle| handle.0)) {
                    object.set_prototype_direct(vm, prototype);
                }
                mark_class(global, &object, &class);
            }
            put_fields(global, &object, fields);
            object.as_value()
        }
    }
}

/// Um valor JavaScript que o hospedeiro guarda (o `Strong` da API C): o coletor não o solta enquanto o handle vive.
pub struct JsHandle(JSValue, #[allow(dead_code)] Root);

impl JsHandle {
    fn new(value: JSValue) -> JsHandle {
        JsHandle(value, Root::new(value))
    }

    /// O texto, quando o valor é uma string; `None` para outro tipo.
    pub fn as_text(&self) -> Option<String> {
        self.0.is_string().then(|| rust_string(&self.0.to_wtf_string()))
    }

    pub fn is_undefined(&self) -> bool {
        self.0.is_undefined()
    }
}

/// Um argumento de [`NodeRunner::call`].
pub enum NodeArg<'a> {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(&'a str),
    Handle(&'a JsHandle),
}

/// O programa em curso. Clonar é barato (duas contagens de referência): os closures que o hospedeiro entrega ao JavaScript
/// guardam um clone para chamar de volta.
#[derive(Clone)]
pub struct NodeRunner {
    vm: Rc<VM>,
    global_object: JSGlobalObjectRef,
}

impl NodeRunner {
    /// Avalia `source` como script clássico de nome `url` e esvazia as microtarefas. Devolve o valor de completude.
    /// `Err` é a exceção não capturada (`Nome: mensagem`).
    pub fn eval(&self, source: &str, url: &str) -> Result<JsHandle, String> {
        let result = evaluate(&self.global_object, &named_program_source(source, url)).map(JsHandle::new);
        self.vm.drain_microtasks();
        result.map_err(|exception| describe_exception(&exception))
    }

    /// Como [`NodeRunner::call_data`], mas a exceção vem como valor copiado (um erro é um objeto com `name` e `message`),
    /// não como texto: quem repassa o erro adiante não perde a mensagem original.
    pub fn call_data_throwing(&self, function: &JsHandle, args: &[JsData]) -> Result<JsData, JsData> {
        let values: Vec<JSValue> = args.iter().map(|arg| write_value(&self.global_object, arg)).collect();
        let _arguments = RootScope::new(&values);
        let result = call_function(&self.global_object, function.0, JSValue::undefined(), &values);
        let outcome = match result {
            Some(value) => Ok(read_value(&self.global_object, value, 0)),
            None => {
                let thrown = self.vm.exception().map(|exception| exception.value());
                self.vm.clear_exception();
                Err(thrown.map_or(JsData::Undefined, |value| read_value(&self.global_object, value, 0)))
            }
        };
        self.vm.drain_microtasks();
        outcome
    }

    /// Chama `function` com `args` já como [`JsData`] (cada um vira valor JavaScript) e devolve o resultado copiado.
    pub fn call_data(&self, function: &JsHandle, args: &[JsData]) -> Result<JsData, String> {
        let values: Vec<JSValue> = args.iter().map(|arg| write_value(&self.global_object, arg)).collect();
        let _arguments = RootScope::new(&values);
        let result = call_function(&self.global_object, function.0, JSValue::undefined(), &values).ok_or_else(|| {
            let message = self.vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
            self.vm.clear_exception();
            message
        });
        self.vm.drain_microtasks();
        result.map(|value| read_value(&self.global_object, value, 0))
    }

    /// Chama a função de `function` com `args` (sem `this`), esvazia as microtarefas e devolve o resultado.
    pub fn call(&self, function: &JsHandle, args: &[NodeArg]) -> Result<JsHandle, String> {
        let values: Vec<JSValue> = args
            .iter()
            .map(|arg| match arg {
                NodeArg::Undefined => JSValue::undefined(),
                NodeArg::Null => JSValue::null(),
                NodeArg::Bool(flag) => js_boolean(*flag),
                NodeArg::Num(number) => js_number(*number),
                NodeArg::Str(text) => JSValue::from_js_string(js_string(&self.vm, &WtfString::from_utf8(text.as_bytes()))),
                NodeArg::Handle(handle) => handle.0,
            })
            .collect();
        // Os argumentos já criados vivem só aqui até a chamada os pôr na pilha do JavaScript.
        let _arguments = RootScope::new(&values);
        let result = call_function(&self.global_object, function.0, JSValue::undefined(), &values).map(JsHandle::new).ok_or_else(|| {
            let message = self.vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
            self.vm.clear_exception();
            message
        });
        self.vm.drain_microtasks();
        result
    }

    /// Cria uma função JavaScript (`name`, `length`) cujo corpo é `callback`.
    pub fn make_function(&self, name: &str, length: u32, callback: NativeCallback) -> JsHandle {
        let function = JSFunction::create_native(
            &self.vm,
            &self.global_object,
            length,
            &WtfString::from_utf8(name.as_bytes()),
            native_trampoline,
            ImplementationVisibility::Public,
            crate::runtime::intrinsic::Intrinsic::NoIntrinsic,
            crate::runtime::js_function::call_host_function_as_constructor,
        );
        let value = function.as_value();
        if let JSValue::Cell(id) = value {
            NATIVES.with(|natives| natives.borrow_mut().insert(id, callback));
        }
        JsHandle::new(value)
    }

    /// Como [`NodeRunner::make_function`], mas os argumentos que são objetos chegam como [`JsData::Handle`] (a identidade, o
    /// protótipo e os métodos da classe do usuário ficam intactos) em vez de uma cópia das propriedades próprias. É o que um
    /// `registerXProvider(selector, provider)` precisa: o provedor é um objeto cujos métodos usam `this`. [`NodeRunner::export`]
    /// copia um desses handles quando só os dados interessam.
    pub fn make_function_by_reference(&self, name: &str, length: u32, callback: NativeCallback) -> JsHandle {
        let function = JSFunction::create_native(
            &self.vm,
            &self.global_object,
            length,
            &WtfString::from_utf8(name.as_bytes()),
            reference_trampoline,
            ImplementationVisibility::Public,
            crate::runtime::intrinsic::Intrinsic::NoIntrinsic,
            crate::runtime::js_function::call_host_function_as_constructor,
        );
        let value = function.as_value();
        if let JSValue::Cell(id) = value {
            REFERENCE_NATIVES.with(|natives| natives.borrow_mut().insert(id, callback));
        }
        JsHandle::new(value)
    }

    /// Define a classe `spec` e devolve o construtor (quem a quiser global usa [`NodeRunner::set_global`]; a API `vscode` a põe no módulo). As instâncias nascem pelo `new` do JavaScript (o
    /// construtor devolve os campos próprios) ou quando um [`JsData::Object`] com [`CLASS_KEY`] atravessa a fronteira.
    pub fn define_class(&self, spec: ClassSpec) -> JsHandle {
        self.define_accessor_class(spec, Vec::new())
    }

    /// Como [`NodeRunner::define_class`], com `setters`: acessores de escrita no protótipo (`set text(value)`). O corpo recebe o
    /// `this` copiado e o valor atribuído como único argumento. Um nome que está em `spec.getters` e em `setters` vira um
    /// acessor único (leitura e escrita); um nome só em `setters` é um acessor só de escrita.
    pub fn define_accessor_class(&self, spec: ClassSpec, setters: Vec<(String, MethodCallback)>) -> JsHandle {
        let vm = &self.vm;
        let global = &self.global_object;
        let constructor = JSFunction::create_native(
            vm,
            global,
            spec.length,
            &WtfString::from_utf8(spec.name.as_bytes()),
            class_call,
            ImplementationVisibility::Public,
            crate::runtime::intrinsic::Intrinsic::NoIntrinsic,
            class_construct,
        );
        let constructor_value = constructor.as_value();
        let prototype = construct_empty_object(global);
        let key = |name: &str| PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()));
        if let Some(parent) = &spec.parent {
            let parent_class = CLASSES.with(|classes| classes.borrow().get(parent).map(|handle| (handle.0, CLASS_CONSTRUCTORS.with(|c| c.borrow().get(parent).map(|h| h.0)))));
            if let Some((parent_prototype, parent_constructor)) = parent_class {
                prototype.set_prototype_direct(vm, parent_prototype);
                if let Some(parent_constructor) = parent_constructor {
                    constructor.set_prototype_direct(vm, parent_constructor);
                }
            }
        }
        prototype.put_direct(vm, &key("constructor"), constructor_value, DONT_ENUM);
        constructor.put_direct(vm, &key("prototype"), prototype.as_value(), DONT_ENUM);
        if let JSValue::Cell(id) = constructor_value {
            CONSTRUCTORS.with(|constructors| constructors.borrow_mut().insert(id, ClassEntry { name: spec.name.clone(), construct: spec.construct.clone() }));
        }
        for (name, length, callback) in &spec.methods {
            let function = JSFunction::create_native(
                vm,
                global,
                *length,
                &WtfString::from_utf8(name.as_bytes()),
                native_method,
                ImplementationVisibility::Public,
                crate::runtime::intrinsic::Intrinsic::NoIntrinsic,
                crate::runtime::js_function::call_host_function_as_constructor,
            );
            if let JSValue::Cell(id) = function.as_value() {
                METHODS.with(|methods| methods.borrow_mut().insert(id, callback.clone()));
            }
            prototype.put_direct(vm, &key(name), function.as_value(), DONT_ENUM);
        }
        // Um acessor por nome, com o getter e o setter que houver (a união dos dois conjuntos).
        let mut accessor_names: Vec<&String> = spec.getters.iter().map(|(name, _)| name).collect();
        accessor_names.extend(setters.iter().map(|(name, _)| name).filter(|name| !spec.getters.iter().any(|(getter, _)| getter == *name)));
        for name in accessor_names {
            let native_accessor = |label: &str, body: MethodCallback| {
                let function = JSFunction::create_native(
                    vm,
                    global,
                    0,
                    &WtfString::from_utf8(format!("{label} {name}").as_bytes()),
                    native_method,
                    ImplementationVisibility::Public,
                    crate::runtime::intrinsic::Intrinsic::NoIntrinsic,
                    crate::runtime::js_function::call_host_function_as_constructor,
                );
                if let JSValue::Cell(id) = function.as_value() {
                    METHODS.with(|methods| methods.borrow_mut().insert(id, body));
                }
                function.as_value()
            };
            let getter = spec.getters.iter().find(|(getter, _)| getter == name).map(|(_, callback)| {
                let callback = callback.clone();
                native_accessor("get", Rc::new(move |this: &JsData, _: &[JsData]| callback(this, &[])))
            });
            let setter = setters.iter().find(|(setter, _)| setter == name).map(|(_, callback)| native_accessor("set", callback.clone()));
            let accessor = GetterSetter::create_from_values(vm, getter.unwrap_or_else(JSValue::undefined), setter.unwrap_or_else(JSValue::undefined));
            let _ = prototype.put_direct_accessor(vm, &key(name), accessor, DONT_ENUM | ACCESSOR);
        }
        for (name, length, callback) in &spec.statics {
            let function = self.make_function(name, *length, callback.clone());
            constructor.put_direct(vm, &key(name), function.0, DONT_ENUM);
            // A função precisa sobreviver ao handle que saiu de escopo: o `JsHandle` solto tira a raiz, mas a propriedade
            // da classe a mantém viva (a classe mesma está enraizada em `CLASSES`).
        }
        let handle = JsHandle::new(constructor_value);
        CLASS_CONSTRUCTORS.with(|constructors| constructors.borrow_mut().insert(spec.name.clone(), JsHandle::new(constructor_value)));
        CLASSES.with(|classes| classes.borrow_mut().insert(spec.name.clone(), JsHandle::new(prototype.as_value())));
        handle
    }

    /// Registra `exports` como o módulo `name` do hospedeiro: `require(name)` o devolve (e `require.resolve(name)` devolve
    /// `name`), antes dos módulos embutidos do Node e de qualquer sonda de arquivo.
    pub fn register_module(&self, name: &str, exports: &JsData) {
        let handle = self.import(exports);
        HOST_MODULES.with(|modules| modules.borrow_mut().insert(name.to_owned(), handle));
    }

    /// `target[name](...args)` com `this` igual a `target`; devolve o resultado copiado. Esvazia as microtarefas.
    pub fn call_method(&self, target: &JsHandle, name: &str, args: &[JsData]) -> Result<JsData, String> {
        let vm = &self.vm;
        let method = get_object_property(&self.global_object, target.0, &Identifier::from_span(vm, name.as_bytes())).map_err(|_| {
            let message = vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
            vm.clear_exception();
            message
        })?;
        let values: Vec<JSValue> = args.iter().map(|arg| write_value(&self.global_object, arg)).collect();
        let _arguments = RootScope::new(&values);
        let result = call_function(&self.global_object, method, target.0, &values).ok_or_else(|| {
            let message = vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
            vm.clear_exception();
            message
        });
        vm.drain_microtasks();
        result.map(|value| read_value(&self.global_object, value, 0))
    }

    /// Uma promessa JavaScript nova, ainda pendente. [`NodeRunner::resolve_promise`] e [`NodeRunner::reject_promise`] a assentam.
    pub fn new_promise(&self) -> JsHandle {
        JsHandle::new(JSPromise::create(&self.vm, &self.global_object.promise_structure()).as_value())
    }

    /// Resolve `promise` (uma promessa de [`NodeRunner::new_promise`]) com `data`; só a primeira chamada vale.
    pub fn resolve_promise(&self, promise: &JsHandle, data: &JsData) -> bool {
        let Some(target) = JSPromise::from_value(&promise.0) else { return false };
        let value = write_value(&self.global_object, data);
        let _rooted = RootScope::new(&[value]);
        target.resolve(&*self.global_object, value);
        self.vm.drain_microtasks();
        true
    }

    /// Rejeita `promise` com `data` (em geral um erro); só a primeira chamada vale.
    pub fn reject_promise(&self, promise: &JsHandle, data: &JsData) -> bool {
        let Some(target) = JSPromise::from_value(&promise.0) else { return false };
        let value = write_value(&self.global_object, data);
        let _rooted = RootScope::new(&[value]);
        target.reject(&*self.global_object, value);
        self.vm.drain_microtasks();
        true
    }

    /// Converte `data` em valor JavaScript e o guarda.
    pub fn import(&self, data: &JsData) -> JsHandle {
        JsHandle::new(write_value(&self.global_object, data))
    }

    /// Copia o valor de `handle` para o lado do Rust.
    pub fn export(&self, handle: &JsHandle) -> JsData {
        read_value(&self.global_object, handle.0, 0)
    }

    /// `target[name] = data`: o membro estático de uma classe (`ThemeIcon.File`), por exemplo. Um `data` com [`CLASS_KEY`] vira
    /// instância da classe já definida; a exceção do motor volta como mensagem.
    pub fn set_property(&self, target: &JsHandle, name: &str, data: &JsData) -> Result<(), String> {
        let value = write_value(&self.global_object, data);
        let _rooted = RootScope::new(&[value]);
        set_object_property(&self.global_object, target.0, &Identifier::from_span(&self.vm, name.as_bytes()), value).map_err(|_| {
            let message = self.vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
            self.vm.clear_exception();
            message
        })
    }

    /// `globalThis[name] = data`.
    pub fn set_global(&self, name: &str, data: &JsData) {
        let value = write_value(&self.global_object, data);
        let _rooted = RootScope::new(&[value]);
        crate::runtime::native_class_support::install_global(&self.global_object, name, value);
    }

    /// Instala a ponte `__je_call(nome, json)` e o `handler` que a atende.
    pub fn set_bridge(&self, handler: Bridge) {
        BRIDGE.with(|slot| *slot.borrow_mut() = Some(handler));
        install_global_function_with_visibility(&self.global_object, "__je_call", 2, bridge_call, ImplementationVisibility::Public);
    }

    /// Roda o laço de eventos (timers, immediates, microtarefas, rede) por `budget_ms` de tempo do laço, que o
    /// hospedeiro alimenta com o tempo real decorrido. `false` quando o laço parou de vez (exceção fatal).
    pub fn run_event_loop_for(&self, budget_ms: f64) -> bool {
        crate::runtime::timers::run_event_loop_for(&self.global_object, budget_ms)
    }

    /// Só as microtarefas pendentes.
    pub fn drain_microtasks(&self) {
        self.vm.drain_microtasks();
    }
}

/// Cria o programa Node: um global novo com `console` e o sistema de arquivos `fs` dos módulos, roda `main_source`
/// (arquivo `main_url`) como programa principal (ES ou CommonJS, como o bun escolhe) e entrega o [`NodeRunner`] a `body`.
/// O programa principal que lança é `Err`; o que um timer ou uma promessa sem tratador lançam vai ao stderr do `console`
/// e NÃO derruba o programa (um host de extensões segue vivo).
pub fn run_node<T>(
    console: Rc<dyn ConsoleHost>,
    fs: Rc<dyn ModuleFs>,
    main_url: &str,
    main_source: &str,
    body: impl FnOnce(&NodeRunner) -> T,
) -> Result<T, String> {
    run_program(|| {
        let _state = ProgramState;
        crate::runtime::process_system::begin_main_script(main_url);
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        global_object.set_console_host(console);
        install_cjs_fs(Some(fs.clone()));
        global_object.set_module_fs(Some(fs));
        let shared_source: Rc<str> = Rc::from(main_source);
        crate::runtime::uncaught_report::remember_main_source(shared_source.clone());
        let report_source = shared_source.clone();
        let report = Rc::new(move |global: &JSGlobalObject, error: JSValue, with_trace: bool| {
            let mut text = Vec::new();
            crate::runtime::uncaught_report::append_exception_report(&mut text, global, &report_source, error, with_trace);
            if let Some(console) = global.console_host() {
                console.write_stderr(&[text, b"\n".to_vec()].concat());
            }
        });
        let on_error = report.clone();
        vm.set_unhandled_error_reporter(Some(Rc::new(move |global: &JSGlobalObject, error: JSValue| on_error(global, error, true))));
        let on_rejection = report.clone();
        vm.set_unhandled_rejection_reporter(Some(Rc::new(move |global: &JSGlobalObject, _promise: JSValue, reason: JSValue| {
            on_rejection(global, reason, false)
        })));
        let on_exception = report;
        vm.set_uncaught_exception_reporter(Some(Rc::new(move |global: &JSGlobalObject, exception: &Rc<crate::runtime::exception::Exception>| {
            on_exception(global, exception.value(), true)
        })));
        crate::runtime::timers::enable_event_loop();
        if let Err(exception) = run_main_program(&global_object, main_source, main_url) {
            let message = describe_exception(&exception);
            vm.clear_exception();
            return Err(message);
        }
        vm.drain_microtasks();
        let runner = NodeRunner { vm: vm.clone(), global_object };
        Ok(body(&runner))
    })
}
