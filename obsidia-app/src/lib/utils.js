export const I = { home:'<path d="M3 11l9-8 9 8v10h-6v-6H9v6H3z"/>', profiles:'<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>', mods:'<path d="M12 2l9 5v10l-9 5-9-5V7z"/><path d="M12 12l9-5M12 12v10M12 12L3 7"/>', servers:'<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18"/>', settings:'<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2"/>' };
export const ic = k => `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">${I[k]}</svg>`;
export const TABS = [['home','Inicio'],['profiles','Instancias'],['mods','Mods'],['servers','Servidores'],['settings','Ajustes']];
export const G = ['linear-gradient(135deg,#8B5CF6,#C026D3)','linear-gradient(135deg,#22D3EE,#8B5CF6)','linear-gradient(135deg,#C026D3,#1E1B4B)','linear-gradient(135deg,#6366F1,#22D3EE)'];
export const TRASH = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14M10 11v6M14 11v6"/></svg>';
export const M = [
  ['Sodium','Motor de renderizado que sube los FPS sin cambiar el aspecto del juego.','Optimización','linear-gradient(135deg,#22D3EE,#8B5CF6)','sodium'],
  ['Lithium','Optimiza la lógica del juego: menos lag en mundos y servidores grandes.','Optimización','linear-gradient(135deg,#8B5CF6,#C026D3)','lithium'],
  ['Iris Shaders','Shaders compatibles con Sodium para darle otra cara a tu mundo.','Gráficos','linear-gradient(135deg,#C026D3,#F472B6)','iris'],
  ['Mod Menu','Menú para ver y configurar todos tus mods desde el juego.','Utilidades','linear-gradient(135deg,#6366F1,#22D3EE)','modmenu'],
  ['Fabric API','Biblioteca base que necesitan la mayoría de los mods de Fabric.','Utilidades','linear-gradient(135deg,#A78BFA,#1E1B4B)','fabric-api'],
  ['Xaero’s Minimap','Minimapa en pantalla con marcadores para tus bases.','Utilidades','linear-gradient(135deg,#34D399,#22D3EE)','xaeros-minimap']
];
export const LOADERS = ['Vanilla','Fabric','Forge'];
export const RAMS = [2,3,4,6,8,10,12,16,24,32];
export const STG = { version:[0,5], loader:[0,5], java:[5,25], libraries:[25,60], assets:[60,95], starting:[95,99] };
export const LBL = { version:'Preparando la versión…', loader:'Instalando el loader…', java:'Descargando Java…', libraries:'Descargando librerías…', assets:'Descargando recursos del juego…', starting:'Abriendo Minecraft…' };

export const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
export const norm = s => String(s).toLowerCase().replace(/[^a-z0-9]/g, '');
export const validName = n => /^\w{3,16}$/.test(n);
export const normRam = r => { const s = String(r || '').trim().toUpperCase(); const n = parseInt(s) || 4; return s.endsWith('M') ? Math.max(1, Math.round(n/1024)) : n; };
export const trb = (kind, file, h) => `<button class="mb" style="width:${h}px;height:${h}px;padding:0;color:#F87171" aria-label="Eliminar ${esc(file)}" data-act="fdel" data-kind="${kind}" data-file="${esc(file)}">${TRASH}</button>`;

export function head(s) {
  return `<svg width="${s}" height="${s}" viewBox="0 0 8 8" shape-rendering="crispEdges" aria-hidden="true" style="border-radius:8px;flex:none"><rect width="8" height="8" fill="#E0A97B"/><rect width="8" height="3" fill="#3B2A1E"/><rect y="3" width="1" height="2" fill="#3B2A1E"/><rect x="7" y="3" width="1" height="2" fill="#3B2A1E"/><rect x="1" y="4" width="2" height="1" fill="#fff"/><rect x="5" y="4" width="2" height="1" fill="#fff"/><rect x="2" y="4" width="1" height="1" fill="#3B5BDB"/><rect x="5" y="4" width="1" height="1" fill="#3B5BDB"/><rect x="3" y="6" width="2" height="1" fill="#B5835A"/></svg>`;
}

export function skinFace(i, size, skins) {
  const url = (i >= 0 && i < skins.length) ? skins[i] : null;
  if (!url || !/^data:image\/png;base64,[A-Za-z0-9+/=]+$/.test(String(url))) return head(size);
  const bs = size * 8;
  const layer = x => `<div style="position:absolute;inset:0;background:url('${url}') -${x}px -${size}px/${bs}px auto no-repeat;image-rendering:pixelated"></div>`;
  return `<div style="position:relative;width:${size}px;height:${size}px;overflow:hidden;border-radius:8px;flex:none;background:#1a1a2e">${layer(size)}${layer(size*5)}</div>`;
}
// Cuerpo completo de frente a partir de un PNG 64x64 (o 64x32), en <canvas> con escala entera.
// Respeta el modelo elegido: brazos de 4 px (clásico) o 3 px (slim).
export function skinFull(url, model, scale) {
  const slim = model === 'slim';
  const aw = slim ? 3 : 4;
  const w = 8 + aw * 2;
  const s = Math.max(1, Math.round(+scale || 8));
  const ok = url && /^data:image\/png;base64,[A-Za-z0-9+/=]+$/.test(String(url));
  return `<canvas class="js-skin" width="${w*s}" height="${32*s}" data-skin="${ok ? url : ''}" data-model="${slim ? 'slim' : 'classic'}" data-scale="${s}" role="img" aria-label="Skin de jugador" style="width:${w*s}px;height:${32*s}px;image-rendering:pixelated;display:block"></canvas>`;
}

