//! Porte de `src/vs/workbench/api/test/node/extHostTunnelService.test.ts` (suíte `ExtHostTunnelService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda criado no começo de cada caso (o `setup` roda na criação e o
//!   `teardown` no descarte).
//! - **Literais com `template string`.** `tcp`, `tcp6` e `procSockets` mantêm o recuo dos literais do original (as linhas depois
//!   da primeira começam com tabulações), montadas por [`table`] linha a linha com o `\n` do literal.
//! - **`Object.keys(result).length` e `result[key].pid`.** O `getSockets` devolve um `HashMap` pelo número do socket
//!   (`result.len()` e `result.values()`).
//! - **`deepStrictEqual(result[0], {...})`.** O objeto de colunas é um `BTreeMap` de texto para texto, comparado com o esperado
//!   do original (as chaves numéricas `10` a `16` são o texto do índice).
//! - **`loadListeningPorts(tcp, tcp6)`.** Recebe a lista de textos (`...stdouts`).
//! - **`tryFindRootPorts([...], psStdOut, new Map())`.** O `Map` vazio é o `RootPortMap::new()`; `result.size` é `len()` e
//!   `result.get(5000)?.pid` é `get(5000)` com o `pid`.
//! - **`findPorts` assíncrona.** Devolve uma `Promise` e o caso espera o resultado dentro do laço de eventos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_workbench::api::node::ext_host_tunnel_service::{
    ListeningPort, ProcessInfo, RootPortMap, find_ports, get_root_processes, get_sockets, load_connection_table, load_listening_ports, parse_ip_address,
    try_find_root_ports,
};

