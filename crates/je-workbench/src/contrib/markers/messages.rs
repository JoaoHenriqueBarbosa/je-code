//! Porte de `src/vs/workbench/contrib/markers/browser/messages.ts`: os textos do painel de problemas (`Messages`).
//!
//! Notas de tradução:
//! - **`class Messages` com membros estáticos** é a struct sem campos [`Messages`], com uma função associada por membro.
//!   Os membros de texto fixo (`static X: string = nls.localize(...)`) viram funções sem argumentos, porque o texto
//!   localizado depende do idioma ativo no momento da chamada; os de função (`(n) => localize(...)`) recebem os mesmos
//!   argumentos, numéricos como `i64` e formatados com `toString` (o `'' + n` do original).
//! - **`nls.localize2`** devolve o [`LocalizedString`] (valor e original).
//! - **`MARKERS_TREE_ARIA_LABEL_MARKER`.** Reproduz os argumentos do original ao pé da letra, inclusive o ramo `default`
//!   com origem, em que o primeiro argumento é a origem (`{0}`) e a mensagem fica em `{1}`, e o ramo de aviso sem origem,
//!   que passa o texto das informações relacionadas duas vezes.

use je_base::nls::localize;
use je_platform::action::{LocalizedString, localize2};
use je_platform::markers::markers::{MarkerSeverity, RelatedInformation as IRelatedInformation};
use je_platform::resources::ext_uri;

use super::markers_model::Marker;

/// `Messages`.
pub struct Messages;

impl Messages {
    pub fn markers_panel_toggle_label() -> String {
        localize("problems.view.toggle.label", "Toggle Problems (Errors, Warnings, Infos)", &[])
    }

    pub fn markers_panel_show_label() -> LocalizedString {
        localize2("problems.view.focus.label", "Focus Problems (Errors, Warnings, Infos)")
    }

    pub fn problems_panel_configuration_title() -> String {
        localize("problems.panel.configuration.title", "Problems View", &[])
    }

    pub fn problems_panel_configuration_auto_reveal() -> String {
        localize("problems.panel.configuration.autoreveal", "Controls whether Problems view should automatically reveal files when opening them.", &[])
    }

    pub fn problems_panel_configuration_view_mode() -> String {
        localize("problems.panel.configuration.viewMode", "Controls the default view mode of the Problems view.", &[])
    }

    pub fn problems_panel_configuration_show_current_status() -> String {
        localize("problems.panel.configuration.showCurrentInStatus", "When enabled shows the current problem in the status bar.", &[])
    }

    pub fn problems_panel_configuration_compare_order() -> String {
        localize("problems.panel.configuration.compareOrder", "Controls the order in which problems are navigated.", &[])
    }

    pub fn problems_panel_configuration_compare_order_severity() -> String {
        localize("problems.panel.configuration.compareOrder.severity", "Navigate problems ordered by severity", &[])
    }

    pub fn problems_panel_configuration_compare_order_position() -> String {
        localize("problems.panel.configuration.compareOrder.position", "Navigate problems ordered by position", &[])
    }

    pub fn markers_panel_title_problems() -> LocalizedString {
        localize2("markers.panel.title.problems", "Problems")
    }

    pub fn markers_panel_no_problems_built() -> String {
        localize("markers.panel.no.problems.build", "No problems have been detected in the workspace.", &[])
    }

    pub fn markers_panel_no_problems_active_file_built() -> String {
        localize("markers.panel.no.problems.activeFile.build", "No problems have been detected in the current file.", &[])
    }

    pub fn markers_panel_no_problems_filters() -> String {
        localize("markers.panel.no.problems.filters", "No results found with provided filter criteria.", &[])
    }

    pub fn markers_panel_action_tooltip_more_filters() -> String {
        localize("markers.panel.action.moreFilters", "More Filters...", &[])
    }

    pub fn markers_panel_filter_label_show_errors() -> String {
        localize("markers.panel.filter.showErrors", "Show Errors", &[])
    }

    pub fn markers_panel_filter_label_show_warnings() -> String {
        localize("markers.panel.filter.showWarnings", "Show Warnings", &[])
    }

    pub fn markers_panel_filter_label_show_infos() -> String {
        localize("markers.panel.filter.showInfos", "Show Infos", &[])
    }

    pub fn markers_panel_filter_label_excluded_files() -> String {
        localize("markers.panel.filter.useFilesExclude", "Hide Excluded Files", &[])
    }

    pub fn markers_panel_filter_label_active_file() -> String {
        localize("markers.panel.filter.activeFile", "Show Active File Only", &[])
    }

    pub fn markers_panel_action_tooltip_filter() -> String {
        localize("markers.panel.action.filter", "Filter Problems", &[])
    }

    pub fn markers_panel_action_tooltip_quickfix() -> String {
        localize("markers.panel.action.quickfix", "Show fixes", &[])
    }

    pub fn markers_panel_filter_aria_label() -> String {
        localize("markers.panel.filter.ariaLabel", "Filter Problems", &[])
    }

    pub fn markers_panel_filter_placeholder() -> String {
        localize("markers.panel.filter.placeholder", "Filter (e.g. text, **/*.ts, !**/node_modules/**, @source:ts)", &[])
    }

