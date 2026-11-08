mod i18n;
mod overlay_proc;

use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::MenuItem;
use i18n::t;
use log::{error, info, warn};
use std::process::Command;
use std::sync::OnceLock;
use zbus::{Connection, Result as ZbusResult};
use omen_types::*;
static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();

fn spawn_task<F>(f: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Some(handle) = RUNTIME.get() {
        handle.spawn(f);
    } else {
        error!("Tokio runtime handle is not initialized");
    }
}

fn acquire_single_instance_lock() -> Option<std::fs::File> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/tmp/user-{}", unsafe { libc::getuid() }));
    let lock_path = format!("{}/omen-tray.lock", runtime_dir);

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .ok()?;

    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if res != 0 {
        return None;
    }

    Some(file)
}

fn spawn_gui() {
    omen_types::repair_graphical_session_env();

    let spawned: Option<std::process::Child> = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("omen-gui")))
        .and_then(|gui_path| {
            if gui_path.exists() {
                Command::new(gui_path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                    .ok()
            } else {
                None
            }
        })
        .or_else(|| {
            Command::new("omen-gui")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .ok()
        })
        .or_else(|| {
            Command::new("/usr/bin/omen-gui")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .ok()
        });

    // Reap the child once it exits so it never lingers as a zombie.
    // (Zombies previously accumulated and broke the OMEN-key toggle, which
    // relies on pgrep -x omen-gui.)
    if let Some(mut child) = spawned {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Serialises overlay toggles so a tray-menu click and a hotkey signal that arrive
/// together cannot both decide "not running" and start two HUDs.
static OVERLAY_TOGGLE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Toggle the quick HUD: close it if a live one exists, otherwise start it.
///
/// "Live" is decided from `/proc` (see `overlay_proc`), not `pgrep`: a zombie
/// `omen-overlay` (e.g. a child someone started and never reaped) matches
/// `pgrep -x`, which used to make this function "close" a corpse and never open
/// the HUD.
fn spawn_overlay() {
    let _guard = OVERLAY_TOGGLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    omen_types::repair_graphical_session_env();

    // SAFETY: getuid() has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let running = overlay_proc::find_live_pids(std::path::Path::new("/proc"), "omen-overlay", uid);

    if !running.is_empty() {
        for pid in running {
            // SAFETY: plain kill(2) on a pid we just matched as a live omen-overlay of ours.
            let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            if rc == 0 {
                info!("Closed overlay (pid {})", pid);
            } else {
                warn!("Could not signal overlay pid {}: {}", pid, std::io::Error::last_os_error());
            }
        }
        return;
    }

    // Prefer the binary next to this one, then fall back to $PATH and /usr/bin.
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(sibling) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("omen-overlay")))
    {
        if sibling.exists() {
            candidates.push(sibling);
        }
    }
    candidates.push(std::path::PathBuf::from("omen-overlay"));
    candidates.push(std::path::PathBuf::from("/usr/bin/omen-overlay"));

    let mut last_error: Option<(std::path::PathBuf, std::io::Error)> = None;
    for candidate in candidates {
        // stderr is inherited (the journal when run as a user unit) so a crashing
        // overlay is no longer invisible; its stdout is not interesting.
        match Command::new(&candidate)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()
        {
            Ok(mut child) => {
                info!("Started overlay {} (pid {})", candidate.display(), child.id());
                // Reap the child once it exits so it never lingers as a zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return;
            }
            Err(e) => last_error = Some((candidate, e)),
        }
    }
    if let Some((path, e)) = last_error {
        warn!("Failed to start overlay '{}': {}", path.display(), e);
    }
}

async fn toggle_overlay() {
    spawn_overlay();
}

#[derive(Debug, Clone)]
struct Tray {
    power_profile: String,
    fan_mode: String,
    gpu_mode: String,
    /// Firmware accepts Advanced Optimus (from Mux.GetGpuInfo supported_modes).
    gpu_advanced: bool,
    battery_info: Option<String>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "omenspace_tray".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    fn icon_name(&self) -> String {
        "omenspace".into()
    }

    fn icon_theme_path(&self) -> String {
        "/usr/share/omen-space/assets".into()
    }

