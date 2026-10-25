/* Laptop dashboard: overview, library, devices, settings. */

const { $, $$, icon, bytes, ago, clock, api, toast, escape } = PS;

const ui = {
  info: null,
  stats: null,
  devices: [],
  files: [],
  view: "overview",
  query: "",
};

const VIEW_TITLES = {
  overview: "Overview",
  files: "Files",
  devices: "Devices",
  settings: "Settings",
};

/* --------------------------------------------------------------------- boot */

async function boot() {
  wire();
  await Promise.all([loadInfo(), loadStats(), loadSettings()]);
  loadFiles();
  loadActivity();
  connectStream();
  setInterval(async () => {
    await loadStats();
    if (ui.view === "devices") loadDevices();
  }, 15_000);
  setInterval(() => {
    if (ui.info) paintUptime();
    $$(".act-time").forEach((el) => {
      const at = Number(el.dataset.at);
      if (at) el.textContent = clock(at);
    });
  }, 30_000);
}

async function loadInfo() {
  ui.info = await api("/api/info");
  $("#rail-version").textContent = `v${ui.info.version}`;
  const qs = new URLSearchParams();
  if (PS.key) qs.set("k", PS.key);
  qs.set("t", Date.now());
  $("#qr-img").src = `/api/qr.svg?${qs}`;
  $("#qr-img-2").src = `/api/qr.svg?${qs}`;
  const clean = ui.info.sync_url;
  $("#url-text").textContent = clean.replace(/^https?:\/\//, "");
  $("#url-text-2").textContent = clean.replace(/^https?:\/\//, "");
  $("#kv-port").textContent = `:${ui.info.port}`;
  $("#kv-version").textContent = `v${ui.info.version} · ${ui.info.os}`;
  $("#install-cmd").textContent = "phonesync service install";
  $("#root-path").textContent = ui.info.root;
  $("#set-root").value = ui.info.root;
  paintUptime();
}

function paintUptime() {
  if (!ui.info) return;
  const ms = Date.now() - ui.info.started;
  const mins = Math.floor(ms / 60_000);
  const hours = Math.floor(mins / 60);
  const days = Math.floor(hours / 24);
  const text =
    days > 0
      ? `${days}d ${hours % 24}h`
      : hours > 0
        ? `${hours}h ${mins % 60}m`
        : `${mins}m`;
  $("#rail-uptime").textContent = `uptime ${text}`;
}

/* -------------------------------------------------------------------- stats */

async function loadStats() {
  try {
    const s = await api("/api/stats");
    ui.stats = s;
    ui.devices = s.devices || [];
    paintTiles(s);
    paintChart(s);
    paintDevices(s.devices || []);
    $("#count-files").textContent = s.files;
    $("#count-devices").textContent = (s.devices || []).length;
    $("#disk-used").textContent = `${s.human_total} used`;
    $("#disk-free").textContent = `${s.human_free} free`;
    const total = s.bytes + (s.free_space || 0);
    $("#disk-bar").style.width = `${total ? Math.max(1, (s.bytes / total) * 100) : 1}%`;
    $("#kv-root-free").textContent = s.free_space
      ? `${s.human_free} free on this drive`
      : "unknown";
    const paused = s.paused;
    $("#rail-dot").className = `dot ${paused ? "warn" : "live"}`;
    $("#rail-status").textContent = paused ? "paused" : "listening";
    $("#sw-pause").setAttribute("aria-checked", String(!!paused));
  } catch (err) {
    $("#rail-dot").className = "dot bad";
    $("#rail-status").textContent = err.status === 401 ? "locked" : "offline";
  }
}

function paintTiles(s) {
  $("#tile-total").textContent = s.human_total;
  $("#tile-total-sub").textContent = `${s.files} file${s.files === 1 ? "" : "s"}`;
  $("#tile-today").textContent = s.today_files;
  $("#tile-today-sub").textContent = `files · ${bytes(s.today_bytes)}`;
  $("#tile-free").textContent = s.human_free;
  $("#tile-free-sub").textContent = s.free_space
    ? `${bytes(s.free_space)} available`
    : "unknown";
  $("#tile-devices").textContent = (s.devices || []).length;
  $("#tile-devices-sub").textContent = s.devices?.length
    ? `last: ${ago(Math.max(...s.devices.map((d) => d.last_seen)))}`
    : "waiting for a phone";
}

function paintChart(s) {
  const days = s.days || [];
  const max = Math.max(1, ...days.map((d) => d.bytes));
  const host = $("#chart");
  const today = new Date().toISOString().slice(0, 10);
  host.innerHTML = days
    .map((d) => {
      const pct = Math.max(2, (d.bytes / max) * 100);
      const label = d.day.slice(8).replace(/^0/, "");
      const isToday = d.day === today;
      return `<div class="col${isToday ? " today" : ""}" title="${d.day} · ${bytes(
        d.bytes
      )} · ${d.files} files">
        <i style="height:${pct}%"></i><span>${label}</span>
      </div>`;
    })
    .join("");
  const total = days.reduce((a, d) => a + d.bytes, 0);
  const peak = days.reduce((a, d) => Math.max(a, d.bytes), 0);
  $("#chart-chip").textContent = bytes(total);
  $("#chart-peak").textContent = bytes(peak);
  $("#chart-avg").textContent = bytes(Math.round(total / Math.max(1, days.length)));
}

function paintDevices(devices) {
  const host = $("#device-list");
  $("#device-chip").textContent = devices.length;
  if (!devices.length) {
    host.innerHTML = `<div class="empty">${icon(
      "phone"
    )}No phone has connected yet</div>`;
    return;
  }
  host.innerHTML = devices
    .slice(0, 4)
    .map(
      (d) => `<div class="device">
        <span class="dev-ico">${icon("phone")}</span>
        <div style="min-width: 0; flex: 1">
          <div class="dev-name">${escape(d.name)}</div>
          <div class="dev-meta">${bytes(d.bytes)} · ${d.files} files · ${ago(
            d.last_seen
          )}</div>
        </div>
        <span class="dot ${
          Date.now() - d.last_seen < 120_000 ? "live" : ""
        }"></span>
      </div>`
    )
    .join("");
}

/* -------------------------------------------------------------------- files */

let filesTimer;
function scheduleFiles() {
  clearTimeout(filesTimer);
  filesTimer = setTimeout(loadFiles, 220);
}

async function loadFiles() {
  try {
    const data = await api(
      `/api/files?limit=300&q=${encodeURIComponent(ui.query)}`
    );
    ui.files = data.files;
    const body = $("#files-body");
    $("#files-count").textContent = `${data.files.length} files`;
    if (!data.files.length) {
      body.innerHTML = `<tr><td colspan="5"><div class="empty">${icon(
        "inbox"
      )}${ui.query ? "Nothing matches that search" : "No files yet — connect a phone"}</div></td></tr>`;
      return;
    }
    const token = PS.key ? `?k=${encodeURIComponent(PS.key)}` : "";
    body.innerHTML = data.files
      .map((f) => {
        const [tag, ico] = PS.kindOf(f.name);
        const preview =
          tag === "img" && f.size < 24 * 1024 * 1024
            ? `<img alt="" loading="lazy" src="/api/raw/${f.id}${token}" style="width:100%;height:100%;object-fit:cover" />`
            : icon(ico);
        return `<tr>
          <td>
            <div class="cell-name">
              <span class="thumb">${preview}</span>
              <span class="nm" title="${escape(f.rel)}">${escape(f.name)}</span>
            </div>
          </td>
          <td class="muted">${escape(f.device)}</td>
          <td class="mono faint">${bytes(f.size)}</td>
          <td class="mono faint">${ago(f.received)}</td>
          <td class="right">
            <button class="btn quiet" data-reveal="${f.id}" title="Show in Explorer">
              <svg class="ic" data-icon="external"></svg>
            </button>
          </td>
        </tr>`;
      })
      .join("");
    PS.hydrateIcons(body);
    body.querySelectorAll("[data-reveal]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        try {
          await api("/api/actions/reveal", {
            method: "POST",
            json: { id: btn.dataset.reveal },
          });
        } catch (err) {
          toast(err.message, "bad");
        }
      });
    });
  } catch (err) {
    toast(err.message, "bad");
  }
}

