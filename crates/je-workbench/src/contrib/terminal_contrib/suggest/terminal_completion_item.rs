//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/browser/terminalCompletionItem.ts`:
//! `TerminalCompletionItemKind`, `mapLspKindToTerminalKind`, `ITerminalCompletion` e
//! `TerminalCompletionItem`.
//!
//! Diferenças do original:
//! - `ITerminalCompletion extends ISimpleCompletion` repete os campos da base (o trait
//!   [`ISimpleCompletionData`] é o que o item e o modelo leem) e `TerminalCompletionItem extends
//!   SimpleCompletionItem` guarda o item base em `base` e o expõe por `Deref`/`DerefMut`.
//! - `_unresolvedItem` e `_resolveProvider` perdem o sublinhado: [`ITerminalCompletion::unresolved_item`] e
//!   [`ITerminalCompletion::resolve_provider`].
//! - `resolve` devolve a `Promise<()>` do `resolveCache` e, como no original, grava o resultado dentro da
//!   própria promessa. Como a reação é `'static`, `detail` e `documentation` do [`ITerminalCompletion`] são
//!   células compartilhadas (`Rc<RefCell<Option<_>>>`, com getters e setters): quem espera a promessa já vê o
//!   item atualizado. Clonar um `ITerminalCompletion` compartilha as células, como duas referências ao mesmo
//!   objeto.
//! - `pathSeparator?: string` é `Option<&str>`.
//! - `fileExtScores` escolhe a tabela por `cfg!(windows)` (o `isWindows` do original; o TODO de usar o SO do
//!   processo continua valendo).
//! - O `Promise.catch` do `resolve` (`catch (error) { return; }`) é o ramo de erro da reação.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::char_code as cc;
use je_base::errors::Error;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_editor::languages::{CompletionItem, CompletionItemKind, CompletionItemProvider, StringOrMarkdown};

use crate::services::suggest::simple_completion_item::{ISimpleCompletionData, SimpleCompletionItem, SimpleCompletionLabel};

/// `TerminalCompletionItemKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCompletionItemKind {
    // Tipos do host de extensões
    File = 0,
    Folder = 1,
    Method = 2,
    Alias = 3,
    Argument = 4,
    Option = 5,
    OptionValue = 6,
    Flag = 7,
    SymbolicLinkFile = 8,
    SymbolicLinkFolder = 9,
    Commit = 10,
    Branch = 11,
    Tag = 12,
    Stash = 13,
    Remote = 14,
    PullRequest = 15,
    PullRequestDone = 16,

    // Tipos só do núcleo
    InlineSuggestion = 100,
    InlineSuggestionAlwaysOnTop = 101,
}

/// Mapeia o `CompletionItemKind` de uma completion de servidor de linguagem para o
/// `TerminalCompletionItemKind`.
pub fn map_lsp_kind_to_terminal_kind(lsp_kind: CompletionItemKind) -> TerminalCompletionItemKind {
    // TODO do original: mais tipos para outros provedores LSP (https://github.com/microsoft/vscode/issues/249480)
    match lsp_kind {
        CompletionItemKind::File => TerminalCompletionItemKind::File,
        CompletionItemKind::Folder => TerminalCompletionItemKind::Folder,
        CompletionItemKind::Method => TerminalCompletionItemKind::Method,
        CompletionItemKind::Text => TerminalCompletionItemKind::Argument, // considerar um tipo novo?
        CompletionItemKind::Variable => TerminalCompletionItemKind::Argument, // idem
        CompletionItemKind::EnumMember => TerminalCompletionItemKind::OptionValue, // idem
        CompletionItemKind::Keyword => TerminalCompletionItemKind::Alias,
        _ => TerminalCompletionItemKind::Method,
    }
}

/// `ITerminalCompletion` (com os campos de `ISimpleCompletion`).
#[derive(Clone, Default)]
pub struct ITerminalCompletion {
    // `ISimpleCompletion`
    /// O rótulo da completion, que aparece à esquerda do ícone.
    pub label: SimpleCompletionLabel,
    /// O id do provedor de onde a completion veio.
    pub provider: String,
    /// O ícone à esquerda do widget de sugestões.
    pub icon: Option<ThemeIcon>,
    /// O tipo do item, incluído no rótulo de acessibilidade.
    pub kind_label: Option<String>,
    /// O detalhe, que aparece à direita da lista. É uma célula compartilhada porque `resolve` o grava de
    /// dentro da promessa; leia por [`Self::detail`] e grave por [`Self::set_detail`].
    pub detail: Rc<RefCell<Option<String>>>,
    /// Texto legível que representa um comentário de documentação (célula compartilhada, como `detail`).
    pub documentation: Rc<RefCell<Option<StringOrMarkdown>>>,
    /// Intervalo (início inclusive, fim exclusive) do texto da linha trocado ao aplicar a completion.
    pub replacement_range: Option<[i64; 2]>,

