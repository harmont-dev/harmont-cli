//! All host functions exported to plugins. The exhaustive list lives
//! in the design spec §3.3; this file is the single source of truth
//! for which fn names exist and what types they accept.

// `extism::host_fn!` expands to plain `pub fn` items whose bodies do
// `plugin.memory_get_val(&inputs[i])` for each arg. The macro produces
// expressions clippy wants to grumble about (needless pass-by-value of
// `Json<T>` newtypes; non-erroring `Ok(())` wrappers); we accept the
// macro idiom rather than fight it at every call site. Scope is this file.
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::missing_errors_doc)]
// extism wraps every host-fn arg/ret through `MemoryHandle`, which is a
// 64-bit pointer; cast-precision warnings are not actionable here.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
// `all()` is intentionally a single Vec literal — splitting it would obscure
// the 1:1 mapping between HOST_FN_NAMES and the constructed Function set.
#![allow(clippy::too_many_lines)]
// The tiny `pty` helper sits adjacent to its only call site inside `all()`;
// hoisting it to module scope would force readers to jump out of the table.
#![allow(clippy::items_after_statements)]
// Several `*_impl` fns are no-op stubs that could be `const fn` today
// but will gain side-effecting bodies in Plan 2; flipping them now would
// mean another churn pass.
#![allow(clippy::missing_const_for_fn)]
// `GLOBAL.lock().map(|s| s.cancel).unwrap_or(false)` reads as
// "treat host-fn failure as 'not cancelled'"; collapsing to `is_ok_and`
// would obscure the fallback intent.
#![allow(clippy::map_unwrap_or)]
// `Lazy::new(|| …)` is the once_cell idiom we use across the workspace;
// the `LazyLock` migration is a separate sweep.
#![allow(clippy::incompatible_msrv)]
#![allow(clippy::non_std_lazy_statics)]
// `GLOBAL.lock()` returns a guard with significant `Drop`; clippy flags
// holding it across the `.get(key).cloned()` call. The lock IS the
// scrutinee on purpose — we want a coherent read.
#![allow(clippy::significant_drop_in_scrutinee)]
#![allow(clippy::significant_drop_tightening)]

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use extism::convert::Json;
use extism::{Function, PTR, UserData, ValType, host_fn};
use hm_plugin_protocol::host_abi::{
    ArchiveReadArgs, CallbackData, KeyringArgs, KeyringSetArgs, KvScope, Level, LoopbackHandle,
    LoopbackRecvArgs, SocketHandle, SocketReadArgs, SocketWriteArgs, TtyConfirmArgs, TtyPromptArgs,
};
use hm_plugin_protocol::{
    ArchiveId, BuildEvent, DockerCommitArgs, DockerExecArgs, DockerExtractArgs, DockerStartArgs,
    StdStream,
};
use once_cell::sync::Lazy;

/// The canonical list of host fns we expose. Plugin manifests are
/// validated against this set at load time.
pub const HOST_FN_NAMES: &[&str] = &[
    "hm_log",
    "hm_emit_step_log",
    "hm_emit_event",
    "hm_kv_get",
    "hm_kv_set",
    "hm_archive_read",
    "hm_archive_total_size",
    "hm_fs_read_config",
    "hm_unix_socket_connect",
    "hm_socket_write",
    "hm_socket_read",
    "hm_socket_close",
    "hm_keyring_get",
    "hm_keyring_set",
    "hm_keyring_delete",
    "hm_tty_prompt",
    "hm_tty_confirm",
    "hm_browser_open",
    "hm_spawn_loopback",
    "hm_loopback_recv",
    "hm_should_cancel",
    "hm_docker_ping",
    "hm_docker_image_exists",
    "hm_docker_pull",
    "hm_docker_start_container",
    "hm_docker_extract_workspace",
    "hm_docker_exec",
    "hm_docker_commit",
    "hm_docker_remove_image",
    "hm_docker_stop_remove",
    "hm_write_stdout",
    "hm_write_stderr",
];

// ─── host_fn! declarations ──────────────────────────────────────────────────
//
// Each `host_fn!` invocation expands to a plain `pub fn name(...)` matching
// extism's host-fn signature. We wire each into a `Function` value below.

