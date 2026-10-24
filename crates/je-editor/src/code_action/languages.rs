//! O recorte de `src/vs/editor/common/languages.ts` que o `codeAction` consome: `Command`, `CodeAction`,
//! `CodeActionTriggerType`, `CodeActionContext`, `CodeActionList`, `CodeActionProvider`, `ProviderId` e os tipos de
//! `WorkspaceEdit` (`IWorkspaceTextEdit`, `IWorkspaceFileEdit`, `ICustomEdit`, `WorkspaceEditMetadata`,
//! `WorkspaceFileEditOptions`).
//!
//! Representação:
//! - `ProviderResult<T>` é [`ProviderResult`] (`Promise<Option<T>>`); `ITextModel` é o `Rc<TextModel>`; `IMarkerData` é
//!   o [`MarkerData`] da plataforma e `IRange` o [`Range`].
//! - Os objetos de ação são compartilhados por referência no original: o provedor devolve o `CodeAction`, o
//!   `CodeActionItem` guarda o mesmo objeto e o `resolve` e o modelo o alteram (`action.edit = ...`,
//!   `action.diagnostics = ...`). É o [`CodeActionRef`], `Rc<RefCell<CodeAction>>`.
//! - `range: Range | Selection` do `provideCodeActions` é o [`SelectionOrRange`] do editor.
//! - `CodeActionList` é o trait [`CodeActionList`] (com `IDisposable`); o objeto literal `{ actions, dispose }` que
//!   os provedores e os testes devolvem é [`CodeActionListObject`].
//! - Os campos opcionais do TypeScript que o provedor deixa de fora (`readonly providedCodeActionKinds?`,
//!   `documentation?`, `resolveCodeAction?`, `_getAdditionalMenuItems?`) são métodos com implementação padrão que
//!   devolvem `None` ou a lista vazia; `has_resolve_code_action` e `has_additional_menu_items` fazem o papel do
//!   teste de existência (`provider.resolveCodeAction && ...`, `if (provider._getAdditionalMenuItems)`).
//! - `WorkspaceEdit.edits: Array<IWorkspaceTextEdit | IWorkspaceFileEdit | ICustomEdit>` é um `Vec` de
//!   [`WorkspaceEditItem`]; `TextEdit & { insertAsSnippet?, keepWhitespace? }` é o [`TextEdit`] deste módulo (o
//!   `TextEdit` de `languages.ts`, `{ range, text, eol? }`, com os dois campos a mais).
//! - `Command.arguments?: unknown[]` é uma lista de [`Dynamic`].
//! - `WorkspaceFileEditOptions.contents` (`Promise<VSBuffer>`) é uma `Promise<VSBuffer>`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::markers::MarkerData;

use crate::code_editor_widget::SelectionOrRange;
use crate::language_providers::ProviderResult;
use crate::model::EndOfLineSequence;
use crate::range::Range;
use crate::text_model::TextModel;

/// `Command`.
#[derive(Clone, Debug)]
pub struct Command {
    pub id: String,
    pub title: String,
    pub tooltip: Option<String>,
    pub arguments: Option<Vec<Dynamic>>,
}

impl Command {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self { id: id.into(), title: title.into(), tooltip: None, arguments: None }
    }
}

/// `ProviderId`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderId {
    pub extension_id: Option<String>,
    pub extension_version: Option<String>,
    pub provider_id: Option<String>,
}

impl ProviderId {
    pub fn new(extension_id: Option<String>, extension_version: Option<String>, provider_id: Option<String>) -> Self {
        Self { extension_id, extension_version, provider_id }
    }

    /// `ProviderId.fromExtensionId(extensionId)`.
    pub fn from_extension_id(extension_id: Option<&str>) -> Self {
        Self::new(extension_id.map(str::to_owned), None, None)
    }