    /// Texto próprio a digitar no terminal ao escolher esta completion. Só é preciso quando o rótulo não é
    /// o que se digita.
    pub input_data: Option<String>,
    /// O tipo do item de completion do terminal.
    pub kind: Option<TerminalCompletionItemKind>,
    /// Anula a checagem de tipo e trata a completion como arquivo na hora de ordenar. Algumas completions
    /// do pwsh chegam como métodos mesmo sendo arquivos; isto garante a ordenação certa.
    pub is_file_override: Option<bool>,
    /// Se a completion é uma palavra-chave.
    pub is_keyword: Option<bool>,
    /// Item não resolvido do provedor do servidor de linguagem (`_unresolvedItem`).
    pub unresolved_item: Option<CompletionItem>,
    /// Provedor que sabe resolver este item (`_resolveProvider`).
    pub resolve_provider: Option<Rc<dyn CompletionItemProvider>>,
}

impl ITerminalCompletion {
    /// `completion.detail`.
    pub fn detail(&self) -> Option<String> {
        self.detail.borrow().clone()
    }

    /// `completion.detail = detail`.
    pub fn set_detail(&self, detail: Option<String>) {
        *self.detail.borrow_mut() = detail;
    }

    /// `completion.documentation`.
    pub fn documentation(&self) -> Option<StringOrMarkdown> {
        self.documentation.borrow().clone()
    }

    /// `completion.documentation = documentation`.
    pub fn set_documentation(&self, documentation: Option<StringOrMarkdown>) {
        *self.documentation.borrow_mut() = documentation;
    }
}

impl ISimpleCompletionData for ITerminalCompletion {
    fn label(&self) -> &SimpleCompletionLabel {
        &self.label
    }

    fn replacement_range(&self) -> Option<[i64; 2]> {
        self.replacement_range
    }
}

/// `TerminalCompletionItem`.
pub struct TerminalCompletionItem {
    /// O `SimpleCompletionItem` de que o original herda.
    pub base: SimpleCompletionItem<ITerminalCompletion>,

    /// `labelLow` sem a extensão do arquivo.
    pub label_low_exclude_file_ext: JsString,

    /// O rótulo em minúsculas; quando a completion é arquivo ou diretório, tem os separadores de caminho
    /// normalizados (`/`) no Windows e nenhum separador final nos diretórios.
    pub label_low_normalized_path: JsString,

    /// A extensão do arquivo em `labelLow`.
    pub file_ext_low: JsString,

    /// Penalidade das completions só de pontuação, ou de arquivos e pastas que começam com `_`.
    pub punctuation_penalty: u8,

    /// Os detalhes (documentação etc.) podem ser resolvidos de forma preguiçosa quando o item recebe o foco.
    pub resolve_cache: Option<Promise<()>>,
}

