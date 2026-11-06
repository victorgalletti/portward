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
function avatar(name, path, small = false) {
  const url = path && icons.get(path);
  const cls = `avatar${small ? " sm" : ""}`;
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
  } catch (e) {
    state.down = String(e);
    renderDown();
  }
  const slow = ["rules", "settings", "lists"].includes(state.page);
  timer = setTimeout(refresh, slow ? 5000 : 1500);
}

function renderSide() {
  const s = state.status;
  if (s.mode === "monitor") {
    $("#side-status").innerHTML = `<b style="color:var(--blue)">Modo monitor</b><br>${s.capture_interfaces.length ? `observando ${esc(s.capture_interfaces.join(", "))}` : "sem captura"}<br><span class="muted">v${esc(s.version)}</span>`;
    return;
  }
  const st = !s.dns_ok ? `<span class="red">DNS parado</span>` : s.paused ? `<span class="yellow">Pausado</span>` : `<span class="green">Protegendo</span>`;
  $("#side-status").innerHTML = `<b>${st}</b><br>${fmt(s.counters.dns_blocked)} bloqueios<br><span class="muted">v${esc(s.version)}</span>`;
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
    <div class="idgrid">${ids.length ? ids.map(idCard).join("") : `<div class="card muted">Descobrindo seu IP público…</div>`}</div>
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
      <div class="page-head mt" style="margin-top:22px"><div class="seg"><button data-v="server">Por servidor</button><button data-v="app">Por app</button></div>
        <input type="text" id="eq" placeholder="Filtrar por app, servidor, país ou empresa" value="${esc(state.expQ)}" style="min-width:300px"></div>
      <div class="table-wrap" id="expBody"></div>
      <p class="muted" style="font-size:11.5px;margin-top:10px">Países e empresas: <a href="https://db-ip.com" target="_blank">IP Geolocation by DB-IP</a> (CC BY 4.0), consultados localmente.</p>`;
    $("#recheck").onclick = async () => { await post("/api/v1/exposure/recheck"); toast("Consultando seu IP público…"); };
    document.querySelectorAll(".seg button").forEach((b) => (b.onclick = () => { state.expView = b.dataset.v; refresh(); }));
    $("#eq").oninput = (e) => { state.expQ = e.target.value; refresh(); };
  }
  document.querySelectorAll(".seg button").forEach((b) => b.classList.toggle("on", b.dataset.v === state.expView));
  const withIp = exp.interfaces.filter((i) => i.public_ipv4 || i.public_ipv6);
  $("#ids").innerHTML = withIp.length ? withIp.map(idCard).join("") : `<div class="card muted">Descobrindo seu IP público…</div>`;
  const ifLabel = Object.fromEntries(exp.interfaces.map((i) => [i.name, i]));
  const q = state.expQ.toLowerCase();
  exp.servers.forEach((s) => (s.apps = Object.entries(s.apps).map(([path, name]) => ({ path, name }))));
  const servers = exp.servers.filter((s) => !q || [s.host, s.org, s.country_name, ...s.apps.map((a) => a.name)].some((v) => (v || "").toLowerCase().includes(q)));
  const via = (s) => [...s.interfaces].map((n) => {
    const i = ifLabel[n];
    return i && i.kind === "vpn" ? `<span class="pill vpn">${esc(i.label)}</span>` : `<span class="pill wifi">${esc(i ? i.label : n)}</span>`;
  }).join(" ");
  const appIcons = (s) => s.apps.map((a) => avatar(a.name, a.path, true)).join("");

  if (state.expView === "server") {
    $("#expBody").innerHTML = servers.length ? `<table><thead><tr><th>Servidor</th><th>País</th><th>Apps</th><th>Seu IP que ele vê</th><th>Saída</th><th>↓ / ↑</th><th>Visto</th></tr></thead><tbody>${servers.map((s) => `<tr>
      <td class="selectable" style="max-width:300px" title="${esc([...s.ips].join(", "))}"><div class="ellipsis mono" style="max-width:300px">${s.active ? `<span class="dot"></span>` : `<span class="dot off"></span>`}${esc(s.host)}</div><div class="muted ellipsis" style="max-width:300px;font-size:11.5px;margin-left:13px">${esc(s.org || "")}</div></td>
      <td>${s.country ? `${flag(s.country)} ${esc(s.country_name || s.country)}` : `<span class="muted">—</span>`}</td>
      <td><span class="apps-inline" title="${esc(s.apps.map((a) => a.name).join(", "))}">${appIcons(s)} <span class="muted ellipsis" style="max-width:130px">${esc(s.apps.length === 1 ? s.apps[0].name : s.apps.length + " apps")}</span></span></td>
      <td class="mono selectable">${ipCell(s.exposed_ips)}</td>
      <td>${via(s)}</td>
      <td class="bytes">${fmtBytes(s.bytes_in)} / ${fmtBytes(s.bytes_out)}</td>
      <td class="muted">${ago(s.last_seen)}</td></tr>`).join("")}</tbody></table>` : `<div class="empty">Nenhuma conexão com a internet ainda.</div>`;
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
    $("#expBody").innerHTML = rows.length ? `<table><thead><tr><th>App</th><th>Seu IP que os servidores veem</th><th>Saída</th><th>Servidores</th><th>Países</th><th>↓ / ↑</th></tr></thead><tbody>${rows.map((a) => `<tr>
      <td><span class="apps-inline">${avatar(a.name, a.path, true)} <b>${esc(a.name)}</b></span></td>
      <td class="mono selectable">${ipCell([...a.ips])}</td>
      <td>${via({ interfaces: [...a.ifs] })}</td>
      <td>${fmt(a.servers)}</td>
      <td>${[...a.countries].map(flag).join(" ")}</td>
      <td class="bytes">${fmtBytes(a.bin)} / ${fmtBytes(a.bout)}</td></tr>`).join("")}</tbody></table>` : `<div class="empty">Nenhum app com conexões na internet.</div>`;
  }
};

function bindRuleButtons(root = document) {
  root.querySelectorAll("[data-block]").forEach((b) => (b.onclick = (e) => { e.stopPropagation(); quickRule("block", b.dataset.block); }));
  root.querySelectorAll("[data-permit]").forEach((b) => (b.onclick = (e) => { e.stopPropagation(); quickRule("permit", b.dataset.permit); }));
}

// ---------- monitor ----------
pages.monitor = async (first) => {
  const [apps, conns] = await Promise.all([get("/api/v1/apps"), get(`/api/v1/connections?active=${state.activeOnly}`)]);
  if (first || !$("#applist")) {
    $("#main").innerHTML = `
      <div class="page-head"><div><h1>Atividade de rede</h1><p class="sub">Cada app e as conexões que ele abre, em tempo real.</p></div>
        <div class="toolbar"><input type="text" id="cf" placeholder="Filtrar por domínio, IP ou app" value="${esc(state.connFilter)}">
        <label class="check"><input type="checkbox" id="ao" ${state.activeOnly ? "checked" : ""}> Só ativas</label></div></div>
      <div class="split"><div class="card applist" id="applist"></div><div class="card conns" id="conns"></div></div>`;
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
  if (!state.selApp && visible.length) state.selApp = visible[0].path;

  $("#applist").innerHTML = visible.length
    ? visible
        .map((a) => {
          const n = (byApp.get(a.path) || []).length;
          return `<div class="app ${a.path === state.selApp ? "sel" : ""}" data-app="${esc(a.path)}">${avatar(a.name, a.path)}
            <div class="meta"><div class="name">${esc(a.name)}</div><div class="info">${n} ${n === 1 ? "conexão" : "conexões"} · ${fmt(a.total_connections)} no histórico</div></div>
            ${a.active_connections ? `<span class="dot"></span>` : `<span class="dot off"></span>`}</div>`;
        })
        .join("")
    : `<div class="empty">Nenhum app com conexões.</div>`;
  document.querySelectorAll("[data-app]").forEach((el) => (el.onclick = () => { state.selApp = el.dataset.app; refresh(); }));

  const app = apps.find((a) => a.path === state.selApp);
  const list = (byApp.get(state.selApp) || []).sort((a, b) => b.last_seen - a.last_seen);
  if (!app) {
    $("#conns").innerHTML = `<div class="empty">Selecione um app.</div>`;
    return;
  }
  $("#conns").innerHTML = `
    <div class="head">${avatar(app.name, app.path)}<div style="min-width:0;flex:1"><h2 style="margin:0">${esc(app.name)}</h2>
      <div class="muted mono ellipsis" style="max-width:100%">${esc(app.path)}</div></div>
      <button class="small" id="reveal">Mostrar no Finder</button></div>
    <div class="body">${
      list.length
        ? `<table><thead><tr><th>Destino</th><th>IP</th><th>País / empresa</th><th>Porta</th><th>Seu IP visto</th><th>↓ / ↑</th><th>Estado</th><th>Visto</th><th></th></tr></thead><tbody>${list
            .map(
              (c) => `<tr>
          <td class="ellipsis selectable" style="max-width:240px">${c.blocked ? `<span class="pill block">BLOQ</span> ` : ""}${c.inbound ? `<span class="pill blue">ENTRADA</span> ` : ""}${c.domain ? esc(c.domain) : `<span class="muted">sem nome</span>`}</td>
          <td class="mono selectable">${esc(c.remote_ip)}</td>
          <td class="ellipsis" style="max-width:200px">${c.geo ? `${flag(c.geo.country)} ${esc(c.geo.org || c.geo.country_name || "")}` : `<span class="pill blue">${esc(c.scope)}</span>`}</td>
          <td class="mono">${c.protocol.toUpperCase()} ${c.remote_port}</td>
          <td class="mono">${c.exposed_ip ? `${c.via_vpn ? `<span class="pill vpn">VPN</span> ` : ""}${ipCell([c.exposed_ip])}` : `<span class="muted">—</span>`}</td>
          <td class="bytes">${fmtBytes(c.bytes_in)} / ${fmtBytes(c.bytes_out)}</td>
          <td class="muted">${esc(c.state || (c.active ? "ativa" : "encerrada"))}</td><td class="muted">${ago(c.last_seen)}</td>
          <td class="actions">${!isFilter() ? "" : c.domain ? `<button class="small danger" data-block="${esc(c.domain)}">Bloquear domínio</button>` : `<button class="small danger" data-block="${esc(c.remote_ip)}">Bloquear IP</button>`}</td></tr>`
            )
            .join("")}</tbody></table>`
        : `<div class="empty">Sem conexões ${state.activeOnly ? "ativas " : ""}para este app.</div>`
    }</div>`;
  $("#reveal").onclick = () => tauri.core.invoke("reveal", { path: app.path });
  bindRuleButtons($("#conns"));
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
          <table class="mono" style="font-size:12px"><tbody>
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
