//! Brightness below the panel's minimum (OMEN 17 8C75).
//!
//! From about 8 % of KDE's brightness slider downwards the backlight barely changes: the
//! firmware's minimum is reached at 6 % and 7-8 % are only one step above it. In that range the
//! image itself is darkened with an ICC profile whose calibration curve (vcgt) scales every
//! channel: 100 % at the threshold down to 50 % at 0 %. KWin applies it in its colour pipeline,
//! so Night Light still works on top. Only reacts to KDE's BrightnessChanged signal (no polling).

use futures::StreamExt;
use log::{info, warn};
use std::path::PathBuf;
use std::process::Command;
use zbus::{Connection, MatchRule, MessageStream};

/// KDE brightness percentage from which the image is shown unchanged.
const THRESHOLD_PCT: u32 = 8;
/// Image level at 0 %.
const MIN_FACTOR: f64 = 0.5;

/// lcms2's built-in sRGB profile; the vcgt tag is appended to it.
const SRGB_BASE: &[u8] = include_bytes!("../assets/srgb-base.icc");

const SB_SERVICE: &str = "org.kde.ScreenBrightness";
const SB_PATH: &str = "/org/kde/ScreenBrightness";
const SB_DISPLAY_IFACE: &str = "org.kde.ScreenBrightness.Display";

fn factor_for(level: u32) -> f64 {
    MIN_FACTOR + (1.0 - MIN_FACTOR) * level as f64 / THRESHOLD_PCT as f64
}

/// SRGB_BASE plus a vcgt tag (3 channels x 256 entries, 16 bit) scaled by `factor`.
fn build_profile(factor: f64) -> Option<Vec<u8>> {
    let base = SRGB_BASE;
    let be32 = |o: usize| -> Option<u32> { Some(u32::from_be_bytes(base.get(o..o + 4)?.try_into().ok()?)) };
    let count = be32(128)? as usize;
    let data_start = 132 + 12 * count;
    let mut body = base.get(data_start..)?.to_vec();
    while body.len() % 4 != 0 {
        body.push(0);
    }

    let mut vcgt = Vec::with_capacity(18 + 3 * 512);
    vcgt.extend_from_slice(b"vcgt");
    vcgt.extend_from_slice(&[0; 4]);
    vcgt.extend_from_slice(&0u32.to_be_bytes()); // table
    vcgt.extend_from_slice(&3u16.to_be_bytes()); // channels
    vcgt.extend_from_slice(&256u16.to_be_bytes()); // entries
    vcgt.extend_from_slice(&2u16.to_be_bytes()); // bytes per entry
    for _ in 0..3 {
        for i in 0..256u32 {
            let v = (i as f64 / 255.0 * factor * 65535.0).round() as u16;
            vcgt.extend_from_slice(&v.to_be_bytes());
        }
    }

    // The tag table grows by one entry, so every existing offset moves by 12 bytes.
    let mut table = Vec::with_capacity(4 + 12 * (count + 1));
    table.extend_from_slice(&((count + 1) as u32).to_be_bytes());
    for i in 0..count {
        let e = 132 + 12 * i;
        table.extend_from_slice(base.get(e..e + 4)?);
        table.extend_from_slice(&(be32(e + 4)? + 12).to_be_bytes());
        table.extend_from_slice(&be32(e + 8)?.to_be_bytes());
    }
    let vcgt_offset = (data_start + 12 + body.len()) as u32;
    table.extend_from_slice(b"vcgt");
    table.extend_from_slice(&vcgt_offset.to_be_bytes());
    table.extend_from_slice(&(vcgt.len() as u32).to_be_bytes());

    let mut profile = base.get(..128)?.to_vec();
    profile.extend_from_slice(&table);
    profile.extend_from_slice(&body);
    profile.extend_from_slice(&vcgt);
    let size = profile.len() as u32;
    profile[0..4].copy_from_slice(&size.to_be_bytes());
    profile[84..100].fill(0); // profile ID: not computed
    Some(profile)
}

fn profile_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("omen-space/icc"))
}

fn profile_path(level: u32) -> Option<PathBuf> {
    Some(profile_dir()?.join(format!("brillo-{level}.icc")))
}

/// Writes the per-level profiles, rewriting any that differ (e.g. after the curve changed).
fn ensure_profiles() -> bool {
    let Some(dir) = profile_dir() else { return false };
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    (0..THRESHOLD_PCT).all(|level| {
        let (Some(path), Some(data)) = (profile_path(level), build_profile(factor_for(level))) else {
            return false;
        };
        std::fs::read(&path).ok().as_deref() == Some(&data[..]) || std::fs::write(&path, &data).is_ok()
    })
}

/// Name of the connected built-in panel (eDP-*).
fn panel_name() -> Option<String> {
    let out = Command::new("kscreen-doctor").arg("-j").output().ok()?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    json["outputs"].as_array()?.iter().find_map(|o| {
        let name = o["name"].as_str()?;
        (name.starts_with("eDP") && o["connected"].as_bool() == Some(true)).then(|| name.to_string())
    })
}

