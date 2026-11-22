use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use crate::i18n;

/* ─────────────────────────────────────────────────────────────
   updater.rs — Latest upstream release & firmware
   ───────────────────────────────────────────────────────────── */

const UPSTREAM_RELEASES_URL: &str = "https://github.com/yunusemreyl/omen-space/releases";
const UPSTREAM_LATEST_API: &str = "https://api.github.com/repos/yunusemreyl/omen-space/releases/latest";

async fn curl_fetch(url: &str) -> Result<String, String> {
    let out = tokio::process::Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "-sSL",
            "--connect-timeout",
            "8",
            "--max-time",
            "15",
            "-H",
            "User-Agent: OmenSpace-GUI-Updater",
            url,
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Err(format!("HTTP request failed ({})", out.status));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

/// Tag of the latest upstream release (e.g. "v2.3.0").
async fn latest_release_tag() -> Option<String> {
    let body = curl_fetch(UPSTREAM_LATEST_API).await.ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    json["tag_name"].as_str().map(str::to_string)
}

pub fn build_page(window: &adw::ApplicationWindow) -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    let status_page = adw::StatusPage::builder()
        .icon_name("software-update-available-symbolic")
        .title(i18n::t("title_updater"))
        .description(i18n::t("updater_desc"))
        .margin_top(18)
        .margin_bottom(8)
        .build();
    page.append(&status_page);

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .margin_start(24)
        .margin_end(24)
        .margin_bottom(32)
        .build();
    page.append(&content_box);


    // ── 1. Upstream releases ─────────────────────────────────
    // Personal fork: no version check or updater, just the latest upstream release and a
    // link to review it. The tag is fetched once, the first time this page is shown.
    let app_group = adw::PreferencesGroup::builder()
        .title("OmenSpace")
        .build();

    let release_row = adw::ActionRow::builder()
        .title(i18n::t("upstream_latest_title"))
        .subtitle(i18n::t("upstream_latest_sub"))
        .use_markup(false)
        .activatable(true)
        .build();
    release_row.add_prefix(
        &gtk::Image::builder()
            .icon_name("software-update-available-symbolic")
            .pixel_size(24)
            .margin_end(12)
            .build(),
    );
    let tag_label = gtk::Label::builder()
        .label("…")
        .css_classes(["os-chip-btn", "accent"])
        .valign(gtk::Align::Center)
        .build();
    release_row.add_suffix(&tag_label);
    let open_btn = gtk::Button::builder()
        .icon_name("web-browser-symbolic")
        .tooltip_text(i18n::t("upstream_open_releases"))
        .valign(gtk::Align::Center)
        .build();
    release_row.add_suffix(&open_btn);
    release_row.set_activatable_widget(Some(&open_btn));
    open_btn.connect_clicked(glib::clone!(@weak window => move |_| {
        gtk::UriLauncher::new(UPSTREAM_RELEASES_URL).launch(
            Some(&window),
            gtk::gio::Cancellable::NONE,
            |_| {},
        );
    }));
    app_group.add(&release_row);
    content_box.append(&app_group);

    let fetched = std::cell::Cell::new(false);
    release_row.connect_map(move |_| {
        if fetched.replace(true) {
            return;
        }
        let tag_label = tag_label.clone();
        glib::spawn_future_local(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            crate::daemon_client::get_runtime().spawn(async move {
                let _ = tx.send(latest_release_tag().await);
            });
            match rx.await.ok().flatten() {
                Some(tag) => tag_label.set_label(&tag),
                None => {
                    tag_label.set_label("—");
                    tag_label.set_tooltip_text(Some(i18n::t("upstream_fetch_failed")));
                }
            }
        });
    });

    // ── 3. Firmware & Kernel Driver Group ────────────────────
    let specs = crate::daemon_client::get_hardware_specs_sync();
    let fw_group = adw::PreferencesGroup::builder()
        .title(i18n::t("firmware_group"))
        .description(i18n::t("firmware_desc"))
        .build();

    // Live DKMS driver status row
    let dkms_loaded = std::path::Path::new("/sys/module/hp_wmi").exists()
        && std::path::Path::new("/sys/module/hp_omen_extra").exists();
    // Set plain text after construction: ActionRow otherwise parses the initial
    // title/subtitle as markup ("Loaded & Active") before applying use-markup=false.
    let dkms_row = adw::ActionRow::builder().use_markup(false).build();
    dkms_row.set_title(i18n::t("updater_dkms_driver"));
    dkms_row.set_subtitle(if dkms_loaded {
        i18n::t("updater_dkms_loaded")
    } else {
        i18n::t("updater_dkms_missing")
    });
    let dkms_icon = gtk::Image::builder()
        .icon_name(if dkms_loaded {
            "object-select-symbolic"
        } else {
            "dialog-warning-symbolic"
        })
        .pixel_size(24)
        .margin_end(12)
        .build();
    dkms_row.add_prefix(&dkms_icon);
    fw_group.add(&dkms_row);

    let icons = [
        "firmware-motherboard-symbolic",
        "cpu-symbolic",
        "video-display-symbolic",
    ];
    for (i, (device, ver)) in [
        ("HP BIOS", specs.bios_version.as_str()),
        ("HP EC Firmware", specs.ec_version.as_str()),
        ("NVIDIA vBIOS", specs.vbios_version.as_str()),
    ]
    .iter()
    .enumerate()
    {
        let row = adw::ActionRow::builder()
            .title(*device)
            .subtitle(*ver)
            .build();
        let icon = gtk::Image::builder()
            .icon_name(icons[i])
            .pixel_size(24)
            .margin_end(12)
            .build();
        row.add_prefix(&icon);
        fw_group.add(&row);
    }

    let fwupd_row = adw::ActionRow::builder()
        .title(i18n::t("scan_fwupd"))
        .subtitle(i18n::t("scan_fwupd_sub"))
        .build();
    let fwupd_icon = gtk::Image::builder()
        .icon_name("system-software-update-symbolic")
        .pixel_size(24)
        .margin_end(12)
        .build();
    fwupd_row.add_prefix(&fwupd_icon);

    let fwupd_btn = gtk::Button::builder()
        .label(i18n::t("scan_fwupd"))
        .css_classes(["suggested-action", "pill"])
        .valign(gtk::Align::Center)
        .build();

    let win_clone = window.clone();
    fwupd_btn.connect_clicked(move |_| {
        show_firmware_update_modal(&win_clone);
    });
    fwupd_row.add_suffix(&fwupd_btn);

    fw_group.add(&fwupd_row);
    content_box.append(&fw_group);

    page
}

