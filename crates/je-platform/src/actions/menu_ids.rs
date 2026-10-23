//! Os `MenuId` embutidos de `actions.ts` (gerados da fonte do VS Code 1.143.1): uma função por `MenuId.Nome`, com o nome
//! em snake_case (`MenuId.CommandPalette` é `menu_ids::command_palette()`), e a tabela `(nome, identificador)` na ordem
//! da fonte.

use super::MenuId;

/// `(nome da constante no TypeScript, identificador)`, na ordem da fonte.
pub const BUILTIN_MENU_IDS: &[(&str, &str)] = &[
    ("CommandPalette", "CommandPalette"),
    ("DebugBreakpointsContext", "DebugBreakpointsContext"),
    ("DebugCallStackContext", "DebugCallStackContext"),
    ("DebugConsoleContext", "DebugConsoleContext"),
    ("DebugVariablesContext", "DebugVariablesContext"),
    ("NotebookVariablesContext", "NotebookVariablesContext"),
    ("DebugHoverContext", "DebugHoverContext"),
    ("DebugWatchContext", "DebugWatchContext"),
    ("DebugToolBar", "DebugToolBar"),
    ("DebugToolBarStop", "DebugToolBarStop"),
    ("DebugDisassemblyContext", "DebugDisassemblyContext"),
    ("DebugCallStackToolbar", "DebugCallStackToolbar"),
    ("DebugCreateConfiguration", "DebugCreateConfiguration"),
    ("DebugScopesContext", "DebugScopesContext"),
    ("EditorContext", "EditorContext"),
    ("SimpleEditorContext", "SimpleEditorContext"),
    ("EditorContent", "EditorContent"),
    ("EditorLineNumberContext", "EditorLineNumberContext"),
    ("EditorContextCopy", "EditorContextCopy"),
    ("EditorContextPeek", "EditorContextPeek"),
    ("EditorContextShare", "EditorContextShare"),
    ("EditorTitle", "EditorTitle"),
    ("EditorTitleLayout", "EditorTitleLayout"),
    ("ModalEditorTitle", "ModalEditorTitle"),
    ("ModalEditorTitleContext", "ModalEditorTitleContext"),
    ("ModalEditorEditorTitle", "ModalEditorEditorTitle"),
    ("CompactWindowEditorTitle", "CompactWindowEditorTitle"),
    ("EditorTitleRun", "EditorTitleRun"),
    ("EditorTitleContext", "EditorTitleContext"),
    ("EditorTitleContextShare", "EditorTitleContextShare"),
    ("EmptyEditorGroup", "EmptyEditorGroup"),
    ("EmptyEditorGroupContext", "EmptyEditorGroupContext"),
    ("EditorGroupWatermarkToolbar", "EditorGroupWatermarkToolbar"),
    ("EditorTabsBarContext", "EditorTabsBarContext"),
    ("EditorTabsBarShowTabsSubmenu", "EditorTabsBarShowTabsSubmenu"),
    ("EditorTabsBarShowTabsZenModeSubmenu", "EditorTabsBarShowTabsZenModeSubmenu"),
    ("EditorActionsPositionSubmenu", "EditorActionsPositionSubmenu"),
    ("DiffEditorViewSubmenu", "DiffEditorViewSubmenu"),
    ("EditorRenderWhitespaceSubmenu", "EditorRenderWhitespaceSubmenu"),
    ("EditorSplitMoveSubmenu", "EditorSplitMoveSubmenu"),
    ("ExplorerContext", "ExplorerContext"),
    ("ExplorerContextShare", "ExplorerContextShare"),
    ("ExtensionContext", "ExtensionContext"),
    ("ExtensionEditorContextMenu", "ExtensionEditorContextMenu"),
    ("GlobalActivity", "GlobalActivity"),
    ("CommandCenter", "CommandCenter"),
    ("CommandCenterCenter", "CommandCenterCenter"),
    ("LayoutControlMenuSubmenu", "LayoutControlMenuSubmenu"),
    ("LayoutControlMenu", "LayoutControlMenu"),
    ("MenubarMainMenu", "MenubarMainMenu"),
    ("MenubarAppearanceMenu", "MenubarAppearanceMenu"),
    ("MenubarDebugMenu", "MenubarDebugMenu"),
    ("MenubarEditMenu", "MenubarEditMenu"),
    ("MenubarCopy", "MenubarCopy"),
    ("MenubarFileMenu", "MenubarFileMenu"),
    ("MenubarGoMenu", "MenubarGoMenu"),
    ("MenubarHelpMenu", "MenubarHelpMenu"),
    ("MenubarLayoutMenu", "MenubarLayoutMenu"),
    ("MenubarNewBreakpointMenu", "MenubarNewBreakpointMenu"),
    ("PanelAlignmentMenu", "PanelAlignmentMenu"),
    ("PanelPositionMenu", "PanelPositionMenu"),
    ("ActivityBarPositionMenu", "ActivityBarPositionMenu"),
    ("NotificationsCenterPositionMenu", "NotificationsCenterPositionMenu"),
    ("MenubarPreferencesMenu", "MenubarPreferencesMenu"),
    ("MenubarRecentMenu", "MenubarRecentMenu"),
    ("MenubarSelectionMenu", "MenubarSelectionMenu"),
    ("MenubarShare", "MenubarShare"),
    ("MenubarSwitchEditorMenu", "MenubarSwitchEditorMenu"),
    ("MenubarSwitchGroupMenu", "MenubarSwitchGroupMenu"),
    ("MenubarTerminalMenu", "MenubarTerminalMenu"),
    ("MenubarTerminalSuggestStatusMenu", "MenubarTerminalSuggestStatusMenu"),
    ("MenubarViewMenu", "MenubarViewMenu"),
    ("MenubarHomeMenu", "MenubarHomeMenu"),
    ("OpenEditorsContext", "OpenEditorsContext"),
    ("OpenEditorsContextShare", "OpenEditorsContextShare"),
    ("ProblemsPanelContext", "ProblemsPanelContext"),
    ("SCMInputBox", "SCMInputBox"),
    ("SCMChangeContext", "SCMChangeContext"),
    ("SCMResourceContext", "SCMResourceContext"),
    ("SCMResourceContextShare", "SCMResourceContextShare"),
    ("SCMResourceFolderContext", "SCMResourceFolderContext"),
    ("SCMResourceGroupContext", "SCMResourceGroupContext"),
    ("SCMSourceControl", "SCMSourceControl"),
    ("SCMSourceControlInline", "SCMSourceControlInline"),
    ("SCMSourceControlTitle", "SCMSourceControlTitle"),
    ("SCMHistoryTitle", "SCMHistoryTitle"),
    ("SCMHistoryItemContext", "SCMHistoryItemContext"),
    ("SCMHistoryItemChangeContext", "SCMHistoryItemChangeContext"),
    ("SCMHistoryItemRefContext", "SCMHistoryItemRefContext"),
    ("SCMArtifactGroupContext", "SCMArtifactGroupContext"),
    ("SCMArtifactContext", "SCMArtifactContext"),
    ("SCMQuickDiffDecorations", "SCMQuickDiffDecorations"),
    ("SCMTitle", "SCMTitle"),
    ("SearchContext", "SearchContext"),
    ("SearchActionMenu", "SearchActionContext"),
    ("StatusBarWindowIndicatorMenu", "StatusBarWindowIndicatorMenu"),
    ("StatusBarRemoteIndicatorMenu", "StatusBarRemoteIndicatorMenu"),
    ("StickyScrollContext", "StickyScrollContext"),
    ("TestItem", "TestItem"),
    ("TestItemGutter", "TestItemGutter"),
    ("TestProfilesContext", "TestProfilesContext"),
    ("TestMessageContext", "TestMessageContext"),
    ("TestMessageContent", "TestMessageContent"),
    ("TestPeekElement", "TestPeekElement"),
    ("TestPeekTitle", "TestPeekTitle"),
    ("TestCallStack", "TestCallStack"),
    ("TestCoverageFilterItem", "TestCoverageFilterItem"),
    ("TouchBarContext", "TouchBarContext"),
    ("TitleBar", "TitleBar"),
    ("TitleBarAdjacentCenter", "TitleBarAdjacentCenter"),
    ("TitleBarUpdate", "TitleBarUpdate"),
    ("TitleBarContext", "TitleBarContext"),
    ("TitleBarTitleContext", "TitleBarTitleContext"),
    ("TunnelContext", "TunnelContext"),
    ("TunnelPrivacy", "TunnelPrivacy"),
    ("TunnelProtocol", "TunnelProtocol"),
    ("TunnelPortInline", "TunnelInline"),
    ("TunnelTitle", "TunnelTitle"),
    ("TunnelLocalAddressInline", "TunnelLocalAddressInline"),
    ("TunnelOriginInline", "TunnelOriginInline"),
    ("ViewItemContext", "ViewItemContext"),
    ("ViewContainerTitle", "ViewContainerTitle"),
    ("ViewContainerTitleContext", "ViewContainerTitleContext"),
    ("ViewTitle", "ViewTitle"),
    ("ViewTitleContext", "ViewTitleContext"),
    ("CommentEditorActions", "CommentEditorActions"),
    ("CommentThreadTitle", "CommentThreadTitle"),
    ("CommentThreadActions", "CommentThreadActions"),
    ("CommentThreadAdditionalActions", "CommentThreadAdditionalActions"),
    ("CommentThreadTitleContext", "CommentThreadTitleContext"),
    ("CommentThreadCommentContext", "CommentThreadCommentContext"),
    ("CommentTitle", "CommentTitle"),
    ("CommentActions", "CommentActions"),
    ("CommentsViewThreadActions", "CommentsViewThreadActions"),
    ("InteractiveToolbar", "InteractiveToolbar"),
    ("InteractiveCellTitle", "InteractiveCellTitle"),
    ("InteractiveCellDelete", "InteractiveCellDelete"),
    ("InteractiveCellExecute", "InteractiveCellExecute"),
    ("InteractiveInputExecute", "InteractiveInputExecute"),
    ("InteractiveInputConfig", "InteractiveInputConfig"),
    ("ReplInputExecute", "ReplInputExecute"),
    ("IssueReporter", "IssueReporter"),
    ("NotebookToolbar", "NotebookToolbar"),
    ("NotebookToolbarContext", "NotebookToolbarContext"),
    ("NotebookStickyScrollContext", "NotebookStickyScrollContext"),
    ("NotebookCellTitle", "NotebookCellTitle"),
    ("NotebookCellDelete", "NotebookCellDelete"),
    ("NotebookCellInsert", "NotebookCellInsert"),
    ("NotebookCellBetween", "NotebookCellBetween"),
    ("NotebookCellListTop", "NotebookCellTop"),
    ("NotebookCellExecute", "NotebookCellExecute"),
    ("NotebookCellExecuteGoTo", "NotebookCellExecuteGoTo"),
    ("NotebookCellExecutePrimary", "NotebookCellExecutePrimary"),
    ("NotebookDiffCellInputTitle", "NotebookDiffCellInputTitle"),
    ("NotebookDiffDocumentMetadata", "NotebookDiffDocumentMetadata"),
    ("NotebookDiffCellMetadataTitle", "NotebookDiffCellMetadataTitle"),
    ("NotebookDiffCellOutputsTitle", "NotebookDiffCellOutputsTitle"),
    ("NotebookOutputToolbar", "NotebookOutputToolbar"),
    ("NotebookOutlineFilter", "NotebookOutlineFilter"),
    ("NotebookOutlineActionMenu", "NotebookOutlineActionMenu"),
    ("NotebookEditorLayoutConfigure", "NotebookEditorLayoutConfigure"),
    ("NotebookKernelSource", "NotebookKernelSource"),
    ("BulkEditTitle", "BulkEditTitle"),
    ("BulkEditContext", "BulkEditContext"),
    ("TimelineItemContext", "TimelineItemContext"),
    ("TimelineTitle", "TimelineTitle"),
    ("TimelineTitleContext", "TimelineTitleContext"),
    ("TimelineFilterSubMenu", "TimelineFilterSubMenu"),
    ("AccountsContext", "AccountsContext"),
    ("SidebarTitle", "SidebarTitle"),
    ("PanelTitle", "PanelTitle"),
    ("AuxiliaryBarTitle", "AuxiliaryBarTitle"),
    ("TerminalInstanceContext", "TerminalInstanceContext"),
    ("TerminalEditorInstanceContext", "TerminalEditorInstanceContext"),
    ("TerminalNewDropdownContext", "TerminalNewDropdownContext"),
    ("TerminalTabContext", "TerminalTabContext"),
    ("TerminalTabEmptyAreaContext", "TerminalTabEmptyAreaContext"),
    ("TerminalStickyScrollContext", "TerminalStickyScrollContext"),
    ("WebviewContext", "WebviewContext"),
    ("InlineCompletionsActions", "InlineCompletionsActions"),
    ("InlineEditsActions", "InlineEditsActions"),
    ("NewFile", "NewFile"),
    ("MergeInput1Toolbar", "MergeToolbar1Toolbar"),
    ("MergeInput2Toolbar", "MergeToolbar2Toolbar"),
    ("MergeBaseToolbar", "MergeBaseToolbar"),
    ("MergeInputResultToolbar", "MergeToolbarResultToolbar"),
    ("InlineSuggestionToolbar", "InlineSuggestionToolbar"),
    ("InlineEditToolbar", "InlineEditToolbar"),
    ("ChatContext", "ChatContext"),
    ("ChatCodeBlock", "ChatCodeblock"),
    ("ChatCompareBlock", "ChatCompareBlock"),
    ("ChatMessageTitle", "ChatMessageTitle"),
    ("ChatWelcomeContext", "ChatWelcomeContext"),
    ("ChatMessageFooter", "ChatMessageFooter"),
    ("ChatSubagentContent", "ChatSubagentContent"),
    ("ChatExecute", "ChatExecute"),
    ("ChatExecuteQueue", "ChatExecuteQueue"),
    ("ChatInput", "ChatInput"),
    ("ChatInputSecondary", "ChatInputSecondary"),
    ("ChatInputStatus", "ChatInputStatus"),
    ("ChatInputSide", "ChatInputSide"),
    ("AutomationsDialogInput", "AutomationsDialogInput"),
    ("ChatModePicker", "ChatModePicker"),
    ("ChatEditingWidgetToolbar", "ChatEditingWidgetToolbar"),
    ("ChatEditingSessionChangesToolbar", "ChatEditingSessionChangesToolbar"),
    ("ChatEditingSessionTitleToolbar", "ChatEditingSessionTitleToolbar"),
    ("ChatEditingSessionChangesVersionsSubmenu", "ChatEditingSessionChangesVersionsSubmenu"),
    ("ChatEditingSessionChangesFileHeaderToolbar", "ChatEditingSessionChangesFileHeaderToolbar"),
    ("ChatEditingSessionChangesFileHeaderRightToolbar", "ChatEditingSessionChangesFileHeaderRightToolbar"),
    ("ChatEditingEditorContent", "ChatEditingEditorContent"),
    ("ChatEditingEditorHunk", "ChatEditingEditorHunk"),
    ("ChatEditingDeletedNotebookCell", "ChatEditingDeletedNotebookCell"),
    ("ChatInputAttachmentToolbar", "ChatInputAttachmentToolbar"),
    ("ChatEditingWidgetModifiedFilesToolbar", "ChatEditingWidgetModifiedFilesToolbar"),
    ("ChatInputResourceAttachmentContext", "ChatInputResourceAttachmentContext"),
    ("ChatInputSymbolAttachmentContext", "ChatInputSymbolAttachmentContext"),
    ("ChatInlineResourceAnchorContext", "ChatInlineResourceAnchorContext"),
    ("ChatInlineSymbolAnchorContext", "ChatInlineSymbolAnchorContext"),
    ("ChatMessageCheckpoint", "ChatMessageCheckpoint"),
    ("ChatMessageRestoreCheckpoint", "ChatMessageRestoreCheckpoint"),
    ("ChatNewMenu", "ChatNewMenu"),
    ("ChatEditingCodeBlockContext", "ChatEditingCodeBlockContext"),
    ("ChatTitleBarMenu", "ChatTitleBarMenu"),
    ("ChatAttachmentsContext", "ChatAttachmentsContext"),
    ("ChatTipContext", "ChatTipContext"),
    ("ChatTipToolbar", "ChatTipToolbar"),
    ("ChatToolOutputResourceToolbar", "ChatToolOutputResourceToolbar"),
    ("ChatTextEditorMenu", "ChatTextEditorMenu"),
    ("ChatToolOutputResourceContext", "ChatToolOutputResourceContext"),
    ("ChatMultiDiffContext", "ChatMultiDiffContext"),
    ("ChatConfirmationMenu", "ChatConfirmationMenu"),
    ("ChatEditorInlineMenu", "ChatEditorInlineGutter"),
    ("ChatEditorInlineExecute", "ChatEditorInputExecute"),
    ("ChatEditorInlineInputSide", "ChatEditorInputSide"),
    ("InlineChatEditorAffordance", "InlineChatEditorAffordance"),
    ("AccessibleView", "AccessibleView"),
    ("MultiDiffEditorContent", "MultiDiffEditorContent"),
    ("MultiDiffEditorFileToolbar", "MultiDiffEditorFileToolbar"),
    ("DiffEditorHunkToolbar", "DiffEditorHunkToolbar"),
    ("DiffEditorSelectionToolbar", "DiffEditorSelectionToolbar"),
    ("BrowserNavigationToolbar", "BrowserNavigationToolbar"),
    ("BrowserActionsToolbar", "BrowserActionsToolbar"),
    ("BrowserChatActionsMenu", "BrowserChatActionsMenu"),
    ("BrowserEmulationToolbar", "BrowserEmulationToolbar"),
    ("AgentSessionsViewerFilterSubMenu", "AgentSessionsViewerFilterSubMenu"),
    ("AgentSessionsExternalFilterSubMenu", "AgentSessionsExternalFilterSubMenu"),
    ("AgentSessionsContext", "AgentSessionsContext"),
    ("AgentSessionSectionContext", "AgentSessionSectionContext"),
    ("AgentSessionsCreateSubMenu", "AgentSessionsCreateSubMenu"),
    ("AgentSessionsToolbar", "AgentSessionsToolbar"),
    ("AgentSessionItemToolbar", "AgentSessionItemToolbar"),
    ("AgentSessionSectionToolbar", "AgentSessionSectionToolbar"),
    ("SessionItemContextMenu", "SessionItemContextMenu"),
    ("SessionHeaderContext", "SessionsSessionHeaderContext"),
    ("AgentsTitleBarControlMenu", "AgentsTitleBarControlMenu"),
    ("AgentsChangesToolbar", "AgentsChangesToolbar"),
    ("AgentsChangesPrimaryActionSubMenu", "AgentsChangesPrimaryActionSubMenu"),
    ("AgentsChangeInlineToolbar", "AgentsChangeInlineToolbar"),
    ("ChatViewSessionTitleNavigationToolbar", "ChatViewSessionTitleNavigationToolbar"),
    ("ChatViewSessionTitleToolbar", "ChatViewSessionTitleToolbar"),
    ("ChatContextUsageActions", "ChatContextUsageActions"),
    ("MarkerHoverStatusBar", "MarkerHoverParticipant.StatusBar"),
];

