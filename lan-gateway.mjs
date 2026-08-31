#!/usr/bin/env node
// lan-gateway.mjs — DSHLauncher 局域网共享网关（零依赖 Node.js 实现）
// ---------------------------------------------------------------------------
// 职责：把本机 dsh web（默认仅监听 127.0.0.1）安全地暴露给同一 WiFi 下的移动设备。
//   - 只绑定启动器指定的【具体局域网 IP】（DSH_LAN_HOST），绝不绑定 0.0.0.0；
//   - PIN/Token 门禁：首次访问必须输入密码，验证通过写入 HttpOnly Cookie；
//   - 简单速率限制：按来源 IP 滑窗计数，防止同网段设备刷接口；
//   - 反向代理到 DSH_TARGET，自动完成 dsh 启动令牌兑换（进程级 Cookie），
//     支持 SSE 流式响应与 WebSocket Upgrade 透传；
//   - PWA 增强：注入 manifest / service worker / 图标（HTTP 局域网下 SW 受浏览器
//     安全限制时自动降级，iOS 添加到主屏幕不受影响）。
// 运行方式：node lan-gateway.mjs（配置全部来自环境变量，无任何外部依赖）。
// ---------------------------------------------------------------------------

import http from 'node:http';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const env = process.env;
const HOST = env.DSH_LAN_HOST || '127.0.0.1';
const PORT = Number(env.DSH_LAN_PORT || 3081);
const TARGET = env.DSH_TARGET || 'http://127.0.0.1:3080';
const PIN_FILE = env.DSH_LAN_PIN_FILE || '';
const TOKEN_FILE = env.DSH_LAN_TOKEN_FILE || '';
const SECRET_FILE = env.DSH_LAN_SECRET_FILE || '';
const LOG_FILE = env.DSH_LAN_LOG || '';
const COOKIE_NAME = 'dsh_lan';
const COOKIE_MAX_AGE = 30 * 24 * 3600; // 30 天
const PIN_RATE = { limit: Number(env.DSH_LAN_LOGIN_LIMIT || 10), windowMs: 60000 };
const API_RATE = { limit: Number(env.DSH_LAN_API_LIMIT || 180), windowMs: 60000 };
const TOTAL_RATE = { limit: Number(env.DSH_LAN_TOTAL_LIMIT || 900), windowMs: 60000 };
// PWA 图标：官方 DeepSeek 鲸鱼 LOGO（256x256 PNG，由启动器注入）
const ICON_B64 = env.DSH_LAN_ICON_B64 || '';

function log(line) {
  const ts = new Date().toISOString();
  const msg = '[lan-gateway] ' + ts + ' ' + line;
  try { console.log(msg); } catch { /* ignore */ }
  if (LOG_FILE) {
    try { fs.appendFileSync(LOG_FILE, msg + '\n'); } catch { /* ignore */ }
  }
}
function fail(msg) { log('FATAL ' + msg); process.exit(1); }

if (!env.DSH_LAN_HOST) fail('缺少环境变量 DSH_LAN_HOST（局域网 IP），拒绝绑定 0.0.0.0');
function validIpv4(s) {
  const p = String(s).split('.');
  if (p.length !== 4) return false;
  for (let i = 0; i < 4; i++) {
    if (!/^\d{1,3}$/.test(p[i])) return false;
    const n = Number(p[i]);
    if (n < 0 || n > 255) return false;
  }
  return true;
}
if (!validIpv4(HOST)) fail('DSH_LAN_HOST 不是合法 IPv4 地址: ' + HOST);

// ---------------- PIN 解析（禁止硬编码：环境变量 -> PIN 文件） ----------------
function readPin() {
  if (env.DSH_LAN_PIN && env.DSH_LAN_PIN.length > 0) return env.DSH_LAN_PIN;
  if (PIN_FILE) {
    try {
      const s = fs.readFileSync(PIN_FILE, 'utf8').trim();
      if (s.length > 0) return s;
    } catch { /* not yet created */ }
  }
  return '';
}
function pinOk(input) {
  const pin = readPin();
  if (!pin || typeof input !== 'string' || input.length === 0) return false;
  const a = crypto.createHash('sha256').update(input, 'utf8').digest();
  const b = crypto.createHash('sha256').update(pin, 'utf8').digest();
  return crypto.timingSafeEqual(a, b);
}

// ---------------- 会话签名（持久化随机密钥，重启后 Cookie 仍有效） ----------------
function loadSecret() {
  if (SECRET_FILE) {
    try {
      const s = fs.readFileSync(SECRET_FILE, 'utf8').trim();
      if (s.length >= 16) return s;
    } catch { /* generate */ }
    try {
      fs.mkdirSync(path.dirname(SECRET_FILE), { recursive: true });
      const s = crypto.randomBytes(32).toString('base64url');
      fs.writeFileSync(SECRET_FILE, s, { mode: 0o600 });
      return s;
    } catch (e) { log('无法写入会话密钥文件: ' + e.message); }
  }
  return crypto.randomBytes(32).toString('base64url'); // 无文件时每次启动生成（重启需重新输入 PIN）
}
const SECRET = loadSecret();
function signCookie() {
  const exp = Math.floor(Date.now() / 1000) + COOKIE_MAX_AGE;
  const sig = crypto.createHmac('sha256', SECRET).update(String(exp)).digest('base64url');
  return exp + '.' + sig;
}
function cookieOk(value) {
  if (typeof value !== 'string') return false;
  const parts = value.split('.');
  if (parts.length !== 2) return false;
  const exp = Number(parts[0]);
  if (!Number.isFinite(exp) || exp < Date.now() / 1000) return false;
  const expect = crypto.createHmac('sha256', SECRET).update(String(exp)).digest('base64url');
  const a = Buffer.from(parts[1] || '');
  const b = Buffer.from(expect);
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}
function readCookies(req) {
  const out = {};
  const h = req.headers.cookie;
  if (!h) return out;
  for (const seg of h.split(';')) {
    const i = seg.indexOf('=');
    if (i < 0) continue;
    out[seg.slice(0, i).trim()] = seg.slice(i + 1).trim();
  }
  return out;
}
function isAuthed(req) {
  const c = readCookies(req)[COOKIE_NAME];
  return c !== undefined && cookieOk(c);
}

