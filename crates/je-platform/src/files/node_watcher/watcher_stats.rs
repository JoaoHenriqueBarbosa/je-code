//! Porte de `src/vs/platform/files/node/watcher/watcherStats.ts`: o relatório de estatísticas dos pedidos
//! e dos watchers que o `UniversalWatcher` registra quando o log detalhado liga.

use std::cmp::Ordering;
use std::rc::Rc;

use crate::files::watcher::WatchRequest;

use super::base_watcher::{BaseWatcher, Suspended, request_details_to_string};
use super::nodejs::{NodeJsWatcher, NodeJsWatcherInstance};
use super::parcel::{ParcelWatcher, ParcelWatcherInstance};

/// `computeStats(requests, failedRecursiveRequests, recursiveWatcher, nonRecursiveWatcher)`.
pub fn compute_stats(requests: &[WatchRequest], failed_recursive_requests: usize, recursive_watcher: &ParcelWatcher, non_recursive_watcher: &NodeJsWatcher) -> String {
    let mut lines: Vec<String> = Vec::new();

    let all_recursive_requests = sorted_by_path_prefix(requests.iter().filter(|r| r.is_recursive()).cloned().collect(), |r| &r.path);
    let recursive_base = recursive_watcher.base();
    let (non_suspended_recursive, suspended_polling_recursive, suspended_non_polling_recursive) = split_by_suspension(&all_recursive_requests, recursive_base);

    let recursive_requests_status = compute_request_status(&all_recursive_requests, recursive_base);
    let recursive_watchers = recursive_watcher.watchers();
    let recursive_watcher_status = compute_recursive_watch_status(&recursive_watchers);

    let all_non_recursive_requests = sorted_by_path_prefix(requests.iter().filter(|r| !r.is_recursive()).cloned().collect(), |r| &r.path);
    let non_recursive_base = non_recursive_watcher.base();
    let (non_suspended_non_recursive, suspended_polling_non_recursive, suspended_non_polling_non_recursive) = split_by_suspension(&all_non_recursive_requests, non_recursive_base);

    let non_recursive_requests_status = compute_request_status(&all_non_recursive_requests, non_recursive_base);
    let non_recursive_watchers = non_recursive_watcher.watchers();
    let non_recursive_watcher_status = compute_non_recursive_watch_status(&non_recursive_watchers);

    lines.push("[Summary]".to_string());
    lines.push(format!(
        "- Recursive Requests:     total: {}, suspended: {}, polling: {}, failed: {failed_recursive_requests}",
        all_recursive_requests.len(),
        recursive_requests_status.suspended,
        recursive_requests_status.polling
    ));
    lines.push(format!(
        "- Non-Recursive Requests: total: {}, suspended: {}, polling: {}",
        all_non_recursive_requests.len(),
        non_recursive_requests_status.suspended,
        non_recursive_requests_status.polling
    ));
    lines.push(format!(
        "- Recursive Watchers:     total: {}, active: {}, failed: {}, stopped: {}",
        recursive_watchers.len(),
        recursive_watcher_status.active,
        recursive_watcher_status.failed,
        recursive_watcher_status.stopped
    ));
    lines.push(format!(
        "- Non-Recursive Watchers: total: {}, active: {}, failed: {}, reusing: {}",
        non_recursive_watchers.len(),
        non_recursive_watcher_status.active,
        non_recursive_watcher_status.failed,
        non_recursive_watcher_status.reusing
    ));
    lines.push(format!(
        "- I/O Handles Impact:     total: {}",
        recursive_requests_status.polling + non_recursive_requests_status.polling + recursive_watcher_status.active + non_recursive_watcher_status.active
    ));

    lines.push(format!(
        "\n[Recursive Requests ({}, suspended: {}, polling: {})]:",
        all_recursive_requests.len(),
        recursive_requests_status.suspended,
        recursive_requests_status.polling
    ));
    let mut recursive_request_lines = Vec::new();
    for request in non_suspended_recursive.iter().chain(&suspended_polling_recursive).chain(&suspended_non_polling_recursive) {
        fill_request_stats(&mut recursive_request_lines, request, recursive_base);
    }
    lines.extend(align_text_columns(recursive_request_lines));

    let mut recursive_watcher_lines = Vec::new();
    fill_recursive_watcher_stats(&mut recursive_watcher_lines, &recursive_watchers);
    lines.extend(align_text_columns(recursive_watcher_lines));

    lines.push(format!(
        "\n[Non-Recursive Requests ({}, suspended: {}, polling: {})]:",
        all_non_recursive_requests.len(),
        non_recursive_requests_status.suspended,
        non_recursive_requests_status.polling
    ));
    let mut non_recursive_request_lines = Vec::new();
    for request in non_suspended_non_recursive.iter().chain(&suspended_polling_non_recursive).chain(&suspended_non_polling_non_recursive) {
        fill_request_stats(&mut non_recursive_request_lines, request, non_recursive_base);
    }
    lines.extend(align_text_columns(non_recursive_request_lines));

    let mut non_recursive_watcher_lines = Vec::new();
    fill_non_recursive_watcher_stats(&mut non_recursive_watcher_lines, &non_recursive_watchers);
    lines.extend(align_text_columns(non_recursive_watcher_lines));

    format!("\n\n[File Watcher] request stats:\n\n{}\n\n", lines.join("\n"))
}