/// `MenuId.CommandPalette`.
pub fn command_palette() -> MenuId {
    MenuId::for_id("CommandPalette")
}

/// `MenuId.DebugBreakpointsContext`.
pub fn debug_breakpoints_context() -> MenuId {
    MenuId::for_id("DebugBreakpointsContext")
}

/// `MenuId.DebugCallStackContext`.
pub fn debug_call_stack_context() -> MenuId {
    MenuId::for_id("DebugCallStackContext")
}

/// `MenuId.DebugConsoleContext`.
pub fn debug_console_context() -> MenuId {
    MenuId::for_id("DebugConsoleContext")
}

/// `MenuId.DebugVariablesContext`.
pub fn debug_variables_context() -> MenuId {
    MenuId::for_id("DebugVariablesContext")
}

/// `MenuId.NotebookVariablesContext`.
pub fn notebook_variables_context() -> MenuId {
    MenuId::for_id("NotebookVariablesContext")
}

/// `MenuId.DebugHoverContext`.
pub fn debug_hover_context() -> MenuId {
    MenuId::for_id("DebugHoverContext")
}

/// `MenuId.DebugWatchContext`.
pub fn debug_watch_context() -> MenuId {
    MenuId::for_id("DebugWatchContext")
}

/// `MenuId.DebugToolBar`.
pub fn debug_tool_bar() -> MenuId {
    MenuId::for_id("DebugToolBar")
}

