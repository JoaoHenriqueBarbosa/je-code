//! `navigator` do global. Não é do JavaScriptCore: é do bun. Medido no bun 1.4.2:
//!
//! - Propriedade de dados comum do global (`writable`, `enumerable`, `configurable`); na ordem de chaves vem
//!   entre `crypto` e `performance`, o que a tabela `ORDER` de `js_global_object_init.rs` já resolve.
//! - O valor é um objeto comum (protótipo `Object.prototype`, extensível, sem construtor `Navigator`: o global
//!   `Navigator` não existe) com três acessores próprios, `userAgent`, `platform` e `hardwareConcurrency`,
//!   `enumerable` e `configurable`, só com `get` (atribuir lança `TypeError: Attempted to assign to readonly
//!   property.` em modo estrito), e um `Symbol.toStringTag` próprio "Navigator" (só `configurable`). Por isso
//!   `String(navigator)` é `[object Navigator]` e `JSON.stringify(navigator)` traz as três chaves.
//! - Os `get` são funções nativas de `name` "get userAgent" (e assim por diante), `length` 0, sem `prototype`,
//!   e ignoram o `this`. Não existem `language`, `languages` nem `onLine`.
//!
//! Valores do Debian 13 x86_64 que o sandbox simula (a fonte única do porte, já que ele não tem outra):
//! - `userAgent`: `Bun/1.4.2`, a versão do oráculo.
//! - `platform`: `Linux x86_64`.
//! - `hardwareConcurrency`: [`HARDWARE_CONCURRENCY`], os 16 processadores lógicos da bancada.

use crate::host_function;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_custom_accessor_function::create_host_getter_function;
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, js_undefined, JSValue};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::property_attribute::ACCESSOR;
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::wtf_string::String as WtfString;

/// `navigator.userAgent`.
pub const USER_AGENT: &str = "Bun/1.4.2";

/// `navigator.platform`.
pub const PLATFORM: &str = "Linux x86_64";

/// `navigator.hardwareConcurrency`: o número de processadores lógicos do Debian simulado (o `nproc` da
/// bancada). Constante única do porte; quando o sandbox ganhar um `/proc/cpuinfo` próprio, esta é a ponta a ligar
/// nele.
pub const HARDWARE_CONCURRENCY: u32 = 16;

fn user_agent_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(USER_AGENT.as_bytes()))))
}

fn platform_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(PLATFORM.as_bytes()))))
}

fn hardware_concurrency_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(f64::from(HARDWARE_CONCURRENCY)))
}

host_function!(navigator_user_agent, user_agent_body);
host_function!(navigator_platform, platform_body);
host_function!(navigator_hardware_concurrency, hardware_concurrency_body);

/// Instala `navigator` no global.
pub fn install_navigator(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let navigator = construct_empty_object(global_object);
    let getters: [(&str, NativeFunction); 3] = [
        ("userAgent", navigator_user_agent as NativeFunction),
        ("platform", navigator_platform as NativeFunction),
        ("hardwareConcurrency", navigator_hardware_concurrency as NativeFunction),
    ];
    for (name, function) in getters {
        let getter = create_host_getter_function(vm, global_object, name, function, Intrinsic::NoIntrinsic);
        let accessor = GetterSetter::create_from_values(vm, getter.as_value(), js_undefined());
        let key = PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()));
        let _ = navigator.put_direct_accessor(vm, &key, accessor, ACCESSOR);
    }
    put_to_string_tag(vm, &navigator, "Navigator");
    let key = PropertyName::from_identifier(&Identifier::from_span(vm, b"navigator"));
    global_object.put_direct(vm, &key, navigator.as_value(), 0);
}
