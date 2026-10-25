//! Porte de `src/vs/workbench/contrib/comments/common/commentModel.ts`: o evento de mudança de threads com o dono
//! (`ICommentThreadChangedEvent`), o nó de comentário (`CommentNode`) e o recurso com as threads dele
//! (`ResourceWithCommentThreads`).
//!
//! Divergências:
//! - **`ICommentThreadChangedEvent extends CommentThreadChangedEvent<IRange>`** é a struct com o evento base no campo `event`
//!   e os três campos do dono; o `Deref` deixa ler `removed`, `changed`, `added` e `pending` direto, como no original.
//! - **`CommentNode`** e **`ResourceWithCommentThreads`** são compartilhados por referência entre o `CommentsModel`, a lista
//!   achatada e quem os lê (a árvore do painel), então vivem em `Rc`. O que o original muda depois de criar (`ownerLabel`, a lista
//!   `commentThreads` do recurso) fica atrás de `RefCell`; `isRoot` e `replies` só são atribuídos em
//!   [`ResourceWithCommentThreads::create_comment_node`], antes de o nó ser compartilhado, e são campos comuns.
//! - **`lastUpdatedAt`** compara textos pelo valor de byte UTF-8 (o JavaScript compara por unidades UTF-16); para os
//!   carimbos de data ASCII dos controladores a ordem é a mesma.
//! - `comments!.map(...)` e `commentNodes[0].isRoot = true` lançam `TypeError` em uma thread sem comentários; aqui
//!   [`ResourceWithCommentThreads::create_comment_node`] lança o mesmo `TypeError`.

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::uri::URI;
use je_editor::languages::{Comment, CommentThread, CommentThreadApplicability, CommentThreadChangedEvent, CommentThreadState};
use je_editor::range::Range;

/// `ICommentThreadChangedEvent`.
#[derive(Clone)]
pub struct ICommentThreadChangedEvent {
    pub event: CommentThreadChangedEvent<Range>,
    pub unique_owner: String,
    pub owner: String,
    pub owner_label: String,
}

impl Deref for ICommentThreadChangedEvent {
    type Target = CommentThreadChangedEvent<Range>;

    fn deref(&self) -> &Self::Target {
        &self.event
    }
}

/// `CommentNode`.
pub struct CommentNode {
    pub is_root: bool,
    pub replies: Vec<Rc<CommentNode>>,
    pub thread_id: String,
    pub range: Option<Range>,
    pub thread_state: Option<CommentThreadState>,
    pub thread_relevance: Option<CommentThreadApplicability>,
    pub context_value: Option<String>,
    pub controller_handle: i64,
    pub thread_handle: i64,
    pub unique_owner: String,
    pub owner: String,
    pub resource: URI,
    pub comment: Comment,
    pub thread: Rc<dyn CommentThread<Range>>,
    last_updated_at: RefCell<Option<String>>,
}

impl CommentNode {
    pub fn new(unique_owner: &str, owner: &str, resource: &URI, comment: Comment, thread: &Rc<dyn CommentThread<Range>>) -> Self {
        Self {
            is_root: false,
            replies: Vec::new(),
            thread_id: thread.thread_id(),
            range: thread.range(),
            thread_state: thread.state(),
            thread_relevance: thread.applicability(),
            context_value: thread.context_value(),
            controller_handle: thread.controller_handle(),
            thread_handle: thread.comment_thread_handle(),
            unique_owner: unique_owner.to_owned(),
            owner: owner.to_owned(),
            resource: resource.clone(),
            comment,
            thread: thread.clone(),
            last_updated_at: RefCell::new(None),
        }
    }

    /// `hasReply()`.
    pub fn has_reply(&self) -> bool {
        !self.replies.is_empty()
    }

    /// `lastUpdatedAt`.
    pub fn last_updated_at(&self) -> String {
        if let Some(cached) = self.last_updated_at.borrow().as_ref() {
            return cached.clone();
        }

        let mut updated_at = self.comment.timestamp.clone().unwrap_or_default();
        if let Some(reply) = self.replies.last() {
            let reply_updated_at = reply.last_updated_at();
            if reply_updated_at > updated_at {
                updated_at = reply_updated_at;
            }
        }
        *self.last_updated_at.borrow_mut() = Some(updated_at.clone());
        updated_at
    }
}

/// `ResourceWithCommentThreads`.
pub struct ResourceWithCommentThreads {
    pub id: String,
    pub unique_owner: String,
    pub owner: String,
    pub owner_label: RefCell<Option<String>>,
    /// The top level comments on the file. Replys are nested under each node.
    pub comment_threads: RefCell<Vec<Rc<CommentNode>>>,
    pub resource: URI,
    last_updated_at: RefCell<Option<String>>,
}

impl ResourceWithCommentThreads {
    pub fn new(unique_owner: &str, owner: &str, resource: &URI, comment_threads: &[Rc<dyn CommentThread<Range>>]) -> Self {
        let comment_threads: Vec<Rc<CommentNode>> = comment_threads
            .iter()
            .filter(|thread| thread.comments().is_some_and(|comments| !comments.is_empty()))
            .map(|thread| Self::create_comment_node(unique_owner, owner, resource, thread))
            .collect();
        Self {
            id: resource.to_string(false).to_string_lossy(),
            unique_owner: unique_owner.to_owned(),
            owner: owner.to_owned(),
            owner_label: RefCell::new(None),
            comment_threads: RefCell::new(comment_threads),
            resource: resource.clone(),
            last_updated_at: RefCell::new(None),
        }
    }

    /// `createCommentNode(uniqueOwner, owner, resource, commentThread)`.
    pub fn create_comment_node(unique_owner: &str, owner: &str, resource: &URI, comment_thread: &Rc<dyn CommentThread<Range>>) -> Rc<CommentNode> {
        let Some(comments) = comment_thread.comments() else {
            errors::throw(Error::type_error("Cannot read properties of undefined (reading 'map')"));
        };

        let mut comment_nodes = comments.into_iter().map(|comment| CommentNode::new(unique_owner, owner, resource, comment, comment_thread));
        let Some(mut root) = comment_nodes.next() else {
            errors::throw(Error::type_error("Cannot set properties of undefined (setting 'isRoot')"));
        };
        root.replies = comment_nodes.map(Rc::new).collect();
        root.is_root = true;

        Rc::new(root)
    }

    /// `lastUpdatedAt`.
    pub fn last_updated_at(&self) -> String {
        if let Some(cached) = self.last_updated_at.borrow().as_ref() {
            return cached.clone();
        }

        let mut updated_at = String::new();
        // Return result without cahcing as we expect data to arrive later
        let comment_threads = self.comment_threads.borrow();
        if comment_threads.is_empty() {
            return updated_at;
        }
        for thread in comment_threads.iter() {
            let thread_updated_at = thread.last_updated_at();
            if !thread_updated_at.is_empty() && thread_updated_at > updated_at {
                updated_at = thread_updated_at;
            }
        }
        *self.last_updated_at.borrow_mut() = Some(updated_at.clone());
        updated_at
    }
}
