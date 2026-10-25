use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};

const INDEX: &str = include_str!("ui/index.html");
const DASHBOARD: &str = include_str!("ui/dashboard.html");
const CSS: &str = include_str!("ui/app.css");
const CORE: &str = include_str!("ui/core.js");
const PHONE: &str = include_str!("ui/phone.js");
const DASH: &str = include_str!("ui/dash.js");
const MANIFEST: &str = include_str!("ui/manifest.webmanifest");
const SW: &str = include_str!("ui/sw.js");
const ICON: &str = include_str!("ui/icon.svg");

pub async fn index() -> Html<&'static str> {
    Html(INDEX)
}

pub async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD)
}

pub async fn asset(Path(file): Path<String>) -> Response {
    let (body, mime) = match file.as_str() {
        "app.css" => (CSS, "text/css; charset=utf-8"),
        "core.js" => (CORE, "application/javascript; charset=utf-8"),
        "phone.js" => (PHONE, "application/javascript; charset=utf-8"),
        "dash.js" => (DASH, "application/javascript; charset=utf-8"),
        "manifest.webmanifest" => (MANIFEST, "application/manifest+json"),
        "sw.js" => (SW, "application/javascript; charset=utf-8"),
        "icon.svg" => (ICON, "image/svg+xml"),
        _ => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

/// SVG QR code for the phone URL, so the dashboard can show "scan to connect".
pub fn qr_svg(data: &str) -> Option<String> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    let image = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(180, 180)
        .dark_color(qrcode::render::svg::Color("#0a0a0c"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    Some(image)
}
