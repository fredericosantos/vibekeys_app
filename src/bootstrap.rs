//! `vibekeys bootstrap` — auto-configures the local + SSH-remote workflow.
//!
//! Detection signals:
//!   1. BLE adapter present?         (btleplug enumerates one)
//!   2. `$SSH_CONNECTION` set?       (we are inside an sshd-spawned shell)
//!   3. Bridge tunnel reachable?     (TCP probe to 127.0.0.1:<port>)
//!
//! From these we pick a `Mode`, persist it to `~/.config/vibekeys/config.json`,
//! and apply the side effects for that mode (launchd plist + load, or shell rc
//! export, or both). Idempotent — safe to re-run.

use clap::ValueEnum;
use log::info;
use std::env;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_BRIDGE: &str = "http://127.0.0.1:7777";
const DEFAULT_BIND: &str = "127.0.0.1:7777";
const LAUNCHD_LABEL: &str = "sh.secondstate.vibekeys";

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    /// Single-machine: Claude Code and the keyboard are on this laptop. BLE direct.
    Local,
    /// This is the laptop and we want the bridge running for SSH'd Claude Code sessions.
    SshLaptop,
    /// This is a remote SSH host with no BLE; hooks forward to the laptop's bridge.
    SshRemote,
}

pub async fn run(forced: Option<Mode>, dry_run: bool) -> anyhow::Result<()> {
    let detected = detect().await;
    let mode = forced.unwrap_or(detected.suggested);

    println!("VibeKeys bootstrap");
    println!("  detected: BLE={}, SSH={}, bridge={}", yn(detected.ble), yn(detected.ssh), yn(detected.bridge_reachable));
    println!("  mode:     {:?}{}", mode, if forced.is_some() { " (forced)" } else { " (auto)" });
    if dry_run {
        println!("  (dry run — no changes will be made)");
    }

    write_config(mode, dry_run)?;

    match mode {
        Mode::Local => {
            println!("\nLocal mode — no service needed. `vibekeys hook` will talk to the keyboard directly.");
        }
        Mode::SshLaptop => {
            install_bridge_service(dry_run)?;
            println!("\nLaptop bridge installed.");
            println!("On any remote you SSH into, add this to ~/.ssh/config (on this laptop):");
            println!();
            println!("  Host <your-remote>");
            println!("      RemoteForward 7777 127.0.0.1:7777");
            println!();
            println!("Then on the remote, run `vibekeys bootstrap --mode ssh-remote` (or just `vibekeys bootstrap`).");
        }
        Mode::SshRemote => {
            configure_remote_env(dry_run)?;
            println!("\nRemote mode configured.");
            println!("Open a new shell (or `source ~/.zshrc`) so $VIBEKEYS_REMOTE is set,");
            println!("then verify the tunnel: `curl -s {DEFAULT_BRIDGE}` should print `ok`.");
            println!("If it doesn't, add `RemoteForward 7777 127.0.0.1:7777` on the laptop side and reconnect.");
        }
    }

    Ok(())
}

struct Detected {
    ble: bool,
    ssh: bool,
    bridge_reachable: bool,
    suggested: Mode,
}

async fn detect() -> Detected {
    let ble = probe_ble().await;
    let ssh = env::var("SSH_CONNECTION").is_ok();
    let bridge_reachable = probe_tcp(DEFAULT_BIND, Duration::from_millis(150));

    let suggested = match (ble, ssh) {
        (true, false) => Mode::Local,
        (true, true) => Mode::Local, // BLE wins; user's right here
        (false, true) => Mode::SshRemote,
        (false, false) => Mode::Local, // best guess; user can override
    };

    Detected { ble, ssh, bridge_reachable, suggested }
}

async fn probe_ble() -> bool {
    use btleplug::api::Manager as _;
    use tokio::time::timeout;

    let fut = async {
        let m = btleplug::platform::Manager::new().await.ok()?;
        let adapters = m.adapters().await.ok()?;
        Some(!adapters.is_empty())
    };
    matches!(timeout(Duration::from_millis(800), fut).await, Ok(Some(true)))
}

fn probe_tcp(addr: &str, timeout: Duration) -> bool {
    let parsed: Result<SocketAddr, _> = addr.parse();
    let Ok(sa) = parsed else { return false };
    TcpStream::connect_timeout(&sa, timeout).is_ok()
}

fn config_dir() -> anyhow::Result<PathBuf> {
    let home = env::var("HOME").map_err(|_| anyhow::anyhow!("$HOME not set"))?;
    Ok(PathBuf::from(home).join(".config").join("vibekeys"))
}

