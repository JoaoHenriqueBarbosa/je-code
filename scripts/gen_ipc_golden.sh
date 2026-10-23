#!/usr/bin/env bash
# Gera crates/je-base/tests/golden/ipc.txt: roda o ipc.ts, o ipc.net.ts e o webSocketFraming.ts reais do VS Code
# (ref/vscode, 1.143.1) no bun e grava os bytes que eles produzem, para o porte em Rust conferir byte a byte.
# Uso: scripts/gen_ipc_golden.sh
#
# O bun usa os decoradores do TC39, e o VS Code usa os do TypeScript (`experimentalDecorators`), então o script
# copia `vs/base` para um diretório temporário com o `tsconfig.json` certo e roda de lá.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/src/vs" "$work/scripts"
cp -r "$root/ref/vscode/src/vs/base" "$work/src/vs/"
cp "$root"/ref/vscode/src/vs/nls*.ts "$work/src/vs/"
echo '{"compilerOptions":{"experimentalDecorators":true,"useDefineForClassFields":false,"target":"ES2022","module":"ESNext"}}' > "$work/tsconfig.json"
cp "$root/scripts/gen_ipc_golden.ts.txt" "$work/scripts/gen.ts"
(cd "$work" && bun run scripts/gen.ts) > "$root/crates/je-base/tests/golden/ipc.txt"
