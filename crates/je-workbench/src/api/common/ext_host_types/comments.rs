//! Os tipos de comentários da API `vscode` (`vscode.d.ts` e as propostas `commentThreadApplicability`,
//! `commentReveal`, `commentingRangeHint`, `commentsDraftState`, `commentReactor`) que `extHostComments.ts` usa:
//! `Comment`, `CommentReaction`, `CommentThreadRevealOptions`, `CommentingRangeProvider` e o valor de `state`
//! de uma `CommentThread`.
//!
//! Os enums (`CommentMode`, `CommentState`, `CommentThreadState`, `CommentThreadCollapsibleState`,
//! `CommentThreadApplicability`, `CommentThreadFocus`) moram em [`super::enums`].
//!
//! Divergências:
//! - Em JavaScript um `vscode.Comment` é um objeto literal com identidade, que a extensão muda (`comment.body = ...`) e
//!   que o `Map` da thread usa como chave. Aqui é um [`Comment`] dentro de `Rc` (a identidade é o ponteiro) com os
//!   campos num [`RefCell`] ([`Comment::data`]).
//! - Um objeto da API que atravessa um [`Dynamic`] (argumento de comando) é um objeto com carga nativa
//!   ([`ObjectRef::with_host`]). A carga é um [`Weak`] ([`WeakHost`]) do objeto, e não um `Rc`: o objeto guarda o
//!   próprio `Dynamic` (para a identidade ser estável), e um `Rc` na carga fecharia um ciclo que nunca é coletado.
//! - `CommentAuthorInformation`, `CommentOptions` e `CommentingRangeResourceHint` são os de
//!   [`ext_host_protocol`](crate::api::common::ext_host_protocol): o original os usa dos dois lados (`languages.*`) com
//!   a mesma forma.
//! - `vscode.CommentReaction.reactors` (`readonly string[] | CommentAuthorInformation[]`) é [`CommentReactors`]; o
//!   `iconPath: string | Uri` é [`PathOrUri`] e fica ausente quando a extensão não o informa.
//! - O `timestamp` (`Date`) é o valor de tempo em milissegundos.
//! - `ProviderResult<Range[] | CommentingRanges>` do `CommentingRangeProvider` é [`CommentingRangesResult`] dentro de um
//!   [`MaybePromise`]; o `provideCommentingRanges(document, token)` é um fecho.

use std::cell::{OnceCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::promise::MaybePromise;

use super::{CommentMode, CommentState, CommentThreadApplicability, CommentThreadFocus, CommentThreadState, MarkdownString, PathOrUri, Range};
use crate::api::common::ext_host_document_data::TextDocument;
use crate::api::common::ext_host_protocol::{CommentAuthorInformation, CommentingRangeResourceHint};

// ---- a carga nativa de um objeto da API dentro de um Dynamic -----------------------------------------------

/// A carga nativa de um objeto da API que cruza como [`Dynamic`]: um [`Weak`] do objeto.
pub struct WeakHost<T>(pub Weak<T>);

/// O [`Dynamic`] do objeto `this`, criado na primeira chamada e guardado em `cache`, para a identidade ser a mesma
/// em toda leitura (o `value` do original).
pub fn weak_host_dynamic<T: 'static>(cache: &OnceCell<Dynamic>, this: &Weak<T>) -> Dynamic {
    cache.get_or_init(|| Dynamic::Object(ObjectRef::with_host(Rc::new(WeakHost(this.clone()))))).clone()
}

/// O objeto da API que um [`Dynamic`] criado por [`weak_host_dynamic`] representa, se ainda estiver vivo.
pub fn host_object<T: 'static>(value: &Dynamic) -> Option<Rc<T>> {
    let Dynamic::Object(object) = value else {
        return None;
    };
    object.host::<WeakHost<T>>()?.0.upgrade()
}

// ---- Comment ------------------------------------------------------------------------------------------------

/// `string | vscode.MarkdownString`: o `body` de um comentário.
#[derive(Clone, Debug)]
pub enum CommentBody {
    Text(String),
    Markdown(MarkdownString),
}