/// Junta as linhas de um literal do original, cada uma terminada pelo `\n`.
fn table(lines: &[&str]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

fn tcp() -> String {
    table(&[
        "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode",
        "\t0: 00000000:0BBA 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 2335214 1 0000000010173312 100 0 0 10 0",
        "\t1: 00000000:1AF3 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 2334514 1 000000008815920b 100 0 0 10 0",
        "\t2: 0100007F:A9EA 0100007F:1AF3 01 00000000:00000000 00:00000000 00000000  1000        0 2334521 1 00000000a37d44c6 21 4 0 10 -1",
        "\t3: 0100007F:E8B4 0100007F:98EF 01 00000000:00000000 00:00000000 00000000  1000        0 2334532 1 0000000031b88f06 21 4 0 10 -1",
        "\t4: 0100007F:866C 0100007F:8783 01 00000000:00000000 00:00000000 00000000  1000        0 2334510 1 00000000cbf670bb 21 4 30 10 -1",
        "\t5: 0100007F:1AF3 0100007F:A9EA 01 00000000:00000000 00:00000000 00000000  1000        0 2338989 1 0000000000bace62 21 4 1 10 -1",
    ])
}

fn tcp6() -> String {
    table(&[
        "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode",
        "\t0: 00000000000000000000000000000000:815B 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 2321070 1 00000000c44f3f02 100 0 0 10 0",
        "\t1: 00000000000000000000000000000000:8783 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 2334509 1 000000003915e812 100 0 0 10 0",
        "\t2: 00000000000000000000000000000000:9907 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 2284465 1 00000000f13b9374 100 0 0 10 0",
        "\t3: 00000000000000000000000000000000:98EF 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 2334531 1 00000000184cae9c 100 0 0 10 0",
        "\t4: 00000000000000000000000000000000:8BCF 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 2329890 1 00000000c05a3466 100 0 0 10 0",
        "\t5: 0000000000000000FFFF00000100007F:8783 0000000000000000FFFF00000100007F:866C 01 00000000:00000000 00:00000000 00000000  1000        0 2334511 1 00000000bf547132 21 4 1 10 -1",
        "\t6: 0000000000000000FFFF00000100007F:98EF 0000000000000000FFFF00000100007F:E8B4 01 00000000:00000000 00:00000000 00000000  1000        0 2334533 1 0000000039d0bcd2 21 4 1 10 -1",
        "\t7: 0000000000000000FFFF0000DFD317AC:9907 0000000000000000FFFF000001D017AC:C123 01 0000005A:00000000 01:00000017 00000000  1000        0 2311039 3 0000000067b6c8db 23 5 25 10 52",
        "\t8: 0000000000000000FFFF0000DFD317AC:9907 0000000000000000FFFF000001D017AC:C124 01 00000000:00000000 00:00000000 00000000  1000        0 2311040 1 00000000230bb017 25 4 30 10 28",
        "\t9: 0000000000000000FFFF0000DFD317AC:9907 0000000000000000FFFF000001D017AC:C213 01 00000000:00000000 00:00000000 00000000  1000        0 2331501 1 00000000957fcb4a 26 4 30 10 57",
        "\t10: 0000000000000000FFFF0000DFD317AC:9907 0000000000000000FFFF000001D017AC:C214 01 00000000:00000000 00:00000000 00000000  1000        0 2331500 1 00000000d7f87ceb 25 4 28 10 -1",
    ])
}

/// O `procSockets` do original: sem o `\n` final, e com 4 tabulações antes de cada linha depois da primeira.
fn proc_sockets() -> String {
    let lines = [
        "ls: cannot access '/proc/8289/fd/255': No such file or directory",
        "ls: cannot access '/proc/8289/fd/3': No such file or directory",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/230/fd/3 -> socket:[21862]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/0 -> socket:[2311043]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/1 -> socket:[2311045]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/19 -> socket:[2311040]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/2 -> socket:[2311047]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/20 -> socket:[2314928]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/22 -> socket:[2307042]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/24 -> socket:[2307051]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/25 -> socket:[2307044]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/27 -> socket:[2307046]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/29 -> socket:[2307053]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/3 -> socket:[2311049]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/30 -> socket:[2307048]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/32 -> socket:[2307055]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/33 -> socket:[2307067]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/34 -> socket:[2307057]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/35 -> socket:[2321483]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/37 -> socket:[2321070]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/41 -> socket:[2321485]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/42 -> socket:[2321074]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/43 -> socket:[2321487]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/44 -> socket:[2329890]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/45 -> socket:[2321489]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2504/fd/46 -> socket:[2334509]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/47 -> socket:[2334510]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/48 -> socket:[2329894]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/49 -> socket:[2334511]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/50 -> socket:[2334515]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/51 -> socket:[2334519]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/52 -> socket:[2334518]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/53 -> socket:[2334521]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/54 -> socket:[2334531]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/55 -> socket:[2334532]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/2504/fd/56 -> socket:[2334533]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2515/fd/3 -> socket:[2311053]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2719/fd/0 -> socket:[2307043]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2719/fd/1 -> socket:[2307045]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2719/fd/2 -> socket:[2307047]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2719/fd/3 -> socket:[2307049]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2725/fd/0 -> socket:[2307052]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2725/fd/1 -> socket:[2307054]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2725/fd/2 -> socket:[2307056]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2725/fd/20 -> socket:[2290617]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2725/fd/3 -> socket:[2307058]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2739/fd/0 -> socket:[2307052]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2739/fd/1 -> socket:[2307054]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2739/fd/2 -> socket:[2307056]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2739/fd/3 -> socket:[2290618]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2795/fd/0 -> socket:[2321484]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2795/fd/1 -> socket:[2321486]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2795/fd/2 -> socket:[2321488]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/2795/fd/3 -> socket:[2321490]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/18 -> socket:[2284465]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/19 -> socket:[2311039]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/23 -> socket:[2331501]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/24 -> socket:[2311052]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/25 -> socket:[2311042]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/26 -> socket:[2331504]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/27 -> socket:[2311051]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/29 -> socket:[2311044]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/314/fd/30 -> socket:[2321909]",
        "lrwx------ 1 alex alex 64 Dec  8 14:59 /proc/314/fd/31 -> socket:[2311046]",
        "lrwx------ 1 alex alex 64 Dec  8 15:14 /proc/314/fd/33 -> socket:[2311048]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/314/fd/35 -> socket:[2329692]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/314/fd/37 -> socket:[2331506]",
        "lrwx------ 1 alex alex 64 Dec  8 15:20 /proc/314/fd/40 -> socket:[2331508]",
        "lrwx------ 1 alex alex 64 Dec  8 15:20 /proc/314/fd/42 -> socket:[2331510]",
        "lrwx------ 1 alex alex 64 Dec  8 15:17 /proc/314/fd/68 -> socket:[2322083]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4412/fd/20 -> socket:[2335214]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/0 -> socket:[2331505]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/1 -> socket:[2331507]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/2 -> socket:[2331509]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/23 -> socket:[2334514]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/24 -> socket:[2338989]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/26 -> socket:[2338276]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/27 -> socket:[2331500]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/3 -> socket:[2331511]",
        "lrwx------ 1 alex alex 64 Dec  8 15:22 /proc/4496/fd/31 -> socket:[2338285]",
    ];
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| if index == 0 { (*line).to_owned() } else { format!("\t\t\t\t{line}") })
        .collect::<Vec<_>>()
        .join("\n")
}

