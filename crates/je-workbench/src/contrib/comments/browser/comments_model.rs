//! Porte de `src/vs/workbench/contrib/comments/browser/commentsModel.ts`: o modelo que agrupa as threads de todos os
//! controladores por recurso (`CommentsModel`) e o contrato que o painel lê (`ICommentsModel`).
//!
//! Divergências:
//! - **`ICommentsModel`** é um trait com `&self`: o modelo guarda o estado atrás de `RefCell`, porque o `CommentService` o
//!   compartilha por `Rc` com o painel. `resourceCommentThreads` e `commentThreadsMap` devolvem cópias do estado atual (os
//!   `Rc` dos recursos são os mesmos objetos, então `ownerLabel` e as threads continuam compartilhados).
//! - **`commentThreadsMap`** é um [`OrderedMap`] (o `Map` do JavaScript mantém a ordem de inserção e não a muda ao sobrescrever).
//! - **`groupBy(commentThreads, _compareURIs)`** compara os textos das URIs por byte UTF-8 (o JavaScript, por unidade UTF-16);
//!   para URIs codificadas a ordem é a mesma.
//! - `URI.parse(thread.resource!)` com `resource` nulo lança `TypeError` no original; aqui [`resource_of`] lança o mesmo erro.
//! - `updateCommentThreads` altera no lugar o array de recursos do dono e o grava de volta; aqui a lista é copiada, alterada e
//!   gravada de volta, com o mesmo resultado para quem lê depois.
//! - `deleteCommentsByOwner(uniqueOwner?)` trata o texto vazio como ausente, como o `if (uniqueOwner)`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::arrays::group_by;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;
use je_base::nls::localize;
use je_base::uri::URI;
use je_editor::languages::{CommentBody, CommentThread};
use je_editor::range::Range;

use crate::contrib::comments::common::comment_model::{ICommentThreadChangedEvent, ResourceWithCommentThreads};

/// `threadHasMeaningfulComments(thread)`.
pub fn thread_has_meaningful_comments(thread: &dyn CommentThread<Range>) -> bool {
    thread.comments().is_some_and(|comments| {
        comments.iter().any(|comment| match &comment.body {
            CommentBody::Markdown(markdown) => !markdown.value.is_empty(),
            CommentBody::Text(text) => !text.is_empty(),
        })
    })
}

/// O valor do `Map` de `commentThreadsMap`: `{ resourceWithCommentThreads, ownerLabel? }`.
#[derive(Clone)]
pub struct CommentThreadsEntry {
    pub resource_with_comment_threads: Vec<Rc<ResourceWithCommentThreads>>,
    pub owner_label: Option<String>,
}

/// `ICommentsModel`.
pub trait ICommentsModel {
    fn has_comment_threads(&self) -> bool;
    fn get_message(&self) -> String;
    fn resource_comment_threads(&self) -> Vec<Rc<ResourceWithCommentThreads>>;
    fn comment_threads_map(&self) -> OrderedMap<String, CommentThreadsEntry>;
}

/// O `thread.resource!` como `URI.parse` o leria: lança `TypeError` se a thread não tem recurso.
fn resource_of(thread: &Rc<dyn CommentThread<Range>>) -> String {
    thread.resource().unwrap_or_else(|| errors::throw(Error::type_error("Cannot read properties of null (reading 'toString')")))
}

/// `CommentsModel._compareURIs`.
fn compare_uris(a: &Rc<dyn CommentThread<Range>>, b: &Rc<dyn CommentThread<Range>>) -> f64 {
    let resource_a = resource_of(a);
    let resource_b = resource_of(b);
    if resource_a < resource_b {
        -1.0
    } else if resource_a > resource_b {
        1.0
    } else {
        0.0
    }
}

/// `CommentsModel`.
pub struct CommentsModel {
    base: Rc<Disposable>,
    resource_comment_threads: RefCell<Vec<Rc<ResourceWithCommentThreads>>>,
    comment_threads_map: RefCell<OrderedMap<String, CommentThreadsEntry>>,
}

