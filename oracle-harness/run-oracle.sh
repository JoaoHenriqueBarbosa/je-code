#!/usr/bin/env bash
# Sobe o VS Code real (ref/oracle) sob xvfb com CDP na porta 9333, perfil isolado em wip/scratch.
# Uso: oracle-harness/run-oracle.sh <segundos de vida> [arquivos ou pastas para abrir...]
# Sai quando o CDP responde; o oráculo morre sozinho ao fim do prazo.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
life="$1"; shift
ud="$root/wip/scratch/oracle-ud"
mkdir -p "$ud/User"
[[ -f "$ud/User/settings.json" ]] || printf '{\n  "chat.disableAIFeatures": true,\n  "workbench.secondarySideBar.defaultVisibility": "hidden",\n  "telemetry.telemetryLevel": "off",\n  "update.mode": "none"\n}\n' > "$ud/User/settings.json"
cd "$root/ref/oracle/VSCode-linux-x64"
DONT_PROMPT_WSL_INSTALL=1 setsid timeout "$life" xvfb-run -a -s '-screen 0 1280x800x24' ./code --no-sandbox --disable-gpu \
  --user-data-dir="$ud" --extensions-dir="$root/wip/scratch/oracle-ext" --skip-welcome --skip-release-notes \
  --disable-workspace-trust --remote-debugging-port=9333 --new-window "$@" >"$root/wip/scratch/oracle.log" 2>&1 </dev/null &
for _ in $(seq 1 40); do
  curl -s --max-time 1 http://127.0.0.1:9333/json/version >/dev/null && exit 0
  sleep 0.5
done
echo "oráculo não respondeu; ver wip/scratch/oracle.log" >&2
exit 1