fn write_config(mode: Mode, dry_run: bool) -> anyhow::Result<()> {
    let dir = config_dir()?;
    let path = dir.join("config.json");
    let mode_str = match mode {
        Mode::Local => "local",
        Mode::SshLaptop => "ssh-laptop",
        Mode::SshRemote => "ssh-remote",
    };
    let body = serde_json::json!({
        "mode": mode_str,
        "bridge_url": DEFAULT_BRIDGE,
    });
    let pretty = serde_json::to_string_pretty(&body)?;

    println!("\nconfig: {}", path.display());
    if dry_run {
        println!("---\n{pretty}\n---");
        return Ok(());
    }
    fs::create_dir_all(&dir)?;
    fs::write(&path, pretty)?;
    Ok(())
}

fn install_bridge_service(dry_run: bool) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        install_launchd(dry_run)
    } else if cfg!(target_os = "linux") {
        install_systemd_user(dry_run)
    } else {
        println!("Bridge service auto-install not supported on this OS — run `vibekeys serve` manually.");
        Ok(())
    }
}

fn install_launchd(dry_run: bool) -> anyhow::Result<()> {
    let exe = env::current_exe()?;
    let home = env::var("HOME").map_err(|_| anyhow::anyhow!("$HOME not set"))?;
    let plist_dir = PathBuf::from(&home).join("Library/LaunchAgents");
    let plist_path = plist_dir.join(format!("{LAUNCHD_LABEL}.plist"));

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LAUNCHD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>serve</string>
        <string>--bind</string>
        <string>{DEFAULT_BIND}</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
    <key>StandardOutPath</key><string>/tmp/vibekeys.out.log</string>
    <key>StandardErrorPath</key><string>/tmp/vibekeys.err.log</string>
</dict>
</plist>
"#,
        exe = exe.display(),
    );

    println!("launchd plist: {}", plist_path.display());
    if dry_run {
        println!("---\n{plist}---");
        return Ok(());
    }
    fs::create_dir_all(&plist_dir)?;
    fs::write(&plist_path, plist)?;
    // Reload (unload-then-load is idempotent and works whether or not it was loaded before).
    let _ = std::process::Command::new("launchctl").args(["unload", plist_path.to_str().unwrap()]).status();
    let status = std::process::Command::new("launchctl").args(["load", plist_path.to_str().unwrap()]).status()?;
    if !status.success() {
        anyhow::bail!("launchctl load failed");
    }
    info!("launchd agent loaded");
    Ok(())
}

fn install_systemd_user(dry_run: bool) -> anyhow::Result<()> {
    let exe = env::current_exe()?;
    let home = env::var("HOME").map_err(|_| anyhow::anyhow!("$HOME not set"))?;
    let unit_dir = PathBuf::from(&home).join(".config/systemd/user");
    let unit_path = unit_dir.join("vibekeys.service");
    let unit = format!(
        "[Unit]\nDescription=VibeKeys local BLE bridge\n\n[Service]\nExecStart={} serve --bind {}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
        exe.display(),
        DEFAULT_BIND,
    );

    println!("systemd unit: {}", unit_path.display());
    if dry_run {
        println!("---\n{unit}---");
        return Ok(());
    }
    fs::create_dir_all(&unit_dir)?;
    fs::write(&unit_path, unit)?;
    let _ = std::process::Command::new("systemctl").args(["--user", "daemon-reload"]).status();
    let _ = std::process::Command::new("systemctl").args(["--user", "enable", "--now", "vibekeys.service"]).status();
    info!("systemd user service enabled");
    Ok(())
}

fn configure_remote_env(dry_run: bool) -> anyhow::Result<()> {
    let home = env::var("HOME").map_err(|_| anyhow::anyhow!("$HOME not set"))?;
    let shell = env::var("SHELL").unwrap_or_default();
    let rc = if shell.ends_with("zsh") {
        Path::new(&home).join(".zshrc")
    } else {
        Path::new(&home).join(".bashrc")
    };

    let line = format!("export VIBEKEYS_REMOTE={DEFAULT_BRIDGE}\n");
    let marker = "# vibekeys: forward Claude Code hooks to laptop bridge\n";
    let block = format!("\n{marker}{line}");

    println!("shell rc: {}", rc.display());
    if dry_run {
        println!("would append:\n{block}");
        return Ok(());
    }

    let existing = fs::read_to_string(&rc).unwrap_or_default();
    if existing.contains("VIBEKEYS_REMOTE") {
        info!("VIBEKEYS_REMOTE already present in {}, leaving as-is", rc.display());
        return Ok(());
    }
    let mut f = fs::OpenOptions::new().create(true).append(true).open(&rc)?;
    f.write_all(block.as_bytes())?;
    Ok(())
}

fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}