impl Deref for TerminalCompletionItem {
    type Target = SimpleCompletionItem<ITerminalCompletion>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for TerminalCompletionItem {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl AsRef<SimpleCompletionItem<ITerminalCompletion>> for TerminalCompletionItem {
    fn as_ref(&self) -> &SimpleCompletionItem<ITerminalCompletion> {
        &self.base
    }
}

impl AsMut<SimpleCompletionItem<ITerminalCompletion>> for TerminalCompletionItem {
    fn as_mut(&mut self) -> &mut SimpleCompletionItem<ITerminalCompletion> {
        &mut self.base
    }
}

/// `str.replaceAll(from, to)` para um único caractere.
fn replace_unit(units: &[u16], from: u16, to: u16) -> JsString {
    JsString::from_units(units.iter().map(|&unit| if unit == from { to } else { unit }).collect())
}

impl TerminalCompletionItem {
    /// `pathSeparator`: o separador usado pelo terminal. Quando informado, vale no lugar do detectado no
    /// rótulo. Isso importa em cenários remotos (por exemplo, WSL), em que o SO remoto pode usar
    /// separadores diferentes dos do SO local.
    pub fn new(completion: ITerminalCompletion, path_separator: Option<&str>) -> Self {
        let mut base = SimpleCompletionItem::new(completion);

        // Detecta o separador pelo rótulo se não veio informado. Isso garante o comportamento certo em todo
        // cenário (Windows local, Unix local, WSL, remotos SSH), usando o separador que está na completion
        // em vez de supor pelo SO local.
        let use_windows_style_path = match path_separator {
            Some(separator) => separator == "\\",
            None => base.label_low.units().contains(&cc::Backslash),
        };
        let kind = base.completion.kind;
        let completion_is_file = is_file(&base.completion);

        // garante as variantes em minúsculas (desempenho)
        let mut label_low_exclude_file_ext = base.label_low.clone();
        let mut label_low_normalized_path = base.label_low.clone();
        let mut file_ext_low = JsString::new();

        // HACK do original: trata branch como separador de caminho, senão elas são filtradas. A
        // documentação fica fixa por ora, mas o melhor seria vir por um `kind`
        // Ver https://github.com/microsoft/vscode/issues/255864
        if (completion_is_file || kind == Some(TerminalCompletionItemKind::Branch)) && use_windows_style_path {
            base.label_low = replace_unit(base.label_low.units(), cc::Slash, cc::Backslash);
        }

        if completion_is_file {
            // Não conta dotfiles como extensão ao ordenar
            let units = base.label_low.units();
            if let Some(ext_index) = units.iter().rposition(|&unit| unit == cc::Period)
                && ext_index > 0
            {
                label_low_exclude_file_ext = JsString::from(&units[..ext_index]);
                file_ext_low = JsString::from(&units[ext_index + 1..]);
            }
        }

        if completion_is_file || kind == Some(TerminalCompletionItemKind::Folder) {
            if use_windows_style_path {
                label_low_normalized_path = replace_unit(base.label_low.units(), cc::Backslash, cc::Slash);
            }
            if kind == Some(TerminalCompletionItemKind::Folder) {
                let mut units = label_low_normalized_path.units().to_vec();
                if units.last() == Some(&cc::Slash) {
                    units.pop();
                }
                label_low_normalized_path = JsString::from_units(units);
            }
        }

        let punctuation_penalty = u8::from(should_penalize_for_punctuation(&label_low_exclude_file_ext));

        Self {
            base,
            label_low_exclude_file_ext,
            label_low_normalized_path,
            file_ext_low,
            punctuation_penalty,
            resolve_cache: None,
        }
    }

    /// Resolve os detalhes do item de forma preguiçosa, quando precisa. A própria promessa grava o
    /// `detail` e a `documentation` resolvidos na completion, como no original.
    pub fn resolve(&mut self, token: &CancellationToken) -> Promise<()> {
        if let Some(cache) = &self.resolve_cache {
            return cache.clone();
        }

        let (Some(unresolved_item), Some(provider)) =
            (self.base.completion.unresolved_item.clone(), self.base.completion.resolve_provider.clone())
        else {
            return Promise::resolved(());
        };
        if !provider.has_resolve_completion_item() {
            return Promise::resolved(());
        }

        let completion_detail = self.base.completion.detail.clone();
        let completion_documentation = self.base.completion.documentation.clone();
        let cache = provider.resolve_completion_item(&unresolved_item, token).then_result(move |result| {
            // `catch (error) { return; }`: erro do provedor deixa o item como está
            if let Ok(Some(resolved)) = result {
                // Atualiza a completion com os detalhes resolvidos
                if let Some(detail) = resolved.detail
                    && !detail.is_empty()
                {
                    *completion_detail.borrow_mut() = Some(detail);
                }
                if let Some(documentation) = resolved.documentation
                    && documentation.is_truthy()
                {
                    *completion_documentation.borrow_mut() = Some(documentation);
                }
            }
            Ok::<(), Error>(())
        });
        self.resolve_cache = Some(cache.clone());
        cache
    }
}

fn is_file(completion: &ITerminalCompletion) -> bool {
    completion.kind == Some(TerminalCompletionItemKind::File) || completion.is_file_override == Some(true)
}

/// `[\[\]\{\}\(\)\.,;:!?\/\\\-_@#~*%^=$]`.
fn is_punctuation_unit(unit: u16) -> bool {
    matches!(
        u8::try_from(unit),
        Ok(b'[' | b']' | b'{' | b'}' | b'(' | b')' | b'.' | b',' | b';' | b':' | b'!' | b'?' | b'/' | b'\\' | b'-' | b'_' | b'@' | b'#' | b'~' | b'*' | b'%' | b'^' | b'=' | b'$')
    )
}

fn should_penalize_for_punctuation(label: &JsString) -> bool {
    let environment = ProcessEnvironment;
    let basename = PathModule::native(&environment).basename(&label.to_string_lossy(), None);
    basename.starts_with('_') || (!label.is_empty() && label.units().iter().all(|&unit| is_punctuation_unit(unit)))
}
