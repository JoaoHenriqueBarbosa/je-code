//! Porte do núcleo de `src/vs/editor/browser/widget/codeEditor/codeEditorWidget.ts` (e de
//! `codeEditorContributions.ts` e `common/editorAction.ts`, de que o widget depende): a classe
//! [`CodeEditorWidget`] sem a `View`. É o que o `withTestCodeEditor` do original instancia nos testes do editor
//! (`cursor.test.ts`, `sideEditing`, `wordOperations`, `multicursor`...): modelo, `ViewModel`, cursores, edições,
//! comandos e eventos, sem DOM.
//!
//! Representação:
//! - O widget é compartilhado (`Rc<CodeEditorWidget>`, criado por `Rc::new_cyclic`) e muda de estado por trás de
//!   `&self` (`Cell`/`RefCell`), como o resto do crate. Os callbacks que no original capturam `this` guardam um
//!   `Weak` (campo `me`) e somem em silêncio se o widget já foi solto.
//! - Os serviços que o construtor recebe por injeção (`@ILanguageConfigurationService`, `@IThemeService`,
//!   `@INotificationService`, `@ICommandService`) e o que o original busca em singletons globais
//!   (`EditorExtensionsRegistry`, `runWhenWindowIdle`, `DOMLineBreaksComputerFactory`,
//!   `scheduleAtNextAnimationFrame`) chegam em [`CodeEditorWidgetServices`]. A configuração
//!   (`_createConfiguration`) chega pronta, como `Rc<dyn IEditorConfiguration>`: quem constrói o widget cria o
//!   `EditorConfiguration` com `isSimpleWidget`, `contextMenuId` e as opções, e o widget passa a ser o dono dela
//!   (registra o `dispose`).
//! - `EditorExtensionsRegistry` (`editorExtensions.ts`, porte em [`crate::editor_extensions`]) chega por
//!   injeção. O widget só consulta o registro por três perguntas, que são o trait
//!   [`IEditorExtensionsRegistry`]: as contribuições (`getEditorContributions`), as ações (`getEditorActions`)
//!   e o comando de editor de um `handlerId` (`getEditorCommand`). `EditorAction`/`EditorCommand` viram o
//!   [`EditorActionDescriptor`] e o trait [`IEditorCommand`]. O `ServicesAccessor` que o original recebe do
//!   `invokeFunction` do `_instantiationService` é `Option<&ServicesAccessor>` e o widget, que não tem
//!   serviço de instanciação, passa sempre `None`.
//! - O `payload: unknown` de `trigger` é [`Dynamic`]. As cargas dos handlers internos (`TypePayload`...) têm
//!   struct tipada com `to_dynamic`, e o widget as lê do `Dynamic` como o original lê o `Partial<...>` (cada
//!   campo ausente vale o `||` do original: `''`, `0`, `false`, `null`).
//! - `string | null | undefined` de `source` é `Option<&str>`; `source: string = 'api'` (parâmetro com padrão) é
//!   `Option<&str>` em que `None` vale `"api"`; `scrollType = ScrollType.Smooth` e afins são `Option<ScrollType>`.
//!   `IPosition`/`IRange`/`ISelection` são `Position`/`Range`/`Selection` (os checks de `Position.isIPosition`,
//!   `Range.isIRange`, `typeof n !== 'number'` e `throw new Error('Invalid arguments')` que o tipo já garante
//!   não existem; a exceção é `setSelections` com lista vazia, que lança).
//! - O texto digitado, colado e o valor do modelo são [`JsString`] (UTF-16, como o `ViewModel`).
//! - `setSelection(range | selection)` (as quatro sobrecargas) é [`SelectionOrRange`], com `From` dos dois.
//! - `executeEdits(source: string | TextModelEditSource, edits, endCursorState?: ICursorStateComputer |
//!   Selection[])` é [`ExecuteEditsSource`] e [`EndCursorState`].
//! - `InteractionEmitter` (um `Emitter` que chama `contributions.onBeforeInteractionEvent()` antes de cada `fire`)
//!   é o método privado `fire_interaction`, usado em todo disparo dos emitters que o original cria como
//!   `InteractionEmitter`: `onDidAttemptReadOnlyEdit`, `onWillType`, `onDidType`, `onDidCompositionStart`,
//!   `onDidCompositionEnd` e `onDidPaste`.
//! - `BooleanEventEmitter` está como [`BooleanEventEmitter`]. `ModelData` é privado.
//! - O foco (`View.isFocused`, `View.isWidgetFocused`) é o estado `has_text_focus`/`has_widget_focus` do widget,
//!   com os setters [`CodeEditorWidget::set_has_text_focus`] e [`CodeEditorWidget::set_has_widget_focus`]
//!   (a futura `View` os chamará). Os dois só valem com modelo ligado, e `focus()` marca os dois (foco no texto
//!   implica foco no widget).
//! - `IEditorContribution` é [`IEditorContribution`]: `saveViewState`/`restoreViewState` opcionais são os métodos
//!   com padrão e o `typeof x.saveViewState === 'function'` é [`IEditorContribution::has_view_state`].
//!   `getContribution<T>(id)` devolve `Rc<dyn IEditorContribution>`; a busca por tipo (`X.get(editor)`) usa
//!   [`IEditorContribution::as_any`].
//! - `createDecorationsCollection` devolve a [`EditorDecorationsCollection`], que guarda o editor como `Weak`.
//! - `ICodeEditor` (a interface pública) é o trait [`ICodeEditor`], que o widget implementa: os métodos públicos
//!   do original que só conferem o modelo e repassam ao `ViewModel` (ou ao `TextModel`) ficam como corpo da
//!   implementação do trait, sem função de repasse na struct. Os métodos genéricos têm `where Self: Sized` para o
//!   trait continuar usável como `dyn ICodeEditor`. Os eventos (`onDid...`) são métodos da struct (gerados por
//!   uma macro a partir de uma lista só, que também cria os emitters).
//! - `try { ... } finally { this._endUpdate() }` é o guarda `UpdateScope`: o `end` roda ao sair do escopo.
//!   (No `trigger` o `fire` de `onWillTriggerEditorOperationEvent` acontece antes do `begin`, para um listener que
//!   lança não deixar o contador negativo, o que o `finally` do original faria.)
//! - `_attachModel`: `viewModel.onEvent` e `model.onWillDispose` registram ouvintes que o `ModelData` descarta em
//!   `dispose`. O `IBatchableTarget` do `ViewModel` é o [`WidgetBatchTarget`] (`_beginUpdate`/`_endUpdate`).
//! - `restoreViewState`: o original sai cedo se não há `View` real (`hasRealView`), e com `View` chama
//!   `view.restoreState`. Aqui o que a `View` faria é feito direto: `viewLayout.setScrollPosition(..., Immediate)`
//!   seguido de `viewModel.visibleLinesStabilized()` (o corpo de `View.restoreState`).
//! - `IPasteEvent.clipboardEvent` (`ClipboardEvent` do DOM) não existe: [`IPasteEvent`] traz só `range` e
//!   `languageId`. O mesmo vale para o campo `clipboardEvent` de `PastePayload`.
//! - `ICursorStateComputer` é o `&mut dyn FnMut(&[ValidEditOperation]) -> Option<Vec<Selection>>` do
//!   `ViewModel::execute_edits`.
//!
//! Divergências (fora do escopo deste porte, por nome de membro do original):
//! - A `View` e o DOM: `_createView`, `View`, `ModelData.view`/`hasRealView` (sempre sem `View` real),
//!   `layout`, `render`, `renderAsync`, `getDomNode`, `getContainerDomNode`, `getOverflowWidgetsDomNode`,
//!   `onVisible`, `onHide` (os dois só chamam `view.refreshFocusState`), `writeScreenReaderContent`,
//!   `applyFontInfo`, `setBanner`, `setAriaOptions`, `createOverviewRuler`, `delegateVerticalScrollbarPointerDown`,
//!   `delegateScrollFromMouseWheelEvent`, `changeViewZones`, `getTargetAtClientPoint`, `getScrolledVisiblePosition`,
//!   `hasPendingScrollAnimation` (o `ViewLayout`
//!   ainda não o expõe), `focus` de DOM (ver acima), os atributos `data-mode-id` e `data-uri`, a variável CSS
//!   `--editor-font-size`, o `DragAndDropObserver` com `showDropIndicatorAt`/`removeDropIndicator` e
//!   `dropIntoEditorDecorationOptions`. `getOffsetForColumn`, `getWidthOfLine` e `resetLineWidthCaches` existem
//!   e valem o que o original devolve sem `View` real (`-1`, `-1` e nada).
//! - Widgets de conteúdo e de overlay: `addContentWidget`, `layoutContentWidget`, `removeContentWidget`,
//!   `addOverlayWidget`, `layoutOverlayWidget` e `removeOverlayWidget` existem e guardam os widgets e a última
//!   posição de cada um (`_contentWidgets`, `_overlayWidgets`); a chamada à `View` (`hasRealView`) não existe. Os
//!   tipos estão em `editor_browser` e não têm `getDomNode()`.
//! - Widgets de margem de glifos: `addGlyphMarginWidget`, `layoutGlyphMarginWidget`, `removeGlyphMarginWidget`.
//! - Eventos que só a `View` produz: `onMouseUp`, `onMouseDown`, `onMouseDrag`, `onMouseDrop`,
//!   `onMouseDropCanceled`, `onContextMenu`, `onMouseWheel`,
//!   `onWillCopy`, `onWillCut`, `onWillPaste`, `onDropIntoEditor`. `onMouseMove`, `onMouseLeave`, `onKeyDown` e
//!   `onKeyUp` existem, e quem tem a `View` os dispara por `fire_mouse_move`, `fire_mouse_leave`, `fire_key_down` e
//!   `fire_key_up`.
//! - `ICodeEditorService`: `willCreateCodeEditor`, `addCodeEditor` e `removeCodeEditor` chegam pelo
//!   [`ICodeEditorRegistry`] opcional de [`CodeEditorWidgetServices`] (o construtor e o `dispose` os chamam, como o
//!   original).
//! - Serviços sem porte aqui: `ICodeEditorService` (os tipos de decoração `setDecorationsByType`, `setDecorationsByTypeFast`, `removeDecorationsByType`,
//!   `_registerDecorationType`, `_removeDecorationType`, `_removeDecorationTypes`, `_resolveDecorationOptions`),
//!   `IInstantiationService` (`invokeWithinContext`, o filho com o `IContextKeyService`),
//!   `IContextKeyService` (`contextKeyService`, `setContextValue`, `contextKeyValues`, `EditorContextKeysManager`,
//!   `EditorModeContext`), `IAccessibilityService`, `ILanguageFeaturesService`, `IUserInteractionService` e
//!   `TabFocus`. A precondição de uma ação (`ContextKeyExpression`) é a função de [`EditorPrecondition`].
//! - O tema do `registerThemingParticipant` (regras CSS dos rabiscos e `getSquigglySVGData`,
//!   `getDotDotDotSVGData`): é CSS.
//! - `IDiffEditorModel`/`IDiffEditorViewModel` em `setModel`: o widget só recebe `ITextModel`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::RunWhenIdle;
use je_base::dynamic::{Dynamic, Truthy, number_to_js_string};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, EmitterOptions, Event, EventDeliveryQueue};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::scrollable::{INewScrollPosition, ScheduleAtNextAnimationFrame};
use je_base::severity::Severity;
use je_base::uri::URI;
use je_instantiation::{InstantiationService, ServicesAccessor};
use je_platform::actions::MenuId;
use je_platform::commands::{CommandMetadata, ICommandService};
use je_platform::notification::notification_message::{NotificationPart, NotificationText};
use je_platform::notification::{
    BasePromptChoice, INotificationService, NotificationMessage, PromptChoice, PromptChoiceKind,
};

use crate::attached_views::AttachedView;
use crate::config::editor_options::layout_info::{EditorLayoutInfo, LAYOUT_INFO};
use crate::config::editor_options::registry::{MULTI_CURSOR_LIMIT, READ_ONLY, WORD_SEPARATORS};
use crate::config::editor_options::word_segmenter_locales::WORD_SEGMENTER_LOCALES;
use crate::config::editor_options::{ComputedEditorOptions, ConfigurationChangedEvent, Opt};
use crate::config::editor_configuration::IEditorConfiguration;
use crate::cursor::SavedCursorState;
use crate::cursor_columns;
use crate::editor_decorations_collection::EditorDecorationsCollection;
use crate::cursor_common::CursorSimpleModel;
use crate::cursor_events::{CursorChangeReason, ICursorPositionChangedEvent, ICursorSelectionChangedEvent};
use crate::cursor_word_operations::WordOperations;
use crate::editor_browser::{ContentWidgetPosition, EditorMouseEvent, IContentWidget, IOverlayWidget, OverlayWidgetPosition, PartialEditorMouseEvent};
use crate::edit_operation::SingleEditOperation;
use crate::edits::text_edit::TextEdit;
use crate::editor_common::{Command, ScrollType};
use crate::model::{EndOfLinePreference, ModelDecoration, ModelDeltaDecoration, ValidEditOperation};
use crate::model_line_projection_data::LineBreaksComputerFactory;
use crate::monospace_line_breaks_computer::MonospaceLineBreaksComputerFactory;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::services::abstract_code_editor_service::ICodeEditorRegistry;
use crate::text_model::{ModelDecorationsChangeAccessor, TextModel, TextSource};
use crate::text_model_edit_source::{EditSources, TextModelEditSource};
use crate::text_model_events as model_events;
use crate::view_model_decorations::{filter_font_decorations, filter_validation_decorations};
use crate::view_model_event_dispatcher::{ContentSizeChangedEvent, OutgoingViewModelEvent, ScrollChangedEvent};
use crate::view_model_impl::{HiddenAreasSource, IThemeService, ViewModel};
use crate::view_model_impl_api::{IViewState, ReducedRestoreState};
use crate::view_model_impl_support::IBatchableTarget;
use crate::view_model_types::{IEditorWhitespace, IViewLayout, IViewModel, VerticalRevealType};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::word_helper::WordAtPosition;

