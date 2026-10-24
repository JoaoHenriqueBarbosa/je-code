//! Porte de `src/vs/workbench/contrib/preferences/browser/settingsLayout.ts`: o sumário do editor de configurações
//! (`tocData`), com os padrões de chave que cada grupo reúne, e os dados do grupo "Commonly Used".
//!
//! Divergências:
//! - **`tocData`** (uma constante calculada ao carregar o módulo, com `localize`) é [`toc_data`], que monta a árvore uma
//!   vez por thread e entrega um `Rc` (os rótulos dependem do idioma, lido na primeira chamada). `isWeb` e `isWindows`
//!   são os da plataforma detectada.
//! - **`ITOCEntry<T>`** é a struct genérica [`ITOCEntry`], com os campos opcionais como `Option`. `ITOCFilter` é a struct
//!   [`ITOCFilter`], com os mesmos campos.
//! - **`ChatAIDisabledSettingId`** (`platform/chat/common/chatSettings.ts`) é a constante [`CHAT_AI_DISABLED_SETTING_ID`].
//! - **`getCommonlyUsedData`** devolve um `ITOCEntry<Rc<ISetting>>`: as configurações são compartilhadas por `Rc`, como o
//!   objeto do original.

use std::collections::HashMap;
use std::rc::Rc;

use je_base::nls::localize;
use je_base::platform::Platform;

use crate::services::preferences::preferences::{ISetting, ISettingsGroup};

/// `ChatAIDisabledSettingId`.
pub const CHAT_AI_DISABLED_SETTING_ID: &str = "chat.disableAIFeatures";

/// `ITOCFilter.include` e `ITOCFilter.exclude`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ITOCFilterPatterns {
    pub key_patterns: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

/// `ITOCFilter`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ITOCFilter {
    pub include: Option<ITOCFilterPatterns>,
    pub exclude: Option<ITOCFilterPatterns>,
}

/// `ITOCEntry<T>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITOCEntry<T> {
    pub id: String,
    pub label: String,
    pub order: Option<f64>,
    pub children: Option<Vec<ITOCEntry<T>>>,
    pub settings: Option<Vec<T>>,
    pub hide: Option<bool>,
}

const COMMONLY_USED_SETTINGS: [&str; 13] = [
    "editor.fontSize",
    "editor.formatOnSave",
    "files.autoSave",
    "GitHub.copilot-chat.manageExtension",
    "editor.defaultFormatter",
    "editor.fontFamily",
    "editor.wordWrap",
    "chat.agent.maxRequests",
    "files.exclude",
    "workbench.colorTheme",
    "editor.tabSize",
    "editor.mouseWheelZoom",
    "editor.formatOnPaste",
];

/// `getCommonlyUsedData`.
pub fn get_commonly_used_data(setting_groups: &[ISettingsGroup]) -> ITOCEntry<Rc<ISetting>> {
    let mut all_settings: HashMap<&str, &Rc<ISetting>> = HashMap::new();
    for group in setting_groups {
        for section in &group.sections {
            for s in &section.settings {
                all_settings.insert(s.key.as_str(), s);
            }
        }
    }
    let mut settings: Vec<Rc<ISetting>> = Vec::new();
    for id in COMMONLY_USED_SETTINGS {
        if let Some(setting) = all_settings.get(id) {
            settings.push((*setting).clone());
        }
    }
    ITOCEntry {
        id: "commonlyUsed".to_owned(),
        label: localize("commonlyUsed", "Commonly Used", &[]),
        order: None,
        children: None,
        settings: Some(settings),
        hide: None,
    }
}

/// Uma entrada do sumário: `{ id, label, settings, children, hide }`.
fn toc(id: &str, label: String, settings: Option<&[&str]>, children: Option<Vec<ITOCEntry<String>>>, hide: Option<bool>) -> ITOCEntry<String> {
    ITOCEntry {
        id: id.to_owned(),
        label,
        order: None,
        children,
        settings: settings.map(|settings| settings.iter().map(|setting| (*setting).to_owned()).collect()),
        hide,
    }
}

/// Uma folha do sumário: `{ id, label, settings }`.
fn leaf(id: &str, label: String, settings: &[&str]) -> ITOCEntry<String> {
    toc(id, label, Some(settings), None, None)
}