/// `MenuId.DebugToolBarStop`.
pub fn debug_tool_bar_stop() -> MenuId {
    MenuId::for_id("DebugToolBarStop")
}

/// `MenuId.DebugDisassemblyContext`.
pub fn debug_disassembly_context() -> MenuId {
    MenuId::for_id("DebugDisassemblyContext")
}

/// `MenuId.DebugCallStackToolbar`.
pub fn debug_call_stack_toolbar() -> MenuId {
    MenuId::for_id("DebugCallStackToolbar")
}

/// `MenuId.DebugCreateConfiguration`.
pub fn debug_create_configuration() -> MenuId {
    MenuId::for_id("DebugCreateConfiguration")
}

/// `MenuId.DebugScopesContext`.
pub fn debug_scopes_context() -> MenuId {
    MenuId::for_id("DebugScopesContext")
}

/// `MenuId.EditorContext`.
pub fn editor_context() -> MenuId {
    MenuId::for_id("EditorContext")
}

/// `MenuId.SimpleEditorContext`.
pub fn simple_editor_context() -> MenuId {
    MenuId::for_id("SimpleEditorContext")
}

/// `MenuId.EditorContent`.
pub fn editor_content() -> MenuId {
    MenuId::for_id("EditorContent")
}

/// `MenuId.EditorLineNumberContext`.
pub fn editor_line_number_context() -> MenuId {
    MenuId::for_id("EditorLineNumberContext")
}

/// `MenuId.EditorContextCopy`.
pub fn editor_context_copy() -> MenuId {
    MenuId::for_id("EditorContextCopy")
}

/// `MenuId.EditorContextPeek`.
pub fn editor_context_peek() -> MenuId {
    MenuId::for_id("EditorContextPeek")
}

/// `MenuId.EditorContextShare`.
pub fn editor_context_share() -> MenuId {
    MenuId::for_id("EditorContextShare")
}

/// `MenuId.EditorTitle`.
pub fn editor_title() -> MenuId {
    MenuId::for_id("EditorTitle")
}

/// `MenuId.EditorTitleLayout`.
pub fn editor_title_layout() -> MenuId {
    MenuId::for_id("EditorTitleLayout")
}

/// `MenuId.ModalEditorTitle`.
pub fn modal_editor_title() -> MenuId {
    MenuId::for_id("ModalEditorTitle")
}

/// `MenuId.ModalEditorTitleContext`.
pub fn modal_editor_title_context() -> MenuId {
    MenuId::for_id("ModalEditorTitleContext")
}

/// `MenuId.ModalEditorEditorTitle`.
pub fn modal_editor_editor_title() -> MenuId {
    MenuId::for_id("ModalEditorEditorTitle")
}

/// `MenuId.CompactWindowEditorTitle`.
pub fn compact_window_editor_title() -> MenuId {
    MenuId::for_id("CompactWindowEditorTitle")
}

/// `MenuId.EditorTitleRun`.
pub fn editor_title_run() -> MenuId {
    MenuId::for_id("EditorTitleRun")
}

