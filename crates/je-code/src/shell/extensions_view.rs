//! A vista Extensions (`workbench/contrib/extensions/browser/extensionsViews.ts`) com as extensões instaladas: as
//! embutidas (a pasta `extensions` ao lado do executável, a mesma das linguagens e dos temas) e as do usuário
//! (`~/.vscode/extensions`, a pasta do VS Code), lidas pelo `ExtensionsScannerService` portado do
//! `extensionsScannerService.ts`, com nome, descrição e editor traduzidos pelo `package.nls.json`. As seções são as
//! do VS Code: `Installed` (as do usuário) e `Built-in`, cada linha com o nome em negrito, a versão, a descrição
//! e o editor. O marketplace fica de fora (exige rede e o serviço de galeria ligado).

use std::path::Path;
use std::rc::Rc;

use je_extensions::fs::DiskFs;
use je_extensions::manifest::ExtensionType;
use je_extensions::paths::{from_fs_path, join_path};
use je_extensions::profile::UserDataProfiles;
use je_extensions::profile_scanner::ExtensionsProfileScannerService;
use je_extensions::scanner::{ExtensionsProduct, ExtensionsScannerService, ExtensionsScannerServiceParts, ScannerEnvironment, ScannerLocations, SystemExtensionsScanOptions, TranslationsMap, UserExtensionsScanOptions};
use je_platform::log::NullLogService;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Content, NodeId, Role, Tree};
use je_widgets::text::{ellipsize, measure_text, ui_font};

#[derive(Clone, Debug)]
pub struct ExtensionEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub publisher: String,
    pub version: String,
    pub builtin: bool,
}

/// A pasta de extensões do usuário do VS Code (`~/.vscode/extensions`).
fn user_extensions_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(std::path::PathBuf::from(home).join(".vscode").join("extensions"))
}