// Pinta los <canvas class="js-skin"> ya insertados en el DOM (los <script> del innerHTML no corren).
export function paintSkinCanvases(root) {
  const box = root || document;
  box.querySelectorAll('canvas.js-skin').forEach(cv => {
    if (cv.dataset.painted === '1') return;
    cv.dataset.painted = '1';
    const s = Math.max(1, Math.round(+cv.dataset.scale || 8));
    const aw = cv.dataset.model === 'slim' ? 3 : 4;
    const ctx = cv.getContext('2d');
    if (!ctx) return;
    ctx.imageSmoothingEnabled = false;
    const steve = () => paintSteve(ctx, aw, s);
    const url = cv.dataset.skin || '';
    if (!url) { steve(); return; }
    const img = new Image();
    img.onload = () => { try { paintSkinImage(ctx, img, aw, s); } catch(e) { steve(); } };
    img.onerror = steve;
    img.src = url;
  });
}

function paintSkinImage(ctx, img, aw, s) {
  const short = img.naturalHeight < 64;
  const put = (sx, sy, dx, dy, sw, sh) => ctx.drawImage(img, sx, sy, sw, sh, dx * s, dy * s, sw * s, sh * s);
  // Base (frente): piernas, torso, brazos y cabeza.
  put(4, 20, aw, 20, 4, 12);
  put(short ? 4 : 20, short ? 20 : 52, aw + 4, 20, 4, 12);
  put(20, 20, aw, 8, 8, 12);
  put(44, 20, 0, 8, aw, 12);
  put(short ? 44 : 36, short ? 20 : 52, aw + 8, 8, aw, 12);
  put(8, 8, aw, 0, 8, 8);
  if (short) return; // 64x32: sin capas exteriores ni mitad inferior.
  // Capas exteriores (overlay) sobre cada parte.
  put(4, 36, aw, 20, 4, 12);
  put(4, 52, aw + 4, 20, 4, 12);
  put(20, 36, aw, 8, 8, 12);
  put(44, 36, 0, 8, aw, 12);
  put(52, 52, aw + 8, 8, aw, 12);
  put(40, 8, aw, 0, 8, 8);
}

function paintSteve(ctx, aw, s) {
  const r = (x, y, w, h, c) => { ctx.fillStyle = c; ctx.fillRect(x * s, y * s, w * s, h * s); };
  const skin = '#E0A97B', hair = '#3B2A1E', shirt = '#00A8A8', pants = '#3B3B8F', shoe = '#373737';
  r(aw, 20, 4, 9, pants); r(aw, 29, 4, 3, shoe);
  r(aw + 4, 20, 4, 9, pants); r(aw + 4, 29, 4, 3, shoe);
  r(aw, 8, 8, 12, shirt);
  r(0, 8, aw, 12, skin);
  r(aw + 8, 8, aw, 12, skin);
  r(aw, 0, 8, 8, skin);
  r(aw, 0, 8, 3, hair);
  r(aw, 3, 1, 2, hair); r(aw + 7, 3, 1, 2, hair);
  r(aw + 1, 4, 2, 1, '#fff'); r(aw + 5, 4, 2, 1, '#fff');
  r(aw + 2, 4, 1, 1, '#3B5BDB'); r(aw + 5, 4, 1, 1, '#3B5BDB');
  r(aw + 3, 6, 2, 1, '#B5835A');
}

export function accCard(sk, nick, ms, skins) {
  return `${skinFace(sk, 40, skins)}<div class="who"><b>${esc(nick) || 'Jugador'}</b><span>${ms ? 'Cuenta Microsoft' : 'Cuenta offline'}</span></div><i class="status" aria-hidden="true"></i>`;
}