async function loadDevices() {
  try {
    const data = await api("/api/devices");
    ui.devices = data.devices;
    paintDevices(data.devices);
    renderDeviceCards(data.devices);
  } catch {
    /* ignore */
  }
}

function renderDeviceCards(devices) {
  const host = $("#device-cards");
  if (!devices.length) {
    host.innerHTML = `<div class="panel"><div class="empty">${icon(
      "phone"
    )}No devices yet.<br />Scan the QR code on the Overview tab.</div></div>`;
    return;
  }
  host.innerHTML = devices
    .map(
      (d) => `<div class="panel device">
        <span class="dev-ico">${icon("phone")}</span>
        <div style="min-width: 0; flex: 1">
          <div class="dev-name">${escape(d.name)}</div>
          <div class="dev-meta">${escape(d.platform || "unknown")} · ${escape(
            d.ip || "—"
          )}</div>
          <div class="dev-meta" style="margin-top:6px">${bytes(d.bytes)} · ${
            d.files
          } files</div>
        </div>
        <div class="dev-meta" style="text-align:right">${ago(d.last_seen)}</div>
      </div>`
    )
    .join("");
}

/* ----------------------------------------------------------------- settings */

async function loadSettings() {
  try {
    const s = await api("/api/settings");
    $$("#seg-organize button").forEach((b) =>
      b.setAttribute("aria-pressed", String(b.dataset.value === s.organize))
    );
    $("#sw-junk").setAttribute("aria-checked", String(s.skip_junk));
    $("#sw-sidecar").setAttribute("aria-checked", String(s.sidecars));
    $("#sw-pause").setAttribute("aria-checked", String(s.paused));
    $("#set-name").value = s.device_name;
    $("#set-root").value = s.root;
    $("#kv-token").textContent = s.token || "disabled";
    $("#kv-port").textContent = `:${s.port}`;
  } catch {
    /* ignore */
  }
}