host_fn!(pub _hm_log(_user_data: (); level: Json<Level>, msg: String) {
    let Json(level) = level;
    log_impl(level, &msg);
    Ok(())
});

host_fn!(pub _hm_emit_step_log(_user_data: (); stream: Json<StdStream>, bytes: Vec<u8>) {
    let Json(stream) = stream;
    emit_step_log_impl(stream, &bytes);
    Ok(())
});

host_fn!(pub _hm_emit_event(_user_data: (); event: Json<BuildEvent>) {
    let Json(event) = event;
    emit_event_impl(event);
    Ok(())
});

host_fn!(pub _hm_kv_get(_user_data: (); scope: Json<KvScope>, key: String) -> Json<Option<Vec<u8>>> {
    let Json(scope) = scope;
    Ok(Json(kv_get_impl(scope, &key)))
});

host_fn!(pub _hm_kv_set(_user_data: (); scope: Json<KvScope>, key: String, val: Vec<u8>) {
    let Json(scope) = scope;
    kv_set_impl(scope, &key, val);
    Ok(())
});

host_fn!(pub _hm_archive_read(_user_data: (); args: Json<ArchiveReadArgs>) -> Vec<u8> {
    let Json(args) = args;
    Ok(archive_read_impl(args))
});

host_fn!(pub _hm_archive_total_size(_user_data: (); id: Json<ArchiveId>) -> u64 {
    let Json(id) = id;
    Ok(archive_total_size_impl(id))
});

host_fn!(pub _hm_fs_read_config(_user_data: (); rel_path: String) -> Json<Option<Vec<u8>>> {
    Ok(Json(fs_read_config_impl(&rel_path)))
});

host_fn!(pub _hm_unix_socket_connect(_user_data: (); path: String) -> Json<SocketHandle> {
    Ok(Json(unix_socket_connect_impl(&path)))
});

host_fn!(pub _hm_socket_write(_user_data: (); args: Json<SocketWriteArgs>) -> u64 {
    let Json(args) = args;
    Ok(socket_write_impl(args))
});

host_fn!(pub _hm_socket_read(_user_data: (); args: Json<SocketReadArgs>) -> Vec<u8> {
    let Json(args) = args;
    Ok(socket_read_impl(args))
});

host_fn!(pub _hm_socket_close(_user_data: (); h: Json<SocketHandle>) {
    let Json(h) = h;
    socket_close_impl(h);
    Ok(())
});

host_fn!(pub _hm_keyring_get(_user_data: (); args: Json<KeyringArgs>) -> Json<Option<String>> {
    let Json(args) = args;
    Ok(Json(keyring_get_impl(&args.service, &args.account)))
});

host_fn!(pub _hm_keyring_set(_user_data: (); args: Json<KeyringSetArgs>) {
    let Json(args) = args;
    keyring_set_impl(&args.service, &args.account, &args.secret);
    Ok(())
});

host_fn!(pub _hm_keyring_delete(_user_data: (); args: Json<KeyringArgs>) {
    let Json(args) = args;
    keyring_delete_impl(&args.service, &args.account);
    Ok(())
});

host_fn!(pub _hm_tty_prompt(_user_data: (); args: Json<TtyPromptArgs>) -> String {
    let Json(args) = args;
    Ok(tty_prompt_impl(&args.msg, args.mask))
});

host_fn!(pub _hm_tty_confirm(_user_data: (); args: Json<TtyConfirmArgs>) -> u32 {
    let Json(args) = args;
    Ok(u32::from(tty_confirm_impl(&args.msg, args.default)))
});

host_fn!(pub _hm_browser_open(_user_data: (); url: String) -> u32 {
    Ok(u32::from(browser_open_impl(&url)))
});

host_fn!(pub _hm_spawn_loopback(_user_data: (); port: Json<Option<u16>>) -> Json<LoopbackHandle> {
    let Json(port) = port;
    let handle = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(spawn_loopback_impl_async(port))
    })?;
    Ok(Json(handle))
});

host_fn!(pub _hm_loopback_recv(_user_data: (); args: Json<LoopbackRecvArgs>) -> Json<Option<CallbackData>> {
    let Json(args) = args;
    let data = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(loopback_recv_impl_async(args))
    });
    Ok(Json(data))
});