/// `MenuId.EditorTitleContext`.
pub fn editor_title_context() -> MenuId {
    MenuId::for_id("EditorTitleContext")
}

/// `MenuId.EditorTitleContextShare`.
pub fn editor_title_context_share() -> MenuId {
    MenuId::for_id("EditorTitleContextShare")
}

/// `MenuId.EmptyEditorGroup`.
pub fn empty_editor_group() -> MenuId {
    MenuId::for_id("EmptyEditorGroup")
}

/// `MenuId.EmptyEditorGroupContext`.
pub fn empty_editor_group_context() -> MenuId {
    MenuId::for_id("EmptyEditorGroupContext")
}

/// `MenuId.EditorGroupWatermarkToolbar`.
pub fn editor_group_watermark_toolbar() -> MenuId {
    MenuId::for_id("EditorGroupWatermarkToolbar")
}

/// `MenuId.EditorTabsBarContext`.
pub fn editor_tabs_bar_context() -> MenuId {
    MenuId::for_id("EditorTabsBarContext")
}

/// `MenuId.EditorTabsBarShowTabsSubmenu`.
pub fn editor_tabs_bar_show_tabs_submenu() -> MenuId {
    MenuId::for_id("EditorTabsBarShowTabsSubmenu")
}

/// `MenuId.EditorTabsBarShowTabsZenModeSubmenu`.
pub fn editor_tabs_bar_show_tabs_zen_mode_submenu() -> MenuId {
    MenuId::for_id("EditorTabsBarShowTabsZenModeSubmenu")
}

/// `MenuId.EditorActionsPositionSubmenu`.
pub fn editor_actions_position_submenu() -> MenuId {
    MenuId::for_id("EditorActionsPositionSubmenu")
}

/// `MenuId.DiffEditorViewSubmenu`.
pub fn diff_editor_view_submenu() -> MenuId {
    MenuId::for_id("DiffEditorViewSubmenu")
}

/// `MenuId.EditorRenderWhitespaceSubmenu`.
pub fn editor_render_whitespace_submenu() -> MenuId {
    MenuId::for_id("EditorRenderWhitespaceSubmenu")
}

/// `MenuId.EditorSplitMoveSubmenu`.
pub fn editor_split_move_submenu() -> MenuId {
    MenuId::for_id("EditorSplitMoveSubmenu")
}

/// `MenuId.ExplorerContext`.
pub fn explorer_context() -> MenuId {
    MenuId::for_id("ExplorerContext")
}

/// `MenuId.ExplorerContextShare`.
pub fn explorer_context_share() -> MenuId {
    MenuId::for_id("ExplorerContextShare")
}

/// `MenuId.ExtensionContext`.
pub fn extension_context() -> MenuId {
    MenuId::for_id("ExtensionContext")
}

/// `MenuId.ExtensionEditorContextMenu`.
pub fn extension_editor_context_menu() -> MenuId {
    MenuId::for_id("ExtensionEditorContextMenu")
}

/// `MenuId.GlobalActivity`.
pub fn global_activity() -> MenuId {
    MenuId::for_id("GlobalActivity")
}

/// `MenuId.CommandCenter`.
pub fn command_center() -> MenuId {
    MenuId::for_id("CommandCenter")
}

/// `MenuId.CommandCenterCenter`.
pub fn command_center_center() -> MenuId {
    MenuId::for_id("CommandCenterCenter")
}

/// `MenuId.LayoutControlMenuSubmenu`.
pub fn layout_control_menu_submenu() -> MenuId {
    MenuId::for_id("LayoutControlMenuSubmenu")
}

/// `MenuId.LayoutControlMenu`.
pub fn layout_control_menu() -> MenuId {
    MenuId::for_id("LayoutControlMenu")
}

/// `MenuId.MenubarMainMenu`.
pub fn menubar_main_menu() -> MenuId {
    MenuId::for_id("MenubarMainMenu")
}

/// `MenuId.MenubarAppearanceMenu`.
pub fn menubar_appearance_menu() -> MenuId {
    MenuId::for_id("MenubarAppearanceMenu")
}

/// `MenuId.MenubarDebugMenu`.
pub fn menubar_debug_menu() -> MenuId {
    MenuId::for_id("MenubarDebugMenu")
}

/// `MenuId.MenubarEditMenu`.
pub fn menubar_edit_menu() -> MenuId {
    MenuId::for_id("MenubarEditMenu")
}

/// `MenuId.MenubarCopy`.
pub fn menubar_copy() -> MenuId {
    MenuId::for_id("MenubarCopy")
}

/// `MenuId.MenubarFileMenu`.
pub fn menubar_file_menu() -> MenuId {
    MenuId::for_id("MenubarFileMenu")
}

/// `MenuId.MenubarGoMenu`.
pub fn menubar_go_menu() -> MenuId {
    MenuId::for_id("MenubarGoMenu")
}

/// `MenuId.MenubarHelpMenu`.
pub fn menubar_help_menu() -> MenuId {
    MenuId::for_id("MenubarHelpMenu")
}

/// `MenuId.MenubarLayoutMenu`.
pub fn menubar_layout_menu() -> MenuId {
    MenuId::for_id("MenubarLayoutMenu")
}

/// `MenuId.MenubarNewBreakpointMenu`.
pub fn menubar_new_breakpoint_menu() -> MenuId {
    MenuId::for_id("MenubarNewBreakpointMenu")
}

/// `MenuId.PanelAlignmentMenu`.
pub fn panel_alignment_menu() -> MenuId {
    MenuId::for_id("PanelAlignmentMenu")
}

/// `MenuId.PanelPositionMenu`.
pub fn panel_position_menu() -> MenuId {
    MenuId::for_id("PanelPositionMenu")
}

/// `MenuId.ActivityBarPositionMenu`.
pub fn activity_bar_position_menu() -> MenuId {
    MenuId::for_id("ActivityBarPositionMenu")
}

/// `MenuId.NotificationsCenterPositionMenu`.
pub fn notifications_center_position_menu() -> MenuId {
    MenuId::for_id("NotificationsCenterPositionMenu")
}

/// `MenuId.MenubarPreferencesMenu`.
pub fn menubar_preferences_menu() -> MenuId {
    MenuId::for_id("MenubarPreferencesMenu")
}

/// `MenuId.MenubarRecentMenu`.
pub fn menubar_recent_menu() -> MenuId {
    MenuId::for_id("MenubarRecentMenu")
}

/// `MenuId.MenubarSelectionMenu`.
pub fn menubar_selection_menu() -> MenuId {
    MenuId::for_id("MenubarSelectionMenu")
}

/// `MenuId.MenubarShare`.
pub fn menubar_share() -> MenuId {
    MenuId::for_id("MenubarShare")
}

/// `MenuId.MenubarSwitchEditorMenu`.
pub fn menubar_switch_editor_menu() -> MenuId {
    MenuId::for_id("MenubarSwitchEditorMenu")
}

/// `MenuId.MenubarSwitchGroupMenu`.
pub fn menubar_switch_group_menu() -> MenuId {
    MenuId::for_id("MenubarSwitchGroupMenu")
}

/// `MenuId.MenubarTerminalMenu`.
pub fn menubar_terminal_menu() -> MenuId {
    MenuId::for_id("MenubarTerminalMenu")
}

/// `MenuId.MenubarTerminalSuggestStatusMenu`.
pub fn menubar_terminal_suggest_status_menu() -> MenuId {
    MenuId::for_id("MenubarTerminalSuggestStatusMenu")
}

/// `MenuId.MenubarViewMenu`.
pub fn menubar_view_menu() -> MenuId {
    MenuId::for_id("MenubarViewMenu")
}

/// `MenuId.MenubarHomeMenu`.
pub fn menubar_home_menu() -> MenuId {
    MenuId::for_id("MenubarHomeMenu")
}

/// `MenuId.OpenEditorsContext`.
pub fn open_editors_context() -> MenuId {
    MenuId::for_id("OpenEditorsContext")
}

/// `MenuId.OpenEditorsContextShare`.
pub fn open_editors_context_share() -> MenuId {
    MenuId::for_id("OpenEditorsContextShare")
}

