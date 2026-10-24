//! O ponto de encaixe do transpilador de fonte: o bun reimprime todo arquivo JavaScript e TypeScript pelo impressor
//! dele antes de entregá-lo ao JSC, e remapeia as posições de `Error.stack` pelo mapa que o impressor emite. O zjsc
//! não conhece o transpilador (que vive em `wip/zbun/transpile`): o hospedeiro instala um [`SourceTranspiler`] e o
//! carregador de arquivos (o principal, os módulos ES e o `require` de CommonJS) passa o fonte por ele. O texto
//! reimpresso vira o fonte do `SourceProvider` (é o que `Function.prototype.toString` e as mensagens com trecho do
//! fonte citam) e o mapa de posições (`PositionMap::from_segments`) leva as posições de volta ao fonte original.
//!
//! Sem transpilador instalado nada muda: o fonte segue direto ao motor, como nos goldens que já guardam o texto
//! reimpresso do bun.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::position_map::{MapSegment, PositionMap};

/// O que o transpilador devolve para um arquivo.
pub struct TranspiledSource {
    /// O texto que o motor recebe. Para CommonJS é o programa inteiro, com o cabeçalho do wrapper na primeira linha.
    pub text: String,
    /// O mapa do impressor, ordenado por posição gerada.
    pub segments: Vec<MapSegment>,
    /// O transpilador classificou o arquivo como CommonJS: `text` é a função de wrapper, não um módulo ES.
    pub is_commonjs: bool,
}

/// Onde uma mensagem do transpilador aponta (os campos de `BuildMessage.position` do bun): `line` e `column` de 1 em
/// diante, `offset` em bytes do fonte e `length` os bytes que o trecho destaca.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildPosition {
    pub line_text: String,
    pub file: String,
    pub namespace: String,
    pub line: i32,
    pub column: i32,
    pub length: usize,
    pub offset: usize,
}

/// Um erro do transpilador: o texto, a posição (quando há) e as notas que o acompanham, o que o `BuildMessage` do bun expõe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildDiagnostic {
    pub message: String,
    pub position: Option<BuildPosition>,
    pub notes: Vec<BuildDiagnostic>,
}

impl BuildDiagnostic {
    /// Uma mensagem sem posição nem notas.
    pub fn plain(message: impl Into<String>) -> BuildDiagnostic {
        BuildDiagnostic { message: message.into(), position: None, notes: Vec::new() }
    }
}

/// O transpilador que o hospedeiro instala.
pub trait SourceTranspiler {
    /// Transpila o arquivo `path` (o loader sai da extensão). `None` quando o caminho não é de JavaScript nem de
    /// TypeScript (o texto segue como está); `Some(Err)` quando o fonte não parseia, com cada erro (texto, posição e notas; o carregador de módulos os relata como o bun, `BuildMessage`;
    /// os outros caminhos entregam o texto cru e o motor relata o próprio `SyntaxError`).
    /// `compile_body`: o texto é o corpo de `module._compile`, que o chamador embrulha (sem função de wrapper no
    /// texto devolvido).
    fn transpile(&self, path: &str, source: &str, compile_body: bool) -> Option<Result<TranspiledSource, Vec<BuildDiagnostic>>>;
}

thread_local! {
    static TRANSPILER: RefCell<Option<Rc<dyn SourceTranspiler>>> = const { RefCell::new(None) };
}

/// Instala (ou, com `None`, tira) o transpilador da thread: o motor roda numa thread só.
pub fn install_source_transpiler(transpiler: Option<Rc<dyn SourceTranspiler>>) {
    TRANSPILER.with(|slot| *slot.borrow_mut() = transpiler);
}

/// O resultado de [`transpile_source`]: o texto, o mapa de posições e se é CommonJS.
pub struct Transpiled {
    pub text: String,
    pub map: Rc<PositionMap>,
    pub is_commonjs: bool,
}

/// Passa `source` do arquivo `path` pelo transpilador instalado. `None` sem transpilador, para o que ele não trata e
/// para o fonte que não parseia: o chamador segue com o texto original.
pub fn transpile_source(path: &str, source: &str, compile_body: bool) -> Option<Transpiled> {
    attempt_transpile(path, source, compile_body)?.ok()
}

/// Como [`transpile_source`], mas o fonte que não parseia devolve o texto de cada erro.
pub(crate) fn attempt_transpile(path: &str, source: &str, compile_body: bool) -> Option<Result<Transpiled, Vec<BuildDiagnostic>>> {
    let transpiler = TRANSPILER.with(|slot| slot.borrow().clone())?;
    let done = match transpiler.transpile(path, source, compile_body)? {
        Ok(done) => done,
        Err(messages) => return Some(Err(messages)),
    };
    Some(Ok(Transpiled {
        text: done.text,
        map: Rc::new(PositionMap::from_segments(done.segments).with_original_source(source)),
        is_commonjs: done.is_commonjs,
    }))
}

/// Há transpilador instalado nesta thread.
pub fn has_source_transpiler() -> bool {
    TRANSPILER.with(|slot| slot.borrow().is_some())
}

/// Os módulos ES que um host de módulos passou pelo transpilador de fonte, com o mapa de cada um por chave (o loader os
/// pede depois de `fetch`). O CommonJS que um `import` puxa segue como está.
#[derive(Default)]
pub struct ReprintedModules {
    maps: RefCell<HashMap<String, Rc<PositionMap>>>,
    failures: RefCell<HashMap<String, Vec<BuildDiagnostic>>>,
    /// Os arquivos que o transpilador classificou como CommonJS, por chave: o `import` deles roda o wrapper.
    commonjs: RefCell<HashMap<String, Rc<Transpiled>>>,
}

impl ReprintedModules {
    /// O texto que o módulo `key` roda: o reimpresso (guardando o mapa) ou o original.
    pub fn reprint(&self, key: &str, text: String) -> String {
        match attempt_transpile(key, &text, false) {
            Some(Ok(done)) if !done.is_commonjs => {
                self.maps.borrow_mut().insert(key.to_owned(), done.map);
                done.text
            }
            Some(Ok(done)) => {
                self.commonjs.borrow_mut().insert(key.to_owned(), Rc::new(done));
                text
            }
            Some(Err(messages)) => {
                self.failures.borrow_mut().insert(key.to_owned(), messages);
                text
            }
            _ => text,
        }
    }

    pub fn position_map(&self, key: &str) -> Option<Rc<PositionMap>> {
        self.maps.borrow().get(key).cloned()
    }

    /// O arquivo `key` quando o transpilador o classificou como CommonJS (o `import` o executa como o bun).
    pub fn commonjs_module(&self, key: &str) -> Option<Rc<Transpiled>> {
        self.commonjs.borrow().get(key).cloned()
    }

    /// Os erros do transpilador para o módulo `key`, quando ele o recusou.
    pub fn build_errors(&self, key: &str) -> Option<Vec<BuildDiagnostic>> {
        self.failures.borrow().get(key).cloned()
    }
}