host_fn!(pub _hm_should_cancel(_user_data: ();) -> u32 {
    Ok(u32::from(should_cancel_impl()))
});

host_fn!(pub _hm_docker_ping(_user_data: ();) -> u32 {
    let ok = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::ping_impl())
    });
    Ok(u32::from(ok))
});

host_fn!(pub _hm_docker_image_exists(_user_data: (); tag: String) -> u32 {
    let exists = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::image_exists_impl(tag))
    });
    Ok(u32::from(exists))
});

host_fn!(pub _hm_docker_pull(_user_data: (); tag: String) {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::pull_impl(tag))
    })?;
    Ok(())
});

host_fn!(pub _hm_docker_start_container(_user_data: (); args: Json<DockerStartArgs>) -> String {
    let Json(args) = args;
    let id = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::start_container_impl(args))
    })?;
    Ok(id)
});

host_fn!(pub _hm_docker_extract_workspace(_user_data: (); args: Json<DockerExtractArgs>) {
    let Json(args) = args;
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::extract_workspace_impl(args))
    })?;
    Ok(())
});

host_fn!(pub _hm_docker_exec(_user_data: (); args: Json<DockerExecArgs>) -> i32 {
    let Json(args) = args;
    let rc = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::exec_impl(args))
    })?;
    Ok(rc)
});

host_fn!(pub _hm_docker_commit(_user_data: (); args: Json<DockerCommitArgs>) -> String {
    let Json(args) = args;
    let tag = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::commit_impl(args))
    })?;
    Ok(tag)
});

host_fn!(pub _hm_docker_remove_image(_user_data: (); tag: String) {
    let _ = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::remove_image_impl(tag))
    });
    Ok(())
});

host_fn!(pub _hm_docker_stop_remove(_user_data: (); container_id: String) {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::orchestrator::docker_host_fns::stop_remove_impl(container_id));
    });
    Ok(())
});

host_fn!(pub _hm_write_stdout(_user_data: (); bytes: Vec<u8>) {
    write_stdout_impl(&bytes);
    Ok(())
});

host_fn!(pub _hm_write_stderr(_user_data: (); bytes: Vec<u8>) {
    write_stderr_impl(&bytes);
    Ok(())
});