fn split_by_suspension(requests: &[WatchRequest], base: &BaseWatcher) -> (Vec<WatchRequest>, Vec<WatchRequest>, Vec<WatchRequest>) {
    let of = |state: Suspended| requests.iter().filter(|r| base.is_suspended(r) == state).cloned().collect();
    (of(Suspended::No), of(Suspended::Polling), of(Suspended::Yes))
}

fn align_text_columns(mut lines: Vec<String>) -> Vec<String> {
    let max_length = lines.iter().map(|line| line.split('\t').next().unwrap_or("").encode_utf16().count()).max().unwrap_or(0);

    for line in &mut lines {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() == 2 {
            let padding = " ".repeat(max_length - parts[0].encode_utf16().count());
            *line = format!("{}{padding}\t{}", parts[0], parts[1]);
        }
    }

    lines
}

struct RequestStatus {
    suspended: usize,
    polling: usize,
}

fn compute_request_status(requests: &[WatchRequest], base: &BaseWatcher) -> RequestStatus {
    let mut polling = 0;
    let mut suspended = 0;

    for request in requests {
        let is_suspended = base.is_suspended(request);
        if is_suspended == Suspended::No {
            continue;
        }

        suspended += 1;

        if is_suspended == Suspended::Polling {
            polling += 1;
        }
    }

    RequestStatus { suspended, polling }
}

struct RecursiveWatchStatus {
    active: usize,
    failed: usize,
    stopped: usize,
}

fn compute_recursive_watch_status(watchers: &[Rc<ParcelWatcherInstance>]) -> RecursiveWatchStatus {
    let mut status = RecursiveWatchStatus { active: 0, failed: 0, stopped: 0 };

    for watcher in watchers {
        if !watcher.failed() && !watcher.stopped() {
            status.active += 1;
        }
        if watcher.failed() {
            status.failed += 1;
        }
        if watcher.stopped() {
            status.stopped += 1;
        }
    }

    status
}

struct NonRecursiveWatchStatus {
    active: usize,
    failed: usize,
    reusing: usize,
}

fn compute_non_recursive_watch_status(watchers: &[Rc<NodeJsWatcherInstance>]) -> NonRecursiveWatchStatus {
    let mut status = NonRecursiveWatchStatus { active: 0, failed: 0, reusing: 0 };

    for watcher in watchers {
        if !watcher.instance.failed() && !watcher.instance.is_reusing_recursive_watcher() {
            status.active += 1;
        }
        if watcher.instance.failed() {
            status.failed += 1;
        }
        if watcher.instance.is_reusing_recursive_watcher() {
            status.reusing += 1;
        }
    }

    status
}

