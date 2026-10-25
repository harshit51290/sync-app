use std::time::{Duration, Instant};

use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent};

pub const SERVICE: &str = "_phonesync._tcp.local.";

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Found {
    pub name: String,
    pub host: String,
    pub ip: String,
    pub port: u16,
}

impl Found {
    pub fn url(&self) -> String {
        if self.ip.contains(':') {
            format!("http://[{}]:{}", self.ip, self.port)
        } else {
            format!("http://{}:{}", self.ip, self.port)
        }
    }
}

pub fn browse(seconds: u64, want: Option<&str>) -> Result<Vec<Found>> {
    let daemon = ServiceDaemon::new()?;
    let receiver = daemon.browse(SERVICE)?;
    let deadline = Instant::now() + Duration::from_secs(seconds.max(1));
    let mut found: Vec<Found> = Vec::new();

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let ip = info
                    .get_addresses()
                    .iter()
                    .find(|ip| ip.is_ipv4())
                    .or_else(|| info.get_addresses().iter().next())
                    .map(|ip| ip.to_string());
                let Some(ip) = ip else { continue };
                let name = info
                    .get_property_val_str("name")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| info.get_fullname().to_string());
                let entry = Found {
                    name: name.clone(),
                    host: info.get_hostname().to_string(),
                    ip,
                    port: info.get_port(),
                };
                let matches = want
                    .map(|w| {
                        let w = w.to_ascii_lowercase();
                        name.to_ascii_lowercase().contains(&w)
                            || info.get_fullname().to_ascii_lowercase().contains(&w)
                    })
                    .unwrap_or(true);
                if matches && !found.iter().any(|f| f.name == entry.name) {
                    found.push(entry);
                    if want.is_some() {
                        break;
                    }
                }
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }

    let _ = daemon.shutdown();
    Ok(found)
}

pub fn run(seconds: u64) -> Result<()> {
    println!("listening for PhoneSync receivers ({seconds}s)…");
    let found = browse(seconds, None)?;
    if found.is_empty() {
        println!("\nnothing found.");
        println!("• is `phonesync serve` running on the laptop?");
        println!("• are both devices on the same Wi-Fi (not a guest network)?");
        println!("• Windows may block mDNS — pass --to http://<laptop-ip>:8787 instead");
        return Ok(());
    }
    println!("\n{:<28} {:<10} {}", "NAME", "ADDRESS", "PORT");
    for entry in &found {
        println!("{:<28} {:<10} {}", entry.name, entry.ip, entry.port);
    }
    println!("\nuse: phonesync send <path> --to {}", found[0].url());
    Ok(())
}
