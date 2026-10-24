//! O que o runtime e o carregador de módulos precisam do sistema de arquivos do hospedeiro. Como o `ConsoleHost`
//! (`console_host.rs`), o runtime não conhece o VFS: quem hospeda o programa fornece a implementação e a instala no
//! global (`JSGlobalObject::set_module_fs`). O `fetch('file://...')` lê por aqui, na hora da leitura do corpo.

use std::rc::Rc;

use crate::runtime::js_global_object::JSGlobalObject;

/// Tipo de uma entrada do sistema de arquivos, como o `stat` a distingue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}

/// O que o carregador de módulos precisa do sistema de arquivos. Os caminhos são absolutos e
/// normalizados; qualquer falha (inexistente, sem permissão, não UTF-8) vira `None`.
pub trait ModuleFs: std::fmt::Debug {
    /// Conteúdo do arquivo como texto.
    fn read_file(&self, path: &str) -> Option<String>;
    /// Conteúdo do arquivo como bytes (o `fetch('file://...')` lê qualquer arquivo, não só texto UTF-8).
    fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.read_file(path).map(String::into_bytes)
    }
    /// Tipo da entrada, seguindo links simbólicos.
    fn stat(&self, path: &str) -> Option<EntryKind>;
    /// Caminho canônico, com os links simbólicos resolvidos.
    fn realpath(&self, path: &str) -> Option<String>;

    fn is_file(&self, path: &str) -> bool {
        self.stat(path) == Some(EntryKind::File)
    }
}

impl JSGlobalObject {
    /// Instala o sistema de arquivos do hospedeiro (`None`: o runtime não lê arquivo).
    pub fn set_module_fs(&self, fs: Option<Rc<dyn ModuleFs>>) {
        *self.module_fs.borrow_mut() = fs;
    }

    /// O sistema de arquivos instalado, se houver.
    pub fn module_fs(&self) -> Option<Rc<dyn ModuleFs>> {
        self.module_fs.borrow().clone()
    }
}
