"use strict";

// ---------- helpers ----------
const $ = (s, el = document) => el.querySelector(s);
const esc = (v) =>
  String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const fmt = (n) => Number(n || 0).toLocaleString("pt-BR");
const pct = (a, b) => (b ? ((a / b) * 100).toFixed(1) : "0.0") + "%";

function ago(ts) {
  const s = Math.max(0, Math.floor(Date.now() / 1000 - ts));
  if (s < 5) return "agora";
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86400) return `${Math.floor(s / 3600)} h`;
  return `${Math.floor(s / 86400)} d`;
}
function hhmm(ts) {
  return new Date(ts * 1000).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}
function color(str) {
  let h = 0;
  for (const c of String(str)) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 45% 38%)`;
}
function fmtBytes(n) {
  n = Number(n || 0);
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let i = -1;
  do { n /= 1024; i++; } while (n >= 1024 && i < u.length - 1);
  return `${n.toFixed(n < 10 ? 1 : 0)} ${u[i]}`;
}
function shortIp(ip) {
  if (!ip || !ip.includes(":")) return ip || "";
  const parts = ip.split(":");
  return parts.length > 4 ? `${parts.slice(0, 2).join(":")}:…:${parts[parts.length - 1]}` : ip;
}
const ipCell = (ips) => ips.length ? ips.map((ip) => `<span title="${esc(ip)}">${esc(shortIp(ip))}</span>`).join("<br>") : `<span class="muted">—</span>`;
function flag(cc) {
  if (!cc || cc.length !== 2) return "";
  return String.fromCodePoint(...[...cc.toUpperCase()].map((c) => 0x1f1a5 + c.charCodeAt(0)));
}

// Real app icons, fetched once per path from the Tauri side and cached.
const icons = new Map(); // path -> dataURL | null | "pending"
function avatar(name, path, small = false, large = false) {
  const url = path && icons.get(path);
  const cls = `avatar${small ? " sm" : ""}${large ? " lg" : ""}`;
  if (url && url !== "pending") return `<div class="${cls} has-img"><img src="${url}" alt=""></div>`;
  return `<div class="${cls}" style="background:${color(name)}" ${path ? `data-icon="${esc(path)}"` : ""}>${esc((name || "?").trim()[0] || "?").toUpperCase()}</div>`;
}
function hydrateIcons(root = document) {
  root.querySelectorAll("[data-icon]").forEach((el) => {
    const path = el.dataset.icon;
    const cached = icons.get(path);
    if (cached && cached !== "pending") {
      el.classList.add("has-img");
      el.innerHTML = `<img src="${cached}" alt="">`;
      el.removeAttribute("data-icon");
    } else if (cached === undefined && tauri) {
      icons.set(path, "pending");
      tauri.core.invoke("app_icon", { path }).then((url) => {
        icons.set(path, url || null);
        if (url) hydrateIcons();
      });
    }
  });
}

let toastTimer;
function toast(msg, err = false) {
  const t = $("#toast");
  t.textContent = msg;
  t.className = "show" + (err ? " err" : "");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.className = ""), 2600);
}

// ---------- API ----------
const tauri = window.__TAURI__;
async function api(method, path, body) {
  if (!tauri) throw new Error("NO_TAURI");
  return tauri.core.invoke("api", { method, path, body: body ?? null });
}
const get = (p) => api("GET", p);
const post = (p, b) => api("POST", p, b ?? {});
const put = (p, b) => api("PUT", p, b);

async function quickRule(action, entity) {
  try {
    const r = await post("/api/v1/rules/quick", { action, entity });
    toast(`Regra adicionada: ${r.rule}`);
    refresh();
  } catch (e) {
    toast(String(e), true);
  }
}

// ---------- router ----------
const state = { page: "dashboard", selApp: null, connFilter: "", activeOnly: true, dnsBlocked: false, dnsQ: "", status: null, down: null, expView: "server", expQ: "" };
const isFilter = () => state.status && state.status.mode === "filter";

// Don't redraw while the user is pressing the mouse (dragging a scrollbar,
// selecting text) and keep every scroll position across redraws.
let pointerDown = false;
document.addEventListener("pointerdown", () => (pointerDown = true), true);
document.addEventListener("pointerup", () => (pointerDown = false), true);
window.addEventListener("blur", () => (pointerDown = false));
function saveScroll() {
  const m = new Map();
  document.querySelectorAll("#main, #main [id]").forEach((el) => {
    if (el.scrollTop || el.scrollLeft) m.set(el.id || "#main", [el.scrollTop, el.scrollLeft]);
  });
  return m;
}
function restoreScroll(m) {
  m.forEach(([t, l], id) => {
    const el = id === "#main" ? $("#main") : document.getElementById(id);
    if (el) { el.scrollTop = t; el.scrollLeft = l; }
  });
}
const pages = {};
let timer;

function go(page) {
  state.page = page;
  document.querySelectorAll("#nav a").forEach((a) => a.classList.toggle("active", a.dataset.page === page));
  $("#main").innerHTML = "";
  $("#main").scrollTop = 0;
  refresh(true);
}
document.querySelectorAll("#nav a").forEach((a) => a.addEventListener("click", () => go(a.dataset.page)));

async function refresh(first = false) {
  clearTimeout(timer);
  if (pointerDown && !first) {
    timer = setTimeout(refresh, 400);
    return;
  }
  try {
    state.status = await get("/api/v1/status");
    state.down = null;
    renderSide();
    const sc = first ? new Map() : saveScroll();
    await pages[state.page](first);
    restoreScroll(sc);
    hydrateIcons();
    if (document.querySelector(".mapslot")) { ensureMap(); placeMap(); }
    else if (geo.host && !geo.overlay && geo.host.parentElement !== document.body) document.body.appendChild(geo.host);
  } catch (e) {
    state.down = String(e);
    renderDown();
  }
  const slow = ["rules", "settings", "lists"].includes(state.page);
  timer = setTimeout(refresh, slow ? 5000 : 1500);
}

// ---------- updates prepared by scripts/build.sh ----------
let updateState = { info: null, busy: false, checked: 0 };
async function checkUpdate() {
  if (!tauri || Date.now() - updateState.checked < 8000) return;
  updateState.checked = Date.now();
  try { updateState.info = await tauri.core.invoke("update_info"); } catch {}
}
function updateBanner() {
  const u = updateState.info;
  if (!u || !u.staged || !state.status) return "";
  const daemonBuild = state.status.build;
  if (u.staged.build === u.app_build && u.staged.build === daemonBuild) return "";
  return `<div class="upd"><b>Nova versão ${esc(u.staged.version)} pronta</b><div class="muted" style="font-size:11.5px">build ${esc(u.staged.build.split("-")[0])}</div>
    <button class="primary small" id="updBtn" ${updateState.busy ? "disabled" : ""}>${updateState.busy ? "Aguardando senha…" : "Instalar atualização"}</button></div>`;
}
document.addEventListener("click", async (e) => {
  if (e.target && e.target.id === "updBtn") {
    updateState.busy = true;
    renderSide();
    try {
      await tauri.core.invoke("install_update");
    } catch (err) {
      toast(String(err), true);
    }
    updateState.busy = false;
    renderSide();
  }
});

function renderSide() {
  checkUpdate();
  const s = state.status;
  if (s.mode === "monitor") {
    $("#side-status").innerHTML = updateBanner() + `<b style="color:var(--blue)">Modo monitor</b><br>${s.capture_interfaces.length ? `observando ${esc(s.capture_interfaces.join(", "))}` : "sem captura"}<br><span class="muted">v${esc(s.version)}</span>`;
    return;
  }
  const st = !s.dns_ok ? `<span class="red">DNS parado</span>` : s.paused ? `<span class="yellow">Pausado</span>` : `<span class="green">Protegendo</span>`;
  $("#side-status").innerHTML = updateBanner() + `<b>${st}</b><br>${fmt(s.counters.dns_blocked)} bloqueios<br><span class="muted">v${esc(s.version)}</span>`;
}

function renderDown() {
  $("#side-status").innerHTML = `<b class="red">Serviço offline</b>`;
  const noToken = state.down.includes("NO_TOKEN");
  $("#main").innerHTML = `
    <div class="down-screen">
      <h1>${noToken ? "Portward não está instalado" : "O serviço do Portward não está respondendo"}</h1>
      <p class="sub">A interface conversa com o serviço <code>portwardd</code>, que roda como root em segundo plano e faz a filtragem.</p>
      <div class="card">
        <h2>Instalar ou iniciar o serviço</h2>
        <p class="muted">No Terminal, dentro da pasta do projeto:</p>
        <pre>sudo ./scripts/install.sh</pre>
        <p class="muted">Para ver se ele está rodando:</p>
        <pre>sudo launchctl print system/io.github.portward.daemon | head -20</pre>
        <div class="errorbox mono">${esc(state.down)}</div>
      </div>
    </div>`;
}

// ---------- dashboard ----------
const ICON_SHIELD = `<svg viewBox="0 0 24 24"><path d="M12 2 4 5v6c0 5 3.4 9.7 8 11 4.6-1.3 8-6 8-11V5zm-1.2 14.2-3.5-3.5 1.4-1.4 2.1 2.1 4.9-4.9 1.4 1.4z"/></svg>`;
const ICON_PAUSE = `<svg viewBox="0 0 24 24"><path d="M12 2 4 5v6c0 5 3.4 9.7 8 11 4.6-1.3 8-6 8-11V5zM10 15H8V8h2zm6 0h-2V8h2z"/></svg>`;
const ICON_X = `<svg viewBox="0 0 24 24"><path d="M12 2 4 5v6c0 5 3.4 9.7 8 11 4.6-1.3 8-6 8-11V5zm3.5 12.1-1.4 1.4L12 13.4l-2.1 2.1-1.4-1.4 2.1-2.1-2.1-2.1 1.4-1.4 2.1 2.1 2.1-2.1 1.4 1.4-2.1 2.1z"/></svg>`;

function chart(buckets) {
  const now = Math.floor(Date.now() / 60000) * 60;
  const map = new Map(buckets.map((b) => [b.minute, b]));
  const pts = [];
  for (let i = 59; i >= 0; i--) pts.push(map.get(now - i * 60) || { queries: 0, blocked: 0 });
  const max = Math.max(5, ...pts.map((p) => p.queries));
  const w = 600 / 60;
  const bars = pts
    .map((p, i) => {
      const hq = (p.queries / max) * 110;
      const hb = (p.blocked / max) * 110;
      return `<rect x="${i * w + 1}" y="${118 - hq}" width="${w - 2}" height="${hq}" rx="1.5" fill="#2f4f7a"/>` +
        `<rect x="${i * w + 1}" y="${118 - hb}" width="${w - 2}" height="${hb}" rx="1.5" fill="#d12e2e"/>`;
    })
    .join("");
  return `<div class="chart"><svg viewBox="0 0 600 120" preserveAspectRatio="none">${bars}</svg>
    <div class="legend"><span><b style="background:#2f4f7a"></b>Consultas</span><span><b style="background:#d12e2e"></b>Bloqueadas</span><span style="margin-left:auto">últimos 60 minutos · máx ${fmt(max)}/min</span></div></div>`;
}

function topList(items, red, actions) {
  if (!items.length) return `<div class="empty">Nada por enquanto.</div>`;
  const max = Math.max(...items.map((i) => i.count));
  return `<ul class="toplist">${items
    .map(
      (i) => `<li><div class="bar ${red ? "red" : ""}"><i style="width:${(i.count / max) * 100}%"></i><span class="mono">${esc(i.key)}</span></div>
      <span class="n">${fmt(i.count)}</span>${actions ? actions(i.key) : ""}</li>`
    )
    .join("")}</ul>`;
}

const ICON_EYE = `<svg viewBox="0 0 24 24"><path d="M12 4.5C7 4.5 2.7 7.6 1 12c1.7 4.4 6 7.5 11 7.5s9.3-3.1 11-7.5c-1.7-4.4-6-7.5-11-7.5zM12 17a5 5 0 1 1 0-10 5 5 0 0 1 0 10zm0-8a3 3 0 1 0 0 6 3 3 0 0 0 0-6z"/></svg>`;

function countryList(items) {
  return topList(items.map((i) => {
    const [cc, name] = i.key.split("|");
    return { key: `${flag(cc)}  ${name || cc}`, count: i.count };
  }), false);
}

pages.dashboard = async () => {
  if (!isFilter()) return dashboardMonitor();
  return dashboardFilter();
};

async function dashboardMonitor() {
  const s = state.status;
  const [stats, conns, exp] = await Promise.all([get("/api/v1/stats"), get("/api/v1/connections?active=true"), get("/api/v1/exposure")]);
  const apps = new Set(conns.map((x) => x.process.app_path)).size;
  const ids = exp.interfaces.filter((i) => i.public_ipv4 || i.public_ipv6);
  $("#main").innerHTML = `
    <div class="hero monitor">
      <div class="shield">${ICON_EYE}</div>
      <div><h1>Modo monitor</h1><p>Observando ${fmt(apps)} apps falando com ${fmt(stats.remote_servers)} servidores na internet. Nada é bloqueado e o DNS do sistema não foi alterado.</p></div>
      <div class="spacer"></div>
      <button id="toExp">Quem vê seu IP</button>
    </div>
    <div class="mt">${s.warnings.map((w) => `<div class="warn">${esc(w)}</div>`).join("")}</div>
    <div class="grid g4 mt">
      <div class="card stat"><div class="label">Servidores contatados</div><div class="value">${fmt(stats.remote_servers)}</div><div class="hint">agora e nos últimos 15 min</div></div>
      <div class="card stat"><div class="label">Países</div><div class="value">${fmt(stats.countries.length)}</div><div class="hint">${stats.countries.slice(0, 4).map((c) => flag(c.key.split("|")[0])).join(" ")}</div></div>
      <div class="card stat"><div class="label">Recebido / enviado</div><div class="value" style="font-size:20px">${fmtBytes(stats.bytes_in)} <span class="muted">/</span> ${fmtBytes(stats.bytes_out)}</div><div class="hint">das conexões na lista</div></div>
      <div class="card stat"><div class="label">Consultas DNS vistas</div><div class="value">${fmt(s.counters.dns_queries)}</div><div class="hint">${fmt(conns.length)} conexões ativas</div></div>
    </div>
    <div class="cat">Seu IP como os servidores veem</div>
    <div class="idgrid">${ids.length ? ids.map(idCard).join("") + liveMapCard(ids) : `<div class="card muted">Descobrindo seu IP público…</div>`}</div>
    <div class="grid g2 mt">
      <div class="card"><h2>Para onde vão seus dados (países)</h2>${countryList(stats.countries)}</div>
      <div class="card"><h2>Empresas que recebem conexões</h2>${topList(stats.orgs, false)}</div>
    </div>
    <div class="grid g2 mt">
      <div class="card"><h2>Apps com mais conexões agora</h2>${topList(stats.top_apps, false)}</div>
      <div class="card"><h2>Domínios mais consultados</h2>${topList(stats.top_domains, false)}</div>
    </div>`;
  $("#toExp").onclick = () => go("exposure");
}

// ---------- live map (MapLibre GL + OpenFreeMap vector tiles) ----------
// One map instance lives for the whole session; page redraws only move its
// DOM node into the new slot. Falls back to the offline SVG map on failure.
const geo = { host: null, map: null, ml: null, markers: [], key: "", failed: false, loading: false, ready: false, routesAt: 0, overlay: false, origins: [] };
async function ensureMap() {
  if (geo.map || geo.failed || geo.loading) return;
  geo.loading = true;
  try {
    const ml = await import("./vendor/maplibre/maplibre-gl.mjs");
    ml.setWorkerUrl(new URL("vendor/maplibre/maplibre-gl-worker.mjs", location.href).href);
    geo.ml = ml;
    geo.host = document.createElement("div");
    geo.host.className = "maphost";
    document.body.appendChild(geo.host); // parked until a slot exists
    geo.map = new ml.Map({
      container: geo.host,
      style: "https://tiles.openfreemap.org/styles/dark",
      center: [-47.9, -15.8],
      zoom: 2.6,
      attributionControl: false,
      dragRotate: false,
      pitchWithRotate: false,
    });
    geo.map.on("load", () => {
      geo.map.addSource("routes", { type: "geojson", data: { type: "FeatureCollection", features: [] } });
      geo.map.addSource("dests", { type: "geojson", data: { type: "FeatureCollection", features: [] } });
      geo.map.addLayer({ id: "routes-glow", type: "line", source: "routes", layout: { "line-cap": "round" },
        paint: { "line-color": ["case", ["get", "vpn"], "#b39ddb", "#ff2a36"], "line-opacity": 0.12, "line-width": ["+", 3, ["*", 2, ["get", "w"]]], "line-blur": 3 } });
      geo.map.addLayer({ id: "routes", type: "line", source: "routes", layout: { "line-cap": "round" },
        paint: { "line-color": ["case", ["get", "vpn"], "#b39ddb", "#ff5a63"],
          "line-opacity": ["case", ["boolean", ["feature-state", "hover"], false], 0.95, 0.4],
          "line-width": ["case", ["boolean", ["feature-state", "hover"], false], ["+", 2.2, ["get", "w"]], ["+", 0.8, ["get", "w"]]] } });
      // invisible, wide copy of the lines so they are easy to hover
      geo.map.addLayer({ id: "routes-hit", type: "line", source: "routes", paint: { "line-color": "#000", "line-opacity": 0, "line-width": 14 } });
      geo.map.addLayer({ id: "dests", type: "circle", source: "dests",
        paint: { "circle-color": "#ff5a63", "circle-opacity": 0.55, "circle-radius": ["+", 2.5, ["*", 1.6, ["get", "w"]]], "circle-stroke-color": "#ffd0d3", "circle-stroke-width": 0.6, "circle-stroke-opacity": 0.5 } });
      geo.map.addLayer({ id: "dest-labels", type: "symbol", source: "dests", minzoom: 1.5,
        layout: { "text-field": ["get", "label"], "text-size": 10.5, "text-offset": [0, 1.1], "text-anchor": "top", "text-font": ["Noto Sans Regular"], "text-optional": true },
        paint: { "text-color": "#d9d9d9", "text-halo-color": "#101317", "text-halo-width": 1.2 } });
      const popup = new ml.Popup({ closeButton: false, closeOnClick: false, offset: 12, className: "pw-pop", maxWidth: "320px" });
      let hovered = null;
      const setHover = (id) => {
        if (hovered !== null) geo.map.setFeatureState({ source: "routes", id: hovered }, { hover: false });
        hovered = id;
        if (id !== null) geo.map.setFeatureState({ source: "routes", id }, { hover: true });
      };
      const show = (e) => {
        const f = e.features[0];
        const p = f.properties;
        setHover(f.id);
        geo.map.getCanvas().style.cursor = "pointer";
        const hosts = JSON.parse(p.hosts || "[]");
        popup.setLngLat(e.lngLat).setHTML(`
          <div class="pp-route">${p.vpn ? `<span class="pill vpn">VPN</span> ` : ""}${esc(p.from || "Você")} <span class="pp-arrow">→</span> <b>${esc(p.label)}</b></div>
          ${p.org ? `<div class="pp-org">${esc(p.org)}</div>` : ""}
          <div class="pp-count">${p.n} ${p.n == 1 ? "conexão" : "conexões"} · ${esc(p.apps)}</div>
          <div class="pp-hosts">${hosts.map(([h, n]) => `<div><span>${esc(h)}</span><i>${n}</i></div>`).join("")}${p.more > 0 ? `<div class="muted">+ ${p.more} outros</div>` : ""}</div>`).addTo(geo.map);
      };
      const hide = () => { setHover(null); geo.map.getCanvas().style.cursor = ""; popup.remove(); };
      for (const layer of ["routes-hit", "dests"]) {
        geo.map.on("mousemove", layer, show);
        geo.map.on("mouseleave", layer, hide);
      }
      geo.ready = true;
      geo.routesAt = 0;
      updateRoutes();
    });
    geo.map.on("error", (e) => console.warn("map", e && e.error));
  } catch (e) {
    console.warn("map unavailable", e);
    geo.failed = true;
  }
  geo.loading = false;
  placeMap();
}

// Points along the great circle between two coordinates (flight-route look).
function greatCircle(a, b, n = 48) {
  const rad = Math.PI / 180, deg = 180 / Math.PI;
  const [l1, p1, l2, p2] = [a[0] * rad, a[1] * rad, b[0] * rad, b[1] * rad];
  const d = 2 * Math.asin(Math.sqrt(Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin((l2 - l1) / 2) ** 2));
  if (d < 1e-6) return [a, b];
  const out = [];
  let prevLon = null;
  for (let i = 0; i <= n; i++) {
    const f = i / n;
    const A = Math.sin((1 - f) * d) / Math.sin(d), B = Math.sin(f * d) / Math.sin(d);
    const x = A * Math.cos(p1) * Math.cos(l1) + B * Math.cos(p2) * Math.cos(l2);
    const y = A * Math.cos(p1) * Math.sin(l1) + B * Math.cos(p2) * Math.sin(l2);
    const z = A * Math.sin(p1) + B * Math.sin(p2);
    let lon = Math.atan2(y, x) * deg;
    const lat = Math.atan2(z, Math.sqrt(x * x + y * y)) * deg;
    if (prevLon !== null) while (lon - prevLon > 180) lon -= 360;
    if (prevLon !== null) while (prevLon - lon > 180) lon += 360;
    prevLon = lon;
    out.push([lon, lat]);
  }
  return out;
}

// Lines from where you are (per exit interface) to where your connections go.
async function updateRoutes() {
  if (!geo.ready || Date.now() - geo.routesAt < 6000 || !geo.origins.length) return;
  geo.routesAt = Date.now();
  let conns;
  try { conns = await get("/api/v1/connections?active=true"); } catch { return; }
  const originOf = (c) => geo.origins.find((o) => o.iface === c.interface) || geo.origins.find((o) => !o.vpn) || geo.origins[0];
  const groups = new Map();
  for (const c of conns) {
    const g = c.geo;
    if (!g || g.lat == null || g.lon == null || c.scope !== "internet") continue;
    const o = originOf(c);
    const key = `${o.lon.toFixed(1)},${o.lat.toFixed(1)}>${g.lon.toFixed(1)},${g.lat.toFixed(1)}`;
    const e = groups.get(key) || { o, d: [g.lon, g.lat], n: 0, apps: new Set(), hosts: new Map(), vpn: !!o.vpn, org: g.org, label: [g.city, g.country].filter(Boolean).join(", ") };
    e.n++;
    e.apps.add(c.process.name);
    const h = c.domain || c.remote_ip;
    e.hosts.set(h, (e.hosts.get(h) || 0) + 1);
    groups.set(key, e);
  }
  const routes = [], dests = [];
  let id = 0;
  for (const e of groups.values()) {
    const w = Math.min(3, Math.log2(1 + e.n) * 0.8);
    const hosts = [...e.hosts.entries()].sort((a, b) => b[1] - a[1]);
    const props = {
      w, vpn: e.vpn, n: e.n, label: e.label, org: e.org || "", from: e.o.label || "",
      apps: [...e.apps].slice(0, 5).join(", "),
      hosts: JSON.stringify(hosts.slice(0, 6)), more: Math.max(0, hosts.length - 6),
    };
    routes.push({ type: "Feature", id: ++id, properties: props, geometry: { type: "LineString", coordinates: greatCircle([e.o.lon, e.o.lat], e.d) } });
    dests.push({ type: "Feature", id, properties: props, geometry: { type: "Point", coordinates: e.d } });
  }
  geo.map.getSource("routes").setData({ type: "FeatureCollection", features: routes });
  geo.map.getSource("dests").setData({ type: "FeatureCollection", features: dests });
}

function placeMap() {
  const slot = geo.overlay ? document.querySelector("#mapOverlay .mapslot") : document.querySelector("#main .mapslot");
  if (!slot || !geo.map) return;
  if (geo.host.parentElement !== slot) {
    slot.appendChild(geo.host);
    geo.map.resize();
  }
  const src = document.querySelector("#main .mapslot") || slot;
  const pts = JSON.parse(src.dataset.points || "[]");
  geo.origins = pts;
  updateRoutes();
  const key = JSON.stringify(pts);
  if (key === geo.key) return;
  geo.key = key;
  geo.markers.forEach((m) => m.remove());
  geo.markers = pts.map((p) => {
    const el = document.createElement("div");
    el.className = "mk" + (p.vpn ? " vpn" : "");
    el.innerHTML = `<i></i><b></b>`;
    el.title = p.label;
    return new geo.ml.Marker({ element: el }).setLngLat([p.lon, p.lat]).addTo(geo.map);
  });
  // Country-level view around you; scroll to zoom out and see the routes.
  if (pts.length === 1) geo.map.jumpTo({ center: [pts[0].lon, pts[0].lat], zoom: 2.6 });
  else if (pts.length > 1) {
    const b = new geo.ml.LngLatBounds();
    pts.forEach((p) => b.extend([p.lon, p.lat]));
    geo.map.fitBounds(b, { padding: 60, maxZoom: 3, duration: 0 });
  }
}

const ICON_EXPAND = `<svg viewBox="0 0 24 24"><path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5"/></svg>`;
const ICON_CLOSE = `<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18"/></svg>`;
function openMapOverlay() {
  if (geo.overlay) return;
  geo.overlay = true;
  const o = document.createElement("div");
  o.id = "mapOverlay";
  o.innerHTML = `<div class="panel"><div class="ohead"><b>Mapa das suas conexões</b>
    <span class="muted"><span class="legend-line"></span>rotas do seu IP até os servidores · role para zoom</span>
    <span style="flex:1"></span><span class="muted" style="font-size:11px">Mapa © <a href="https://openfreemap.org" target="_blank">OpenFreeMap</a> · © <a href="https://www.openmaptiles.org" target="_blank">OpenMapTiles</a> · dados © <a href="https://www.openstreetmap.org/copyright" target="_blank">OpenStreetMap</a></span>
    <button class="mapbtn" id="mapClose" title="Fechar (Esc)">${ICON_CLOSE}</button></div><div class="mapslot"></div></div>`;
  document.body.appendChild(o);
  const close = () => { geo.overlay = false; o.remove(); document.removeEventListener("keydown", onKey); placeMap(); };
  const onKey = (e) => e.key === "Escape" && close();
  document.addEventListener("keydown", onKey);
  o.addEventListener("click", (e) => e.target === o && close());
  $("#mapClose").onclick = close;
  placeMap();
}
document.addEventListener("click", (e) => { if (e.target.closest && e.target.closest("#mapExpand")) openMapOverlay(); });

