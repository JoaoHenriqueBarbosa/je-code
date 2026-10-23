//! O `record!` define as estruturas de dados do `vs/base/common` que são lidas e escritas como JSON (as
//! interfaces de `product.ts`, `policy.ts`, `defaultAccount.ts`): campos camelCase, `Option` omitido quando
//! ausente e as chaves que a estrutura não conhece guardadas em `extra` e escritas de volta, para a leitura
//! de um arquivo de outra versão do VS Code não perder nada.
//!
//! Sintaxe: `req { campo: Tipo, ... }` são os campos obrigatórios e `opt { campo: Tipo, ... }` os opcionais
//! (o tipo do campo vira `Option<Tipo>`). Um `#[serde(rename = "...")]` num campo troca o nome da chave. As chaves
//! são camelCase; `record! { @rename "snake_case"; ... }` troca a convenção da estrutura toda.

/// As chaves desconhecidas de um objeto, na ordem do arquivo.
pub type Extra = serde_json::Map<String, serde_json::Value>;

macro_rules! record {
    (
        @rename $rename:literal;
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $( req { $( $(#[$rm:meta])* $rf:ident : $rt:ty ),* $(,)? } )?
            $( opt { $( $(#[$om:meta])* $of:ident : $ot:ty ),* $(,)? } )?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = $rename)]
        $vis struct $name {
            $($( $(#[$rm])* pub $rf: $rt, )*)?
            $($( $(#[$om])* #[serde(default, skip_serializing_if = "Option::is_none")] pub $of: Option<$ot>, )*)?
            /// As chaves que esta versão do porte não conhece.
            #[serde(flatten)]
            pub extra: $crate::record::Extra,
        }
    };
    ($($rest:tt)*) => {
        record! { @rename "camelCase"; $($rest)* }
    };
}

/// Um `number` do JavaScript lido de um JSON: guarda o `f64` e escreve um inteiro como inteiro (`3`, não
/// `3.0`), que é como o JSON de origem o escreveu.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct JsNumber(pub f64);

impl serde::Serialize for JsNumber {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let n = self.0;
        // 2^53: o limite em que o `f64` ainda representa todos os inteiros.
        if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
            serializer.serialize_i64(n as i64)
        } else {
            serializer.serialize_f64(n)
        }
    }
}

impl<'de> serde::Deserialize<'de> for JsNumber {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f64::deserialize(deserializer).map(JsNumber)
    }
}
