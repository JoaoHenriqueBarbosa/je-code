//! Porte de `src/vs/base/browser/ui/severityIcon/severityIcon.ts` e `media/severityIcon.css`: o ícone que
//! representa uma [`Severity`] (`info`, `warning` e `error` do codicon; `Ignore` usa o de `info` com a
//! classe `severity-ignore`) e as cores que a folha de estilo dá a cada um.
//!
//! Como o DOM vira [`je_ui`]: `SeverityIcon.className` devolve a lista de classes do `<span>` do ícone
//! ([`SeverityIcon::class_name`]), a mesma que se poria num nó com [`je_ui::Tree::set_class_name`], e
//! [`SeverityIcon::theme_icon`] o `ThemeIcon` de onde ela sai. As regras de CSS (`.codicon.codicon-error {
//! color: var(--vscode-problemsErrorIcon-foreground) }` e as duas irmãs) dependem do contêiner onde o ícone
//! está (`.markers-panel`, `.zone-widget`...); aqui viram [`SeverityIconStyles`], que o dono do ícone aplica
//! com [`SeverityIconStyles::color`].
//!
//! Fica de fora o ramo `default: return ''` do `switch`: `Severity` é um enum fechado, então não há valor
//! fora dos quatro casos.

use je_base::severity::Severity;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_theme::ColorTheme;

/// `namespace SeverityIcon`.
pub struct SeverityIcon;

impl SeverityIcon {
    /// O `ThemeIcon` do codicon da severidade (`Codicon.info`, `Codicon.warning`, `Codicon.error`).
    pub fn theme_icon(severity: Severity) -> ThemeIcon {
        ThemeIcon::from_id(match severity {
            Severity::Ignore | Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        })
    }

    /// `SeverityIcon.className(severity)`.
    pub fn class_name(severity: Severity) -> String {
        let icon = Self::theme_icon(severity).as_class_name();
        match severity {
            Severity::Ignore => format!("severity-ignore {icon}"),
            _ => icon,
        }
    }
}

/// As cores de `severityIcon.css`: `--vscode-problemsErrorIcon-foreground` e as duas irmãs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeverityIconStyles {
    pub error: Option<Rgba>,
    pub warning: Option<Rgba>,
    pub info: Option<Rgba>,
}

impl SeverityIconStyles {
    /// A cor do ícone da severidade; `Ignore` usa o ícone de `info` e a cor dele.
    pub fn color(&self, severity: Severity) -> Option<Rgba> {
        match severity {
            Severity::Error => self.error,
            Severity::Warning => self.warning,
            Severity::Info | Severity::Ignore => self.info,
        }
    }
}

/// As cores de `severityIcon.css` resolvidas no tema.
pub fn default_severity_icon_styles(theme: &ColorTheme) -> SeverityIconStyles {
    SeverityIconStyles {
        error: theme.get_color("problemsErrorIcon.foreground"),
        warning: theme.get_color("problemsWarningIcon.foreground"),
        info: theme.get_color("problemsInfoIcon.foreground"),
    }
}
