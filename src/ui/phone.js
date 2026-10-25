/* Phone surface: pick → chunked upload → live progress. */

const { $, icon, bytes, ago, api, toast, escape, deviceName, platform, sleep } =
  PS;

const state = {
  info: null,
  total: 0,
  sent: 0,
  busy: false,
  seq: 0,
  items: [],
};

/* ------------------------------------------------------------------ boot */

async function boot() {
  renderRecentSkeleton();
  await connect();
  setInterval(connect, 20_000);
  await Promise.all([loadRecent(), loadStats()]);
  wire();
  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("/assets/sw.js").catch(() => {});
  }
}

async function connect() {
  try {
    const info = await api("/api/info");
    state.info = info;
    $("#server-name").textContent = info.name;
    $("#server-addr").textContent = (info.sync_url || "").replace(/^https?:\/\//, "").split("?")[0];
    $("#conn-dot").className = "dot live";
    $("#conn-text").textContent = "connected";
    $("#auto-cmd").textContent = buildAutoCommand(info);
    if (info.paused) {
      $("#status").textContent = "Paused on the laptop";
      $("#status-sub").textContent =
        "The laptop stopped accepting files. Resume it from the dashboard.";
      $("#status-dot").className = "dot warn";
    }
  } catch (err) {
    $("#conn-dot").className = "dot bad";
    $("#conn-text").textContent = err.status === 401 ? "token needed" : "offline";
    if (err.status === 401) openSettings();
  }
}

function buildAutoCommand(info) {
  const origin = (info.sync_url || location.origin).split("?")[0];
  const token = PS.key ? ` --token ${PS.key}` : "";
  return `phonesync send /sdcard/DCIM /sdcard/Pictures /sdcard/Download --to ${origin}${token} --watch`;
}

/* ------------------------------------------------------------------- files */

function isJunk(rel) {
  const lower = rel.toLowerCase();
  if (lower.startsWith(".")) return true;
  const parts = lower.split("/");
  if (parts.some((p) => p.startsWith("."))) return true;
  return (
    lower.includes("/android/data/") ||
    lower.includes("/android/obb/") ||
    lower.includes("node_modules") ||
    lower.includes("/.thumbnails/")
  );
}

function addFiles(fileList) {
  const files = Array.from(fileList || []);
  if (!files.length) return;
  let added = 0;
  let skipped = 0;

  for (const file of files) {
    const rel = (file.webkitRelativePath || file.name).replace(/\\/g, "/");
    if (isJunk(rel)) {
      skipped++;
      continue;
    }
    const dir = rel.includes("/") ? rel.slice(0, rel.lastIndexOf("/")) : "";
    const item = {
      key: ++state.seq,
      file,
      name: file.name,
      rel,
      dir,
      size: file.size,
      sent: 0,
      status: "queued",
      error: "",
      url: file.type.startsWith("image/") ? URL.createObjectURL(file) : "",
    };
    state.items.push(item);
    added++;
  }

  $("#queue-panel").hidden = state.items.length === 0;
  renderQueue();
  recalc();

  if (added) {
    toast(
      `${added} file${added === 1 ? "" : "s"} queued${
        skipped ? `, ${skipped} junk skipped` : ""
      }`,
      "ok"
    );
    pump();
  } else if (skipped) {
    toast("Only junk files were selected", "warn");
  }
}

function renderQueue() {
  const list = $("#queue");
  list.innerHTML = "";
  for (const item of state.items) {
    const row = document.createElement("li");
    row.className = "queue-row";
    const [, ico] = PS.kindOf(item.name);
    row.innerHTML = `
      <span class="thumb">${
        item.url
          ? `<img alt="" src="${item.url}" />`
          : icon(ico)
      }</span>
      <div class="file-main">
        <div class="file-name" title="${escape(item.name)}">${escape(item.name)}</div>
        <div class="file-meta">
          <span class="mono">${bytes(item.size)}</span>
          ${item.dir ? `<span class="sep">·</span><span>${escape(item.dir)}</span>` : ""}
        </div>
        <div class="bar slim"><i></i></div>
      </div>
      <span class="queue-state">queued</span>`;
    item.el = {
      row,
      bar: row.querySelector(".bar > i"),
      label: row.querySelector(".queue-state"),
      meta: row.querySelector(".file-meta"),
    };
    list.appendChild(row);
    paint(item);
  }
}

function paint(item) {
  if (!item.el) return;
  const pct = item.size ? Math.min(100, (item.sent / item.size) * 100) : 0;
  item.el.bar.style.width = `${pct}%`;
  const s = item.el.label;
  s.className = "queue-state";
  switch (item.status) {
    case "sending":
      s.textContent = `${Math.round(pct)}%`;
      break;
    case "done":
      s.textContent = "saved";
      s.classList.add("done");
      break;
    case "dup":
      s.textContent = "already there";
      s.classList.add("dup");
      break;
    case "error":
      s.textContent = "retry";
      s.classList.add("err");
      s.title = item.error;
      s.style.cursor = "pointer";
      s.onclick = () => {
        item.status = "queued";
        item.error = "";
        paint(item);
        pump();
      };
      break;
    default:
      s.textContent = "queued";
  }
}

function recalc() {
  state.total = state.items.reduce((a, i) => a + i.size, 0);
  state.sent = state.items.reduce((a, i) => a + Math.min(i.sent, i.size), 0);
  const pct = state.total ? Math.round((state.sent / state.total) * 100) : 0;

  const ring = $("#ring-fill");
  const circumference = 207.3;
  ring.style.strokeDashoffset = String(circumference * (1 - pct / 100));
  $("#ring-pct").textContent = `${pct}%`;
  $("#hero-bar").style.width = `${pct}%`;

  const done = state.items.filter((i) => i.status === "done" || i.status === "dup").length;
  const failed = state.items.filter((i) => i.status === "error").length;
  const sending = state.items.filter((i) => i.status === "sending").length;

  $("#queue-stats").textContent = `${done} / ${state.items.length}`;

  if (!state.items.length) {
    $("#status").textContent = "Ready when you are";
    $("#status-sub").textContent =
      "Pick photos, videos or any folder. Files are copied to the laptop over Wi-Fi, never through the cloud.";
    $("#status-dot").className = "dot live";
    return;
  }
  if (sending) {
    $("#status").textContent = `Sending ${done + 1} of ${state.items.length}`;
    $("#status-sub").textContent = `${bytes(state.sent)} of ${bytes(state.total)} — keep this tab open until it finishes.`;
    $("#status-dot").className = "dot live";
  } else if (failed) {
    $("#status").textContent = `${failed} file${failed === 1 ? "" : "s"} failed`;
    $("#status-sub").textContent = "Tap retry next to the file. The laptop may have gone to sleep.";
    $("#status-dot").className = "dot bad";
  } else if (done) {
    $("#status").textContent = `All done — ${done} file${done === 1 ? "" : "s"} copied`;
    $("#status-sub").textContent = state.info
      ? `Saved into ${state.info.root}\\${deviceName()}`
      : "Saved on the laptop.";
    $("#status-dot").className = "dot ok";
  }
}

/* ------------------------------------------------------------------ upload */

async function pump() {
  if (state.busy) return;
  state.busy = true;
  try {
    for (;;) {
      const next = state.items.find((i) => i.status === "queued");
      if (!next) break;
      await sendOne(next);
    }
  } finally {
    state.busy = false;
    recalc();
    if (state.items.some((i) => i.status === "done" || i.status === "dup")) {
      loadRecent();
      loadStats();
    }
  }
}

async function sendOne(item) {
  item.status = "sending";
  paint(item);
  recalc();

  const meta = {
    name: item.name,
    rel_path: item.rel,
    size: item.size,
    mtime_ms: item.file.lastModified || Date.now(),
    device: deviceName(),
    platform: platform(),
  };

  try {
    const session = await withRetry(() =>
      api("/api/session", { method: "POST", json: meta })
    );
    let offset = session.offset || 0;
    const chunk = session.chunk_size || 8 * 1024 * 1024;
    item.sent = offset;
    paint(item);

    while (offset < item.size) {
      const end = Math.min(offset + chunk, item.size);
      const blob = item.file.slice(offset, end);
      const data = await withRetry(async () => {
        const res = await fetch(`/api/session/${session.id}`, {
          method: "PUT",
          headers: { "x-sync-key": PS.key, "x-offset": String(offset) },
          body: blob,
        });
        if (!res.ok) {
          const body = await res.json().catch(() => ({}));
          const err = new Error(body.error || `upload failed (${res.status})`);
          err.status = res.status;
          throw err;
        }
        return res.json();
      });
      offset = data.offset;
      item.sent = offset;
      paint(item);
      recalc();
    }

    const done = await withRetry(() =>
      api(`/api/session/${session.id}/complete`, { method: "POST" })
    );
    item.status = done.deduped ? "dup" : "done";
    item.sent = item.size;
  } catch (err) {
    item.status = "error";
    item.error = err.message || "upload failed";
    if (err.status === 401) openSettings();
    toast(`${item.name}: ${item.error}`, "bad");
  } finally {
    paint(item);
    recalc();
  }
}

async function withRetry(fn, attempts = 4) {
  let last;
  for (let i = 0; i < attempts; i++) {
    try {
      return await fn();
    } catch (err) {
      last = err;
      if (err.status && err.status < 500 && err.status !== 429) throw err;
      await sleep(400 * Math.pow(2, i));
    }
  }
  throw last;
}

/* ------------------------------------------------------------- recent files */

function renderRecentSkeleton() {
  $("#recent").innerHTML = Array.from({ length: 3 })
    .map(
      () => `<li class="file-row">
        <span class="thumb"></span>
        <div class="file-main">
          <div class="file-name faint" style="width:45%">loading…</div>
        </div>
      </li>`
    )
    .join("");
}

async function loadRecent() {
  try {
    const data = await api("/api/files?limit=6");
    const list = $("#recent");
    if (!data.files.length) {
      list.innerHTML = `<li class="empty">${icon(
        "inbox"
      )}Nothing here yet. Send your first batch.</li>`;
      return;
    }
    const token = PS.key ? `?k=${encodeURIComponent(PS.key)}` : "";
    list.innerHTML = data.files
      .map((f) => {
        const [tag, ico] = PS.kindOf(f.name);
        const preview =
          tag === "img" && f.size < 24 * 1024 * 1024
            ? `<img alt="" loading="lazy" src="/api/raw/${f.id}${token}" />`
            : icon(ico);
        return `<li class="file-row">
          <span class="thumb">${preview}</span>
          <div class="file-main">
            <div class="file-name" title="${escape(f.name)}">${escape(f.name)}</div>
            <div class="file-meta">
              <span>${escape(f.device)}</span>
              <span class="sep">·</span>
              <span>${bytes(f.size)}</span>
              <span class="sep">·</span>
              <span>${ago(f.received)}</span>
            </div>
          </div>
          ${
            tag
              ? `<span class="tag ${tag}">${PS.extOf(f.name) || "file"}</span>`
              : ""
          }
        </li>`;
      })
      .join("");
  } catch {
    /* keep skeleton */
  }
}

async function loadStats() {
  try {
    const s = await api("/api/stats");
    $("#storage-used").textContent = `${s.human_total} synced · ${s.files} files`;
    $("#storage-free").textContent = `${s.human_free} free on laptop`;
    const total = s.bytes + (s.free_space || 0);
    const pct = total ? Math.min(100, (s.bytes / total) * 100) : 0;
    $("#storage-bar").style.width = `${Math.max(1.5, pct)}%`;
  } catch {
    /* ignore */
  }
}

/* ------------------------------------------------------------------ wiring */

function wire() {
  const pick = (id, el) => $(id).addEventListener("click", () => $(el).click());
  pick("#pick-media", "#in-media");
  pick("#pick-file", "#in-file");
  pick("#pick-folder", "#in-folder");

  for (const id of ["#in-media", "#in-file", "#in-folder"]) {
    $(id).addEventListener("change", (e) => {
      addFiles(e.target.files);
      e.target.value = "";
    });
  }

  $("#queue-clear").addEventListener("click", () => {
    for (const item of state.items) if (item.url) URL.revokeObjectURL(item.url);
    state.items = [];
    $("#queue-panel").hidden = true;
    recalc();
  });

  $("#refresh-recent").addEventListener("click", () => {
    loadRecent();
    loadStats();
  });

  $("#copy-cmd").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText($("#auto-cmd").textContent);
      toast("Command copied", "ok");
    } catch {
      toast("Select and copy the command manually", "warn");
    }
  });

  $("#open-settings").addEventListener("click", openSettings);
  $("#settings-form").addEventListener("submit", (e) => {
    const action = e.submitter?.value;
    if (action === "save") {
      const name = $("#set-device").value.trim();
      if (name) localStorage.setItem("phonesync.device", name);
      const token = $("#set-token").value.trim();
      if (token !== PS.key) {
        PS.setKey(token);
        connect();
      }
    }
  });

  // Drag and drop (desktop browsers).
  const zone = $("#dropzone");
  let depth = 0;
  window.addEventListener("dragenter", (e) => {
    e.preventDefault();
    depth++;
    zone.classList.add("on");
  });
  window.addEventListener("dragover", (e) => e.preventDefault());
  window.addEventListener("dragleave", () => {
    depth = Math.max(0, depth - 1);
    if (!depth) zone.classList.remove("on");
  });
  window.addEventListener("drop", (e) => {
    e.preventDefault();
    depth = 0;
    zone.classList.remove("on");
    if (e.dataTransfer?.files?.length) addFiles(e.dataTransfer.files);
  });

  window.addEventListener("online", connect);
  window.addEventListener("offline", () => {
    $("#conn-dot").className = "dot bad";
    $("#conn-text").textContent = "no wi-fi";
  });
}

function openSettings() {
  $("#set-device").value = deviceName();
  $("#set-token").value = PS.key;
  const dialog = $("#settings-dialog");
  if (typeof dialog.showModal === "function" && !dialog.open) dialog.showModal();
}

boot();