    /// `toStringWithoutVersion()`.
    pub fn to_string_without_version(&self) -> String {
        let mut result = String::new();
        if let Some(extension_id) = self.extension_id.as_deref().filter(|id| !id.is_empty()) {
            result.push_str(extension_id);
        }
        if let Some(provider_id) = self.provider_id.as_deref().filter(|id| !id.is_empty()) {
            result.push(':');
            result.push_str(provider_id);
        }
        result
    }
}

impl std::fmt::Display for ProviderId {
    /// `toString()`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        if let Some(extension_id) = self.extension_id.as_deref().filter(|id| !id.is_empty()) {
            result.push_str(extension_id);
        }
        if let Some(extension_version) = self.extension_version.as_deref().filter(|version| !version.is_empty()) {
            result.push('@');
            result.push_str(extension_version);
        }
        if let Some(provider_id) = self.provider_id.as_deref().filter(|id| !id.is_empty()) {
            result.push(':');
            result.push_str(provider_id);
        }
        if result.is_empty() {
            result.push_str("unknown");
        }
        f.write_str(&result)
    }
}

/// `WorkspaceEditMetadata.iconPath`: `ThemeIcon | URI | { light: URI; dark: URI }`.
#[derive(Clone)]
pub enum IconPath {
    ThemeIcon(ThemeIcon),
    Uri(URI),
    LightDark { light: URI, dark: URI },
}

/// `WorkspaceEditMetadata`.
#[derive(Clone)]
pub struct WorkspaceEditMetadata {
    pub needs_confirmation: bool,
    pub label: String,
    pub description: Option<String>,
    pub icon_path: Option<IconPath>,
}

/// `WorkspaceFileEditOptions`.
#[derive(Clone, Default)]
pub struct WorkspaceFileEditOptions {
    pub overwrite: Option<bool>,
    pub ignore_if_not_exists: Option<bool>,
    pub ignore_if_exists: Option<bool>,
    pub recursive: Option<bool>,
    pub copy: Option<bool>,
    pub folder: Option<bool>,
    pub skip_trash_bin: Option<bool>,
    pub max_size: Option<f64>,
    pub contents: Option<Promise<VSBuffer>>,
}

/// `IWorkspaceFileEdit`.
#[derive(Clone, Default)]
pub struct IWorkspaceFileEdit {
    pub old_resource: Option<URI>,
    pub new_resource: Option<URI>,
    pub options: Option<WorkspaceFileEditOptions>,
    pub metadata: Option<WorkspaceEditMetadata>,
}

/// `TextEdit & { insertAsSnippet?: boolean; keepWhitespace?: boolean }`.
#[derive(Clone)]
pub struct TextEdit {
    pub range: Range,
    pub text: String,
    pub eol: Option<EndOfLineSequence>,
    pub insert_as_snippet: Option<bool>,
    pub keep_whitespace: Option<bool>,
}

/// `IWorkspaceTextEdit`.
#[derive(Clone)]
pub struct IWorkspaceTextEdit {
    pub resource: URI,
    pub text_edit: TextEdit,
    pub version_id: Option<i64>,
    pub metadata: Option<WorkspaceEditMetadata>,
}

/// `ICustomEdit`.
pub trait ICustomEdit {
    fn resource(&self) -> URI;
    fn metadata(&self) -> Option<WorkspaceEditMetadata> {
        None
    }
    fn undo(&self) -> Promise<()>;
    fn redo(&self) -> Promise<()>;
}

/// Um item de `WorkspaceEdit.edits`: `IWorkspaceTextEdit | IWorkspaceFileEdit | ICustomEdit`.
#[derive(Clone)]
pub enum WorkspaceEditItem {
    Text(IWorkspaceTextEdit),
    File(IWorkspaceFileEdit),
    Custom(Rc<dyn ICustomEdit>),
}

/// `WorkspaceEdit`.
#[derive(Clone, Default)]
pub struct WorkspaceEdit {
    pub edits: Vec<WorkspaceEditItem>,
}