fn build_toc_data() -> ITOCEntry<String> {
    let platform = Platform::detect();
    let l = |key: &str, message: &str| localize(key, message, &[]);
    toc(
        "root",
        "root".to_owned(),
        None,
        Some(vec![
            toc(
                "editor",
                l("textEditor", "Text Editor"),
                Some(&["editor.*"]),
                Some(vec![
                    leaf("editor/cursor", l("cursor", "Cursor"), &["editor.cursor*"]),
                    leaf("editor/find", l("find", "Find"), &["editor.find.*"]),
                    leaf("editor/font", l("font", "Font"), &["editor.font*"]),
                    leaf("editor/format", l("formatting", "Formatting"), &["editor.format*"]),
                    leaf("editor/diffEditor", l("diffEditor", "Diff Editor"), &["diffEditor.*"]),
                    leaf("editor/multiDiffEditor", l("multiDiffEditor", "Multi-File Diff Editor"), &["multiDiffEditor.*"]),
                    leaf("editor/minimap", l("minimap", "Minimap"), &["editor.minimap.*"]),
                    leaf("editor/suggestions", l("suggestions", "Suggestions"), &["editor.*suggest*"]),
                    leaf("editor/files", l("files", "Files"), &["files.*"]),
                ]),
                None,
            ),
            toc(
                "workbench",
                l("workbench", "Workbench"),
                Some(&["workbench.*"]),
                Some(vec![
                    leaf(
                        "workbench/appearance",
                        l("appearance", "Appearance"),
                        &[
                            "workbench.activityBar.*",
                            "workbench.*color*",
                            "workbench.fontAliasing",
                            "workbench.iconTheme",
                            "workbench.sidebar.location",
                            "workbench.*.visible",
                            "workbench.tips.enabled",
                            "workbench.tree.*",
                            "workbench.view.*",
                        ],
                    ),
                    leaf("workbench/breadcrumbs", l("breadcrumbs", "Breadcrumbs"), &["breadcrumbs.*"]),
                    leaf("workbench/editor", l("editorManagement", "Editor Management"), &["workbench.editor.*"]),
                    leaf("workbench/settings", l("settings", "Settings Editor"), &["workbench.settings.*"]),
                    leaf("workbench/zenmode", l("zenMode", "Zen Mode"), &["zenmode.*"]),
                    leaf("workbench/screencastmode", l("screencastMode", "Screencast Mode"), &["screencastMode.*"]),
                    leaf("workbench/browser", l("browser", "Browser"), &["workbench.browser.*"]),
                ]),
                None,
            ),
            toc(
                "window",
                l("window", "Window"),
                Some(&["window.*"]),
                Some(vec![leaf("window/newWindow", l("newWindow", "New Window"), &["window.*newwindow*"])]),
                None,
            ),
            toc(
                "chat",
                l("chat", "Chat"),
                None,
                Some(vec![
                    leaf(
                        "chat/agent",
                        l("chatAgent", "Agent"),
                        &[
                            "chat.agent.*",
                            "chat.checkpoints.*",
                            "chat.editRequests",
                            "chat.requestQueuing.*",
                            "chat.undoRequests.*",
                            "chat.customAgentInSubagent.*",
                            "chat.editing.autoAcceptDelay",
                            "chat.editing.confirmEditRequest*",
                            "chat.planAgent.defaultModel",
                        ],
                    ),
                    leaf(
                        "chat/appearance",
                        l("chatAppearance", "Appearance"),
                        &[
                            "chat.editor.*",
                            "chat.fontFamily",
                            "chat.fontSize",
                            "chat.math.*",
                            "chat.agentsControl.*",
                            "chat.alternativeToolAction.*",
                            "chat.codeBlock.*",
                            "chat.editing.explainChanges.enabled",
                            "chat.editorAssociations",
                            "chat.extensionUnification.*",
                            "chat.inlineReferences.*",
                            "chat.notifyWindow*",
                            "chat.statusWidget.*",
                            "chat.tips.*",
                            "chat.unifiedAgentsBar.*",
                            "accessibility.signals.chatUserActionRequired",
                            "accessibility.signals.chatResponseReceived",
                        ],
                    ),
                    leaf(
                        "chat/sessions",
                        l("chatSessions", "Sessions"),
                        &[
                            "chat.agentSessionProjection.*",
                            "chat.sessions.*",
                            "chat.viewProgressBadge.*",
                            "chat.viewSessions.*",
                            "chat.restoreLastPanelSession",
                            "chat.exitAfterDelegation",
                            "chat.repoInfo.*",
                        ],
                    ),
                    leaf("chat/tools", l("chatTools", "Tools"), &["chat.tools.*", "chat.extensionTools.*"]),
                    leaf("chat/mcp", l("chatMcp", "MCP"), &["mcp", "chat.mcp.*", "mcp.*"]),
                    leaf(
                        "chat/context",
                        l("chatContext", "Context"),
                        &[
                            "chat.detectParticipant.*",
                            "chat.experimental.detectParticipant.*",
                            "chat.implicitContext.*",
                            "chat.promptFilesLocations",
                            "chat.instructionsFilesLocations",
                            "chat.modeFilesLocations",
                            "chat.agentFilesLocations",
                            "chat.agentSkillsLocations",
                            "chat.hookFilesLocations",
                            "chat.promptFilesRecommendations",
                            "chat.useAgentsMdFile",
                            "chat.useNestedAgentsMdFiles",
                            "chat.useAgentSkills",
                            "chat.experimental.useSkillAdherencePrompt",
                            "chat.useHooks",
                            "chat.includeApplyingInstructions",
                            "chat.includeReferencedInstructions",
                            "chat.useClaudeMdFile",
                        ],
                    ),
                    leaf("chat/inlineChat", l("chatInlineChat", "Inline Chat"), &["inlineChat.*"]),
                    leaf("chat/miscellaneous", l("chatMiscellaneous", "Miscellaneous"), &[CHAT_AI_DISABLED_SETTING_ID, "chat.allowAnonymousAccess"]),
                ]),
                None,
            ),
            toc(
                "features",
                l("features", "Features"),
                None,
                Some(vec![
                    leaf("features/accessibilitySignals", l("accessibility.signals", "Accessibility Signals"), &["accessibility.signal*"]),
                    leaf("features/accessibility", l("accessibility", "Accessibility"), &["accessibility.*"]),
                    leaf("features/explorer", l("fileExplorer", "Explorer"), &["explorer.*", "outline.*"]),
                    leaf("features/search", l("search", "Search"), &["search.*"]),
                    leaf("features/debug", l("debug", "Debug"), &["debug.*", "launch"]),
                    leaf("features/testing", l("testing", "Testing"), &["testing.*"]),
                    leaf("features/scm", l("scm", "Source Control"), &["scm.*"]),
                    leaf("features/extensions", l("extensions", "Extensions"), &["extensions.*"]),
                    leaf("features/terminal", l("terminal", "Terminal"), &["terminal.*"]),
                    leaf("features/task", l("task", "Task"), &["task.*"]),
                    leaf("features/problems", l("problems", "Problems"), &["problems.*"]),
                    leaf("features/output", l("output", "Output"), &["output.*"]),
                    leaf("features/comments", l("comments", "Comments"), &["comments.*"]),
                    leaf("features/remote", l("remote", "Remote"), &["remote.*"]),
                    leaf("features/timeline", l("timeline", "Timeline"), &["timeline.*"]),
                    leaf("features/notebook", l("notebook", "Notebook"), &["notebook.*", "interactiveWindow.*"]),
                    leaf("features/mergeEditor", l("mergeEditor", "Merge Editor"), &["mergeEditor.*"]),
                    toc("features/issueReporter", l("issueReporter", "Issue Reporter"), Some(&["issueReporter.*"]), None, Some(!platform.is_web)),
                ]),
                None,
            ),
            toc(
                "application",
                l("application", "Application"),
                None,
                Some(vec![
                    leaf("application/http", l("proxy", "Proxy"), &["http.*"]),
                    leaf("application/keyboard", l("keyboard", "Keyboard"), &["keyboard.*"]),
                    leaf("application/update", l("update", "Update"), &["update.*"]),
                    leaf("application/telemetry", l("telemetry", "Telemetry"), &["telemetry.*"]),
                    leaf("application/settingsSync", l("settingsSync", "Settings Sync"), &["settingsSync.*"]),
                    leaf("application/network", l("network", "Network"), &["network.*"]),
                    leaf("application/experimental", l("experimental", "Experimental"), &["application.experimental.*"]),
                    toc("application/other", l("other", "Other"), Some(&["application.*"]), None, Some(platform.is_windows)),
                ]),
                None,
            ),
            toc(
                "security",
                l("security", "Security"),
                Some(&["security.*"]),
                Some(vec![leaf("security/workspace", l("workspace", "Workspace"), &["security.workspace.*"])]),
                None,
            ),
        ]),
        None,
    )
}

thread_local! {
    static TOC_DATA: Rc<ITOCEntry<String>> = Rc::new(build_toc_data());
}

/// `tocData`.
pub fn toc_data() -> Rc<ITOCEntry<String>> {
    TOC_DATA.with(Rc::clone)
}
