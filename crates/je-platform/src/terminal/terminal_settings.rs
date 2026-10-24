//! Porte dos `const enum` de configuração de `src/vs/platform/terminal/common/terminal.ts`:
//! `TerminalSettingPrefix`, `TerminalSettingId` e `ShellIntegrationTimeoutOverride`.
//!
//! Representação: os `const enum` de string são enums com `as_str()` devolvendo o valor do original (a
//! mesma convenção de [`super::terminal`]); o `const enum` numérico `ShellIntegrationTimeoutOverride` é um
//! enum com [`ShellIntegrationTimeoutOverride::value`].

/// Declara um `const enum` de string do original: as variantes e o `as_str()` com o valor.
macro_rules! setting_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$variant_meta:meta])* $variant:ident = $value:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }
    };
}

setting_enum! {
    /// `TerminalSettingPrefix`.
    TerminalSettingPrefix {
        AutomationProfile = "terminal.integrated.automationProfile.",
        DefaultProfile = "terminal.integrated.defaultProfile.",
        Profiles = "terminal.integrated.profiles.",
    }
}

setting_enum! {
    /// `TerminalSettingId`.
    TerminalSettingId {
        SendKeybindingsToShell = "terminal.integrated.sendKeybindingsToShell",
        AutomationProfileLinux = "terminal.integrated.automationProfile.linux",
        AutomationProfileMacOs = "terminal.integrated.automationProfile.osx",
        AutomationProfileWindows = "terminal.integrated.automationProfile.windows",
        AgentHostProfileLinux = "terminal.integrated.agentHostProfile.linux",
        AgentHostProfileMacOs = "terminal.integrated.agentHostProfile.osx",
        AgentHostProfileWindows = "terminal.integrated.agentHostProfile.windows",
        ProfilesWindows = "terminal.integrated.profiles.windows",
        ProfilesMacOs = "terminal.integrated.profiles.osx",
        ProfilesLinux = "terminal.integrated.profiles.linux",
        DefaultProfileLinux = "terminal.integrated.defaultProfile.linux",
        DefaultProfileMacOs = "terminal.integrated.defaultProfile.osx",
        DefaultProfileWindows = "terminal.integrated.defaultProfile.windows",
        UseWslProfiles = "terminal.integrated.useWslProfiles",
        TabsDefaultColor = "terminal.integrated.tabs.defaultColor",
        TabsDefaultIcon = "terminal.integrated.tabs.defaultIcon",
        TabsEnabled = "terminal.integrated.tabs.enabled",
        TabsEnableAnimation = "terminal.integrated.tabs.enableAnimation",
        TabsHideCondition = "terminal.integrated.tabs.hideCondition",
        TabsShowActiveTerminal = "terminal.integrated.tabs.showActiveTerminal",
        TabsShowActions = "terminal.integrated.tabs.showActions",
        TabsLocation = "terminal.integrated.tabs.location",
        TabsFocusMode = "terminal.integrated.tabs.focusMode",
        TabsAllowAgentCliTitle = "terminal.integrated.tabs.allowAgentCliTitle",
        MacOptionIsMeta = "terminal.integrated.macOptionIsMeta",
        MacOptionClickForcesSelection = "terminal.integrated.macOptionClickForcesSelection",
        AltClickMovesCursor = "terminal.integrated.altClickMovesCursor",
        CopyOnSelection = "terminal.integrated.copyOnSelection",
        EnableMultiLinePasteWarning = "terminal.integrated.enableMultiLinePasteWarning",
        DrawBoldTextInBrightColors = "terminal.integrated.drawBoldTextInBrightColors",
        FontFamily = "terminal.integrated.fontFamily",
        FontRendering = "terminal.integrated.fontRendering",
        FontSize = "terminal.integrated.fontSize",
        LetterSpacing = "terminal.integrated.letterSpacing",
        LineHeight = "terminal.integrated.lineHeight",
        MinimumContrastRatio = "terminal.integrated.minimumContrastRatio",
        TabStopWidth = "terminal.integrated.tabStopWidth",
        FastScrollSensitivity = "terminal.integrated.fastScrollSensitivity",
        MouseWheelScrollSensitivity = "terminal.integrated.mouseWheelScrollSensitivity",
        BellDuration = "terminal.integrated.bellDuration",
        FontWeight = "terminal.integrated.fontWeight",
        FontWeightBold = "terminal.integrated.fontWeightBold",
        CursorBlinking = "terminal.integrated.cursorBlinking",
        TextBlinking = "terminal.integrated.textBlinking",
        CursorStyle = "terminal.integrated.cursorStyle",
        CursorStyleInactive = "terminal.integrated.cursorStyleInactive",
        CursorWidth = "terminal.integrated.cursorWidth",
        Scrollback = "terminal.integrated.scrollback",
        DetectLocale = "terminal.integrated.detectLocale",
        DefaultLocation = "terminal.integrated.defaultLocation",
        GpuAcceleration = "terminal.integrated.gpuAcceleration",
        TerminalTitleSeparator = "terminal.integrated.tabs.separator",
        TerminalTitle = "terminal.integrated.tabs.title",
        TerminalDescription = "terminal.integrated.tabs.description",
        RightClickBehavior = "terminal.integrated.rightClickBehavior",
        MiddleClickBehavior = "terminal.integrated.middleClickBehavior",
        Cwd = "terminal.integrated.cwd",
        ConfirmOnExit = "terminal.integrated.confirmOnExit",
        ConfirmOnKill = "terminal.integrated.confirmOnKill",
        EnableBell = "terminal.integrated.enableBell",
        EnableVisualBell = "terminal.integrated.enableVisualBell",
        CommandsToSkipShell = "terminal.integrated.commandsToSkipShell",
        AllowChords = "terminal.integrated.allowChords",
        AllowMnemonics = "terminal.integrated.allowMnemonics",
        TabFocusMode = "terminal.integrated.tabFocusMode",
        EnvMacOs = "terminal.integrated.env.osx",
        EnvLinux = "terminal.integrated.env.linux",
        EnvWindows = "terminal.integrated.env.windows",
        EnvironmentChangesRelaunch = "terminal.integrated.environmentChangesRelaunch",
        ShowExitAlert = "terminal.integrated.showExitAlert",
        SplitCwd = "terminal.integrated.splitCwd",
        WindowsUseConptyDll = "terminal.integrated.windowsUseConptyDll",
        WordSeparators = "terminal.integrated.wordSeparators",
        EnableFileLinks = "terminal.integrated.enableFileLinks",
        AllowedLinkSchemes = "terminal.integrated.allowedLinkSchemes",
        UnicodeVersion = "terminal.integrated.unicodeVersion",
        EnablePersistentSessions = "terminal.integrated.enablePersistentSessions",
        PersistentSessionReviveProcess = "terminal.integrated.persistentSessionReviveProcess",
        HideOnStartup = "terminal.integrated.hideOnStartup",
        HideOnLastClosed = "terminal.integrated.hideOnLastClosed",
        CustomGlyphs = "terminal.integrated.customGlyphs",
        RescaleOverlappingGlyphs = "terminal.integrated.rescaleOverlappingGlyphs",
        PersistentSessionScrollback = "terminal.integrated.persistentSessionScrollback",
        InheritEnv = "terminal.integrated.inheritEnv",
        ShowLinkHover = "terminal.integrated.showLinkHover",
        IgnoreProcessNames = "terminal.integrated.ignoreProcessNames",
        ShellIntegrationEnabled = "terminal.integrated.shellIntegration.enabled",
        ShellIntegrationShowWelcome = "terminal.integrated.shellIntegration.showWelcome",
        ShellIntegrationDecorationsEnabled = "terminal.integrated.shellIntegration.decorationsEnabled",
        ShellIntegrationTimeout = "terminal.integrated.shellIntegration.timeout",
        ShellIntegrationQuickFixEnabled = "terminal.integrated.shellIntegration.quickFixEnabled",
        ShellIntegrationEnvironmentReporting = "terminal.integrated.shellIntegration.environmentReporting",
        EnableImages = "terminal.integrated.enableImages",
        SmoothScrolling = "terminal.integrated.smoothScrolling",
        IgnoreBracketedPasteMode = "terminal.integrated.ignoreBracketedPasteMode",
        FocusAfterRun = "terminal.integrated.focusAfterRun",
        FontLigaturesEnabled = "terminal.integrated.fontLigatures.enabled",
        FontLigaturesFeatureSettings = "terminal.integrated.fontLigatures.featureSettings",
        FontLigaturesFallbackLigatures = "terminal.integrated.fontLigatures.fallbackLigatures",
        EnableKittyKeyboardProtocol = "terminal.integrated.enableKittyKeyboardProtocol",
        EnableWin32InputMode = "terminal.integrated.enableWin32InputMode",
        AllowInUntrustedWorkspace = "terminal.integrated.allowInUntrustedWorkspace",

        // Developer/debug settings

        /// Simulated latency applied to all calls made to the pty host
        DeveloperPtyHostLatency = "terminal.integrated.developer.ptyHost.latency",
        /// Simulated startup delay of the pty host process
        DeveloperPtyHostStartupDelay = "terminal.integrated.developer.ptyHost.startupDelay",
        /// Shows the textarea element
        DevMode = "terminal.integrated.developer.devMode",
    }
}

/// `ShellIntegrationTimeoutOverride`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellIntegrationTimeoutOverride {
    DisableForTests,
}

impl ShellIntegrationTimeoutOverride {
    /// O valor numérico do `const enum` (`DisableForTests = -2`).
    pub fn value(self) -> f64 {
        match self {
            Self::DisableForTests => -2.0,
        }
    }
}