/// `editorCommon.EditorType.ICodeEditor`.
pub const EDITOR_TYPE_ICODE_EDITOR: &str = "vs.editor.ICodeEditor";

/// `IScrollEvent` (`editorCommon.ts`): o `ScrollChangedEvent` do `ViewModel` tem os mesmos campos públicos.
pub type IScrollEvent = ScrollChangedEvent;

/// `IContentSizeChangedEvent` (`editorCommon.ts`): o `ContentSizeChangedEvent` do `ViewModel` tem os mesmos
/// campos públicos.
pub type IContentSizeChangedEvent = ContentSizeChangedEvent;

/// O `ViewModel` que o widget cria: o alvo transacional é o [`WidgetBatchTarget`].
pub type EditorViewModel = ViewModel<WidgetBatchTarget>;

thread_local! {
    /// `let EDITOR_ID = 0`.
    static EDITOR_ID: Cell<i32> = const { Cell::new(0) };
}

/// `++EDITOR_ID`.
fn next_editor_id() -> i32 {
    EDITOR_ID.with(|id| {
        id.set(id.get() + 1);
        id.get()
    })
}

// #region editorCommon: handlers, payloads, eventos e interfaces

/// `editorCommon.Handler`: os comandos embutidos de `trigger`.
pub struct Handler;

impl Handler {
    pub const COMPOSITION_START: &'static str = "compositionStart";
    pub const COMPOSITION_END: &'static str = "compositionEnd";
    pub const TYPE: &'static str = "type";
    pub const REPLACE_PREVIOUS_CHAR: &'static str = "replacePreviousChar";
    pub const COMPOSITION_TYPE: &'static str = "compositionType";
    pub const PASTE: &'static str = "paste";
    pub const CUT: &'static str = "cut";
}

/// `editorCommon.TypePayload`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypePayload {
    pub text: JsString,
}

impl TypePayload {
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([("text", Dynamic::from(self.text.clone()))])
    }
}

/// `editorCommon.ReplacePreviousCharPayload`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplacePreviousCharPayload {
    pub text: JsString,
    pub replace_char_cnt: i32,
}

impl ReplacePreviousCharPayload {
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("text", Dynamic::from(self.text.clone())),
            ("replaceCharCnt", Dynamic::from(self.replace_char_cnt)),
        ])
    }
}

/// `editorCommon.CompositionTypePayload`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompositionTypePayload {
    pub text: JsString,
    pub replace_prev_char_cnt: i32,
    pub replace_next_char_cnt: i32,
    pub position_delta: i32,
}

impl CompositionTypePayload {
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("text", Dynamic::from(self.text.clone())),
            ("replacePrevCharCnt", Dynamic::from(self.replace_prev_char_cnt)),
            ("replaceNextCharCnt", Dynamic::from(self.replace_next_char_cnt)),
            ("positionDelta", Dynamic::from(self.position_delta)),
        ])
    }
}

/// `editorBrowser.PastePayload` (sem o `clipboardEvent`, que é do DOM).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PastePayload {
    pub text: JsString,
    pub paste_on_new_line: bool,
    pub multicursor_text: Option<Vec<JsString>>,
    pub mode: Option<String>,
    pub is_block: bool,
}

impl PastePayload {
    pub fn to_dynamic(&self) -> Dynamic {
        let multicursor_text = match &self.multicursor_text {
            Some(lines) => Dynamic::array(lines.iter().cloned().map(Dynamic::from)),
            None => Dynamic::Null,
        };
        let mode = match &self.mode {
            Some(mode) => Dynamic::from(mode.as_str()),
            None => Dynamic::Null,
        };
        Dynamic::object([
            ("text", Dynamic::from(self.text.clone())),
            ("pasteOnNewLine", Dynamic::from(self.paste_on_new_line)),
            ("multicursorText", multicursor_text),
            ("mode", mode),
            ("isBlock", Dynamic::from(self.is_block)),
        ])
    }
}

/// `payload || {}`: o objeto vazio no lugar de um valor falso.
pub fn payload_or_empty(payload: Option<Dynamic>) -> Dynamic {
    match payload {
        Some(payload) if payload.is_truthy() => payload,
        _ => Dynamic::object(std::iter::empty::<(&str, Dynamic)>()),
    }
}

/// `args.<key> || ''`.
pub fn payload_text(payload: &Dynamic, key: &str) -> JsString {
    match payload.get_str(key) {
        Dynamic::String(text) => text,
        _ => JsString::default(),
    }
}

/// `args.<key> || 0`.
pub fn payload_count(payload: &Dynamic, key: &str) -> i32 {
    match payload.get_str(key) {
        Dynamic::Number(n) if !n.is_nan() => n as i32,
        _ => 0,
    }
}

/// `args.<key> || false`.
pub fn payload_flag(payload: &Dynamic, key: &str) -> bool {
    payload.get_str(key).is_truthy()
}

/// `args.multicursorText || null`.
pub fn payload_lines(payload: &Dynamic, key: &str) -> Option<Vec<JsString>> {
    match payload.get_str(key) {
        Dynamic::Array(lines) => Some(
            lines
                .to_vec()
                .into_iter()
                .map(|line| match line {
                    Dynamic::String(text) => text,
                    _ => JsString::default(),
                })
                .collect(),
        ),
        _ => None,
    }
}

/// `args.mode || null`.
fn payload_optional_text(payload: &Dynamic, key: &str) -> Option<String> {
    match payload.get_str(key) {
        Dynamic::String(text) if !text.is_empty() => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `editorCommon.IModelChangedEvent`: o evento de `editor.setModel()`.
#[derive(Clone)]
pub struct IModelChangedEvent {
    /// O `uri` do modelo anterior, ou `None`.
    pub old_model_url: Option<URI>,
    /// O `uri` do modelo novo, ou `None`.
    pub new_model_url: Option<URI>,
}

/// `editorCommon.ITriggerEditorOperationEvent`.
#[derive(Clone)]
pub struct ITriggerEditorOperationEvent {
    pub source: Option<String>,
    pub handler_id: String,
    pub payload: Dynamic,
}

/// O `{ source: string | undefined }` de `onBeforeExecuteEdit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeforeExecuteEditEvent {
    pub source: Option<String>,
}

/// `editorBrowser.IPasteEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IPasteEvent {
    pub range: Range,
    pub language_id: Option<String>,
}

/// `editorCommon.ICodeEditorViewState`: o estado serializável do editor.
#[derive(Clone)]
pub struct ICodeEditorViewState {
    pub cursor_state: Vec<SavedCursorState>,
    pub view_state: IViewState,
    pub contributions_state: HashMap<String, Dynamic>,
}

/// `editorCommon.IEditorAction`.
pub trait IEditorAction {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    fn alias(&self) -> &str;
    fn metadata(&self) -> Option<&CommandMetadata>;
    fn is_supported(&self) -> bool;
    fn run(&self, args: Option<&Dynamic>) -> Promise<()>;
}

/// `editorCommon.IEditorContribution`: criada junto com cada editor e descartada com ele. O `Any` como supertrait
/// permite o `getContribution<T>(id)` tipado do original ([`get_contribution_as`]): o `Rc<dyn IEditorContribution>` sobe
/// para `Rc<dyn Any>` e desce para o tipo concreto.
pub trait IEditorContribution: IDisposable + std::any::Any {
    /// `typeof contribution.saveViewState === 'function'` (e o mesmo para `restoreViewState`).
    fn has_view_state(&self) -> bool {
        false
    }

    /// `saveViewState()`.
    fn save_view_state(&self) -> Dynamic {
        Dynamic::Undefined
    }

    /// O `T` de `getContribution<T>(id)`: a contribuição como [`Any`], para a busca por tipo
    /// (`MultiCursorSelectionController.get(editor)`) recuperar o tipo concreto. Quem não é procurado por tipo não
    /// implementa.
    fn as_any(&self) -> Option<&dyn Any> {
        None
    }

    /// `restoreViewState(state)`.
    fn restore_view_state(&self, _state: &Dynamic) {}
}

/// `SelectionOrRange`: as quatro sobrecargas de `setSelection` (`IRange`, `Range`, `ISelection`, `Selection`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionOrRange {
    Selection(Selection),
    Range(Range),
}

impl From<Selection> for SelectionOrRange {
    fn from(selection: Selection) -> Self {
        SelectionOrRange::Selection(selection)
    }
}

impl From<Range> for SelectionOrRange {
    fn from(range: Range) -> Self {
        SelectionOrRange::Range(range)
    }
}

/// O `options` de `getValue(options)`: `{ preserveBOM, lineEnding }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GetValueOptions {
    pub preserve_bom: bool,
    pub line_ending: Option<String>,
}

/// O `source` de `executeEdits`: `string | null | undefined | TextModelEditSource`.
pub enum ExecuteEditsSource<'a> {
    Name(Option<&'a str>),
    Reason(TextModelEditSource),
}

/// O `endCursorState` de `executeEdits`: `ICursorStateComputer | Selection[]`.
pub enum EndCursorState<'a> {
    Selections(Vec<Selection>),
    Computer(&'a mut dyn FnMut(&[ValidEditOperation]) -> Option<Vec<Selection>>),
}

// #endregion

// #region Registro de extensões, contribuições e ações (injetados)

/// `EditorContributionInstantiation` (`editorExtensions.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorContributionInstantiation {
    /// Criada ao instanciar o editor. Só as eager participam de `saveViewState`/`restoreViewState`.
    Eager = 0,
    /// Criada no máximo 50 ms depois do primeiro render (ou antes, se houver tempo ocioso).
    AfterFirstRender = 1,
    /// Criada no máximo antes do primeiro evento de mouse ou teclado (ou antes, se houver tempo ocioso).
    BeforeFirstInteraction = 2,
    /// Criada só quando há tempo ocioso (com prazo de 5 s).
    Eventually = 3,
    /// Criada só quando pedida de forma explícita por `getContribution`.
    Lazy = 4,
}

/// `EditorContributionCtor`: cria a contribuição para um editor (o `createInstance(desc.ctor, editor)`). Um pânico
/// do construtor é capturado e vai para `onUnexpectedError`, como a exceção do original.
pub type EditorContributionCtor = Rc<dyn Fn(&Rc<CodeEditorWidget>) -> Rc<dyn IEditorContribution>>;

/// `IEditorContributionDescription`.
#[derive(Clone)]
pub struct EditorContributionDescription {
    pub id: String,
    pub ctor: EditorContributionCtor,
    pub instantiation: EditorContributionInstantiation,
}

/// `EditorCommand`: o comando de editor do registro (`runEditorCommand(accessor, editor, args)`). O `accessor`
/// é `Option` porque o original aceita `null` (os testes chamam `runEditorCommand(null, editor, null)`) e o
/// widget não tem serviço de instanciação para criá-lo. Um comando síncrono devolve uma promessa já resolvida.
pub trait IEditorCommand {
    fn run_editor_command(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()>;
}

/// A precondição de uma ação, no lugar da `ContextKeyExpression` avaliada pelo `IContextKeyService`
/// (`contextMatchesRules`): sem precondição a ação é suportada.
pub type EditorPrecondition = Rc<dyn Fn(&CodeEditorWidget) -> bool>;

/// `EditorAction` do registro (o que `getEditorActions` devolve): os campos que o widget lê para montar a
/// [`InternalEditorAction`] mais o comando que a executa.
#[derive(Clone)]
pub struct EditorActionDescriptor {
    pub id: String,
    pub label: String,
    pub alias: String,
    pub metadata: Option<CommandMetadata>,
    pub precondition: Option<EditorPrecondition>,
    pub command: Rc<dyn IEditorCommand>,
}

/// O que o widget pergunta ao `EditorExtensionsRegistry`.
pub trait IEditorExtensionsRegistry {
    /// `getEditorContributions()`.
    fn get_editor_contributions(&self) -> Vec<EditorContributionDescription>;
    /// `getEditorActions()`.
    fn get_editor_actions(&self) -> Vec<EditorActionDescriptor>;
    /// `getEditorCommand(commandId)`.
    fn get_editor_command(&self, command_id: &str) -> Option<Rc<dyn IEditorCommand>>;
}

/// `InternalEditorAction` (`editorAction.ts`).
struct InternalEditorAction {
    id: String,
    label: String,
    alias: String,
    metadata: Option<CommandMetadata>,
    precondition: Option<EditorPrecondition>,
    command: Rc<dyn IEditorCommand>,
    /// O editor dono da ação (no original, a ação captura `this` e o `IContextKeyService` dele).
    editor: Weak<CodeEditorWidget>,
}

impl IEditorAction for InternalEditorAction {
    fn id(&self) -> &str {
        &self.id
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn alias(&self) -> &str {
        &self.alias
    }

    fn metadata(&self) -> Option<&CommandMetadata> {
        self.metadata.as_ref()
    }

    fn is_supported(&self) -> bool {
        match &self.precondition {
            None => true,
            Some(precondition) => self.editor.upgrade().is_some_and(|editor| precondition(&editor)),
        }
    }

    fn run(&self, args: Option<&Dynamic>) -> Promise<()> {
        if !self.is_supported() {
            return Promise::resolved(());
        }
        let Some(editor) = self.editor.upgrade() else {
            return Promise::resolved(());
        };
        let undefined = Dynamic::Undefined;
        editor.run_command_with_services(&self.command, args.unwrap_or(&undefined))
    }
}

/// `CodeEditorContributions` (`codeEditorContributions.ts`).
pub struct CodeEditorContributions {
    base: Rc<Disposable>,
    editor: RefCell<Option<Weak<CodeEditorWidget>>>,
    /// Todas as contribuições já instanciadas.
    instances: Rc<DisposableMap<String, dyn IEditorContribution>>,
    /// As ainda não instanciadas, na ordem de inserção (um `Map`).
    pending: RefCell<Vec<EditorContributionDescription>>,
    /// Quais tipos de instanciação já foram feitos (indexado por [`EditorContributionInstantiation`]).
    finished_instantiation: [Cell<bool>; 4],
}

impl CodeEditorContributions {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let instances: Rc<DisposableMap<String, dyn IEditorContribution>> = DisposableMap::new();
        base.register(instances.clone());
        Rc::new(Self {
            base,
            editor: RefCell::new(None),
            instances,
            pending: RefCell::new(Vec::new()),
            finished_instantiation: Default::default(),
        })
    }

