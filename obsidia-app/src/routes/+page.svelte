<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  const I = { home:'<path d="M3 11l9-8 9 8v10h-6v-6H9v6H3z"/>', profiles:'<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>', mods:'<path d="M12 2l9 5v10l-9 5-9-5V7z"/><path d="M12 12l9-5M12 12v10M12 12L3 7"/>', servers:'<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18"/>', settings:'<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2"/>' };
  const ic = k => `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">${I[k]}</svg>`;
  const TABS = [['home','Inicio'],['profiles','Perfiles'],['mods','Mods'],['servers','Servidores'],['settings','Ajustes']];
  const G = ['linear-gradient(135deg,#8B5CF6,#C026D3)','linear-gradient(135deg,#22D3EE,#8B5CF6)','linear-gradient(135deg,#C026D3,#1E1B4B)','linear-gradient(135deg,#6366F1,#22D3EE)'];
  let P = [];
  let profileVersions = [];
  let profileUsers = [];
  let S = { sec:'mods', sa:null, tab:'home', p:0, ram:4, q:'', f:'Todos', pg:null, java:'Java 21', t:{close:true,lite:false,cons:false}, mods:{Sodium:1,Lithium:1,'Mod Menu':1,'Fabric API':1}, ro:[], ra:{}, sk:-1, skins:[], nick:'Jugador', ms:false, ml:false, sv:[], af:false, rf:false, dc:null, jt:null, np:false, npName:'', npVersion:'1.21.4', npLoader:'Vanilla', npRam:4, npEdit:-1, es:null, ns:'', na:'', results:{mods:null,shaders:null,packs:null}, searching:false, installedMR:[], installedShaders:{}, installedPacks:{}, totalRam:16 };
  const M = [['Sodium','Motor de renderizado que sube los FPS sin cambiar el aspecto del juego.','Optimización','linear-gradient(135deg,#22D3EE,#8B5CF6)'],['Lithium','Optimiza la lógica del juego: menos lag en mundos y servidores grandes.','Optimización','linear-gradient(135deg,#8B5CF6,#C026D3)'],['Iris Shaders','Shaders compatibles con Sodium para darle otra cara a tu mundo.','Gráficos','linear-gradient(135deg,#C026D3,#F472B6)'],['Mod Menu','Menú para ver y configurar todos tus mods desde el juego.','Utilidades','linear-gradient(135deg,#6366F1,#22D3EE)'],['Fabric API','Biblioteca base que necesitan la mayoría de los mods de Fabric.','Utilidades','linear-gradient(135deg,#A78BFA,#1E1B4B)'],['Xaero’s Minimap','Minimapa en pantalla con marcadores para tus bases.','Utilidades','linear-gradient(135deg,#34D399,#22D3EE)']];
  const SH = [['Complementary Reimagined','Look fiel a Minecraft con mejor luz y agua; buen equilibrio.','Medio','linear-gradient(135deg,#FBBF24,#F472B6)'],['BSL Shaders','Un clásico: sombras suaves, reflejos y atmósfera cálida.','Medio','linear-gradient(135deg,#F97316,#8B5CF6)'],['Photon','Nubes volumétricas y sombras de estilo cinematográfico.','Pesado','linear-gradient(135deg,#6366F1,#22D3EE)'],['Rethinking Voxels','Iluminación dinámica y reflejos realistas para PCs potentes.','Pesado','linear-gradient(135deg,#C026D3,#1E1B4B)'],['MakeUp Ultra Fast','Mejora visual con impacto mínimo en los FPS.','Ligero','linear-gradient(135deg,#34D399,#22D3EE)'],['Sildur’s Vibrant Lite','Colores vivos y buena compatibilidad en equipos modestos.','Ligero','linear-gradient(135deg,#A3E635,#34D399)']];
  const PC = { Ligero:'#34D399', Medio:'#FBBF24', Pesado:'#F87171' };
  const RP = [['Faithful 32x','Texturas vanilla al doble de resolución, fieles al original.','32x','linear-gradient(135deg,#34D399,#22D3EE)'],['Bare Bones','Estilo minimalista y plano, muy liviano.','16x','linear-gradient(135deg,#A78BFA,#F472B6)'],['Default Dark Mode','Interfaz oscura para todo el juego, ideal de noche.','16x','linear-gradient(135deg,#1E1B4B,#6366F1)'],['Fresh Animations','Animaciones más fluidas para los mobs.','16x','linear-gradient(135deg,#F97316,#FBBF24)'],['Better Leaves','Hojas más frondosas y con volumen.','16x','linear-gradient(135deg,#84CC16,#16A34A)'],['Stay True','Mejora el aspecto sin perder la esencia de Minecraft.','16x','linear-gradient(135deg,#22D3EE,#8B5CF6)']];
  const TY = {'Faithful 32x':'base','Stay True':'base','Bare Bones':'base','Default Dark Mode':'retoque','Fresh Animations':'retoque','Better Leaves':'retoque'};
  const SKN = [['#3B2A1E','#E0A97B','#3B5BDB','#B5835A','Clásica'],['#1E1B4B','#F1C9A5','#8B5CF6','#C98F6B','Noche'],['#C026D3','#F5D0B5','#22D3EE','#D4A07A','Neón'],['#111827','#8D5A3B','#34D399','#6B3F28','Bosque']];
  const TRASH = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14M10 11v6M14 11v6"/></svg>';
  const head = (i, s) => { const k = SKN[i]; return `<svg width="${s}" height="${s}" viewBox="0 0 8 8" shape-rendering="crispEdges" aria-hidden="true" style="border-radius:8px;flex:none"><rect width="8" height="8" fill="${k[1]}"/><rect width="8" height="3" fill="${k[0]}"/><rect y="3" width="1" height="2" fill="${k[0]}"/><rect x="7" y="3" width="1" height="2" fill="${k[0]}"/><rect x="1" y="4" width="2" height="1" fill="#fff"/><rect x="5" y="4" width="2" height="1" fill="#fff"/><rect x="2" y="4" width="1" height="1" fill="${k[2]}"/><rect x="5" y="4" width="1" height="1" fill="${k[2]}"/><rect x="3" y="6" width="2" height="1" fill="${k[3]}"/></svg>`; };
  const esc = s => String(s).replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
  S.ro = [];
  S.ra = {};

  const Q = s => document.querySelector(s);
  const sw = (k,l,d) => `<div class="row gl"><div><div style="font-weight:600">${l}</div><div class="mu">${d}</div></div><button class="sw" role="switch" aria-label="${l}" aria-checked="${S.t[k]}" data-act="tg" data-k="${k}"></button></div>`;
  const seg = () => `<div class="seg">${[['mods','Mods'],['shaders','Shaders'],['packs','Resource packs']].map(s=>`<button class="${S.sec==s[0]?'on':''}" data-act="seg" data-s="${s[0]}">${s[1]}</button>`).join('')}</div>`;
  const trb = (n,h) => `<button class="mb" style="width:${S.dc==n?'auto':h+'px'};height:${h}px;padding:0 ${S.dc==n?'14px':'0'};font-size:13px;font-weight:600;color:${S.dc==n?'#fff':'#F87171'};background:${S.dc==n?'#DC2626':'rgba(255,255,255,.06)'}" aria-label="Eliminar ${n}" data-act="rpdel" data-n="${n}">${S.dc==n?'¿Eliminar?':TRASH}</button>`;

  function skinFace(i, size){ const url = (i>=0 && i < S.skins.length) ? S.skins[i] : null; if (url){ const scale = size/8; return `<div style="width:${size}px;height:${size}px;overflow:hidden;border-radius:8px;flex:none;background:#1a1a2e"><img src="${url}" style="image-rendering:pixelated;transform:scale(${scale});transform-origin:top left;width:auto;height:auto;display:block" alt=""></div>`; } return head(0, size); }
