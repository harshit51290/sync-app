/* Shared helpers for both surfaces. No build step, no dependencies. */

const PS = (() => {
  const ICONS = {
    bolt: '<path d="M13 2 4.5 13.5H11l-1 8.5 8.5-11.5H12l1-8.5Z"/>',
    phone:
      '<rect x="6" y="2.5" width="12" height="19" rx="3"/><path d="M10.5 18.5h3"/>',
    laptop:
      '<path d="M4 5.5h16v10H4z"/><path d="M2 19h20"/><path d="M9.5 15.5h5"/>',
    image:
      '<rect x="3" y="4" width="18" height="16" rx="2.5"/><circle cx="9" cy="10" r="1.6"/><path d="m4 18 5-5 4 4 3-3 4 4"/>',
    video:
      '<rect x="2.5" y="5.5" width="14" height="13" rx="2.5"/><path d="m17 10 4.5-3v10L17 14z"/>',
    folder:
      '<path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h6a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    file: '<path d="M6 2.5h7l5 5V21.5H6z"/><path d="M13 2.5v5h5"/>',
    upload: '<path d="M12 16V4"/><path d="m7 9 5-5 5 5"/><path d="M4 16v3.5a1.5 1.5 0 0 0 1.5 1.5h13a1.5 1.5 0 0 0 1.5-1.5V16"/>',
    check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
    alert: '<path d="M12 3 2.5 20h19z"/><path d="M12 9v4.5"/><path d="M12 17h.01"/>',
    clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7.5V12l3 2"/>',
    search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.9-3.9"/>',
    refresh:
      '<path d="M20 12a8 8 0 1 1-2.4-5.7"/><path d="M20 4v4.5h-4.5"/>',
    external:
      '<path d="M14 4h6v6"/><path d="M20 4 11 13"/><path d="M18 14v5.5a1.5 1.5 0 0 1-1.5 1.5H5.5A1.5 1.5 0 0 1 4 19.5V8.5A1.5 1.5 0 0 1 5.5 7H11"/>',
    settings:
      '<circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.2 5.2l2.1 2.1M16.7 16.7l2.1 2.1M18.8 5.2l-2.1 2.1M7.3 16.7l-2.1 2.1"/>',
    grid: '<rect x="3" y="3" width="7" height="7" rx="1.6"/><rect x="14" y="3" width="7" height="7" rx="1.6"/><rect x="3" y="14" width="7" height="7" rx="1.6"/><rect x="14" y="14" width="7" height="7" rx="1.6"/>',
    drive:
      '<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><path d="M2.5 15h19"/><path d="M6 18h.01"/>',
    chev: '<path d="m6 9 6 6 6-6"/>',
    x: '<path d="M6 6l12 12M18 6 6 18"/>',
    copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M15 5.5A1.5 1.5 0 0 0 13.5 4h-8A1.5 1.5 0 0 0 4 5.5v8A1.5 1.5 0 0 0 5.5 15"/>',
    pause: '<rect x="7" y="5" width="3.5" height="14" rx="1"/><rect x="13.5" y="5" width="3.5" height="14" rx="1"/>',
    play: '<path d="M8 5.5v13l10-6.5z"/>',
    arrow: '<path d="M5 12h14"/><path d="m13 6 6 6-6 6"/>',
    wifi: '<path d="M2.5 8.5a14 14 0 0 1 19 0"/><path d="M6 12a9 9 0 0 1 12 0"/><path d="M9.5 15.5a4 4 0 0 1 5 0"/><path d="M12 19h.01"/>',
    shield: '<path d="M12 3 5 6v6c0 4.2 2.9 7.6 7 9 4.1-1.4 7-4.8 7-9V6z"/>',
    spark: '<path d="M12 3v4M12 17v4M3 12h4M17 12h4M6.3 6.3l2.8 2.8M14.9 14.9l2.8 2.8M17.7 6.3l-2.8 2.8M9.1 14.9l-2.8 2.8"/>',
    inbox: '<path d="M3 12h5l2 3h4l2-3h5"/><path d="M4.5 5h15l1.5 7v5.5a1.5 1.5 0 0 1-1.5 1.5H4.5A1.5 1.5 0 0 1 3 17.5V12z"/>',
  };

  const icon = (name, cls = "ic") =>
    `<svg class="${cls}" viewBox="0 0 24 24" aria-hidden="true">${
      ICONS[name] || ICONS.file
    }</svg>`;

  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

  const bytes = (n) => {
    if (!n) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = n;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${i === 0 ? v : v.toFixed(v >= 10 ? 0 : 1)} ${units[i]}`;
  };

  const ago = (ms) => {
    if (!ms) return "never";
    const diff = Date.now() - ms;
    if (diff < 45_000) return "just now";
    const mins = Math.round(diff / 60_000);
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.round(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.round(hours / 24);
    if (days < 30) return `${days}d ago`;
    return new Date(ms).toLocaleDateString();
  };

  const clock = (ms) =>
    new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

  const extOf = (name) => (name.split(".").pop() || "").toLowerCase();

  const kindOf = (name) => {
    const ext = extOf(name);
    if (["jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "bmp", "avif"].includes(ext))
      return ["img", "image"];
    if (["mp4", "mov", "mkv", "3gp", "avi", "webm", "m4v", "ts"].includes(ext))
      return ["vid", "video"];
    if (["mp3", "m4a", "opus", "wav", "flac", "aac", "ogg", "amr"].includes(ext))
      return ["aud", "file"];
    if (["pdf", "doc", "docx", "txt", "md", "xlsx", "csv", "pptx"].includes(ext))
      return ["doc", "file"];
    if (["apk", "zip", "7z", "rar", "tar", "gz"].includes(ext)) return ["pkg", "file"];
    return ["", "file"];
  };

  let key = (() => {
    const url = new URL(location.href);
    const fromUrl = url.searchParams.get("k");
    if (fromUrl) {
      localStorage.setItem("phonesync.key", fromUrl);
      // Drop only the secret: `?app=1` tells the dashboard it is inside the
      // desktop window, and losing it hides the native folder picker.
      url.searchParams.delete("k");
      const rest = url.searchParams.toString();
      history.replaceState({}, "", url.pathname + (rest ? `?${rest}` : "") + url.hash);
      return fromUrl;
    }
    return localStorage.getItem("phonesync.key") || "";
  })();

  const setKey = (value) => {
    key = value || "";
    if (key) localStorage.setItem("phonesync.key", key);
    else localStorage.removeItem("phonesync.key");
  };

  async function api(path, opts = {}) {
    const headers = { "x-sync-key": key, ...(opts.headers || {}) };
    let body = opts.body;
    if (opts.json !== undefined) {
      headers["content-type"] = "application/json";
      body = JSON.stringify(opts.json);
    }
    const res = await fetch(path, {
      method: opts.method || "GET",
      headers,
      body,
      signal: opts.signal,
    });
    if (!res.ok) {
      let message = res.statusText;
      try {
        const data = await res.json();
        message = data.error || message;
      } catch {
        /* not json */
      }
      const err = new Error(message);
      err.status = res.status;
      throw err;
    }
    return res.json();
  }

  function toast(message, kind = "ok") {
    const host = document.getElementById("toasts");
    if (!host) return;
    const el = document.createElement("div");
    el.className = "toast";
    const color = kind === "bad" ? "bad" : kind === "warn" ? "warn" : "ok";
    el.innerHTML = `<span class="dot ${color}"></span><span>${escape(message)}</span>`;
    host.appendChild(el);
    setTimeout(() => {
      el.classList.add("out");
      setTimeout(() => el.remove(), 260);
    }, 3200);
  }

  const escape = (s) =>
    String(s ?? "").replace(
      /[&<>"']/g,
      (c) =>
        ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
          c
        ]
    );

  const deviceName = () => {
    const custom = localStorage.getItem("phonesync.device");
    if (custom) return custom;
    const ua = navigator.userAgent;
    if (/iPhone/.test(ua)) return "iPhone";
    if (/iPad/.test(ua)) return "iPad";
    const android = ua.match(/Android[^;]*;\s*([^)]+?)(?:\s+Build|\))/);
    if (android) return android[1].replace(/\s+/g, " ").trim();
    if (/Android/.test(ua)) return "Android phone";
    return "Phone";
  };

  const platform = () =>
    /Android/.test(navigator.userAgent)
      ? "android"
      : /iPhone|iPad|iPod/.test(navigator.userAgent)
        ? "ios"
        : "web";

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  const hydrateIcons = (root = document) => {
    root.querySelectorAll("svg[data-icon]").forEach((el) => {
      const name = el.getAttribute("data-icon");
      const holder = document.createElement("div");
      holder.innerHTML = icon(name, el.getAttribute("class") || "ic");
      const fresh = holder.firstElementChild;
      Object.keys(el.dataset).forEach((k) => (fresh.dataset[k] = el.dataset[k]));
      el.replaceWith(fresh);
    });
  };

  document.addEventListener("DOMContentLoaded", () => hydrateIcons());

  return {
    icon,
    hydrateIcons,
    $,
    $$,
    bytes,
    ago,
    clock,
    kindOf,
    extOf,
    api,
    toast,
    escape,
    deviceName,
    platform,
    sleep,
    setKey,
    get key() {
      return key;
    },
    ICONS,
  };
})();
