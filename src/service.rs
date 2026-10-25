use anyhow::{Context, Result};
use std::process::Command;

use crate::ServiceAction;

const TASK: &str = "PhoneSync";

pub fn run(action: ServiceAction) -> Result<()> {
    #[cfg(windows)]
    {
        match action {
            ServiceAction::Install => install(),
            ServiceAction::Uninstall => uninstall(),
            ServiceAction::Status => status(),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = action;
        println!("The autostart helper only targets Windows.");
        println!("On Linux use a systemd user unit running: phonesync serve");
        println!("On Termux: termux-services or `nohup phonesync serve &`");
        Ok(())
    }
}

#[cfg(windows)]
fn install() -> Result<()> {
    let exe = std::env::current_exe().context("locating phonesync.exe")?;
    let quoted = format!("\"{}\" serve --hide-console", exe.display());
    let status = Command::new("schtasks")
        .args([
            "/Create", "/F", "/SC", "ONLOGON", "/TN", TASK, "/TR", &quoted, "/RL", "LIMITED",
        ])
        .status()
        .context("running schtasks")?;
    if !status.success() {
        anyhow::bail!("schtasks failed — try running this from a normal (non-admin) prompt");
    }
    println!("installed: PhoneSync starts with Windows (task “{TASK}”)");
    println!("the receiver listens on port 8787 in the background");
    println!("remove it later with: phonesync service uninstall");
    Ok(())
}

#[cfg(windows)]
fn uninstall() -> Result<()> {
    let status = Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", TASK])
        .status()
        .context("running schtasks")?;
    if status.success() {
        println!("removed the autostart task");
    } else {
        println!("no task named “{TASK}” was installed");
    }
    Ok(())
}

#[cfg(windows)]
fn status() -> Result<()> {
    let output = Command::new("schtasks")
        .args(["/Query", "/TN", TASK, "/FO", "LIST"])
        .output()
        .context("running schtasks")?;
    if output.status.success() {
        println!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        println!("PhoneSync is not registered to start with Windows.");
        println!("run: phonesync service install");
    }
    Ok(())
}

pub fn firewall(port: u16) -> Result<()> {
    #[cfg(windows)]
    {
        let rule = format!("localport={port}");
        let status = Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "add",
                "rule",
                "name=PhoneSync",
                "dir=in",
                "action=allow",
                "protocol=TCP",
                &rule,
                "profile=private,domain",
            ])
            .status()
            .context("running netsh")?;
        if !status.success() {
            anyhow::bail!("netsh failed — open PowerShell as Administrator and try again");
        }
        println!("Windows Firewall now allows PhoneSync on TCP {port} (private networks).");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        println!("No firewall step needed on this OS for port {port}.");
        Ok(())
    }
}