function accCard(){ return `${skinFace(S.sk,40)}<div><div style="font-weight:600">${esc(S.nick)||'Jugador'}</div><div class="mu">${S.ms?'Cuenta Microsoft':'Cuenta offline'}</div></div>`; }
  let VERSIONS = ['1.21.4','1.21.1','1.21','1.20.6','1.20.4','1.20.1','1.19.4','1.19.2','1.18.2','1.17.1','1.16.5'];
  const LOADERS = ['Vanilla','Fabric','Forge','NeoForge','Quilt'];
  const RAMS = [2,3,4,6,8,10,12,16];
  function npModal(){
    const title = S.npEdit>=0 ? 'Editar perfil' : 'Nuevo perfil';
    const rams = RAMS.filter(r => r <= (S.totalRam||16));
    return `<div class="md-back" data-act="npclose"><div class="md gl" style="width:min(520px,92vw);padding:24px;display:flex;flex-direction:column;gap:14px">
      <div style="display:flex;justify-content:space-between;align-items:center"><h2 style="margin:0">${title}</h2><button class="mb" data-act="npclose" aria-label="Cerrar">✕</button></div>
      <div><label class="mu" for="np-name">Nombre del perfil</label><input id="np-name" class="srch" style="width:100%;margin-top:6px" maxlength="24" autocomplete="off" value="${esc(S.npName)}"></div>
      <div><label class="mu" for="np-version">Versión</label><select id="np-version" class="srch" style="width:100%;margin-top:6px">${VERSIONS.map(v=>`<option ${v==S.npVersion?'selected':''}>${v}</option>`).join('')}</select></div>
      <div><label class="mu" for="np-loader">Loader</label><select id="np-loader" class="srch" style="width:100%;margin-top:6px">${LOADERS.map(l=>`<option ${l==S.npLoader?'selected':''}>${l}</option>`).join('')}</select></div>
      <div><label class="mu" for="np-ram">RAM asignada</label><select id="np-ram" class="srch" style="width:100%;margin-top:6px">${rams.map(r=>`<option value="${r}" ${r==S.npRam?'selected':''}>${r} GB</option>`).join('')}</select></div>
      <div style="display:flex;gap:10px;justify-content:flex-end;margin-top:6px"><button class="sec" style="height:44px" data-act="npclose">Cancelar</button><button class="ib" style="margin:0;height:44px;padding:0 24px" data-act="npcreate">Crear perfil</button></div>
    </div></div>`;
  }

  function cards(){ return P.map((p,i)=>`<button class="pc gl ${i==S.p?'on':''}" data-act="sel" data-i="${i}" style="position:relative"><div class="th" style="background:${G[i%4]}"></div><div style="flex:1"><div style="font-weight:600">${p.n}</div><div class="mu">${i==S.p?'Activo · ':''}${p.s}</div></div><button class="mb" style="width:auto;height:32px;padding:0 12px;font-size:13px" data-act="ped" data-i="${i}">Editar</button></button>`).join(''); }

  function home(){
    const p = P[S.p] || {n:'Sin perfil',s:''};
    const n = Object.keys(S.mods).filter(k=>S.mods[k]===1).length;
    const g = S.pg;
    const st = g===null?'':g<30?'Verificando archivos…':g<80?'Descargando recursos del juego…':g<100?'Iniciando Java…':S.jt?'Conectando a '+S.jt+' (simulado)':'Minecraft iniciado (simulado)';
    return `<section class="hero"><div class="shard" aria-hidden="true"></div><div class="mu" style="color:#C4B5FD;font-weight:600;letter-spacing:2px;font-size:12px">PERFIL ACTIVO</div><h1 style="font-size:42px;line-height:1.1">${p.n}</h1><div style="color:#C9C7DD">${p.s}${p.s?' · ':''}${S.java} · ${S.ram} GB de RAM${S.sa?' · Shader: '+S.sa:''}</div><div style="display:flex;gap:12px;flex-wrap:wrap;margin-top:6px"><button class="play" data-act="play" ${g!==null||!P.length?'disabled':''}>${g===null?'JUGAR':g<100?'Preparando… '+g+' %':'EN JUEGO'}</button><button class="sec" data-act="go" data-t="settings">Configurar</button></div></section>
    ${g!==null?`<div class="gl" style="padding:14px 18px"><div style="display:flex;justify-content:space-between;font-size:13px;color:#C9C7DD"><span>${st}</span><span>${g} %</span></div><div class="bar" style="margin-top:10px"><i style="width:${g}%"></i></div></div>`:''}
    <h2>Tus perfiles</h2><div class="g3">${cards()}${!P.length?`<button class="pc gl" style="border-style:dashed;justify-content:center;min-height:92px;color:var(--mu)" data-act="new">+ Nuevo perfil</button>`:''}</div>`;
  }

  function profiles(){ return `<h1>Perfiles</h1><div class="mu">Elegí con cuál jugar o creá uno nuevo.</div><div class="g3">${cards()}<button class="pc gl" style="border-style:dashed;justify-content:center;min-height:92px;color:var(--mu)" data-act="new">+ Nuevo perfil</button></div>`; }

  function sgrid(){
    const q = S.q.trim();
    const res = S.results.shaders;
    if (q && res) {
      const ir = S.mods['Iris Shaders'];
      const bn = ir===1 ? '' : `<div class="gl" style="grid-column:1/-1;display:flex;justify-content:space-between;align-items:center;gap:16px;padding:16px 20px;border-color:rgba(251,191,36,.45)"><div><b>Los shaders necesitan Iris Shaders</b><div class="mu">Instalalo en este perfil para poder activarlos.</div></div><button class="ib" style="margin:0;padding:0 22px" data-act="mod" data-n="Iris Shaders">${ir==='w'?'Instalando…':'Instalar Iris'}</button></div>`;
      const cards = res.length ? res.map(s=>{
        const installed = S.installedShaders[s.slug];
        return `<div class="mc gl"><div style="height:88px;border-radius:12px;background:linear-gradient(135deg,#FBBF24,#F472B6)"></div><div style="display:flex;justify-content:space-between;align-items:center;gap:8px"><b>${esc(s.title)}</b></div><div class="mu" style="line-height:1.5">${esc(s.description)}</div><div class="mu" style="font-size:12px">${(s.downloads||0).toLocaleString()} descargas</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${installed?'in':''}" style="flex:1;margin:0" data-act="sh_install" data-slug="${esc(s.slug)}">${installed?'Instalado ✓':'Instalar'}</button>${installed?`<button class="mb" style="width:auto;height:38px;padding:0 14px;font-size:13px;color:#F87171" data-act="sh_delete" data-slug="${esc(s.slug)}">Eliminar</button>`:''}<button class="sec" style="height:38px;padding:0 14px" data-act="openmod" data-slug="${esc(s.slug)}">Ver</button></div></div>`;
      }).join('') : '<div class="mu">No hay resultados en Modrinth.</div>';
      return bn + cards;
    }
    const ql = S.q.toLowerCase();
    const ir = S.mods['Iris Shaders'];
    const bn = ir===1 ? '' : `<div class="gl" style="grid-column:1/-1;display:flex;justify-content:space-between;align-items:center;gap:16px;padding:16px 20px;border-color:rgba(251,191,36,.45)"><div><b>Los shaders necesitan Iris Shaders</b><div class="mu">Instalalo en este perfil para poder activarlos.</div></div><button class="ib" style="margin:0;padding:0 22px" data-act="mod" data-n="Iris Shaders">${ir==='w'?'Instalando…':'Instalar Iris'}</button></div>`;
    return bn + SH.filter(s=>s[0].toLowerCase().includes(ql)).map(s=>{
      const v = S.mods[s[0]]||0;
      const on = S.sa==s[0];
      const lock = v===1 && ir!==1;
      return `<div class="mc gl"><div style="height:88px;border-radius:12px;background:${s[3]}"></div><div style="display:flex;justify-content:space-between;align-items:center;gap:8px"><b>${s[0]}</b><span class="pf" style="color:${PC[s[2]]}">${s[2]}</span></div><div class="mu" style="line-height:1.5">${s[1]}</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${on?'in':''}" style="flex:1;margin:0${lock?';opacity:.6;cursor:default':''}" ${lock?'disabled':''} data-act="sh" data-n="${s[0]}">${v==='w'?'Instalando…':v!==1?'Instalar':lock?'Requiere Iris':on?'Activo ✓':'Activar'}</button>${v===1?trb(s[0],38):''}</div></div>`;
    }).join('') || '<div class="mu">No hay resultados.</div>';
  }
  function shAct(n){
    const v = S.mods[n]||0;
    if (v==='w') return;
    if (v===1){ if (S.mods['Iris Shaders']!==1) return; S.sa = S.sa==n?null:n; }
    else { S.mods[n]='w'; setTimeout(()=>{ S.mods[n]=1; if (Q('#grid')) Q('#grid').innerHTML = grid(); },1100); }
    if (Q('#grid')) Q('#grid').innerHTML = grid();
  }

  function panel(){
    const o = S.ro;
    if (!o.length) return '';
    const bad = o.some((n,i)=>TY[n]=='base' && o.slice(i+1).some(m=>TY[m]=='retoque'));
    const two = o.filter(n=>TY[n]=='base').length > 1;
    const ok = !bad && !two;
    return `<div class="gl" style="grid-column:1/-1;padding:18px 20px;display:flex;flex-direction:column;gap:12px;border-color:${ok?'rgba(52,211,153,.4)':'rgba(251,191,36,.45)'}"><div style="display:flex;justify-content:space-between;align-items:center;gap:16px"><div><b>Orden de prioridad</b><div class="mu">Arriba = mayor prioridad. Lo de arriba se ve sobre lo de abajo.</div></div><button class="ib" style="margin:0;padding:0 20px" data-act="auto">Ordenar automático</button></div>${o.map((n,i)=>`<div style="display:flex;align-items:center;gap:12px;padding:8px 12px;border-radius:12px;background:rgba(255,255,255,.04)"><span class="mu" style="width:20px">${i+1}</span><b style="flex:1">${n}</b><span class="pf" style="color:#C9C7DD">${TY[n]=='base'?'Base':'Retoque'}</span><button class="mb" aria-label="Subir ${n}" data-act="mv" data-n="${n}" data-d="-1" ${i==0?'disabled':''}>↑</button><button class="mb" aria-label="Bajar ${n}" data-act="mv" data-n="${n}" data-d="1" ${i==o.length-1?'disabled':''}>↓</button></div>`).join('')}<div style="font-size:13px;color:${ok?'#6EE7B7':'#FCD34D'}">${ok?'Orden correcto ✓ Los retoques se ven sobre el pack base.':bad?'Hay un pack base encima de un retoque, así que el retoque no se va a ver. Usá Ordenar automático.':'Tenés más de un pack base: solo se ve el de más arriba donde se pisan.'}</div></div>`;
  }
  function panel2(){
    const o = S.ro;
    if (!o.length) return '';
    const ac = o.filter(n=>S.ra[n]);
    const bad = ac.some((n,i)=>TY[n]=='base' && ac.slice(i+1).some(m=>TY[m]=='retoque'));
    const two = ac.filter(n=>TY[n]=='base').length > 1;
    const ok = !bad && !two;
    return `<div class="gl" style="grid-column:1/-1;padding:18px 20px;display:flex;flex-direction:column;gap:12px;border-color:${ok?'rgba(52,211,153,.4)':'rgba(251,191,36,.45)'}"><div style="display:flex;justify-content:space-between;align-items:center;gap:16px"><div><b>Tus resource packs</b><div class="mu">Arriba = mayor prioridad. Los apagados no cuentan.</div></div><button class="ib" style="margin:0;padding:0 20px" data-act="auto">Ordenar automático</button></div>${o.map((n,i)=>`<div style="display:flex;align-items:center;gap:12px;padding:8px 12px;border-radius:12px;background:rgba(255,255,255,.04);opacity:${S.ra[n]?1:.6}"><span class="mu" style="width:20px">${i+1}</span><b style="flex:1">${n}</b><span class="pf" style="color:#C9C7DD">${TY[n]=='base'?'Base':'Retoque'}</span><button class="sw" role="switch" aria-label="Activar ${n}" aria-checked="${!!S.ra[n]}" data-act="ract" data-n="${n}"></button><button class="mb" aria-label="Subir ${n}" data-act="mv" data-n="${n}" data-d="-1" ${i==0?'disabled':''}>↑</button><button class="mb" aria-label="Bajar ${n}" data-act="mv" data-n="${n}" data-d="1" ${i==o.length-1?'disabled':''}>↓</button>${trb(n,34)}</div>`).join('')}<div style="font-size:13px;color:${!ac.length?'#C9C7DD':ok?'#6EE7B7':'#FCD34D'}">${!ac.length?'Activá un pack con el interruptor para usarlo.':ok?'Orden correcto ✓ Los retoques se ven sobre el pack base.':bad?'Hay un pack base encima de un retoque, así que el retoque no se va a ver. Usá Ordenar automático.':'Tenés más de un pack base: solo se ve el de más arriba donde se pisan.'}</div></div>`;
  }
  function rgrid(){
    const q = S.q.trim();
    const res = S.results.packs;
    if (q && res) {
      const cards = res.length ? res.map(r=>{
        const installed = S.installedPacks[r.slug];
        return `<div class="mc gl"><div style="height:88px;border-radius:12px;background:repeating-linear-gradient(90deg,rgba(0,0,0,.14) 0 8px,transparent 8px 16px),linear-gradient(135deg,#34D399,#22D3EE)"></div><div style="display:flex;justify-content:space-between;align-items:center;gap:8px"><b>${esc(r.title)}</b></div><div class="mu" style="line-height:1.5">${esc(r.description)}</div><div class="mu" style="font-size:12px">${(r.downloads||0).toLocaleString()} descargas</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${installed?'in':''}" style="flex:1;margin:0" data-act="rp_install" data-slug="${esc(r.slug)}">${installed?'Instalado ✓':'Instalar'}</button>${installed?`<button class="mb" style="width:auto;height:38px;padding:0 14px;font-size:13px;color:#F87171" data-act="rp_delete" data-slug="${esc(r.slug)}">Eliminar</button>`:''}<button class="sec" style="height:38px;padding:0 14px" data-act="openmod" data-slug="${esc(r.slug)}">Ver</button></div></div>`;
      }).join('') : '<div class="mu">No hay resultados en Modrinth.</div>';
      return panel2() + cards;
    }
    const ql = S.q.toLowerCase();
    return panel2() + RP.filter(r=>r[0].toLowerCase().includes(ql)).map(r=>{
      const v = S.mods[r[0]]||0;
      const on = !!S.ra[r[0]];
      return `<div class="mc gl"><div style="height:88px;border-radius:12px;background:repeating-linear-gradient(90deg,rgba(0,0,0,.14) 0 8px,transparent 8px 16px),${r[3]}"></div><div style="display:flex;justify-content:space-between;align-items:center;gap:8px"><b>${r[0]}</b><span class="pf" style="color:#C9C7DD">${r[2]}</span></div><div class="mu" style="line-height:1.5">${r[1]}</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${on?'in':''}" style="flex:1;margin:0" data-act="rp" data-n="${r[0]}">${v==='w'?'Instalando…':v!==1?'Instalar':on?'Activo ✓':'Activar'}</button></div></div>`;
    }).join('') || '<div class="mu">No hay resultados.</div>';
  }
  function rpAct(n){
    S.dc = null;
    const v = S.mods[n]||0;
    if (v==='w') return;
    if (v===1){ S.ra[n] = !S.ra[n]; }
    else { S.mods[n]='w'; setTimeout(()=>{ S.mods[n]=1; S.ro.unshift(n); if (Q('#grid')) Q('#grid').innerHTML = grid(); },1100); }
    if (Q('#grid')) Q('#grid').innerHTML = grid();
  }

  function servers(){
    const pv = (P[S.p] && P[S.p].s.match(/\d+\.\d+(\.\d+)?/)||[])[0];
    const pc = x => x<60?'#34D399':x<120?'#FBBF24':'#F87171';
    return `<div style="display:flex;justify-content:space-between;align-items:center;gap:16px"><h1>Servidores</h1><div style="display:flex;gap:10px"><button class="sec" style="height:44px" data-act="sref">${S.rf?'Actualizando…':'Actualizar'}</button><button class="ib" style="margin:0;padding:0 22px;height:44px" data-act="sadd">${S.af?'+ Añadir servidor':'+ Añadir servidor'}</button></div></div>
    ${S.af?`<div class="gl" style="padding:18px 20px;display:flex;gap:12px;align-items:center;flex-wrap:wrap"><input id="sn" class="srch" style="flex:1;min-width:180px" aria-label="Nombre del servidor" placeholder="Nombre del servidor" value="${esc(S.ns)}"><input id="sa" class="srch" style="flex:1;min-width:220px" aria-label="Dirección del servidor" placeholder="Dirección (ej. mi.servidor.net)" value="${esc(S.na)}"><button class="ib" style="margin:0;padding:0 24px;height:48px" data-act="ssave">${S.es!==null?'Guardar cambios':'Guardar'}</button><button class="sec" style="height:48px" data-act="scancel">Cancelar</button></div>`:''}
    <div class="mu">Se comparan con la versión de tu perfil activo: ${P[S.p]?P[S.p].n:''}${pv?' ('+pv+')':''}.</div>
    <div style="display:grid;grid-template-columns:repeat(2,1fr);gap:16px">${S.sv.map((s,i)=>{
      const diff = pv && s.v !== pv;
      return `<div class="gl" style="padding:18px;display:flex;flex-direction:column;gap:14px"><div style="display:flex;align-items:center;gap:14px"><div class="th" style="width:52px;height:52px;background:${G[i%4]}"></div><div style="flex:1;min-width:0"><b>${s.n}</b><div class="mu">${s.a}</div></div><span class="pf" style="color:${s.on?'#34D399':'#F87171'}">${s.on?'En línea':'Sin conexión'}</span></div><div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap"><span class="pf" style="color:#C9C7DD">${s.v}</span>${s.on?`<span class="pf" style="color:#C9C7DD">${s.pl} jugadores</span><span class="pf" style="color:${pc(s.pg)}">${s.pg} ms</span>`:''}${diff?'<span class="pf" style="color:#FCD34D">Versión distinta a tu perfil</span>':''}</div><div style="display:flex;gap:10px"><button class="ib" style="flex:1;margin:0;${s.on?'':'opacity:.5;cursor:default'}" ${s.on?'':'disabled'} data-act="join" data-n="${s.n}">Unirse</button><button class="sec" style="height:38px;padding:0 14px" data-act="sedit" data-i="${i}">Editar</button><button class="mb" style="width:auto;height:38px;padding:0 14px;font-size:14px" aria-label="Quitar ${s.n}" data-act="sdel" data-i="${i}">Quitar</button></div></div>`;
    }).join('')}</div>`;
  }

  const SKN_HEAD = SKN;
  function account(){
    const ok = /^\w{3,16}Q/.test(S.nick);
    return `<h1>Mi cuenta</h1><div style="display:grid;grid-template-columns:1fr 1.3fr;gap:16px;align-items:start"><div class="gl" style="padding:26px;display:flex;flex-direction:column;align-items:center;gap:18px">${skinFace(S.sk,132)}<div style="width:100%"><label class="mu" for="nick">Nombre de jugador</label><input id="nick" class="srch" style="width:100%;margin-top:6px" maxlength="16" autocomplete="off" value="${esc(S.nick)}"><div id="nm" style="margin-top:8px;font-size:13px;color:${ok?'#6EE7B7':'#FCD34D'}">${ok?'Nombre válido ✓':'Usá de 3 a 16 letras, números o guion bajo.'}</div></div></div><div style="display:flex;flex-direction:column;gap:16px"><div class="gl" style="padding:20px 22px;display:flex;flex-direction:column;gap:10px"><b>Tipo de cuenta</b><div style="color:${S.ms?'#6EE7B7':'#C9C7DD'}">${S.ms?'Cuenta Microsoft vinculada (simulado)':'Estás jugando en modo offline'}</div><div class="mu">${S.ms?'Podés entrar a servidores premium y usar tu skin oficial.':'En modo offline no podés entrar a servidores que verifican la cuenta.'}</div>${S.ms?'<button class="sec" style="height:44px" data-act="mso">Cerrar sesión</button>':`<button class="ib" style="margin:0;height:44px" data-act="ms">${S.ml?'Conectando…':'Iniciar sesión con Microsoft'}</button>`}</div><div class="gl" style="padding:20px 22px;display:flex;flex-direction:column;gap:14px"><b>Skin</b><div style="display:grid;grid-template-columns:repeat(4,1fr);gap:12px">${S.skins.map((u,i)=>`<button class="pc gl ${i==S.sk?'on':''}" style="flex-direction:column;padding:12px" data-act="skin" data-i="${i}">${skinFace(i,56)}<span class="mu">Skin ${i+1}</span></button>`).join('')}${S.skins.length===0?`<div class="mu" style="grid-column:1/-1">No hay skins cargadas. Subí una para empezar (máx. 4).</div>`:''}</div><input type="file" id="skinfile" accept="image/png,image/jpeg" style="display:none"><button class="ib" style="margin:0;height:44px" data-act="skinup" ${S.skins.length>=4?'disabled':''}>${S.skins.length>=4?'Límite de 4 skins alcanzado':'Subir skin'}</button></div></div></div>`;
  }

  async function doSearch(sec){
  const key = sec=='mods'?'mods':sec=='shaders'?'shaders':'packs';
  const cmd = sec=='mods'?'search_mods':sec=='shaders'?'search_shaders':'search_resourcepacks';
  const q = S.q.trim();
  if (!q){ S.results[key] = null; if (Q('#grid')) Q('#grid').innerHTML = grid(); return; }
  S.searching = true;
  if (Q('#grid')) Q('#grid').innerHTML = `<div class="mu">Buscando en Modrinth…</div>`;
  try {
    const res = await invoke(cmd, { query: q });
    S.results[key] = Array.isArray(res) ? res : [];
  } catch(e){ console.error(e); S.results[key] = []; }
  S.searching = false;
  if (Q('#grid')) Q('#grid').innerHTML = grid();
}
function mv(){
    const q = S.q.toLowerCase();
    return `<div style="display:flex;justify-content:space-between;align-items:center"><h1>Mods</h1><div class="gl" style="padding:9px 16px;color:#C9C7DD;font-size:14px">Perfil: ${P[S.p]?P[S.p].n:''}</div></div>${seg()}<input id="q" class="srch" aria-label="Buscar mods" placeholder="Buscar en Modrinth…" value="${S.q}"><div class="chips">${['Todos','Optimización','Gráficos','Utilidades'].map(c=>`<button class="chip ${c==S.f?'on':''}" data-act="f" data-c="${c}">${c}</button>`).join('')}</div><div class="g3" id="grid">${grid()}</div>`;
  }
  function mods(){ return S.sec=='shaders'?shaders():S.sec=='packs'?packs():mv(); }
  function shaders(){ return `<div style="display:flex;justify-content:space-between;align-items:center"><h1>Mods</h1><div class="gl" style="padding:9px 16px;color:#C9C7DD;font-size:14px">Perfil: ${P[S.p]?P[S.p].n:''}</div></div>${seg()}<input id="q" class="srch" aria-label="Buscar shaders" placeholder="Buscar shaders…" value="${S.q}"><div class="g3" id="grid">${sgrid()}</div>`; }
  function packs(){ return `<div style="display:flex;justify-content:space-between;align-items:center"><h1>Mods</h1><div class="gl" style="padding:9px 16px;color:#C9C7DD;font-size:14px">Perfil: ${P[S.p]?P[S.p].n:''}</div></div>${seg()}<input id="q" class="srch" aria-label="Buscar resource packs" placeholder="Buscar resource packs…" value="${S.q}"><div class="mu">Podés activar varios: los de arriba tienen prioridad.</div><div class="g3" id="grid">${rgrid()}</div>`; }
  function grid(){
    if (S.sec=='shaders') return sgrid();
    if (S.sec=='packs') return rgrid();
    const q = S.q.trim();
    const res = S.results.mods;
    if (q && res) {
      if (!res.length) return '<div class="mu">No hay resultados en Modrinth.</div>';
      return res.map(r=>`<div class="mc gl"><div style="display:flex;align-items:center;gap:12px"><div class="mi" style="background:linear-gradient(135deg,#8B5CF6,#22D3EE)"></div><b>${esc(r.title)}</b></div><div class="mu" style="line-height:1.5">${esc(r.description)}</div><div class="mu" style="font-size:12px">${(r.downloads||0).toLocaleString()} descargas</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${S.installedMR.includes(r.slug)?'in':''}" style="flex:1;margin:0" data-act="modrinth_install" data-slug="${esc(r.slug)}" data-title="${esc(r.title)}" ${S.installedMR.includes(r.slug)?'disabled':''}>${S.installedMR.includes(r.slug)?'Instalado ✓':'Instalar'}</button><button class="sec" style="height:38px;padding:0 14px" data-act="openmod" data-slug="${esc(r.slug)}">Ver en Modrinth</button></div></div>`).join('');
    }
    const ql = S.q.toLowerCase();
    return M.filter(m=>(S.f=='Todos'||m[2]==S.f)&&m[0].toLowerCase().includes(ql)).map(m=>{
      const v = S.mods[m[0]]||0;
      return `<div class="mc gl"><div style="display:flex;align-items:center;gap:12px"><div class="mi" style="background:${m[3]}"></div><b>${m[0]}</b></div><div class="mu" style="line-height:1.5">${m[1]}</div><div style="display:flex;gap:10px;margin-top:auto"><button class="ib ${v===1?'in':''}" style="flex:1;margin:0" data-act="mod" data-n="${m[0]}">${v==='w'?'Instalando…':v===1?'Instalado ✓':'Instalar'}</button>${v===1?trb(m[0],38):''}</div></div>`;
    }).join('') || '<div class="mu">No hay resultados.</div>';
  }

  function settings(){
    return `<h1>Ajustes</h1><div class="row gl"><div><div style="font-weight:600">Memoria RAM</div><div class="mu">Cuánta memoria usa Minecraft</div></div><div style="display:flex;align-items:center;gap:14px"><input type="range" id="ram" min="2" max="16" value="${S.ram}" aria-label="Memoria RAM"><b id="ramv" style="width:60px">${S.ram} GB</b></div></div><div class="row gl"><div><div style="font-weight:600">Versión de Java</div><div class="mu">Obsidia la descarga sola si falta</div></div><select id="java" aria-label="Versión de Java">${['Java 8','Java 17','Java 21'].map(j=>`<option ${j==S.java?'selected':''}>${j}</option>`).join('')}</select></div>${sw('close','Cerrar el launcher al jugar','Libera memoria mientras jugás')}${sw('lite','Modo ligero','Sin animaciones, ideal para PCs modestas')}${sw('cons','Mostrar consola','Ver los logs del juego en vivo')}`;
  }

  function render(){
    document.body.classList.toggle('lite', S.t.lite);
    Q('#acc').innerHTML = accCard();
    Q('#acc').classList.toggle('on', S.tab=='account');
    Q('#nav').innerHTML = TABS.map(t=>`<button class="${t[0]==S.tab?'on':''}" data-act="go" data-t="${t[0]}">${ic(t[0])}${t[1]}</button>`).join('');
    let html = {home,profiles,mods,servers,settings,account}[S.tab]();
    if (S.np) html += npModal();
    Q('#view').innerHTML = html;
  }

  function play(){
    if (S.pg !== null) return;
    S.pg = 0;
    const id = setInterval(()=>{
      S.pg += 4;
      if (S.pg >= 100){ S.pg = 100; clearInterval(id); setTimeout(()=>{ S.pg = null; render(); }, 2500); }
      render();
    }, 120);
    const version = (P[S.p] && P[S.p].version) || '1.21.4';
    const username = (P[S.p] && P[S.p].username) || 'Player';
    invoke('launch_version', { version, username, ram: S.ram + 'G', javaPath: null }).catch(e => { console.error(e); });
  }

  async function persistSettings(){
    try { await invoke('save_settings', { settings: { ram: S.ram + 'G', java_path: null, light_mode: S.t.lite, console: S.t.cons, username: S.nick } }); } catch(e) { console.error(e); }
  }

  function loadProfileData(i){
    if (profileVersions[i]) {
      // keep
    }
  }

  onMount(async () => {
    try {
      const installed = await invoke('get_installed_versions').catch(()=>[]);
      const remote = await invoke('list_versions').catch(()=>[]);
      const profiles = await invoke('get_profiles').catch(()=>[]);
      const javas = await invoke('detect_javas').catch(()=>[]);
      const installedMods = await invoke('list_installed_mods').catch(()=>[]);
      const shaderpacks = await invoke('list_shaderpacks').catch(()=>[]);
      const resourcepacks = await invoke('list_resourcepacks').catch(()=>[]);
      const servers = await invoke('get_servers').catch(()=>[]);
      const settings = await invoke('get_settings').catch(()=>null);
      const msProfile = await invoke('ms_get_profile').catch(()=>null);
      const totalRam = await invoke('get_total_ram').catch(()=>16);
      if (remote && remote.length) VERSIONS = remote;
      S.totalRam = totalRam || 16;

      if (profiles && profiles.length) {
        P = profiles.map(p => ({ n: p.name, s: `${p.version} · ${p.username || 'Player'}`, version: p.version || '1.21.4', ram: parseInt(p.ram) || 4, loader: 'Vanilla', username: p.username || 'Player' }));
        // profileVersions/profileUsers removed
        // (kept for compatibility not used)
      }
      if (servers && servers.length) {
        S.sv = servers.map(s => ({ n: s.name, a: s.address, v: s.version || '1.21.4', on: true, pl: '0/20', pg: 50 }));
      }
      if (settings) {
        S.ram = parseInt(settings.ram) || 4;
        S.t.lite = !!settings.light_mode;
        S.t.cons = !!settings.console;
        S.nick = settings.username || S.nick;
      }
      if (msProfile) S.ms = true;
    } catch (e) { console.error(e); }

    document.addEventListener('click', e => {
      const b = e.target.closest('[data-act]');
      if (!b) return;
      const a = b.dataset.act;
      const d = b.dataset;
      if (a=='go'){ S.tab = d.t; render(); }
      else if (a=='sel'){ S.p = +d.i; if (P[S.p]) S.ram = P[S.p].ram; render(); }
      else if (a=='play'){ S.jt = null; play(); }
      else if (a=='new'){ S.np = true; S.npEdit = -1; S.npName = ''; S.npVersion = '1.21.4'; S.npLoader = 'Vanilla'; S.npRam = 4; render(); }
      else if (a=='ped'){
        const i = +d.i;
        if (P[i]){
          S.npEdit = i;
          S.np = true;
          S.npName = P[i].n || '';
          S.npVersion = P[i].version || '1.21.4';
          S.npLoader = P[i].loader || 'Vanilla';
          S.npRam = P[i].ram || 4;
          render();
        }
      }
      else if (a=='f'){ S.f = d.c; render(); }
      else if (a=='tg'){ S.t[d.k] = !S.t[d.k]; if (d.k==='lite') document.body.classList.toggle('lite', S.t.lite); persistSettings(); render(); }
      else if (a=='seg'){ S.sec = d.s; S.q = ''; render(); }
      else if (a=='sh'){ shAct(d.n); }
      else if (a=='join'){ if (S.pg === null){ S.jt = d.n; play(); } S.tab = 'home'; render(); }
      else if (a=='sadd'){ S.af = !S.af; if (S.af){ S.es = null; S.ns = ''; S.na = ''; } render(); }
      else if (a=='scancel'){ S.af = false; S.es = null; S.ns = ''; S.na = ''; render(); }
      else if (a=='sedit'){
        const i = +d.i;
        const s = S.sv[i];
        if (s){ S.af = true; S.es = s.a; S.ns = s.n; S.na = s.a; render(); }
      }
      else if (a=='ssave'){
        const nm = (Q('#sn')||{}).value ? Q('#sn').value.trim() : (S.ns||'').trim();
        const ad = (Q('#sa')||{}).value ? Q('#sa').value.trim() : (S.na||'').trim();
        if (nm && ad){
          const oldAddr = S.es;
          const doSave = () => invoke('save_server', { name: nm, address: ad, version: (P[S.p] && P[S.p].version) || null });
          const chain = (oldAddr && oldAddr !== ad) ? invoke('delete_server', { address: oldAddr }).then(doSave).catch(doSave) : doSave();
          chain.then(async () => {
            const servers = await invoke('get_servers').catch(()=>[]);
            if (servers && servers.length) S.sv = servers.map(s=>({n:s.name,a:s.address,v:s.version||'1.21.4',on:true,pl:'0/20',pg:50}));
            S.af = false; S.es = null; S.ns = ''; S.na = ''; render();
          }).catch(e=>{ console.error(e); render(); });
        } else { render(); }
      }
      else if (a=='sdel'){ const i = +d.i; const s = S.sv[i]; if (s) invoke('delete_server', { address: s.a }).catch(()=>{}); S.sv.splice(i,1); render(); }
      else if (a=='sref'){ S.rf = true; render(); setTimeout(()=>{ S.sv.forEach(s=>{ if (s.on) s.pg = Math.max(15, s.pg + Math.round((Math.random()-.5)*30)); }); S.rf = false; render(); }, 800); }
      else if (a=='skin'){ S.sk = +d.i; persistSettings(); render(); }
else if (a=='skinup'){ const fi = Q('#skinfile'); if (fi) fi.click(); }
      else if (a=='ms'){
        S.ml = true; render();
        invoke('ms_device_code').then(dc => {
          // store code globally not in S; show via alert-like? keep simple
          invoke('ms_login', { deviceCode: dc.device_code }).then(profile => {
            S.ms = true; S.ml = false; S.nick = (profile && profile.name) ? profile.name : S.nick; render();
          }).catch(e => { S.ml = false; console.error(e); render(); });
        }).catch(e => { S.ml = false; console.error(e); render(); });
      }
      else if (a=='mso'){ invoke('ms_logout').catch(()=>{}); S.ms = false; render(); }
      else if (a=='ract'){ S.ra[d.n] = !S.ra[d.n]; if (Q('#grid')) Q('#grid').innerHTML = grid(); }
      else if (a=='rpdel'){
        const n = d.n;
        if (S.dc == n){
          S.mods[n] = 0; S.ro = S.ro.filter(x=>x!=n); delete S.ra[n]; if (S.sa==n || n=='Iris Shaders') S.sa = null; S.dc = null;
          invoke('delete_file_in', { folder: 'resourcepacks', name: n }).catch(()=>{});
        } else {
          S.dc = n;
          setTimeout(()=>{ if (S.dc==n){ S.dc = null; if (Q('#grid')) Q('#grid').innerHTML = grid(); } }, 3000);
        }
        if (Q('#grid')) Q('#grid').innerHTML = grid();
      }
      else if (a=='rp'){ rpAct(d.n); }
      else if (a=='mv'){
        const i = S.ro.indexOf(d.n);
        const j = i + +d.d;
        if (j>=0 && j<S.ro.length){ const a2 = [...S.ro]; [a2[i],a2[j]]=[a2[j],a2[i]]; S.ro = a2; }
        if (Q('#grid')) Q('#grid').innerHTML = grid();
      }
      else if (a=='auto'){ S.ro = [...S.ro.filter(n=>TY[n]=='retoque'), ...S.ro.filter(n=>TY[n]=='base')]; if (Q('#grid')) Q('#grid').innerHTML = grid(); }
      else if (a=='mod'){
        const v = S.mods[d.n]||0;
        if (v==='w') return;
        if (v===1) return;
        else { S.mods[d.n]='w'; setTimeout(()=>{ S.mods[d.n]=1; if (Q('#grid')) Q('#grid').innerHTML = grid(); },1100); }
        if (Q('#grid')) Q('#grid').innerHTML = grid();
      }
      else if (a=='openmod'){
        const slug = d.slug;
        const type = S.sec=='shaders'?'shader':S.sec=='packs'?'resourcepack':'mod';
        const url = `https://modrinth.com/${type}/${slug}`;
        try { invoke('plugin:opener|open_url', { url }).catch(()=>{ window.open(url,'_blank'); }); } catch(e){ window.open(url,'_blank'); }
      }
      else if (a=='modrinth_install'){
        const slug = d.slug;
        const profile = P[S.p];
        const mcVersion = (profile && profile.version) || '1.21.4';
        const loader = ((profile && profile.loader) || 'Fabric').toLowerCase();
        if (!['fabric','forge','neoforge','quilt'].includes(loader)){ alert('Este perfil usa un loader no soportado para instalar mods (usa Fabric/Forge/NeoForge/Quilt).'); return; }
        invoke('install_mod', { slug, mcVersion, loader }).then(()=>{ if (!S.installedMR.includes(slug)) S.installedMR.push(slug); if (Q('#grid')) Q('#grid').innerHTML = grid(); }).catch(e=>{ console.error(e); alert('No se pudo instalar: ' + e); });
        if (Q('#grid')) Q('#grid').innerHTML = grid();
      }
      else if (a=='sh_install'){
        const slug = d.slug;
        invoke('install_shaderpack', { slug }).then(filename => {
          S.installedShaders[slug] = filename;
          if (Q('#grid')) Q('#grid').innerHTML = grid();
        }).catch(e => { console.error(e); alert('No se pudo instalar: ' + e); });
      }
      else if (a=='sh_delete'){
        const slug = d.slug;
        const filename = S.installedShaders[slug];
        if (filename){ invoke('delete_file_in', { folder: 'shaderpacks', name: filename }).catch(()=>{}); }
        delete S.installedShaders[slug];
        if (Q('#grid')) Q('#grid').innerHTML = grid();
      }
      else if (a=='rp_install'){
        const slug = d.slug;
        invoke('install_resourcepack', { slug }).then(filename => {
          S.installedPacks[slug] = filename;
          if (Q('#grid')) Q('#grid').innerHTML = grid();
        }).catch(e => { console.error(e); alert('No se pudo instalar: ' + e); });
      }
      else if (a=='rp_delete'){
        const slug = d.slug;
        const filename = S.installedPacks[slug];
        if (filename){ invoke('delete_file_in', { folder: 'resourcepacks', name: filename }).catch(()=>{}); }
        delete S.installedPacks[slug];
        if (Q('#grid')) Q('#grid').innerHTML = grid();
      }
      else if (a=='npclose'){ if (e.target !== b) return; S.np = false; S.npEdit = -1; render(); }
      else if (a=='npcreate'){
        const nameEl = Q('#np-name');
        const versionEl = Q('#np-version');
        const loaderEl = Q('#np-loader');
        const ramEl = Q('#np-ram');
        const name = nameEl && nameEl.value ? nameEl.value.trim() : '';
        const version = versionEl && versionEl.value ? versionEl.value : '1.21.4';
        const loader = loaderEl && loaderEl.value ? loaderEl.value : 'Vanilla';
        const ram = ramEl && ramEl.value ? +ramEl.value : 4;
        const finalName = name || ('Perfil nuevo ' + (P.length + 1));
        if (S.npEdit>=0 && S.npEdit < P.length){
          const i = S.npEdit;
          P[i] = { ...P[i], n: finalName, s: `${loader} · ${version}`, version, ram, loader };
          S.p = i;
        } else {
          P.push({ n: finalName, s: `${loader} · ${version}`, version, ram, loader, username: 'Player' });
          S.p = P.length - 1;
        }
        S.ram = ram;
        S.np = false;
        S.npEdit = -1;
        render();
      }
    });

    document.addEventListener('change', e => {
      if (e.target && e.target.id=='skinfile'){
        const f = e.target.files && e.target.files[0];
        if (f && S.skins.length < 4){
          const url = URL.createObjectURL(f);
          S.skins.push(url);
          S.sk = S.skins.length - 1;
          render();
        }
        e.target.value = '';
      }
    });
    document.addEventListener('input', e => {
      if (e.target.id=='q'){ S.q = e.target.value; if (Q('#grid')) Q('#grid').innerHTML = grid(); if (S.q.trim().length >= 2) doSearch(S.sec); }
      else if (e.target.id=='skinfile'){
        const f = e.target.files && e.target.files[0];
        if (f && S.skins.length < 4){
          const url = URL.createObjectURL(f);
          S.skins.push(url);
          S.sk = S.skins.length - 1;
          render();
        }
        e.target.value = '';
      }
      else if (e.target.id=='ram'){ S.ram = +e.target.value; const rv = Q('#ramv'); if (rv) rv.textContent = S.ram+' GB'; persistSettings(); }
      else if (e.target.id=='java'){ S.java = e.target.value; }
      else if (e.target.id=='nick'){ S.nick = e.target.value; const acc = Q('#acc'); if (acc) acc.innerHTML = accCard(); const ok = /^\w{3,16}Q/.test(S.nick); const nm = Q('#nm'); if (nm){ nm.textContent = ok?'Nombre válido ✓':'Usá de 3 a 16 letras, números o guion bajo.'; nm.style.color = ok?'#6EE7B7':'#FCD34D'; } persistSettings(); }
      else if (e.target.id=='sn'){ S.ns = e.target.value; }
      else if (e.target.id=='sa'){ S.na = e.target.value; }
    });

    render();
  });
