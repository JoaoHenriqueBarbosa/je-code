//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/browser/terminalCompletionModel.ts`: o
//! `TerminalCompletionModel` e a função de comparação que ordena as completions do terminal.
//!
//! Diferenças do original:
//! - `TerminalCompletionModel extends SimpleCompletionModel<TerminalCompletionItem>` guarda o modelo base
//!   e o expõe por `Deref`/`DerefMut`.
//! - `a.localeCompare(b, undefined, { ignorePunctuation: true })` é o `Collator` do `je-base` com
//!   `ignore_punctuation`.
//! - `fileExtScores` escolhe a tabela por `cfg!(windows)` (o `isWindows` do original).
//! - `/^\s*git\b/` é [`is_git_command`], escrita à mão com as mesmas classes de caractere.

use std::ops::{Deref, DerefMut};

use je_base::JsString;
use je_base::arrays::CompareResult;
use je_base::char_code as cc;
use je_base::collator::{Collator, CollatorOptions};
use je_base::js_string::is_js_whitespace;
use je_base::strings::count;

use super::terminal_completion_item::{ITerminalCompletion, TerminalCompletionItem, TerminalCompletionItemKind as Kind};
use crate::services::suggest::simple_completion_item::SimpleCompletionLabel;
use crate::services::suggest::simple_completion_model::{LineContext, SimpleCompletionModel};

/// `TerminalCompletionModel`.
pub struct TerminalCompletionModel {
    base: SimpleCompletionModel<TerminalCompletionItem, ITerminalCompletion>,
}

impl TerminalCompletionModel {
    pub fn new(items: Vec<TerminalCompletionItem>, line_context: LineContext) -> Self {
        Self { base: SimpleCompletionModel::new(items, line_context, Some(compare_completions_fn)) }
    }
}

