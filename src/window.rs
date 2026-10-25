//! Desktop shell: a real window around the dashboard instead of a browser tab.
//!
//! The receiver keeps running on a background thread and the window is just a
//! WebView2 view of `127.0.0.1`. Closing the window exits the process, which
//! also stops the server.

use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, Theme, WindowBuilder};
use wry::WebViewBuilder;

use crate::config::Config;
use crate::ServeArgs;

/// `phonesync app` — start the receiver and show it as a native window.
pub fn run(cfg: Config, args: ServeArgs) -> Result<()> {
    let port = args.port;
    let title = format!(
        "PhoneSync — {}",
        args.name.clone().unwrap_or_else(|| cfg.device_name.clone())
    );
    let token = cfg.token.clone();

    let serve_cfg = cfg.clone();
    let serve_args = ServeArgs {
        port,
        dashboard: false,
        no_qr: true,
        ..args
    };
    // A second launch should just focus another window on the running receiver
    // instead of failing with "port in use".
    if !port_open(port) {
        std::thread::Builder::new()
            .name("phonesync-window-host".into())
            .spawn(move || {
                if let Err(err) = crate::server::run(serve_cfg, serve_args) {
                    eprintln!("receiver stopped: {err:#}");
                }
            })
            .context("starting the receiver thread")?;

        wait_for_port(port, Duration::from_secs(10))
            .context("the receiver did not start listening in time")?;
    }

    let url = if token.is_empty() {
        format!("http://127.0.0.1:{port}/dashboard?app=1")
    } else {
        format!("http://127.0.0.1:{port}/dashboard?app=1&k={token}")
    };
    show_window(&title, &url)
}

/// Poll the loopback port until the server answers, so the window never opens
/// on a connection error page.
fn wait_for_port(port: u16, limit: Duration) -> Result<()> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if port_open(port) {
            // Give axum a moment to finish binding its routes.
            std::thread::sleep(Duration::from_millis(120));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    anyhow::bail!("port {port} never became reachable")
}

fn port_open(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

enum UserEvent {
    /// The user picked a folder in the native dialog.
    Folder(PathBuf),
}

fn show_window(title: &str, url: &str) -> Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(LogicalSize::new(1320.0, 860.0))
        .with_min_inner_size(LogicalSize::new(960.0, 620.0))
        .with_window_icon(Some(app_icon()))
        .with_theme(Some(Theme::Dark))
        .build(&event_loop)
        .context("creating the app window")?;

    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_background_color((10, 10, 11, 255))
        .with_ipc_handler(move |req| {
            if req.body() == "pick-folder" {
                spawn_folder_picker(proxy.clone());
            }
        })
        .build(&window)
        .context("starting the embedded webview (is the WebView2 runtime installed?)")?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested | WindowEvent::Destroyed,
                ..
            } => {
                // Closing the window is closing the app: stop the receiver too.
                std::process::exit(0);
            }
            Event::UserEvent(UserEvent::Folder(dir)) => {
                let js = format!(
                    "window.__psPicked && window.__psPicked({})",
                    serde_json::to_string(&dir.display().to_string()).unwrap_or_default()
                );
                let _ = webview.evaluate_script(&js);
            }
            _ => {}
        }
    });
}

/// The dialog blocks its thread, so it runs off the event loop and reports back
/// through a proxy; the webview is only touched on the main thread.
fn spawn_folder_picker(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Choose where PhoneSync saves files")
            .pick_folder();
        if let Some(dir) = picked {
            let _ = proxy.send_event(UserEvent::Folder(dir));
        }
    });
}

/// 64×64 lime bolt on a near-black tile, drawn here so the binary carries no
/// image assets.
fn app_icon() -> Icon {
    const S: usize = 64;
    let bolt = [
        (0.60, 0.06),
        (0.26, 0.56),
        (0.46, 0.56),
        (0.40, 0.94),
        (0.74, 0.42),
        (0.53, 0.42),
    ];
    let mut rgba = vec![0u8; S * S * 4];
    for y in 0..S {
        for x in 0..S {
            let fx = (x as f32 + 0.5) / S as f32;
            let fy = (y as f32 + 0.5) / S as f32;
            let i = (y * S + x) * 4;
            if !inside_rounded_tile(fx, fy, 0.16) {
                continue;
            }
            let on_bolt = point_in_polygon(fx, fy, &bolt);
            let (r, g, b) = if on_bolt {
                (205, 245, 100)
            } else {
                (14, 14, 16)
            };
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
    }
    Icon::from_rgba(rgba, S as u32, S as u32).expect("icon data is 64×64 RGBA")
}

fn inside_rounded_tile(x: f32, y: f32, radius: f32) -> bool {
    let cx = x.clamp(radius, 1.0 - radius);
    let cy = y.clamp(radius, 1.0 - radius);
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= radius * radius
}

fn point_in_polygon(x: f32, y: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}
