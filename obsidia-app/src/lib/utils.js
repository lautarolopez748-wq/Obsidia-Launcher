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