    /// `initialize(editor, contributions, instantiationService)`: o `runWhenWindowIdle(window, ...)` é o
    /// `run_when_idle` injetado.
    pub fn initialize(
        self: &Rc<Self>,
        editor: &Rc<CodeEditorWidget>,
        contributions: Vec<EditorContributionDescription>,
        run_when_idle: &RunWhenIdle,
    ) {
        *self.editor.borrow_mut() = Some(Rc::downgrade(editor));

        for desc in contributions {
            if self.pending.borrow().iter().any(|pending| pending.id == desc.id) {
                errors::on_unexpected_error(&Error::new(format!("Cannot have two contributions with the same id {}", desc.id)));
                continue;
            }
            self.pending.borrow_mut().push(desc);
        }

        self.instantiate_some(EditorContributionInstantiation::Eager);

        // AfterFirstRender
        // - these extensions will be instantiated at the latest 50ms after the first render.
        // - but if there is idle time, we will instantiate them sooner.
        // BeforeFirstInteraction
        // - these extensions will be instantiated at the latest before a mouse or a keyboard event.
        // - but if there is idle time, we will instantiate them sooner.
        // Eventually
        // - these extensions will only be instantiated when there is idle time.
        // - since there is no guarantee that there will ever be idle time, we set a timeout of 5s here.
        for (instantiation, timeout) in [
            (EditorContributionInstantiation::AfterFirstRender, None),
            (EditorContributionInstantiation::BeforeFirstInteraction, None),
            (EditorContributionInstantiation::Eventually, Some(5000)),
        ] {
            let me = Rc::downgrade(self);
            self.base.register(run_when_idle(
                Box::new(move |_deadline| {
                    if let Some(contributions) = me.upgrade() {
                        contributions.instantiate_some(instantiation);
                    }
                }),
                timeout,
            ));
        }
    }

    /// `saveViewState()`.
    pub fn save_view_state(&self) -> HashMap<String, Dynamic> {
        let mut contributions_state = HashMap::new();
        for id in self.instances.keys() {
            if let Some(contribution) = self.instances.get(&id)
                && contribution.has_view_state()
            {
                contributions_state.insert(id, contribution.save_view_state());
            }
        }
        contributions_state
    }

    /// `restoreViewState(contributionsState)`: uma contribuição sem estado salvo recebe `undefined`.
    pub fn restore_view_state(&self, contributions_state: &HashMap<String, Dynamic>) {
        for id in self.instances.keys() {
            if let Some(contribution) = self.instances.get(&id)
                && contribution.has_view_state()
            {
                contribution.restore_view_state(contributions_state.get(&id).unwrap_or(&Dynamic::Undefined));
            }
        }
    }

    /// `get(id)`: instancia a contribuição pendente, se for o caso.
    pub fn get(&self, id: &str) -> Option<Rc<dyn IEditorContribution>> {
        self.instantiate_by_id(id);
        self.instances.get(&id.to_owned())
    }

    /// `set(id, value)`: usado por testes.
    pub fn set(&self, id: &str, value: Rc<dyn IEditorContribution>) {
        self.instances.set(id.to_owned(), value, false);
    }

    /// `onBeforeInteractionEvent()`: chamado com muita frequência pelo editor.
    pub fn on_before_interaction_event(&self) {
        self.instantiate_some(EditorContributionInstantiation::BeforeFirstInteraction);
    }

    /// `onAfterModelAttached()`.
    pub fn on_after_model_attached(self: &Rc<Self>, run_when_idle: &RunWhenIdle) -> DisposableRef {
        let me = Rc::downgrade(self);
        run_when_idle(
            Box::new(move |_deadline| {
                if let Some(contributions) = me.upgrade() {
                    contributions.instantiate_some(EditorContributionInstantiation::AfterFirstRender);
                }
            }),
            Some(50),
        )
    }

    fn instantiate_some(&self, instantiation: EditorContributionInstantiation) {
        let finished = &self.finished_instantiation[instantiation as usize];
        if finished.get() {
            // already done with this instantiation!
            return;
        }
        finished.set(true);

        let ids: Vec<String> = self
            .pending
            .borrow()
            .iter()
            .filter(|desc| desc.instantiation == instantiation)
            .map(|desc| desc.id.clone())
            .collect();
        for id in ids {
            self.instantiate_by_id(&id);
        }
    }

    fn instantiate_by_id(&self, id: &str) {
        let desc = {
            let mut pending = self.pending.borrow_mut();
            let Some(index) = pending.iter().position(|desc| desc.id == id) else {
                return;
            };
            pending.remove(index)
        };

        let editor = self.editor.borrow().as_ref().and_then(Weak::upgrade);
        let Some(editor) = editor else {
            errors::throw(Error::new("Cannot instantiate contributions before being initialized!"));
        };

        match errors::catch(|| (desc.ctor)(&editor)) {
            Ok(instance) => {
                self.instances.set(desc.id.clone(), instance.clone(), false);
                if instance.has_view_state() && desc.instantiation != EditorContributionInstantiation::Eager {
                    eprintln!(
                        "Editor contribution '{}' should be eager instantiated because it uses saveViewState / restoreViewState.",
                        desc.id
                    );
                }
            }
            Err(error) => errors::on_unexpected_error(&error),
        }
    }
}

impl IDisposable for CodeEditorContributions {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// #endregion

// #region Emitters

#[derive(Clone, Copy, PartialEq, Eq)]
enum BooleanEventValue {
    NotSet,
    False,
    True,
}

/// `BooleanEventEmitter`: dispara `onDidChangeToTrue`/`onDidChangeToFalse` só quando o valor muda.
pub struct BooleanEventEmitter {
    base: Rc<Disposable>,
    on_did_change_to_true: Rc<Emitter<()>>,
    on_did_change_to_false: Rc<Emitter<()>>,
    value: Cell<BooleanEventValue>,
}

impl BooleanEventEmitter {
    /// `new BooleanEventEmitter({ deliveryQueue })`.
    pub fn new(delivery_queue: Option<EventDeliveryQueue>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_to_true = new_emitter(&base, delivery_queue.as_ref());
        let on_did_change_to_false = new_emitter(&base, delivery_queue.as_ref());
        Rc::new(Self { base, on_did_change_to_true, on_did_change_to_false, value: Cell::new(BooleanEventValue::NotSet) })
    }

    pub fn on_did_change_to_true(&self) -> Event<()> {
        self.on_did_change_to_true.event()
    }

    pub fn on_did_change_to_false(&self) -> Event<()> {
        self.on_did_change_to_false.event()
    }

    pub fn set_value(&self, value: bool) {
        let value = if value { BooleanEventValue::True } else { BooleanEventValue::False };
        if self.value.get() == value {
            return;
        }
        self.value.set(value);
        match value {
            BooleanEventValue::True => self.on_did_change_to_true.fire(()),
            BooleanEventValue::False => self.on_did_change_to_false.fire(()),
            BooleanEventValue::NotSet => {}
        }
    }
}

impl IDisposable for BooleanEventEmitter {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `new Emitter<T>(options)` registrado no `Disposable` dono (`this._register(...)`).
fn new_emitter<T: 'static>(base: &Disposable, delivery_queue: Option<&EventDeliveryQueue>) -> Rc<Emitter<T>> {
    let emitter = Emitter::with_options(EmitterOptions { delivery_queue: delivery_queue.cloned(), ..EmitterOptions::default() });
    base.register(emitter.clone());
    emitter
}

/// Os emitters com `deliveryQueue` entram na fila compartilhada (`queued`), os demais não (`direct`).
macro_rules! emitter_queue {
    (queued, $queue:expr) => {
        Some($queue)
    };
    (direct, $queue:expr) => {
        None
    };
}

/// Cria, de uma lista só, a struct com os emitters do widget e os métodos `onDid...` que devolvem os eventos.
macro_rules! editor_emitters {
    ($($field:ident, $getter:ident, $ty:ty, $kind:ident, $doc:literal;)*) => {
        struct EditorEmitters {
            $($field: Rc<Emitter<$ty>>,)*
        }

        impl EditorEmitters {
            fn new(base: &Disposable, queue: &EventDeliveryQueue) -> Self {
                Self { $($field: new_emitter(base, emitter_queue!($kind, queue)),)* }
            }
        }

        impl CodeEditorWidget {
            $(
                #[doc = $doc]
                pub fn $getter(&self) -> Event<$ty> {
                    self.emitters.$field.event()
                }
            )*
        }
    };
}

editor_emitters! {
    did_dispose, on_did_dispose, (), direct, "`onDidDispose`.";
    did_change_model_content, on_did_change_model_content, model_events::ModelContentChangedEvent, queued, "`onDidChangeModelContent`.";
    did_change_model_language, on_did_change_model_language, model_events::ModelLanguageChangedEvent, queued, "`onDidChangeModelLanguage`.";
    did_change_model_language_configuration, on_did_change_model_language_configuration, model_events::ModelLanguageConfigurationChangedEvent, queued, "`onDidChangeModelLanguageConfiguration`.";
    did_change_model_options, on_did_change_model_options, model_events::ModelOptionsChangedEvent, queued, "`onDidChangeModelOptions`.";
    did_change_model_decorations, on_did_change_model_decorations, model_events::ModelDecorationsChangedEvent, queued, "`onDidChangeModelDecorations`.";
    did_change_line_height, on_did_change_line_height, model_events::ModelLineHeightChangedEvent, queued, "`onDidChangeLineHeight`.";
    did_change_font, on_did_change_font, model_events::ModelFontChangedEvent, queued, "`onDidChangeFont`.";
    did_change_model_tokens, on_did_change_model_tokens, model_events::ModelTokensChangedEvent, queued, "`onDidChangeModelTokens`.";
    did_change_configuration, on_did_change_configuration, ConfigurationChangedEvent, queued, "`onDidChangeConfiguration`.";
    will_change_model, on_will_change_model, IModelChangedEvent, queued, "`onWillChangeModel`.";
    did_change_model, on_did_change_model, IModelChangedEvent, queued, "`onDidChangeModel`.";
    did_change_cursor_position, on_did_change_cursor_position, ICursorPositionChangedEvent, queued, "`onDidChangeCursorPosition`.";
    did_change_cursor_selection, on_did_change_cursor_selection, ICursorSelectionChangedEvent, queued, "`onDidChangeCursorSelection`.";
    did_attempt_read_only_edit, on_did_attempt_read_only_edit, (), queued, "`onDidAttemptReadOnlyEdit`.";
    did_layout_change, on_did_layout_change, EditorLayoutInfo, queued, "`onDidLayoutChange`.";
    will_type, on_will_type, JsString, queued, "`onWillType`.";
    did_type, on_did_type, JsString, queued, "`onDidType`.";
    did_composition_start, on_did_composition_start, (), queued, "`onDidCompositionStart`.";
    did_composition_end, on_did_composition_end, (), queued, "`onDidCompositionEnd`.";
    did_paste, on_did_paste, IPasteEvent, queued, "`onDidPaste`.";
    did_content_size_change, on_did_content_size_change, IContentSizeChangedEvent, queued, "`onDidContentSizeChange`.";
    did_scroll_change, on_did_scroll_change, IScrollEvent, queued, "`onDidScrollChange`.";
    did_change_view_zones, on_did_change_view_zones, (), queued, "`onDidChangeViewZones`.";
    did_change_hidden_areas, on_did_change_hidden_areas, (), queued, "`onDidChangeHiddenAreas`.";
    will_trigger_editor_operation_event, on_will_trigger_editor_operation_event, ITriggerEditorOperationEvent, direct, "`onWillTriggerEditorOperationEvent`.";
    begin_update, on_begin_update, (), direct, "`onBeginUpdate`.";
    end_update, on_end_update, (), direct, "`onEndUpdate`.";
    before_execute_edit, on_before_execute_edit, BeforeExecuteEditEvent, direct, "`onBeforeExecuteEdit`.";
    key_down, on_key_down, je_platform::keybinding::keybinding::KeyboardEvent, direct, "`onKeyDown`: quem tem a `View` (ou o host nativo que a substitui) entrega as teclas por `fire_key_down`.";
    key_up, on_key_up, je_platform::keybinding::keybinding::KeyboardEvent, direct, "`onKeyUp`: quem tem a `View` (ou o host nativo que a substitui) entrega as teclas por `fire_key_up`.";
    mouse_move, on_mouse_move, EditorMouseEvent, direct, "`onMouseMove`: quem tem a `View` (ou o host nativo que a substitui) entrega o evento por `fire_mouse_move`.";
    mouse_leave, on_mouse_leave, PartialEditorMouseEvent, direct, "`onMouseLeave`: quem tem a `View` (ou o host nativo que a substitui) entrega o evento por `fire_mouse_leave`.";
}

impl CodeEditorWidget {
    /// Entrega um movimento do mouse aos ouvintes de `onMouseMove` (o `View` do original dispara o evento).
    pub fn fire_mouse_move(&self, event: EditorMouseEvent) {
        self.emitters.mouse_move.fire(event);
    }

    /// Entrega a saída do mouse aos ouvintes de `onMouseLeave` (o `View` do original dispara o evento).
    pub fn fire_mouse_leave(&self, event: PartialEditorMouseEvent) {
        self.emitters.mouse_leave.fire(event);
    }
}

impl CodeEditorWidget {
    /// Entrega uma tecla pressionada aos ouvintes de `onKeyDown` (o `View` do original dispara o evento).
    pub fn fire_key_down(&self, event: je_platform::keybinding::keybinding::KeyboardEvent) {
        self.emitters.key_down.fire(event);
    }

    /// Entrega uma tecla solta aos ouvintes de `onKeyUp` (o `View` do original dispara o evento).
    pub fn fire_key_up(&self, event: je_platform::keybinding::keybinding::KeyboardEvent) {
        self.emitters.key_up.fire(event);
    }