/// `MenuId.ProblemsPanelContext`.
pub fn problems_panel_context() -> MenuId {
    MenuId::for_id("ProblemsPanelContext")
}

/// `MenuId.SCMInputBox`.
pub fn scm_input_box() -> MenuId {
    MenuId::for_id("SCMInputBox")
}

/// `MenuId.SCMChangeContext`.
pub fn scm_change_context() -> MenuId {
    MenuId::for_id("SCMChangeContext")
}

/// `MenuId.SCMResourceContext`.
pub fn scm_resource_context() -> MenuId {
    MenuId::for_id("SCMResourceContext")
}

/// `MenuId.SCMResourceContextShare`.
pub fn scm_resource_context_share() -> MenuId {
    MenuId::for_id("SCMResourceContextShare")
}

/// `MenuId.SCMResourceFolderContext`.
pub fn scm_resource_folder_context() -> MenuId {
    MenuId::for_id("SCMResourceFolderContext")
}

/// `MenuId.SCMResourceGroupContext`.
pub fn scm_resource_group_context() -> MenuId {
    MenuId::for_id("SCMResourceGroupContext")
}

/// `MenuId.SCMSourceControl`.
pub fn scm_source_control() -> MenuId {
    MenuId::for_id("SCMSourceControl")
}

/// `MenuId.SCMSourceControlInline`.
pub fn scm_source_control_inline() -> MenuId {
    MenuId::for_id("SCMSourceControlInline")
}

/// `MenuId.SCMSourceControlTitle`.
pub fn scm_source_control_title() -> MenuId {
    MenuId::for_id("SCMSourceControlTitle")
}

/// `MenuId.SCMHistoryTitle`.
pub fn scm_history_title() -> MenuId {
    MenuId::for_id("SCMHistoryTitle")
}

/// `MenuId.SCMHistoryItemContext`.
pub fn scm_history_item_context() -> MenuId {
    MenuId::for_id("SCMHistoryItemContext")
}

/// `MenuId.SCMHistoryItemChangeContext`.
pub fn scm_history_item_change_context() -> MenuId {
    MenuId::for_id("SCMHistoryItemChangeContext")
}

/// `MenuId.SCMHistoryItemRefContext`.
pub fn scm_history_item_ref_context() -> MenuId {
    MenuId::for_id("SCMHistoryItemRefContext")
}

/// `MenuId.SCMArtifactGroupContext`.
pub fn scm_artifact_group_context() -> MenuId {
    MenuId::for_id("SCMArtifactGroupContext")
}

/// `MenuId.SCMArtifactContext`.
pub fn scm_artifact_context() -> MenuId {
    MenuId::for_id("SCMArtifactContext")
}

/// `MenuId.SCMQuickDiffDecorations`.
pub fn scm_quick_diff_decorations() -> MenuId {
    MenuId::for_id("SCMQuickDiffDecorations")
}

/// `MenuId.SCMTitle`.
pub fn scm_title() -> MenuId {
    MenuId::for_id("SCMTitle")
}

/// `MenuId.SearchContext`.
pub fn search_context() -> MenuId {
    MenuId::for_id("SearchContext")
}

/// `MenuId.SearchActionMenu`.
pub fn search_action_menu() -> MenuId {
    MenuId::for_id("SearchActionContext")
}

/// `MenuId.StatusBarWindowIndicatorMenu`.
pub fn status_bar_window_indicator_menu() -> MenuId {
    MenuId::for_id("StatusBarWindowIndicatorMenu")
}

/// `MenuId.StatusBarRemoteIndicatorMenu`.
pub fn status_bar_remote_indicator_menu() -> MenuId {
    MenuId::for_id("StatusBarRemoteIndicatorMenu")
}

/// `MenuId.StickyScrollContext`.
pub fn sticky_scroll_context() -> MenuId {
    MenuId::for_id("StickyScrollContext")
}

/// `MenuId.TestItem`.
pub fn test_item() -> MenuId {
    MenuId::for_id("TestItem")
}

/// `MenuId.TestItemGutter`.
pub fn test_item_gutter() -> MenuId {
    MenuId::for_id("TestItemGutter")
}

/// `MenuId.TestProfilesContext`.
pub fn test_profiles_context() -> MenuId {
    MenuId::for_id("TestProfilesContext")
}

/// `MenuId.TestMessageContext`.
pub fn test_message_context() -> MenuId {
    MenuId::for_id("TestMessageContext")
}

/// `MenuId.TestMessageContent`.
pub fn test_message_content() -> MenuId {
    MenuId::for_id("TestMessageContent")
}

/// `MenuId.TestPeekElement`.
pub fn test_peek_element() -> MenuId {
    MenuId::for_id("TestPeekElement")
}

/// `MenuId.TestPeekTitle`.
pub fn test_peek_title() -> MenuId {
    MenuId::for_id("TestPeekTitle")
}

/// `MenuId.TestCallStack`.
pub fn test_call_stack() -> MenuId {
    MenuId::for_id("TestCallStack")
}

/// `MenuId.TestCoverageFilterItem`.
pub fn test_coverage_filter_item() -> MenuId {
    MenuId::for_id("TestCoverageFilterItem")
}

/// `MenuId.TouchBarContext`.
pub fn touch_bar_context() -> MenuId {
    MenuId::for_id("TouchBarContext")
}

/// `MenuId.TitleBar`.
pub fn title_bar() -> MenuId {
    MenuId::for_id("TitleBar")
}

/// `MenuId.TitleBarAdjacentCenter`.
pub fn title_bar_adjacent_center() -> MenuId {
    MenuId::for_id("TitleBarAdjacentCenter")
}

/// `MenuId.TitleBarUpdate`.
pub fn title_bar_update() -> MenuId {
    MenuId::for_id("TitleBarUpdate")
}

/// `MenuId.TitleBarContext`.
pub fn title_bar_context() -> MenuId {
    MenuId::for_id("TitleBarContext")
}

/// `MenuId.TitleBarTitleContext`.
pub fn title_bar_title_context() -> MenuId {
    MenuId::for_id("TitleBarTitleContext")
}

/// `MenuId.TunnelContext`.
pub fn tunnel_context() -> MenuId {
    MenuId::for_id("TunnelContext")
}

/// `MenuId.TunnelPrivacy`.
pub fn tunnel_privacy() -> MenuId {
    MenuId::for_id("TunnelPrivacy")
}

/// `MenuId.TunnelProtocol`.
pub fn tunnel_protocol() -> MenuId {
    MenuId::for_id("TunnelProtocol")
}

/// `MenuId.TunnelPortInline`.
pub fn tunnel_port_inline() -> MenuId {
    MenuId::for_id("TunnelInline")
}

/// `MenuId.TunnelTitle`.
pub fn tunnel_title() -> MenuId {
    MenuId::for_id("TunnelTitle")
}

/// `MenuId.TunnelLocalAddressInline`.
pub fn tunnel_local_address_inline() -> MenuId {
    MenuId::for_id("TunnelLocalAddressInline")
}

/// `MenuId.TunnelOriginInline`.
pub fn tunnel_origin_inline() -> MenuId {
    MenuId::for_id("TunnelOriginInline")
}

/// `MenuId.ViewItemContext`.
pub fn view_item_context() -> MenuId {
    MenuId::for_id("ViewItemContext")
}

/// `MenuId.ViewContainerTitle`.
pub fn view_container_title() -> MenuId {
    MenuId::for_id("ViewContainerTitle")
}

/// `MenuId.ViewContainerTitleContext`.
pub fn view_container_title_context() -> MenuId {
    MenuId::for_id("ViewContainerTitleContext")
}

/// `MenuId.ViewTitle`.
pub fn view_title() -> MenuId {
    MenuId::for_id("ViewTitle")
}

/// `MenuId.ViewTitleContext`.
pub fn view_title_context() -> MenuId {
    MenuId::for_id("ViewTitleContext")
}