// ---------------- 速率限制（按来源 IP 滑窗计数） ----------------
const buckets = new Map();
let lastPrune = Date.now();
function rateLimit(ip, key, cfg) {
  const now = Date.now();
  if (now - lastPrune > 120000) {
    lastPrune = now;
    for (const [id, b] of buckets) if (b.reset < now) buckets.delete(id);
  }
  const id = ip + '|' + key;
  let b = buckets.get(id);
  if (!b || b.reset < now) { b = { count: 0, reset: now + cfg.windowMs }; buckets.set(id, b); }
  b.count += 1;
  const ok = b.count <= cfg.limit;
  return { ok, retryAfter: Math.max(1, Math.ceil((b.reset - now) / 1000)) };
}
function clientIp(req) {
  // 默认只使用 TCP 直连对端地址：X-Forwarded-For 由客户端完全可控，
  // 直接信任会让速率限制（唯一防线）被伪造 IP 绕过 → PIN 可被暴力破解。
  // 仅当网关确实部署在可信反向代理之后，且显式设置 DSH_LAN_TRUST_PROXY=1 时才读取该头。
  if (env.DSH_LAN_TRUST_PROXY === '1') {
    const fwd = req.headers['x-forwarded-for'];
    if (fwd) { const first = String(fwd).split(',')[0].trim(); if (first) return first; }
  }
  const sock = req.socket;
  return sock && sock.remoteAddress ? String(sock.remoteAddress).replace(/^::ffff:/, '') : 'unknown';
}

// ---------------- dsh 启动令牌兑换（进程级 Cookie 罐） ----------------
let dshCookie = '';
function readToken() {
  if (env.DSH_LAN_TOKEN && env.DSH_LAN_TOKEN.length > 0) return env.DSH_LAN_TOKEN;
  if (TOKEN_FILE) { try { return fs.readFileSync(TOKEN_FILE, 'utf8').trim(); } catch { /* ignore */ } }
  return '';
}
function extractAuthCookie(setCookieHeaders) {
  if (!setCookieHeaders) return '';
  const list = Array.isArray(setCookieHeaders) ? setCookieHeaders : [setCookieHeaders];
  for (const h of list) {
    const name = String(h).split('=')[0].trim();
    if (name.startsWith('dsh-auth-')) return String(h).split(';')[0].trim();
  }
  return '';
}
let lastExchangeAttempt = 0;
async function exchangeToken() {
  const nowMs = Date.now();
  if (nowMs - lastExchangeAttempt < 10000) return false; // 失败退避：10 秒内不重复尝试
  lastExchangeAttempt = nowMs;
  const token = readToken();
  if (!token) return false;
  try {
    const base = TARGET.endsWith('/') ? TARGET.slice(0, -1) : TARGET;
    const res = await fetch(base + '/?token=' + encodeURIComponent(token), {
      redirect: 'manual',
      signal: AbortSignal.timeout(10000) // 目标挂起时 10 秒超时，避免兑换流程永久阻塞
    });
    const sc = typeof res.headers.getSetCookie === 'function'
      ? res.headers.getSetCookie()
      : (res.headers.get('set-cookie') ? [res.headers.get('set-cookie')] : []);
    const c = extractAuthCookie(sc);
    if (c) { dshCookie = c; log('dsh 令牌兑换成功（进程 Cookie 已就绪）'); return true; }
    log('令牌兑换未获得 Cookie（HTTP ' + res.status + '）');
  } catch (e) { log('令牌兑换失败: ' + e.message); }
  return false;
}
async function ensureDshCookie() {
  if (dshCookie) return true;
  return exchangeToken();
}

// ---------------- PWA：manifest / service worker / 图标 / HTML 注入 ----------------
const MANIFEST = {
  name: 'DeepSeek Harness',
  short_name: 'DSH',
  description: 'DeepSeek Harness 局域网版',
  start_url: '/',
  scope: '/',
  display: 'standalone',
  orientation: 'any',
  background_color: '#0f1115',
  theme_color: '#0f1115',
  icons: [
    { src: '/__lan/icon-192.png', sizes: '192x192', type: 'image/png' },
    { src: '/__lan/icon-512.png', sizes: '512x512', type: 'image/png' },
    { src: '/__lan/icon-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' }
  ]
};
// SW 版本随机化：每次网关重启（如重新生成 PIN）产生新缓存名，SW 内容变化 →
// 手机浏览器自动检测并更新 SW → 自动清除旧缓存 → 拿到最新 UI，无需手动清缓存
const SW_VERSION = 'dsh-lan-' + crypto.randomBytes(6).toString('hex');
const SERVICE_WORKER = 'const CACHE = "' + SW_VERSION + '";\n' +
  'self.addEventListener("install", (e) => { self.skipWaiting(); });\n' +
  'self.addEventListener("activate", (e) => { e.waitUntil(Promise.all([\n' +
  '  self.clients.claim(),\n' +
  '  self.clients.matchAll({includeUncontrolled:true}).then(function(cs){cs.forEach(function(c){c.navigate(c.url).catch(function(){})})}),\n' +
  '  caches.keys().then(function(keys){return Promise.all(keys.filter(function(k){return k!==CACHE}).map(function(k){return caches.delete(k)}))})\n' +
  '])); });\n' +
  'self.addEventListener("fetch", (e) => {\n' +
  '  const url = new URL(e.request.url);\n' +
  '  if (e.request.method !== "GET" || url.pathname.startsWith("/__lan/") || url.pathname.startsWith("/api/")) return;\n' +
  '  e.respondWith(\n' +
  '    fetch(e.request).then((res) => {\n' +
  '      const ct = res.headers ? String(res.headers.get("content-type") || "") : "";\n' +
  '      // SSE/流式响应不缓存：res.clone() 对永不结束的流会持续挂起，断网回放还会拿到过期数据\n' +
  '      if (res && res.ok && ct.indexOf("text/event-stream") < 0) caches.open(CACHE).then((c) => c.put(e.request, res.clone())).catch(function(){});\n' +
  '      return res;\n' +
  '    }).catch(() => caches.match(e.request))\n' +
  '  );\n' +
  '});\n';

const PWA_HEAD = '<style>html{-webkit-text-size-adjust:100%}input,textarea,select,button{font-size:16px}button,a,[role=button]{touch-action:manipulation}</style>' +
  '<meta name="apple-mobile-web-app-capable" content="yes">' +
  '<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">' +
  '<link rel="manifest" href="/__lan/manifest.webmanifest">' +
  '<link rel="apple-touch-icon" href="/__lan/icon-192.png">' +
  '<script>(function(){try{if("serviceWorker" in navigator){navigator.serviceWorker.register("/__lan/sw.js",{updateViaCache:"none"}).then(function(r){if(r&&r.update)r.update()}).catch(function(){})}}catch(e){}})();</script>' +
  '<script>window.__dshLanMobileLite = function(){var IS_WV=!!(window.chrome&&window.chrome.webview);var MOBILE=!IS_WV&&(/Mobile|Android|iPhone|iPad|iPod/i.test(navigator.userAgent)||(window.matchMedia&&window.matchMedia(\"(pointer: coarse)\").matches)||(navigator.maxTouchPoints>0&&window.innerWidth<=1024));if(!MOBILE)return;var hide=function(){try{var q=function(s){return Array.from(document.querySelectorAll(s))};q("button[aria-label=\\"新建会话\\"]").forEach(function(b){b.style.display="none"});q("button[aria-label=\\"添加工作区\\"]").forEach(function(b){b.style.display="none"});q("button[aria-label=\\"选择工作区\\"]").forEach(function(b){b.style.display="none"});}catch(e){}};hide();var mo=new MutationObserver(hide);try{mo.observe(document.documentElement,{childList:true,subtree:true})}catch(e){};setInterval(hide,2000);};window.__dshLanMobileLite();</script>';