    pub fn markers_panel_filter_errors() -> String {
        localize("markers.panel.filter.errors", "errors", &[])
    }

    pub fn markers_panel_filter_warnings() -> String {
        localize("markers.panel.filter.warnings", "warnings", &[])
    }

    pub fn markers_panel_filter_infos() -> String {
        localize("markers.panel.filter.infos", "infos", &[])
    }

    pub fn markers_panel_single_error_label() -> String {
        localize("markers.panel.single.error.label", "1 Error", &[])
    }

    pub fn markers_panel_multiple_errors_label(no_of_errors: i64) -> String {
        localize("markers.panel.multiple.errors.label", "{0} Errors", &[&no_of_errors.to_string()])
    }

    pub fn markers_panel_single_warning_label() -> String {
        localize("markers.panel.single.warning.label", "1 Warning", &[])
    }

    pub fn markers_panel_multiple_warnings_label(no_of_warnings: i64) -> String {
        localize("markers.panel.multiple.warnings.label", "{0} Warnings", &[&no_of_warnings.to_string()])
    }

    pub fn markers_panel_single_info_label() -> String {
        localize("markers.panel.single.info.label", "1 Info", &[])
    }

    pub fn markers_panel_multiple_infos_label(no_of_infos: i64) -> String {
        localize("markers.panel.multiple.infos.label", "{0} Infos", &[&no_of_infos.to_string()])
    }

    pub fn markers_panel_single_unknown_label() -> String {
        localize("markers.panel.single.unknown.label", "1 Unknown", &[])
    }

    pub fn markers_panel_multiple_unknowns_label(no_of_unknowns: i64) -> String {
        localize("markers.panel.multiple.unknowns.label", "{0} Unknowns", &[&no_of_unknowns.to_string()])
    }

    pub fn markers_panel_at_line_col_number(ln: i64, col: i64) -> String {
        localize("markers.panel.at.ln.col.number", "[Ln {0}, Col {1}]", &[&ln.to_string(), &col.to_string()])
    }

    pub fn markers_tree_aria_label_resource(no_of_problems: usize, file_name: &str, folder: &str) -> String {
        localize("problems.tree.aria.label.resource", "{0} problems in file {1} of folder {2}", &[&no_of_problems.to_string(), file_name, folder])
    }

    pub fn markers_tree_aria_label_marker(marker: &Marker) -> String {
        let related_information_message = if marker.related_information.is_empty() {
            String::new()
        } else {
            localize("problems.tree.aria.label.marker.relatedInformation", " This problem has references to {0} locations.", &[&marker.related_information.len().to_string()])
        };
        let raw = &marker.marker;
        let (message, line, column) = (raw.message.as_str(), raw.start_line_number.to_string(), raw.start_column.to_string());
        let source = raw.source.as_deref().filter(|source| !source.is_empty());
        let related = related_information_message.as_str();
        match raw.severity {
            MarkerSeverity::ERROR => match source {
                Some(source) => {
                    localize("problems.tree.aria.label.error.marker", "Error: {0} at line {1} and character {2}.{3} generated by {4}", &[message, &line, &column, related, source])
                }
                None => localize("problems.tree.aria.label.error.marker.nosource", "Error: {0} at line {1} and character {2}.{3}", &[message, &line, &column, related]),
            },
            MarkerSeverity::WARNING => match source {
                Some(source) => {
                    localize("problems.tree.aria.label.warning.marker", "Warning: {0} at line {1} and character {2}.{3} generated by {4}", &[message, &line, &column, related, source])
                }
                None => localize("problems.tree.aria.label.warning.marker.nosource", "Warning: {0} at line {1} and character {2}.{3}", &[message, &line, &column, related, related]),
            },
            MarkerSeverity::INFO => match source {
                Some(source) => localize("problems.tree.aria.label.info.marker", "Info: {0} at line {1} and character {2}.{3} generated by {4}", &[message, &line, &column, related, source]),
                None => localize("problems.tree.aria.label.info.marker.nosource", "Info: {0} at line {1} and character {2}.{3}", &[message, &line, &column, related]),
            },
            _ => match source {
                Some(source) => {
                    localize("problems.tree.aria.label.marker", "Problem: {0} at line {1} and character {2}.{3} generated by {4}", &[source, message, &line, &column, related, source])
                }
                None => localize("problems.tree.aria.label.marker.nosource", "Problem: {0} at line {1} and character {2}.{3}", &[message, &line, &column, related]),
            },
        }
    }

    pub fn markers_tree_aria_label_related_information(related_information: &IRelatedInformation) -> String {
        localize(
            "problems.tree.aria.label.relatedinfo.message",
            "{0} at line {1} and character {2} in {3}",
            &[
                related_information.message.as_str(),
                &related_information.start_line_number.to_string(),
                &related_information.start_column.to_string(),
                &ext_uri().basename(&related_information.resource, None),
            ],
        )
    }

    pub fn show_errors_warnings_action_label() -> String {
        localize("errors.warnings.show.label", "Show Errors and Warnings", &[])
    }
}