/// `MenuId.CommentEditorActions`.
pub fn comment_editor_actions() -> MenuId {
    MenuId::for_id("CommentEditorActions")
}

/// `MenuId.CommentThreadTitle`.
pub fn comment_thread_title() -> MenuId {
    MenuId::for_id("CommentThreadTitle")
}

/// `MenuId.CommentThreadActions`.
pub fn comment_thread_actions() -> MenuId {
    MenuId::for_id("CommentThreadActions")
}

/// `MenuId.CommentThreadAdditionalActions`.
pub fn comment_thread_additional_actions() -> MenuId {
    MenuId::for_id("CommentThreadAdditionalActions")
}

/// `MenuId.CommentThreadTitleContext`.
pub fn comment_thread_title_context() -> MenuId {
    MenuId::for_id("CommentThreadTitleContext")
}

/// `MenuId.CommentThreadCommentContext`.
pub fn comment_thread_comment_context() -> MenuId {
    MenuId::for_id("CommentThreadCommentContext")
}

/// `MenuId.CommentTitle`.
pub fn comment_title() -> MenuId {
    MenuId::for_id("CommentTitle")
}

/// `MenuId.CommentActions`.
pub fn comment_actions() -> MenuId {
    MenuId::for_id("CommentActions")
}

/// `MenuId.CommentsViewThreadActions`.
pub fn comments_view_thread_actions() -> MenuId {
    MenuId::for_id("CommentsViewThreadActions")
}

/// `MenuId.InteractiveToolbar`.
pub fn interactive_toolbar() -> MenuId {
    MenuId::for_id("InteractiveToolbar")
}

/// `MenuId.InteractiveCellTitle`.
pub fn interactive_cell_title() -> MenuId {
    MenuId::for_id("InteractiveCellTitle")
}

/// `MenuId.InteractiveCellDelete`.
pub fn interactive_cell_delete() -> MenuId {
    MenuId::for_id("InteractiveCellDelete")
}

/// `MenuId.InteractiveCellExecute`.
pub fn interactive_cell_execute() -> MenuId {
    MenuId::for_id("InteractiveCellExecute")
}

/// `MenuId.InteractiveInputExecute`.
pub fn interactive_input_execute() -> MenuId {
    MenuId::for_id("InteractiveInputExecute")
}

/// `MenuId.InteractiveInputConfig`.
pub fn interactive_input_config() -> MenuId {
    MenuId::for_id("InteractiveInputConfig")
}

/// `MenuId.ReplInputExecute`.
pub fn repl_input_execute() -> MenuId {
    MenuId::for_id("ReplInputExecute")
}

/// `MenuId.IssueReporter`.
pub fn issue_reporter() -> MenuId {
    MenuId::for_id("IssueReporter")
}

/// `MenuId.NotebookToolbar`.
pub fn notebook_toolbar() -> MenuId {
    MenuId::for_id("NotebookToolbar")
}

/// `MenuId.NotebookToolbarContext`.
pub fn notebook_toolbar_context() -> MenuId {
    MenuId::for_id("NotebookToolbarContext")
}

/// `MenuId.NotebookStickyScrollContext`.
pub fn notebook_sticky_scroll_context() -> MenuId {
    MenuId::for_id("NotebookStickyScrollContext")
}

/// `MenuId.NotebookCellTitle`.
pub fn notebook_cell_title() -> MenuId {
    MenuId::for_id("NotebookCellTitle")
}

/// `MenuId.NotebookCellDelete`.
pub fn notebook_cell_delete() -> MenuId {
    MenuId::for_id("NotebookCellDelete")
}

/// `MenuId.NotebookCellInsert`.
pub fn notebook_cell_insert() -> MenuId {
    MenuId::for_id("NotebookCellInsert")
}

/// `MenuId.NotebookCellBetween`.
pub fn notebook_cell_between() -> MenuId {
    MenuId::for_id("NotebookCellBetween")
}

/// `MenuId.NotebookCellListTop`.
pub fn notebook_cell_list_top() -> MenuId {
    MenuId::for_id("NotebookCellTop")
}

/// `MenuId.NotebookCellExecute`.
pub fn notebook_cell_execute() -> MenuId {
    MenuId::for_id("NotebookCellExecute")
}

/// `MenuId.NotebookCellExecuteGoTo`.
pub fn notebook_cell_execute_go_to() -> MenuId {
    MenuId::for_id("NotebookCellExecuteGoTo")
}

/// `MenuId.NotebookCellExecutePrimary`.
pub fn notebook_cell_execute_primary() -> MenuId {
    MenuId::for_id("NotebookCellExecutePrimary")
}

/// `MenuId.NotebookDiffCellInputTitle`.
pub fn notebook_diff_cell_input_title() -> MenuId {
    MenuId::for_id("NotebookDiffCellInputTitle")
}

/// `MenuId.NotebookDiffDocumentMetadata`.
pub fn notebook_diff_document_metadata() -> MenuId {
    MenuId::for_id("NotebookDiffDocumentMetadata")
}

/// `MenuId.NotebookDiffCellMetadataTitle`.
pub fn notebook_diff_cell_metadata_title() -> MenuId {
    MenuId::for_id("NotebookDiffCellMetadataTitle")
}

/// `MenuId.NotebookDiffCellOutputsTitle`.
pub fn notebook_diff_cell_outputs_title() -> MenuId {
    MenuId::for_id("NotebookDiffCellOutputsTitle")
}

/// `MenuId.NotebookOutputToolbar`.
pub fn notebook_output_toolbar() -> MenuId {
    MenuId::for_id("NotebookOutputToolbar")
}

/// `MenuId.NotebookOutlineFilter`.
pub fn notebook_outline_filter() -> MenuId {
    MenuId::for_id("NotebookOutlineFilter")
}

/// `MenuId.NotebookOutlineActionMenu`.
pub fn notebook_outline_action_menu() -> MenuId {
    MenuId::for_id("NotebookOutlineActionMenu")
}

/// `MenuId.NotebookEditorLayoutConfigure`.
pub fn notebook_editor_layout_configure() -> MenuId {
    MenuId::for_id("NotebookEditorLayoutConfigure")
}

/// `MenuId.NotebookKernelSource`.
pub fn notebook_kernel_source() -> MenuId {
    MenuId::for_id("NotebookKernelSource")
}

/// `MenuId.BulkEditTitle`.
pub fn bulk_edit_title() -> MenuId {
    MenuId::for_id("BulkEditTitle")
}

/// `MenuId.BulkEditContext`.
pub fn bulk_edit_context() -> MenuId {
    MenuId::for_id("BulkEditContext")
}

/// `MenuId.TimelineItemContext`.
pub fn timeline_item_context() -> MenuId {
    MenuId::for_id("TimelineItemContext")
}

/// `MenuId.TimelineTitle`.
pub fn timeline_title() -> MenuId {
    MenuId::for_id("TimelineTitle")
}

/// `MenuId.TimelineTitleContext`.
pub fn timeline_title_context() -> MenuId {
    MenuId::for_id("TimelineTitleContext")
}

/// `MenuId.TimelineFilterSubMenu`.
pub fn timeline_filter_sub_menu() -> MenuId {
    MenuId::for_id("TimelineFilterSubMenu")
}

/// `MenuId.AccountsContext`.
pub fn accounts_context() -> MenuId {
    MenuId::for_id("AccountsContext")
}

/// `MenuId.SidebarTitle`.
pub fn sidebar_title() -> MenuId {
    MenuId::for_id("SidebarTitle")
}

/// `MenuId.PanelTitle`.
pub fn panel_title() -> MenuId {
    MenuId::for_id("PanelTitle")
}

/// `MenuId.AuxiliaryBarTitle`.
pub fn auxiliary_bar_title() -> MenuId {
    MenuId::for_id("AuxiliaryBarTitle")
}

/// `MenuId.TerminalInstanceContext`.
pub fn terminal_instance_context() -> MenuId {
    MenuId::for_id("TerminalInstanceContext")
}

/// `MenuId.TerminalEditorInstanceContext`.
pub fn terminal_editor_instance_context() -> MenuId {
    MenuId::for_id("TerminalEditorInstanceContext")
}