/// Returns the host function table passed into every `Plugin::new`.
///
/// extism wraps every host-fn argument and return value as a 64-bit
/// memory handle (`PTR == ValType::I64`), regardless of the underlying
/// Rust type. So every `params: …` and `returns: …` list below is just
/// `[PTR; N]` where `N` is the arg/return arity.
pub fn all() -> Vec<Function> {
    let ud: UserData<()> = UserData::default();
    fn pty(n: usize) -> Vec<ValType> {
        (0..n).map(|_| PTR).collect()
    }
    vec![
        Function::new("hm_log", pty(2), pty(0), ud.clone(), _hm_log),
        Function::new(
            "hm_emit_step_log",
            pty(2),
            pty(0),
            ud.clone(),
            _hm_emit_step_log,
        ),
        Function::new("hm_emit_event", pty(1), pty(0), ud.clone(), _hm_emit_event),
        Function::new("hm_kv_get", pty(2), pty(1), ud.clone(), _hm_kv_get),
        Function::new("hm_kv_set", pty(3), pty(0), ud.clone(), _hm_kv_set),
        Function::new(
            "hm_archive_read",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_archive_read,
        ),
        Function::new(
            "hm_archive_total_size",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_archive_total_size,
        ),
        Function::new(
            "hm_fs_read_config",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_fs_read_config,
        ),
        Function::new(
            "hm_unix_socket_connect",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_unix_socket_connect,
        ),
        Function::new(
            "hm_socket_write",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_socket_write,
        ),
        Function::new(
            "hm_socket_read",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_socket_read,
        ),
        Function::new(
            "hm_socket_close",
            pty(1),
            pty(0),
            ud.clone(),
            _hm_socket_close,
        ),
        Function::new(
            "hm_keyring_get",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_keyring_get,
        ),
        Function::new(
            "hm_keyring_set",
            pty(1),
            pty(0),
            ud.clone(),
            _hm_keyring_set,
        ),
        Function::new(
            "hm_keyring_delete",
            pty(1),
            pty(0),
            ud.clone(),
            _hm_keyring_delete,
        ),
        Function::new("hm_tty_prompt", pty(1), pty(1), ud.clone(), _hm_tty_prompt),
        Function::new(
            "hm_tty_confirm",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_tty_confirm,
        ),
        Function::new(
            "hm_browser_open",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_browser_open,
        ),
        Function::new(
            "hm_spawn_loopback",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_spawn_loopback,
        ),
        Function::new(
            "hm_loopback_recv",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_loopback_recv,
        ),
        Function::new(
            "hm_should_cancel",
            pty(0),
            pty(1),
            ud.clone(),
            _hm_should_cancel,
        ),
        Function::new(
            "hm_docker_ping",
            pty(0),
            pty(1),
            ud.clone(),
            _hm_docker_ping,
        ),
        Function::new(
            "hm_docker_image_exists",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_docker_image_exists,
        ),
        Function::new(
            "hm_docker_pull",
            pty(1),
            pty(0),
            ud.clone(),
            _hm_docker_pull,
        ),
        Function::new(
            "hm_docker_start_container",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_docker_start_container,
        ),
        Function::new(
            "hm_docker_extract_workspace",
            pty(1),
            pty(0),
            ud.clone(),
            _hm_docker_extract_workspace,
        ),
        Function::new(
            "hm_docker_exec",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_docker_exec,
        ),
        Function::new(
            "hm_docker_commit",
            pty(1),
            pty(1),
            ud.clone(),
            _hm_docker_commit,
        ),
        Function::new(
            "hm_docker_remove_image",
            pty(1),
            pty(0),
            ud.clone(),
            _hm_docker_remove_image,
        ),
        Function::new(
            "hm_docker_stop_remove",
            pty(1),
            pty(0),
            ud,
            _hm_docker_stop_remove,
        ),
        Function::new(
            "hm_write_stdout",
            [ValType::I64],
            [],
            UserData::default(),
            _hm_write_stdout,
        ),
        Function::new(
            "hm_write_stderr",
            [ValType::I64],
            [],
            UserData::default(),
            _hm_write_stderr,
        ),
    ]
}

// ─── Implementations (minimal, correct, lockable). ──────────────────────────
// "Minimal" here means: the simple host-side behaviour that fixture
// tests in Task 28 will exercise. Heavier behaviours (real cancellation
// propagation, archive byte-streaming under load) get hardened in
// later plans when real plugins drive them.

static GLOBAL: Lazy<Mutex<HostState>> = Lazy::new(|| Mutex::new(HostState::default()));

#[derive(Debug, Default)]
struct HostState {
    build_kv: BTreeMap<String, Vec<u8>>,
    step_kv: BTreeMap<String, Vec<u8>>,
    // `SocketHandle` only implements `Hash + Eq`, not `Ord`, so a
    // `HashMap` is the right shape here.
    sockets: HashMap<SocketHandle, Vec<u8>>,
    next_socket: u64,
    /// Live loopback listeners. Keyed by the bound port (also the
    /// returned `LoopbackHandle.0`). The `Arc<LoopbackSlot>` is shared
    /// between the axum task and `loopback_recv_impl_async`.
    /// `LoopbackHandle` implements `Hash + Eq` but not `Ord`, so this
    /// is a `HashMap` rather than `BTreeMap` (same shape as `sockets`).
    loopback_slots: HashMap<LoopbackHandle, Arc<LoopbackSlot>>,
}

/// Per-handle state for an in-flight loopback listener.
///
/// `receiver` is `Some(_)` until the first `hm_loopback_recv` consumes
/// it; subsequent calls with the same handle return `None`. `shutdown_token`
/// is cancelled by the axum route closure after the first callback is
/// captured, which causes `axum::serve(..).with_graceful_shutdown(..)` to
/// return and the listener to close.
#[derive(Debug)]
struct LoopbackSlot {
    receiver: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<CallbackData>>>,
    #[allow(
        dead_code,
        reason = "held to keep the token alive; cancellation is driven by the route closure's clone"
    )]
    shutdown_token: tokio_util::sync::CancellationToken,
}