function liveMapCard(ifs) {
  const located = ifs.filter((i) => i.location);
  if (geo.failed || !located.length) return mapCard(ifs);
  const pts = located.map((i) => ({ iface: i.name, lat: i.location.lat, lon: i.location.lon, vpn: i.kind === "vpn", label: `${i.label}: ${[i.location.city, i.location.region].filter(Boolean).join(", ")}` }));
  const main = located[0].location;
  return `<div class="idcard mapcard live"><div class="mapslot" data-points="${esc(JSON.stringify(pts))}"></div>
    <button class="mapbtn" id="mapExpand" title="Expandir mapa">${ICON_EXPAND}</button>
    <div class="maplabel"><b>${flag(main.country)} ${esc([main.city, main.region].filter(Boolean).join(", "))}</b>
      <span class="muted">aproximado, pela localização do seu IP${pts.length > 1 ? " · roxo = VPN" : ""}</span></div></div>`;
}

// Approximate location of your public IPs, on an offline world map.
function mapCard(ifs) {
  const K = 1000 / 360;
  const pts = ifs.filter((i) => i.location).map((i) => ({ x: (i.location.lon + 180) * K, y: (90 - i.location.lat) * K, vpn: i.kind === "vpn", i }));
  if (!pts.length) return `<div class="idcard mapcard"><div class="muted" style="padding:20px">Localizando seu IP…</div></div>`;
  const xs = pts.map((p) => p.x), ys = pts.map((p) => p.y);
  const cx = (Math.min(...xs) + Math.max(...xs)) / 2, cy = (Math.min(...ys) + Math.max(...ys)) / 2;
  const w = Math.max(150, (Math.max(...xs) - Math.min(...xs)) * 1.8), h = w * 0.5;
  const vb = `${(cx - w / 2).toFixed(1)} ${(cy - h / 2).toFixed(1)} ${w.toFixed(1)} ${h.toFixed(1)}`;
  const r = w / 90;
  const main = pts[0].i.location;
  const lines = pts.length > 1 ? `<line x1="${pts[0].x}" y1="${pts[0].y}" x2="${pts[1].x}" y2="${pts[1].y}" stroke="#b39ddb" stroke-width="${r / 3}" stroke-dasharray="${r} ${r}"/>` : "";
  return `<div class="idcard mapcard">
    <svg viewBox="${vb}" preserveAspectRatio="xMidYMid slice">
      <rect x="-10" y="-10" width="1020" height="520" fill="#101317"/>
      <path d="${WORLD_LAND}" fill="#2f3540"/>
      <path d="${WORLD_COUNTRIES}" fill="none" stroke="#4b5363" stroke-width="${r / 6}"/>
      ${lines}
      ${pts.map((p) => `<g><circle class="pulse" cx="${p.x}" cy="${p.y}" r="${r * 2.4}" fill="${p.vpn ? "#b39ddb" : "#4e97fa"}" opacity=".25"/>
        <circle cx="${p.x}" cy="${p.y}" r="${r}" fill="${p.vpn ? "#b39ddb" : "#4e97fa"}" stroke="#fff" stroke-width="${r / 3}"/></g>`).join("")}
    </svg>
    <div class="maplabel"><b>${flag(main.country)} ${esc([main.city, main.region].filter(Boolean).join(", "))}</b>
      <span class="muted">aproximado, pela localização do seu IP${pts.length > 1 ? " · roxo = VPN" : ""}</span></div>
  </div>`;
}

function idCard(i) {
  const g = i.geo_v4 || i.geo_v6 || {};
  const kind = i.kind === "vpn" ? `<span class="pill vpn">VPN</span>` : i.kind === "wifi" ? `<span class="pill wifi">Wi‑Fi</span>` : `<span class="pill gray">${esc(i.kind)}</span>`;
  return `<div class="idcard ${i.kind === "vpn" ? "vpn" : ""}">
    <div style="display:flex;gap:8px;align-items:center">${kind}<b>${esc(i.label)}</b><span class="muted mono">${esc(i.name)}</span></div>
    ${i.public_ipv4 ? `<div class="ip">${esc(i.public_ipv4)}</div>` : ""}
    ${i.public_ipv6 ? `<div class="ip6">IPv6 ${esc(i.public_ipv6)}</div>` : ""}
    ${!i.public_ipv4 && !i.public_ipv6 ? `<div class="ip6">${esc(i.error || "sem IP público")}</div>` : ""}
    <div class="who">${g.country ? `${flag(g.country)} ${esc(g.country_name || g.country)} · ` : ""}${esc(g.org || "")}</div>
    <div class="meta">IP local ${esc(i.local_ips.join(", "))}</div>
  </div>`;
}

async function dashboardFilter() {
  const s = state.status;
  const [stats, conns] = await Promise.all([get("/api/v1/stats"), get("/api/v1/connections?active=true")]);
  const c = s.counters;
  const cls = !s.dns_ok ? "down" : s.paused ? "paused" : "";
  const title = !s.dns_ok ? "Filtro DNS parado" : s.paused ? "Proteção pausada" : "Você está protegido";
  const desc = !s.dns_ok
    ? "O servidor DNS não conseguiu iniciar. Veja os avisos abaixo."
    : s.paused
    ? "Todo o tráfego está liberado até você retomar."
    : `${fmt(s.blocked_domains)} domínios de anúncios, rastreadores e malware bloqueados${s.last_upstream ? ` · resolvendo via ${esc(s.last_upstream)}` : ""}.`;
  const inet = conns.filter((x) => x.scope === "internet").length;
  const apps = new Set(conns.map((x) => x.process.app_path)).size;
  $("#main").innerHTML = `
    <div class="hero ${cls}">
      <div class="shield">${!s.dns_ok ? ICON_X : s.paused ? ICON_PAUSE : ICON_SHIELD}</div>
      <div><h1>${title}</h1><p>${desc}</p></div>
      <div class="spacer"></div>
      <button class="${s.paused ? "primary" : ""}" id="pauseBtn">${s.paused ? "Retomar proteção" : "Pausar"}</button>
    </div>
    <div class="mt">${s.warnings.map((w) => `<div class="warn">${esc(w)}</div>`).join("")}</div>
    <div class="grid g4 mt">
      <div class="card stat"><div class="label">Consultas DNS</div><div class="value">${fmt(c.dns_queries)}</div><div class="hint">${fmt(c.dns_cached)} do cache</div></div>
      <div class="card stat"><div class="label">Bloqueadas</div><div class="value red">${fmt(c.dns_blocked)}</div><div class="hint">${pct(c.dns_blocked, c.dns_queries)} das consultas</div></div>
      <div class="card stat"><div class="label">Conexões ativas</div><div class="value">${fmt(conns.length)}</div><div class="hint">${fmt(inet)} com a internet</div></div>
      <div class="card stat"><div class="label">Apps na rede</div><div class="value">${fmt(apps)}</div><div class="hint">${fmt(c.connections_seen)} conexões vistas</div></div>
    </div>
    <div class="card mt">${chart(stats.per_minute)}</div>
    <div class="grid g2 mt">
      <div class="card"><h2>Mais bloqueados</h2>${topList(stats.top_blocked, true, (k) => `<button class="small ok" data-permit="${esc(k)}">Permitir</button>`)}</div>
      <div class="card"><h2>Domínios mais consultados</h2>${topList(stats.top_domains, false, (k) => `<button class="small danger" data-block="${esc(k)}">Bloquear</button>`)}</div>
    </div>
    <div class="card mt"><h2>Apps com mais conexões agora</h2>${topList(stats.top_apps, false)}</div>`;
  $("#pauseBtn").onclick = async () => {
    await post("/api/v1/pause", { paused: !s.paused });
    refresh();
  };
  bindRuleButtons();
}

// ---------- exposure: who sees my IP ----------
pages.exposure = async (first) => {
  const exp = await get("/api/v1/exposure");
  if (first || !$("#expBody")) {
    $("#main").innerHTML = `
      <div class="page-head"><div><h1>Quem vê seu IP</h1><p class="sub">Todo servidor que recebe uma conexão sua vê um destes endereços. Com VPN, os apps que passam pelo túnel mostram o IP da VPN.</p></div>
        <button id="recheck">Verificar IP de novo</button></div>
      <div class="idgrid" id="ids"></div>
      <div class="page-head mt" style="margin-top:22px"><div class="seg"><button data-v="server">Por servidor</button><button data-v="app">Por app</button><button data-v="packets">Pacotes</button></div>
        <input type="text" id="eq" placeholder="Filtrar por app, servidor, país ou empresa" value="${esc(state.expQ)}" style="min-width:300px"></div>
      <div class="table-wrap" id="expBody"></div>
      <p class="muted" style="font-size:11.5px;margin-top:10px">Países e empresas: <a href="https://db-ip.com" target="_blank">IP Geolocation by DB-IP</a> (CC BY 4.0), consultados localmente.</p>`;
    $("#recheck").onclick = async () => { await post("/api/v1/exposure/recheck"); toast("Consultando seu IP público…"); };
    document.querySelectorAll(".seg button").forEach((b) => (b.onclick = () => { state.expView = b.dataset.v; refresh(); }));
    $("#eq").oninput = (e) => { state.expQ = e.target.value; refresh(); };
  }
  document.querySelectorAll(".seg button").forEach((b) => b.classList.toggle("on", b.dataset.v === state.expView));
  const withIp = exp.interfaces.filter((i) => i.public_ipv4 || i.public_ipv6);
  $("#ids").innerHTML = withIp.length ? withIp.map(idCard).join("") + liveMapCard(withIp) : `<div class="card muted">Descobrindo seu IP público…</div>`;
  const ifLabel = Object.fromEntries(exp.interfaces.map((i) => [i.name, i]));
  const q = state.expQ.toLowerCase();
  exp.servers.forEach((s) => (s.apps = Object.entries(s.apps).map(([path, name]) => ({ path, name }))));
  const servers = exp.servers.filter((s) => !q || [s.host, s.org, s.country_name, ...s.apps.map((a) => a.name)].some((v) => (v || "").toLowerCase().includes(q)));
  const via = (s) => [...s.interfaces].map((n) => {
    const i = ifLabel[n];
    return i && i.kind === "vpn" ? `<span class="pill vpn">${esc(i.label)}</span>` : `<span class="pill wifi">${esc(i ? i.label : n)}</span>`;
  }).join(" ");
  const appIcons = (s) => s.apps.map((a) => avatar(a.name, a.path, true)).join("");

  if (state.expView === "packets") return renderPackets();
  if (state.expView === "server") {
    $("#expBody").innerHTML = servers.length ? `<table><thead><tr><th>Servidor</th><th>País</th><th>Apps</th><th>Seu IP que ele vê</th><th>Saída</th><th>↓ / ↑</th><th>Visto</th></tr></thead><tbody>${servers.map((s) => `<tr class="clickable" data-srv="${esc(s.host)}" data-ips="${esc([...s.ips].join(","))}" title="Clique para ver os pacotes">
      <td class="selectable" style="max-width:300px" title="${esc([...s.ips].join(", "))}"><div class="ellipsis mono" style="max-width:300px">${s.active ? `<span class="dot"></span>` : `<span class="dot off"></span>`}${esc(s.host)}</div><div class="muted ellipsis" style="max-width:300px;font-size:11.5px;margin-left:13px">${esc(s.org || "")}</div></td>
      <td>${s.country ? `${flag(s.country)} ${esc(s.country_name || s.country)}` : `<span class="muted">—</span>`}</td>
      <td><span class="apps-inline" title="${esc(s.apps.map((a) => a.name).join(", "))}">${appIcons(s)} <span class="muted ellipsis" style="max-width:130px">${esc(s.apps.length === 1 ? s.apps[0].name : s.apps.length + " apps")}</span></span></td>
      <td class="mono selectable">${ipCell(s.exposed_ips)}</td>
      <td>${via(s)}</td>
      <td class="bytes">${fmtBytes(s.bytes_in)} / ${fmtBytes(s.bytes_out)}</td>
      <td class="muted">${ago(s.last_seen)}</td></tr>`).join("")}</tbody></table>` : `<div class="empty">Nenhuma conexão com a internet ainda.</div>`;
    document.querySelectorAll("[data-srv]").forEach((tr) => (tr.onclick = () => {
      state.pkFilter = { host: tr.dataset.srv, ips: tr.dataset.ips };
      state.expView = "packets";
      refresh();
    }));
  } else {
    const byApp = new Map();
    for (const s of servers) s.apps.forEach(({ name, path: key }) => {
      if (!byApp.has(key)) byApp.set(key, { name, path: key, servers: 0, countries: new Set(), ips: new Set(), ifs: new Set(), vpn: false, bin: 0, bout: 0 });
      const a = byApp.get(key);
      a.servers++;
      if (s.country) a.countries.add(s.country);
      s.exposed_ips.forEach((ip) => a.ips.add(ip));
      s.interfaces.forEach((i) => a.ifs.add(i));
      a.vpn ||= s.via_vpn;
      a.bin += s.bytes_in; a.bout += s.bytes_out;
    });
    const rows = [...byApp.values()].sort((a, b) => b.servers - a.servers);
    $("#expBody").innerHTML = rows.length ? `<table><thead><tr title="Clique num app para ver os pacotes dele"><th>App</th><th>Seu IP que os servidores veem</th><th>Saída</th><th>Servidores</th><th>Países</th><th>↓ / ↑</th></tr></thead><tbody>${rows.map((a) => `<tr class="clickable" data-pkapp="${esc(a.path)}" data-pkname="${esc(a.name)}" title="Clique para ver os pacotes de ${esc(a.name)}">
      <td><span class="apps-inline">${avatar(a.name, a.path, true)} <b>${esc(a.name)}</b></span></td>
      <td class="mono selectable">${ipCell([...a.ips])}</td>
      <td>${via({ interfaces: [...a.ifs] })}</td>
      <td>${fmt(a.servers)}</td>
      <td>${[...a.countries].map(flag).join(" ")}</td>
      <td class="bytes">${fmtBytes(a.bin)} / ${fmtBytes(a.bout)}</td></tr>`).join("")}</tbody></table>` : `<div class="empty">Nenhum app com conexões na internet.</div>`;
    document.querySelectorAll("[data-pkapp]").forEach((tr) => (tr.onclick = () => {
      state.pkFilter = { app: tr.dataset.pkapp, label: tr.dataset.pkname };
      state.expView = "packets";
      refresh();
    }));
  }
};

function bindRuleButtons(root = document) {
  root.querySelectorAll("[data-block]").forEach((b) => (b.onclick = (e) => { e.stopPropagation(); quickRule("block", b.dataset.block); }));
  root.querySelectorAll("[data-permit]").forEach((b) => (b.onclick = (e) => { e.stopPropagation(); quickRule("permit", b.dataset.permit); }));
}

// ---------- packets (Wireshark-style) ----------
const pk = { rows: [], last: 0, key: null, sel: null, paused: false };
function hexdump(hex) {
  const b = hex.match(/../g) || [];
  const lines = [];
  for (let i = 0; i < b.length; i += 16) {
    const chunk = b.slice(i, i + 16);
    const hx = chunk.map((x, k) => x + (k === 7 ? "  " : " ")).join("").padEnd(50);
    const asc = chunk.map((x) => { const c = parseInt(x, 16); return c >= 32 && c < 127 ? String.fromCharCode(c) : "."; }).join("");
    lines.push(`<span class="off">${i.toString(16).padStart(4, "0")}</span>  ${esc(hx)} ${esc(asc)}`);
  }
  return lines.join("\n");
}
function pkClass(p) {
  const i = p.info;
  return [p.outbound ? "out" : "in", i.startsWith("TLS") || i.includes(" TLS") ? "tls" : "", i.startsWith("DNS") ? "dns" : "", p.flags.includes("SYN") ? "syn" : "", p.flags.includes("RST") ? "rst" : ""].join(" ");
}
async function renderPackets() {
  const f = state.pkFilter;
  const key = f ? (f.app ? "app:" + f.app : f.ips) : "";
  if (key !== pk.key) { pk.rows = []; pk.last = 0; pk.key = key; pk.sel = null; }
  if (!pk.paused) {
    const filt = !f ? "" : f.app ? `&app=${encodeURIComponent(f.app)}` : `&ips=${encodeURIComponent(f.ips)}`;
    const fresh = await get(`/api/v1/packets?after=${pk.last}&limit=500${filt}`);
    if (fresh.length) {
      pk.rows = pk.rows.concat(fresh).slice(-800);
      pk.last = fresh[fresh.length - 1].id;
    }
  }
  const q = state.expQ.toLowerCase();
  const rows = q ? pk.rows.filter((p) => [p.info, p.remote, p.app].some((v) => (v || "").toLowerCase().includes(q))) : pk.rows;
  const sel = pk.rows.find((p) => p.id === pk.sel);
  const t0 = pk.rows.length ? pk.rows[0].t : 0;
  $("#expBody").innerHTML = `
    <div class="toolbar" style="padding:10px 12px;border-bottom:1px solid var(--g300)">
      ${f ? `<span class="chip">${f.app ? `App: ${esc(f.label)}` : `Servidor: ${esc(f.host)}`} <button id="pkClear" title="Ver todos">✕</button></span>` : `<span class="muted">Todos os pacotes ao vivo</span>`}
      <span class="muted">${rows.length} pacotes${pk.paused ? " · pausado" : ""}</span>
      <span style="flex:1"></span>
      <button class="small" id="pkPause">${pk.paused ? "▶ Continuar" : "❚❚ Pausar"}</button>
      <button class="small" id="pkClearRows">Limpar</button>
    </div>
    <div class="pk-wrap" style="padding:10px">
      <div class="pk-table" id="pkTable">${rows.length ? `<table><thead><tr><th>Nº</th><th>Tempo</th><th></th><th>Seu endereço</th><th>Servidor</th><th>Proto</th><th>Tam.</th><th>App</th><th>Info</th></tr></thead><tbody>${rows.map((p) => `
        <tr class="${pkClass(p)} ${p.id === pk.sel ? "sel" : ""}" data-pk="${p.id}">
          <td class="muted">${p.id}</td><td class="muted">${(p.t - t0).toFixed(3)}</td><td>${p.outbound ? "→" : "←"}</td>
          <td>${esc(shortIp(p.local.replace(/:\d+$/, "")))}${esc(p.local.match(/:\d+$/)?.[0] || "")}</td>
          <td>${esc(p.remote)}</td><td>${p.proto.toUpperCase()}</td><td>${p.len}</td>
          <td class="muted">${esc(p.app || "")}</td><td>${esc(p.info)}</td></tr>`).join("")}</tbody></table>` : `<div class="empty">${state.status.running_as_root ? "Aguardando pacotes…" : "A captura de pacotes precisa do serviço rodando como root."}</div>`}</div>
      ${sel ? `<div class="pk-detail">
        <div>
          <div><b>Pacote ${sel.id}</b> <span class="muted">${new Date(sel.t * 1000).toLocaleTimeString("pt-BR")}.${String(Math.floor((sel.t % 1) * 1000)).padStart(3, "0")}</span></div>
          <div><span class="k">Direção</span> ${sel.outbound ? "→ enviado por você" : "← recebido"}</div>
          <div><span class="k">Interface</span> ${esc(sel.iface)}</div>
          <div><span class="k">Origem</span> <span class="mono selectable">${esc(sel.outbound ? sel.local : sel.remote)}</span></div>
          <div><span class="k">Destino</span> <span class="mono selectable">${esc(sel.outbound ? sel.remote : sel.local)}</span></div>
          <div><span class="k">Protocolo</span> IPv${(sel.hex[0] || "4")} / ${sel.proto.toUpperCase()}</div>
          <div><span class="k">Tamanho</span> ${sel.len} bytes · <span class="k">TTL</span> ${sel.ttl}</div>
          ${sel.flags ? `<div><span class="k">Flags TCP</span> ${esc(sel.flags)}</div>` : ""}
          ${sel.app ? `<div><span class="k">App</span> ${esc(sel.app)}</div>` : ""}
          <div style="margin-top:6px">${esc(sel.info)}</div>
        </div>
        <div><div class="k" style="margin-bottom:4px">Primeiros ${sel.hex.length / 2} bytes do pacote IP (cabeçalhos + início dos dados; o conteúdo HTTPS é criptografado)</div><pre class="hexdump">${hexdump(sel.hex)}</pre></div>
      </div>` : `<div class="muted" style="font-size:12px">Clique num pacote para ver os detalhes e os bytes.</div>`}
    </div>`;
  document.querySelectorAll("[data-pk]").forEach((tr) => (tr.onclick = () => { pk.sel = Number(tr.dataset.pk); pk.paused = true; refresh(); }));
  $("#pkPause").onclick = () => { pk.paused = !pk.paused; refresh(); };
  $("#pkClearRows").onclick = () => { pk.rows = []; pk.sel = null; refresh(); };
  if ($("#pkClear")) $("#pkClear").onclick = () => { state.pkFilter = null; refresh(); };
  // keep the newest packets in view while live
  const tb = $("#pkTable");
  if (tb && !pk.paused) tb.scrollTop = tb.scrollHeight;
}

// ---------- monitor (app detail, Portmaster-style) ----------
const expanded = new Set();
let ifaceCache = { t: 0, list: [] };
async function interfaces() {
  if (Date.now() - ifaceCache.t > 8000) {
    try { ifaceCache = { t: Date.now(), list: (await get("/api/v1/exposure")).interfaces }; } catch {}
  }
  return ifaceCache.list;
}
function identityOf(ip, ifs) {
  for (const i of ifs) {
    if (i.public_ipv4 === ip) return { iface: i, geo: i.geo_v4 };
    if (i.public_ipv6 === ip) return { iface: i, geo: i.geo_v6 };
  }
  return { iface: null, geo: null };
}
const ifacePill = (i, fallback) => i ? (i.kind === "vpn" ? `<span class="pill vpn">${esc(i.label)}</span>` : `<span class="pill wifi">${esc(i.label)}</span>`) : `<span class="pill gray">${esc(fallback || "?")}</span>`;
const ENCRYPTED_PORTS = new Set([443, 853, 993, 995, 465, 5223, 8443, 22]);

function trafficChart(buckets) {
  const now = Math.floor(Date.now() / 10000) * 10;
  const n = 180; // 30 min
  const map = new Map(buckets.map((b) => [b.t, b]));
  const pts = [];
  for (let i = n - 1; i >= 0; i--) pts.push(map.get(now - i * 10) || { bytes_in: 0, bytes_out: 0 });
  const max = Math.max(1024, ...pts.map((p) => Math.max(p.bytes_in, p.bytes_out)));
  const W = 1000, H = 130;
  const area = (key) => {
    const xy = pts.map((p, i) => [(i / (n - 1)) * W, H - (p[key] / max) * (H - 8)]);
    // smooth with a simple moving average for a softer silhouette
    const sm = xy.map(([x], i) => [x, xy.slice(Math.max(0, i - 2), i + 3).reduce((a, q) => a + q[1], 0) / xy.slice(Math.max(0, i - 2), i + 3).length]);
    return `M0,${H} ` + sm.map(([x, y]) => `L${x.toFixed(1)},${y.toFixed(1)}`).join(" ") + ` L${W},${H} Z`;
  };
  const total = pts.reduce((a, p) => [a[0] + p.bytes_in, a[1] + p.bytes_out], [0, 0]);
  return `<div class="tchart"><svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none">
      <defs><linearGradient id="gIn" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1de966" stop-opacity=".45"/><stop offset="1" stop-color="#1de966" stop-opacity=".02"/></linearGradient>
      <linearGradient id="gOut" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#4e97fa" stop-opacity=".5"/><stop offset="1" stop-color="#4e97fa" stop-opacity=".03"/></linearGradient></defs>
      <path d="${area("bytes_in")}" fill="url(#gIn)" stroke="#1de966" stroke-opacity=".7" stroke-width="1.2" vector-effect="non-scaling-stroke"/>
      <path d="${area("bytes_out")}" fill="url(#gOut)" stroke="#4e97fa" stroke-opacity=".8" stroke-width="1.2" vector-effect="non-scaling-stroke"/></svg>
    <div class="legend"><span><b style="background:#1de966"></b>Recebido ${fmtBytes(total[0])}</span><span><b style="background:#4e97fa"></b>Enviado ${fmtBytes(total[1])}</span><span style="margin-left:auto">últimos 30 min · pico ${fmtBytes(max / 10)}/s</span></div></div>`;
}

function connDetails(c, ifs) {
  const id = c.exposed_ip ? identityOf(c.exposed_ip, ifs) : { iface: ifs.find((i) => i.name === c.interface) };
  const iface = id.iface || ifs.find((i) => i.name === c.interface);
  const g = c.geo || {};
  const enc = ENCRYPTED_PORTS.has(c.remote_port);
  const kv = (k, v) => `<div><span class="k">${k}</span> <span class="v selectable">${v}</span></div>`;
  const egeo = id.geo || (iface && (iface.geo_v4 || iface.geo_v6)) || {};
  return `<div class="cdetail">
    <div class="cgrid">
      <div>
        ${kv("Início", new Date(c.first_seen * 1000).toLocaleTimeString("pt-BR"))}
        ${kv("Fim", c.active ? `<span class="green">ativa</span>` : new Date(c.last_seen * 1000).toLocaleTimeString("pt-BR"))}
        ${kv("Processo", `${esc(c.process.name)} (PID ${c.process.pid})`)}
        ${kv("Endereço local", `<span class="mono">${esc(c.local)}</span>`)}
        ${kv("Binário", `<span class="mono">${esc(c.process.path)}</span>`)}
      </div>
      <div>
        ${kv("Direção", c.inbound ? "↙ Entrada" : "↗ Saída")}
        ${kv("Protocolo", c.protocol.toUpperCase())}
        ${kv("Criptografada", enc ? "sim (provável, porta " + c.remote_port + ")" : "não verificado")}
        ${kv("Pelo túnel VPN", c.via_vpn ? `<span style="color:#b39ddb">sim</span>` : "não")}
        ${kv("Tráfego", `↓ ${fmtBytes(c.bytes_in)} · ↑ ${fmtBytes(c.bytes_out)}`)}
      </div>
      <div>
        ${kv("Domínio", c.domain ? esc(c.domain) : `<span class="muted">desconhecido</span>`)}
        ${kv("Servidor", `${flag(g.country)} <span class="mono">${esc(c.remote)}</span>`)}
        ${kv("País", g.country ? `${esc(g.country_name || g.country)} (${esc(g.country)})` : "—")}
        ${kv("ASN", g.asn ? "AS" + g.asn : "—")}
        ${kv("Empresa", esc(g.org || "—"))}
      </div>
    </div>
    <div class="route">
      <div class="hop"><span class="node"></span><div><b>Seu dispositivo</b><div class="muted mono">${esc(c.local)} · ${esc(c.interface || "?")}</div></div></div>
      ${c.scope === "internet" ? `<div class="hop"><span class="node ${c.via_vpn ? "vpn" : "exit"}"></span><div><b>Saída para a internet</b> ${ifacePill(iface, c.interface)}
        <div class="muted"><span class="mono">${esc(c.exposed_ip || "?")}</span> ${egeo.country ? `· ${flag(egeo.country)} ${esc(egeo.org || "")}` : ""} — é este IP que o servidor vê</div></div></div>` : ""}
      <div class="hop"><span class="node dest"></span><div><b>${esc(c.domain || c.remote_ip)}</b>
        <div class="muted">${flag(g.country)} <span class="mono">${esc(c.remote)}</span>${g.org ? ` · ${esc(g.org)}` : ""}${g.asn ? ` (AS${g.asn})` : ""}</div></div></div>
    </div>
  </div>`;
}

pages.monitor = async (first) => {
  const [apps, conns, ifs] = await Promise.all([get("/api/v1/apps"), get(`/api/v1/connections?active=${state.activeOnly}`), interfaces()]);
  if (first || !$("#applist")) {
    $("#main").innerHTML = `
      <div class="split2">
        <div class="leftcol">
          <div class="search"><input type="text" id="cf" placeholder="Buscar app, domínio ou IP" value="${esc(state.connFilter)}"></div>
          <div class="toolbar" style="margin:8px 2px"><label class="check"><input type="checkbox" id="ao" ${state.activeOnly ? "checked" : ""}> Só ativas</label><span class="muted" id="appcount" style="margin-left:auto"></span></div>
          <div class="applist" id="applist"></div>
        </div>
        <div class="detail" id="detail"></div>
      </div>`;
    $("#cf").oninput = (e) => { state.connFilter = e.target.value; refresh(); };
    $("#ao").onchange = (e) => { state.activeOnly = e.target.checked; refresh(); };
  }
  const q = state.connFilter.toLowerCase();
  const match = (c) => !q || [c.domain, c.remote_ip, c.process.name].some((v) => (v || "").toLowerCase().includes(q));
  const byApp = new Map();
  for (const c of conns.filter(match)) {
    if (!byApp.has(c.process.app_path)) byApp.set(c.process.app_path, []);
    byApp.get(c.process.app_path).push(c);
  }
  const visible = apps.filter((a) => byApp.has(a.path) || (!q && !state.activeOnly));
  if ((!state.selApp || !visible.find((a) => a.path === state.selApp)) && visible.length) state.selApp = visible[0].path;
  $("#appcount").textContent = `${visible.length} apps`;
  const maxN = Math.max(1, ...visible.map((a) => (byApp.get(a.path) || []).length));
  $("#applist").innerHTML = visible.length ? visible.map((a) => {
    const n = (byApp.get(a.path) || []).length;
    return `<div class="app ${a.path === state.selApp ? "sel" : ""}" data-app="${esc(a.path)}">${avatar(a.name, a.path)}
      <div class="meta"><div class="name">${esc(a.name)}</div></div>
      <div class="cbar" title="${n} conexões"><span>${n > 999 ? "1k+" : n}</span><i><b style="width:${(n / maxN) * 100}%"></b></i></div></div>`;
  }).join("") : `<div class="empty">Nenhum app com conexões.</div>`;
  document.querySelectorAll("[data-app]").forEach((el) => (el.onclick = () => { state.selApp = el.dataset.app; refresh(); }));

  const app = apps.find((a) => a.path === state.selApp);
  if (!app) { $("#detail").innerHTML = `<div class="empty">Selecione um app.</div>`; return; }
  const list = (byApp.get(state.selApp) || []).sort((a, b) => (b.active - a.active) || (b.last_seen - a.last_seen));
  const traffic = await get(`/api/v1/traffic?app=${encodeURIComponent(app.path)}`);
  const binary = (list[0] && list[0].process.path) || app.path;

  // identities: which of your IPs this app's servers see
  const idmap = new Map();
  for (const c of list.filter((c) => c.exposed_ip)) {
    const e = idmap.get(c.exposed_ip) || { ip: c.exposed_ip, n: 0, vpn: c.via_vpn, ifname: c.interface };
    e.n++;
    idmap.set(c.exposed_ip, e);
  }
  const ids = [...idmap.values()].sort((a, b) => b.n - a.n);

  $("#detail").innerHTML = `
    <div class="crumb muted">Apps › ${esc(app.name)}</div>
    <div class="dhead">
      ${avatar(app.name, app.path, false, true)}
      <div style="min-width:0;flex:1"><h1 style="margin:0">${esc(app.name)}</h1>
        <div class="kv ellipsis" style="max-width:100%" title="${esc(app.path)}"><span class="k">Caminho</span> <span class="mono selectable">${esc(app.path)}</span></div>
        <div class="kv"><span class="k">Binário</span> <span class="mono selectable">${esc(binary.split("/").pop())}</span></div>
        <div class="kv"><span class="k">Conexões ativas</span> ${list.filter((c) => c.active).length} · <span class="k">no histórico</span> ${fmt(app.total_connections)}</div>
      </div>
      <button class="small" id="reveal">Mostrar no Finder</button>
    </div>
    ${trafficChart(traffic)}
    ${ids.length ? `<div class="cat">${ids.length} ${ids.length === 1 ? "identidade" : "identidades"} · o IP que os servidores deste app veem</div>
      <div class="idents">${ids.map((e) => {
        let { iface, geo } = identityOf(e.ip, ifs);
        // Older IPv6 privacy addresses: same interface, not the current one.
        const temp = !iface && e.ip.includes(":");
        if (!iface) iface = ifs.find((i) => i.name === e.ifname) || null;
        if (!geo && iface) geo = e.ip.includes(":") ? iface.geo_v6 || iface.geo_v4 : iface.geo_v4;
        return `<div class="ident ${e.vpn ? "vpn" : ""}"><div class="mono selectable" title="${esc(e.ip)}">${flag(geo && geo.country)} ${esc(shortIp(e.ip))}</div>
          <div class="row2"><span>${e.n} ${e.n === 1 ? "conexão" : "conexões"}${temp ? ` · <span title="O macOS troca o endereço IPv6 de tempos em tempos por privacidade; conexões antigas seguem no endereço anterior.">IPv6 temporário anterior</span>` : ""}</span>${ifacePill(iface, e.vpn ? "VPN" : "")}</div></div>`;
      }).join("")}</div>` : ""}
    <div class="tabs"><span class="on">Conexões</span><span class="muted">${list.length}</span></div>
    <div class="clist" id="clist">${list.length ? list.map((c) => {
      const open = expanded.has(c.id);
      return `<div class="crow ${open ? "open" : ""}">
        <div class="chead" data-conn="${esc(c.id)}">
          <span class="chev">${open ? "▾" : "›"}</span><span class="dot ${c.active ? "" : "off"}"></span>
          <span class="cname ellipsis mono">${esc(c.domain || c.remote_ip)}</span>
          <span class="cc">${c.geo && c.geo.country ? esc(c.geo.country) : c.scope === "internet" ? "" : "LAN"}</span>
          <span class="cpeer mono">${c.inbound ? "↙" : "↗"} ${esc(c.remote_ip)}</span>
          <span class="cstate muted">${c.active ? "ativa" : "encerrada há " + ago(c.last_seen)}</span>
          <span class="bytes">${fmtBytes(c.bytes_in + c.bytes_out)}</span>
          ${c.via_vpn ? `<span class="pill vpn">VPN</span>` : `<span></span>`}
          ${isFilter() ? `<button class="small danger" data-block="${esc(c.domain || c.remote_ip)}">Bloquear</button>` : ""}
        </div>
        ${open ? connDetails(c, ifs) : ""}
      </div>`;
    }).join("") : `<div class="empty">Sem conexões ${state.activeOnly ? "ativas " : ""}para este app.</div>`}</div>`;
  $("#reveal").onclick = () => tauri.core.invoke("reveal", { path: app.path });
  document.querySelectorAll("[data-conn]").forEach((el) => (el.onclick = () => {
    const id = el.dataset.conn;
    expanded.has(id) ? expanded.delete(id) : expanded.add(id);
    refresh();
  }));
  bindRuleButtons($("#detail"));
};

// ---------- dns ----------
pages.dns = async (first) => {
  if (first || !$("#dnsTable")) {
    $("#main").innerHTML = `
      <div class="page-head"><div><h1>Consultas DNS</h1><p class="sub">${isFilter() ? "Todo domínio que o sistema tenta resolver passa por aqui." : "Nomes que o Mac pediu ao servidor DNS, observados na rede. Apps com DNS criptografado próprio não aparecem aqui."}</p></div>
      <div class="toolbar"><input type="text" id="dq" placeholder="Buscar domínio" value="${esc(state.dnsQ)}">
      <label class="check"><input type="checkbox" id="db" ${state.dnsBlocked ? "checked" : ""}> Só bloqueadas</label>
      <button id="flush">Limpar cache DNS</button></div></div>
      <div class="table-wrap" id="dnsTable"></div>`;
    $("#dq").oninput = (e) => { state.dnsQ = e.target.value; refresh(); };
    $("#db").onchange = (e) => { state.dnsBlocked = e.target.checked; refresh(); };
    $("#flush").onclick = async () => { await post("/api/v1/cache/flush"); toast("Cache DNS limpo"); };
  }
  const ev = await get(`/api/v1/dns?limit=400&blocked=${state.dnsBlocked}&q=${encodeURIComponent(state.dnsQ)}`);
  $("#dnsTable").innerHTML = ev.length
    ? `<table><thead><tr><th>Hora</th><th>Domínio</th><th>Tipo</th><th>Resultado</th><th>Respostas</th><th>Processo</th><th>ms</th><th></th></tr></thead><tbody>${ev
        .map(
          (e) => `<tr><td class="muted mono">${hhmm(e.time)}</td>
          <td class="ellipsis mono selectable" title="${esc(e.domain)}">${esc(e.domain)}</td><td class="muted">${esc(e.qtype)}</td>
          <td>${e.blocked ? `<span class="pill block" title="${esc(e.reason)}">BLOQUEADO</span> <span class="muted">${esc(e.reason)}</span>` : e.reason ? `<span class="pill gray">${esc(e.reason)}</span>` : `<span class="pill ok">${e.cached ? "CACHE" : "OK"}</span>`}</td>
          <td class="ellipsis mono muted" style="max-width:220px" title="${esc(e.answers.join(", "))}">${esc(e.answers.join(", "))}</td>
          <td class="muted ellipsis" style="max-width:160px">${esc(e.process ? e.process.name : "")}</td><td class="muted mono">${e.latency_ms}</td>
          <td class="actions">${!isFilter() ? "" : e.blocked ? `<button class="small ok" data-permit="${esc(e.domain)}">Permitir</button>` : `<button class="small danger" data-block="${esc(e.domain)}">Bloquear</button>`}</td></tr>`
        )
        .join("")}</tbody></table>`
    : `<div class="empty">Nenhuma consulta ainda.</div>`;
  bindRuleButtons($("#dnsTable"));
};

// ---------- filter lists ----------
let cfgCache = null;
async function loadConfig() {
  const r = await get("/api/v1/config");
  cfgCache = r.config;
  return r;
}
async function saveConfig(mutate, msg = "Configuração salva") {
  const { config } = await loadConfig();
  mutate(config);
  try {
    const r = await put("/api/v1/config", config);
    toast(r.restart_required ? "Salvo. Reinicie o serviço para aplicar os endereços." : msg);
  } catch (e) {
    toast(String(e), true);
    throw e;
  }
  refresh();
}

function sw(id, checked) {
  return `<label class="switch"><input type="checkbox" id="${esc(id)}" ${checked ? "checked" : ""}><span></span></label>`;
}

pages.lists = async (first) => {
  const [lists, { config }] = await Promise.all([get("/api/v1/filterlists"), loadConfig()]);
  if (!first && document.activeElement && document.activeElement.tagName === "INPUT") return;
  const cats = [...new Set(lists.map((l) => l.category))];
  const total = lists.filter((l) => l.enabled).reduce((a, l) => a + l.domains, 0);
  $("#main").innerHTML = `
    ${isFilter() ? "" : `<div class="filter-only">As listas só bloqueiam no <b>modo bloqueio</b>. Você está no modo monitor; troque em Configurações.</div>`}
    <div class="page-head"><div><h1>Listas de filtro</h1><p class="sub">${fmt(state.status.blocked_domains)} domínios únicos bloqueados (${fmt(total)} entradas em listas ativas). Atualizadas a cada ${config.filterlist_refresh_hours} h.</p></div>
      <button id="upd">Atualizar agora</button></div>
    <div class="card">
      <div class="row"><div><div class="t">Bloquear subdomínios</div><div class="d">Se <code>ads.exemplo.com</code> está na lista, bloqueia também <code>x.ads.exemplo.com</code>.</div></div>${sw("subs", config.filter_subdomains)}</div>
      <div class="row"><div><div class="t">Verificar CNAMEs</div><div class="d">Bloqueia rastreadores disfarçados de subdomínio do próprio site (CNAME cloaking).</div></div>${sw("cname", config.filter_cname)}</div>
    </div>
    ${cats
      .map(
        (cat) => `<div class="cat">${esc(cat)}</div><div class="card">${lists
          .filter((l) => l.category === cat)
          .map(
            (l) => `<div class="row"><div><div class="t">${esc(l.name)} ${l.enabled && l.domains ? `<span class="pill gray">${fmt(l.domains)}</span>` : ""}</div>
            <div class="d">${esc(l.description)}${l.updated ? ` · atualizada há ${ago(l.updated)}` : l.enabled ? " · baixando…" : ""}</div>
            ${l.error ? `<div class="d red">Erro: ${esc(l.error)}</div>` : ""}</div>${sw("fl-" + l.id, l.enabled)}</div>`
          )
          .join("")}</div>`
      )
      .join("")}`;
  $("#upd").onclick = async () => { await post("/api/v1/filterlists/update"); toast("Atualizando listas…"); };
  $("#subs").onchange = (e) => saveConfig((c) => (c.filter_subdomains = e.target.checked));
  $("#cname").onchange = (e) => saveConfig((c) => (c.filter_cname = e.target.checked));
  lists.forEach((l) => {
    $("#fl-" + CSS.escape(l.id)).onchange = (e) =>
      saveConfig((c) => { c.filter_lists.find((x) => x.id === l.id).enabled = e.target.checked; }, e.target.checked ? `${l.name} ativada` : `${l.name} desativada`);
  });
};

// ---------- rules ----------
pages.rules = async (first) => {
  if (!first) return; // don't clobber the editor while typing
  const { config, rule_errors } = await loadConfig();
  $("#main").innerHTML = `
    ${isFilter() ? "" : `<div class="filter-only">Regras só são aplicadas no <b>modo bloqueio</b>. Você está no modo monitor.</div>`}
    <div class="page-head"><div><h1>Regras</h1><p class="sub">Regras próprias valem antes das listas de filtro. A primeira que combinar decide.</p></div>
      <button class="primary" id="save">Salvar regras</button></div>
    <div class="grid" style="grid-template-columns: 1fr 320px">
      <div><textarea id="rules" spellcheck="false" placeholder="- ads.exemplo.com">${esc(config.rules.join("\n"))}</textarea>
        ${rule_errors.map((e) => `<div class="errorbox">${esc(e)}</div>`).join("")}<div id="rerr"></div></div>
      <div>
        <div class="card"><h2>Sintaxe</h2>
          <table class="mono syntax" style="font-size:12px"><tbody>
          <tr><td>- exemplo.com</td><td class="muted">bloqueia o domínio</td></tr>
          <tr><td>- .exemplo.com</td><td class="muted">domínio e subdomínios</td></tr>
          <tr><td>- *.exemplo.com</td><td class="muted">só subdomínios</td></tr>
          <tr><td>- *track*</td><td class="muted">curinga</td></tr>
          <tr><td>+ exemplo.com</td><td class="muted">permite (passa por cima das listas)</td></tr>
          <tr><td>- 203.0.113.0/24</td><td class="muted">bloqueia IPs (pf, precisa de root)</td></tr>
          <tr><td># comentário</td><td></td></tr></tbody></table></div>
        <div class="card mt"><h2>Testar domínio</h2>
          <div class="toolbar"><input type="text" id="tq" placeholder="tracker.exemplo.com" style="min-width:0;flex:1"><button id="tb">Testar</button></div>
          <div id="tr" class="mt muted"></div></div>
      </div>
    </div>`;
  $("#save").onclick = async () => {
    const lines = $("#rules").value.split("\n").map((l) => l.trim()).filter(Boolean);
    try {
      await saveConfig((c) => (c.rules = lines), `${lines.filter((l) => !l.startsWith("#")).length} regras salvas`);
      $("#rerr").innerHTML = "";
    } catch (e) {
      $("#rerr").innerHTML = `<div class="errorbox">${esc(e)}</div>`;
    }
  };
  const test = async () => {
    const d = $("#tq").value.trim();
    if (!d) return;
    const r = await get(`/api/v1/check?domain=${encodeURIComponent(d)}`);
    $("#tr").innerHTML = r.decision
      ? `<span class="pill ${r.decision.action === "block" ? "block" : "ok"}">${r.decision.action === "block" ? "BLOQUEADO" : "PERMITIDO"}</span> ${esc(r.decision.reason)}`
      : `<span class="pill ok">PERMITIDO</span> nenhuma regra ou lista combina`;
  };
  $("#tb").onclick = test;
  $("#tq").onkeydown = (e) => e.key === "Enter" && test();
};

// ---------- settings ----------
pages.settings = async (first) => {
  if (!first) return;
  const { config } = await loadConfig();
  const s = state.status;
  const yes = (v, a, b) => (v ? `<span class="pill ok">${a}</span>` : `<span class="pill gray">${b}</span>`);
  $("#main").innerHTML = `
    <div class="page-head"><div><h1>Configurações</h1><p class="sub">Serviço iniciado há ${ago(s.started)}.</p></div></div>
    <div class="cat">Modo</div>
    <div class="mode-cards">
      <div class="mode-card ${config.mode === "monitor" ? "on" : ""}" data-mode="monitor"><div class="t">Monitor</div><div class="d">Só observa: quais apps falam com quais servidores, países, empresas, quanto tráfego e qual IP seu cada um vê. Não altera o DNS nem bloqueia nada.</div></div>
      <div class="mode-card ${config.mode === "filter" ? "on" : ""}" data-mode="filter"><div class="t">Monitor + bloqueio</div><div class="d">Tudo do monitor, e ainda bloqueia anúncios, rastreadores e malware. O DNS do Mac passa a apontar para o Portward (é restaurado ao parar).</div></div>
    </div>
    <div class="cat">Estado do sistema</div>
    <div class="card">
      <div class="row"><div><div class="t">Serviço como root</div><div class="d">Sem root, só os seus processos ficam visíveis e regras de IP não são aplicadas.</div></div>${yes(s.running_as_root, "SIM", "NÃO")}</div>
      <div class="row"><div><div class="t">DNS do sistema aponta para o Portward</div><div class="d">Servidor em <code>${esc(s.dns_listen)}</code>. Ao parar o serviço, o DNS original é restaurado.</div></div>${yes(s.system_dns_managed, "SIM", "NÃO")}</div>
      <div class="row"><div><div class="t">Firewall de pacotes (pf)</div><div class="d">Usado para regras de IP, no anchor <code>com.apple/portward</code>.</div></div>${yes(s.pf_active, "ATIVO", "INATIVO")}</div>
    </div>
    <div class="cat">Resolução de nomes</div>
    <div class="card">
      <div class="row"><div><div class="t">Servidores DNS (em ordem)</div><div class="d">DNS-over-HTTPS criptografa suas consultas. Um por linha: <code>nome | doh | https://… | ip,ip</code> ou <code>nome | udp | 1.1.1.1:53</code>.</div></div></div>
      <textarea id="ups" spellcheck="false" style="min-height:110px">${esc(config.upstreams.map((u) => [u.name, u.kind, u.address, (u.bootstrap || []).join(",")].filter((x, i) => i < 3 || x).join(" | ")).join("\n"))}</textarea>
      <div class="row"><div><div class="t">Resposta para domínios bloqueados</div><div class="d"><b>0.0.0.0</b> falha na hora; <b>NXDOMAIN</b> diz que o domínio não existe.</div></div>
        <select id="br"><option value="zero_ip" ${config.block_response === "zero_ip" ? "selected" : ""}>0.0.0.0 / ::</option><option value="nxdomain" ${config.block_response === "nxdomain" ? "selected" : ""}>NXDOMAIN</option></select></div>
    </div>
    <div class="cat">Histórico</div>
    <div class="card">
      <div class="row"><div><div class="t">Guardar histórico por</div><div class="d">Conexões e consultas ficam num banco SQLite local, nunca saem da máquina.</div></div>
        <div class="toolbar"><input type="number" id="hd" min="1" max="365" value="${config.history_days}" style="width:80px"> dias</div></div>
      <div class="row"><div><div class="t">Aplicar regras de IP com pf</div><div class="d">Desligue se outro firewall já gerencia o pf.</div></div>${sw("eip", config.enforce_ip_rules)}</div>
    </div>
    <div class="toolbar mt"><button class="primary" id="saveS">Salvar configurações</button><button id="flushS">Limpar cache DNS</button></div>
    <div id="serr"></div>`;
  document.querySelectorAll("[data-mode]").forEach((el) => (el.onclick = async () => {
    const mode = el.dataset.mode;
    if (mode === config.mode) return;
    if (mode === "filter" && !confirm("Ativar o bloqueio? O DNS do Mac passará a apontar para o Portward enquanto o serviço estiver rodando.")) return;
    const { config: c } = await loadConfig();
    c.mode = mode;
    await put("/api/v1/config", c);
    toast("Trocando de modo, o serviço reinicia em alguns segundos…");
    setTimeout(() => go("dashboard"), 4000);
  }));
  $("#flushS").onclick = async () => { await post("/api/v1/cache/flush"); toast("Cache DNS limpo"); };
  $("#saveS").onclick = async () => {
    try {
      const ups = $("#ups").value.split("\n").map((l) => l.trim()).filter(Boolean).map((l) => {
        const [name, kind, address, boot] = l.split("|").map((x) => (x || "").trim());
        if (!name || !["doh", "udp"].includes(kind) || !address) throw new Error(`Linha inválida: ${l}`);
        return { name, kind, address, bootstrap: boot ? boot.split(",").map((x) => x.trim()).filter(Boolean) : [] };
      });
      await saveConfig((c) => {
        c.upstreams = ups;
        c.block_response = $("#br").value;
        c.history_days = Math.max(1, parseInt($("#hd").value, 10) || 7);
        c.enforce_ip_rules = $("#eip").checked;
      });
      $("#serr").innerHTML = "";
    } catch (e) {
      $("#serr").innerHTML = `<div class="errorbox">${esc(e.message || e)}</div>`;
    }
  };
};

go("dashboard");
setTimeout(() => { const sp = document.getElementById("splash"); if (sp) sp.remove(); }, 750);
