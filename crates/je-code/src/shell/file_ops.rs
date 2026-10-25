//! As operações de arquivo do explorador (`explorer.newFile`, `renameFile`, `deleteFile`, `revealFileInOS`): em
//! disco, com a mensagem de erro que o shell mostra na barra de status.

use std::path::{Path, PathBuf};

fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." || name.contains('\\') {
        return Err("O nome do arquivo ou da pasta não pode ser vazio nem conter barras invertidas.".to_owned());
    }
    Ok(())
}

/// Um caminho digitado: `~` é a pasta pessoal e o relativo parte do workspace (ou da pasta atual).
pub fn resolve(text: &str, base: Option<&Path>) -> Result<PathBuf, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("O caminho está vazio.".to_owned());
    }
    let home = || std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let expanded = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => home().map(|h| h.join(rest.trim_start_matches(['/', '\\']))).ok_or("Não achei a pasta pessoal.")?,
        _ => PathBuf::from(text),
    };
    Ok(if expanded.is_absolute() { expanded } else { base.map_or_else(|| expanded.clone(), |b| b.join(&expanded)) })
}

/// Grava `content` em `path`, criando as pastas que faltam; sobrescreve o que houver (o Save As confirma antes
/// no VS Code; aqui o prompt já é a confirmação).
pub fn write_new(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))
}

/// Cria `name` (que pode ter subpastas com `/`) em `dir` e devolve o caminho; falha se já existe.
pub fn create_file(dir: &Path, name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    let path = dir.join(name);
    if path.exists() {
        return Err(format!("Já existe um arquivo ou pasta com o nome '{name}' neste local."));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn create_folder(dir: &Path, name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    let path = dir.join(name);
    if path.exists() {
        return Err(format!("Já existe um arquivo ou pasta com o nome '{name}' neste local."));
    }
    std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Renomeia `path` para `name` na mesma pasta e devolve o caminho novo.
pub fn rename(path: &Path, name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    let target = path.with_file_name(name);
    if target == path {
        return Ok(target);
    }
    if target.exists() {
        return Err(format!("Já existe um arquivo ou pasta com o nome '{name}' neste local."));
    }
    std::fs::rename(path, &target).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(target)
}

/// Se a lixeira existe (`FileSystemProviderCapabilities.Trash`): no navegador não há lixeira do sistema, e o VS Code web
/// apaga de vez, com o diálogo de exclusão permanente.
pub const TRASH_SUPPORTED: bool = cfg!(not(target_arch = "wasm32"));

/// Apaga `path`: para a lixeira do sistema quando ela existe, senão de vez (`deleteFiles` com `useTrash`).
pub fn delete(path: &Path) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        ::trash::delete(path).map_err(|e| format!("Falha ao mover '{}' para a lixeira: {e}", path.display()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let result = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
        result.map_err(|e| format!("Falha ao apagar '{}': {e}", path.display()))
    }
}

/// Mostra o item no gerenciador de arquivos do sistema (`revealFileInOS`).
pub fn reveal(path: &Path) -> Result<(), String> {
    use std::process::Command;
    let result = if cfg!(windows) {
        Command::new("explorer.exe").arg(format!("/select,{}", path.display())).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg("-R").arg(path).spawn()
    } else {
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        Command::new("xdg-open").arg(dir).spawn()
    };
    result.map(|_| ()).map_err(|e| format!("Não foi possível abrir o gerenciador de arquivos: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rename_and_refuse_duplicates() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-ops", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        let file = create_file(&dir, "a/b.txt").expect("cria com subpasta");
        assert!(file.is_file());
        assert!(create_file(&dir, "a/b.txt").is_err(), "não sobrescreve");
        let renamed = rename(&file, "c.txt").expect("renomeia");
        assert_eq!(renamed, dir.join("a/c.txt"));
        assert!(!file.exists() && renamed.is_file());
        assert!(create_folder(&dir, "").is_err());
        std::fs::remove_dir_all(&dir).expect("remove");
    }
}