fn log_impl(level: Level, msg: &str) {
    match level {
        Level::Trace => tracing::trace!(target: "plugin", "{msg}"),
        Level::Debug => tracing::debug!(target: "plugin", "{msg}"),
        Level::Info => tracing::info!(target: "plugin", "{msg}"),
        Level::Warn => tracing::warn!(target: "plugin", "{msg}"),
        Level::Error => tracing::error!(target: "plugin", "{msg}"),
    }
}

fn emit_step_log_impl(stream: StdStream, bytes: &[u8]) {
    let Some(state) = crate::orchestrator::state::current() else {
        return;
    };
    let Some(step_id) = current_step_id() else {
        return;
    };
    let line = String::from_utf8_lossy(bytes).into_owned();
    state.event_bus.emit(BuildEvent::StepLog {
        step_id,
        stream,
        line,
        ts: chrono::Utc::now(),
    });
}

fn emit_event_impl(event: BuildEvent) {
    if let Some(state) = crate::orchestrator::state::current() {
        state.event_bus.emit(event);
    }
}

fn kv_get_impl(scope: KvScope, key: &str) -> Option<Vec<u8>> {
    match scope {
        KvScope::Plugin => load_plugin_kv().get(key).cloned(),
        KvScope::Build | KvScope::Step => {
            let s = GLOBAL.lock().ok()?;
            let m = match scope {
                KvScope::Build => &s.build_kv,
                KvScope::Step => &s.step_kv,
                KvScope::Plugin => unreachable!(),
            };
            m.get(key).cloned()
        }
    }
}

#[doc(hidden)] // pub for integration tests; not stable API
pub fn kv_set_impl(scope: KvScope, key: &str, val: Vec<u8>) {
    match scope {
        KvScope::Plugin => {
            // Hold an exclusive advisory lock for the full read-modify-write
            // window. Without this, concurrent writers each load the same map,
            // insert into their own copy, and the second writer's atomic save
            // clobbers the first writer's insert. See plugin_kv_concurrency.rs.
            //
            // If lock acquisition fails (no config dir, no current plugin
            // name, fs error), we fall back to the prior unprotected write —
            // better than dropping the value entirely. This matches the
            // existing best-effort framing of save_plugin_kv.
            let lock = lock_plugin_kv();
            if lock.is_none() {
                tracing::warn!(
                    target: "plugin::kv",
                    "plugin-scope KV lock acquisition failed; \
                     falling back to unprotected write. Concurrent \
                     writers may lose updates."
                );
            }
            let mut kv = load_plugin_kv();
            kv.insert(key.to_string(), val);
            save_plugin_kv(&kv);
            // `lock` drops here, releasing the file lock.
        }
        KvScope::Build | KvScope::Step => {
            let Ok(mut s) = GLOBAL.lock() else { return };
            let m = match scope {
                KvScope::Build => &mut s.build_kv,
                KvScope::Step => &mut s.step_kv,
                KvScope::Plugin => unreachable!(),
            };
            m.insert(key.to_string(), val);
        }
    }
}

// ─── Disk-backed Plugin-scope KV ────────────────────────────────────────────
//
// `KvScope::Plugin` persists across hm invocations so plugins (e.g. the
// cloud plugin) can stash the active org slug, last-seen tokens, etc.
// Path: `<config_dir>/harmont/state/<plugin-name>.kv`. Per-plugin
// isolation is enforced by the `CURRENT_PLUGIN_NAME` thread-local,
// which `LoadedPlugin::call_capability` sets around every call.
//
// Concurrency: write operations (`KvScope::Plugin`) take an exclusive
// advisory lock on a per-plugin `<plugin-name>.lock` sibling file via
// `fs2::FileExt::lock_exclusive`. Readers do NOT lock —
// `load_plugin_kv` is read-only and works against the atomically
// written `.kv` file (tmp + rename in `save_plugin_kv`), so a reader
// either sees the pre-write or post-write state, never a torn map.
// Concurrent invocations of `hm` against the same plugin's KV
// serialise on the `.lock` file; the held window is small (load +
// insert + atomic write of a typically-small JSON map) so contention
// is not a practical concern.

fn plugin_state_path() -> Option<std::path::PathBuf> {
    let dir = hm_util::dirs::harmont_plugin_state_dir()?;
    let plugin = current_plugin_name()?;
    Some(dir.join(format!("{plugin}.kv")))
}