    /// `_onKeyDown._size`: quantos ouvintes de `onKeyDown` estão registrados (os testes medem vazamento por aqui).
    pub fn key_down_listener_count(&self) -> usize {
        self.emitters.key_down.size_for_testing()
    }
}

impl CodeEditorWidget {
    /// `onDidFocusEditorText` (`_editorTextFocus.onDidChangeToTrue`).
    pub fn on_did_focus_editor_text(&self) -> Event<()> {
        self.editor_text_focus.on_did_change_to_true()
    }

    /// `onDidBlurEditorText` (`_editorTextFocus.onDidChangeToFalse`).
    pub fn on_did_blur_editor_text(&self) -> Event<()> {
        self.editor_text_focus.on_did_change_to_false()
    }

    /// `onDidFocusEditorWidget` (`_editorWidgetFocus.onDidChangeToTrue`).
    pub fn on_did_focus_editor_widget(&self) -> Event<()> {
        self.editor_widget_focus.on_did_change_to_true()
    }

    /// `onDidBlurEditorWidget` (`_editorWidgetFocus.onDidChangeToFalse`).
    pub fn on_did_blur_editor_widget(&self) -> Event<()> {
        self.editor_widget_focus.on_did_change_to_false()
    }
}

// #endregion



// #region Modelo e visão ligados

/// O `IBatchableTarget` do `ViewModel`: o `batchChanges` do original chama `_beginUpdate` e `_endUpdate`.
pub struct WidgetBatchTarget(Weak<CodeEditorWidget>);

impl IBatchableTarget for WidgetBatchTarget {
    fn batch_changes<T>(&self, cb: impl FnOnce() -> T) -> T {
        match self.0.upgrade() {
            Some(editor) => {
                let _scope = editor.update_scope();
                cb()
            }
            None => cb(),
        }
    }
}

/// `try { this._beginUpdate(); ... } finally { this._endUpdate(); }`: o `end` roda ao sair do escopo.
struct UpdateScope<'a>(&'a CodeEditorWidget);

impl Drop for UpdateScope<'_> {
    fn drop(&mut self) {
        self.0.end_update();
    }
}

/// `ModelData`: o que está logicamente associado a um modelo. Sem `View` (ver "Divergências").
struct ModelData {
    model: Rc<TextModel>,
    view_model: Rc<EditorViewModel>,
    listeners_to_remove: Vec<DisposableRef>,
    attached_view: Rc<AttachedView>,
}

impl ModelData {
    fn dispose(&self) {
        for listener in &self.listeners_to_remove {
            listener.dispose();
        }
        self.model.on_before_detached(&self.attached_view);
        self.view_model.dispose();
    }
}

// #endregion

/// O que o construtor do `CodeEditorWidget` recebe do mundo de fora (ver o topo do módulo).
pub struct CodeEditorWidgetServices {
    /// `@ILanguageConfigurationService`.
    pub language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    /// `@IThemeService`.
    pub theme_service: Rc<dyn IThemeService>,
    /// `@INotificationService`.
    pub notification_service: Rc<dyn INotificationService>,
    /// `@ICommandService`.
    pub command_service: Rc<dyn ICommandService>,
    /// `EditorExtensionsRegistry`.
    pub extensions_registry: Rc<dyn IEditorExtensionsRegistry>,
    /// `DOMLineBreaksComputerFactory.create(window)`: o medidor de quebra de linha da quebra avançada.
    pub dom_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory>,
    /// `dom.scheduleAtNextAnimationFrame(window, callback)`.
    pub schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame,
    /// `runWhenWindowIdle(window, callback, timeout)`.
    pub run_when_idle: RunWhenIdle,
    /// `@ICodeEditorService`, só no que o widget chama (`willCreateCodeEditor`, `addCodeEditor`,
    /// `removeCodeEditor`). O original sempre o injeta; `None` é o widget sem registro de editores.
    pub code_editor_service: Option<Rc<dyn ICodeEditorRegistry>>,
}

/// `ICodeEditorWidgetOptions` (sem `isSimpleWidget`, `contextMenuId` e `contextKeyValues`, que pertencem à
/// configuração e ao serviço de chaves de contexto).
#[derive(Default)]
pub struct CodeEditorWidgetOptions {
    /// As contribuições a instanciar. Com `Some`, só elas; com `None`, as do registro.
    pub contributions: Option<Vec<EditorContributionDescription>>,
    /// Dados de telemetria associados ao widget.
    pub telemetry_data: Option<Dynamic>,
}

/// `IContentWidgetData`: o widget de conteúdo e a última posição que ele deu.
#[derive(Clone)]
pub struct ContentWidgetData {
    pub widget: Rc<dyn IContentWidget>,
    pub position: Option<ContentWidgetPosition>,
}

/// `IOverlayWidgetData`: o widget de overlay e a última posição que ele deu.
#[derive(Clone)]
pub struct OverlayWidgetData {
    pub widget: Rc<dyn IOverlayWidget>,
    pub position: Option<OverlayWidgetPosition>,
}

/// `CodeEditorWidget`.
pub struct CodeEditorWidget {
    me: Weak<CodeEditorWidget>,
    base: Rc<Disposable>,
    id: i32,
    configuration: Rc<dyn IEditorConfiguration>,
    services: CodeEditorWidgetServices,
    telemetry_data: Option<Dynamic>,
    emitters: EditorEmitters,
    editor_text_focus: Rc<BooleanEventEmitter>,
    editor_widget_focus: Rc<BooleanEventEmitter>,
    /// `View.isFocused()`.
    has_text_focus: Cell<bool>,
    /// `View.isWidgetFocused()`.
    has_widget_focus: Cell<bool>,
    contributions: Rc<CodeEditorContributions>,
    contributions_disposable: RefCell<Option<DisposableRef>>,
    /// `_actions`, na ordem de inserção (um `Map`).
    actions: RefCell<Vec<(String, Rc<dyn IEditorAction>)>>,
    /// `_modelData`: o `Rc` é clonado para fora do `RefCell` antes de qualquer chamada, porque os eventos
    /// reentram (`model.onWillDispose` chama `setModel(null)`).
    model_data: RefCell<Option<Rc<ModelData>>>,
    update_counter: Cell<i32>,
    in_composition: Cell<bool>,
    /// O serviço de instanciação com que os comandos de editor rodam (o `invokeFunction` do original); sem ele
    /// o `accessor` chega como `None`.
    instantiation_service: RefCell<Option<Rc<InstantiationService>>>,
    /// `_contentWidgets`, na ordem de inserção (as chaves de um objeto): o par é `(widget.getId(), dados)`.
    content_widgets: RefCell<Vec<(String, ContentWidgetData)>>,
    /// `_overlayWidgets`, na mesma forma.
    overlay_widgets: RefCell<Vec<(String, OverlayWidgetData)>>,
}

impl CodeEditorWidget {
    /// Instala o serviço de instanciação que dá o `ServicesAccessor` aos comandos e ações do editor. Os comandos
    /// que exigem o `accessor` (apagar palavra, aparar espaços) não rodam sem ele.
    pub fn set_instantiation_service(&self, instantiation_service: Rc<InstantiationService>) {
        *self.instantiation_service.borrow_mut() = Some(instantiation_service);
    }

    /// `instantiationService.invokeFunction((accessor) => command.runEditorCommand(accessor, this, args))`.
    fn run_command_with_services(&self, command: &Rc<dyn IEditorCommand>, args: &Dynamic) -> Promise<()> {
        let Some(editor) = self.me.upgrade() else {
            return Promise::resolved(());
        };
        let service = self.instantiation_service.borrow().clone();
        match service {
            Some(service) => service.invoke_function(|accessor| command.run_editor_command(Some(accessor), &editor, args)),
            None => command.run_editor_command(None, &editor, args),
        }
    }