impl CommentsModel {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), resource_comment_threads: RefCell::new(Vec::new()), comment_threads_map: RefCell::new(OrderedMap::new()) })
    }

    fn update_resource_comment_threads(&self) {
        let map = self.comment_threads_map.borrow();
        let include_label = map.len() > 1;
        let mut resources = Vec::new();
        for value in map.values() {
            for resource in &value.resource_with_comment_threads {
                *resource.owner_label.borrow_mut() = if include_label { value.owner_label.clone() } else { None };
                resources.push(resource.clone());
            }
        }
        *self.resource_comment_threads.borrow_mut() = resources;
    }

    /// `setCommentThreads(uniqueOwner, owner, ownerLabel, commentThreads)`.
    pub fn set_comment_threads(&self, unique_owner: &str, owner: &str, owner_label: &str, comment_threads: &[Rc<dyn CommentThread<Range>>]) {
        let resource_with_comment_threads = Self::group_by_resource(unique_owner, owner, comment_threads);
        self.comment_threads_map
            .borrow_mut()
            .insert(unique_owner.to_owned(), CommentThreadsEntry { owner_label: Some(owner_label.to_owned()), resource_with_comment_threads });
        self.update_resource_comment_threads();
    }

    /// `deleteCommentsByOwner(uniqueOwner?)`.
    pub fn delete_comments_by_owner(&self, unique_owner: Option<&str>) {
        match unique_owner.filter(|owner| !owner.is_empty()) {
            Some(unique_owner) => {
                let mut map = self.comment_threads_map.borrow_mut();
                let owner_label = map.get(&unique_owner.to_owned()).and_then(|existing| existing.owner_label.clone());
                map.insert(unique_owner.to_owned(), CommentThreadsEntry { owner_label, resource_with_comment_threads: Vec::new() });
            }
            None => self.comment_threads_map.borrow_mut().clear(),
        }
        self.update_resource_comment_threads();
    }

    /// `updateCommentThreads(event)`: devolve se alguma thread foi removida, alterada ou adicionada.
    pub fn update_comment_threads(&self, event: &ICommentThreadChangedEvent) -> bool {
        let unique_owner = event.unique_owner.as_str();
        let owner = event.owner.as_str();

        let mut threads_for_owner = self.comment_threads_map.borrow().get(&unique_owner.to_owned()).map(|entry| entry.resource_with_comment_threads.clone()).unwrap_or_default();

        for thread in &event.removed {
            // Find resource that has the comment thread
            let thread_resource = thread.resource();
            let matching_resource_index = threads_for_owner.iter().position(|resource_data| Some(resource_data.id.as_str()) == thread_resource.as_deref());
            if let Some(matching_resource_index) = matching_resource_index {
                let matching_resource_data = threads_for_owner[matching_resource_index].clone();

                // Find comment node on resource that is that thread and remove it
                let thread_id = thread.thread_id();
                let index = matching_resource_data.comment_threads.borrow().iter().position(|comment_thread| comment_thread.thread_id == thread_id);
                if let Some(index) = index {
                    matching_resource_data.comment_threads.borrow_mut().remove(index);
                }

                // If the comment thread was the last thread for a resource, remove that resource from the list
                if matching_resource_data.comment_threads.borrow().is_empty() {
                    threads_for_owner.remove(matching_resource_index);
                }
            }
        }

        for thread in &event.changed {
            // Find resource that has the comment thread
            let thread_resource = thread.resource();
            let Some(matching_resource_data) = threads_for_owner.iter().find(|resource_data| Some(resource_data.id.as_str()) == thread_resource.as_deref()).cloned() else {
                continue;
            };

            // Find comment node on resource that is that thread and replace it
            let thread_id = thread.thread_id();
            let index = matching_resource_data.comment_threads.borrow().iter().position(|comment_thread| comment_thread.thread_id == thread_id);
            if let Some(index) = index {
                let node = ResourceWithCommentThreads::create_comment_node(unique_owner, owner, &URI::parse(matching_resource_data.id.as_str(), false), thread);
                matching_resource_data.comment_threads.borrow_mut()[index] = node;
            } else if thread.comments().is_some_and(|comments| !comments.is_empty()) {
                let node = ResourceWithCommentThreads::create_comment_node(unique_owner, owner, &URI::parse(matching_resource_data.id.as_str(), false), thread);
                matching_resource_data.comment_threads.borrow_mut().push(node);
            }
        }

        for added in &event.added {
            let thread = &added.thread;
            let thread_resource = thread.resource();
            let existing_resource =
                threads_for_owner.iter().find(|resource_with_threads| Some(resource_with_threads.resource.to_string(false).to_string_lossy()) == thread_resource).cloned();
            match existing_resource {
                Some(resource) => {
                    if thread.comments().is_some_and(|comments| !comments.is_empty()) {
                        let node = ResourceWithCommentThreads::create_comment_node(unique_owner, owner, &resource.resource, thread);
                        resource.comment_threads.borrow_mut().push(node);
                    }
                }
                None => {
                    let resource = URI::parse(resource_of(thread), false);
                    threads_for_owner.push(Rc::new(ResourceWithCommentThreads::new(unique_owner, owner, &resource, std::slice::from_ref(thread))));
                }
            }
        }

        self.comment_threads_map
            .borrow_mut()
            .insert(unique_owner.to_owned(), CommentThreadsEntry { owner_label: Some(event.owner_label.clone()), resource_with_comment_threads: threads_for_owner });
        self.update_resource_comment_threads();

        !event.removed.is_empty() || !event.changed.is_empty() || !event.added.is_empty()
    }

    fn group_by_resource(unique_owner: &str, owner: &str, comment_threads: &[Rc<dyn CommentThread<Range>>]) -> Vec<Rc<ResourceWithCommentThreads>> {
        group_by(comment_threads, compare_uris)
            .into_iter()
            .map(|group| Rc::new(ResourceWithCommentThreads::new(unique_owner, owner, &URI::parse(resource_of(&group[0]), false), &group)))
            .collect()
    }
}

impl ICommentsModel for CommentsModel {
    fn has_comment_threads(&self) -> bool {
        // There's a resource with at least one thread
        self.resource_comment_threads.borrow().iter().any(|resource| {
            // At least one of the threads in the resource has comments
            let threads = resource.comment_threads.borrow();
            !threads.is_empty() && threads.iter().any(|thread| {
                // At least one of the comments in the thread is not empty
                thread_has_meaningful_comments(&*thread.thread)
            })
        })
    }

    fn get_message(&self) -> String {
        if self.resource_comment_threads.borrow().is_empty() {
            localize("noComments", "There are no comments in this workspace yet.", &[])
        } else {
            String::new()
        }
    }

    fn resource_comment_threads(&self) -> Vec<Rc<ResourceWithCommentThreads>> {
        self.resource_comment_threads.borrow().clone()
    }

    fn comment_threads_map(&self) -> OrderedMap<String, CommentThreadsEntry> {
        self.comment_threads_map.borrow().clone()
    }
}

impl IDisposable for CommentsModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}