function injectPwa(html) {
  if (!/<head[^>]*>/i.test(html)) return html;
  // 强制 1280px 桌面视口：手机浏览器以桌面宽度渲染，dsh 前端恢复完整 UI
  // （会话侧栏 / 工作区选择器 / 聊天记录同步），否则窄视口触发移动布局导致界面残缺。
  html = html.replace(/<meta[^>]*name=["']viewport["'][^>]*>/gi, '');
  const head = '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">'
    + '<meta name="dsh-lan-version" content="' + SW_VERSION + '">' + PWA_HEAD;
  return html.replace(/<head[^>]*>/i, (m) => m + head);
}

// ---------------- 移动端专属 UI（/__lan/m）：全新设计，独立于 dsh 原生界面 ----------------
const MOBILE_UI = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="apple-mobile-web-app-capable" content="yes">
<title>DeepSeek Harness · 手机端</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
:root{--bg:#0e1014;--card:#161a22;--card2:#1c212b;--text:#e8ebf2;--sub:#8b93a5;--accent:#4d6bfe;--line:rgba(255,255,255,.07);--user:#2f5bff}
html,body{height:100%}
body{background:var(--bg);color:var(--text);font-family:system-ui,-apple-system,"PingFang SC","Microsoft YaHei",sans-serif;-webkit-text-size-adjust:100%;display:flex;flex-direction:column}
header{position:sticky;top:0;z-index:10;background:rgba(14,16,20,.96);backdrop-filter:blur(10px);padding:14px 16px 12px;border-bottom:1px solid var(--line)}
header h1{font-size:19px;font-weight:700}
header p{font-size:13px;color:var(--sub);margin-top:3px}
#list{padding:10px 12px 80px;overflow-y:auto;flex:1}
.group{margin-bottom:10px;border:1px solid var(--line);border-radius:14px;background:var(--card);overflow:hidden}
.groupHead{display:flex;align-items:center;gap:8px;padding:13px 14px;cursor:pointer;background:rgba(255,255,255,.02)}
.groupHead:active{background:var(--card2)}
.gName{font-size:15px;font-weight:600;flex:1;line-height:1.35;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;word-break:break-all}
.gCount{font-size:12px;color:var(--sub);background:rgba(255,255,255,.06);border-radius:20px;padding:2px 9px}
.gArrow{font-size:13px;color:var(--sub);transition:transform .15s}
.group.collapsed .gArrow{transform:rotate(-90deg)}
.group.collapsed .groupBody{display:none}
.groupBody{padding:2px 10px 10px}
.card{background:var(--card2);border:1px solid var(--line);border-radius:12px;padding:13px 14px;margin-top:8px;min-height:54px;display:flex;flex-direction:column;gap:6px;cursor:pointer}
.card:active{background:#242a37}
.card .title{font-size:15.5px;line-height:1.45;font-weight:500;word-break:break-word;overflow-wrap:anywhere}
.card .meta{font-size:12px;color:var(--sub);display:flex;gap:8px;align-items:center;flex-wrap:wrap}
.card .run{color:#ffb454;font-size:11.5px}
.status{text-align:center;color:var(--sub);padding:60px 24px;font-size:15px;line-height:1.9}
.err{color:#ff7a7a}
footer{position:sticky;bottom:0;padding:10px 16px calc(10px + env(safe-area-inset-bottom));background:rgba(14,16,20,.96);border-top:1px solid var(--line);text-align:center;font-size:12px;color:var(--sub)}
footer b{color:#9aa7ff;font-weight:500}
/* 聊天 */
#chatView{display:none;flex-direction:column;flex:1;min-height:0}
#chatHeader{display:flex;align-items:center;gap:8px;padding:10px 12px;border-bottom:1px solid var(--line);background:rgba(14,16,20,.96)}
#backBtn{background:none;border:1px solid var(--line);color:var(--text);border-radius:10px;padding:8px 12px;font-size:14px;flex:none}
#chatTitle{font-size:14.5px;font-weight:600;flex:1;line-height:1.35;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;word-break:break-all}
#navBtn{background:none;border:1px solid var(--line);color:var(--text);border-radius:10px;padding:8px 12px;font-size:14px;flex:none}
#messages{flex:1;overflow-y:auto;padding:12px 12px 16px;display:flex;flex-direction:column;gap:10px}
.moreBar{text-align:center;padding:6px;font-size:13px;color:var(--accent)}
.msg{max-width:88%;padding:12px 14px;border-radius:16px;font-size:16px;line-height:1.6;word-break:break-word;white-space:pre-wrap}
.msg.user{align-self:flex-end;background:var(--user);color:#fff;border-bottom-right-radius:6px}
.msg.assistant{align-self:flex-start;background:var(--card);border:1px solid var(--line);border-bottom-left-radius:6px}
#composer{display:flex;gap:8px;padding:10px 12px calc(10px + env(safe-area-inset-bottom));border-top:1px solid var(--line);background:rgba(14,16,20,.97)}
#input{flex:1;min-height:48px;max-height:120px;background:var(--card);border:1px solid var(--line);border-radius:12px;color:var(--text);font-size:16px;padding:12px 14px;outline:none;resize:none}
#input:focus{border-color:var(--accent)}
#sendBtn{width:60px;min-height:48px;border:0;border-radius:12px;background:var(--accent);color:#fff;font-size:15px;font-weight:600;flex:none}
#sendBtn:active{opacity:.85}
#sendBtn:disabled{opacity:.4}
/* 导航抽屉 */
#navMask{display:none;position:fixed;inset:0;background:rgba(0,0,0,.45);z-index:20}
#navDrawer{display:none;position:fixed;top:0;right:0;bottom:0;width:78%;max-width:320px;background:var(--bg);border-left:1px solid var(--line);z-index:21;flex-direction:column;transform:translateX(100%);transition:transform .2s}
#navDrawer.open{transform:translateX(0)}
#navDrawer header{padding:14px 16px;font-size:15px;font-weight:600;border-bottom:1px solid var(--line)}
#navList{flex:1;overflow-y:auto;padding:8px 10px}
.navItem{padding:11px 12px;border-radius:10px;margin-bottom:4px;cursor:pointer;display:flex;flex-direction:column;gap:3px}
.navItem:active{background:var(--card)}
.navItem .t{font-size:14px;color:var(--text);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.navItem .tm{font-size:11px;color:var(--sub)}
.navEmpty{text-align:center;color:var(--sub);padding:40px 16px;font-size:13px}
</style>
</head>
<body>
<header>
<h1>DeepSeek Harness</h1>
<p>手机端 · 继续主机端会话</p>
</header>
<main id="list"><div class="status">正在加载会话…</div></main>
<div id="chatView">
<div id="chatHeader"><button id="backBtn">‹ 返回</button><div id="chatTitle"></div><button id="navBtn">≡ 大纲</button></div>
<div id="messages"></div>
<div id="composer"><textarea id="input" rows="1" placeholder="输入消息…"></textarea><button id="sendBtn">发送</button></div>
</div>
<div id="navMask"></div>
<div id="navDrawer"><header>对话导航</header><div id="navList"></div></div>
<footer>可继续对话 · 不能新建/切换/管理工作区 · <b>请在电脑端操作</b></footer>
<script>
var state={sessionId:null,title:'',seenSeq:{},minSeq:Infinity,pollTimer:null,sending:false,loadingMore:false};
var $=function(s){return document.querySelector(s)};
function esc(s){return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#39;')}
function fmt(ts){var d=new Date(ts);return (d.getMonth()+1)+'/'+d.getDate()+' '+String(d.getHours()).padStart(2,'0')+':'+String(d.getMinutes()).padStart(2,'0')}
function api(method,args){return fetch('/api/'+method,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({type:'client-request',rpcId:'m'+Date.now()+Math.random().toString(36).slice(2,6),method:method,payload:{args:args}})}).then(function(r){if(r.status===401||r.status===302){window.location.href='/__lan/login';throw new Error('unauthorized')}return r.json()}).then(function(d){return d.result})}
function extractText(blocks){var out='';(blocks||[]).forEach(function(b){if(b.type==='text'&&b.text)out+=b.text});return out}
function pathName(p){if(!p)return '未分组';var s=p.split(/[\\/]/);return s[s.length-1]||p}
/* 会话列表：按工作区分组 + 折叠 */
function renderList(){
  var box=$('#list');
  api('session/list',{_request:{}}).then(function(res){
    if(!res||res.ok===false){box.innerHTML='<div class="status err">加载失败：'+esc(res&&res.error?res.error.message:'未知错误')+'</div>';return}
    var items=(res.value&&res.value.items)||[];
    if(!items.length){box.innerHTML='<div class="status">暂无会话<br>请在电脑端新建会话</div>';return}
    var groups={};
    items.forEach(function(s){var k=s.cwd||'';(groups[k]=groups[k]||[]).push(s)});
    var html='';
    Object.keys(groups).forEach(function(key){
      var list=groups[key];
      var name=pathName(key);
      html+='<div class="group" data-key="'+esc(key)+'"><div class="groupHead"><span class="gName">'+esc(name)+'</span><span class="gCount">'+list.length+'</span><span class="gArrow">▾</span></div><div class="groupBody">';
      list.forEach(function(s){
        var t=(s.projections&&s.projections.values&&s.projections.values.title)||'未命名会话';
        var run=s.running?'<span class="run">● 运行中</span>':'';
        html+='<div class="card" data-id="'+s.sessionId+'" data-title="'+esc(t)+'"><div class="title">'+esc(t)+'</div><div class="meta">'+fmt(s.updatedAt)+' '+run+'</div></div>';
      });
      html+='</div></div>';
    });
    box.innerHTML=html;
  }).catch(function(){box.innerHTML='<div class="status err">加载失败，请下拉刷新</div>'});
}
/* 聊天 */
function openChat(id,title){
  state.sessionId=id;state.title=title;state.seenSeq={};state.minSeq=Infinity;
  $('#list').style.display='none';
  $('#chatView').style.display='flex';
  $('#chatTitle').textContent=title;
  $('#messages').innerHTML='';
  loadMessages();
  if(state.pollTimer)clearInterval(state.pollTimer);
  state.pollTimer=setInterval(loadMessages,3000);
}
function backToList(){
  if(state.pollTimer)clearInterval(state.pollTimer);
  state.sessionId=null;closeNav();
  $('#chatView').style.display='none';
  $('#list').style.display='';
  renderList();
}
function getAsOfSeq(){
  return api('session/list',{_request:{}}).then(function(res){
    if(!res||res.ok===false)return 0;
    var items=(res.value&&res.value.items)||[];
    for(var i=0;i<items.length;i++){if(items[i].sessionId===state.sessionId)return items[i].projections?items[i].projections.asOfSeq:0}
    return 0;
  });
}
function loadMessages(){
  if(!state.sessionId)return;
  getAsOfSeq().then(function(seq){
    return api('session/page',{request:{address:{kind:'session',sessionId:state.sessionId},throughSeq:seq,maxMessages:300}});
  }).then(function(res){
    if(!res||res.ok===false)return;
    renderRecords(res.value.records,false);
  }).catch(function(){});
}
function loadEarlier(){
  if(!state.sessionId||state.loadingMore||state.minSeq===Infinity)return;
  state.loadingMore=true;
  var bar=$('#moreBar');
  getAsOfSeq().then(function(seq){
    var through=Math.min(seq,state.minSeq-1);
    if(through<1){state.loadingMore=false;if(bar)bar.textContent='已到最早';return null}
    return api('session/page',{request:{address:{kind:'session',sessionId:state.sessionId},throughSeq:through,maxMessages:200}});
  }).then(function(res){
    if(res&&res.ok){renderRecords(res.value.records,true);if(bar)bar.textContent=''}else if(bar){bar.textContent='已到最早'}
    state.loadingMore=false;
  }).catch(function(){state.loadingMore=false});
}
function renderRecords(records,prepend){
  var box=$('#messages');var rows=[];
  (records||[]).forEach(function(r){
    if(r.type!=='event')return;
    var t=r.event.type;var seq=r.event.seq;
    if(state.seenSeq[seq])return;
    if(t==='user/message'){
      state.seenSeq[seq]=1;
      var ut=extractText(r.event.data&&r.event.data.content);
      if(ut)rows.push({seq:seq,role:'user',text:ut,time:r.event.time});
    }else if(t==='assistant/message'){
      state.seenSeq[seq]=1;
      var m=r.event.data&&r.event.data.message;
      var at=extractText(m&&m.content);
      if(at)rows.push({seq:seq,role:'assistant',text:at,time:r.event.time});
    }
    if(seq<state.minSeq)state.minSeq=seq;
  });
  if(!rows.length)return;
  rows.sort(function(a,b){return a.seq-b.seq});
  var html='';
  rows.forEach(function(m){
    html+='<div class="msg '+m.role+'" id="msg-'+m.seq+'" data-seq="'+m.seq+'" data-time="'+m.time+'">'+esc(m.text)+'</div>';
  });
  if(prepend){
    box.insertAdjacentHTML('afterbegin',html);
  }else{
    if(!$('#moreBar')&&box.scrollHeight<box.clientHeight*3){
      box.insertAdjacentHTML('afterbegin','<div id="moreBar" class="moreBar">↑ 上滑加载更早</div>');
    }
    box.insertAdjacentHTML('beforeend',html);
    box.scrollTop=box.scrollHeight;
  }
}
function sendMsg(){
  var text=$('#input').value.trim();
  if(!text||state.sending||!state.sessionId)return;
  state.sending=true;$('#sendBtn').disabled=true;
  var req={requestId:'mq'+Date.now()+Math.random().toString(36).slice(2,8),sessionId:state.sessionId,mode:'queue',content:[{type:'text',text:text}]};
  try{req.clientTimeZone=Intl.DateTimeFormat().resolvedOptions().timeZone}catch(e){}
  api('session/prompt',{request:req}).then(function(res){
    if(!res||res.ok===false){alert('发送失败：'+(res&&res.error?res.error.message:'未知错误'))}
    else{$('#input').value='';loadMessages()}
  }).catch(function(){alert('发送失败，请重试')}).then(function(){state.sending=false;$('#sendBtn').disabled=false});
}
/* 对话导航（右侧抽屉） */
function openNav(){
  var nav=[];
  document.querySelectorAll('.msg.user').forEach(function(el){
    nav.push({seq:el.getAttribute('data-seq'),text:el.textContent.slice(0,24),time:el.getAttribute('data-time')});
  });
  var list=$('#navList');
  if(!nav.length){list.innerHTML='<div class="navEmpty">暂无对话节点</div>'}
  else{
    var h='';
    nav.slice().reverse().forEach(function(n){h+='<div class="navItem" data-seq="'+n.seq+'"><span class="t">'+esc(n.text||'…')+'</span><span class="tm">'+fmt(Number(n.time)||Date.now())+'</span></div>'});
    list.innerHTML=h;
  }
  $('#navDrawer').style.display='flex';$('#navMask').style.display='block';
  setTimeout(function(){$('#navDrawer').classList.add('open')},10);
}
function closeNav(){
  $('#navDrawer').classList.remove('open');
  setTimeout(function(){if(!$('#navDrawer').classList.contains('open')){$('#navDrawer').style.display='none';$('#navMask').style.display='none'}},200);
}
$('#list').addEventListener('click',function(e){
  var head=e.target.closest('.groupHead');
  if(head){head.parentElement.classList.toggle('collapsed');return}
  var card=e.target.closest('.card');
  if(card)openChat(card.getAttribute('data-id'),card.getAttribute('data-title'));
});
$('#backBtn').addEventListener('click',backToList);
$('#sendBtn').addEventListener('click',sendMsg);
$('#input').addEventListener('keydown',function(e){if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();sendMsg()}});
$('#navBtn').addEventListener('click',openNav);
$('#navMask').addEventListener('click',closeNav);
$('#navList').addEventListener('click',function(e){
  var item=e.target.closest('.navItem');
  if(!item)return;
  var seq=item.getAttribute('data-seq');
  closeNav();
  var el=document.getElementById('msg-'+seq);
  if(el)el.scrollIntoView({behavior:'smooth',block:'center'});
});
$('#messages').addEventListener('scroll',function(){
  if(this.scrollTop<40)loadEarlier();
});
renderList();
</script>
</body>
</html>`;



function iconBytes() {
  if (ICON_B64) { try { return Buffer.from(ICON_B64, 'base64'); } catch { /* fallthrough */ } }
  return null;
}

// ---------------- 页面（单文件 HTML + 内联 CSS，移动端适配） ----------------
function page(title, body, err) {
  const css = '*{box-sizing:border-box;margin:0;padding:0}html,body{height:100%}body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;background:radial-gradient(1200px 600px at 50% -10%,#1c2333 0%,#0f1115 55%,#0a0c10 100%);color:#e6e9f0;display:flex;align-items:center;justify-content:center;padding:24px;min-height:100dvh}.card{width:100%;max-width:380px;background:rgba(255,255,255,.05);border:1px solid rgba(255,255,255,.1);border-radius:16px;padding:28px 24px}.logo{text-align:center;font-size:15px;color:#9aa3b5;letter-spacing:2px;margin-bottom:6px}h1{font-size:19px;font-weight:600;text-align:center;margin-bottom:8px}.sub{font-size:13px;color:#9aa3b5;text-align:center;margin-bottom:22px;line-height:1.6}label{display:block;font-size:13px;color:#c3c9d6;margin-bottom:6px}input[type=password],input[type=text]{width:100%;height:46px;border-radius:10px;border:1px solid rgba(255,255,255,.16);background:rgba(0,0,0,.35);color:#fff;padding:0 14px;font-size:16px;outline:none}input:focus{border-color:#4d6bfe}button{width:100%;height:46px;margin-top:16px;border:0;border-radius:10px;background:#4d6bfe;color:#fff;font-size:16px;font-weight:600;cursor:pointer}button:active{transform:scale(.99)}.err{color:#ff7a7a;font-size:13px;margin-top:12px;text-align:center;display:none}.foot{margin-top:20px;font-size:12px;color:#6b7280;text-align:center;line-height:1.7}';
  return '<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8">' +
    '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">' +
    '<title>' + title + '</title><style>' + css + '</style></head>' +
    '<body><div class="card"><div class="logo">DEEPSEEK HARNESS</div>' +
    '<h1>' + title + '</h1>' + body + '</div></body></html>';
}
function loginPage(errMsg) {
  return page('局域网访问',
    '<div class="sub">请输入访问密码（PIN）<br>密码可在电脑端启动器「局域网共享」面板查看</div>' +
    '<form method="post" action="/__lan/login" autocomplete="off">' +
    '<label for="pin">访问密码</label>' +
    '<input type="password" id="pin" name="pin" inputmode="numeric" autocomplete="current-password" required autofocus>' +
    '<button type="submit">进入 DeepSeek Harness</button></form>' +
    '<div class="err"' + (errMsg ? " style='display:block'" : '') + '>' + (errMsg || '') + '</div>' +
    '<div class="foot">仅限同一 WiFi（局域网）使用 · 请求受速率限制保护</div>');
}
function statusPage(code, text) {
  return page(code + ' ' + text,
    '<div class="sub">' + text + '<br><br><a href="/" style="color:#4d6bfe">返回首页</a></div>');
}

// ---------------- 响应工具 ----------------
function send(res, status, headers, body) {
  res.writeHead(status, headers);
  res.end(body);
}
function sendJson(res, status, obj) {
  send(res, status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' }, JSON.stringify(obj));
}
function noCacheHtml(res, status, html) {
  send(res, status, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' }, html);
}
function setAuthCookie(res) {
  res.setHeader('set-cookie', COOKIE_NAME + '=' + signCookie() +
    '; Max-Age=' + COOKIE_MAX_AGE + '; Path=/; HttpOnly; SameSite=Strict');
}

// ---------------- 反向代理 ----------------
const TARGET_URL = new URL(TARGET);
function cleanHeaders(headers, extra) {
  const out = {};
  for (const [k, v] of Object.entries(headers)) {
    const lk = k.toLowerCase();
    if (lk === 'host' || lk === 'origin' || lk === 'referer' || lk === 'connection' ||
        lk === 'upgrade' || lk === 'sec-fetch-site' || lk === 'sec-fetch-mode' ||
        lk === 'sec-fetch-dest' || lk === 'sec-fetch-user' || lk === 'cookie' ||
        lk === 'content-length' || lk === 'transfer-encoding' || lk === 'keep-alive' ||
        lk === 'proxy-connection' || lk === 'accept-encoding' || lk === 'authorization') continue;
    out[k] = v;
  }
  out.host = TARGET_URL.host;
  if (dshCookie) out.cookie = dshCookie;
  if (extra) Object.assign(out, extra);
  return out;
}
// 读取 dsh workspace 注册表里的归档会话 ID（%USERPROFILE%/.dsh/storages/workspace.json）
// 带 30s TTL 缓存：移动端 session/list 每次调用都会触发，避免反复全量读+解析 JSON
let archivedCache = { at: 0, ids: null };
function readArchivedSessionIds() {
  const now = Date.now();
  if (archivedCache.ids && now - archivedCache.at < 30000) return archivedCache.ids;
  let ids = new Set();
  try {
    var home = process.env.DSH_HOME || (process.env.USERPROFILE + '/.dsh');
    var p = home + '/storages/workspace.json';
    if (fs.existsSync(p)) {
      var j = JSON.parse(fs.readFileSync(p, 'utf8'));
      if (j && j.global && Array.isArray(j.global.archivedSessionIds)) {
        ids = new Set(j.global.archivedSessionIds.map(function(s){return String(s)}));
      }
    }
  } catch (e) {}
  archivedCache = { at: now, ids: ids };
  return ids;
}
function isMobileRequest(req) {
  const ua = String(req.headers['user-agent'] || '');
  return /Mobile|Android|iPhone|iPad|iPod|Windows Phone/i.test(ua);
}
// 拦截手机端对原生目录选择器（directoryPicker/pick）的调用：
// dsh 该 RPC 会在【电脑端】弹出原生文件夹选择框，手机端调用会导致电脑弹窗而手机无反应。
// 放行 list/createDirectory（手机端可用输入路径/浏览方式），仅拦截 pick。
// 手机端只读模式：拒绝新建会话/切换工作区/管理工作区等写操作（后端强制，不依赖前端隐藏）
function isRestrictedMobileMethod(method) {
  return method === 'directoryPicker/pick'
    || method === 'session/create'
    || method === 'session/openWorkspacePath'
    || method === 'session/canOpenWorkspacePath'
    || method === 'workspace/create'
    || method === 'workspace/delete'
    || method === 'workspace/rename';
}
function mobileRestrictMessage(method) {
  if (method === 'session/create') return '手机端为只读模式：请在电脑端新建会话。';
  if (method === 'session/openWorkspacePath' || method === 'session/canOpenWorkspacePath') return '手机端不能切换工作区：请继续当前会话，或在电脑端切换。';
  if (method === 'workspace/create' || method === 'workspace/delete' || method === 'workspace/rename') return '手机端不能管理工作区：请在电脑端操作。';
  return '手机端不支持此操作：请在电脑端完成。';
}
function interceptMobileDirectoryPick(req, res) {
  if (req.method !== 'POST' || !req.url.startsWith('/api/') || !isMobileRequest(req)) return false;
  const chunks = [];
  let bodySize = 0;
  req.on('data', (ch) => {
    chunks.push(ch);
    bodySize += ch.length; // 按字节累计（原实现按块数判断，256K 块形同虚设）
    if (bodySize > 256 * 1024) {
      try { res.writeHead(413, { 'content-type': 'text/plain' }); res.end('request too large'); } catch { /* ignore */ }
      req.destroy();
    }
  });
  req.on('end', () => {
    let body = '';
    try { body = Buffer.concat(chunks).toString('utf8'); } catch { /* ignore */ }
    let rpcId = 'req-' + Date.now();
    let method = '';
    try { const j = JSON.parse(body); if (j) { if (typeof j.rpcId === 'string') rpcId = j.rpcId; if (typeof j.method === 'string') method = j.method; } } catch { /* not json */ }
    if (isRestrictedMobileMethod(method)) {
      log('拦截手机端受限操作（' + method + '）');
      sendJson(res, 200, { type: 'server-response', rpcId, result: { ok: false, error: {
        code: 'mobile/readonly',
        message: mobileRestrictMessage(method),
        details: {}
      } } });
      return;
    }
    const bodyBuf = Buffer.from(body, 'utf8');
    // 手机端 session/list：过滤归档会话（dsh 的 list 全量返回，桌面端由前端分组显示）
    if (method === 'session/list') {
      proxySessionListFiltered(req, res, bodyBuf);
      return;
    }
    // 放行：用已收集的 body 正常代理（body 已消费，直接传 Buffer 供重放）
    doProxy(req, res, bodyBuf, false, null);
  });
  return true;
}
// ---------------- 统一反向代理 ----------------
// doProxy：一次性完成 401 自动重试（重新兑换 token 后重发，body 可重放）、上游超时、
// 客户端断连传播、HTML PWA 注入与可选的 JSON 响应过滤（onJson 返回改写后的 body，返回 null 保持原样）
const UPSTREAM_TIMEOUT_MS = 30000;
function doProxy(req, res, body, retried, onJson) {
  const headers = cleanHeaders(req.headers);
  headers['content-length'] = String(body.length);
  const proxy = http.request({
    host: TARGET_URL.hostname, port: TARGET_URL.port || 80,
    method: req.method, path: req.url, headers, setHost: false,
    timeout: UPSTREAM_TIMEOUT_MS
  });
  const destroyUpstream = () => { try { proxy.destroy(); } catch { /* ignore */ } };
  // 客户端断连 → 销毁上游请求，避免 dsh 继续执行（如 LLM 生成空转、SSE 悬挂）
  req.on('aborted', destroyUpstream);
  res.on('close', () => { if (!res.writableEnded) destroyUpstream(); });
  proxy.on('timeout', destroyUpstream);
  proxy.on('error', (e) => {
    try { noCacheHtml(res, 502, statusPage(502, 'Harness 服务未运行或暂不可用')); } catch { /* ignore */ }
  });
  proxy.on('response', (pres) => {
    // dsh 令牌过期（401）且未重试：销毁当前响应 → 重新兑换 → 重发一次
    if (!retried && pres.statusCode === 401) {
      try { res.destroy(); } catch { /* ignore */ }
      destroyUpstream();
      log('目标返回 401，重新兑换 dsh 令牌后重试…');
      exchangeToken().then(() => {
        if (dshCookie) doProxy(req, res, body, true, onJson);
        else { try { noCacheHtml(res, 502, statusPage(502, 'Harness 认证失败，请重新开启局域网共享')); } catch { /* ignore */ } }
      });
      return;
    }
    const ct = String(pres.headers['content-type'] || '');
    const needCollect = ct.includes('text/html') || !!onJson;
    if (needCollect) {
      const hc = [];
      pres.on('data', (ch) => hc.push(ch));
      pres.on('end', () => {
        let buf = Buffer.concat(hc).toString('utf8');
        if (onJson) { try { const out = onJson(buf, pres); if (out != null) buf = out; } catch { /* keep original */ } }
        if (ct.includes('text/html')) buf = injectPwa(buf);
        const b = Buffer.from(buf, 'utf8');
        const h = { ...pres.headers, 'content-length': String(b.length), 'cache-control': 'no-cache, no-store, must-revalidate' };
        delete h['transfer-encoding'];
        delete h['set-cookie']; // 不透传目标认证 cookie 到手机浏览器
        res.writeHead(pres.statusCode || 200, h);
        res.end(b);
      });
      return;
    }
    const sh = { ...pres.headers };
    delete sh['set-cookie']; // 不透传目标认证 cookie 到手机浏览器
    res.writeHead(pres.statusCode || 200, sh);
    pres.pipe(res);
  });
  proxy.end(body);
}

// 代理 session/list 并在响应中剔除归档会话（仅手机端调用）
function proxySessionListFiltered(req, res, body) {
  doProxy(req, res, body, false, function(jsonBody) {
    const json = JSON.parse(jsonBody);
    if (json.result && json.result.ok === true && json.result.value && Array.isArray(json.result.value.items)) {
      const archived = readArchivedSessionIds();
      const before = json.result.value.items.length;
      json.result.value.items = json.result.value.items.filter(function(it){
        if (!it) return false;
        if (archived.has(String(it.sessionId))) return false;   // 归档
        if (it.blank === true) return false;                   // 空白
        if (it.origin === 'subagent') return false;            // 子代理
        if (it.parentSessionId) return false;                  // 有父会话
        return true;
      });
      if (json.result.value.items.length !== before) log('手机端已过滤 ' + (before - json.result.value.items.length) + ' 个非用户会话');
      return JSON.stringify(json);
    }
    return null; // 非目标结构保持原样
  });
}

async function proxyWithRetry(req, res, retried) {
  const ip = clientIp(req);
  const rl = rateLimit(ip, 'total', TOTAL_RATE);
  if (!rl.ok) { noCacheHtml(res, 429, statusPage(429, '请求过于频繁，请稍后再试（' + rl.retryAfter + ' 秒）')); return; }
  if (req.url.startsWith('/api/')) {
    const ar = rateLimit(ip, 'api', API_RATE);
    if (!ar.ok) { sendJson(res, 429, { error: 'rate_limited', retryAfter: ar.retryAfter }); return; }
    // 手机端拦截原生目录选择器/受限操作（内部自行读取 body）
    if (interceptMobileDirectoryPick(req, res)) return;
  }
  await ensureDshCookie();
  // 收集请求体（限 8MB），便于 401 重试时重放
  const chunks = [];
  let bodySize = 0;
  req.on('data', (c) => { chunks.push(c); bodySize += c.length; if (bodySize > 8 * 1024 * 1024) { req.destroy(); } });
  req.on('end', () => {
    doProxy(req, res, Buffer.concat(chunks), !!retried, null);
  });
}

// ---------------- WebSocket Upgrade 透传 ----------------
function proxyUpgrade(req, socket, head) {
  const rl = rateLimit(clientIp(req), 'total', TOTAL_RATE);
  if (!rl.ok) { socket.write('HTTP/1.1 429 Too Many Requests\r\nRetry-After: ' + rl.retryAfter + '\r\n\r\n'); socket.destroy(); return; }
  const headers = cleanHeaders(req.headers, {
    'connection': 'Upgrade',
    'upgrade': 'websocket',
    'sec-websocket-key': req.headers['sec-websocket-key'],
    'sec-websocket-version': req.headers['sec-websocket-version']
  });
  const proxy = http.request({
    host: TARGET_URL.hostname, port: TARGET_URL.port || 80,
    method: req.method || 'GET', path: req.url, headers, setHost: false,
    timeout: 15000
  });
  proxy.on('error', (e) => {
    try { socket.write('HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n'); } catch { /* ignore */ }
    try { socket.destroy(); } catch { /* ignore */ }
  });
  proxy.on('timeout', () => {
    try { proxy.destroy(); } catch { /* ignore */ }
  });
  // 目标对 upgrade 返回普通 HTTP 响应（如 401/500）时：把响应转发给客户端后关闭，
  // 避免客户端 socket 永久挂起（原实现无 'response' 监听）
  proxy.on('response', (pres) => {
    try {
      let headText = 'HTTP/1.1 ' + (pres.statusCode || 502) + ' ' + (pres.statusMessage || '') + '\r\n';
      for (const [k, v] of Object.entries(pres.headers)) {
        if (k.toLowerCase() === 'connection' || k.toLowerCase() === 'upgrade') continue;
        headText += k + ': ' + (Array.isArray(v) ? v.join(', ') : v) + '\r\n';
      }
      headText += '\r\n';
      socket.write(headText);
      pres.pipe(socket);
      socket.on('error', () => { try { pres.destroy(); } catch { /* ignore */ } });
    } catch (e) {
      try { socket.destroy(); } catch { /* ignore */ }
    }
  });
  proxy.on('upgrade', (pres, psocket, phead) => {
    let headText = 'HTTP/1.1 101 Switching Protocols\r\n';
    for (const [k, v] of Object.entries(pres.headers)) {
      if (k.toLowerCase() === 'connection' || k.toLowerCase() === 'upgrade') continue;
      headText += k + ': ' + (Array.isArray(v) ? v.join(', ') : v) + '\r\n';
    }
    headText += '\r\n';
    try {
      socket.write(headText);
      if (phead && phead.length) psocket.unshift(phead);
      socket.pipe(psocket);
      psocket.pipe(socket);
      psocket.on('error', () => { try { socket.destroy(); } catch { /* ignore */ } });
      socket.on('error', () => { try { psocket.destroy(); } catch { /* ignore */ } });
    } catch (e) {
      try { psocket.destroy(); } catch { /* ignore */ }
      try { socket.destroy(); } catch { /* ignore */ }
    }
  });
  proxy.end();
}

// ---------------- 主服务器 ----------------
const server = http.createServer((req, res) => {
  const url = req.url || '/';
  const ip = clientIp(req);
  // 拒绝绝对形式 request-target（GET http://... HTTP/1.1），阻断非预期请求面
  if (!url.startsWith('/')) {
    sendJson(res, 400, { error: 'bad_request' });
    return;
  }
  // ---- 网关自身端点（无需 PIN）----
  // 健康探测只暴露最少信息（局域网内避免泄露 pid/host/target）
  if (url === '/__lan/health') {
    sendJson(res, 200, { ok: true });
    return;
  }
  // 手机端缓存清理入口：返回 Clear-Site-Data 头，浏览器清除旧 SW/HTTP 缓存后重新加载
  if (url === '/__lan/m' || url === '/__lan/mobile') {
    send(res, 200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' }, MOBILE_UI);
    return;
  }
  if (url === '/__lan/cache-bust') {
    send(res, 200, {
      'content-type': 'text/html; charset=utf-8',
      'cache-control': 'no-store',
      'clear-site-data': '"cache", "storage"'
    }, '<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>缓存已清理</title><style>body{font-family:system-ui,sans-serif;background:#0f1115;color:#e6e9f0;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0;padding:24px;text-align:center}.c{max-width:420px}h1{font-size:20px}p{font-size:15px;color:#9aa3b5;line-height:1.7}</style></head><body><div class="c"><h1>缓存已清理 ✓</h1><p>旧版缓存已清除。请关闭此页面，重新打开<br>http://192.168.5.5:' + PORT + '/ 并输入 PIN。</p></div></body></html>');
    return;
  }
  if (url === '/__lan/manifest.webmanifest') {
    send(res, 200, { 'content-type': 'application/manifest+json; charset=utf-8' }, JSON.stringify(MANIFEST));
    return;
  }
  if (url === '/__lan/sw.js') {
    send(res, 200, { 'content-type': 'application/javascript; charset=utf-8', 'cache-control': 'no-cache' }, SERVICE_WORKER);
    return;
  }
  if (url === '/__lan/icon-192.png' || url === '/__lan/icon-512.png') {
    const b = iconBytes();
    if (b) { send(res, 200, { 'content-type': 'image/png', 'cache-control': 'public, max-age=86400' }, b); }
    else { send(res, 404, { 'content-type': 'text/plain' }, 'icon unavailable'); }
    return;
  }
  if (url === '/__lan/login') {
    if (req.method === 'GET') {
      if (isAuthed(req)) { res.writeHead(303, { location: '/' }); res.end(); return; }
      noCacheHtml(res, 200, loginPage(''));
      return;
    }
    if (req.method === 'POST') {
      const lr = rateLimit(ip, 'login', PIN_RATE);
      if (!lr.ok) { noCacheHtml(res, 429, statusPage(429, '尝试次数过多，请 ' + lr.retryAfter + ' 秒后再试')); return; }
      let body = '';
      req.on('data', (c) => {
        body += c;
        if (body.length > 4096) {
          try { res.writeHead(413, { 'content-type': 'text/plain' }); res.end('too large'); } catch { /* ignore */ }
          req.destroy();
        }
      });
      req.on('end', () => {
        const pin = new URLSearchParams(body).get('pin') || '';
        if (pinOk(pin)) {
          setAuthCookie(res);
          res.writeHead(303, { location: '/', 'cache-control': 'no-store' });
          res.end();
          log('PIN 验证通过（' + ip + '）');
        } else {
          noCacheHtml(res, 401, loginPage('密码错误，请重试'));
        }
      });
      return;
    }
  }
  // 登出改为 POST 且要求已认证，避免局域网内任意页面 <img src=.../__lan/logout> 触发登出（CSRF）
  if (url === '/__lan/logout') {
    if (req.method !== 'POST') { res.writeHead(405, { allow: 'POST' }); res.end(); return; }
    if (!isAuthed(req)) { sendJson(res, 401, { error: 'unauthorized' }); return; }
    res.writeHead(303, { location: '/__lan/login', 'set-cookie': COOKIE_NAME + '=; Max-Age=0; Path=/; HttpOnly; SameSite=Strict' });
    res.end();
    return;
  }
  if (url.startsWith('/__lan/')) {
    sendJson(res, 404, { error: 'not_found' });
    return;
  }
  // ---- 手机端：访问首页自动进入全新移动 UI（/__lan/m）；embed=1 用于打开会话聊天 ----
  if (url === '/' && isMobileRequest(req) && req.url.indexOf('embed') < 0) {
    res.writeHead(302, { location: '/__lan/m', 'cache-control': 'no-store' });
    res.end();
    return;
  }
  // ---- 其余一律走 PIN 门禁 + 代理 ----
  if (!isAuthed(req)) {
    if (req.headers.accept && String(req.headers.accept).includes('text/html')) {
      res.writeHead(302, { location: '/__lan/login', 'cache-control': 'no-store' });
      res.end();
    } else {
      sendJson(res, 401, { error: 'unauthorized', login: '/__lan/login' });
    }
    return;
  }
  proxyWithRetry(req, res, false);
});

server.on('upgrade', (req, socket, head) => {
  const url = req.url || '/';
  if (url.startsWith('/__lan/')) { try { socket.destroy(); } catch { /* ignore */ } return; }
  if (!isAuthed(req)) {
    try { socket.write('HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n'); } catch { /* ignore */ }
    try { socket.destroy(); } catch { /* ignore */ }
    return;
  }
  ensureDshCookie().then(() => proxyUpgrade(req, socket, head));
});

server.on('error', (e) => {
  if (e.code === 'EADDRINUSE') fail('端口 ' + PORT + ' 已被占用（可能已有局域网网关在运行）');
  else fail('服务器错误: ' + e.message);
});

server.listen(PORT, HOST, () => {
  log('局域网网关已启动: http://' + HOST + ':' + PORT + '  ->  ' + TARGET);
  log('PIN 门禁: ' + (readPin() ? '已启用' : '未设置（请先在启动器面板设置或生成 PIN）'));
  exchangeToken();
});