    /// O construtor: `configuration` é a `_configuration` já criada (ver o topo do módulo).
    pub fn new(
        configuration: Rc<dyn IEditorConfiguration>,
        options: CodeEditorWidgetOptions,
        services: CodeEditorWidgetServices,
    ) -> Rc<Self> {
        if let Some(code_editor_service) = &services.code_editor_service {
            code_editor_service.will_create_code_editor();
        }
        let base = Disposable::new();
        let delivery_queue = EventDeliveryQueue::new();
        let contributions = CodeEditorContributions::new();
        base.register(contributions.clone());
        let emitters = EditorEmitters::new(&base, &delivery_queue);
        let editor_text_focus = BooleanEventEmitter::new(Some(delivery_queue.clone()));
        let editor_widget_focus = BooleanEventEmitter::new(Some(delivery_queue));
        base.register(editor_text_focus.clone());
        base.register(editor_widget_focus.clone());

        let configuration_disposable: DisposableRef = configuration.clone();
        base.register(configuration_disposable);

        let CodeEditorWidgetOptions { contributions: contributions_option, telemetry_data } = options;
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            id: next_editor_id(),
            configuration,
            services,
            telemetry_data,
            emitters,
            editor_text_focus,
            editor_widget_focus,
            has_text_focus: Cell::new(false),
            has_widget_focus: Cell::new(false),
            contributions,
            contributions_disposable: RefCell::new(None),
            actions: RefCell::new(Vec::new()),
            model_data: RefCell::new(None),
            update_counter: Cell::new(0),
            in_composition: Cell::new(false),
            instantiation_service: RefCell::new(None),
            content_widgets: RefCell::new(Vec::new()),
            overlay_widgets: RefCell::new(Vec::new()),
        });

        this.register_listener(this.configuration.on_did_change(), |editor, e| {
            editor.emitters.did_change_configuration.fire(e.clone());

            if e.has_changed(LAYOUT_INFO.id()) {
                let layout_info = *editor.configuration.options().get(LAYOUT_INFO);
                editor.emitters.did_layout_change.fire(layout_info);
            }
        });

        let contributions = match contributions_option {
            Some(contributions) => contributions,
            None => this.services.extensions_registry.get_editor_contributions(),
        };
        this.contributions.initialize(&this, contributions, &this.services.run_when_idle);

        for descriptor in this.services.extensions_registry.get_editor_actions() {
            if this.actions.borrow().iter().any(|(id, _)| *id == descriptor.id) {
                errors::on_unexpected_error(&Error::new(format!("Cannot have two actions with the same id {}", descriptor.id)));
                continue;
            }
            let id = descriptor.id.clone();
            let action: Rc<dyn IEditorAction> = Rc::new(InternalEditorAction {
                id: descriptor.id,
                label: descriptor.label,
                alias: descriptor.alias,
                metadata: descriptor.metadata,
                precondition: descriptor.precondition,
                command: descriptor.command,
                editor: this.me.clone(),
            });
            this.actions.borrow_mut().push((id, action));
        }

        if let Some(code_editor_service) = &this.services.code_editor_service {
            code_editor_service.add_code_editor(this.clone());
        }

        this
    }

    /// O `Rc` do próprio editor: o `this` do original, que o JavaScript passa adiante por referência (o `BulkEditService`
    /// o recebe como `ICodeEditor` do `IEditorService`).
    pub fn rc(&self) -> Rc<CodeEditorWidget> {
        self.me.upgrade().expect("o editor está vivo enquanto um método dele roda")
    }

    /// `this._register(event(listener))`, com o callback recebendo o widget (o `this` do original) em vez de
    /// capturá-lo: o `Weak` sobe a cada chamada, e a chamada some se o widget já foi solto.
    fn register_listener<E: 'static>(&self, event: Event<E>, handler: impl Fn(&CodeEditorWidget, &E) + 'static) {
        let me = self.me.clone();
        self.base.register(event.listen(move |e| {
            if let Some(editor) = me.upgrade() {
                handler(&editor, e);
            }
        }));
    }

    /// `isSimpleWidget`.
    pub fn is_simple_widget(&self) -> bool {
        self.configuration.is_simple_widget()
    }

    /// O `ICodeEditorService` injetado (`_codeEditorService`), se houver.
    pub fn code_editor_service(&self) -> Option<Rc<dyn ICodeEditorRegistry>> {
        self.services.code_editor_service.clone()
    }

    /// `contextMenuId`.
    pub fn context_menu_id(&self) -> MenuId {
        self.configuration.context_menu_id()
    }

    /// `getTelemetryData()`.
    pub fn get_telemetry_data(&self) -> Option<&Dynamic> {
        self.telemetry_data.as_ref()
    }

    /// `_contributions` (protegido no original): a subclasse de teste (`TestCodeEditor`) registra contribuições
    /// por ele (`registerAndInstantiateContribution`).
    pub fn contributions(&self) -> &Rc<CodeEditorContributions> {
        &self.contributions
    }

    /// `inComposition`.
    pub fn in_composition(&self) -> bool {
        self.in_composition.get()
    }

    /// `createDecorationsCollection(decorations?)`: a coleção é [`EditorDecorationsCollection`] (módulo próprio).
    pub fn create_decorations_collection(&self, decorations: Option<Vec<ModelDeltaDecoration>>) -> EditorDecorationsCollection {
        let editor = self.me.upgrade().expect("o editor está vivo enquanto cria a coleção de decorações");
        EditorDecorationsCollection::new(&editor, decorations.as_deref())
    }

    /// `_getViewModel()`: o `ViewModel` do modelo atual.
    pub fn get_view_model(&self) -> Option<Rc<EditorViewModel>> {
        self.model_data().map(|model_data| model_data.view_model.clone())
    }

    /// `handleInitialized()`.
    pub fn handle_initialized(&self) {
        if let Some(view_model) = self.get_view_model() {
            view_model.visible_lines_stabilized();
        }
    }

    /// `getConfiguredWordAtPosition(position)`.
    pub fn get_configured_word_at_position(&self, position: Position) -> Option<WordAtPosition> {
        let model_data = self.model_data()?;
        let options = self.configuration.options();
        let word_separators: Vec<u16> = options.get(WORD_SEPARATORS).encode_utf16().collect();
        WordOperations::get_word_at_position(&model_data.model, &word_separators, options.get(WORD_SEGMENTER_LOCALES), position)
    }

    /// O `@ILanguageConfigurationService` que o construtor recebeu. Os comandos de contribuição que o original lê do
    /// `accessor` (`accessor.get(ILanguageConfigurationService)`) usam este, porque o serviço ainda não tem
    /// identificador no porte (ver "Divergências" de `editor_extensions`).
    pub fn language_configuration_service(&self) -> Rc<dyn ILanguageConfigurationService> {
        self.services.language_configuration_service.clone()
    }

    /// `edit(edit, reason)`.
    pub fn edit(&self, edit: &TextEdit, reason: TextModelEditSource) -> bool {
        let edits = edit
            .replacements
            .iter()
            .map(|replacement| SingleEditOperation::new(replacement.range, Some(replacement.text.clone()), false))
            .collect();
        self.execute_edits(ExecuteEditsSource::Reason(reason), edits, None)
    }

    // #region Foco (o estado que a View dava)

    /// Define o foco do texto (o `View` chama `viewModel.setHasFocus`). Sem modelo não faz nada. O
    /// `FocusChanged` que o `ViewModel` emite atualiza o estado e o `BooleanEventEmitter`.
    pub fn set_has_text_focus(&self, has_focus: bool) {
        if let Some(model_data) = self.model_data() {
            model_data.view_model.set_has_focus(has_focus);
        }
    }

    /// Define o foco do widget (o `View` chama `viewModel.setHasWidgetFocus`). Sem modelo não faz nada.
    pub fn set_has_widget_focus(&self, has_focus: bool) {
        if let Some(model_data) = self.model_data() {
            model_data.view_model.set_has_widget_focus(has_focus);
        }
    }

    // #endregion

    // #region Widgets de conteúdo e de overlay, e medidas que dependem da View

    /// `addContentWidget(widget)`. Sem `View` real, o widget só fica registrado (e guarda a posição).
    pub fn add_content_widget(&self, widget: Rc<dyn IContentWidget>) {
        let id = widget.get_id();
        let data = ContentWidgetData { position: widget.get_position(), widget };
        let mut widgets = self.content_widgets.borrow_mut();
        match widgets.iter_mut().find(|(widget_id, _)| *widget_id == id) {
            Some((_, existing)) => {
                eprintln!("Overwriting a content widget with the same id:{id}");
                *existing = data;
            }
            None => widgets.push((id, data)),
        }
    }

    /// `layoutContentWidget(widget)`: relê a posição do widget registrado com o mesmo id.
    pub fn layout_content_widget(&self, widget: &Rc<dyn IContentWidget>) {
        let id = widget.get_id();
        // `getPosition` roda fora do empréstimo: o widget pode ler o editor.
        let position = widget.get_position();
        if let Some((_, data)) = self.content_widgets.borrow_mut().iter_mut().find(|(widget_id, _)| *widget_id == id) {
            data.position = position;
        }
    }

    /// `removeContentWidget(widget)`.
    pub fn remove_content_widget(&self, widget: &Rc<dyn IContentWidget>) {
        let id = widget.get_id();
        self.content_widgets.borrow_mut().retain(|(widget_id, _)| *widget_id != id);
    }

    /// `_contentWidgets`: os widgets de conteúdo registrados, na ordem de inserção (a `View` os lê).
    pub fn content_widgets(&self) -> Vec<ContentWidgetData> {
        self.content_widgets.borrow().iter().map(|(_, data)| data.clone()).collect()
    }

    /// `addOverlayWidget(widget)`. Sem `View` real, o widget só fica registrado (e guarda a posição).
    pub fn add_overlay_widget(&self, widget: Rc<dyn IOverlayWidget>) {
        let id = widget.get_id();
        let data = OverlayWidgetData { position: widget.get_position(), widget };
        let mut widgets = self.overlay_widgets.borrow_mut();
        match widgets.iter_mut().find(|(widget_id, _)| *widget_id == id) {
            Some((_, existing)) => {
                eprintln!("Overwriting an overlay widget with the same id.");
                *existing = data;
            }
            None => widgets.push((id, data)),
        }
    }

    /// `layoutOverlayWidget(widget)`: relê a posição do widget registrado com o mesmo id.
    pub fn layout_overlay_widget(&self, widget: &Rc<dyn IOverlayWidget>) {
        let id = widget.get_id();
        // `getPosition` roda fora do empréstimo: o widget pode ler o editor.
        let position = widget.get_position();
        if let Some((_, data)) = self.overlay_widgets.borrow_mut().iter_mut().find(|(widget_id, _)| *widget_id == id) {
            data.position = position;
        }
    }

    /// `removeOverlayWidget(widget)`.
    pub fn remove_overlay_widget(&self, widget: &Rc<dyn IOverlayWidget>) {
        let id = widget.get_id();
        self.overlay_widgets.borrow_mut().retain(|(widget_id, _)| *widget_id != id);
    }

    /// `_overlayWidgets`: os widgets de overlay registrados, na ordem de inserção (a `View` os lê).
    pub fn overlay_widgets(&self) -> Vec<OverlayWidgetData> {
        self.overlay_widgets.borrow().iter().map(|(_, data)| data.clone()).collect()
    }

    /// `getOffsetForColumn(lineNumber, column)`: sem `View` real (`hasRealView` falso) é `-1`.
    pub fn get_offset_for_column(&self, _line_number: i32, _column: i32) -> f64 {
        -1.0
    }

    /// `getWidthOfLine(lineNumber)`: sem `View` real (`hasRealView` falso) é `-1`.
    pub fn get_width_of_line(&self, _line_number: i32) -> f64 {
        -1.0
    }

    /// `resetLineWidthCaches()`: sem `View` real (`hasRealView` falso) não faz nada.
    pub fn reset_line_width_caches(&self) {}

    // #endregion

    // #region Estado interno

    /// `_modelData`, com o empréstimo do `RefCell` já solto.
    fn model_data(&self) -> Option<Rc<ModelData>> {
        self.model_data.borrow().clone()
    }

    /// `_beginUpdate()`.
    fn begin_update(&self) {
        self.update_counter.set(self.update_counter.get() + 1);
        if self.update_counter.get() == 1 {
            self.emitters.begin_update.fire(());
        }
    }

    /// `_endUpdate()`.
    fn end_update(&self) {
        self.update_counter.set(self.update_counter.get() - 1);
        if self.update_counter.get() == 0 {
            self.emitters.end_update.fire(());
        }
    }

    /// `_beginUpdate()` com o `_endUpdate()` no `Drop`.
    fn update_scope(&self) -> UpdateScope<'_> {
        self.begin_update();
        UpdateScope(self)
    }

    /// `fire` de um `InteractionEmitter`: garante as contribuições antes do evento.
    fn fire_interaction<T: 'static>(&self, emitter: &Emitter<T>, event: T) {
        self.contributions.on_before_interaction_event();
        emitter.fire(event);
    }

    // #endregion

    // #region Ligar e desligar o modelo

    /// `_attachModel(model)`.
    fn attach_model(&self, model: Option<Rc<TextModel>>) {
        let Some(model) = model else {
            *self.model_data.borrow_mut() = None;
            return;
        };

        let mut listeners_to_remove: Vec<DisposableRef> = Vec::new();

        self.configuration.set_is_dominated_by_long_lines(model.is_dominated_by_long_lines());
        self.configuration.set_model_line_count(CursorSimpleModel::get_line_count(&*model));

        let attached_view = model.on_before_attached();

        let view_model = ViewModel::new(
            self.id,
            self.configuration.clone(),
            model.clone(),
            self.services.dom_line_breaks_computer_factory.clone(),
            Rc::new(MonospaceLineBreaksComputerFactory::create(self.configuration.options())),
            self.services.schedule_at_next_animation_frame.clone(),
            self.services.language_configuration_service.clone(),
            self.services.theme_service.clone(),
            attached_view.clone(),
            WidgetBatchTarget(self.me.clone()),
        );

        // Someone might destroy the model from under the editor, so prevent any exceptions by setting a null model
        let me = self.me.clone();
        listeners_to_remove.push(model.on_will_dispose().listen(move |_| {
            if let Some(editor) = me.upgrade() {
                editor.set_model(None);
            }
        }));

        let me = self.me.clone();
        listeners_to_remove.push(view_model.on_event.listen(move |e| {
            if let Some(editor) = me.upgrade() {
                editor.on_view_model_event(e);
            }
        }));

        *self.model_data.borrow_mut() = Some(Rc::new(ModelData { model, view_model, listeners_to_remove, attached_view }));
    }

    /// O `switch (e.kind)` do ouvinte de `viewModel.onEvent` em `_attachModel`.
    fn on_view_model_event(&self, e: &OutgoingViewModelEvent) {
        match e {
            OutgoingViewModelEvent::ContentSizeChanged(e) => self.emitters.did_content_size_change.fire(e.clone()),
            OutgoingViewModelEvent::FocusChanged(e) => {
                self.has_text_focus.set(e.has_focus);
                self.editor_text_focus.set_value(e.has_focus);
            }
            OutgoingViewModelEvent::WidgetFocusChanged(e) => {
                self.has_widget_focus.set(e.has_focus);
                self.editor_widget_focus.set_value(e.has_focus);
            }
            OutgoingViewModelEvent::ScrollChanged(e) => self.emitters.did_scroll_change.fire(*e),
            OutgoingViewModelEvent::ViewZonesChanged(_) => self.emitters.did_change_view_zones.fire(()),
            OutgoingViewModelEvent::HiddenAreasChanged(_) => self.emitters.did_change_hidden_areas.fire(()),
            OutgoingViewModelEvent::ReadOnlyEditAttempt(_) => {
                self.fire_interaction(&*self.emitters.did_attempt_read_only_edit, ());
            }
            OutgoingViewModelEvent::CursorStateChanged(e) => {
                if e.reached_max_cursor_count {
                    self.notify_cursor_limit_reached();
                }

                let positions: Vec<Position> = e.selections.iter().map(Selection::get_position).collect();

                let e1 = ICursorPositionChangedEvent {
                    position: positions[0],
                    secondary_positions: positions[1..].to_vec(),
                    reason: e.reason,
                    source: e.source.clone(),
                };
                self.emitters.did_change_cursor_position.fire(e1);

                let e2 = ICursorSelectionChangedEvent {
                    selection: e.selections[0],
                    secondary_selections: e.selections[1..].to_vec(),
                    model_version_id: e.model_version_id,
                    old_selections: e.old_selections.clone(),
                    old_model_version_id: e.old_model_version_id,
                    source: e.source.clone(),
                    reason: e.reason,
                };
                self.emitters.did_change_cursor_selection.fire(e2);
            }
            OutgoingViewModelEvent::ModelDecorationsChanged(e) => {
                self.emitters.did_change_model_decorations.fire(e.event.clone());
            }
            OutgoingViewModelEvent::ModelLanguageChanged(e) => self.emitters.did_change_model_language.fire(e.event.clone()),
            OutgoingViewModelEvent::ModelLanguageConfigurationChanged(e) => {
                self.emitters.did_change_model_language_configuration.fire(e.event.clone());
            }
            OutgoingViewModelEvent::ModelContentChanged(e) => self.emitters.did_change_model_content.fire(e.event.clone()),
            OutgoingViewModelEvent::ModelOptionsChanged(e) => self.emitters.did_change_model_options.fire(e.event.clone()),
            OutgoingViewModelEvent::ModelTokensChanged(e) => self.emitters.did_change_model_tokens.fire(e.event.clone()),
            OutgoingViewModelEvent::ModelLineHeightChanged(e) => self.emitters.did_change_line_height.fire(e.event.clone()),
            OutgoingViewModelEvent::ModelFontChanged(e) => self.emitters.did_change_font.fire(e.event.clone()),
        }
    }

    /// O aviso de `reachedMaxCursorCount` do ramo `CursorStateChanged` de `_attachModel`.
    fn notify_cursor_limit_reached(&self) {
        let multi_cursor_limit = *self.configuration.options().get(MULTI_CURSOR_LIMIT);
        let message = NotificationText::format(
            &localize(
                "cursors.maximumLinked",
                "The number of cursors has been limited to {0}. Consider using {1} for larger changes or increase the editor multi cursor limit setting.",
                &[],
            ),
            &[
                NotificationPart::Text(number_to_js_string(multi_cursor_limit)),
                NotificationPart::Rich(NotificationText::link(
                    &localize("cursors.findAndReplaceLink", "find and replace", &[]),
                    "https://code.visualstudio.com/docs/editor/codebasics#_find-and-replace",
                    None,
                )),
            ],
        );

        let command_service = self.services.command_service.clone();
        let find_and_replace: Rc<dyn Fn()> = Rc::new(move || {
            let _ = command_service.execute_command("editor.action.startFindReplaceAction", Vec::new());
        });
        let command_service = self.services.command_service.clone();
        let open_settings: Rc<dyn Fn()> = Rc::new(move || {
            let _ = command_service.execute_command(
                "workbench.action.openSettings2",
                vec![Dynamic::object([("query", Dynamic::from("editor.multiCursorLimit"))])],
            );
        });

        let choice = |label: String, run: Rc<dyn Fn()>| {
            PromptChoiceKind::Plain(PromptChoice {
                base: BasePromptChoice { telemetry_id: None, label, keep_open: false, run },
                is_secondary: false,
            })
        };
        self.services.notification_service.prompt(
            Severity::Warning,
            NotificationMessage::Rich(message),
            vec![
                choice("Find and Replace".to_owned(), find_and_replace),
                choice(localize("goToSetting", "Increase Multi Cursor Limit", &[]), open_settings),
            ],
            None,
        );
    }

    /// `_postDetachModelCleanup(detachedModel)`.
    fn post_detach_model_cleanup(&self, detached_model: Option<Rc<TextModel>>) {
        if let Some(model) = detached_model {
            model.remove_all_decorations_with_owner_id(self.id);
        }
    }

    /// `_detachModel()`.
    fn detach_model(&self) -> Option<Rc<TextModel>> {
        if let Some(contributions_disposable) = self.contributions_disposable.borrow_mut().take() {
            contributions_disposable.dispose();
        }
        let model_data = self.model_data()?;
        model_data.dispose();
        *self.model_data.borrow_mut() = None;
        Some(model_data.model.clone())
    }

    // #endregion

    // #region Comandos internos de `trigger`

    /// `_startComposition()`.
    fn handle_start_composition(&self) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        self.in_composition.set(true);
        model_data.view_model.start_composition();
        self.fire_interaction(&*self.emitters.did_composition_start, ());
    }

    /// `_endComposition(source)`.
    fn handle_end_composition(&self, source: Option<&str>) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        self.in_composition.set(false);
        model_data.view_model.end_composition(source);
        self.fire_interaction(&*self.emitters.did_composition_end, ());
    }

    /// `_type(source, text)`.
    fn handle_type(&self, source: Option<&str>, text: &JsString) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        if text.is_empty() {
            return;
        }
        if source == Some("keyboard") {
            self.fire_interaction(&*self.emitters.will_type, text.clone());
        }
        model_data.view_model.r#type(text.units(), source);
        if source == Some("keyboard") {
            self.fire_interaction(&*self.emitters.did_type, text.clone());
        }
    }

    /// `_compositionType(source, text, replacePrevCharCnt, replaceNextCharCnt, positionDelta)`.
    fn handle_composition_type(
        &self,
        source: Option<&str>,
        text: &JsString,
        replace_prev_char_cnt: i32,
        replace_next_char_cnt: i32,
        position_delta: i32,
    ) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        model_data.view_model.composition_type(text.units(), replace_prev_char_cnt, replace_next_char_cnt, position_delta, source);
    }

    /// `_paste(source, text, pasteOnNewLine, multicursorText, mode, isBlock)`.
    fn handle_paste(
        &self,
        source: Option<&str>,
        text: &JsString,
        paste_on_new_line: bool,
        multicursor_text: Option<Vec<JsString>>,
        mode: Option<String>,
        is_block: bool,
    ) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        let view_model = &model_data.view_model;
        let start_position = view_model.get_selection().get_start_position();
        view_model.paste(text.units(), paste_on_new_line, multicursor_text.as_deref(), source, is_block);
        let end_position = view_model.get_selection().get_start_position();
        if source == Some("keyboard") {
            self.fire_interaction(
                &*self.emitters.did_paste,
                IPasteEvent {
                    range: Range::new(start_position.line_number, start_position.column, end_position.line_number, end_position.column),
                    language_id: mode,
                },
            );
        }
    }

    /// `_cut(source)`.
    fn handle_cut(&self, source: Option<&str>) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        model_data.view_model.cut(source);
    }

    /// `_triggerEditorCommand(source, handlerId, payload)`.
    fn trigger_editor_command(&self, source: Option<&str>, handler_id: &str, payload: &Dynamic) -> bool {
        let Some(command) = self.services.extensions_registry.get_editor_command(handler_id) else {
            return false;
        };
        if let Dynamic::Object(object) = payload {
            object.set(&JsString::from("source"), source.map_or(Dynamic::Undefined, |source| Dynamic::from(source)));
        }
        let _ = self.run_command_with_services(&command, payload).catch(|error| errors::on_unexpected_error(&error));
        true
    }

    /// `_triggerCommand(handlerId, payload)`.
    fn trigger_command(&self, handler_id: &str, payload: Dynamic) {
        let _ = self.services.command_service.execute_command(handler_id, vec![payload]);
    }

    // #endregion

    // #region Layout e posição (privados)

    /// `_getVerticalOffsetForPosition(modelData, modelLineNumber, modelColumn, includeViewZones)`.
    fn vertical_offset_for_position(model_data: &ModelData, line_number: i32, column: i32, include_view_zones: bool) -> f64 {
        let model_position = model_data.model.validate_position(Position::new(line_number, column));
        let view_position =
            model_data.view_model.coordinates_converter.convert_model_position_to_view_position(model_position, None, None, None);
        model_data.view_model.layout().get_vertical_offset_for_line_number(view_position.line_number, Some(include_view_zones))
    }

    /// `_getVerticalOffsetAfterPosition(modelData, modelLineNumber, modelColumn, includeViewZones)`.
    fn vertical_offset_after_position(model_data: &ModelData, line_number: i32, column: i32, include_view_zones: bool) -> f64 {
        let model_position = model_data.model.validate_position(Position::new(line_number, column));
        let view_position =
            model_data.view_model.coordinates_converter.convert_model_position_to_view_position(model_position, None, None, None);
        model_data.view_model.layout().get_vertical_offset_after_line_number(view_position.line_number, Some(include_view_zones))
    }

    /// `_sendRevealRange(modelRange, verticalType, revealHorizontal, scrollType)`.
    fn send_reveal_range(&self, model_range: Range, vertical_type: VerticalRevealType, reveal_horizontal: bool, scroll_type: ScrollType) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        let validated_model_range = model_data.model.validate_range(&model_range);
        let view_range = model_data.view_model.coordinates_converter.convert_model_range_to_view_range(validated_model_range, None);

        model_data.view_model.reveal_range(Some("api"), reveal_horizontal, view_range, vertical_type, scroll_type);
    }

    /// `_revealLine(lineNumber, revealType, scrollType)`.
    fn reveal_line_impl(&self, line_number: i32, reveal_type: VerticalRevealType, scroll_type: Option<ScrollType>) {
        self.send_reveal_range(Range::new(line_number, 1, line_number, 1), reveal_type, false, scroll_or_smooth(scroll_type));
    }

    /// `_revealPosition(position, verticalType, revealHorizontal, scrollType)`.
    fn reveal_position_impl(&self, position: Position, vertical_type: VerticalRevealType, scroll_type: Option<ScrollType>) {
        self.send_reveal_range(
            Range::new(position.line_number, position.column, position.line_number, position.column),
            vertical_type,
            true,
            scroll_or_smooth(scroll_type),
        );
    }

    /// `_revealLines(startLineNumber, endLineNumber, verticalType, scrollType)`.
    fn reveal_lines_impl(
        &self,
        start_line_number: i32,
        end_line_number: i32,
        vertical_type: VerticalRevealType,
        scroll_type: Option<ScrollType>,
    ) {
        self.send_reveal_range(Range::new(start_line_number, 1, end_line_number, 1), vertical_type, false, scroll_or_smooth(scroll_type));
    }

    /// `_revealRange(range, verticalType, revealHorizontal, scrollType)`.
    fn reveal_range_impl(&self, range: Range, vertical_type: VerticalRevealType, reveal_horizontal: bool, scroll_type: Option<ScrollType>) {
        self.send_reveal_range(range, vertical_type, reveal_horizontal, scroll_or_smooth(scroll_type));
    }

    // #endregion
}