impl Deref for TerminalCompletionModel {
    type Target = SimpleCompletionModel<TerminalCompletionItem, ITerminalCompletion>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for TerminalCompletionModel {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

thread_local! {
    static IGNORE_PUNCTUATION_COLLATOR: Collator =
        Collator::new(None, CollatorOptions { ignore_punctuation: true, ..CollatorOptions::default() });
}

/// `a.localeCompare(b, undefined, { ignorePunctuation: true })`.
fn locale_compare_ignore_punctuation(a: &JsString, b: &JsString) -> CompareResult {
    IGNORE_PUNCTUATION_COLLATOR.with(|collator| f64::from(collator.compare(a.units(), b.units())))
}

/// `/^\s*git\b/`.
fn is_git_command(leading_line_content: &JsString) -> bool {
    let units = leading_line_content.units();
    let start = units.iter().take_while(|&&unit| is_js_whitespace(unit)).count();
    let git: [u16; 3] = [u16::from(b'g'), u16::from(b'i'), u16::from(b't')];
    let rest = &units[start..];
    if !rest.starts_with(&git) {
        return false;
    }
    // `\b` depois do `t`: o próximo caractere não é de palavra (ou não existe)
    !rest.get(3).is_some_and(|&unit| u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
}

fn has_description(label: &SimpleCompletionLabel) -> bool {
    matches!(label, SimpleCompletionLabel::Item(item) if item.description.as_deref().is_some_and(|description| !description.is_empty()))
}

/// O `?1 : 0` de uma `string` opcional: `1` quando é verdadeira (não vazia).
fn truthy(text: &Option<String>) -> f64 {
    f64::from(u8::from(text.as_deref().is_some_and(|text| !text.is_empty())))
}

fn is_resource_kind(kind: Option<Kind>) -> bool {
    matches!(kind, Some(Kind::File | Kind::Folder | Kind::SymbolicLinkFile | Kind::SymbolicLinkFolder))
}

fn is_method_or_alias(kind: Option<Kind>) -> bool {
    matches!(kind, Some(Kind::Method | Kind::Alias))
}

fn compare_completions_fn(leading_line_content: &JsString, a: &TerminalCompletionItem, b: &TerminalCompletionItem) -> CompareResult {
    let (a_kind, b_kind) = (a.completion.kind, b.completion.kind);

    // Sobe as completions inline sempre no topo
    if a_kind == Some(Kind::InlineSuggestionAlwaysOnTop) && a_kind != b_kind {
        return -1.0;
    }
    if b_kind == Some(Kind::InlineSuggestionAlwaysOnTop) && a_kind != b_kind {
        return 1.0;
    }

    // Ordena pela pontuação
    let mut score = f64::from(b.score[0] - a.score[0]);
    if score != 0.0 {
        return score;
    }

    // Sobe as completions inline
    if a_kind == Some(Kind::InlineSuggestion) && a_kind != b_kind {
        return -1.0;
    }
    if b_kind == Some(Kind::InlineSuggestion) && a_kind != b_kind {
        return 1.0;
    }

    if a.punctuation_penalty != b.punctuation_penalty {
        // Ordena pela penalidade de sublinhado (por exemplo, `__init__/` é penalizado)
        // Ordena pela penalidade de pontuação (por exemplo, `;` é penalizado)
        return f64::from(a.punctuation_penalty) - f64::from(b.punctuation_penalty);
    }

    // Ordena arquivos de mesmo nome pela extensão
    let is_arg = leading_line_content.units().contains(&cc::Space);
    if !is_arg && a_kind == Some(Kind::File) && b_kind == Some(Kind::File) {
        // Se o nome sem a extensão difere, faz a ordenação comum
        if a.label_low_exclude_file_ext != b.label_low_exclude_file_ext {
            return locale_compare_ignore_punctuation(&a.label_low_exclude_file_ext, &b.label_low_exclude_file_ext);
        }
        // Depois pelo tamanho do rótulo, crescente (sem a extensão, se for arquivo)
        score = a.label_low_exclude_file_ext.len() as f64 - b.label_low_exclude_file_ext.len() as f64;
        if score != 0.0 {
            return score;
        }
        // Se são arquivos no começo da linha de comando, sobe as extensões conforme o sistema operacional
        score = file_ext_score(&b.file_ext_low) - file_ext_score(&a.file_ext_low);
        if score != 0.0 {
            return score;
        }
        // Depois pelo tamanho da extensão, crescente
        score = a.file_ext_low.len() as f64 - b.file_ext_low.len() as f64;
        if score != 0.0 {
            return score;
        }
    }

    // Sobe os branches main e master nos comandos git
    // HACK do original: por ora só confere o conteúdo da linha, deveria conferir que a completion é um branch
    if a_kind == Some(Kind::Argument) && b_kind == Some(Kind::Argument) && is_git_command(leading_line_content) {
        let a_is_main_or_master = matches!(a.completion.label.text(), "main" | "master");
        let b_is_main_or_master = matches!(b.completion.label.text(), "main" | "master");

        if a_is_main_or_master && !b_is_main_or_master {
            return -1.0;
        }
        if b_is_main_or_master && !a_is_main_or_master {
            return 1.0;
        }
    }

    // Ordena pelas completions mais detalhadas
    if a_kind == Some(Kind::Method) && b_kind == Some(Kind::Method) {
        let (a_description, b_description) = (has_description(&a.completion.label), has_description(&b.completion.label));
        if a_description && b_description {
            score = 0.0;
        } else if a_description {
            score = -2.0;
        } else if b_description {
            score = 2.0;
        }
        let documentation = |item: &TerminalCompletionItem| item.completion.documentation().is_some_and(|documentation| documentation.is_truthy());
        score += truthy(&b.completion.detail()) + if documentation(b) { 2.0 } else { 0.0 }
            - truthy(&a.completion.detail())
            - if documentation(a) { 2.0 } else { 0.0 };
        if score != 0.0 {
            return score;
        }
    }

    // Ordena pela profundidade da pasta (por exemplo, `vscode/` vem antes de `vscode-.../`)
    if a_kind == Some(Kind::Folder) && b_kind == Some(Kind::Folder) && !a.label_low_normalized_path.is_empty() && !b.label_low_normalized_path.is_empty() {
        // Diretórios
        // Conta a profundidade do caminho (número de ocorrências de `/` ou `\`)
        let slash = [cc::Slash];
        score = count(a.label_low_normalized_path.units(), &slash) as f64 - count(b.label_low_normalized_path.units(), &slash) as f64;
        if score != 0.0 {
            return score;
        }

        // Garante que os prefixos mais curtos aparecem primeiro
        if b.label_low_normalized_path.units().starts_with(a.label_low_normalized_path.units()) {
            return -1.0; // `a` é prefixo de `b`, então `a` vem primeiro
        }
        if a.label_low_normalized_path.units().starts_with(b.label_low_normalized_path.units()) {
            return 1.0; // `b` é prefixo de `a`, então `b` vem primeiro
        }
    }

    if a_kind != b_kind {
        // Ordena pelo tipo
        if is_method_or_alias(a_kind) && !is_method_or_alias(b_kind) {
            return -1.0; // Métodos e aliases vêm primeiro
        }
        if is_method_or_alias(b_kind) && !is_method_or_alias(a_kind) {
            return 1.0; // Métodos e aliases vêm primeiro
        }
        if a_kind == Some(Kind::Argument) && b_kind != Some(Kind::Argument) {
            return -1.0; // Argumentos vêm antes dos outros tipos
        }
        if b_kind == Some(Kind::Argument) && a_kind != Some(Kind::Argument) {
            return 1.0; // Argumentos vêm antes dos outros tipos
        }
        if is_resource_kind(a_kind) && !is_resource_kind(b_kind) {
            return 1.0; // Recursos vêm por último
        }
        if is_resource_kind(b_kind) && !is_resource_kind(a_kind) {
            return -1.0; // Recursos vêm por último
        }
    }

    // Ordena em ordem alfabética; ignorar a pontuação faz os dotfiles se misturarem em vez de ficarem
    // todos no topo
    locale_compare_ignore_punctuation(&a.label_low, &b.label_low)
}

/// O `fileExtScores.get(ext) || 0`.
///
/// TODO do original: isto deveria depender do SO do processo, não do SO local.
/// Aumentos de pontuação para extensões específicas no Windows. Só vale quando o arquivo é a _primeira_
/// parte da linha de comando.
fn file_ext_score(ext: &JsString) -> f64 {
    let ext = ext.to_string_lossy();
    if cfg!(windows) {
        match ext.as_str() {
            // Windows: .ps1 > .exe > .bat > .cmd. É a precedência ao rodar os arquivos sem extensão,
            // testada à mão no pwsh v7.4.4
            "ps1" => 0.09,
            "exe" => 0.08,
            "bat" | "cmd" => 0.07,
            "msi" | "com" => 0.06,
            // Não Windows
            "sh" | "bash" | "zsh" | "fish" => -0.05,
            "csh" | "ksh" => -0.06, // C shell, Korn shell
            // Os arquivos de linguagem de script ficam de fora: o comportamento padrão no Windows só abre o
            // arquivo num editor de texto, não o executa
            _ => 0.0,
        }
    } else {
        match ext.as_str() {
            // Pwsh
            "ps1" => 0.05,
            // Windows
            "bat" | "cmd" | "exe" => -0.05,
            // Não Windows
            "sh" | "bash" | "zsh" | "fish" => 0.05,
            "csh" | "ksh" => 0.04, // C shell, Korn shell
            // Linguagens de script
            "py" | "pl" => 0.05, // Python, Perl
            _ => 0.0,
        }
    }
}