async function patch(payload) {
  try {
    await api("/api/settings", { method: "PATCH", json: payload });
    await loadSettings();
    await loadStats();
  } catch (err) {
    toast(err.message, "bad");
  }
}

/* ------------------------------------------------------------------- stream */

function connectStream() {
  const token = PS.key ? `?k=${encodeURIComponent(PS.key)}` : "";
  const source = new EventSource(`/api/events${token}`);
  source.onmessage = (event) => {
    let data;
    try {
      data = JSON.parse(event.data);
    } catch {
      return;
    }
    pushActivity(data);
    if (data.kind === "stored" || data.kind === "duplicate") loadStats();
    if (data.kind === "device") loadDevices();
    if (ui.view === "overview" || ui.view === "files") scheduleFiles();
  };
  source.onerror = () => {
    $("#live-chip").innerHTML = `<span class="dot bad"></span>reconnecting`;
  };
  source.onopen = () => {
    $("#live-chip").innerHTML = `<span class="dot live"></span>live`;
  };
}

function activityRow({ kind, title, detail, size, at }) {
  const div = document.createElement("div");
  div.className = "act";
  const ico = kind === "duplicate" ? "check" : kind === "device" ? "phone" : "upload";
  div.innerHTML = `
    <span class="act-ico ${escape(kind || "stored")}">${icon(ico)}</span>
    <div class="act-body">
      <div class="act-title">${escape(title)}
        ${size ? `<span class="tag">${bytes(size)}</span>` : ""}
      </div>
      <div class="act-detail" title="${escape(detail || "")}">${escape(
        detail || ""
      )}</div>
    </div>
    <span class="act-time" data-at="${at}">${clock(at)}</span>`;
  return div;
}

function pushActivity(event) {
  const host = $("#activity");
  if (host.querySelector(".empty")) host.innerHTML = "";
  host.prepend(activityRow(event));
  while (host.children.length > 40) host.lastElementChild.remove();
}

/** Seed the feed with what arrived before this page opened, so a fresh
 *  dashboard is never a blank panel. Live events stack on top of these. */
