#!/usr/bin/env bash
# Compila o scene-web para wasm, gera a cola com o wasm-bindgen-cli (mesma versão do Cargo.lock) e
# monta o diretório servido em target/scene-web-serve (fontes, texto e página). Uso:
#   testbench/scene-web/build.sh [porta]
# Sem porta, só monta; com porta, serve em 127.0.0.1. Para o ganho do SIMD do wasm, exporte
# RUSTFLAGS="-C target-feature=+simd128" antes. Abra: /index.html?mode=gpu|cpu&dense=1|4&frames=300
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build -p scene-web --target wasm32-unknown-unknown --release
out=target/scene-web-serve
rm -rf "$out"
mkdir -p "$out/assets"
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" target/wasm32-unknown-unknown/release/scene_web.wasm
cp testbench/scene-web/index.html "$out/"
cp "${SCENE_MONO:-/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf}" "$out/assets/mono.ttf"
cp "${SCENE_UI:-/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf}" "$out/assets/ui.ttf"
cp "${SCENE_SOURCE:-ref/vscode/src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeBase.ts}" "$out/assets/source.ts"
ls -l "$out/pkg"
if [ -n "${1:-}" ]; then
  exec python3 -m http.server "$1" --bind 127.0.0.1 --directory "$out"
fi