/// O caminho do servidor do VS Code Insiders nos comandos do original.
const SERVER: &str = "/home/alex/.vscode-server-insiders/bin/bc13785d3dd99b4b0e9da9aed17bb79809a50804";
const WINDOWS_CWD: &str = "/mnt/c/Users/alros/AppData/Local/Programs/Microsoft VS Code Insiders";
const SAMPLE_CWD: &str = "/home/alex/repos/Microsoft/vscode-extension-samples/helloworld-sample";

fn process(pid: u32, cwd: &str, cmd: impl Into<String>) -> ProcessInfo {
    ProcessInfo { pid, cwd: cwd.to_owned(), cmd: cmd.into() }
}

fn processes() -> Vec<ProcessInfo> {
    vec![
        process(230, "/mnt/c/WINDOWS/system32", "dockerserve--addressunix:///home/alex/.docker/run/docker-cli-api.sock"),
        process(2504, WINDOWS_CWD, format!("{SERVER}/node{SERVER}/out/bootstrap-fork--type=extensionHost--transformURIs--useHostProxy=")),
        process(2515, WINDOWS_CWD, format!("{SERVER}/node{SERVER}/out/bootstrap-fork--type=watcherService")),
        process(2526, SAMPLE_CWD, "/bin/bash"),
        process(
            2719,
            WINDOWS_CWD,
            format!(
                "{SERVER}/node--max-old-space-size=3072{SERVER}/extensions/node_modules/typescript/lib/tsserver.js--serverModepartialSemantic--useInferredProjectPerProjectRoot--disableAutomaticTypingAcquisition--cancellationPipeName/tmp/vscode-typescript1000/7cfa7171c0c00aacf1ee/tscancellation-602cd80b954818b6a2f7.tmp*--logVerbosityverbose--logFile/home/alex/.vscode-server-insiders/data/logs/20201208T145954/exthost2/vscode.typescript-language-features/tsserver-log-nxBt2m/tsserver.log--globalPluginstypescript-vscode-sh-plugin--pluginProbeLocations{SERVER}/extensions/typescript-language-features--localeen--noGetErrOnBackgroundUpdate--validateDefaultNpmLocation"
            ),
        ),
        process(
            2725,
            WINDOWS_CWD,
            format!(
                "{SERVER}/node--max-old-space-size=3072{SERVER}/extensions/node_modules/typescript/lib/tsserver.js--useInferredProjectPerProjectRoot--enableTelemetry--cancellationPipeName/tmp/vscode-typescript1000/7cfa7171c0c00aacf1ee/tscancellation-04a0b92f880c2fd535ae.tmp*--logVerbosityverbose--logFile/home/alex/.vscode-server-insiders/data/logs/20201208T145954/exthost2/vscode.typescript-language-features/tsserver-log-fqyBrs/tsserver.log--globalPluginstypescript-vscode-sh-plugin--pluginProbeLocations{SERVER}/extensions/typescript-language-features--localeen--noGetErrOnBackgroundUpdate--validateDefaultNpmLocation"
            ),
        ),
        process(
            2739,
            WINDOWS_CWD,
            format!(
                "{SERVER}/node{SERVER}/extensions/node_modules/typescript/lib/typingsInstaller.js--globalTypingsCacheLocation/home/alex/.cache/typescript/4.1--enableTelemetry--logFile/home/alex/.vscode-server-insiders/data/logs/20201208T145954/exthost2/vscode.typescript-language-features/tsserver-log-fqyBrs/ti-2725.log--typesMapLocation{SERVER}/extensions/node_modules/typescript/lib/typesMap.json--validateDefaultNpmLocation"
            ),
        ),
        process(2795, SAMPLE_CWD, format!("{SERVER}/node{SERVER}/extensions/json-language-features/server/dist/node/jsonServerMain--node-ipc--clientProcessId=2504")),
        process(286, WINDOWS_CWD, "sh-c\"$VSCODE_WSL_EXT_LOCATION/ scripts / wslServer.sh\" bc13785d3dd99b4b0e9da9aed17bb79809a50804 insider .vscode-server-insiders 0  "),
        process(
            287,
            WINDOWS_CWD,
            "sh/mnt/c/Users/alros/.vscode-insiders/extensions/ms-vscode-remote.remote-wsl-0.52.0/scripts/wslServer.shbc13785d3dd99b4b0e9da9aed17bb79809a50804insider.vscode-server-insiders0",
        ),
        process(3058, SAMPLE_CWD, "npm"),
        process(3070, SAMPLE_CWD, "sh-ctsc -watch -p ./"),
        process(3071, SAMPLE_CWD, "node/home/alex/repos/Microsoft/vscode-extension-samples/helloworld-sample/node_modules/.bin/tsc-watch-p./"),
        process(312, WINDOWS_CWD, format!("sh{SERVER}/server.sh--port=0--use-host-proxy--enable-remote-auto-shutdown--print-ip-address")),
        process(314, WINDOWS_CWD, format!("{SERVER}/node{SERVER}/out/server-main.js--port=0--use-host-proxy--enable-remote-auto-shutdown--print-ip-address")),
        process(3172, "/home/alex", "/bin/bash"),
        process(3610, SAMPLE_CWD, "/bin/bash"),
        process(4412, SAMPLE_CWD, "http-server"),
        process(4496, WINDOWS_CWD, format!("{SERVER}/node--inspect-brk=0.0.0.0:6899{SERVER}/out/bootstrap-fork--type=extensionHost--transformURIs--useHostProxy=")),
        process(4507, WINDOWS_CWD, format!("{SERVER}/node{SERVER}/extensions/ms-vscode.js-debug/src/hash.bundle.js")),
    ]
}

