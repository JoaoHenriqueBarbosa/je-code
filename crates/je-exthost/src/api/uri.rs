//! `vscode.Uri`: a classe nativa sobre o [`URI`] de `je_base::uri` (o `uri.ts` do oráculo). A instância carrega os cinco componentes
//! (`scheme`, `authority`, `path`, `query`, `fragment`) como propriedades próprias, o `$mid` de `URI#toJSON` e a marca `$$class`; os
//! métodos reconstroem o [`URI`] a partir do `this` (`URI.revive`) e devolvem o resultado como nova instância.
//!
//! Divergências:
//! - `new vscode.Uri(...)` não existe (o construtor é `protected` no `vscode.d.ts`): o construtor nativo lança e as instâncias
//!   nascem de `Uri.file`, `Uri.parse`, `Uri.from` e `Uri.joinPath`.
//! - Os erros do `uri.ts` (`[UriError]: ...`, que o original levanta como `Error`) chegam ao JavaScript como `TypeError` com a
//!   mesma mensagem (o motor só distingue o erro do hospedeiro por mensagem).
//! - O `Uri.revive` e o `Uri.isUri` do `uri.ts` não fazem parte da API pública do `vscode.d.ts` e não estão aqui.
//! - `fsPath` é calculado a cada leitura (não há o cache `_fsPath`); o `toString()` também (sem `_formatted`).

use std::rc::Rc;

use je_base::errors;
use je_base::uri::{Field, UriChange, UriComponents, URI};
use je_base::JsString;
use je_platform::resources::host_platform;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner};

use crate::convert::{field, opt_string, string_arg, uri_from_js, uri_to_js};

/// O texto de um componente de `Uri.from`, que o `UriComponents` guarda como `JsString`.
fn component(components: &JsData, name: &str) -> Result<Option<JsString>, String> {
    match field(components, name) {
        None | Some(JsData::Undefined) | Some(JsData::Null) => Ok(None),
        Some(JsData::Str(text)) => Ok(Some(JsString::from(text.as_str()))),
        _ => Err(format!("Uri.from: {name} precisa ser texto")),
    }
}

/// Uma das alterações de `Uri#with`: `undefined` mantém, `null` limpa, texto troca.
fn change_field(change: &JsData, name: &str) -> Result<Field, String> {
    match field(change, name) {
        None | Some(JsData::Undefined) => Ok(Field::Keep),
        Some(JsData::Null) => Ok(Field::Clear),
        Some(JsData::Str(text)) => Ok(Field::from(text.as_str())),
        _ => Err(format!("Uri.with: {name} precisa ser texto ou null")),
    }
}

/// O resultado de uma operação do `uri.ts` que pode levantar (`[UriError]`), como `Result` de mensagem.
fn guarded<T>(operation: impl FnOnce() -> T) -> Result<T, String> {
    errors::catch(operation).map_err(|error| error.message)
}

/// A classe `Uri` como entrada do objeto `vscode`.
pub fn class(runner: &NodeRunner) -> (String, JsData) {
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "with".to_owned(),
            1,
            Rc::new(move |this, args| {
                let uri = uri_from_js(this)?;
                let change = args.first().unwrap_or(&JsData::Undefined);
                if matches!(change, JsData::Undefined | JsData::Null) {
                    return Ok(uri_to_js(&uri));
                }
                let change = UriChange {
                    scheme: change_field(change, "scheme")?,
                    authority: change_field(change, "authority")?,
                    path: change_field(change, "path")?,
                    query: change_field(change, "query")?,
                    fragment: change_field(change, "fragment")?,
                };
                Ok(uri_to_js(&guarded(|| uri.with(&change))?))
            }),
        ),
        (
            "toString".to_owned(),
            1,
            Rc::new(move |this, args| {
                let skip_encoding = matches!(args.first(), Some(JsData::Bool(true)));
                Ok(JsData::Str(uri_from_js(this)?.to_string(skip_encoding).to_string_lossy()))
            }),
        ),
        ("toJSON".to_owned(), 0, Rc::new(move |this, _| Ok(uri_to_js(&uri_from_js(this)?)))),
    ];
    let getters: Vec<(String, MethodCallback)> =
        vec![("fsPath".to_owned(), Rc::new(move |this, _| Ok(JsData::Str(uri_from_js(this)?.fs_path(host_platform()).to_string_lossy()))))];
    let statics: Vec<(String, u32, zjsc::api::embed_node::NativeCallback)> = vec![
        (
            "file".to_owned(),
            1,
            Rc::new(|args| {
                let path = string_arg(args, 0, "Uri.file: o caminho")?;
                Ok(uri_to_js(&guarded(|| URI::file(JsString::from(path.as_str()), host_platform()))?))
            }),
        ),
        (
            "parse".to_owned(),
            2,
            Rc::new(|args| {
                let value = string_arg(args, 0, "Uri.parse: o valor")?;
                let strict = matches!(args.get(1), Some(JsData::Bool(true)));
                Ok(uri_to_js(&guarded(|| URI::parse(JsString::from(value.as_str()), strict))?))
            }),
        ),
        (
            "from".to_owned(),
            1,
            Rc::new(|args| {
                let components = args.first().ok_or("Uri.from: faltam os componentes")?;
                let parts = UriComponents {
                    scheme: component(components, "scheme")?,
                    authority: component(components, "authority")?,
                    path: component(components, "path")?,
                    query: component(components, "query")?,
                    fragment: component(components, "fragment")?,
                };
                Ok(uri_to_js(&guarded(|| URI::from(&parts, false))?))
            }),
        ),
        (
            "joinPath".to_owned(),
            2,
            Rc::new(|args| {
                let base = uri_from_js(args.first().ok_or("Uri.joinPath: falta a base")?)?;
                let segments: Vec<String> = args[1..].iter().filter_map(|segment| opt_string(Some(segment))).collect();
                let refs: Vec<&str> = segments.iter().map(String::as_str).collect();
                Ok(uri_to_js(&guarded(|| URI::join_path(&base, &refs, host_platform()))?))
            }),
        ),
    ];
    let constructor = runner.define_class(ClassSpec {
        name: "Uri".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("Uri não tem construtor público: use Uri.file, Uri.parse, Uri.from ou Uri.joinPath".to_owned())),
        methods,
        getters,
        statics,
        parent: None,
    });
    ("Uri".to_owned(), JsData::Handle(Rc::new(constructor)))
}
