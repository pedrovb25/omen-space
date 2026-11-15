<div align="center">
  <img src="images/omenspace.png" alt="OMEN Space Logo" width="110" />

  # OMEN Space · 8C75 Edition

  **A fork of [OMEN Space](https://github.com/yunusemreyl/omen-space) tuned for the HP OMEN 17-db0xxx (board `8C75`) on Linux.**  
  Quiet and stable cooling, honest power controls and a desktop that stays in sync with KDE Plasma, all event-driven.

  <p>
    <img src="https://img.shields.io/badge/Board-HP%208C75-e11d48?style=flat-square" alt="Board 8C75" />
    <img src="https://img.shields.io/badge/Based%20on-OMEN%20Space%202.2.1-6366f1?style=flat-square" alt="Based on OMEN Space 2.2.1" />
    <img src="https://img.shields.io/badge/Daily%20driver-Nobara%2044%20%C2%B7%20KDE%20Plasma%206-0ea5e9?style=flat-square" alt="Nobara 44 · KDE Plasma 6" />
    <img src="https://img.shields.io/badge/Written%20in-Rust-f97316?style=flat-square" alt="Rust" />
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-10b981?style=flat-square" alt="License GPL-3.0" /></a>
  </p>

  <p>
    <b>English</b> · <a href="README.es.md">Español</a>
  </p>
</div>

---

## Why this fork

OMEN Space supports a long list of HP OMEN, Victus and Transcend boards. On the **OMEN 17-db0xxx (8C75)** several things behaved differently from what the generic code expects. The most serious one: the Embedded Controller never restarts fans it has stopped, so in automatic mode a hot laptop could sit at **0 RPM at 95 °C** (upstream [#175](https://github.com/yunusemreyl/omen-space/issues/175)).

This fork fixes that and tunes the rest of the stack for this one machine. Every change was measured on the real hardware: EC register dumps, the BIOS fan table, thermal limits, and power and CPU-usage readings.

> [!IMPORTANT]
> This build is **tuned and tested only on board `8C75`**. Other OMEN, Victus or Transcend models should use the [upstream project](https://github.com/yunusemreyl/omen-space).

### Test machine

| | |
| :--- | :--- |
| **Laptop** | HP OMEN 17-db0xxx · board `8C75` |
| **CPU / GPU** | AMD Ryzen 7 8845HS · NVIDIA GeForce RTX 4070 Laptop (hybrid) |
| **Display** | 17.3" 240 Hz internal panel (eDP) |
| **OS** | Nobara 44 · KDE Plasma 6 on Wayland · kernel 7.3 |

---

## What's different

### 🌀 Cooling

- **Fan control that works on the 8C75.** Fan levels are written to the EC (`XSS1`/`XSS2`). Max mode uses the WMI path, about 4500 RPM. Live RPM telemetry has been restored.
- **Steady Auto curve.** Auto moves in steps, with 6 °C of hysteresis before stepping down, so the fans hold a constant speed instead of ramping up and down every few seconds. The fans reach about 87 % before the CPU hits 90 °C. Full speed comes at 90 °C on the CPU, or at 84 °C on the dGPU.
- **Quiet mode.** A built-in quiet curve, also used automatically by the Eco power profile.
- **Fans stop at idle.** The fans switch off fully when the laptop is cool. Before, a 45 °C floor in the temperature reading and the DDR5 sensors kept them spinning. The DDR5 sensors now only count from 75 °C.
- **Follows the active power profile.** The fan curve follows whatever profile the kernel is using. That includes KDE switching to Eco when you unplug the charger and back when you plug it in.
- **Thermal protection** for sustained overheating, plus the dGPU temperature fix, both taken from upstream v2.2.1.

### ⚡ Power

- **Real AMD power limits.** The real STAPM and PPT limits from `amd-pmf` are shown as read-only values, replacing the Intel-style 45/115 W placeholders. The firmware sets them again on every profile change, so the page explains this instead of offering sliders that would not stick.
- **The desktop owns the power profile.** Plugging or unplugging the charger no longer forces a thermal profile or shows a notification. When KDE or power-profiles-daemon switches the profile, OMEN Space adopts it: it saves the profile and syncs GPU TGP/PPAB.
- **No SMU polling.** `ryzenadj` is never queried in a loop, because polling the SMU on this kernel could freeze the system.
- **No dead controls.** Curve Optimizer and battery care are marked as unavailable, because the firmware rejects them or Linux has no interface for them. The MUX page only offers the GPU modes the BIOS accepts.

### 🖥️ Display & keyboard

- **60 Hz in Eco, 240 Hz otherwise.** The internal panel uses the `freesync_video` 60 Hz mode while in Eco, which switches without a black screen.
- **Brightness below the panel's minimum.** Under 8 % on KDE's slider the backlight barely changes. In that range the image is darkened with a calibration (ICC/vcgt) profile, from 100 % down to 50 % at 0 %. Night Light keeps working on top.
- **Keyboard backlight in KDE.** The keyboard backlight is exposed via `uleds`, so the Plasma brightness slider and on-screen display work with it.

### 🪶 Efficiency

- **No polling anywhere.** The tray uses `ksni` 0.3 and reacts to D-Bus signals from the daemon, power-profiles-daemon and KDE. It no longer checks state every 50 ms.
- **A lighter daemon.** It no longer scans every process once per second. It computes live telemetry only while a window or the overlay shows it, and runs on two threads. Idle CPU use went from about 2 % to under 1 %.
- **Clean shutdown.** The daemon exits immediately on `SIGTERM`, where it used to take 45 s and end in `SIGABRT`.

### 🧭 Quality of life

- Clicking the tray icon opens the window, and clicking again closes it.
- A powered-off dGPU is labelled **Off** instead of `D3Cold`, and the DKMS driver status is shown correctly.
- Version **2.2.1**, with no background update checks. The upstream updater is disabled, because this is a personal fork: upstream releases are reviewed and ported by hand.

---

## Hardware notes (8C75)

Findings collected while building this fork, useful to anyone working on this board:

| Topic | Finding |
| :--- | :--- |
| **EC auto mode** | `XSS1`/`XSS2` = `0xFF` hands control to the EC. The EC stops the fans when cold but never restarts them. |
| **BIOS fan table** | 12 fixed levels, 1600 to 3900 RPM (22 to 46 dB), with no temperature points. HP's top level is about 87 % of the WMI maximum. |
| **Shared heatsink** | Driving the two fans independently was tested and rejected. At 75 %/30 % the CPU kept rising past 87 °C, against 79 °C with both fans at 75 %. |
| **Thermal limits** | The CPU does not throttle at 95 °C (61 W, ~4.2 GHz). The GPU targets 87 °C, throttles at ~88 °C and shuts down at ~98 °C. |
| **AMD power limits** | Set by the `amd-pmf` static slider table on every profile change. On AC: Eco and Balanced 65/54/65 W, Performance slow PPT 60 W. Much lower on battery. |
| **MUX** | The BIOS only accepts modes 0 and 1. On HP, "hybrid" is Advanced Optimus on Windows. |
| **Backlight** | Firmware minimum ≈ 2 % duty, reached at 6 % on KDE's slider. |

---

## Installation

> [!WARNING]
> Only install this fork on an HP OMEN 17-db0xxx (board `8C75`). Check your board with `cat /sys/class/dmi/id/board_name`.

```bash
git clone https://github.com/pedrovb25/omen-space.git
cd omen-space
sudo ./setup.sh install
```

The installer is unchanged from upstream. It installs the build dependencies, compiles the workspace and registers the DKMS kernel drivers (`hp-wmi` and `hp-omen-extra`). To remove everything: `sudo ./setup.sh uninstall`.

<details>
<summary><b>Branches</b></summary>
<br />

`main` is the stable line of this fork. Each change was developed on its own branch (`feat/8c75-*`, `fix/8c75-*`, `perf/*`…), each built on top of the previous one, so every fix can be reviewed or cherry-picked on its own.
</details>

---

## Architecture

The architecture is the same as upstream:

| Component | Role |
| :--- | :--- |
| **`omen-space-daemon`** | Root `systemd` service: WMI, ACPI, sysfs, EC, power and HID over D-Bus (`org.hp.omen.*`). |
| **`omen-gui`** | GTK4 + Libadwaita desktop application. |
| **`omen-tray`** | Tray icon: quick fan/GPU modes, panel refresh rate per profile, low-brightness dimming. |
| **`omen-overlay`** | Quick Access HUD (`Shift+F2`). |
| **`omen-cli`** | Scriptable command-line interface. |
| **`hp-omen-extra`** | DKMS kernel driver package (`hp-wmi` + `hp-omen-extra`). |

---

## Credits & License

All credit for OMEN Space goes to its authors. This fork only adapts it to one board.

- **[yunusemreyl](https://github.com/yunusemreyl)**: OMEN Space lead developer
- **[tuxov](https://github.com/tuxov)**: kernel module lead
- The OMEN Space community contributors (see [upstream](https://github.com/yunusemreyl/omen-space#credits--license))
- **[pedrovb25](https://github.com/pedrovb25)**: 8C75 fork, developed with the help of **Claude Opus 5.5** (Anthropic) for diagnosing the problems and reviewing the code

Licensed under the **GPL-3.0 License**, like the original project.

*Disclaimer: OMEN Space is an independent open-source project and is not affiliated with or endorsed by HP Inc.*