</script>

<svelte:head>
  <style>
    :root{--bg:#0A0A12;--tx:#EDEBF7;--mu:#A9A7C0;--gl:rgba(255,255,255,.05);--bd:rgba(255,255,255,.09);--v:#8B5CF6;--c:#22D3EE;color-scheme:dark;box-sizing:border-box;padding-top:env(safe-area-inset-top,0px);padding-bottom:env(safe-area-inset-bottom,0px)}
    html{height:100%;scroll-padding-top:env(safe-area-inset-top,0px)}
    *{box-sizing:border-box}
    body{margin:0;height:100%;color:var(--tx);font:15px Inter,system-ui,sans-serif;background:radial-gradient(900px 500px at 75% -10%,rgba(139,92,246,.28),transparent 60%),radial-gradient(700px 400px at 100% 100%,rgba(192,38,211,.16),transparent 60%),var(--bg)}
    button,input,select{font:inherit;color:inherit}button{cursor:pointer}
    #wrap{height:100%;overflow:auto}
    #app{display:flex;min-width:1100px;min-height:100%}
    aside{width:232px;flex:none;padding:28px 18px;display:flex;flex-direction:column;gap:28px;border-right:1px solid var(--bd);background:rgba(255,255,255,.02)}
    .logo{display:flex;align-items:center;gap:12px;padding:0 8px;font:700 22px Sora,sans-serif}
    nav{display:flex;flex-direction:column;gap:6px}
    nav button{display:flex;align-items:center;gap:12px;height:44px;padding:0 14px;border:0;border-radius:12px;background:transparent;color:var(--mu);transition:background .2s,color .2s;text-align:left}
    nav button:hover{background:rgba(255,255,255,.05);color:var(--tx)}
    nav button.on{color:#fff;background:rgba(139,92,246,.18);box-shadow:inset 0 0 0 1px rgba(139,92,246,.5)}
    .acc{margin-top:auto;display:flex;align-items:center;gap:12px;padding:12px;border-radius:16px}
    .av{width:40px;height:40px;border-radius:10px;background:linear-gradient(135deg,#8B5CF6,#22D3EE)}
    .acc{text-align:left;width:100%;transition:border-color .2s,background .2s}.acc:hover{background:rgba(255,255,255,.08)}.acc.on{border-color:rgba(139,92,246,.7)}
    .mu{color:var(--mu);font-size:13px}
    main{flex:1;min-width:0;padding:28px 36px;display:flex;flex-direction:column;gap:22px}
    h1{margin:0;font:700 30px Sora,sans-serif}h2{margin:0;font:600 18px Sora,sans-serif}
    .gl{background:var(--gl);border:1px solid var(--bd);border-radius:18px}
    .hero{position:relative;overflow:hidden;min-height:300px;padding:40px;border-radius:24px;display:flex;flex-direction:column;justify-content:flex-end;gap:16px;border:1px solid rgba(255,255,255,.1);background:linear-gradient(135deg,rgba(139,92,246,.45),rgba(192,38,211,.22) 55%,rgba(10,10,18,.2)),#12101F}
    .shard{position:absolute;right:70px;top:24px;width:270px;height:310px;clip-path:polygon(50% 0,100% 30%,80% 100%,20% 100%,0 30%);background:linear-gradient(160deg,rgba(255,255,255,.28),rgba(139,92,246,.4) 40%,rgba(10,10,18,.7));animation:fl 6s ease-in-out infinite}
    .lite .shard{animation:none}
    @keyframes fl{50%{transform:translateY(-10px)}}
    .hero>*{position:relative}.hero .shard{position:absolute}
    .play{height:56px;min-width:190px;padding:0 40px;border:0;border-radius:16px;font:700 18px Sora,sans-serif;letter-spacing:1px;color:#06121A;background:linear-gradient(90deg,#67E8F9,#22D3EE);box-shadow:0 8px 32px rgba(34,211,238,.35);transition:transform .15s}
    .play:hover{transform:translateY(-2px)}.play:disabled{opacity:.85;cursor:default;transform:none}
    .sec{height:56px;padding:0 26px;border-radius:16px;background:rgba(255,255,255,.08);border:1px solid rgba(255,255,255,.14)}
    .g3{display:grid;grid-template-columns:repeat(3,1fr);gap:16px}
    .st{padding:20px}.st b{display:block;margin-top:6px;font:600 26px Sora,sans-serif}
    .pc{display:flex;align-items:center;gap:14px;padding:14px;text-align:left;transition:border-color .2s,background .2s}
    .pc:hover{background:rgba(255,255,255,.08)}.pc.on{border-color:rgba(139,92,246,.7)}
    .th{width:64px;height:64px;flex:none;border-radius:12px}
    .bar{height:8px;border-radius:4px;background:rgba(255,255,255,.1)}.bar i{display:block;height:8px;border-radius:4px;background:linear-gradient(90deg,#22D3EE,#8B5CF6);transition:width .15s}
    .chips{display:flex;gap:10px;flex-wrap:wrap}
    .chip{height:36px;padding:0 16px;border-radius:18px;color:#C9C7DD;background:var(--gl);border:1px solid var(--bd)}
    .chip.on{color:#fff;background:rgba(139,92,246,.18);border-color:rgba(139,92,246,.55)}
    .srch{height:48px;padding:0 18px;border-radius:14px;background:rgba(255,255,255,.06);border:1px solid rgba(255,255,255,.12)}
    .mc{display:flex;flex-direction:column;gap:12px;padding:18px}
    .mi{width:44px;height:44px;border-radius:12px;flex:none}
    .ib{height:38px;margin-top:auto;border:0;border-radius:10px;font-weight:600;color:#fff;background:#7C3AED;transition:background .2s}
    .ib.in{color:#C9C7DD;background:rgba(255,255,255,.08)}
    .row{display:flex;align-items:center;justify-content:space-between;gap:20px;padding:20px 22px}
    .sw{width:50px;height:28px;border-radius:14px;border:0;background:rgba(255,255,255,.14);position:relative;transition:background .2s;flex:none}
    .sw::after{content:"";position:absolute;top:3px;left:3px;width:22px;height:22px;border-radius:50%;background:#fff;transition:left .2s}
    .sw[aria-checked=true]{background:#7C3AED}.sw[aria-checked=true]::after{left:25px}
    input[type=range]{width:260px;accent-color:#8B5CF6}
    select{height:40px;padding:0 12px;border-radius:10px;background:#1A1830;border:1px solid var(--bd);color:var(--tx)}select option{background:#1A1830;color:var(--tx)}
    .lite *{transition:none!important}
    .seg{display:inline-flex;gap:4px;padding:4px;border-radius:14px;background:var(--gl);border:1px solid var(--bd);align-self:flex-start}
    .seg button{height:38px;padding:0 24px;border:0;border-radius:10px;background:transparent;color:var(--mu);font-weight:600;transition:background .2s,color .2s}
    .seg button.on{background:#7C3AED;color:#fff}
    .pf{font-size:12px;font-weight:600;padding:3px 10px;border-radius:10px;background:rgba(255,255,255,.08)}
    .mb{width:34px;height:34px;border-radius:9px;border:1px solid var(--bd);background:rgba(255,255,255,.06);font-size:16px}
    .mb:disabled{opacity:.35;cursor:default}
    .md-back{position:fixed;inset:0;background:rgba(0,0,0,.62);display:flex;align-items:center;justify-content:center;z-index:50;padding:16px}
    .md{max-height:90vh;overflow:auto}
  </style>
</svelte:head>

<div id="wrap"><div id="app">
  <aside>
    <div class="logo"><svg width="34" height="34" viewBox="0 0 34 34" aria-hidden="true"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#C4B5FD"/><stop offset=".5" stop-color="#8B5CF6"/><stop offset="1" stop-color="#C026D3"/></linearGradient></defs><polygon points="17,2 30,12 24,32 10,32 4,12" fill="url(#g)"/><polygon points="17,2 30,12 17,18" fill="rgba(255,255,255,.35)"/></svg>Obsidia</div>
    <nav id="nav"></nav>
    <button class="acc gl" id="acc" data-act="go" data-t="account" aria-label="Abrir mi cuenta"></button>
  </aside>
  <main id="view"></main>
</div></div>