/// Lê as extensões embutidas de `builtin_dir` e as do usuário.
pub fn scan(builtin_dir: &Path) -> Vec<ExtensionEntry> {
    let system = from_fs_path(&builtin_dir.to_string_lossy());
    let user = user_extensions_dir().map(|p| from_fs_path(&p.to_string_lossy())).unwrap_or_default();
    let cache = join_path(&user, &[".je-code-cache"]);
    let profiles = UserDataProfiles::with_default(&user, &cache, &cache);
    let fs = Rc::new(DiskFs);
    let logger = NullLogService::shared();
    let profile_scanner = Rc::new(ExtensionsProfileScannerService::new(&user, fs.clone(), profiles.clone(), logger.clone()));
    let service = ExtensionsScannerService::new(ExtensionsScannerServiceParts {
        locations: ScannerLocations {
            system_extensions_location: system,
            user_extensions_location: user.clone(),
            extensions_control_location: join_path(&cache, &["control.json"]),
            dev_builtin_extensions_location: join_path(&cache, &["builtInExtensions"]),
        },
        environment: ScannerEnvironment { is_built: true, language: "en".to_owned(), ..ScannerEnvironment::default() },
        product: ExtensionsProduct { version: "1.143.1".to_owned(), ..ExtensionsProduct::default() },
        current_profile: profiles.default_profile.clone(),
        profiles: profiles.clone(),
        fs,
        logger,
        profile_scanner,
        get_translations: Box::new(|_| TranslationsMap::new()),
    });
    let mut entries = Vec::new();
    if let Ok(list) = service.scan_system_extensions(&SystemExtensionsScanOptions::default()) {
        entries.extend(list.into_iter().map(|e| (e, true)));
    }
    let options = UserExtensionsScanOptions { profile_location: profiles.default_profile.extensions_resource.clone(), ..UserExtensionsScanOptions::default() };
    if let Ok(list) = service.scan_user_extensions(&options) {
        entries.extend(list.into_iter().filter(|e| e.extension_type == ExtensionType::User).map(|e| (e, false)));
    }
    let mut result: Vec<ExtensionEntry> = entries
        .into_iter()
        .map(|(e, builtin)| {
            let m = &e.manifest;
            ExtensionEntry {
                id: e.identifier.id.clone(),
                name: m.display_name.clone().unwrap_or_else(|| m.name.clone()),
                description: m.description.clone().unwrap_or_default(),
                publisher: e.publisher_display_name.clone().unwrap_or_else(|| m.publisher.clone()),
                version: m.version.clone(),
                builtin,
            }
        })
        .collect();
    result.sort_by(|a, b| a.builtin.cmp(&b.builtin).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    result
}

/// A altura de uma linha da lista (`.extension-list-item`, 62 px no oráculo).
pub const ROW_HEIGHT: f64 = 62.0;

/// Pinta a lista (seções e linhas visíveis a partir de `scroll`) como filhos de `parent`, de `top` até `bottom`.
#[allow(clippy::too_many_arguments)]
pub fn render(tree: &mut Tree, parent: NodeId, entries: &[ExtensionEntry], filter: &str, width: f64, top: f64, bottom: f64, scroll: f64, theme: &ColorTheme) -> f64 {
    let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0, 0xff));
    let fg = color("sideBar.foreground");
    let muted = color("descriptionForeground");
    let filter = filter.to_lowercase();
    let visible: Vec<&ExtensionEntry> = entries.iter().filter(|e| filter.is_empty() || e.name.to_lowercase().contains(&filter) || e.id.to_lowercase().contains(&filter) || e.description.to_lowercase().contains(&filter)).collect();
    let clip = tree.create(parent, Role::List);
    tree.set_bounds(clip, Rect::new(5.0, top, width - 6.0, bottom));
    tree.style_mut(clip).clip = true;
    let inner = width - 11.0;
    let mut y = -scroll;
    for (builtin, title) in [(false, "Installed"), (true, "Built-in")] {
        let group: Vec<&&ExtensionEntry> = visible.iter().filter(|e| e.builtin == builtin).collect();
        // O cabeçalho da seção (`.pane-header`): 22 px, 11 px negrito, com a contagem à direita.
        let header = tree.create(clip, Role::Heading);
        tree.set_bounds(header, Rect::new(0.0, y, inner, y + 22.0));
        let chevron = tree.create(header, Role::Generic);
        tree.set_bounds(chevron, Rect::new(2.0, 3.0, 18.0, 19.0));
        tree.style_mut(chevron).content = Content::Codicon { id: "chevron-down".into(), size: 16.0, color: fg };
        let label = tree.create(header, Role::Generic);
        tree.set_bounds(label, Rect::new(20.0, 0.0, inner - 40.0, 22.0));
        tree.style_mut(label).content = Content::Text { text: title.to_uppercase(), style: ui_font(11.0, 700), color: fg, line_height: 22.0 };
        let count = group.len().to_string();
        let badge_font = ui_font(10.0, 400);
        let bw = (measure_text(&count, &badge_font) + 10.0).max(19.0).round();
        let badge = tree.create(header, Role::Generic);
        tree.set_bounds(badge, Rect::new(inner - bw - 4.0, 1.5, inner - 4.0, 20.5));
        let style = tree.style_mut(badge);
        style.background = Some(color("badge.background"));
        style.radius = 9.5;
        let t = tree.create(badge, Role::Generic);
        tree.set_bounds(t, Rect::new((bw - measure_text(&count, &badge_font)) / 2.0, 0.0, bw, 19.0));
        tree.style_mut(t).content = Content::Text { text: count, style: badge_font, color: color("badge.foreground"), line_height: 19.0 };
        y += 22.0;
        for entry in group {
            if y + ROW_HEIGHT >= 0.0 && y < bottom - top {
                let row = tree.create(clip, Role::ListItem);
                tree.set_bounds(row, Rect::new(0.0, y, inner, y + ROW_HEIGHT));
                // O ícone (sem a imagem da extensão: o codicon de extensões, como o VS Code mostra sem ícone).
                let icon = tree.create(row, Role::Image);
                tree.set_bounds(icon, Rect::new(10.0, 10.0, 52.0, 52.0));
                tree.style_mut(icon).content = Content::Codicon { id: "extensions".into(), size: 32.0, color: muted };
                let x = 62.0;
                let room = inner - x - 8.0;
                let name_font = ui_font(13.0, 700);
                let name = ellipsize(&entry.name, &name_font, room - 60.0);
                let nw = measure_text(&name, &name_font);
                let n = tree.create(row, Role::Generic);
                tree.set_bounds(n, Rect::new(x, 8.0, x + nw, 26.0));
                tree.style_mut(n).content = Content::Text { text: name, style: name_font, color: fg, line_height: 18.0 };
                let small = ui_font(12.0, 400);
                let v = tree.create(row, Role::Generic);
                let vw = measure_text(&entry.version, &small);
                tree.set_bounds(v, Rect::new(x + nw + 6.0, 9.0, x + nw + 6.0 + vw, 26.0));
                tree.style_mut(v).content = Content::Text { text: entry.version.clone(), style: small.clone(), color: muted, line_height: 17.0 };
                let d = tree.create(row, Role::Generic);
                tree.set_bounds(d, Rect::new(x, 26.0, x + room, 44.0));
                tree.style_mut(d).content = Content::Text { text: ellipsize(&entry.description, &ui_font(13.0, 400), room), style: ui_font(13.0, 400), color: fg, line_height: 18.0 };
                let p = tree.create(row, Role::Generic);
                tree.set_bounds(p, Rect::new(x, 44.0, x + room, 60.0));
                tree.style_mut(p).content = Content::Text { text: entry.publisher.clone(), style: small, color: muted, line_height: 16.0 };
            }
            y += ROW_HEIGHT;
        }
    }
    y + scroll
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_the_builtin_extensions_with_translated_names() {
        let Some(dir) = je_editor_view::environment::find_builtin_extensions() else { return };
        let entries = scan(&dir);
        let ts = entries.iter().find(|e| e.id == "vscode.typescript").expect("a embutida do TypeScript");
        assert!(ts.builtin);
        assert_eq!(ts.name, "TypeScript Language Basics", "o nome vem do package.nls.json");
        assert!(entries.iter().filter(|e| e.builtin).count() > 50);
    }
}