/// `CodeAction`. O `is_synthetic` é o `_isSynthetic` do `CustomCodeAction` do host de extensões: no original o objeto da ação
/// é o próprio DTO e leva a propriedade junto; aqui é o campo que o provedor do workbench repassa.
#[derive(Clone, Default)]
pub struct CodeAction {
    pub is_synthetic: Option<bool>,
    pub title: String,
    pub command: Option<Command>,
    pub edit: Option<WorkspaceEdit>,
    pub diagnostics: Option<Vec<MarkerData>>,
    pub kind: Option<String>,
    pub is_preferred: Option<bool>,
    pub is_ai: Option<bool>,
    pub disabled: Option<String>,
    pub ranges: Option<Vec<Range>>,
}

impl CodeAction {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Self::default() }
    }

    /// O `kind` do objeto literal `{ title, kind }`.
    pub fn with_kind(title: impl Into<String>, kind: Option<&str>) -> Self {
        Self { kind: kind.map(str::to_owned), ..Self::new(title) }
    }
}

/// O objeto de ação compartilhado por referência (ver a nota do módulo).
pub type CodeActionRef = Rc<RefCell<CodeAction>>;

/// `CodeActionTriggerType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeActionTriggerType {
    Invoke = 1,
    Auto = 2,
}

/// `CodeActionContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeActionContext {
    pub only: Option<String>,
    pub trigger: CodeActionTriggerType,
}

/// `CodeActionList`.
pub trait CodeActionList: IDisposable {
    fn actions(&self) -> Vec<CodeActionRef>;
}

/// O objeto literal `{ actions, dispose }`: `dispose` roda a cada chamada, como o método do literal.
pub struct CodeActionListObject {
    actions: Vec<CodeActionRef>,
    dispose: Box<dyn Fn()>,
}

impl CodeActionListObject {
    pub fn new(actions: Vec<CodeActionRef>, dispose: impl Fn() + 'static) -> Rc<Self> {
        Rc::new(Self { actions, dispose: Box::new(dispose) })
    }
}

impl CodeActionList for CodeActionListObject {
    fn actions(&self) -> Vec<CodeActionRef> {
        self.actions.clone()
    }
}

impl IDisposable for CodeActionListObject {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// Um item de `CodeActionProvider.documentation`.
#[derive(Clone, Debug)]
pub struct CodeActionDocumentation {
    pub kind: String,
    pub command: Command,
}

/// `CodeActionProvider`.
pub trait CodeActionProvider {
    /// `displayName?`.
    fn display_name(&self) -> Option<&str> {
        None
    }

    /// `extensionId?`.
    fn extension_id(&self) -> Option<&str> {
        None
    }

    /// `provideCodeActions(model, range, context, token)`.
    fn provide_code_actions(
        &self,
        model: &Rc<TextModel>,
        range: &SelectionOrRange,
        context: &CodeActionContext,
        token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>>;

    /// Se o provedor implementa o `resolveCodeAction?` opcional.
    fn has_resolve_code_action(&self) -> bool {
        false
    }

    /// `resolveCodeAction(codeAction, token)`: só é chamado quando [`Self::has_resolve_code_action`] é verdadeiro.
    fn resolve_code_action(&self, _code_action: &CodeActionRef, _token: &CancellationToken) -> ProviderResult<CodeActionRef> {
        Promise::resolved(None)
    }

    /// `providedCodeActionKinds?`: `None` quando o provedor não declara os tipos que devolve.
    fn provided_code_action_kinds(&self) -> Option<Vec<String>> {
        None
    }

    /// `documentation?`.
    fn documentation(&self) -> Option<Vec<CodeActionDocumentation>> {
        None
    }

    /// Se o provedor implementa o `_getAdditionalMenuItems?` opcional.
    fn has_additional_menu_items(&self) -> bool {
        false
    }

    /// `_getAdditionalMenuItems(context, actions)`: só é chamado quando [`Self::has_additional_menu_items`] é
    /// verdadeiro.
    fn get_additional_menu_items(&self, _context: &CodeActionContext, _actions: &[CodeActionRef]) -> Vec<Command> {
        Vec::new()
    }
}