/// `scrollType = editorCommon.ScrollType.Smooth`.
fn scroll_or_smooth(scroll_type: Option<ScrollType>) -> ScrollType {
    scroll_type.unwrap_or(ScrollType::Smooth)
}

/// `scrollType = editorCommon.ScrollType.Immediate`.
fn scroll_or_immediate(scroll_type: Option<ScrollType>) -> ScrollType {
    scroll_type.unwrap_or(ScrollType::Immediate)
}

impl IDisposable for CodeEditorWidget {
    fn dispose(&self) {
        if let Some(code_editor_service) = &self.services.code_editor_service {
            code_editor_service.remove_code_editor(&ICodeEditor::get_id(self));
        }

        self.actions.borrow_mut().clear();
        self.content_widgets.borrow_mut().clear();
        self.overlay_widgets.borrow_mut().clear();

        let detached_model = self.detach_model();
        self.post_detach_model_cleanup(detached_model);

        self.emitters.did_dispose.fire(());

        self.base.dispose();
    }
}

/// `ICodeEditor` (`editorBrowser.ts`) e o `IEditor` de `editorCommon.ts` de que ela herda: a API pública do
/// editor, sem a parte de DOM (ver "Divergências" no topo do módulo).
pub trait ICodeEditor {
    /// `getId()`.
    fn get_id(&self) -> String;

    /// `getEditorType()`.
    fn get_editor_type(&self) -> &'static str;

    /// `updateOptions(newOptions)`: `None` é o `undefined`, tratado como `{}`.
    fn update_options(&self, new_options: Option<&Dynamic>);

    /// `getOptions()`.
    fn get_options(&self) -> &ComputedEditorOptions;

    /// `getOption(id)`.
    fn get_option<V: 'static>(&self, id: Opt<V>) -> &V
    where
        Self: Sized;

    /// `getRawOptions()`.
    fn get_raw_options(&self) -> Dynamic;

    /// `getValue(options)`.
    fn get_value(&self, options: Option<&GetValueOptions>) -> JsString;

    /// `setValue(newValue)`.
    fn set_value(&self, new_value: JsString);

    /// `getModel()`.
    fn get_model(&self) -> Option<Rc<TextModel>>;

    /// `setModel(model)`.
    fn set_model(&self, model: Option<Rc<TextModel>>);

    /// `hasModel()`.
    fn has_model(&self) -> bool;

    /// `getVisibleRanges()`.
    fn get_visible_ranges(&self) -> Vec<Range>;

    /// `getVisibleRangesPlusViewportAboveBelow()`.
    fn get_visible_ranges_plus_viewport_above_below(&self) -> Vec<Range>;

    /// `getWhitespaces()`.
    fn get_whitespaces(&self) -> Vec<IEditorWhitespace>;

    /// `getTopForLineNumber(lineNumber, includeViewZones = false)`.
    fn get_top_for_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64;

    /// `getTopForPosition(lineNumber, column)`.
    fn get_top_for_position(&self, line_number: i32, column: i32) -> f64;

    /// `getBottomForLineNumber(lineNumber, includeViewZones = false)`.
    fn get_bottom_for_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64;

    /// `getLineHeightForPosition(position)`.
    fn get_line_height_for_position(&self, position: Position) -> f64;

    /// `setHiddenAreas(ranges, source?, forceUpdate?)`.
    fn set_hidden_areas(&self, ranges: &[Range], source: HiddenAreasSource, force_update: Option<bool>);

    /// `getVisibleColumnFromPosition(rawPosition)`.
    fn get_visible_column_from_position(&self, raw_position: Position) -> i32;

    /// `getStatusbarColumn(rawPosition)`.
    fn get_statusbar_column(&self, raw_position: Position) -> i32;

    /// `getPosition()`.
    fn get_position(&self) -> Option<Position>;

    /// `setPosition(position, source = 'api')`.
    fn set_position(&self, position: Position, source: Option<&str>);

    /// `revealAllCursors(revealHorizontal, minimalReveal?)`.
    fn reveal_all_cursors(&self, reveal_horizontal: bool, minimal_reveal: Option<bool>);

    /// `revealLine(lineNumber, scrollType = Smooth)`.
    fn reveal_line(&self, line_number: i32, scroll_type: Option<ScrollType>);

    /// `revealLineInCenter(lineNumber, scrollType = Smooth)`.
    fn reveal_line_in_center(&self, line_number: i32, scroll_type: Option<ScrollType>);

    /// `revealLineInCenterIfOutsideViewport(lineNumber, scrollType = Smooth)`.
    fn reveal_line_in_center_if_outside_viewport(&self, line_number: i32, scroll_type: Option<ScrollType>);

    /// `revealLineNearTop(lineNumber, scrollType = Smooth)`.
    fn reveal_line_near_top(&self, line_number: i32, scroll_type: Option<ScrollType>);

    /// `revealPosition(position, scrollType = Smooth)`.
    fn reveal_position(&self, position: Position, scroll_type: Option<ScrollType>);

    /// `revealPositionInCenter(position, scrollType = Smooth)`.
    fn reveal_position_in_center(&self, position: Position, scroll_type: Option<ScrollType>);

    /// `revealPositionInCenterIfOutsideViewport(position, scrollType = Smooth)`.
    fn reveal_position_in_center_if_outside_viewport(&self, position: Position, scroll_type: Option<ScrollType>);

    /// `revealPositionNearTop(position, scrollType = Smooth)`.
    fn reveal_position_near_top(&self, position: Position, scroll_type: Option<ScrollType>);

    /// `getSelection()`.
    fn get_selection(&self) -> Option<Selection>;

    /// `getSelections()`.
    fn get_selections(&self) -> Option<Vec<Selection>>;

    /// `setSelection(range | selection, source = 'api')`.
    fn set_selection(&self, something: SelectionOrRange, source: Option<&str>);

    /// `revealLines(startLineNumber, endLineNumber, scrollType = Smooth)`.
    fn reveal_lines(&self, start_line_number: i32, end_line_number: i32, scroll_type: Option<ScrollType>);

    /// `revealLinesInCenter(startLineNumber, endLineNumber, scrollType = Smooth)`.
    fn reveal_lines_in_center(&self, start_line_number: i32, end_line_number: i32, scroll_type: Option<ScrollType>);

    /// `revealLinesInCenterIfOutsideViewport(startLineNumber, endLineNumber, scrollType = Smooth)`.
    fn reveal_lines_in_center_if_outside_viewport(&self, start_line_number: i32, end_line_number: i32, scroll_type: Option<ScrollType>);

    /// `revealLinesNearTop(startLineNumber, endLineNumber, scrollType = Smooth)`.
    fn reveal_lines_near_top(&self, start_line_number: i32, end_line_number: i32, scroll_type: Option<ScrollType>);

    /// `revealRange(range, scrollType = Smooth, revealVerticalInCenter = false, revealHorizontal = true)`.
    fn reveal_range(
        &self,
        range: Range,
        scroll_type: Option<ScrollType>,
        reveal_vertical_in_center: Option<bool>,
        reveal_horizontal: Option<bool>,
    );

    /// `revealRangeInCenter(range, scrollType = Smooth)`.
    fn reveal_range_in_center(&self, range: Range, scroll_type: Option<ScrollType>);

    /// `revealRangeInCenterIfOutsideViewport(range, scrollType = Smooth)`.
    fn reveal_range_in_center_if_outside_viewport(&self, range: Range, scroll_type: Option<ScrollType>);