    fn title(&self) -> String {
        "OMEN SPACE".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let p_label = match self.power_profile.as_str() {
            "performance" => t("perf"),
            "power-saver" | "eco" => t("eco"),
            _ => t("balanced"),
        };
        let f_label = match self.fan_mode.as_str() {
            "max" => t("max"),
            "ec" => t("ec"),
            "custom" => t("custom"),
            _ => t("auto"),
        };
        let g_label = match self.gpu_mode.as_str() {
            "discrete" => t("tt_gpu_discrete"),
            "advanced" => t("tt_gpu_advanced"),
            _ => t("tt_gpu_hybrid"),
        };
        let mut desc = format!("{}: {}\n{}: {}\n{}: {}", t("tt_power"), p_label, t("tt_fan"), f_label, t("tt_gpu"), g_label);
        if let Some(ref bat) = self.battery_info {
            desc.push_str(&format!("\n{}: {}", t("tt_battery"), bat));
        }
        ksni::ToolTip {
            title: "OMEN Space".into(),
            description: desc,
            icon_name: "omenspace".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        spawn_gui();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let cur_power = self.power_profile.as_str();
        let cur_fan = self.fan_mode.as_str();
        let cur_gpu = self.gpu_mode.as_str();

        vec![
            StandardItem {
                label: t("tray_open").into(),
                icon_name: "omenspace".into(),
                activate: Box::new(|_| {
                    spawn_gui();
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: t("tray_overlay").into(),
                icon_name: "preferences-desktop-display".into(),
                activate: Box::new(|_| {
                    spawn_task(async {
                        toggle_overlay().await;
                    });
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: t("power_profile").into(),
                submenu: vec![
                    CheckmarkItem {
                        label: t("perf").into(),
                        checked: cur_power == "performance",
                        activate: Box::new(|tray: &mut Self| {
                            tray.power_profile = "performance".into();
                            spawn_task(async {
                                set_power_profile("performance").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("balanced").into(),
                        checked: cur_power == "balanced",
                        activate: Box::new(|tray: &mut Self| {
                            tray.power_profile = "balanced".into();
                            spawn_task(async {
                                set_power_profile("balanced").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("eco").into(),
                        checked: cur_power == "power-saver" || cur_power == "eco",
                        activate: Box::new(|tray: &mut Self| {
                            tray.power_profile = "power-saver".into();
                            spawn_task(async {
                                set_power_profile("power-saver").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: t("fan_mode").into(),
                submenu: vec![
                    CheckmarkItem {
                        label: t("auto").into(),
                        checked: cur_fan == "auto",
                        activate: Box::new(|tray: &mut Self| {
                            tray.fan_mode = "auto".into();
                            spawn_task(async {
                                set_fan_mode("auto").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("max").into(),
                        checked: cur_fan == "max",
                        activate: Box::new(|tray: &mut Self| {
                            tray.fan_mode = "max".into();
                            spawn_task(async {
                                set_fan_mode("max").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("ec").into(),
                        checked: cur_fan == "ec",
                        activate: Box::new(|tray: &mut Self| {
                            tray.fan_mode = "ec".into();
                            spawn_task(async {
                                set_fan_mode("ec").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: t("gpu_mode").into(),
                submenu: vec![
                    CheckmarkItem {
                        label: t("hybrid").into(),
                        checked: cur_gpu == "hybrid",
                        activate: Box::new(|tray: &mut Self| {
                            tray.gpu_mode = "hybrid".into();
                            spawn_task(async {
                                set_gpu_mode("hybrid").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("discrete").into(),
                        checked: cur_gpu == "discrete",
                        activate: Box::new(|tray: &mut Self| {
                            tray.gpu_mode = "discrete".into();
                            spawn_task(async {
                                set_gpu_mode("discrete").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                ]
                .into_iter()
                .chain(self.gpu_advanced.then(|| {
                    CheckmarkItem {
                        label: t("advanced").into(),
                        checked: cur_gpu == "advanced",
                        activate: Box::new(|tray: &mut Self| {
                            tray.gpu_mode = "advanced".into();
                            spawn_task(async {
                                set_gpu_mode("advanced").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into()
                }))
                .collect(),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: t("exit").into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| {
                    let _ = Command::new("pkill").arg("-TERM").arg("-x").arg("omen-gui").output();
                    let _ = Command::new("pkill").arg("-TERM").arg("-x").arg("omenctl").output();
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}



static SHARED_CONN: tokio::sync::Mutex<Option<Connection>> = tokio::sync::Mutex::const_new(None);

async fn get_conn() -> ZbusResult<Connection> {
    let mut guard = SHARED_CONN.lock().await;
    if let Some(conn) = &*guard {
        return Ok(conn.clone());
    }
    let conn = Connection::system().await?;
    *guard = Some(conn.clone());
    Ok(conn)
}

async fn fetch_power_profile() -> Option<String> {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = PowerProxy::new(&conn).await {
            if let Ok(json_str) = proxy.get_power_profile().await {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(active) = val.get("active").and_then(|v| v.as_str()) {
                        return Some(active.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Eco (power-saver) runs the internal panel at its ~60 Hz mode and the other profiles at its
/// highest refresh rate, at the current resolution. Only the built-in eDP output is touched;
/// nothing is done when it is already in the wanted mode.
fn apply_panel_refresh_for_profile(profile: &str) {
    let Ok(out) = Command::new("kscreen-doctor").arg("-j").output() else { return };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return };
    let Some(outputs) = json["outputs"].as_array() else { return };
    let Some(panel) = outputs.iter().find(|o| {
        o["name"].as_str().is_some_and(|n| n.starts_with("eDP"))
            && o["connected"].as_bool() == Some(true)
            && o["enabled"].as_bool() == Some(true)
    }) else { return };
    let (Some(name), Some(modes)) = (panel["name"].as_str(), panel["modes"].as_array()) else { return };
    let current_id = panel["currentModeId"].as_str().unwrap_or_default();
    let Some(current) = modes.iter().find(|m| m["id"].as_str() == Some(current_id)) else { return };
    let same_size = modes.iter().filter(|m| m["size"] == current["size"]);
    let rate = |m: &serde_json::Value| m["refreshRate"].as_f64().unwrap_or(0.0);
    let same_size: Vec<&serde_json::Value> = same_size.collect();
    let target = if profile == "power-saver" {
        // With amdgpu.freesync_video=1 there are two ~60 Hz modes: the panel's own (full
        // modeset, ~1 s black) and a VRR variant of the max mode appended after the EDID modes,
        // which switches with just a flicker. Prefer the later (VRR) one.
        let id_num = |m: &serde_json::Value| m["id"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let best = same_size.iter().map(|m| (rate(m) - 60.0).abs()).fold(f64::INFINITY, f64::min);
        same_size
            .iter()
            .copied()
            .filter(|m| (rate(m) - 60.0).abs() <= best + 0.5)
            .max_by_key(|m| id_num(m))
    } else {
        same_size.iter().copied().max_by(|a, b| rate(a).total_cmp(&rate(b)))
    };
    let Some(target_id) = target.and_then(|m| m["id"].as_str()) else { return };
    if target_id != current_id {
        info!("Power profile {}: switching {} to {:.0} Hz", profile, name, target.map(rate).unwrap_or(0.0));
        let _ = Command::new("kscreen-doctor").arg(format!("output.{}.mode.{}", name, target_id)).output();
    }
}

/// Applies the panel refresh rate for the current power profile, then sleeps until
/// power-profiles-daemon signals an ActiveProfile change (no polling).
async fn follow_power_profile_for_panel() {
    use futures::StreamExt;
    loop {
        if let Ok(conn) = Connection::system().await {
            let proxy = zbus::fdo::PropertiesProxy::builder(&conn)
                .destination("net.hadess.PowerProfiles")
                .and_then(|b| b.path("/net/hadess/PowerProfiles"));
            if let Ok(proxy) = match proxy { Ok(b) => b.build().await, Err(e) => Err(e) } {
                if let Ok(mut stream) = proxy.receive_properties_changed().await {
                    let iface = zbus::names::InterfaceName::from_static_str_unchecked("net.hadess.PowerProfiles");
                    if let Ok(v) = proxy.get(iface, "ActiveProfile").await {
                        if let Ok(p) = <String>::try_from(v) {
                            let _ = tokio::task::spawn_blocking(move || apply_panel_refresh_for_profile(&p)).await;
                        }
                    }
                    while let Some(signal) = stream.next().await {
                        let Ok(args) = signal.args() else { continue };
                        if args.interface_name() != "net.hadess.PowerProfiles" {
                            continue;
                        }
                        if let Some(v) = args.changed_properties().get("ActiveProfile") {
                            if let Ok(p) = <&str>::try_from(v) {
                                let p = p.to_string();
                                let _ = tokio::task::spawn_blocking(move || apply_panel_refresh_for_profile(&p)).await;
                            }
                        }
                    }
                }
            }
        }
        // power-profiles-daemon unavailable or restarted: retry later.
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

async fn fetch_fan_mode() -> Option<String> {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = FanProxy::new(&conn).await {
            if let Ok(mode) = proxy.get_fan_mode().await {
                return Some(mode.trim().to_string());
            }
        }
    }
    None
}

/// Whether the firmware accepts Advanced Optimus; true when the daemon doesn't say.
async fn fetch_gpu_supports_advanced() -> bool {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = MuxProxy::new(&conn).await {
            if let Ok(json_str) = proxy.get_gpu_info().await {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(modes) = val.get("supported_modes").and_then(|v| v.as_array()) {
                        return modes.iter().any(|m| m.as_str() == Some("advanced"));
                    }
                }
            }
        }
    }
    true
}

async fn fetch_gpu_mode() -> Option<String> {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = MuxProxy::new(&conn).await {
            if let Ok(json_str) = proxy.get_gpu_info().await {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(mode) = val.get("mode").and_then(|v| v.as_str()) {
                        return Some(mode.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Battery line for the tooltip, read straight from sysfs (a few small files) instead of
/// asking the daemon for full system stats every refresh.
async fn fetch_battery_info() -> Option<Option<String>> {
    let read = |dir: &std::path::Path, f: &str| std::fs::read_to_string(dir.join(f)).ok().map(|s| s.trim().to_string());
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else { return Some(None) };
    let Some(bat) = entries
        .flatten()
        .map(|e| e.path())
        .find(|p| read(p, "type").as_deref() == Some("Battery") && read(p, "scope").as_deref() != Some("Device"))
    else {
        return Some(None);
    };
    let charge = read(&bat, "capacity")?;
    let status_str = match read(&bat, "status").as_deref() {
        Some("Charging") => t("bat_charging"),
        Some("Discharging") => t("bat_discharging"),
        Some("Not charging") => t("bat_not_charging"),
        Some("Full") => t("bat_full"),
        _ => "...",
    };
    let full = read(&bat, "energy_full").or_else(|| read(&bat, "charge_full")).and_then(|v| v.parse::<f64>().ok());
    let design = read(&bat, "energy_full_design").or_else(|| read(&bat, "charge_full_design")).and_then(|v| v.parse::<f64>().ok());
    let health_str = match (full, design) {
        (Some(f), Some(d)) if d > 0.0 => format!(" · {:.0}% {}", (f / d * 100.0).min(100.0), t("battery_health")),
        _ => String::new(),
    };
    Some(Some(format!("{}% ({}){}", charge, status_str, health_str)))
}

async fn set_power_profile(profile: &str) {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = PowerProxy::new(&conn).await {
            match proxy.set_power_profile(profile).await {
                Ok(resp) => info!("Güç profili ayarlandı ({}) -> {}", profile, resp),
                Err(e) => error!("Güç profili değiştirilemedi: {}", e),
            }
        }
    }
}

async fn set_fan_mode(mode: &str) {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = FanProxy::new(&conn).await {
            match proxy.set_fan_mode(mode).await {
                Ok(resp) => info!("Fan modu ayarlandı ({}) -> {}", mode, resp),
                Err(e) => error!("Fan modu değiştirilemedi: {}", e),
            }
        }
    }
}

async fn set_gpu_mode(mode: &str) {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = MuxProxy::new(&conn).await {
            match proxy.set_gpu_mode(mode).await {
                Ok(resp) => {
                    info!("GPU modu ayarlandı ({}) -> {}", mode, resp);
                    if resp.contains("REBOOT") {
                        if let Ok(mut child) = Command::new("notify-send")
                            .arg("OMEN Space")
                            .arg(t("gpu_reboot_notify"))
                            .arg("-i")
                            .arg("dialog-warning")
                            .spawn() {
                            std::thread::spawn(move || { let _ = child.wait(); });
                        }
                    }
                },
                Err(e) => error!("GPU modu değiştirilemedi: {}", e),
            }
        }
    }
}

async fn wait_for_status_notifier_watcher() {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut session_conn: Option<Connection> = None;
    loop {
        if session_conn.is_none() {
            match Connection::session().await {
                Ok(c) => session_conn = Some(c),
                Err(e) => {
                    log::debug!("Could not connect to session D-Bus yet: {}", e);
                }
            }
        }

        if let Some(ref conn) = session_conn {
            let check = tokio::time::timeout(
                std::time::Duration::from_millis(800),
                conn.call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "NameHasOwner",
                    &"org.kde.StatusNotifierWatcher",
                ),
            )
            .await;

            match check {
                Ok(Ok(msg)) => {
                    if let Ok(has_owner) = msg.body().deserialize::<bool>() {
                        if has_owner {
                            info!("StatusNotifierWatcher is active on session D-Bus");
                            return;
                        }
                    }
                    log::debug!("StatusNotifierWatcher name has no owner yet");
                }
                Ok(Err(e)) => {
                    log::debug!("D-Bus check failed: {}", e);
                }
                Err(_) => {
                    log::debug!("StatusNotifierWatcher check timed out; retrying...");
                }
            }
        }

        if tokio::time::Instant::now() >= deadline {
            warn!("Timed out waiting for StatusNotifierWatcher; spawning tray anyway");
            return;
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

#[tokio::main]
async fn main() {
    omen_types::repair_graphical_session_env();

    let _lock_file = match acquire_single_instance_lock() {
        Some(file) => file,
        None => {
            eprintln!("omen-tray zaten çalışıyor, ikinci örnek sonlandırılıyor.");
            return;
        }
    };

    // Show our own info-level lines (hotkey received, overlay started/closed) in the
    // journal by default; RUST_LOG still overrides this.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,omen_tray=info"))
    .init();
    info!("omen-tray başlatılıyor...");

    i18n::init();

    RUNTIME
    .set(tokio::runtime::Handle::current())
    .expect("Failed to initialize runtime handle");

    // Listen for OMEN key presses from the zero-overhead hotkey monitor immediately,
    // before waiting on the desktop tray watcher.
    tokio::spawn(async move {
        use futures::StreamExt;
        loop {
            match get_conn().await {
                Ok(conn) => match PlatformProxy::new(&conn).await {
                    Ok(proxy) => match proxy.receive_macro_key_pressed().await {
                        Ok(mut stream) => {
                            info!("Subscribed to daemon hotkey signals");
                            while let Some(msg) = stream.next().await {
                                if let Ok(args) = msg.args() {
                                    let key_name = args.key_name();
                                    if *key_name == "omen" {
                                        info!("OMEN key detected, launching GUI...");
                                        spawn_gui();
                                    } else if *key_name == "overlay" {
                                        info!("Overlay hotkey detected, toggling overlay...");
                                        spawn_overlay();
                                    }
                                }
                            }
                            warn!("Hotkey signal stream ended; resubscribing");
                        }
                        Err(e) => warn!("Cannot subscribe to MacroKeyPressed: {}", e),
                    },
                    Err(e) => warn!("Cannot create Platform proxy for hotkeys: {}", e),
                },
                Err(e) => warn!("Cannot connect to D-Bus for hotkeys: {}", e),
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    });

    // Wait until org.kde.StatusNotifierWatcher is responsive so ksni 0.2.2
    // does not panic on a 1-second D-Bus NoReply timeout during login.
    wait_for_status_notifier_watcher().await;

    let initial_power = fetch_power_profile().await.unwrap_or_else(|| "balanced".into());
    tokio::spawn(follow_power_profile_for_panel());
    let initial_fan = fetch_fan_mode().await.unwrap_or_else(|| "auto".into());
    let initial_gpu = fetch_gpu_mode().await.unwrap_or_else(|| "hybrid".into());
    let gpu_advanced = fetch_gpu_supports_advanced().await;
    let initial_bat = fetch_battery_info().await.unwrap_or(None);

    let tray = Tray {
        power_profile: initial_power,
        fan_mode: initial_fan,
        gpu_mode: initial_gpu,
        gpu_advanced,
        battery_info: initial_bat,
    };

    let service = ksni::TrayService::new(tray);
    let handle = service.handle();

    // Spawn ksni on a dedicated permanent OS thread so it runs its blocking
    // event loop indefinitely without blocking the Tokio runtime executor.
    std::thread::Builder::new()
    .name("ksni-tray".into())
    .spawn(move || {
        service.spawn();
    })
    .expect("Failed to spawn tray service thread");

    // Main background update loop for refreshing tray tooltip/status states
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let p = fetch_power_profile().await;
        let f = fetch_fan_mode().await;
        let g = fetch_gpu_mode().await;
        let b = fetch_battery_info().await;
        if p.is_some() || f.is_some() || g.is_some() || b.is_some() {
            handle.update(|tray| {
                if let Some(new_p) = p {
                    tray.power_profile = new_p;
                }
                if let Some(new_f) = f {
                    tray.fan_mode = new_f;
                }
                if let Some(new_g) = g {
                    tray.gpu_mode = new_g;
                }
                if let Some(new_b) = b {
                    tray.battery_info = new_b;
                }
            });
        }
    }
}
