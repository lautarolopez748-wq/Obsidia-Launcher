export const I = { home:'<path d="M3 11l9-8 9 8v10h-6v-6H9v6H3z"/>', profiles:'<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>', mods:'<path d="M12 2l9 5v10l-9 5-9-5V7z"/><path d="M12 12l9-5M12 12v10M12 12L3 7"/>', servers:'<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18"/>', settings:'<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2"/>' };
export const ic = k => `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">${I[k]}</svg>`;
export const TABS = [['home','Inicio'],['profiles','Perfiles'],['mods','Mods'],['servers','Servidores'],['settings','Ajustes']];
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

export function accCard(sk, nick, ms, skins) {
  return `${skinFace(sk, 40, skins)}<div><div style="font-weight:600">${esc(nick) || 'Jugador'}</div><div class="mu">${ms ? 'Cuenta Microsoft' : 'Cuenta offline'}</div></div>`;
}

export function modCard(m, busy, installedFile) {
  const inst = installedFile;
  return `<div class="mc gl"><div style="display:flex;align-items:center;gap:12px"><div class="mi" style="background:${m[3]}"></div><b>${esc(m[0])}</b></div><div class="mu" style="line-height:1.5">${esc(m[1])}</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${inst?'in':''}" style="flex:1;margin:0" ${inst||busy?'disabled':''} data-act="inst" data-kind="mods" data-slug="${esc(m[4])}">${busy?'Instalando.':inst?'Instalado V':'Instalar'}</button>${inst?trb('mods',inst,38):''}</div></div>`;
}

export function resCard(kind, r, busy, installedFile) {
  const inst = installedFile;
  const thumb = kind === 'mods'
    ? `<div style="display:flex;align-items:center;gap:12px"><div class="mi" style="background:linear-gradient(135deg,#8B5CF6,#22D3EE)"></div><b>${esc(r.title)}</b></div>`
    : `<div style="height:88px;border-radius:12px;background:${kind === 'packs' ? 'repeating-linear-gradient(90deg,rgba(0,0,0,.14) 0 8px,transparent 8px 16px),linear-gradient(135deg,#34D399,#22D3EE)' : 'linear-gradient(135deg,#FBBF24,#F472B6)'}"></div><b>${esc(r.title)}</b>`;
  return `<div class="mc gl">${thumb}<div class="mu" style="line-height:1.5">${esc(r.description)}</div><div class="mu" style="font-size:12px">${(r.downloads || 0).toLocaleString('es-AR')} descargas</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${inst ? 'in' : ''}" style="flex:1;margin:0" ${inst || busy ? 'disabled' : ''} data-act="inst" data-kind="${kind}" data-slug="${esc(r.slug)}">${busy ? 'Instalando…' : inst ? 'Instalado ✓' : 'Instalar'}</button>${inst ? trb(kind, inst, 38) : ''}<button class="sec" style="height:38px;padding:0 14px" data-act="openmod" data-kind="${kind}" data-slug="${esc(r.slug)}">Ver</button></div></div>`;
}

export function irisBanner(busy) {
  return `<div class="gl" style="grid-column:1/-1;display:flex;justify-content:space-between;align-items:center;gap:16px;padding:16px 20px;border-color:rgba(251,191,36,.45)"><div><b>Los shaders necesitan Iris Shaders</b><div class="mu">Instalalo en este perfil (necesita Fabric) para poder activarlos.</div></div><button class="ib" style="margin:0;padding:0 22px" ${busy?'disabled':''} data-act="inst" data-kind="mods" data-slug="iris">${busy?'Instalando…':'Instalar Iris'}</button></div>`;
}