    /// `revealRangeNearTop(range, scrollType = Smooth)`.
    fn reveal_range_near_top(&self, range: Range, scroll_type: Option<ScrollType>);

    /// `revealRangeNearTopIfOutsideViewport(range, scrollType = Smooth)`.
    fn reveal_range_near_top_if_outside_viewport(&self, range: Range, scroll_type: Option<ScrollType>);

    /// `revealRangeAtTop(range, scrollType = Smooth)`.
    fn reveal_range_at_top(&self, range: Range, scroll_type: Option<ScrollType>);

    /// `setSelections(ranges, source = 'api', reason = CursorChangeReason.NotSet)`: lança com a lista vazia.
    fn set_selections(&self, ranges: &[Selection], source: Option<&str>, reason: Option<CursorChangeReason>);

    /// `getContentWidth()`.
    fn get_content_width(&self) -> f64;

    /// `getScrollWidth()`.
    fn get_scroll_width(&self) -> f64;

    /// `getScrollLeft()`.
    fn get_scroll_left(&self) -> f64;

    /// `getContentHeight()`.
    fn get_content_height(&self) -> f64;

    /// `getScrollHeight()`.
    fn get_scroll_height(&self) -> f64;

    /// `getScrollTop()`.
    fn get_scroll_top(&self) -> f64;

    /// `setScrollLeft(newScrollLeft, scrollType = Immediate)`.
    fn set_scroll_left(&self, new_scroll_left: f64, scroll_type: Option<ScrollType>);

    /// `setScrollTop(newScrollTop, scrollType = Immediate)`.
    fn set_scroll_top(&self, new_scroll_top: f64, scroll_type: Option<ScrollType>);

    /// `setScrollPosition(position, scrollType = Immediate)`.
    fn set_scroll_position(&self, position: INewScrollPosition, scroll_type: Option<ScrollType>);

    /// `saveViewState()`.
    fn save_view_state(&self) -> Option<ICodeEditorViewState>;

    /// `restoreViewState(state)`.
    fn restore_view_state(&self, state: Option<&ICodeEditorViewState>);

    /// `getContribution(id)`.
    fn get_contribution(&self, id: &str) -> Option<Rc<dyn IEditorContribution>>;

    /// `getContribution<T>(id)`: a contribuição como o seu tipo concreto, ou `None` se não existe (ou se é de outro
    /// tipo, o que o `as T` do TypeScript não distingue).
    fn get_contribution_as<T: IEditorContribution>(&self, id: &str) -> Option<Rc<T>>
    where
        Self: Sized,
    {
        let contribution: Rc<dyn std::any::Any> = self.get_contribution(id)?;
        contribution.downcast::<T>().ok()
    }

    /// `getActions()`.
    fn get_actions(&self) -> Vec<Rc<dyn IEditorAction>>;

    /// `getSupportedActions()`.
    fn get_supported_actions(&self) -> Vec<Rc<dyn IEditorAction>>;

    /// `getAction(id)`.
    fn get_action(&self, id: &str) -> Option<Rc<dyn IEditorAction>>;

    /// `trigger(source, handlerId, payload)`.
    fn trigger(&self, source: Option<&str>, handler_id: &str, payload: Option<Dynamic>);

    /// `pushUndoStop()`.
    fn push_undo_stop(&self) -> bool;

    /// `popUndoStop()`.
    fn pop_undo_stop(&self) -> bool;