/// Acquire an exclusive advisory lock on `<config_dir>/harmont/state/<plugin>.lock`.
///
/// Returns `None` if `plugin_state_path()` couldn't resolve (no config
/// dir or no current plugin name). The returned `File` releases the
/// lock on drop — so the caller holds the lock for the lifetime of
/// the binding.
fn lock_plugin_kv() -> Option<std::fs::File> {
    use fs2::FileExt;
    let kv_path = plugin_state_path()?;
    let lock_path = kv_path.with_extension("lock");
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .ok()?;
    f.lock_exclusive().ok()?;
    Some(f)
}

fn current_plugin_name() -> Option<String> {
    CURRENT_PLUGIN_NAME.with(|c| c.borrow().clone())
}

thread_local! {
    pub(crate) static CURRENT_PLUGIN_NAME: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

#[doc(hidden)] // pub for integration tests; not stable API
pub fn set_current_plugin_name(name: String) {
    CURRENT_PLUGIN_NAME.with(|c| *c.borrow_mut() = Some(name));
}

pub(crate) fn clear_current_plugin_name() {
    CURRENT_PLUGIN_NAME.with(|c| *c.borrow_mut() = None);
}

#[doc(hidden)] // pub for integration tests; not stable API
#[must_use]
pub fn load_plugin_kv() -> BTreeMap<String, Vec<u8>> {
    let Some(path) = plugin_state_path() else {
        return BTreeMap::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return BTreeMap::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_plugin_kv(kv: &BTreeMap<String, Vec<u8>>) {
    let Some(path) = plugin_state_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);
    if let Ok(bytes) = serde_json::to_vec(kv) {
        // Atomic write: tmpfile + rename. If rename fails the old file
        // persists; best-effort.
        let tmp = path.with_extension("kv.tmp");
        if std::fs::write(&tmp, &bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn archive_read_impl(args: ArchiveReadArgs) -> Vec<u8> {
    crate::orchestrator::state::current()
        .map(|s| s.archives.read(args.id, args.offset, args.max))
        .unwrap_or_default()
}

fn archive_total_size_impl(id: ArchiveId) -> u64 {
    crate::orchestrator::state::current()
        .map(|s| s.archives.total_size(id))
        .unwrap_or(0)
}

fn fs_read_config_impl(rel_path: &str) -> Option<Vec<u8>> {
    let root_unresolved = std::env::current_dir().ok()?.join(".harmont");
    let root = root_unresolved.canonicalize().ok()?;
    let candidate = root.join(rel_path);
    let canonical = candidate.canonicalize().ok()?;
    if !canonical.starts_with(&root) {
        return None;
    }
    std::fs::read(canonical).ok()
}

fn unix_socket_connect_impl(_path: &str) -> SocketHandle {
    let Ok(mut s) = GLOBAL.lock() else {
        return SocketHandle(0);
    };
    s.next_socket += 1;
    let h = SocketHandle(s.next_socket);
    s.sockets.insert(h, Vec::new());
    h
}

fn socket_write_impl(args: SocketWriteArgs) -> u64 {
    let Ok(mut s) = GLOBAL.lock() else { return 0 };
    if let Some(buf) = s.sockets.get_mut(&args.h) {
        buf.extend_from_slice(&args.bytes);
        args.bytes.len() as u64
    } else {
        0
    }
}

fn socket_read_impl(_args: SocketReadArgs) -> Vec<u8> {
    // Plan 1: in-memory loopback for tests. Plan 2 swaps in a real
    // tokio UnixStream.
    Vec::new()
}

fn socket_close_impl(h: SocketHandle) {
    let Ok(mut s) = GLOBAL.lock() else { return };
    s.sockets.remove(&h);
}

fn keyring_get_impl(service: &str, account: &str) -> Option<String> {
    crate::creds_store::get(service, account)
}

fn keyring_set_impl(service: &str, account: &str, secret: &str) {
    crate::creds_store::set(service, account, secret);
}

fn keyring_delete_impl(service: &str, account: &str) {
    crate::creds_store::delete(service, account);
}

fn tty_prompt_impl(msg: &str, mask: bool) -> String {
    use dialoguer::{Input, Password};
    if mask {
        Password::new()
            .with_prompt(msg)
            .interact()
            .unwrap_or_default()
    } else {
        Input::<String>::new()
            .with_prompt(msg)
            .interact_text()
            .unwrap_or_default()
    }
}

fn tty_confirm_impl(msg: &str, default: bool) -> bool {
    use dialoguer::Confirm;
    Confirm::new()
        .with_prompt(msg)
        .default(default)
        .interact()
        .unwrap_or(default)
}

fn browser_open_impl(url: &str) -> bool {
    webbrowser::open(url).is_ok()
}

/// Bind a real axum oneshot on `127.0.0.1:<port>` (or any free port if
/// `port` is `None`). The first request to ANY path captures the URI's
/// `(path, query)` into a oneshot, then cancels the shutdown token so
/// the listener exits. Returns the bound port as a `LoopbackHandle`.
///
/// The plugin uses `handle.0` both as the recv handle and as the port
/// number to embed in its OAuth redirect URI (`http://127.0.0.1:<port>/cb`).
async fn spawn_loopback_impl_async(port: Option<u16>) -> anyhow::Result<LoopbackHandle> {
    use anyhow::Context;
    use axum::Router;
    use axum::routing::get;
    use std::net::SocketAddr;

    let addr = SocketAddr::from(([127, 0, 0, 1], port.unwrap_or(0)));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind loopback on {addr}"))?;
    let bound_port = listener.local_addr()?.port();

    let (tx, rx) = tokio::sync::oneshot::channel::<CallbackData>();
    // The sender is moved into the route closure, which uses `.take()`
    // to ensure only the FIRST callback fires the channel. Wrapping in
    // `Arc<Mutex<Option<_>>>` makes the closure `Clone` (axum needs the
    // closure to be `FnOnce + Clone` for fallback handlers).
    let tx_for_route: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<CallbackData>>>> =
        Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let shutdown = tokio_util::sync::CancellationToken::new();

    let shutdown_for_route = shutdown.clone();
    let app = Router::new().fallback(get(move |uri: axum::http::Uri| {
        let tx = tx_for_route.clone();
        let shutdown = shutdown_for_route.clone();
        async move {
            let path = uri.path().to_string();
            let mut query: BTreeMap<String, String> = BTreeMap::new();
            if let Some(q) = uri.query() {
                for (k, v) in url::form_urlencoded::parse(q.as_bytes()) {
                    query.insert(k.into_owned(), v.into_owned());
                }
            }
            let data = CallbackData { path, query };
            if let Some(t) = tx.lock().await.take() {
                let _ = t.send(data);
            }
            shutdown.cancel();
            axum::response::Html(
                "<!DOCTYPE html><html><body><h1>You can close this tab.</h1></body></html>",
            )
        }
    }));

    let shutdown_for_server = shutdown.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_for_server.cancelled_owned())
            .await;
    });

    let handle = LoopbackHandle(u64::from(bound_port));
    let slot = Arc::new(LoopbackSlot {
        receiver: tokio::sync::Mutex::new(Some(rx)),
        shutdown_token: shutdown,
    });
    {
        let mut g = GLOBAL
            .lock()
            .map_err(|_| anyhow::anyhow!("global host state lock poisoned"))?;
        g.loopback_slots.insert(handle, slot);
    }
    Ok(handle)
}

/// Await the matching slot's oneshot receiver for up to `timeout_ms`
/// milliseconds. Returns `None` on timeout, on unknown handle, or if the
/// receiver has already been consumed.
async fn loopback_recv_impl_async(args: LoopbackRecvArgs) -> Option<CallbackData> {
    let slot = {
        let g = GLOBAL.lock().ok()?;
        g.loopback_slots.get(&args.h).cloned()
    }?;
    // Hold the slot's async mutex only long enough to `.take()` the
    // receiver — the actual await happens outside the lock so a second
    // caller doesn't block while the first waits.
    let rx_opt = {
        let mut rx_guard = slot.receiver.lock().await;
        rx_guard.take()
    };
    let rx = rx_opt?;
    match tokio::time::timeout(
        std::time::Duration::from_millis(u64::from(args.timeout_ms)),
        rx,
    )
    .await
    {
        Ok(Ok(data)) => Some(data),
        _ => None,
    }
}

fn should_cancel_impl() -> bool {
    crate::orchestrator::state::current()
        .map(|s| s.cancel.is_cancelled())
        .unwrap_or(false)
}

#[allow(
    clippy::print_stdout,
    reason = "this fn's purpose is user-facing stdout output"
)]
fn write_stdout_impl(bytes: &[u8]) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    // Best-effort: drop on error rather than panic. A broken stdout
    // (e.g. SIGPIPE) is reported elsewhere by the parent process.
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