/// `vscode.CommentReaction.reactors`: `readonly string[] | CommentAuthorInformation[]`.
#[derive(Clone, Debug)]
pub enum CommentReactors {
    Names(Vec<String>),
    Authors(Vec<CommentAuthorInformation>),
}

/// `vscode.CommentReaction`.
#[derive(Clone)]
pub struct CommentReaction {
    pub label: Option<String>,
    pub icon_path: Option<PathOrUri>,
    pub count: f64,
    pub author_has_reacted: bool,
    pub reactors: Option<CommentReactors>,
}

/// Os campos de um `vscode.Comment`.
#[derive(Clone)]
pub struct CommentData {
    pub body: CommentBody,
    pub mode: CommentMode,
    pub author: CommentAuthorInformation,
    pub context_value: Option<String>,
    pub reactions: Option<Vec<CommentReaction>>,
    pub label: Option<String>,
    pub state: Option<CommentState>,
    /// `timestamp?: Date`: o valor de tempo em milissegundos.
    pub timestamp: Option<f64>,
}

impl CommentData {
    /// `{ body, mode, author }`, o resto ausente.
    pub fn new(body: CommentBody, mode: CommentMode, author: CommentAuthorInformation) -> CommentData {
        CommentData { body, mode, author, context_value: None, reactions: None, label: None, state: None, timestamp: None }
    }
}

/// `vscode.Comment`: um objeto com identidade.
pub struct Comment {
    this: Weak<Comment>,
    pub data: RefCell<CommentData>,
    value: OnceCell<Dynamic>,
}

impl Comment {
    pub fn new(data: CommentData) -> Rc<Comment> {
        Rc::new_cyclic(|this| Comment { this: this.clone(), data: RefCell::new(data), value: OnceCell::new() })
    }

    /// O comentário como argumento de comando.
    pub fn to_dynamic(&self) -> Dynamic {
        weak_host_dynamic(&self.value, &self.this)
    }

    /// O comentário de um argumento criado por [`Comment::to_dynamic`].
    pub fn from_dynamic(value: &Dynamic) -> Option<Rc<Comment>> {
        host_object::<Comment>(value)
    }
}

// ---- CommentThread ------------------------------------------------------------------------------------------

/// `vscode.CommentThreadState | { resolved?: vscode.CommentThreadState; applicability?: vscode.CommentThreadApplicability }`:
/// o valor de `CommentThread.state`. A forma de objeto exige a proposta `commentThreadApplicability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentThreadStateValue {
    State(CommentThreadState),
    Info { resolved: Option<CommentThreadState>, applicability: Option<CommentThreadApplicability> },
}

/// `vscode.CommentThreadRevealOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommentThreadRevealOptions {
    pub focus: Option<CommentThreadFocus>,
}

/// O primeiro argumento de `CommentThread.reveal`: `Comment | CommentThreadRevealOptions`. O original os distingue
/// por `body !== undefined`.
#[derive(Clone)]
pub enum CommentOrRevealOptions {
    Comment(Rc<Comment>),
    Options(CommentThreadRevealOptions),
}

// ---- CommentingRangeProvider --------------------------------------------------------------------------------

/// `vscode.CommentingRanges`.
#[derive(Clone, Debug, Default)]
pub struct CommentingRanges {
    pub ranges: Option<Vec<Range>>,
    pub enable_file_comments: Option<bool>,
}

/// `Range[] | CommentingRanges`: o resultado de `provideCommentingRanges`.
#[derive(Clone, Debug)]
pub enum CommentingRangesResult {
    Ranges(Vec<Range>),
    Info(CommentingRanges),
}

/// `provideCommentingRanges(document, token)`: `ProviderResult<Range[] | CommentingRanges>`.
pub type ProvideCommentingRanges = Rc<dyn Fn(&TextDocument, &CancellationToken) -> MaybePromise<Option<CommentingRangesResult>>>;

/// `vscode.CommentingRangeProvider`.
#[derive(Clone)]
pub struct CommentingRangeProvider {
    pub provide_commenting_ranges: ProvideCommentingRanges,
    /// `resourceHints` (proposta `commentingRangeHint`).
    pub resource_hints: Option<CommentingRangeResourceHint>,
}