async function loadActivity() {
  const host = $("#activity");
  try {
    const { files } = await api("/api/files?limit=8");
    if (!files.length || host.querySelector(".act")) return;
    host.innerHTML = "";
    for (const f of files) {
      host.append(
        activityRow({
          kind: "stored",
          title: f.name,
          detail: `${f.device} · ${f.rel}`,
          size: f.size,
          at: f.received,
        })
      );
    }
  } catch {
    /* the empty state already explains itself */
  }
}

/* -------------------------------------------------------------------- wiring */

function setView(view) {
  ui.view = view;
  $$(".nav-item").forEach((b) =>
    b.toggleAttribute("aria-current", b.dataset.view === view)
  );
  $$(".view").forEach((v) => (v.hidden = v.id !== `view-${view}`));
  $("#page-title").textContent = VIEW_TITLES[view] || "Overview";
  if (view === "files") loadFiles();
  if (view === "devices") loadDevices();
  if (view === "settings") loadSettings();
  history.replaceState({}, "", view === "overview" ? "/dashboard" : `#${view}`);
}

function wire() {
  $$(".nav-item").forEach((btn) =>
    btn.addEventListener("click", () => setView(btn.dataset.view))
  );
  const initial = location.hash.replace("#", "");
  if (VIEW_TITLES[initial]) setView(initial);

  $("#search").addEventListener("input", (e) => {
    ui.query = e.target.value.trim();
    scheduleFiles();
    if (ui.view !== "files") setView("files");
  });

  for (const id of ["#open-folder", "#open-folder-2"]) {
    const el = $(id);
    if (el)
      el.addEventListener("click", () =>
        api("/api/actions/open-folder", { method: "POST" }).catch((e) =>
          toast(e.message, "bad")
        )
      );
  }

  const dialog = $("#qr-dialog");
  for (const id of ["#show-qr"]) {
    $(id).addEventListener("click", () => {
      if (typeof dialog.showModal === "function") dialog.showModal();
    });
  }

  for (const id of ["#copy-url", "#copy-url-2"]) {
    const el = $(id);
    if (!el) continue;
    el.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(ui.info?.sync_url || "");
        toast("Phone link copied", "ok");
      } catch {
        toast("Copy failed", "bad");
      }
    });
  }

  $("#copy-install").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText($("#install-cmd").textContent);
      toast("Command copied", "ok");
    } catch {
      /* ignore */
    }
  });

  $$("#seg-organize button").forEach((btn) =>
    btn.addEventListener("click", () => patch({ organize: btn.dataset.value }))
  );

  $("#sw-junk").addEventListener("click", () =>
    patch({ skip_junk: $("#sw-junk").getAttribute("aria-checked") !== "true" })
  );
  $("#sw-sidecar").addEventListener("click", () =>
    patch({ sidecars: $("#sw-sidecar").getAttribute("aria-checked") !== "true" })
  );
  $("#sw-pause").addEventListener("click", () =>
    patch({ paused: $("#sw-pause").getAttribute("aria-checked") !== "true" })
  );

  $("#save-name").addEventListener("click", () => {
    const name = $("#set-name").value.trim();
    if (!name) return;
    patch({ device_name: name }).then(() => loadInfo());
  });

  // Storage folder: typed path everywhere, native picker inside the desktop app.
  const inApp = new URLSearchParams(location.search).get("app") === "1";
  if (inApp) $("#browse-root").hidden = false;
  $("#browse-root").addEventListener("click", () => {
    if (window.ipc?.postMessage) window.ipc.postMessage("pick-folder");
    else toast("Browse is only available in the desktop app", "bad");
  });
  window.__psPicked = (dir) => {
    if (typeof dir === "string" && dir) $("#set-root").value = dir;
  };
  $("#save-root").addEventListener("click", async () => {
    const root = $("#set-root").value.trim();
    if (!root) return;
    try {
      await api("/api/settings", { method: "PATCH", json: { root } });
      await Promise.all([loadSettings(), loadStats()]);
      toast("New files will be saved there", "ok");
    } catch (err) {
      toast(err.message, "bad");
    }
  });

  $("#refresh-files").addEventListener("click", loadFiles);

  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement.tagName !== "INPUT") {
      e.preventDefault();
      $("#search").focus();
    }
  });
}

boot();
