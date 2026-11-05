use log::info;
use zbus::fdo::PropertiesProxy;
use futures::StreamExt;

/// Keeps the HP thermal policy in sync with the system power profile (power-profiles-daemon).
///
/// The desktop (e.g. KDE) already switches the power profile on AC plug/unplug according to
/// the user's settings, so the daemon no longer forces Performance/Quiet on AC changes itself:
/// that overrode the user's choice and raised a notification on every plug/unplug.
#[derive(Clone)]
pub struct PowerAutomationService;

impl PowerAutomationService {
    pub fn new() -> Self {
        Self
    }

    pub fn start_monitor(&self) {
        tokio::spawn(async move {
            if let Ok(conn) = zbus::Connection::system().await {
                if let Ok(proxy) = PropertiesProxy::builder(&conn)
                    .destination("net.hadess.PowerProfiles").unwrap()
                    .path("/net/hadess/PowerProfiles").unwrap()
                    .build().await {

                    if let Ok(mut stream) = proxy.receive_properties_changed().await {
                        while let Some(signal) = stream.next().await {
                            if let Ok(args) = signal.args() {
                                if args.interface_name() == "net.hadess.PowerProfiles" {
                                    if let Some(val) = args.changed_properties().get("ActiveProfile") {
                                        if let Ok(profile_str) = <&str>::try_from(val) {
                                            info!("PPD Profile changed to: {}", profile_str);
                                            let mode = match profile_str {
                                                "performance" => "Performance",
                                                "power-saver" => "Quiet",
                                                _ => "Default",
                                            };
                                            let _ = crate::platform::set_thermal_policy_by_name(mode);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