#[allow(
    clippy::print_stderr,
    reason = "this fn's purpose is user-facing stderr output"
)]
fn write_stderr_impl(bytes: &[u8]) {
    use std::io::Write;
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(bytes);
    let _ = err.flush();
}

// ─── Per-step thread-local context ─────────────────────────────────────────
//
// The scheduler sets `CURRENT_STEP_ID` around each
// `call_capability("hm_executor_run", …)` invocation so host fns like
// `emit_step_log` can tag emitted events with the right step. Outside an
// orchestrator-driven run the cell stays `None`, and those host fns
// short-circuit to a no-op.

thread_local! {
    static CURRENT_STEP_ID: std::cell::Cell<Option<uuid::Uuid>> =
        const { std::cell::Cell::new(None) };
}

// Callers land in cluster 10 (scheduler); these setters are part of
// the public-within-crate API the scheduler will wire up.
#[allow(dead_code)]
pub(crate) fn set_current_step_id(id: uuid::Uuid) {
    CURRENT_STEP_ID.with(|c| c.set(Some(id)));
}

#[allow(dead_code)]
pub(crate) fn clear_current_step_id() {
    CURRENT_STEP_ID.with(|c| c.set(None));
}

pub(crate) fn current_step_id() -> Option<uuid::Uuid> {
    CURRENT_STEP_ID.with(std::cell::Cell::get)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    unsafe_code,
    reason = "tests poke env vars via std::env::set_var, which is unsafe in Rust 2024"
)]
mod plugin_kv_tests {
    use super::*;

