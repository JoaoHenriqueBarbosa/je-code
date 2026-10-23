//! `NativeParsedArgs` de `src/vs/platform/environment/common/argv.ts`: a lista de argumentos de linha de
//! comando que o VS Code entende.
//!
//! No original é uma interface com mais de cem campos opcionais; o `parseArgs` devolve um objeto comum que
//! só tem as chaves que apareceram na linha de comando. Aqui é um envoltório sobre o mesmo objeto dinâmico
//! (a ordem e a presença das chaves são observáveis nos testes), com leitores tipados por chave. Como o
//! objeto é compartilhado, `set` altera todas as cópias (o serviço de ambiente grava `logsPath` e
//! `continueOn` nos argumentos, como o original).

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};

/// `NativeParsedArgs`.
#[derive(Clone)]
pub struct NativeParsedArgs(ObjectRef);

impl std::fmt::Debug for NativeParsedArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.to_dynamic(), f)
    }
}

impl Default for NativeParsedArgs {
    fn default() -> Self {
        Self(ObjectRef::new())
    }
}

impl NativeParsedArgs {
    /// Os argumentos a partir do objeto que o `parseArgs` devolve.
    pub fn from_dynamic(value: &Dynamic) -> Self {
        match value {
            Dynamic::Object(object) => Self(object.clone()),
            _ => Self::default(),
        }
    }

    /// O objeto dinâmico (compartilhado, não uma cópia).
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::Object(self.0.clone())
    }

    /// `args[key]`: `Undefined` se a chave não está.
    pub fn get(&self, key: &str) -> Dynamic {
        self.0.get(&JsString::from(key)).unwrap_or(Dynamic::Undefined)
    }

    /// `args[key] = value`.
    pub fn set(&self, key: &str, value: Dynamic) {
        self.0.set(&JsString::from(key), value);
    }

    /// `key in args`.
    pub fn has(&self, key: &str) -> bool {
        self.0.has_own(&JsString::from(key))
    }

    /// Um argumento de texto (`string`), `None` se ausente (ou se não for texto).
    pub fn get_string(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        }
    }

    /// Um argumento booleano: `!!args[key]`.
    pub fn get_bool(&self, key: &str) -> bool {
        match self.get(key) {
            Dynamic::Bool(value) => value,
            Dynamic::String(text) => !text.is_empty(),
            Dynamic::Number(number) => number != 0.0 && !number.is_nan(),
            Dynamic::Undefined | Dynamic::Null => false,
            _ => true,
        }
    }

    /// Um argumento `string[]`, `None` se ausente. Um texto solto vira uma lista de um elemento.
    pub fn get_list(&self, key: &str) -> Option<Vec<String>> {
        match self.get(key) {
            Dynamic::Array(items) => Some(items.to_vec().iter().filter_map(text_of).collect()),
            Dynamic::String(text) => Some(vec![text.to_string_lossy()]),
            _ => None,
        }
    }

    /// `args._`: os argumentos posicionais.
    pub fn positional(&self) -> Vec<String> {
        self.get_list("_").unwrap_or_default()
    }
}

fn text_of(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}
