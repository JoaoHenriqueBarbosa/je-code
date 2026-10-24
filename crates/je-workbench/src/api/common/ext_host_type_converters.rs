//! Porte parcial de `src/vs/workbench/api/common/extHostTypeConverters.ts` (4 340 linhas): os conversores entre
//! as classes da API (`ext_host_types`) e os DTOs do protocolo e do editor. Os assuntos ficam em submódulos de
//! `ext_host_type_converters/`, reexportados aqui:
//!
//! - [`geometry`]: `Selection`, `Range`, `Position`, `Location`/`location` e `NotebookRange`.
//! - [`markers`]: `DiagnosticTag`, `DiagnosticSeverity`, `DiagnosticRelatedInformation`, `Diagnostic`.
//! - [`markup`]: `MarkdownString`.
//! - [`edits`]: `EndOfLine`, `TextEdit`.
//! - [`symbols`]: `SymbolKind`, `SymbolTag`, `DocumentSymbol`, `DocumentHighlight`, `CallHierarchyItem`,
//!   `CallHierarchyIncomingCall`, `CallHierarchyOutgoingCall`, `TypeHierarchyItem`.
//! - [`text_editor`]: `TextEditorLineNumbersStyle`, `ThemableDecorationAttachmentRenderOptions`,
//!   `ThemableDecorationRenderOptions`, `DecorationRangeBehavior`, `DecorationRenderOptions`,
//!   `fromRangeOrRangeWithMessage` e `pathOrURIToURI`.
//!
//! Este arquivo tem o resto: [`ICommandsConverter`] (o `Command.ICommandsConverter`), `ViewColumn` e o
//! `EditorGroupColumn`.
//!
//! Os DTOs que `je_editor` e `je_platform` já têm tipados (`IRange`, `IPosition`, `ISelection`, `IMarkerData`,
//! `languages.TextEdit`, `SymbolKind`, `DocumentSymbol`...) são esses tipos; os que ainda não existem lá, e
//! que os comandos leem de um [`je_base::dynamic::Dynamic`], têm a forma `*_dto`/`*_from_dto` (ver
//! [`geometry`]).
//!
//! AINDA NÃO PORTADO (cada item pede classes ou DTOs que ainda não existem no porte; ficam por nome do
//! `namespace` do original): `TokenType`, `DocumentSelector`, `TabSelector`, `WorkspaceEdit`
//! (depende de `IWorkspaceEditDto` e de `NotebookCellData`), `WorkspaceSymbol`, `DefinitionLink`, `Hover`,
//! `EvaluatableExpression`, `InlineValue`, `InlineValueContext`, `MultiDocumentHighlight`,
//! `CompletionTriggerKind`, `CompletionContext`, `CompletionItemTag`, `CompletionCommand`,
//! `CompletionItemKind`, `CompletionItem`, as demais famílias de linguagem (assinatura, lente, ação de
//! código, dicas embutidas, cor, dobra, intervalos de seleção, links, semântica), `ThemeIcon`/`IconPath`,
//! os de notebook (`NotebookCellData`, `NotebookData`, `NotebookCellOutput`, `NotebookKernel*`,
//! `NotebookCellStatusBarItem`...), depuração, tarefas, testes (`TestItem`, `TestRunProfile`,
//! `TestMessage`, cobertura), terminal, SCM, comentários, chat (`Chat*`, `LanguageModel*`), `Mcp*`, e os
//! de timeline, speech e configurações de IA. Entram junto do primeiro arquivo `extHost*.ts` que os pedir.
//!
//! Divergências: `EditorGroupColumn` (`workbench/services/editor/common/editorGroupColumn.ts`) e as constantes
//! `ACTIVE_GROUP` e `SIDE_GROUP` (`editorService.ts`) ainda não têm porte próprio: ficam definidos aqui, com os
//! mesmos valores do original, até esses arquivos serem portados.

use je_base::errors::{self, Error};
use je_base::lifecycle::DisposableStore;

use super::ext_host_protocol::ICommandDto;
use super::ext_host_types::{Command, ViewColumn as ApiViewColumn};

pub mod edits;
pub mod geometry;
pub mod markers;
pub mod markup;
pub mod symbols;
pub mod text_editor;

pub use edits::{EndOfLine, TextEdit};
pub use geometry::{CellRange, Location, LocationDto, NotebookRange, Position, Range, Selection};
pub use markers::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag};
pub use markup::MarkdownString;
pub use symbols::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, DocumentHighlight, DocumentSymbol, HierarchyItemDto, IncomingCallDto,
    OutgoingCallDto, SymbolKind, SymbolTag, TypeHierarchyItem,
};
pub use text_editor::{
    DecorationRangeBehavior, DecorationRenderOptions, TextEditorLineNumbersStyle, ThemableDecorationAttachmentRenderOptions,
    ThemableDecorationRenderOptions, from_range_or_range_with_message, path_or_uri_to_uri,
};

/// `Command.ICommandsConverter`: a conversão entre o `vscode.Command` da API e o `ICommandDto` do protocolo.
pub trait ICommandsConverter {
    /// `fromInternal(command)`: `None` quando o comando guardado (`$ident`) já foi descartado.
    fn from_internal(&self, command: &ICommandDto) -> Option<Command>;

    /// `toInternal(command, disposables)`: `None` para um comando ausente. O comando com argumentos fica
    /// guardado até `disposables` ser descartado. Lança (aqui, devolve o erro) se a conversão de um argumento
    /// de comando de API falha.
    fn to_internal(&self, command: Option<&Command>, disposables: &DisposableStore) -> Result<Option<ICommandDto>, Error>;
}

/// `EditorGroupColumn`.
pub type EditorGroupColumn = f64;

/// `ACTIVE_GROUP` de `editorService.ts`.
pub const ACTIVE_GROUP: EditorGroupColumn = -1.0;

/// `SIDE_GROUP` de `editorService.ts`.
pub const SIDE_GROUP: EditorGroupColumn = -2.0;

/// `typeConverters.ViewColumn`.
pub struct ViewColumn;

impl ViewColumn {
    /// `ViewColumn.from`: a coluna da API (base 1) para a coluna do grupo de editores (base 0).
    pub fn from(column: Option<f64>) -> EditorGroupColumn {
        match column {
            Some(column) if column >= ApiViewColumn::ONE => column - 1.0, // ajusta o índice zero (ViewColumn.ONE => 0)
            Some(column) if column == ApiViewColumn::BESIDE => SIDE_GROUP,
            _ => ACTIVE_GROUP, // o padrão é sempre o grupo ativo
        }
    }

    /// `ViewColumn.to`: levanta um erro para uma posição que não é de coluna.
    pub fn to(position: EditorGroupColumn) -> f64 {
        if position >= 0.0 {
            return position + 1.0; // ajusta para o índice (ViewColumn.ONE => 1)
        }

        errors::throw(Error::new("invalid 'EditorGroupColumn'"))
    }
}