    // Both tests mutate the process-wide `XDG_CONFIG_HOME` env var,
    // which the platform config_dir lookup reads. Serialize them so
    // parallel test threads don't race on that global.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn plugin_kv_round_trip_through_disk() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        // SAFETY: in-process env var set; serialized by ENV_MUTEX.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp.path());
        }
        set_current_plugin_name("test-plugin".into());

        kv_set_impl(KvScope::Plugin, "key", b"value".to_vec());
        assert_eq!(kv_get_impl(KvScope::Plugin, "key"), Some(b"value".to_vec()));

        let again = kv_get_impl(KvScope::Plugin, "key");
        assert_eq!(again, Some(b"value".to_vec()));

        clear_current_plugin_name();
    }

    #[test]
    fn plugin_kv_isolated_per_plugin_name() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        // SAFETY: in-process env var set; serialized by ENV_MUTEX.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp.path());
        }

        set_current_plugin_name("alpha".into());
        kv_set_impl(KvScope::Plugin, "k", b"a".to_vec());

        set_current_plugin_name("beta".into());
        assert_eq!(kv_get_impl(KvScope::Plugin, "k"), None);

        set_current_plugin_name("alpha".into());
        assert_eq!(kv_get_impl(KvScope::Plugin, "k"), Some(b"a".to_vec()));

        clear_current_plugin_name();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod loopback_tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn spawn_then_recv_callback() {
        let handle = spawn_loopback_impl_async(None).await.unwrap();
        let port = handle.0;

        // Issue the callback against the bound port. Detached: the
        // listener captures the URI and shuts down after responding;
        // whether the client sees a clean close or a reset doesn't
        // matter for our assertion.
        let url = format!("http://127.0.0.1:{port}/cb?code=xyz&state=abc");
        let _client = tokio::spawn(async move {
            let _ = reqwest::get(&url).await;
        });

        let data = loopback_recv_impl_async(LoopbackRecvArgs {
            h: handle,
            timeout_ms: 5000,
        })
        .await
        .expect("got callback");
        assert_eq!(data.path, "/cb");
        assert_eq!(data.query.get("code"), Some(&"xyz".to_string()));
        assert_eq!(data.query.get("state"), Some(&"abc".to_string()));
    }
}