fn ps_std_out() -> String {
    [
        "4 S root         1     0  0  80   0 -   596 -       1440   2 14:41 ?        00:00:00 /bin/sh -c echo Container started ; trap \"exit 0\" 15; while sleep 1 & wait $!; do :; done",
        "4 S root        14     0  0  80   0 -   596 -        764   4 14:41 ?        00:00:00 /bin/sh",
        "4 S root        40     0  0  80   0 -   596 -        700   4 14:41 ?        00:00:00 /bin/sh",
        "4 S root       513   380  0  80   0 -  2476 -       3404   1 14:41 pts/1    00:00:00 sudo npx http-server -p 5000",
        "4 S root       514   513  0  80   0 - 165439 -     41380   5 14:41 pts/1    00:00:00 http-server",
        "0 S root      1052     1  0  80   0 -   573 -        752   5 14:43 ?        00:00:00 sleep 1",
        "0 S node      1056   329  0  80   0 -   596 do_wai   764  10 14:43 ?        00:00:00 /bin/sh -c ps -F -A -l | grep root",
        "0 S node      1058  1056  0  80   0 -   770 pipe_w   888   9 14:43 ?        00:00:00 grep root",
    ]
    .join("\n")
}

#[test]
fn ext_host_tunnel_service_get_sockets() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let result = get_sockets(&proc_sockets());
    assert_eq!(result.len(), 75);
    // 4412 is the pid of the http-server in the test data
    assert!(result.values().any(|value| value.pid == 4412));
}

#[test]
fn ext_host_tunnel_service_load_connection_table() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let result = load_connection_table(&tcp());
    assert_eq!(result.len(), 6);
    let expected: BTreeMap<String, String> = [
        ("10", "1"),
        ("11", "0000000010173312"),
        ("12", "100"),
        ("13", "0"),
        ("14", "0"),
        ("15", "10"),
        ("16", "0"),
        ("inode", "2335214"),
        ("local_address", "00000000:0BBA"),
        ("rem_address", "00000000:0000"),
        ("retrnsmt", "00000000"),
        ("sl", "0:"),
        ("st", "0A"),
        ("timeout", "0"),
        ("tr", "00:00000000"),
        ("tx_queue", "00000000:00000000"),
        ("uid", "1000"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    assert_eq!(result[0], expected);
}

#[test]
fn ext_host_tunnel_service_load_listening_ports() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let result = load_listening_ports(&[&tcp(), &tcp6()]);
    // There should be 7 based on the input data. One of them should be 3002.
    assert_eq!(result.len(), 7);
    assert!(result.iter().any(|value| value.port == 3002));
}

#[test]
fn ext_host_tunnel_service_try_find_root_ports() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let ps_std_out = ps_std_out();
    let root_processes = get_root_processes(&ps_std_out);
    assert_eq!(root_processes.len(), 6);
    let result = try_find_root_ports(&[ListeningPort { socket: 1000, ip: "127.0.0.1".to_owned(), port: 5000 }], &ps_std_out, &RootPortMap::new());
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(5000).and_then(|port| port.pid), Some(514));
}

#[test]
fn ext_host_tunnel_service_find_ports() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let (tcp, tcp6, proc_sockets, processes) = (tcp(), tcp6(), proc_sockets(), processes());
    run_with_real_timers(async move {
        let result = find_ports(&load_listening_ports(&[&tcp, &tcp6]), &get_sockets(&proc_sockets), &processes).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host, "0.0.0.0");
        assert_eq!(result[0].port, 3002);
        assert_eq!(result[0].detail.as_deref(), Some("http-server"));
    });
}

#[test]
fn ext_host_tunnel_service_parse_ip_address() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ip_address("00000000000000000000000001000000"), "0:0:0:0:0:0:0:1");
    assert_eq!(parse_ip_address("0000000000000000FFFF0000040510AC"), "0:0:0:0:0:ffff:ac10:504");
}