/// `level` = Some(0..THRESHOLD_PCT) for a darkened image, None for the normal sRGB one.
fn apply(panel: &str, level: Option<u32>) {
    let mut cmd = Command::new("kscreen-doctor");
    match level.and_then(|l| Some((l, profile_path(l)?))) {
        Some((l, path)) => {
            info!("Low brightness: image at {:.0} %", factor_for(l) * 100.0);
            cmd.arg(format!("output.{panel}.iccprofile.{}", path.display()))
                .arg(format!("output.{panel}.colorProfileSource.ICC"));
        }
        None => {
            cmd.arg(format!("output.{panel}.colorProfileSource.sRGB"));
        }
    }
    if let Err(e) = cmd.output() {
        warn!("Low brightness: kscreen-doctor failed: {e}");
    }
}

fn level_for(brightness: i32, max: i32) -> Option<u32> {
    if max <= 0 {
        return None;
    }
    let pct = ((brightness.max(0) as f64 * 100.0 / max as f64).round()) as u32;
    (pct < THRESHOLD_PCT).then_some(pct)
}

async fn display_prop<T>(conn: &Connection, display: &str, prop: &str) -> Option<T>
where
    T: TryFrom<zbus::zvariant::OwnedValue>,
{
    let proxy = zbus::fdo::PropertiesProxy::builder(conn)
        .destination(SB_SERVICE)
        .ok()?
        .path(format!("{SB_PATH}/{display}"))
        .ok()?
        .build()
        .await
        .ok()?;
    let iface = zbus::names::InterfaceName::from_static_str_unchecked(SB_DISPLAY_IFACE);
    T::try_from(proxy.get(iface, prop).await.ok()?).ok()
}

/// Applies the image level for KDE's current brightness, then sleeps on BrightnessChanged.
pub async fn follow() {
    if !tokio::task::spawn_blocking(ensure_profiles).await.unwrap_or(false) {
        warn!("Low brightness: cannot write the ICC profiles; feature disabled");
        return;
    }
    let mut applied: Option<Option<u32>> = None;
    loop {
        if let Ok(conn) = Connection::session().await {
            let rule = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("org.kde.ScreenBrightness")
                .and_then(|b| b.member("BrightnessChanged"))
                .and_then(|b| b.path(SB_PATH))
                .map(|b| b.build());
            if let Ok(rule) = rule {
                if let Ok(mut stream) = MessageStream::for_match_rule(rule, &conn, None).await {
                    let panel = tokio::task::spawn_blocking(panel_name).await.ok().flatten();
                    let Some(panel) = panel else {
                        warn!("Low brightness: no built-in panel found; feature disabled");
                        return;
                    };

                    let update = |level: Option<u32>, applied: &mut Option<Option<u32>>| {
                        if *applied != Some(level) {
                            *applied = Some(level);
                            let panel = panel.clone();
                            Some(tokio::task::spawn_blocking(move || apply(&panel, level)))
                        } else {
                            None
                        }
                    };

                    if let (Some(b), Some(m)) = (
                        display_prop::<i32>(&conn, "display0", "Brightness").await,
                        display_prop::<i32>(&conn, "display0", "MaxBrightness").await,
                    ) {
                        if display_prop::<bool>(&conn, "display0", "IsInternal").await == Some(true) {
                            if let Some(job) = update(level_for(b, m), &mut applied) {
                                let _ = job.await;
                            }
                        }
                    }

                    while let Some(Ok(msg)) = stream.next().await {
                        let Ok((display, brightness, _, _)) =
                            msg.body().deserialize::<(String, i32, String, String)>()
                        else {
                            continue;
                        };
                        if display_prop::<bool>(&conn, &display, "IsInternal").await != Some(true) {
                            continue;
                        }
                        let Some(max) = display_prop::<i32>(&conn, &display, "MaxBrightness").await else {
                            continue;
                        };
                        if let Some(job) = update(level_for(brightness, max), &mut applied) {
                            let _ = job.await;
                        }
                    }
                }
            }
        }
        // Session bus gone (logout): retry later.
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(level_for(0, 10000), Some(0));
        assert_eq!(level_for(700, 10000), Some(7));
        assert_eq!(level_for(800, 10000), None);
        assert_eq!(level_for(10000, 10000), None);
        assert!((factor_for(0) - 0.5).abs() < 1e-9);
        assert!((factor_for(THRESHOLD_PCT) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn profile_is_well_formed() {
        let p = build_profile(0.5).unwrap();
        let be32 = |o: usize| u32::from_be_bytes(p[o..o + 4].try_into().unwrap());
        assert_eq!(be32(0) as usize, p.len());
        assert_eq!(&p[36..40], b"acsp");
        let count = be32(128) as usize;
        let tags: Vec<_> = (0..count).map(|i| (&p[132 + 12 * i..136 + 12 * i], be32(136 + 12 * i), be32(140 + 12 * i))).collect();
        for (_, off, len) in &tags {
            assert!((*off + *len) as usize <= p.len());
        }
        let (_, off, len) = tags.iter().find(|t| t.0 == b"vcgt").unwrap();
        assert_eq!(&p[*off as usize..*off as usize + 4], b"vcgt");
        assert_eq!(*len as usize, 18 + 3 * 512);
        // last entry of the first channel = 50 % of full scale
        let last = *off as usize + 18 + 255 * 2;
        assert_eq!(u16::from_be_bytes([p[last], p[last + 1]]), 32768);
        // the original desc tag still points at its (v4) multiLocalizedUnicode data
        let (_, doff, _) = tags.iter().find(|t| t.0 == b"desc").unwrap();
        assert_eq!(&p[*doff as usize..*doff as usize + 4], b"mluc");
    }
}