/// `sortByPathPrefix`: compara caminho a caminho, unidade UTF-16 a unidade, e por último o tamanho.
fn sorted_by_path_prefix<T>(mut items: Vec<T>, path: impl Fn(&T) -> &str) -> Vec<T> {
    items.sort_by(|a, b| {
        let (p1, p2): (Vec<u16>, Vec<u16>) = (path(a).encode_utf16().collect(), path(b).encode_utf16().collect());
        let min_length = p1.len().min(p2.len());
        for i in 0..min_length {
            if p1[i] != p2[i] {
                return if p1[i] < p2[i] { Ordering::Less } else { Ordering::Greater };
            }
        }
        p1.len().cmp(&p2.len())
    });
    items
}

fn fill_request_stats(lines: &mut Vec<String>, request: &WatchRequest, base: &BaseWatcher) {
    let mut decorations = Vec::new();
    match base.is_suspended(request) {
        Suspended::No => {}
        Suspended::Polling => decorations.push("[SUSPENDED <polling>]"),
        Suspended::Yes => decorations.push("[SUSPENDED <non-polling>]"),
    }

    lines.push(format!(" {}\t{}({})", request.path, decorate(&decorations), request_details_to_string(request)));
}

fn decorate(decorations: &[&str]) -> String {
    if decorations.is_empty() { String::new() } else { format!("{} ", decorations.join(" ")) }
}

fn fill_recursive_watcher_stats(lines: &mut Vec<String>, watchers: &[Rc<ParcelWatcherInstance>]) {
    let watchers = sorted_by_path_prefix(watchers.to_vec(), |w| &w.request.path);

    let status = compute_recursive_watch_status(&watchers);
    lines.push(format!("\n[Recursive Watchers ({}, active: {}, failed: {}, stopped: {})]:", watchers.len(), status.active, status.failed, status.stopped));

    for watcher in &watchers {
        let mut decorations = Vec::new();
        if watcher.failed() {
            decorations.push("[FAILED]".to_string());
        }
        if watcher.stopped() {
            decorations.push("[STOPPED]".to_string());
        }
        if watcher.subscriptions_count() > 0 {
            decorations.push(format!("[SUBSCRIBED:{}]", watcher.subscriptions_count()));
        }
        if watcher.restarts > 0 {
            decorations.push(format!("[RESTARTED:{}]", watcher.restarts));
        }
        let decorations: Vec<&str> = decorations.iter().map(String::as_str).collect();
        lines.push(format!(" {}\t{}({})", watcher.request.path, decorate(&decorations), request_details_to_string(&watcher.request)));
    }
}

fn fill_non_recursive_watcher_stats(lines: &mut Vec<String>, watchers: &[Rc<NodeJsWatcherInstance>]) {
    let all_watchers = sorted_by_path_prefix(watchers.to_vec(), |w| &w.request.path);
    let active: Vec<_> = all_watchers.iter().filter(|w| !w.instance.failed() && !w.instance.is_reusing_recursive_watcher()).cloned().collect();
    let failed: Vec<_> = all_watchers.iter().filter(|w| w.instance.failed()).cloned().collect();
    let reusing: Vec<_> = all_watchers.iter().filter(|w| w.instance.is_reusing_recursive_watcher()).cloned().collect();

    let status = compute_non_recursive_watch_status(&all_watchers);
    lines.push(format!("\n[Non-Recursive Watchers ({}, active: {}, failed: {}, reusing: {})]:", all_watchers.len(), status.active, status.failed, status.reusing));

    for watcher in active.iter().chain(&failed).chain(&reusing) {
        let mut decorations = Vec::new();
        if watcher.instance.failed() {
            decorations.push("[FAILED]");
        }
        if watcher.instance.is_reusing_recursive_watcher() {
            decorations.push("[REUSING]");
        }
        lines.push(format!(" {}\t{}({})", watcher.request.path, decorate(&decorations), request_details_to_string(&watcher.request)));
    }
}
