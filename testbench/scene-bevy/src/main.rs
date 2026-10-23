//! Sonda H05: a cena de workbench do `scene-bench` montada com o Bevy (bevy_ui + bevy_text).
//!
//! Mesmo conteúdo e mesma rolagem de um pixel por frame. As linhas do editor e do minimap são
//! virtualizadas como no VS Code: só as visíveis existem como entidades, e a linha que entra na
//! viewport é diagramada uma vez e depois só reposicionada.
//! Variáveis: `SCENE_MONO`, `SCENE_UI`, `SCENE_SOURCE`, `SCENE_DENSE` e `SCENE_SECONDS`.

use std::collections::HashMap;
use std::time::Duration;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};

const WIDTH: f32 = 1920.0;
const HEIGHT: f32 = 1080.0;
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 19.0;
const ACTIVITY_BAR: f32 = 48.0;
const SIDEBAR: f32 = 300.0;
const TAB_BAR: f32 = 35.0;
const STATUS_BAR: f32 = 22.0;
const GUTTER: f32 = 64.0;
const MINIMAP: f32 = 110.0;

const BACKGROUND: Color = Color::srgb_u8(0x1f, 0x1f, 0x1f);
const SIDEBAR_BG: Color = Color::srgb_u8(0x18, 0x18, 0x18);
const BORDER: Color = Color::srgb_u8(0x2b, 0x2b, 0x2b);
const LINE_NUMBER: Color = Color::srgb_u8(0x6e, 0x76, 0x81);
const FOREGROUND: Color = Color::srgb_u8(0xcc, 0xcc, 0xcc);
const TOKEN_COLORS: [Color; 6] = [
    Color::srgb_u8(0x56, 0x9c, 0xd6),
    Color::srgb_u8(0x9c, 0xdc, 0xfe),
    Color::srgb_u8(0xce, 0x91, 0x78),
    Color::srgb_u8(0xdc, 0xdc, 0xaa),
    Color::srgb_u8(0x4e, 0xc9, 0xb0),
    Color::srgb_u8(0x6a, 0x99, 0x55),
];

fn token_color(word: &str) -> Color {
    let hash = word.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    TOKEN_COLORS[(hash % TOKEN_COLORS.len() as u32) as usize]
}

fn tokens(text: &str) -> Vec<(String, Color, bool)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_word = false;
    for (i, ch) in text.char_indices() {
        let word = ch.is_alphanumeric() || ch == '_';
        if i > start && word != in_word {
            let piece = &text[start..i];
            out.push((piece.to_owned(), if in_word { token_color(piece) } else { FOREGROUND }, in_word));
            start = i;
        }
        in_word = word;
    }
    if start < text.len() {
        let piece = &text[start..];
        out.push((piece.to_owned(), if in_word { token_color(piece) } else { FOREGROUND }, in_word));
    }
    out
}

#[derive(Resource)]
struct Source {
    lines: Vec<String>,
    mono: Handle<Font>,
    seconds: f32,
}

#[derive(Resource, Default)]
struct Visible {
    code: HashMap<usize, Entity>,
    minimap: HashMap<usize, Entity>,
}

#[derive(Component)]
struct Viewport;

#[derive(Component)]
struct Minimap;

#[derive(Component)]
struct CodeLine(usize);

#[derive(Component)]
struct MinimapRow(usize);

#[derive(Resource, Default)]
struct Frame(u64);

fn read(var: &str, default: &str) -> Vec<u8> {
    let path = std::env::var(var).unwrap_or_else(|_| default.to_owned());
    std::fs::read(&path).unwrap_or_else(|e| panic!("{var}={path}: {e}"))
}

fn absolute(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(width),
        height: Val::Px(height),
        ..default()
    }
}

fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let dense: usize = std::env::var("SCENE_DENSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let text = String::from_utf8(read(
        "SCENE_SOURCE",
        "ref/vscode/src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeBase.ts",
    ))
    .expect("UTF-8");
    let lines = text.lines().map(|l| l.replace('\t', "    ").repeat(dense)).collect();
    let mono = fonts.add(Font::from_bytes(read("SCENE_MONO", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf")));
    let ui = fonts.add(Font::from_bytes(read("SCENE_UI", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")));
    let seconds = std::env::var("SCENE_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(6.0);
    commands.insert_resource(Source { lines, mono, seconds });

    commands.spawn(Camera2d);
    let root = commands
        .spawn((absolute(0.0, 0.0, WIDTH, HEIGHT), BackgroundColor(BACKGROUND)))
        .id();
    let body = HEIGHT - STATUS_BAR;
    let mut parts = vec![
        commands.spawn((absolute(0.0, 0.0, ACTIVITY_BAR, body), BackgroundColor(SIDEBAR_BG))).id(),
        commands.spawn((absolute(ACTIVITY_BAR, 0.0, SIDEBAR, body), BackgroundColor(SIDEBAR_BG))).id(),
        commands.spawn((absolute(ACTIVITY_BAR + SIDEBAR, 0.0, 1.0, body), BackgroundColor(BORDER))).id(),
        commands.spawn((absolute(0.0, body, WIDTH, STATUS_BAR), BackgroundColor(SIDEBAR_BG))).id(),
    ];
    for i in 0..8 {
        parts.push(commands.spawn((absolute(12.0, 12.0 + 48.0 * i as f32, 24.0, 24.0), BackgroundColor(LINE_NUMBER))).id());
    }
    for i in 0..44 {
        let name = format!("{}pieceTreeTextBuffer{i}.ts", "  ".repeat(i % 4));
        parts.push(
            commands
                .spawn((
                    absolute(ACTIVITY_BAR + 20.0, 40.0 + 22.0 * i as f32, SIDEBAR - 20.0, 22.0),
                    Text::new(name),
                    TextFont { font: ui.clone().into(), font_size: FONT_SIZE.into(), ..default() },
                    TextColor(FOREGROUND),
                ))
                .id(),
        );
    }
    let editor_x = ACTIVITY_BAR + SIDEBAR + 1.0;
    for t in 0..6 {
        let x = editor_x + 160.0 * t as f32;
        let bg = if t == 0 { BACKGROUND } else { SIDEBAR_BG };
        parts.push(commands.spawn((absolute(x, 0.0, 159.0, TAB_BAR), BackgroundColor(bg))).id());
        parts.push(commands.spawn((absolute(x + 159.0, 0.0, 1.0, TAB_BAR), BackgroundColor(BORDER))).id());
    }
    let mut viewport = absolute(editor_x, TAB_BAR, WIDTH - editor_x - MINIMAP - 14.0, body - TAB_BAR);
    viewport.overflow = Overflow::clip();
    parts.push(commands.spawn((viewport, Viewport)).id());
    let mut minimap = absolute(WIDTH - MINIMAP - 14.0, TAB_BAR, MINIMAP, body - TAB_BAR);
    minimap.overflow = Overflow::clip();
    parts.push(commands.spawn((minimap, BackgroundColor(BACKGROUND), Minimap)).id());
    commands.entity(root).add_children(&parts);
}

/// Rola um pixel por frame: cria as linhas que entram, remove as que saem, reposiciona as demais.
#[allow(clippy::too_many_arguments)]
fn scroll(
    mut commands: Commands,
    source: Res<Source>,
    mut frame: ResMut<Frame>,
    mut visible: ResMut<Visible>,
    viewport: Single<Entity, With<Viewport>>,
    minimap: Single<Entity, With<Minimap>>,
    mut code_nodes: Query<(&CodeLine, &mut Node), Without<MinimapRow>>,
    mut minimap_nodes: Query<(&MinimapRow, &mut Node), Without<CodeLine>>,
) {
    let height = HEIGHT - STATUS_BAR - TAB_BAR;
    let total = source.lines.len() as f32 * LINE_HEIGHT;
    let scroll = (frame.0 as f32) % (total - height).max(1.0);
    frame.0 += 1;
    let first = (scroll / LINE_HEIGHT) as usize;
    let last = (first + (height / LINE_HEIGHT) as usize + 2).min(source.lines.len());
    visible.code.retain(|&index, entity| {
        let keep = (first..last).contains(&index);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    for index in first..last {
        if visible.code.contains_key(&index) {
            continue;
        }
        let font = TextFont { font: source.mono.clone().into(), font_size: FONT_SIZE.into(), ..default() };
        let line = commands
            .spawn((
                absolute(0.0, 0.0, 4000.0, LINE_HEIGHT),
                CodeLine(index),
                children![(
                    absolute(8.0, 0.0, GUTTER, LINE_HEIGHT),
                    Text::new(format!("{:>5}", index + 1)),
                    font.clone(),
                    TextColor(LINE_NUMBER),
                )],
            ))
            .id();
        let text = commands
            .spawn((absolute(GUTTER, 0.0, 4000.0, LINE_HEIGHT), Text::default(), font.clone(), TextColor(FOREGROUND)))
            .id();
        for (piece, color, _) in tokens(&source.lines[index]) {
            let span = commands.spawn((TextSpan::new(piece), font.clone(), TextColor(color))).id();
            commands.entity(text).add_child(span);
        }
        commands.entity(line).add_child(text);
        commands.entity(*viewport).add_child(line);
        visible.code.insert(index, line);
    }
    for (line, mut node) in &mut code_nodes {
        node.top = Val::Px(line.0 as f32 * LINE_HEIGHT - scroll);
    }

    let minimap_first = (scroll / 4.0) as usize;
    let minimap_last = (minimap_first + (height / 2.0) as usize).min(source.lines.len());
    visible.minimap.retain(|&index, entity| {
        let keep = (minimap_first..minimap_last).contains(&index);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    for index in minimap_first..minimap_last {
        if visible.minimap.contains_key(&index) {
            continue;
        }
        let row = commands.spawn((absolute(0.0, 0.0, MINIMAP, 2.0), MinimapRow(index))).id();
        let mut column = 0.0;
        for (piece, color, word) in tokens(&source.lines[index]) {
            let len = piece.chars().count() as f32;
            if word {
                let rect = commands.spawn((absolute(column, 0.0, len, 2.0), BackgroundColor(color))).id();
                commands.entity(row).add_child(rect);
            }
            column += len;
        }
        commands.entity(*minimap).add_child(row);
        visible.minimap.insert(index, row);
    }
    for (row, mut node) in &mut minimap_nodes {
        node.top = Val::Px(2.0 * (row.0 - minimap_first) as f32);
    }
}

fn finish(time: Res<Time<Real>>, source: Res<Source>, diagnostics: Res<DiagnosticsStore>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() < source.seconds {
        return;
    }
    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.average());
    let frame_time = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.average());
    println!(
        "bevy: {:.1} fps médio, {:.2} ms por frame (média das últimas {} amostras) após {:.1} s",
        fps.unwrap_or(0.0),
        frame_time.unwrap_or(0.0),
        HISTORY,
        time.elapsed_secs()
    );
    exit.write(AppExit::Success);
}

const HISTORY: usize = 240;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "je-code scene-bevy".into(),
                    resolution: WindowResolution::new(WIDTH as u32, HEIGHT as u32).with_scale_factor_override(1.0),
                    present_mode: PresentMode::AutoNoVsync,
                    resizable: false,
                    ..default()
                }),
                ..default()
            }),
            FrameTimeDiagnosticsPlugin { max_history_length: HISTORY, smoothing_factor: 2.0 / (HISTORY as f64 + 1.0) },
        ))
        .insert_resource(Time::<Fixed>::from_duration(Duration::from_millis(16)))
        .init_resource::<Visible>()
        .init_resource::<Frame>()
        .add_systems(Startup, setup)
        .add_systems(Update, (scroll, finish))
        .run();
}
