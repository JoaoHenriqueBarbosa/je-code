# Compilação cruzada Linux -> Windows (x86_64-pc-windows-gnullvm) sem tocar no host (H23).
# O llvm-mingw traz as import libs do Windows (kernel32, user32, opengl32...), a CRT e o libunwind
# que o rust-lld sozinho não acha. Nada disso entra no produto: é ferramenta de link.
FROM rust:1.91-bookworm
ARG LLVM_MINGW=20250528
RUN curl -fsSL "https://github.com/mstorsjo/llvm-mingw/releases/download/${LLVM_MINGW}/llvm-mingw-${LLVM_MINGW}-ucrt-ubuntu-22.04-x86_64.tar.xz" \
      | tar -xJ -C /opt \
 && ln -s /opt/llvm-mingw-${LLVM_MINGW}-ucrt-ubuntu-22.04-x86_64 /opt/llvm-mingw \
 && rustup toolchain install 1.99.1 --profile minimal --target x86_64-pc-windows-gnullvm \
 && rustup default 1.99.1
ENV PATH=/opt/llvm-mingw/bin:$PATH \
    CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang
# libunwind e a CRT entram estáticos: o .exe não pode depender do libunwind.dll do llvm-mingw.
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-static"
