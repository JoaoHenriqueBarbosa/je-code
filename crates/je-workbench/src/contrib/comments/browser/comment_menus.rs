//! Porte de `src/vs/workbench/contrib/comments/browser/commentMenus.ts`: os menus de comentários (`CommentMenus`).
//!
//! Divergências:
//! - O `@IMenuService` chega como o argumento de [`CommentMenus::new`] (a injeção de dependências do original é feita por quem
//!   monta os serviços).
//! - O parâmetro `comment: Comment` de `getCommentTitleActions` e `getCommentActions` não é lido no original e não existe aqui.
//! - `IContextKeyService` entra como `Rc<dyn IContextKeyService>` nos métodos que criam o menu (o menu guarda o serviço) e por
//!   referência em `getCommentThreadTitleContextActions` (as ações saem na hora).

use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_platform::actions::menu_ids;
use je_platform::actions::{IMenu, IMenuService, MenuAction, MenuActionOptions, MenuCreateOptions, MenuId};
use je_platform::contextkey::contextkey::IContextKeyService;

/// `CommentMenus`.
pub struct CommentMenus {
    menu_service: Rc<dyn IMenuService>,
}

impl CommentMenus {
    pub fn new(menu_service: Rc<dyn IMenuService>) -> Self {
        Self { menu_service }
    }

    /// `getCommentThreadTitleActions(contextKeyService)`.
    pub fn get_comment_thread_title_actions(&self, context_key_service: Rc<dyn IContextKeyService>) -> Rc<dyn IMenu> {
        self.get_menu(&menu_ids::comment_thread_title(), context_key_service, None)
    }

    /// `getCommentThreadActions(contextKeyService)`.
    pub fn get_comment_thread_actions(&self, context_key_service: Rc<dyn IContextKeyService>) -> Rc<dyn IMenu> {
        self.get_menu(&menu_ids::comment_thread_actions(), context_key_service, None)
    }

    /// `getCommentEditorActions(contextKeyService)`.
    pub fn get_comment_editor_actions(&self, context_key_service: Rc<dyn IContextKeyService>) -> Rc<dyn IMenu> {
        self.get_menu(&menu_ids::comment_editor_actions(), context_key_service, None)
    }

    /// `getCommentThreadAdditionalActions(contextKeyService)`.
    pub fn get_comment_thread_additional_actions(&self, context_key_service: Rc<dyn IContextKeyService>) -> Rc<dyn IMenu> {
        self.get_menu(
            &menu_ids::comment_thread_additional_actions(),
            context_key_service,
            Some(MenuCreateOptions { emit_events_for_submenu_changes: true, ..MenuCreateOptions::default() }),
        )
    }

    /// `getCommentTitleActions(comment, contextKeyService)`.
    pub fn get_comment_title_actions(&self, context_key_service: Rc<dyn IContextKeyService>) -> Rc<dyn IMenu> {
        self.get_menu(&menu_ids::comment_title(), context_key_service, None)
    }

    /// `getCommentActions(comment, contextKeyService)`.
    pub fn get_comment_actions(&self, context_key_service: Rc<dyn IContextKeyService>) -> Rc<dyn IMenu> {
        self.get_menu(&menu_ids::comment_actions(), context_key_service, None)
    }

    /// `getCommentThreadTitleContextActions(contextKeyService)`.
    pub fn get_comment_thread_title_context_actions(&self, context_key_service: &dyn IContextKeyService) -> Vec<MenuAction> {
        self.get_actions(
            &menu_ids::comment_thread_title_context(),
            context_key_service,
            &MenuActionOptions { should_forward_args: true, ..MenuActionOptions::default() },
        )
    }

    fn get_menu(&self, menu_id: &MenuId, context_key_service: Rc<dyn IContextKeyService>, options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        self.menu_service.create_menu(menu_id, context_key_service, options)
    }

    fn get_actions(&self, menu_id: &MenuId, context_key_service: &dyn IContextKeyService, options: &MenuActionOptions) -> Vec<MenuAction> {
        self.menu_service.get_menu_actions(menu_id, context_key_service, Some(options)).into_iter().flat_map(|(_, actions)| actions).collect()
    }
}

impl IDisposable for CommentMenus {
    fn dispose(&self) {}
}
