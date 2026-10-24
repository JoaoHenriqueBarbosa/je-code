//! O tema de ícones de arquivo (`vs/workbench/services/themes/browser/fileIconThemeData.ts`) com o tema padrão
//! do VS Code, o Seti (`extensions/theme-seti/icons/vs-seti-icon-theme.json`): fonte de ícones em WOFF, um
//! glifo e uma cor por definição, escolhidos pelo nome do arquivo, pela extensão (a mais longa primeiro, como a
//! especificidade das regras de CSS que o `FileIconThemeLoader` gera) e pelo id da linguagem.
//!
//! O Seti não define ícone de pasta: as pastas ficam só com o twistie, como no VS Code.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::{TextStyle, with_text_system, woff_to_sfnt};
use je_theme::color::Color;
use je_theme::to_rgba8;
use je_ui::{Content, NodeId, Role, Tree};
use serde_json::Value;

const THEME: &str = "theme-seti/icons/vs-seti-icon-theme.json";
/// `size: 150%` da fonte do tema, sobre os 13 px do rótulo.
const ICON_SIZE: f32 = 19.5;

#[derive(Clone)]
pub struct FileIconTheme(Rc<Inner>);

struct Inner {
    family: String,
    /// id da definição para (glifo, cor).
    definitions: HashMap<String, (char, Rgba)>,
    file_names: HashMap<String, String>,
    file_extensions: HashMap<String, String>,
    language_ids: HashMap<String, String>,
    file: Option<String>,
}

fn string_map(value: &Value) -> HashMap<String, String> {
    value.as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.to_lowercase(), v.as_str()?.to_owned()))).collect()).unwrap_or_default()
}

impl FileIconTheme {
    /// Carrega o Seti da pasta de extensões embutidas e registra a fonte dele.
    pub fn load(extensions: &Path) -> Result<Self, String> {
        let path = extensions.join(THEME);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let json: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let font = json["fonts"][0]["src"][0]["path"].as_str().ok_or("tema de ícones sem fonte")?;
        let font_path = path.parent().unwrap_or(Path::new(".")).join(font);
        let bytes = std::fs::read(&font_path).map_err(|e| format!("{}: {e}", font_path.display()))?;
        let sfnt = if bytes.starts_with(b"wOFF") { woff_to_sfnt(&bytes)? } else { bytes };
        let family = with_text_system(|t| t.register_font(sfnt)).into_iter().next().ok_or("fonte de ícones sem família")?;
        let definitions = json["iconDefinitions"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(id, def)| {
                        let character = def["fontCharacter"].as_str()?;
                        let glyph = char::from_u32(u32::from_str_radix(character.trim_start_matches('\\'), 16).ok()?)?;
                        let color = def["fontColor"].as_str().map_or(Rgba::opaque(0xd4, 0xd7, 0xd6), |c| to_rgba8(&Color::from_hex(c)));
                        Some((id.clone(), (glyph, color)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self(Rc::new(Inner {
            family,
            definitions,
            file_names: string_map(&json["fileNames"]),
            file_extensions: string_map(&json["fileExtensions"]),
            language_ids: string_map(&json["languageIds"]),
            file: json["file"].as_str().map(str::to_owned),
        })))
    }

    /// O glifo e a cor do arquivo `name` (com o id da linguagem que o VS Code detectou para ele).
    pub fn icon_for(&self, name: &str, language_id: Option<&str>) -> Option<(char, Rgba)> {
        let inner = &self.0;
        let lower = name.to_lowercase();
        let by_name = inner.file_names.get(&lower);
        // `a.b.c` tenta `b.c` e depois `c`: a regra de extensão mais longa é a mais específica.
        let by_extension = || lower.match_indices('.').find_map(|(i, _)| inner.file_extensions.get(&lower[i + 1..]));
        let by_language = || language_id.and_then(|id| inner.language_ids.get(id));
        let id = by_name.or_else(by_extension).or_else(by_language).or(inner.file.as_ref())?;
        inner.definitions.get(id).copied()
    }

    /// Põe o ícone de `name` num filho novo de `parent`, centrado em `rect`.
    pub fn decorate(&self, tree: &mut Tree, parent: NodeId, rect: Rect, name: &str, language_id: Option<&str>) {
        let Some((glyph, color)) = self.icon_for(name, language_id) else { return };
        let node = tree.create(parent, Role::Image);
        let style = TextStyle::new(&self.0.family, ICON_SIZE, 400);
        let width = f64::from(with_text_system(|t| t.line(&glyph.to_string(), &style)).width);
        let x = rect.x0 + (rect.width() - width) / 2.0;
        tree.set_bounds(node, Rect::new(x, rect.y0, x + width, rect.y1));
        tree.style_mut(node).content = Content::Text { text: glyph.to_string(), style, color, line_height: rect.height() };
    }
}