    /// `executeEdits(source, edits, endCursorState?)`.
    fn execute_edits<'a>(
        &self,
        source: ExecuteEditsSource<'a>,
        edits: Vec<SingleEditOperation>,
        end_cursor_state: Option<EndCursorState<'a>>,
    ) -> bool;

    /// `executeCommand(source, command)`.
    fn execute_command(&self, source: Option<&str>, command: Box<dyn Command>);

    /// `executeCommands(source, commands)`.
    fn execute_commands(&self, source: Option<&str>, commands: Vec<Option<Box<dyn Command>>>);

    /// `changeDecorations(callback)`.
    fn change_decorations<T>(&self, callback: impl FnOnce(&ModelDecorationsChangeAccessor<'_>) -> T) -> Option<T>
    where
        Self: Sized;

    /// `getLineDecorations(lineNumber)`.
    fn get_line_decorations(&self, line_number: i32) -> Option<Vec<ModelDecoration>>;

    /// `getDecorationsInRange(range)`.
    fn get_decorations_in_range(&self, range: Range) -> Option<Vec<ModelDecoration>>;

    /// `getFontSizeAtPosition(position)`.
    fn get_font_size_at_position(&self, position: Position) -> Option<String>;

    /// `deltaDecorations(oldDecorations, newDecorations)` (`@deprecated`).
    fn delta_decorations(&self, old_decorations: &[String], new_decorations: &[ModelDeltaDecoration]) -> Vec<String>;

    /// `removeDecorations(decorationIds)`.
    fn remove_decorations(&self, decoration_ids: &[String]);

    /// `getLayoutInfo()`.
    fn get_layout_info(&self) -> EditorLayoutInfo;

    /// `focus()`: sem DOM, marca o foco do texto e do widget (ver "Divergências").
    fn focus(&self);

    /// `hasTextFocus()`.
    fn has_text_focus(&self) -> bool;

    /// `hasWidgetFocus()`.
    fn has_widget_focus(&self) -> bool;
}

impl ICodeEditor for CodeEditorWidget {
    fn get_id(&self) -> String {
        format!("{}:{}", self.get_editor_type(), self.id)
    }

    fn get_editor_type(&self) -> &'static str {
        EDITOR_TYPE_ICODE_EDITOR
    }

    fn update_options(&self, new_options: Option<&Dynamic>) {
        match new_options {
            Some(new_options) => self.configuration.update_options(new_options),
            None => self.configuration.update_options(&payload_or_empty(None)),
        }
    }

    fn get_options(&self) -> &ComputedEditorOptions {
        self.configuration.options()
    }

    fn get_option<V: 'static>(&self, id: Opt<V>) -> &V
    where
        Self: Sized,
    {
        self.configuration.options().get(id)
    }

    fn get_raw_options(&self) -> Dynamic {
        self.configuration.get_raw_options()
    }

    fn get_value(&self, options: Option<&GetValueOptions>) -> JsString {
        let Some(model_data) = self.model_data() else {
            return JsString::default();
        };

        let preserve_bom = options.is_some_and(|options| options.preserve_bom);
        let eol_preference = match options.and_then(|options| options.line_ending.as_deref()) {
            Some("\n") => EndOfLinePreference::Lf,
            Some("\r\n") => EndOfLinePreference::CrLf,
            _ => EndOfLinePreference::TextDefined,
        };
        model_data.model.get_value_with(eol_preference, preserve_bom)
    }

    fn set_value(&self, new_value: JsString) {
        let _scope = self.update_scope();
        let Some(model_data) = self.model_data() else {
            return;
        };
        model_data.model.set_value(TextSource::Text(new_value));
    }

    fn get_model(&self) -> Option<Rc<TextModel>> {
        self.model_data().map(|model_data| model_data.model.clone())
    }

    fn set_model(&self, model: Option<Rc<TextModel>>) {
        let _scope = self.update_scope();
        let old_model = self.get_model();
        match (&old_model, &model) {
            // Current model is the new model
            (None, None) => return,
            (Some(old_model), Some(model)) if Rc::ptr_eq(old_model, model) => return,
            _ => {}
        }

        let e = IModelChangedEvent {
            old_model_url: old_model.as_ref().map(|model| model.uri()),
            new_model_url: model.as_ref().map(|model| model.uri()),
        };
        drop(old_model);
        self.emitters.will_change_model.fire(e.clone());

        let has_text_focus = self.has_text_focus();
        let detached_model = self.detach_model();
        self.attach_model(model);
        if self.has_model() {
            // we have a new model (with a new view)!
            if has_text_focus {
                self.focus();
            }
        } else {
            // we have no model (and no view) anymore
            // make sure the outside world knows we are not focused
            self.has_text_focus.set(false);
            self.has_widget_focus.set(false);
            self.editor_text_focus.set_value(false);
            self.editor_widget_focus.set_value(false);
        }

        self.emitters.did_change_model.fire(e);
        self.post_detach_model_cleanup(detached_model);

        *self.contributions_disposable.borrow_mut() = Some(self.contributions.on_after_model_attached(&self.services.run_when_idle));
    }

    fn has_model(&self) -> bool {
        self.model_data.borrow().is_some()
    }

    fn get_visible_ranges(&self) -> Vec<Range> {
        match self.model_data() {
            Some(model_data) => model_data.view_model.get_visible_ranges(),
            None => Vec::new(),
        }
    }

    fn get_visible_ranges_plus_viewport_above_below(&self) -> Vec<Range> {
        match self.model_data() {
            Some(model_data) => model_data.view_model.get_visible_ranges_plus_viewport_above_below(),
            None => Vec::new(),
        }
    }

    fn get_whitespaces(&self) -> Vec<IEditorWhitespace> {
        match self.model_data() {
            Some(model_data) => model_data.view_model.layout().get_whitespaces(),
            None => Vec::new(),
        }
    }

    fn get_top_for_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64 {
        let Some(model_data) = self.model_data() else {
            return -1.0;
        };
        CodeEditorWidget::vertical_offset_for_position(&model_data, line_number, 1, include_view_zones.unwrap_or(false))
    }

    fn get_top_for_position(&self, line_number: i32, column: i32) -> f64 {
        let Some(model_data) = self.model_data() else {
            return -1.0;
        };
        CodeEditorWidget::vertical_offset_for_position(&model_data, line_number, column, false)
    }

    fn get_bottom_for_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64 {
        let Some(model_data) = self.model_data() else {
            return -1.0;
        };
        // `Number.MAX_SAFE_INTEGER` no original: `validatePosition` limita a coluna ao fim da linha.
        CodeEditorWidget::vertical_offset_after_position(&model_data, line_number, i32::MAX, include_view_zones.unwrap_or(false))
    }

    fn get_line_height_for_position(&self, position: Position) -> f64 {
        let Some(model_data) = self.model_data() else {
            return -1.0;
        };
        let view_model = &model_data.view_model;
        let coordinates_converter = &view_model.coordinates_converter;
        if coordinates_converter.model_position_is_visible(position) {
            let view_position = coordinates_converter.convert_model_position_to_view_position(position, None, None, None);
            return view_model.layout().get_line_height_for_line_number(view_position.line_number);
        }
        0.0
    }

    fn set_hidden_areas(&self, ranges: &[Range], source: HiddenAreasSource, force_update: Option<bool>) {
        if let Some(model_data) = self.model_data() {
            model_data.view_model.set_hidden_areas(ranges.to_vec(), source, force_update.unwrap_or(false));
        }
    }

    fn get_visible_column_from_position(&self, raw_position: Position) -> i32 {
        let Some(model_data) = self.model_data() else {
            return raw_position.column;
        };

        let position = model_data.model.validate_position(raw_position);
        let tab_size = model_data.model.get_options().tab_size;
        let line_content = CursorSimpleModel::get_line_content(&*model_data.model, position.line_number);

        cursor_columns::visible_column_from_column(line_content.units(), position.column, tab_size) + 1
    }

    fn get_statusbar_column(&self, raw_position: Position) -> i32 {
        let Some(model_data) = self.model_data() else {
            return raw_position.column;
        };

        let position = model_data.model.validate_position(raw_position);
        let tab_size = model_data.model.get_options().tab_size;
        let line_content = CursorSimpleModel::get_line_content(&*model_data.model, position.line_number);

        cursor_columns::to_statusbar_column(line_content.units(), position.column, tab_size)
    }

    fn get_position(&self) -> Option<Position> {
        self.model_data().map(|model_data| model_data.view_model.get_position())
    }

    fn set_position(&self, position: Position, source: Option<&str>) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        model_data.view_model.set_selections(
            Some(source.unwrap_or("api")),
            &[Selection::new(position.line_number, position.column, position.line_number, position.column)],
            CursorChangeReason::NotSet,
        );
    }

    fn reveal_all_cursors(&self, reveal_horizontal: bool, minimal_reveal: Option<bool>) {
        if let Some(model_data) = self.model_data() {
            model_data.view_model.reveal_all_cursors(Some("api"), reveal_horizontal, minimal_reveal);
        }
    }

    fn reveal_line(&self, line_number: i32, scroll_type: Option<ScrollType>) {
        self.reveal_line_impl(line_number, VerticalRevealType::Simple, scroll_type);
    }

    fn reveal_line_in_center(&self, line_number: i32, scroll_type: Option<ScrollType>) {
        self.reveal_line_impl(line_number, VerticalRevealType::Center, scroll_type);
    }

    fn reveal_line_in_center_if_outside_viewport(&self, line_number: i32, scroll_type: Option<ScrollType>) {
        self.reveal_line_impl(line_number, VerticalRevealType::CenterIfOutsideViewport, scroll_type);
    }

    fn reveal_line_near_top(&self, line_number: i32, scroll_type: Option<ScrollType>) {
        self.reveal_line_impl(line_number, VerticalRevealType::NearTop, scroll_type);
    }

    fn reveal_position(&self, position: Position, scroll_type: Option<ScrollType>) {
        self.reveal_position_impl(position, VerticalRevealType::Simple, scroll_type);
    }

    fn reveal_position_in_center(&self, position: Position, scroll_type: Option<ScrollType>) {
        self.reveal_position_impl(position, VerticalRevealType::Center, scroll_type);
    }

    fn reveal_position_in_center_if_outside_viewport(&self, position: Position, scroll_type: Option<ScrollType>) {
        self.reveal_position_impl(position, VerticalRevealType::CenterIfOutsideViewport, scroll_type);
    }

    fn reveal_position_near_top(&self, position: Position, scroll_type: Option<ScrollType>) {
        self.reveal_position_impl(position, VerticalRevealType::NearTop, scroll_type);
    }

    fn get_selection(&self) -> Option<Selection> {
        self.model_data().map(|model_data| model_data.view_model.get_selection())
    }

    fn get_selections(&self) -> Option<Vec<Selection>> {
        self.model_data().map(|model_data| model_data.view_model.get_selections())
    }

    fn set_selection(&self, something: SelectionOrRange, source: Option<&str>) {
        let selection = match something {
            SelectionOrRange::Selection(selection) => selection,
            // act as if it was an IRange
            SelectionOrRange::Range(range) => {
                Selection::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column)
            }
        };
        let Some(model_data) = self.model_data() else {
            return;
        };
        model_data.view_model.set_selections(Some(source.unwrap_or("api")), &[selection], CursorChangeReason::NotSet);
    }

    fn reveal_lines(&self, start_line_number: i32, end_line_number: i32, scroll_type: Option<ScrollType>) {
        self.reveal_lines_impl(start_line_number, end_line_number, VerticalRevealType::Simple, scroll_type);
    }

    fn reveal_lines_in_center(&self, start_line_number: i32, end_line_number: i32, scroll_type: Option<ScrollType>) {
        self.reveal_lines_impl(start_line_number, end_line_number, VerticalRevealType::Center, scroll_type);
    }

    fn reveal_lines_in_center_if_outside_viewport(&self, start_line_number: i32, end_line_number: i32, scroll_type: Option<ScrollType>) {
        self.reveal_lines_impl(start_line_number, end_line_number, VerticalRevealType::CenterIfOutsideViewport, scroll_type);
    }

    fn reveal_lines_near_top(&self, start_line_number: i32, end_line_number: i32, scroll_type: Option<ScrollType>) {
        self.reveal_lines_impl(start_line_number, end_line_number, VerticalRevealType::NearTop, scroll_type);
    }

    fn reveal_range(
        &self,
        range: Range,
        scroll_type: Option<ScrollType>,
        reveal_vertical_in_center: Option<bool>,
        reveal_horizontal: Option<bool>,
    ) {
        self.reveal_range_impl(
            range,
            if reveal_vertical_in_center.unwrap_or(false) { VerticalRevealType::Center } else { VerticalRevealType::Simple },
            reveal_horizontal.unwrap_or(true),
            scroll_type,
        );
    }

    fn reveal_range_in_center(&self, range: Range, scroll_type: Option<ScrollType>) {
        self.reveal_range_impl(range, VerticalRevealType::Center, true, scroll_type);
    }

    fn reveal_range_in_center_if_outside_viewport(&self, range: Range, scroll_type: Option<ScrollType>) {
        self.reveal_range_impl(range, VerticalRevealType::CenterIfOutsideViewport, true, scroll_type);
    }

    fn reveal_range_near_top(&self, range: Range, scroll_type: Option<ScrollType>) {
        self.reveal_range_impl(range, VerticalRevealType::NearTop, true, scroll_type);
    }

    fn reveal_range_near_top_if_outside_viewport(&self, range: Range, scroll_type: Option<ScrollType>) {
        self.reveal_range_impl(range, VerticalRevealType::NearTopIfOutsideViewport, true, scroll_type);
    }

    fn reveal_range_at_top(&self, range: Range, scroll_type: Option<ScrollType>) {
        self.reveal_range_impl(range, VerticalRevealType::Top, true, scroll_type);
    }

    fn set_selections(&self, ranges: &[Selection], source: Option<&str>, reason: Option<CursorChangeReason>) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        if ranges.is_empty() {
            errors::throw(Error::new("Invalid arguments"));
        }
        model_data.view_model.set_selections(Some(source.unwrap_or("api")), ranges, reason.unwrap_or(CursorChangeReason::NotSet));
    }

    fn get_content_width(&self) -> f64 {
        match self.model_data() {
            Some(model_data) => model_data.view_model.layout().get_content_width(),
            None => -1.0,
        }
    }

    fn get_scroll_width(&self) -> f64 {
        match self.model_data() {
            Some(model_data) => model_data.view_model.layout().get_scroll_width(),
            None => -1.0,
        }
    }

    fn get_scroll_left(&self) -> f64 {
        match self.model_data() {
            Some(model_data) => model_data.view_model.layout().get_current_scroll_left(),
            None => -1.0,
        }
    }

    fn get_content_height(&self) -> f64 {
        match self.model_data() {
            Some(model_data) => model_data.view_model.layout().get_content_height(),
            None => -1.0,
        }
    }

    fn get_scroll_height(&self) -> f64 {
        match self.model_data() {
            Some(model_data) => model_data.view_model.layout().get_scroll_height(),
            None => -1.0,
        }
    }

    fn get_scroll_top(&self) -> f64 {
        match self.model_data() {
            Some(model_data) => model_data.view_model.layout().get_current_scroll_top(),
            None => -1.0,
        }
    }

    fn set_scroll_left(&self, new_scroll_left: f64, scroll_type: Option<ScrollType>) {
        if let Some(model_data) = self.model_data() {
            model_data
                .view_model
                .layout()
                .set_scroll_position(INewScrollPosition { scroll_left: Some(new_scroll_left), scroll_top: None }, scroll_or_immediate(scroll_type));
        }
    }

    fn set_scroll_top(&self, new_scroll_top: f64, scroll_type: Option<ScrollType>) {
        if let Some(model_data) = self.model_data() {
            model_data
                .view_model
                .layout()
                .set_scroll_position(INewScrollPosition { scroll_left: None, scroll_top: Some(new_scroll_top) }, scroll_or_immediate(scroll_type));
        }
    }

    fn set_scroll_position(&self, position: INewScrollPosition, scroll_type: Option<ScrollType>) {
        if let Some(model_data) = self.model_data() {
            model_data.view_model.layout().set_scroll_position(position, scroll_or_immediate(scroll_type));
        }
    }

    fn save_view_state(&self) -> Option<ICodeEditorViewState> {
        let model_data = self.model_data()?;
        let contributions_state = self.contributions.save_view_state();
        let cursor_state = model_data.view_model.save_cursor_state();
        let view_state = model_data.view_model.save_state();
        Some(ICodeEditorViewState { cursor_state, view_state, contributions_state })
    }

    fn restore_view_state(&self, state: Option<&ICodeEditorViewState>) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        let Some(state) = state else {
            return;
        };
        if !state.cursor_state.is_empty() {
            model_data.view_model.restore_cursor_state(&state.cursor_state);
        }

        self.contributions.restore_view_state(&state.contributions_state);
        let ReducedRestoreState { scroll_left, scroll_top } = model_data.view_model.reduce_restore_state(&state.view_state);
        // `View.restoreState(reducedState)`.
        model_data
            .view_model
            .layout()
            .set_scroll_position(INewScrollPosition { scroll_left: Some(scroll_left), scroll_top: Some(scroll_top) }, ScrollType::Immediate);
        model_data.view_model.visible_lines_stabilized();
    }

    fn get_contribution(&self, id: &str) -> Option<Rc<dyn IEditorContribution>> {
        self.contributions.get(id)
    }

    fn get_actions(&self) -> Vec<Rc<dyn IEditorAction>> {
        self.actions.borrow().iter().map(|(_, action)| action.clone()).collect()
    }

    fn get_supported_actions(&self) -> Vec<Rc<dyn IEditorAction>> {
        self.get_actions().into_iter().filter(|action| action.is_supported()).collect()
    }

    fn get_action(&self, id: &str) -> Option<Rc<dyn IEditorAction>> {
        self.actions.borrow().iter().find(|(action_id, _)| action_id == id).map(|(_, action)| action.clone())
    }

    fn trigger(&self, source: Option<&str>, handler_id: &str, payload: Option<Dynamic>) {
        let payload = payload_or_empty(payload);

        self.emitters.will_trigger_editor_operation_event.fire(ITriggerEditorOperationEvent {
            source: source.map(str::to_owned),
            handler_id: handler_id.to_owned(),
            payload: payload.clone(),
        });
        let _scope = self.update_scope();

        match handler_id {
            Handler::COMPOSITION_START => {
                self.handle_start_composition();
                return;
            }
            Handler::COMPOSITION_END => {
                self.handle_end_composition(source);
                return;
            }
            Handler::TYPE => {
                self.handle_type(source, &payload_text(&payload, "text"));
                return;
            }
            Handler::REPLACE_PREVIOUS_CHAR => {
                self.handle_composition_type(source, &payload_text(&payload, "text"), payload_count(&payload, "replaceCharCnt"), 0, 0);
                return;
            }
            Handler::COMPOSITION_TYPE => {
                self.handle_composition_type(
                    source,
                    &payload_text(&payload, "text"),
                    payload_count(&payload, "replacePrevCharCnt"),
                    payload_count(&payload, "replaceNextCharCnt"),
                    payload_count(&payload, "positionDelta"),
                );
                return;
            }
            Handler::PASTE => {
                self.handle_paste(
                    source,
                    &payload_text(&payload, "text"),
                    payload_flag(&payload, "pasteOnNewLine"),
                    payload_lines(&payload, "multicursorText"),
                    payload_optional_text(&payload, "mode"),
                    payload_flag(&payload, "isBlock"),
                );
                return;
            }
            Handler::CUT => {
                self.handle_cut(source);
                return;
            }
            _ => {}
        }

        if let Some(action) = self.get_action(handler_id) {
            let _ = action.run(Some(&payload)).catch(|error| errors::on_unexpected_error(&error));
            return;
        }

        if !self.has_model() {
            return;
        }

        if self.trigger_editor_command(source, handler_id, &payload) {
            return;
        }

        self.trigger_command(handler_id, payload);
    }

    fn push_undo_stop(&self) -> bool {
        let Some(model_data) = self.model_data() else {
            return false;
        };
        if *self.configuration.options().get(READ_ONLY) {
            // read only editor => sorry!
            return false;
        }
        model_data.model.push_stack_element();
        true
    }

    fn pop_undo_stop(&self) -> bool {
        let Some(model_data) = self.model_data() else {
            return false;
        };
        if *self.configuration.options().get(READ_ONLY) {
            // read only editor => sorry!
            return false;
        }
        model_data.model.pop_stack_element();
        true
    }

    fn execute_edits<'a>(
        &self,
        source: ExecuteEditsSource<'a>,
        edits: Vec<SingleEditOperation>,
        end_cursor_state: Option<EndCursorState<'a>>,
    ) -> bool {
        let Some(model_data) = self.model_data() else {
            return false;
        };
        if *self.configuration.options().get(READ_ONLY) {
            // read only editor => sorry!
            return false;
        }

        let mut cursor_state_computer: Box<dyn FnMut(&[ValidEditOperation]) -> Option<Vec<Selection>> + 'a> = match end_cursor_state {
            None => Box::new(|_| None),
            Some(EndCursorState::Selections(selections)) => Box::new(move |_| Some(selections.clone())),
            Some(EndCursorState::Computer(computer)) => Box::new(computer),
        };

        let (source_str, reason): (Option<String>, TextModelEditSource) = match source {
            ExecuteEditsSource::Reason(reason) => (Some(reason.source.clone()), reason),
            ExecuteEditsSource::Name(name) => (name.map(str::to_owned), EditSources::unknown(name)),
        };

        self.emitters.before_execute_edit.fire(BeforeExecuteEditEvent { source: source_str.clone() });
        model_data.view_model.execute_edits(source_str.as_deref(), edits, &mut *cursor_state_computer, reason);
        true
    }

    fn execute_command(&self, source: Option<&str>, command: Box<dyn Command>) {
        if let Some(model_data) = self.model_data() {
            model_data.view_model.execute_command(command, source);
        }
    }

    fn execute_commands(&self, source: Option<&str>, commands: Vec<Option<Box<dyn Command>>>) {
        if let Some(model_data) = self.model_data() {
            model_data.view_model.execute_commands(commands, source);
        }
    }

    fn change_decorations<T>(&self, callback: impl FnOnce(&ModelDecorationsChangeAccessor<'_>) -> T) -> Option<T>
    where
        Self: Sized,
    {
        // callback will not be called when there is no model
        let model_data = self.model_data()?;
        model_data.model.change_decorations(callback, self.id)
    }

    fn get_line_decorations(&self, line_number: i32) -> Option<Vec<ModelDecoration>> {
        let model_data = self.model_data()?;
        let options = self.configuration.options();
        Some(model_data.model.get_line_decorations(
            line_number,
            self.id,
            filter_validation_decorations(options),
            filter_font_decorations(options),
        ))
    }

    fn get_decorations_in_range(&self, range: Range) -> Option<Vec<ModelDecoration>> {
        let model_data = self.model_data()?;
        let options = self.configuration.options();
        Some(model_data.model.get_decorations_in_range(
            &range,
            self.id,
            filter_validation_decorations(options),
            filter_font_decorations(options),
            false,
            false,
        ))
    }

    fn get_font_size_at_position(&self, position: Position) -> Option<String> {
        let model_data = self.model_data()?;
        model_data.view_model.get_font_size_at_position(position)
    }

    fn delta_decorations(&self, old_decorations: &[String], new_decorations: &[ModelDeltaDecoration]) -> Vec<String> {
        let Some(model_data) = self.model_data() else {
            return Vec::new();
        };

        if old_decorations.is_empty() && new_decorations.is_empty() {
            return old_decorations.to_vec();
        }

        model_data.model.delta_decorations(old_decorations, new_decorations, self.id)
    }

    fn remove_decorations(&self, decoration_ids: &[String]) {
        let Some(model_data) = self.model_data() else {
            return;
        };
        if decoration_ids.is_empty() {
            return;
        }

        model_data.model.change_decorations(
            |change_accessor| {
                change_accessor.delta_decorations(decoration_ids, &[]);
            },
            0,
        );
    }

    fn get_layout_info(&self) -> EditorLayoutInfo {
        *self.configuration.options().get(LAYOUT_INFO)
    }

    fn focus(&self) {
        if !self.has_model() {
            return;
        }
        self.set_has_text_focus(true);
        self.set_has_widget_focus(true);
    }

    fn has_text_focus(&self) -> bool {
        self.has_model() && self.has_text_focus.get()
    }

    fn has_widget_focus(&self) -> bool {
        self.has_model() && self.has_widget_focus.get()
    }
}