fn show_firmware_update_modal(window: &adw::ApplicationWindow) {
    let dialog = adw::MessageDialog::builder()
        .heading(i18n::t("scan_fwupd"))
        .body(i18n::t("checking_updates_body"))
        .transient_for(window)
        .build();

    dialog.add_response("cancel", i18n::t("cancel"));
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    let spinner = gtk::Spinner::builder()
        .spinning(true)
        .halign(gtk::Align::Center)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    dialog.set_extra_child(Some(&spinner));

    let dialog_clone = dialog.clone();
    glib::spawn_future_local(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        crate::daemon_client::get_runtime().spawn(async move {
            let refresh = tokio::process::Command::new("fwupdmgr")
                .args(["refresh"])
                .output()
                .await;
            let updates = tokio::process::Command::new("fwupdmgr")
                .args(["get-updates", "--no-unreported-check"])
                .output()
                .await;
            let _ = tx.send((refresh, updates));
        });

        if let Ok((refresh_res, updates_res)) = rx.await {
            spinner.set_spinning(false);
            spinner.set_visible(false);

            match (refresh_res, updates_res) {
                (Err(_), _) | (_, Err(_)) => {
                    dialog_clone.set_body(i18n::t("fwupdmgr_missing"));
                }
                (Ok(_), Ok(upd)) => {
                    let stdout = String::from_utf8_lossy(&upd.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&upd.stderr).trim().to_string();
                    // Exit code 2 from fwupdmgr get-updates means "No updates available"
                    if upd.status.code() == Some(2)
                        || stdout.to_lowercase().contains("no updates")
                        || stderr.to_lowercase().contains("no updates")
                        || (upd.status.success() && stdout.is_empty())
                    {
                        dialog_clone.set_body(i18n::t("no_updates"));
                    } else if upd.status.success() && !stdout.is_empty() {
                        dialog_clone.set_body(&stdout);
                    } else {
                        dialog_clone.set_body(i18n::t("no_updates"));
                    }
                }
            }
        }

        dialog_clone.add_response("ok", i18n::t("ok_btn"));
        dialog_clone.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    });

    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_codes() {
        let raw = "\x1b[0;32m[OK]\x1b[0m Custom hp-wmi loaded\r";
        assert_eq!(strip_ansi_codes(raw), "[OK] Custom hp-wmi loaded");
    }

    #[test]
    fn test_is_newer_semver() {
        assert!(is_newer_semver("2.1.7", "2.1.8"));
        assert!(!is_newer_semver("2.1.8", "2.1.7"));
        assert!(!is_newer_semver("2.1.8", "2.1.8"));
    }
}
