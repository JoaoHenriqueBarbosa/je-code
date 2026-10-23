#!/usr/bin/env bash
# Confere que nenhuma dependência compila ou liga código C, em cada alvo suportado.
#
# Duas camadas:
# 1. Lockfile: crates cuja razão de existir é fonte C (ban duro).
# O `yeslogic-fontconfig-sys` fica fora da lista: com a feature `dlopen` (que o fontique liga) ele só
# declara a ABI e abre a libfontconfig do sistema em tempo de execução, sem compilar nem ligar C.
# 2. Build scripts: roda `cargo check --target X` (os build.rs executam) e varre o que eles
#    emitiram em target/X/debug/build/*/output. `rustc-link-lib=static=` é C compilado por `cc`
#    (ou `cmake`) entrando no binário; `rustc-link-lib=dylib=` ou sem kind é ligação a lib C
#    do sistema em tempo de link (dlopen não aparece aqui e é aceito). Crate que só tem `cc` como
#    build-dependency sem usá-lo (ex.: wayland-backend sem a feature `log`) passa, porque o que
#    conta é o que foi compilado, não o que foi declarado.
set -euo pipefail
cd "$(dirname "$0")/.."

targets=(
  x86_64-unknown-linux-gnu
  x86_64-pc-windows-msvc
  aarch64-apple-darwin
  wasm32-unknown-unknown
)
banned='^(cmake|openssl-sys|libsqlite3-sys|ring|aws-lc-sys|aws-lc-rs|onig_sys|libz-sys|zstd-sys|bzip2-sys|freetype-sys|servo-fontconfig-sys|libgit2-sys|libssh2-sys|curl-sys|lzma-sys|bindgen|nasm-rs|libffi-sys|sqlite3-src|brotli-sys|libwebp-sys|mozjpeg-sys|libmimalloc-sys|tikv-jemalloc-sys) '

# Só o produto (crates/*) entra na régua. As sondas de testbench/ medem candidatos, inclusive os que
# a régua reprova (o turso completo, o Bevy), e não são distribuídas.
packages=()
for manifest in crates/*/Cargo.toml; do
  packages+=(-p "$(sed -n 's/^name = "\(.*\)"/\1/p' "$manifest" | head -1)")
done

status=0
for target in "${targets[@]}"; do
  if ! tree=$(cargo tree "${packages[@]}" --target "$target" -e normal,build --prefix none --format '{p}' 2>&1 | sort -u); then
    echo "[$target] cargo tree falhou:"; sed 's/^/  /' <<<"$tree"; status=1; continue
  fi
  hits=$(grep -E "$banned" <<<"$tree" || true)
  if [[ -n "$hits" ]]; then
    echo "[$target] crates de fonte C no lockfile:"; sed 's/^/  /' <<<"$hits"; status=1
  fi
  if ! cargo check "${packages[@]}" --target "$target" -q --target-dir target/pure-rust-check 2>/tmp/pure-rust-check.err; then
    echo "[$target] cargo check falhou:"; tail -20 /tmp/pure-rust-check.err | sed 's/^/  /'; status=1; continue
  fi
  linked=$(grep -lE '^cargo:(rustc-)?link-lib=' target/pure-rust-check/"$target"/debug/build/*/output 2>/dev/null || true)
  for out in $linked; do
    crate=$(basename "$(dirname "$out")")
    while IFS= read -r line; do
      case "$line" in
        *link-lib=static=*) echo "[$target] C compilado no binário por $crate: $line"; status=1 ;;
        *) echo "[$target] ligação a lib do sistema por $crate: $line" ;;
      esac
    done < <(grep -E '^cargo:(rustc-)?link-lib=' "$out")
  done
  if grep -qE '^cc ' <<<"$tree"; then
    echo "[$target] nota: cc está na árvore (build-dependency declarada); o que conta é a varredura acima"
  fi
done
if [[ $status -eq 0 ]]; then
  echo "só Rust: nenhum C compilado nem crate de fonte C em ${#targets[@]} alvos"
fi
exit $status