// Solo URLs https: se usan dentro de CSS `url("...")`, así que se rechaza cualquier otro esquema.
/** @param {any} src */
export function safeImageUrl(src) {
  const s = String(src ?? '').trim();
  if (!/^https:\/\//i.test(s)) return '';
  try { return encodeURI(s).replace(/'/g, '%27').replace(/&/g, '%26'); } catch(e){ return ''; }
}

export function modCard(kind, r, busy, installedFile, progress) {
  const inst = installedFile;
  const safe = safeImageUrl(r.icon_url);
  const ico = safe
    ? `<img src="${safe}" alt="${esc(r.title)}" style="width:60px;height:60px;object-fit:contain">`
    : `<div class="ico">${r.title.charAt(0).toUpperCase()}</div>`;

  let tags = '';
  if (r.categories && Array.isArray(r.categories)) {
    tags += r.categories.map(cat => `<span class="tag">${esc(cat)}</span>`).join('');
  }
  if (r.loaders && Array.isArray(r.loaders)) {
    tags += r.loaders.map(l => `<span class="tag">${esc(l)}</span>`).join('');
  }

  const dl = (r.downloads || 0).toLocaleString('es-AR') + ' descargas';

  let btnLabel = 'Instalar';
  let btnDisabled = false;
  if (progress) {
    if (progress.stage === 'resolving') { btnLabel = 'Instalar'; btnDisabled = true; }
    else if (progress.stage === 'downloading') {
      const pct = progress.total > 0 ? Math.round(100 * progress.done / progress.total) : 0;
      btnLabel = `Descargando ${pct} %`; btnDisabled = true;
    }
    else if (progress.stage === 'applying') { btnLabel = 'Instalar'; btnDisabled = true; }
  }
  if (busy) { btnLabel = 'Instalando...'; btnDisabled = true; }
  else if (btnDisabled) btnDisabled = true;
  if (inst) { btnLabel = 'Instalado ✓'; }

  return `<article class="card"><div class="head">${ico}<div><h3>${esc(r.title)}</h3><div class="by">por ${esc(r.author || '')}</div></div></div><div class="desc">${esc(r.description || '')}</div><div class="meta">${tags}<span>${dl}</span></div><div class="foot">${inst ? '<button class="btn done" disabled>Instalado</button>' : `<button class="ib" ${btnDisabled ? 'disabled' : ''} data-act="inst" data-kind="${kind}" data-slug="${esc(r.slug)}">${btnLabel}</button>`}<button class="btn sec" data-act="openmod" data-kind="${kind}" data-slug="${esc(r.slug)}">Ver</button></div></article>`;
}

export function resCard(kind, r, busy, installedFile, progress) {
  const inst = installedFile;
  /** @param {any} src @param {string} grad */
  const bg = (src, grad) => {
    const safe = safeImageUrl(src);
    return safe
      ? `background-image:url("${safe}"),${grad};background-size:cover;background-position:center`
      : `background:${grad}`;
  };
  const modGrad = 'linear-gradient(135deg,#8B5CF6,#22D3EE)';
  const shaderGrad = 'linear-gradient(135deg,#FBBF24,#F472B6)';
  const packGrad = 'repeating-linear-gradient(90deg,rgba(0,0,0,.14) 0 8px,transparent 8px 16px),linear-gradient(135deg,#34D399,#22D3EE)';
  const thumb = kind === 'mods'
    ? `<div style="display:flex;align-items:center;gap:12px"><div class="mi" style='${bg(r.icon_url, modGrad)}'></div><b>${esc(r.title)}</b></div>`
    : `<div style='position:relative;height:176px;border-radius:14px;overflow:hidden;${bg(r.icon_url, kind === 'packs' ? packGrad : shaderGrad)}'><div style="position:absolute;left:0;right:0;bottom:0;padding:14px;background:linear-gradient(transparent,rgba(10,8,20,.9))"><b style="font-size:17px;line-height:1.3">${esc(r.title)}</b></div></div>`;
  let btnLabel = 'Instalar';
  let btnDisabled = false;
  if (progress){
    if (progress.stage === 'resolving'){ btnLabel = 'Instalar'; btnDisabled = true; }
    else if (progress.stage === 'downloading'){
      const pct = progress.total > 0 ? Math.round(100 * progress.done / progress.total) : 0;
      btnLabel = `Descargando ${pct} %`; btnDisabled = true;
    }
    else if (progress.stage === 'applying'){ btnLabel = 'Instalar'; btnDisabled = true; }
  }
  if (btnDisabled || busy){ btnDisabled = true; }
  if (inst){ btnLabel = 'Instalado ✓'; }
  return `<div class="mc gl">${thumb}<div class="mu" style="line-height:1.5">${esc(r.description)}</div><div class="mu" style="font-size:12px">${(r.downloads || 0).toLocaleString('es-AR')} descargas</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${inst ? 'in' : ''}" style="flex:1;margin:0" ${btnDisabled ? 'disabled' : ''} data-act="inst" data-kind="${kind}" data-slug="${esc(r.slug)}">${btnLabel}</button>${inst ? trb(kind, inst, 38) : ''}<button class="sec" style="height:38px;padding:0 14px" data-act="openmod" data-kind="${kind}" data-slug="${esc(r.slug)}">Ver</button></div></div>`;
}

export function irisBanner(busy) {
  return `<div class="gl" style="grid-column:1/-1;display:flex;justify-content:space-between;align-items:center;gap:16px;padding:16px 20px;border-color:rgba(251,191,36,.45)"><div><b>Los shaders necesitan Iris Shaders</b><div class="mu">Instalalo en este perfil (necesita Fabric) para poder activarlos.</div></div><button class="ib" style="margin:0;padding:0 22px" ${busy?'disabled':''} data-act="inst" data-kind="mods" data-slug="iris">Instalar Iris</button></div>`;
}