/// `MenuId.TerminalNewDropdownContext`.
pub fn terminal_new_dropdown_context() -> MenuId {
    MenuId::for_id("TerminalNewDropdownContext")
}

/// `MenuId.TerminalTabContext`.
pub fn terminal_tab_context() -> MenuId {
    MenuId::for_id("TerminalTabContext")
}

/// `MenuId.TerminalTabEmptyAreaContext`.
pub fn terminal_tab_empty_area_context() -> MenuId {
    MenuId::for_id("TerminalTabEmptyAreaContext")
}

/// `MenuId.TerminalStickyScrollContext`.
pub fn terminal_sticky_scroll_context() -> MenuId {
    MenuId::for_id("TerminalStickyScrollContext")
}

/// `MenuId.WebviewContext`.
pub fn webview_context() -> MenuId {
    MenuId::for_id("WebviewContext")
}

/// `MenuId.InlineCompletionsActions`.
pub fn inline_completions_actions() -> MenuId {
    MenuId::for_id("InlineCompletionsActions")
}

/// `MenuId.InlineEditsActions`.
pub fn inline_edits_actions() -> MenuId {
    MenuId::for_id("InlineEditsActions")
}

/// `MenuId.NewFile`.
pub fn new_file() -> MenuId {
    MenuId::for_id("NewFile")
}

/// `MenuId.MergeInput1Toolbar`.
pub fn merge_input1_toolbar() -> MenuId {
    MenuId::for_id("MergeToolbar1Toolbar")
}

/// `MenuId.MergeInput2Toolbar`.
pub fn merge_input2_toolbar() -> MenuId {
    MenuId::for_id("MergeToolbar2Toolbar")
}

/// `MenuId.MergeBaseToolbar`.
pub fn merge_base_toolbar() -> MenuId {
    MenuId::for_id("MergeBaseToolbar")
}

/// `MenuId.MergeInputResultToolbar`.
pub fn merge_input_result_toolbar() -> MenuId {
    MenuId::for_id("MergeToolbarResultToolbar")
}

/// `MenuId.InlineSuggestionToolbar`.
pub fn inline_suggestion_toolbar() -> MenuId {
    MenuId::for_id("InlineSuggestionToolbar")
}

/// `MenuId.InlineEditToolbar`.
pub fn inline_edit_toolbar() -> MenuId {
    MenuId::for_id("InlineEditToolbar")
}

/// `MenuId.ChatContext`.
pub fn chat_context() -> MenuId {
    MenuId::for_id("ChatContext")
}

/// `MenuId.ChatCodeBlock`.
pub fn chat_code_block() -> MenuId {
    MenuId::for_id("ChatCodeblock")
}

/// `MenuId.ChatCompareBlock`.
pub fn chat_compare_block() -> MenuId {
    MenuId::for_id("ChatCompareBlock")
}

/// `MenuId.ChatMessageTitle`.
pub fn chat_message_title() -> MenuId {
    MenuId::for_id("ChatMessageTitle")
}

/// `MenuId.ChatWelcomeContext`.
pub fn chat_welcome_context() -> MenuId {
    MenuId::for_id("ChatWelcomeContext")
}

/// `MenuId.ChatMessageFooter`.
pub fn chat_message_footer() -> MenuId {
    MenuId::for_id("ChatMessageFooter")
}

/// `MenuId.ChatSubagentContent`.
pub fn chat_subagent_content() -> MenuId {
    MenuId::for_id("ChatSubagentContent")
}

/// `MenuId.ChatExecute`.
pub fn chat_execute() -> MenuId {
    MenuId::for_id("ChatExecute")
}

/// `MenuId.ChatExecuteQueue`.
pub fn chat_execute_queue() -> MenuId {
    MenuId::for_id("ChatExecuteQueue")
}

/// `MenuId.ChatInput`.
pub fn chat_input() -> MenuId {
    MenuId::for_id("ChatInput")
}

/// `MenuId.ChatInputSecondary`.
pub fn chat_input_secondary() -> MenuId {
    MenuId::for_id("ChatInputSecondary")
}

/// `MenuId.ChatInputStatus`.
pub fn chat_input_status() -> MenuId {
    MenuId::for_id("ChatInputStatus")
}

/// `MenuId.ChatInputSide`.
pub fn chat_input_side() -> MenuId {
    MenuId::for_id("ChatInputSide")
}

/// `MenuId.AutomationsDialogInput`.
pub fn automations_dialog_input() -> MenuId {
    MenuId::for_id("AutomationsDialogInput")
}

/// `MenuId.ChatModePicker`.
pub fn chat_mode_picker() -> MenuId {
    MenuId::for_id("ChatModePicker")
}

/// `MenuId.ChatEditingWidgetToolbar`.
pub fn chat_editing_widget_toolbar() -> MenuId {
    MenuId::for_id("ChatEditingWidgetToolbar")
}

/// `MenuId.ChatEditingSessionChangesToolbar`.
pub fn chat_editing_session_changes_toolbar() -> MenuId {
    MenuId::for_id("ChatEditingSessionChangesToolbar")
}

/// `MenuId.ChatEditingSessionTitleToolbar`.
pub fn chat_editing_session_title_toolbar() -> MenuId {
    MenuId::for_id("ChatEditingSessionTitleToolbar")
}

/// `MenuId.ChatEditingSessionChangesVersionsSubmenu`.
pub fn chat_editing_session_changes_versions_submenu() -> MenuId {
    MenuId::for_id("ChatEditingSessionChangesVersionsSubmenu")
}

/// `MenuId.ChatEditingSessionChangesFileHeaderToolbar`.
pub fn chat_editing_session_changes_file_header_toolbar() -> MenuId {
    MenuId::for_id("ChatEditingSessionChangesFileHeaderToolbar")
}

/// `MenuId.ChatEditingSessionChangesFileHeaderRightToolbar`.
pub fn chat_editing_session_changes_file_header_right_toolbar() -> MenuId {
    MenuId::for_id("ChatEditingSessionChangesFileHeaderRightToolbar")
}

/// `MenuId.ChatEditingEditorContent`.
pub fn chat_editing_editor_content() -> MenuId {
    MenuId::for_id("ChatEditingEditorContent")
}

/// `MenuId.ChatEditingEditorHunk`.
pub fn chat_editing_editor_hunk() -> MenuId {
    MenuId::for_id("ChatEditingEditorHunk")
}

/// `MenuId.ChatEditingDeletedNotebookCell`.
pub fn chat_editing_deleted_notebook_cell() -> MenuId {
    MenuId::for_id("ChatEditingDeletedNotebookCell")
}

/// `MenuId.ChatInputAttachmentToolbar`.
pub fn chat_input_attachment_toolbar() -> MenuId {
    MenuId::for_id("ChatInputAttachmentToolbar")
}

/// `MenuId.ChatEditingWidgetModifiedFilesToolbar`.
pub fn chat_editing_widget_modified_files_toolbar() -> MenuId {
    MenuId::for_id("ChatEditingWidgetModifiedFilesToolbar")
}

/// `MenuId.ChatInputResourceAttachmentContext`.
pub fn chat_input_resource_attachment_context() -> MenuId {
    MenuId::for_id("ChatInputResourceAttachmentContext")
}

/// `MenuId.ChatInputSymbolAttachmentContext`.
pub fn chat_input_symbol_attachment_context() -> MenuId {
    MenuId::for_id("ChatInputSymbolAttachmentContext")
}

/// `MenuId.ChatInlineResourceAnchorContext`.
pub fn chat_inline_resource_anchor_context() -> MenuId {
    MenuId::for_id("ChatInlineResourceAnchorContext")
}

/// `MenuId.ChatInlineSymbolAnchorContext`.
pub fn chat_inline_symbol_anchor_context() -> MenuId {
    MenuId::for_id("ChatInlineSymbolAnchorContext")
}

/// `MenuId.ChatMessageCheckpoint`.
pub fn chat_message_checkpoint() -> MenuId {
    MenuId::for_id("ChatMessageCheckpoint")
}

/// `MenuId.ChatMessageRestoreCheckpoint`.
pub fn chat_message_restore_checkpoint() -> MenuId {
    MenuId::for_id("ChatMessageRestoreCheckpoint")
}

/// `MenuId.ChatNewMenu`.
pub fn chat_new_menu() -> MenuId {
    MenuId::for_id("ChatNewMenu")
}

/// `MenuId.ChatEditingCodeBlockContext`.
pub fn chat_editing_code_block_context() -> MenuId {
    MenuId::for_id("ChatEditingCodeBlockContext")
}

/// `MenuId.ChatTitleBarMenu`.
pub fn chat_title_bar_menu() -> MenuId {
    MenuId::for_id("ChatTitleBarMenu")
}

/// `MenuId.ChatAttachmentsContext`.
pub fn chat_attachments_context() -> MenuId {
    MenuId::for_id("ChatAttachmentsContext")
}

/// `MenuId.ChatTipContext`.
pub fn chat_tip_context() -> MenuId {
    MenuId::for_id("ChatTipContext")
}

/// `MenuId.ChatTipToolbar`.
pub fn chat_tip_toolbar() -> MenuId {
    MenuId::for_id("ChatTipToolbar")
}

/// `MenuId.ChatToolOutputResourceToolbar`.
pub fn chat_tool_output_resource_toolbar() -> MenuId {
    MenuId::for_id("ChatToolOutputResourceToolbar")
}

/// `MenuId.ChatTextEditorMenu`.
pub fn chat_text_editor_menu() -> MenuId {
    MenuId::for_id("ChatTextEditorMenu")
}

/// `MenuId.ChatToolOutputResourceContext`.
pub fn chat_tool_output_resource_context() -> MenuId {
    MenuId::for_id("ChatToolOutputResourceContext")
}

/// `MenuId.ChatMultiDiffContext`.
pub fn chat_multi_diff_context() -> MenuId {
    MenuId::for_id("ChatMultiDiffContext")
}

/// `MenuId.ChatConfirmationMenu`.
pub fn chat_confirmation_menu() -> MenuId {
    MenuId::for_id("ChatConfirmationMenu")
}

/// `MenuId.ChatEditorInlineMenu`.
pub fn chat_editor_inline_menu() -> MenuId {
    MenuId::for_id("ChatEditorInlineGutter")
}

/// `MenuId.ChatEditorInlineExecute`.
pub fn chat_editor_inline_execute() -> MenuId {
    MenuId::for_id("ChatEditorInputExecute")
}

/// `MenuId.ChatEditorInlineInputSide`.
pub fn chat_editor_inline_input_side() -> MenuId {
    MenuId::for_id("ChatEditorInputSide")
}

/// `MenuId.InlineChatEditorAffordance`.
pub fn inline_chat_editor_affordance() -> MenuId {
    MenuId::for_id("InlineChatEditorAffordance")
}

/// `MenuId.AccessibleView`.
pub fn accessible_view() -> MenuId {
    MenuId::for_id("AccessibleView")
}

/// `MenuId.MultiDiffEditorContent`.
pub fn multi_diff_editor_content() -> MenuId {
    MenuId::for_id("MultiDiffEditorContent")
}

/// `MenuId.MultiDiffEditorFileToolbar`.
pub fn multi_diff_editor_file_toolbar() -> MenuId {
    MenuId::for_id("MultiDiffEditorFileToolbar")
}

/// `MenuId.DiffEditorHunkToolbar`.
pub fn diff_editor_hunk_toolbar() -> MenuId {
    MenuId::for_id("DiffEditorHunkToolbar")
}

/// `MenuId.DiffEditorSelectionToolbar`.
pub fn diff_editor_selection_toolbar() -> MenuId {
    MenuId::for_id("DiffEditorSelectionToolbar")
}

/// `MenuId.BrowserNavigationToolbar`.
pub fn browser_navigation_toolbar() -> MenuId {
    MenuId::for_id("BrowserNavigationToolbar")
}

/// `MenuId.BrowserActionsToolbar`.
pub fn browser_actions_toolbar() -> MenuId {
    MenuId::for_id("BrowserActionsToolbar")
}

/// `MenuId.BrowserChatActionsMenu`.
pub fn browser_chat_actions_menu() -> MenuId {
    MenuId::for_id("BrowserChatActionsMenu")
}

/// `MenuId.BrowserEmulationToolbar`.
pub fn browser_emulation_toolbar() -> MenuId {
    MenuId::for_id("BrowserEmulationToolbar")
}

/// `MenuId.AgentSessionsViewerFilterSubMenu`.
pub fn agent_sessions_viewer_filter_sub_menu() -> MenuId {
    MenuId::for_id("AgentSessionsViewerFilterSubMenu")
}

/// `MenuId.AgentSessionsExternalFilterSubMenu`.
pub fn agent_sessions_external_filter_sub_menu() -> MenuId {
    MenuId::for_id("AgentSessionsExternalFilterSubMenu")
}

/// `MenuId.AgentSessionsContext`.
pub fn agent_sessions_context() -> MenuId {
    MenuId::for_id("AgentSessionsContext")
}

/// `MenuId.AgentSessionSectionContext`.
pub fn agent_session_section_context() -> MenuId {
    MenuId::for_id("AgentSessionSectionContext")
}

/// `MenuId.AgentSessionsCreateSubMenu`.
pub fn agent_sessions_create_sub_menu() -> MenuId {
    MenuId::for_id("AgentSessionsCreateSubMenu")
}

/// `MenuId.AgentSessionsToolbar`.
pub fn agent_sessions_toolbar() -> MenuId {
    MenuId::for_id("AgentSessionsToolbar")
}

/// `MenuId.AgentSessionItemToolbar`.
pub fn agent_session_item_toolbar() -> MenuId {
    MenuId::for_id("AgentSessionItemToolbar")
}

/// `MenuId.AgentSessionSectionToolbar`.
pub fn agent_session_section_toolbar() -> MenuId {
    MenuId::for_id("AgentSessionSectionToolbar")
}

/// `MenuId.SessionItemContextMenu`.
pub fn session_item_context_menu() -> MenuId {
    MenuId::for_id("SessionItemContextMenu")
}

/// `MenuId.SessionHeaderContext`.
pub fn session_header_context() -> MenuId {
    MenuId::for_id("SessionsSessionHeaderContext")
}

/// `MenuId.AgentsTitleBarControlMenu`.
pub fn agents_title_bar_control_menu() -> MenuId {
    MenuId::for_id("AgentsTitleBarControlMenu")
}

/// `MenuId.AgentsChangesToolbar`.
pub fn agents_changes_toolbar() -> MenuId {
    MenuId::for_id("AgentsChangesToolbar")
}

/// `MenuId.AgentsChangesPrimaryActionSubMenu`.
pub fn agents_changes_primary_action_sub_menu() -> MenuId {
    MenuId::for_id("AgentsChangesPrimaryActionSubMenu")
}

/// `MenuId.AgentsChangeInlineToolbar`.
pub fn agents_change_inline_toolbar() -> MenuId {
    MenuId::for_id("AgentsChangeInlineToolbar")
}

/// `MenuId.ChatViewSessionTitleNavigationToolbar`.
pub fn chat_view_session_title_navigation_toolbar() -> MenuId {
    MenuId::for_id("ChatViewSessionTitleNavigationToolbar")
}

/// `MenuId.ChatViewSessionTitleToolbar`.
pub fn chat_view_session_title_toolbar() -> MenuId {
    MenuId::for_id("ChatViewSessionTitleToolbar")
}

/// `MenuId.ChatContextUsageActions`.
pub fn chat_context_usage_actions() -> MenuId {
    MenuId::for_id("ChatContextUsageActions")
}

/// `MenuId.MarkerHoverStatusBar`.
pub fn marker_hover_status_bar() -> MenuId {
    MenuId::for_id("MarkerHoverParticipant.StatusBar")
}
