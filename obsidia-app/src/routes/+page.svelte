<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
import Sidebar from "$lib/components/Sidebar.svelte";
import Toast from "$lib/components/Toast.svelte";

import { I, ic, TABS, G, TRASH, M, LOADERS, RAMS, STG, LBL, esc, norm, validName, normRam, head, skinFace, accCard, modCard, trb, resCard, irisBanner } from "$lib/utils.js";

  // ───────────── Estado ─────────────
  let P = [];
let navHtml = '';
let accHtml = '';
  let VERSIONS = [];
  let S = {
    tab:'home', p:0, sec:'mods', q:'', f:'Todos',
    pg:null, stage:'version', launchErr:'', launchMsg:'',
    defRam:4, totalRam:16, t:{ lite:false },
    nick:'Jugador', ms:false, ml:false, msc:null,
    skins:[], sk:-1, skmodel:'classic',
    pstate:[],
    files:{ mods:[], shaders:[], packs:[] },
    results:{ mods:null, shaders:null, packs:null }, searching:false, seq:0, busy:{},
    sv:[], af:false, rf:false, es:null, ns:'', na:'',
    np:false, npEdit:-1, npName:'', npVersion:'', npLoader:'Vanilla', npRam:4, npJava:'', npErr:'', npDel:false, npBusy:false, npFullscreen:false,
    javas:[]
  };

  // ───────────── Utilidades ─────────────
  const Q = s => document.querySelector(s);
  const findFile = (kind, key) => S.files[kind].find(f => norm(f).includes(norm(key)));
  const refreshGrid = () => { const g = Q('#grid'); if (g) g.innerHTML = grid(); };

  function toast(msg, bad){
    const t = Q('#toast'); if (!t) return;
    t.textContent = msg; t.className = 'toast show' + (bad ? ' bad' : '');
    clearTimeout(toast.t); toast.t = setTimeout(() => { t.className = 'toast'; }, 5000);
  }

  const sw = (k,l,d) => `<div class="row gl"><div><div style="font-weight:600">${l}</div><div class="mu">${d}</div></div><button class="sw" role="switch" aria-label="${l}" aria-checked="${S.t[k]}" data-act="tg" data-k="${k}"></button></div>`;
  const seg = () => `<div class="seg">${[['mods','Mods'],['shaders','Shaders'],['packs','Resource packs']].map(s=>`<button class="${S.sec==s[0]?'on':''}" data-act="seg" data-s="${s[0]}">${s[1]}</button>`).join('')}</div>`;

  // ───────────── Skins ─────────────
  // Cara por defecto (Steve).
  // accCard moved to utils.js

  // ───────────── Perfiles ─────────────
  function cards(){
    return P.map((p,i)=>`<div class="pc gl ${i==S.p?'on':''}" role="button" tabindex="0" data-act="sel" data-i="${i}"><div class="th" style="background:${G[i%4]}"></div><div style="flex:1;min-width:0"><div style="font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(p.n)}</div><div class="mu">${i==S.p?'Activo · ':''}${esc(p.s)}</div></div><button class="mb" style="width:auto;height:32px;padding:0 12px;font-size:13px" data-act="ped" data-i="${i}">Editar</button></div>`).join('');
  }
  const newCard = `<button class="pc gl" style="border-style:dashed;justify-content:center;min-height:92px;color:var(--mu)" data-act="new">+ Nuevo perfil</button>`;

  function npModal(){
    const title = S.npEdit>=0 ? 'Editar perfil' : 'Nuevo perfil';
    const rams = RAMS.filter(r => r <= (S.totalRam||16));
    if (!rams.includes(S.npRam)) rams.push(S.npRam), rams.sort((a,b)=>a-b);
    const versions = VERSIONS.includes(S.npVersion) ? VERSIONS : [S.npVersion, ...VERSIONS];
    return `<div class="md-back" data-act="npclose"><div class="md gl" style="width:min(520px,92vw);padding:24px;display:flex;flex-direction:column;gap:14px">
      <div style="display:flex;justify-content:space-between;align-items:center"><h2 style="margin:0">${title}</h2><button class="mb" data-act="npclose" aria-label="Cerrar">✕</button></div>
      <div><label class="mu" for="np-name">Nombre del perfil</label><input id="np-name" class="srch" style="width:100%;margin-top:6px" maxlength="24" autocomplete="off" value="${esc(S.npName)}"></div>
      <div><label class="mu" for="np-version">Versión de Minecraft</label><select id="np-version" class="srch" style="width:100%;margin-top:6px">${versions.map(v=>`<option ${v==S.npVersion?'selected':''}>${esc(v)}</option>`).join('')}</select></div>
      <div><label class="mu" for="np-loader">Loader</label><select id="np-loader" class="srch" style="width:100%;margin-top:6px">${LOADERS.map(l=>`<option ${l==S.npLoader?'selected':''}>${l}</option>`).join('')}</select><div class="mu" style="margin-top:6px">Fabric y Forge se instalan solos la primera vez que jugás.</div></div>
      <div style="display:grid;grid-template-columns:1fr 1fr;gap:14px">
        <div><label class="mu" for="np-ram">RAM asignada</label><select id="np-ram" class="srch" style="width:100%;margin-top:6px">${rams.map(r=>`<option value="${r}" ${r==S.npRam?'selected':''}>${r} GB</option>`).join('')}</select></div>
        <div><label class="mu" for="np-java">Java</label><select id="np-java" class="srch" style="width:100%;margin-top:6px"><option value="">Automático (recomendado)</option>${S.javas.map(j=>`<option value="${esc(j.path)}" ${j.path==S.npJava?'selected':''}>${esc(j.version||j.path)}</option>`).join('')}</select></div>
<div style="display:flex;align-items:center;gap:10px"><input type="checkbox" id="np-fullscreen" ${S.npFullscreen?'checked':''}><label class="mu" for="np-fullscreen">Pantalla completa</label></div>
      <div style="display:flex;gap:10px;justify-content:space-between;margin-top:6px">
        <div>${S.npEdit>=0?`<button class="sec" style="height:44px;color:#F87171" data-act="npdel">${S.npDel?'¿Seguro? Tocá de nuevo':'Eliminar perfil'}</button>`:''}</div>
        <div style="display:flex;gap:10px"><button class="sec" style="height:44px" data-act="npclose">Cancelar</button><button class="ib" style="margin:0;height:44px;padding:0 24px" data-act="npcreate" ${S.npBusy?'disabled':''}>${S.npEdit>=0?'Guardar':'Crear perfil'}</button></div>
      </div>
    </div></div>`;
  }

  function msModal(){
    const c = S.msc;
    return `<div class="md-back" data-act="msclose"><div class="md gl" style="width:min(460px,92vw);padding:26px;display:flex;flex-direction:column;gap:14px;text-align:center">
      <h2 style="margin:0">Iniciar sesión con Microsoft</h2>
      <div class="mu">1. Abrí la página de Microsoft<br>2. Ingresá este código</div>
      <div style="font:700 34px Sora,sans-serif;letter-spacing:6px;padding:14px;border-radius:14px;background:rgba(255,255,255,.06)">${esc(c.user_code)}</div>
      <div style="display:flex;gap:10px;justify-content:center"><button class="ib" style="margin:0;height:44px;padding:0 22px" data-act="msopen" data-url="${esc(c.verification_uri)}">Abrir ${esc(c.verification_uri.replace(/^https?:\/\//,''))}</button><button class="sec" style="height:44px" data-act="mscopy" data-code="${esc(c.user_code)}">Copiar código</button></div>
      <div class="mu">Esperando que completes el inicio de sesión…</div>
      <button class="sec" style="height:40px" data-act="msclose">Cancelar</button>
    </div></div>`;
  }

  // ───────────── Pantalla de inicio ─────────────
  function home(){
    const p = P[S.p];
    const g = S.pg;
    const btn = g===null ? 'JUGAR' : g<100 ? 'Preparando… '+g+' %' : 'EN JUEGO';
    return `<section class="hero"><div class="shard" aria-hidden="true"></div><div class="mu" style="color:#C4B5FD;font-weight:600;letter-spacing:2px;font-size:12px">PERFIL ACTIVO</div><h1 style="font-size:42px;line-height:1.1">${p?esc(p.n):'Sin perfil'}</h1><div style="color:#C9C7DD">${p?esc(p.s)+' · '+p.ram+' GB de RAM':'Creá un perfil para empezar a jugar.'}</div><div style="display:flex;gap:12px;flex-wrap:wrap;margin-top:6px"><button class="play" data-act="play" ${g!==null?'disabled':''}>${p?btn:'Crear perfil'}</button>${p?`<button class="sec" data-act="ped" data-i="${S.p}">Configurar</button>`:''}</div></section>
    ${g!==null?`<div class="gl" style="padding:14px 18px"><div style="display:flex;justify-content:space-between;font-size:13px;color:#C9C7DD"><span>${LBL[S.stage]||''}</span><span>${g} %</span></div><div class="bar" style="margin-top:10px"><i style="width:${g}%"></i></div></div>`:''}
    ${S.launchErr?`<div class="gl" style="padding:14px 18px;border-color:rgba(248,113,113,.5)"><b style="color:#F87171">No se pudo abrir el juego</b><div class="mu" style="margin-top:6px;white-space:pre-wrap;word-break:break-word">${esc(S.launchErr)}</div></div>`:''}
    ${S.launchMsg?`<div class="gl" style="padding:14px 18px;border-color:rgba(52,211,153,.4)"><div class="mu" style="color:#6EE7B7;word-break:break-word">${esc(S.launchMsg)}</div></div>`:''}
    <h2>Tus perfiles</h2><div class="g3">${cards()}${newCard}</div>`;
  }
  function profiles(){ return `<h1>Perfiles</h1><div class="mu">Elegí con cuál jugar o creá uno nuevo.</div><div class="g3">${cards()}${newCard}</div>`; }

  // ───────────── Mods / shaders / resource packs ─────────────
  function installedList(kind){
    const files = S.files[kind];
    if (!files.length) return '';
    const names = { mods:'Mods instalados', shaders:'Shaders instalados', packs:'Resource packs instalados' };
    return `<div class="gl" style="grid-column:1/-1;padding:18px 20px;display:flex;flex-direction:column;gap:8px"><b>${names[kind]}</b>${files.map(f=>`<div style="display:flex;align-items:center;gap:12px;padding:6px 12px;border-radius:12px;background:rgba(255,255,255,.04)"><span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(f)}</span>${trb(kind,f,34)}</div>`).join('')}</div>`;
  }
  function grid(){
    const kind = S.sec, q = S.q.trim(), res = S.results[kind];
    let h = '';
    if (kind==='shaders' && !findFile('mods','iris')) h += irisBanner(S.busy['iris']);
    if (kind==='mods' && !q){
      const list = M.filter(m => S.f=='Todos' || m[2]==S.f);
      h += list.map(m => modCard(m, S.busy[m[4]], findFile('mods', m[4]))).join('') || '<div class="mu">No hay resultados.</div>';
    } else if (!res){
      h += `<div class="mu">${S.searching?'Buscando en Modrinth…':'Cargando…'}</div>`;
    } else {
      h += res.length ? res.map(r => resCard(kind, r, S.busy[r.slug], findFile(kind, r.slug))).join('') : '<div class="mu">No hay resultados en Modrinth.</div>';
    }
    return h + installedList(kind);
  }
  // Panel de orden de resource packs (lee y escribe options.txt).
  function packsPanel(){
    const list = S.pstate || [];
    if (!list.length) return `<div class="gl" style="padding:18px 20px"><b>Resource packs</b><div class="mu" style="margin-top:6px">No hay packs instalados todavía. Instalá uno abajo y aparece acá.</div></div>`;
    const en = list.filter(x=>x.enabled).length;
    const rows = list.map((x,i)=>`<div style="display:flex;align-items:center;gap:12px;padding:8px 12px;border-radius:12px;background:rgba(255,255,255,.04)"><button class="sw" role="switch" aria-label="${esc(x.name)}" aria-checked="${x.enabled}" data-act="ptg" data-i="${i}"></button><span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(x.name)}</span><button class="mb" data-act="pup" data-i="${i}" ${x.enabled&&i>0?'':'disabled'} aria-label="Subir ${esc(x.name)}">↑</button><button class="mb" data-act="pdown" data-i="${i}" ${x.enabled&&i<en-1?'':'disabled'} aria-label="Bajar ${esc(x.name)}">↓</button></div>`).join('');
    return `<div class="gl" style="padding:18px 20px;display:flex;flex-direction:column;gap:10px"><div style="display:flex;justify-content:space-between;align-items:center;gap:16px;flex-wrap:wrap"><div><b>Resource packs</b><div class="mu">El orden es la prioridad: el de arriba gana.</div></div><button class="sec" style="height:38px;padding:0 16px" data-act="pauto">Ordenar automático</button></div>${rows}</div>`;
  }
  function mods(){
    const p = P[S.p];
    const ph = { mods:'Buscar en Modrinth…', shaders:'Buscar shaders…', packs:'Buscar resource packs…' }[S.sec];
    return `<div style="display:flex;justify-content:space-between;align-items:center"><h1>Mods</h1><div class="gl" style="padding:9px 16px;color:#C9C7DD;font-size:14px">Perfil: ${p?esc(p.n)+' · '+esc(p.loader||'Vanilla'):'—'}</div></div>${seg()}${S.sec==='packs'?packsPanel():''}<input id="q" class="srch" aria-label="${ph}" placeholder="${ph}" value="${esc(S.q)}">${S.sec==='mods'&&!S.q.trim()?`<div class="chips">${['Todos','Optimización','Gráficos','Utilidades'].map(c=>`<button class="chip ${c==S.f?'on':''}" data-act="f" data-c="${c}">${c}</button>`).join('')}</div>`:''}<div class="g3" id="grid">${grid()}</div>`;
  }

  async function refreshFiles(){
    const [m, s, r, ps] = await Promise.all([
      invoke('list_installed_mods').catch(()=>[]),
      invoke('list_shaderpacks').catch(()=>[]),
      invoke('list_resourcepacks').catch(()=>[]),
      invoke('get_resourcepack_state').catch(()=>[])
    ]);
    S.files = { mods:m||[], shaders:s||[], packs:r||[] };
    S.pstate = ps||[];
  }
  async function savePacks(){
    const active = S.pstate.filter(x=>x.enabled).map(x=>x.name);
    try { await invoke('save_resourcepack_state', { active }); }
    catch(e){ toast('No se pudo guardar el orden de packs: '+e, true); }
    await refreshFiles(); refreshGrid();
  }

  async function doSearch(){
    const kind = S.sec, q = S.q.trim(), my = ++S.seq;
    if (kind==='mods' && !q){ S.results.mods = null; S.searching = false; refreshGrid(); return; }
    S.searching = true; S.results[kind] = null; refreshGrid();
    const cmd = { mods:'search_mods', shaders:'search_shaders', packs:'search_resourcepacks' }[kind];
    let res = [];
    try { res = await invoke(cmd, { query:q }); } catch(e){ if (my===S.seq) toast('La búsqueda falló: '+e, true); }
    if (my !== S.seq) return;
    S.results[kind] = Array.isArray(res) ? res : [];
    S.searching = false; refreshGrid();
  }

  async function doInstall(kind, slug){
    if (S.busy[slug]) return;
    const p = P[S.p];
    if (kind==='mods'){
      if (!p){ toast('Creá un perfil primero.', true); return; }
      if ((p.loader||'Vanilla')==='Vanilla'){ toast('Este perfil es Vanilla: creá un perfil Fabric o Forge para usar mods.', true); return; }
    }
    S.busy[slug] = 1; refreshGrid();
    try {
      if (kind==='mods') await invoke('install_mod', { slug, mcVersion:p.version, loader:p.loader.toLowerCase() });
      else if (kind==='shaders') await invoke('install_shaderpack', { slug, mcVersion: (P[S.p]&&P[S.p].version)||null });
      else await invoke('install_resourcepack', { slug, mcVersion: (P[S.p]&&P[S.p].version)||null });
      toast('Instalado correctamente.');
    } catch(e){ toast('No se pudo instalar: '+e, true); }
    delete S.busy[slug];
    await refreshFiles(); refreshGrid();
  }

  // ───────────── Servidores ─────────────
  function servers(){
    const pc = x => x<60?'#34D399':x<120?'#FBBF24':'#F87171';
    return `<div style="display:flex;justify-content:space-between;align-items:center;gap:16px"><h1>Servidores</h1><div style="display:flex;gap:10px"><button class="sec" style="height:44px" data-act="sref">${S.rf?'Actualizando…':'Actualizar'}</button><button class="ib" style="margin:0;padding:0 22px;height:44px" data-act="sadd">+ Añadir servidor</button></div></div>
    ${S.af?`<div class="gl" style="padding:18px 20px;display:flex;gap:12px;align-items:center;flex-wrap:wrap"><input id="sn" class="srch" style="flex:1;min-width:180px" aria-label="Nombre del servidor" placeholder="Nombre del servidor" value="${esc(S.ns)}"><input id="sa" class="srch" style="flex:1;min-width:220px" aria-label="Dirección del servidor" placeholder="Dirección (ej. mi.servidor.net)" value="${esc(S.na)}"><button class="ib" style="margin:0;padding:0 24px;height:48px" data-act="ssave">${S.es!==null?'Guardar cambios':'Guardar'}</button><button class="sec" style="height:48px" data-act="scancel">Cancelar</button></div>`:''}
    ${S.sv.length?'':'<div class="mu">Todavía no agregaste servidores.</div>'}
    <div style="display:grid;grid-template-columns:repeat(auto-fill,minmax(340px,1fr));gap:16px">${S.sv.map((s,i)=>`<div class="gl" style="padding:18px;display:flex;flex-direction:column;gap:14px"><div style="display:flex;align-items:center;gap:14px"><div class="th" style="width:52px;height:52px;background:${G[i%4]}"></div><div style="flex:1;min-width:0"><b>${esc(s.n)}</b><div class="mu" style="overflow:hidden;text-overflow:ellipsis">${esc(s.a)}</div></div><span class="pf" style="color:${s.on===null?'#C9C7DD':s.on?'#34D399':'#F87171'}">${s.on===null?'Comprobando…':s.on?'En línea':'Sin conexión'}</span></div>${s.on?`<div style="display:flex;gap:10px"><span class="pf" style="color:${pc(s.pg)}">${s.pg} ms</span></div>`:''}<div style="display:flex;gap:10px"><button class="ib" style="flex:1;margin:0" data-act="join" data-i="${i}">Unirse</button><button class="sec" style="height:38px;padding:0 14px" data-act="sedit" data-i="${i}">Editar</button><button class="mb" style="width:auto;height:38px;padding:0 14px;font-size:14px" aria-label="Quitar ${esc(s.n)}" data-act="sdel" data-i="${i}">Quitar</button></div></div>`).join('')}</div>`;
  }
  async function pingAll(){
    S.rf = true; if (S.tab==='servers') render();
    await Promise.all(S.sv.map(s => invoke('ping_server', { address:s.a }).then(r => {
      const m = /\((\d+) ms\)/.exec(String(r)); s.on = true; s.pg = m ? +m[1] : 0;
    }).catch(() => { s.on = false; })));
    S.rf = false; if (S.tab==='servers') render();
  }
  async function loadServers(){
    const list = await invoke('get_servers').catch(()=>[]);
    S.sv = (list||[]).map(s => ({ n:s.name, a:s.address, v:s.version, on:null, pg:0 }));
  }

  // ───────────── Cuenta ─────────────
  function account(){
    const ok = validName(S.nick);
    const help = 'En modo offline la skin se aplica con un resource pack automático y solo la ves vos. Con cuenta Microsoft se usa tu skin oficial.';
    const model = (m,l) => `<button class="chip ${S.skmodel==m?'on':''}" data-act="skinmodel" data-m="${m}">${l}</button>`;
    return `<h1>Mi cuenta</h1><div style="display:grid;grid-template-columns:1fr 1.3fr;gap:16px;align-items:start"><div class="gl" style="padding:26px;display:flex;flex-direction:column;align-items:center;gap:18px">${skinFace(S.sk,132,S.skins)}<div style="width:100%"><label class="mu" for="nick">Nombre de jugador</label><input id="nick" class="srch" style="width:100%;margin-top:6px" maxlength="16" autocomplete="off" value="${esc(S.nick)}" ${S.ms?'disabled':''}><div id="nm" style="margin-top:8px;font-size:13px;color:${ok?'#6EE7B7':'#FCD34D'}">${ok?'Nombre válido ✓':'Usá de 3 a 16 letras, números o guion bajo.'}</div></div></div><div style="display:flex;flex-direction:column;gap:16px"><div class="gl" style="padding:20px 22px;display:flex;flex-direction:column;gap:10px"><b>Tipo de cuenta</b><div style="color:${S.ms?'#6EE7B7':'#C9C7DD'}">${S.ms?'Cuenta Microsoft vinculada':'Estás jugando en modo offline'}</div><div class="mu">${S.ms?'Podés entrar a servidores premium y usar tu skin oficial.':'En modo offline no podés entrar a servidores que verifican la cuenta.'}</div>${S.ms?'<button class="sec" style="height:44px" data-act="mso">Cerrar sesión</button>':`<button class="ib" style="margin:0;height:44px" ${S.ml?'disabled':''} data-act="ms">${S.ml?'Esperando a Microsoft…':'Iniciar sesión con Microsoft'}</button>`}</div><div class="gl" style="padding:20px 22px;display:flex;flex-direction:column;gap:14px"><b>Skin</b><div class="mu">${help}</div><div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap"><span class="mu">Modelo:</span>${model('classic','Clásico')}${model('slim','Fino')}</div><div style="display:grid;grid-template-columns:repeat(4,1fr);gap:12px">${S.skins.map((u,i)=>`<button class="pc gl ${i==S.sk?'on':''}" style="flex-direction:column;padding:12px" data-act="skin" data-i="${i}">${skinFace(i,56,S.skins)}<span class="mu">Skin ${i+1}</span></button>`).join('')}${S.skins.length===0?`<div class="mu" style="grid-column:1/-1">No hay skins cargadas. Subí una para empezar (máx. 4).</div>`:''}</div><input type="file" id="skinfile" accept="image/png" style="display:none"><div style="display:flex;gap:10px;flex-wrap:wrap"><button class="ib" style="margin:0;height:44px;flex:1;min-width:180px" data-act="skinup" ${S.skins.length>=4?'disabled':''}>${S.skins.length>=4?'Límite de 4 skins alcanzado':'Subir skin (PNG)'}</button><button class="sec" style="height:44px" data-act="skindel">Quitar skin del juego</button></div></div></div></div>`;
  }
  function saveSkins(){ try { localStorage.setItem('obsidia.skins', JSON.stringify(S.skins)); localStorage.setItem('obsidia.sk', String(S.sk)); localStorage.setItem('obsidia.skmodel', S.skmodel); } catch(e){} }

  // ───────────── Ajustes ─────────────
  function settings(){
    return `<h1>Ajustes</h1><div class="row gl"><div><div style="font-weight:600">Memoria RAM por defecto</div><div class="mu">Se usa al crear perfiles nuevos. Cada perfil guarda la suya.</div></div><div style="display:flex;align-items:center;gap:14px"><input type="range" id="ram" min="2" max="${Math.max(2,S.totalRam)}" value="${S.defRam}" aria-label="Memoria RAM"><b id="ramv" style="width:60px">${S.defRam} GB</b></div></div><div class="row gl"><div><div style="font-weight:600">Java</div><div class="mu">Automático: Obsidia descarga el Java que pide cada versión. Para usar otro, editá el perfil.</div></div></div>${sw('lite','Modo ligero','Sin animaciones, ideal para PCs modestas')}`;
  }

  // ───────────── Render ─────────────
  function render(){
    document.body.classList.toggle('lite', S.t.lite);
    accHtml = accCard(S.sk, S.nick, S.ms, S.skins);
    navHtml = TABS.map(t=>`<button class="${t[0]==S.tab?'on':''}" data-act="go" data-t="${t[0]}">${ic(t[0])}${t[1]}</button>`).join('');
    let html = { home, profiles, mods, servers, settings, account }[S.tab]();
    if (S.np) html += npModal();
    if (S.msc) html += msModal();
    Q('#view').innerHTML = html;
  }

  // ───────────── Lanzar el juego ─────────────
  async function findLoaderVersion(p){
    const inst = await invoke('get_installed_versions').catch(()=>[]);
    const mc = p.version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const rx = p.loader==='Fabric' ? new RegExp('^fabric-loader-.+-'+mc+'$') : new RegExp('^'+mc+'-forge-');
    // Orden numérico ("0.16.10" > "0.9.3"); el orden alfabético elegiría el loader equivocado.
    return (inst||[]).filter(v => rx.test(v)).sort((a,b) => a.localeCompare(b, undefined, { numeric:true })).pop() || null;
  }
  async function launchVersionId(p){
    if (!p.loader || p.loader==='Vanilla') return p.version;
    let id = await findLoaderVersion(p);
    if (id) return id;
    S.stage = 'loader'; S.pg = 3; render();
    await invoke(p.loader==='Fabric' ? 'install_fabric' : 'install_forge', { mcVersion:p.version });
    id = await findLoaderVersion(p);
    if (!id) throw new Error('No encontré el loader '+p.loader+' después de instalarlo.');
    return id;
  }
  // Aplica la skin elegida al resource pack del juego (solo modo offline).
  async function applySkinToGame(silent){
    if (S.ms){ if (!silent) toast('Con cuenta Microsoft se usa tu skin oficial: no se aplica el pack offline.', true); return false; }
    const url = S.skins[S.sk];
    if (!url) return false;
    try {
      await invoke('apply_offline_skin', { skinPngBase64:String(url), model:S.skmodel||'classic', mcVersion:(P[S.p]&&P[S.p].version)||null });
      if (!silent) toast('Skin aplicada al juego.');
      return true;
    } catch(e){ if (!silent) toast('No se pudo aplicar la skin: '+e, true); return false; }
  }
  async function play(server){
    if (S.pg !== null) return;
    const p = P[S.p];
    if (!p){ openNew(); return; }
    if (!validName(S.nick)){ toast('Tu nombre de jugador no es válido. Cambialo en Mi cuenta.', true); S.tab = 'account'; render(); return; }
    S.pg = 0; S.stage = 'version'; S.launchErr = ''; S.launchMsg = ''; render();
    let un = null;
    try {
      un = await listen('launch-progress', ev => {
        const { stage, done, total } = ev.payload || {};
        const r = STG[stage] || [0,99];
        S.stage = stage;
        S.pg = Math.min(99, Math.round(r[0] + (r[1]-r[0]) * (total > 0 ? done/total : 0)));
        if (S.tab==='home') render();
      });
    } catch(e){ /* sin eventos: igual se lanza */ }
    try {
      const id = await launchVersionId(p);
      // Volver a aplicar la skin elegida: si cambió el pack activo, igual queda puesta.
      if (S.ms) invoke('remove_offline_skin').catch(()=>{});
      else if (S.skins[S.sk]) await applySkinToGame(true);
      const msg = await invoke('launch_version', { version:id, username:S.nick, ram:p.ram+'G', javaPath:p.java||null, server:server||null, fullscreen:!!p.fullscreen });
      S.pg = 100; S.launchMsg = msg; render();
      setTimeout(() => { S.pg = null; render(); }, 2500);
    } catch(e){
      S.pg = null; S.launchErr = String(e); render();
    } finally { if (un) un(); }
  }

  // ───────────── Persistencia ─────────────
  async function persistSettings(){
    try { await invoke('save_settings', { settings:{ ram:S.defRam+'G', java_path:null, light_mode:S.t.lite, console:false, username:S.nick } }); } catch(e){ console.error(e); }
  }
  async function loadProfiles(select){
    const list = await invoke('get_profiles').catch(()=>[]);
    P = (list||[]).map(p => {
      const loader = p.loader || 'Vanilla';
      return { n:p.name, version:p.version, loader, ram:normRam(p.ram), java:p.java_path||'', username:p.username, fullscreen:!!p.fullscreen, s:loader+' · '+p.version };
    });
    let i = P.findIndex(x => x.n === select);
    if (i < 0) { try { i = P.findIndex(x => x.n === localStorage.getItem('obsidia.sel')); } catch(e){ i = -1; } }
    S.p = i >= 0 ? i : 0;
  }
  function selectProfile(i){
    S.p = i;
    try { localStorage.setItem('obsidia.sel', P[i] ? P[i].n : ''); } catch(e){}
  }
  function openNew(){
    S.np = true; S.npEdit = -1; S.npName = ''; S.npVersion = VERSIONS[0] || '1.21.4'; S.npLoader = 'Vanilla';
    S.npRam = Math.min(S.defRam, S.totalRam); S.npJava = ''; S.npErr = ''; S.npDel = false; S.npBusy = false; S.npFullscreen = false; render();
  }
  function openEdit(i){
    const p = P[i]; if (!p) return;
    S.np = true; S.npEdit = i; S.npName = p.n; S.npVersion = p.version; S.npLoader = p.loader || 'Vanilla';
    S.npRam = p.ram; S.npJava = p.java || ''; S.npErr = ''; S.npDel = false; S.npBusy = false; S.npFullscreen = !!p.fullscreen; render();
  }
  async function saveProfileFromModal(){
    const val = id => { const e = Q(id); return e ? e.value : ''; };
    const name = val('#np-name').trim() || ('Perfil '+(P.length+1));
    S.npName = name; S.npVersion = val('#np-version'); S.npLoader = val('#np-loader') || 'Vanilla';
    S.npRam = +val('#np-ram') || 4; S.npJava = val('#np-java'); S.npFullscreen = !!(Q('#np-fullscreen') && Q('#np-fullscreen').checked);
    const old = S.npEdit >= 0 ? P[S.npEdit] : null;
    if (P.some((x,i) => x.n === name && i !== S.npEdit)){ S.npErr = 'Ya existe un perfil con ese nombre.'; render(); return; }
    S.npBusy = true;
    try {
      // Primero se guarda el nuevo y recién después se borra el viejo: si algo falla no se pierde el perfil.
      await invoke('save_profile', { profile:{ name, version:S.npVersion, username:S.nick, ram:S.npRam+'G', java_path:S.npJava||null, loader:S.npLoader, fullscreen:S.npFullscreen } });
      if (old && old.n !== name) await invoke('delete_profile', { name:old.n });
      await loadProfiles(name);
      selectProfile(S.p);
      S.np = false; S.npEdit = -1; S.npErr = '';
    } catch(e){ S.npErr = String(e); }
    S.npBusy = false; render();
  }

  // ───────────── Eventos ─────────────
  // Cerrar el modal de Microsoft también cancela la espera en Rust: así no queda una sesión
  // guardada en segundo plano mientras la interfaz dice "offline".
  function closeMs(){ invoke('ms_cancel').catch(()=>{}); S.msc = null; render(); }
  async function onClick(e){
    const b = e.target.closest('[data-act]');
    if (!b) return;
    const a = b.dataset.act, d = b.dataset;
    if (a=='go'){ S.tab = d.t; render(); if (d.t==='servers') pingAll(); if (d.t==='mods'){ refreshFiles().then(()=>{ if (S.tab==='mods') doSearch(); }); } }
    else if (a=='sel'){ selectProfile(+d.i); render(); }
    else if (a=='play'){ play(); }
    else if (a=='new'){ openNew(); }
    else if (a=='ped'){ openEdit(+d.i); }
    else if (a=='npclose'){ if (e.target !== b) return; S.np = false; S.npEdit = -1; render(); }
    else if (a=='npcreate'){ saveProfileFromModal(); }
    else if (a=='npdel'){
      if (!S.npDel){ S.npDel = true; render(); return; }
      const p = P[S.npEdit];
      try { if (p) await invoke('delete_profile', { name:p.name || p.n }); await loadProfiles(); S.p = 0; S.np = false; S.npEdit = -1; }
      catch(err){ S.npErr = String(err); }
      render();
    }
    else if (a=='f'){ S.f = d.c; render(); }
    else if (a=='tg'){ S.t[d.k] = !S.t[d.k]; persistSettings(); render(); }
    else if (a=='seg'){ S.sec = d.s; S.q = ''; render(); doSearch(); }
    else if (a=='ptg'){ const x = S.pstate[+d.i]; if (x){ x.enabled = !x.enabled; S.pstate = [...S.pstate.filter(p=>p.enabled), ...S.pstate.filter(p=>!p.enabled)]; savePacks(); render(); } }
    else if (a=='pup'){ const i = +d.i; if (i>0 && S.pstate[i].enabled && S.pstate[i-1].enabled){ [S.pstate[i-1], S.pstate[i]] = [S.pstate[i], S.pstate[i-1]]; savePacks(); render(); } }
    else if (a=='pdown'){ const i = +d.i; if (i<S.pstate.length-1 && S.pstate[i].enabled && S.pstate[i+1].enabled){ [S.pstate[i+1], S.pstate[i]] = [S.pstate[i], S.pstate[i+1]]; savePacks(); render(); } }
    else if (a=='pauto'){ S.pstate = [...S.pstate].sort((x,y) => (Number(y.enabled)-Number(x.enabled)) || x.name.localeCompare(y.name)); savePacks(); render(); }
    else if (a=='inst'){ doInstall(d.kind, d.slug); }
    else if (a=='fdel'){
      const folder = { mods:'mods', shaders:'shaderpacks', packs:'resourcepacks' }[d.kind];
      try { await invoke('delete_file_in', { folder, name:d.file }); toast('Eliminado.'); }
      catch(err){ toast('No se pudo eliminar: '+err, true); }
      await refreshFiles(); refreshGrid();
    }
    else if (a=='openmod'){
      const type = { mods:'mod', shaders:'shader', packs:'resourcepack' }[d.kind];
      const url = `https://modrinth.com/${type}/${encodeURIComponent(d.slug)}`;
      invoke('plugin:opener|open_url', { url }).catch(() => window.open(url, '_blank'));
    }
    else if (a=='sadd'){ S.af = !S.af; if (S.af){ S.es = null; S.ns = ''; S.na = ''; } render(); }
    else if (a=='scancel'){ S.af = false; S.es = null; S.ns = ''; S.na = ''; render(); }
    else if (a=='sedit'){ const s = S.sv[+d.i]; if (s){ S.af = true; S.es = s.a; S.ns = s.n; S.na = s.a; render(); } }
    else if (a=='ssave'){
      const nm = ((Q('#sn')||{}).value || S.ns || '').trim();
      const ad = ((Q('#sa')||{}).value || S.na || '').trim();
      if (!nm || !ad){ toast('Completá el nombre y la dirección.', true); return; }
      try {
        if (S.es && S.es !== ad) await invoke('delete_server', { address:S.es });
        await invoke('save_server', { name:nm, address:ad, version:(P[S.p] && P[S.p].version) || null });
        await loadServers(); S.af = false; S.es = null; S.ns = ''; S.na = ''; render(); pingAll();
      } catch(err){ toast('No se pudo guardar: '+err, true); }
    }
    else if (a=='sdel'){ const s = S.sv[+d.i]; if (s){ await invoke('delete_server', { address:s.a }).catch(()=>{}); S.sv.splice(+d.i,1); render(); } }
    else if (a=='sref'){ pingAll(); }
    else if (a=='join'){
      const s = S.sv[+d.i];
      if (s){ S.tab = 'home'; render(); play(s.a); }
    }
    else if (a=='skin'){ S.sk = +d.i; saveSkins(); render(); applySkinToGame(); }
    else if (a=='skinmodel'){ S.skmodel = d.m==='slim'?'slim':'classic'; saveSkins(); render(); applySkinToGame(); }
    else if (a=='skindel'){
      try { await invoke('remove_offline_skin'); toast('Skin quitada del juego.'); }
      catch(err){ toast('No se pudo quitar la skin: '+err, true); }
    }
    else if (a=='skinup'){ const fi = Q('#skinfile'); if (fi) fi.click(); }
    else if (a=='ms'){
      S.ml = true; render();
      try {
        const dc = await invoke('ms_device_code');
        S.msc = dc; render();
        const profile = await invoke('ms_login', { deviceCode:dc.device_code });
        if (S.msc){ S.ms = true; S.nick = (profile && profile.name) ? profile.name : S.nick; persistSettings(); toast('Sesión de Microsoft iniciada.'); }
      } catch(err){ if (S.msc) toast('No se pudo iniciar sesión: '+err, true); }
      S.ml = false; S.msc = null; render();
    }
    else if (a=='msclose'){ if (e.target !== b) return; closeMs(); }
    else if (a=='msopen'){ invoke('plugin:opener|open_url', { url:d.url }).catch(() => window.open(d.url, '_blank')); }
    else if (a=='mscopy'){ try { await navigator.clipboard.writeText(d.code); toast('Código copiado.'); } catch(err){} }
    else if (a=='mso'){ invoke('ms_logout').catch(()=>{}); S.ms = false; render(); }
  }

  function onChange(e){
    if (e.target && e.target.id=='skinfile'){
      const f = e.target.files && e.target.files[0];
      if (f && S.skins.length < 4){
        const rd = new FileReader();
        rd.onload = () => { S.skins.push(String(rd.result)); S.sk = S.skins.length - 1; saveSkins(); render(); applySkinToGame(); };
        rd.readAsDataURL(f);
      }
      e.target.value = '';
    }
  }
  function onInput(e){
    const id = e.target.id;
    if (id=='q'){ S.q = e.target.value; clearTimeout(S.dt); S.dt = setTimeout(doSearch, 350); }
    else if (id=='ram'){ S.defRam = +e.target.value; const rv = Q('#ramv'); if (rv) rv.textContent = S.defRam+' GB'; clearTimeout(S.pt); S.pt = setTimeout(persistSettings, 400); }
    else if (id=='nick'){
      S.nick = e.target.value; const acc = Q('#acc'); if (acc) acc.innerHTML = accCard();
      const ok = validName(S.nick); const nm = Q('#nm');
      if (nm){ nm.textContent = ok ? 'Nombre válido ✓' : 'Usá de 3 a 16 letras, números o guion bajo.'; nm.style.color = ok ? '#6EE7B7' : '#FCD34D'; }
      if (ok){ clearTimeout(S.pt); S.pt = setTimeout(persistSettings, 400); }
    }
    else if (id=='sn'){ S.ns = e.target.value; }
    else if (id=='sa'){ S.na = e.target.value; }
  }
  function onKey(e){
    if ((e.key==='Enter' || e.key===' ') && e.target.matches && e.target.matches('[role=button][data-act]')){ e.preventDefault(); e.target.click(); }
    if (e.key==='Escape' && (S.np || S.msc)){ if (S.msc) invoke('ms_cancel').catch(()=>{}); S.np = false; S.msc = null; S.npEdit = -1; render(); }
  }

  // ───────────── Arranque ─────────────
  async function init(){
    try {
      const [remote, settings, msProfile, totalRam, javas] = await Promise.all([
        invoke('list_versions').catch(()=>[]),
        invoke('get_settings').catch(()=>null),
        invoke('ms_get_profile').catch(()=>null),
        invoke('get_total_ram').catch(()=>16),
        invoke('detect_javas').catch(()=>[])
      ]);
      const installed = await invoke('get_installed_versions').catch(()=>[]);
      VERSIONS = [...new Set([...(remote||[]), ...(installed||[]).filter(v => /^\d/.test(v) && !/forge|fabric/.test(v))])];
      S.totalRam = totalRam || 16; S.javas = javas || [];
      if (settings){
        S.defRam = Math.min(normRam(settings.ram), S.totalRam);
        S.t.lite = !!settings.light_mode;
        S.nick = settings.username || S.nick;
      }
      if (msProfile){ S.ms = true; S.nick = msProfile.name || S.nick; }
      try { S.skins = JSON.parse(localStorage.getItem('obsidia.skins') || '[]').slice(0,4); S.sk = Math.min(+localStorage.getItem('obsidia.sk') || (S.skins.length?0:-1), S.skins.length-1); S.skmodel = localStorage.getItem('obsidia.skmodel')==='slim'?'slim':'classic'; } catch(e){}
      await loadProfiles();
      await loadServers();
      await refreshFiles();
    } catch(e){ console.error(e); }
    render();
  }

  onMount(() => {
    document.addEventListener('click', onClick);
    document.addEventListener('change', onChange);
    document.addEventListener('input', onInput);
    document.addEventListener('keydown', onKey);
    init();
    return () => {
      document.removeEventListener('click', onClick);
      document.removeEventListener('change', onChange);
      document.removeEventListener('input', onInput);
      document.removeEventListener('keydown', onKey);
    };
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
    #app{display:flex;min-width:900px;min-height:100%}
    aside{width:232px;flex:none;padding:28px 18px;display:flex;flex-direction:column;gap:28px;border-right:1px solid var(--bd);background:rgba(255,255,255,.02)}
    .logo{display:flex;align-items:center;gap:12px;padding:0 8px;font:700 22px Sora,sans-serif}
    nav{display:flex;flex-direction:column;gap:6px}
    nav button{display:flex;align-items:center;gap:12px;height:44px;padding:0 14px;border:0;border-radius:12px;background:transparent;color:var(--mu);transition:background .2s,color .2s;text-align:left}
    nav button:hover{background:rgba(255,255,255,.05);color:var(--tx)}
    nav button.on{color:#fff;background:rgba(139,92,246,.18);box-shadow:inset 0 0 0 1px rgba(139,92,246,.5)}
    .acc{margin-top:auto;display:flex;align-items:center;gap:12px;padding:12px;border-radius:16px}
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
    .g3{display:grid;grid-template-columns:repeat(auto-fill,minmax(270px,1fr));gap:16px}
    .pc{display:flex;align-items:center;gap:14px;padding:14px;text-align:left;cursor:pointer;transition:border-color .2s,background .2s}
    .pc:hover{background:rgba(255,255,255,.08)}.pc.on{border-color:rgba(139,92,246,.7)}
    .pc:focus-visible,button:focus-visible{outline:2px solid var(--c);outline-offset:2px}
    .th{width:64px;height:64px;flex:none;border-radius:12px}
    .bar{height:8px;border-radius:4px;background:rgba(255,255,255,.1)}.bar i{display:block;height:8px;border-radius:4px;background:linear-gradient(90deg,#22D3EE,#8B5CF6);transition:width .15s}
    .chips{display:flex;gap:10px;flex-wrap:wrap}
    .chip{height:36px;padding:0 16px;border-radius:18px;color:#C9C7DD;background:var(--gl);border:1px solid var(--bd)}
    .chip.on{color:#fff;background:rgba(139,92,246,.18);border-color:rgba(139,92,246,.55)}
    .srch{height:48px;padding:0 18px;border-radius:14px;background:rgba(255,255,255,.06);border:1px solid rgba(255,255,255,.12)}
    select.srch{background:#1A1830;padding:0 14px}
    select.srch option{background:#1A1830;color:var(--tx)}
    .mc{display:flex;flex-direction:column;gap:12px;padding:18px}
    .mi{width:44px;height:44px;border-radius:12px;flex:none}
    .ib{height:38px;margin-top:auto;border:0;border-radius:10px;font-weight:600;color:#fff;background:#7C3AED;transition:background .2s}
    .ib.in{color:#C9C7DD;background:rgba(255,255,255,.08)}
    .ib:disabled{cursor:default;opacity:.8}
    .row{display:flex;align-items:center;justify-content:space-between;gap:20px;padding:20px 22px}
    .sw{width:50px;height:28px;border-radius:14px;border:0;background:rgba(255,255,255,.14);position:relative;transition:background .2s;flex:none}
    .sw::after{content:"";position:absolute;top:3px;left:3px;width:22px;height:22px;border-radius:50%;background:#fff;transition:left .2s}
    .sw[aria-checked=true]{background:#7C3AED}.sw[aria-checked=true]::after{left:25px}
    input[type=range]{width:260px;accent-color:#8B5CF6}
    select{height:40px;padding:0 12px;border-radius:10px;background:#1A1830;border:1px solid var(--bd);color:var(--tx)}select option{background:#1A1830;color:var(--tx)}
    input:disabled{opacity:.6}
    .lite *{transition:none!important}
    .seg{display:inline-flex;gap:4px;padding:4px;border-radius:14px;background:var(--gl);border:1px solid var(--bd);align-self:flex-start}
    .seg button{height:38px;padding:0 24px;border:0;border-radius:10px;background:transparent;color:var(--mu);font-weight:600;transition:background .2s,color .2s}
    .seg button.on{background:#7C3AED;color:#fff}
    .pf{font-size:12px;font-weight:600;padding:3px 10px;border-radius:10px;background:rgba(255,255,255,.08)}
    .mb{width:34px;height:34px;border-radius:9px;border:1px solid var(--bd);background:rgba(255,255,255,.06);font-size:16px;display:inline-flex;align-items:center;justify-content:center}
    .mb:disabled{opacity:.35;cursor:default}
    .md-back{position:fixed;inset:0;background:rgba(5,5,12,.7);backdrop-filter:blur(6px);display:flex;align-items:center;justify-content:center;z-index:50;padding:16px}
    .md{max-height:90vh;overflow:auto}
    .md.gl{background:#14122A;border-color:rgba(255,255,255,.14);box-shadow:0 24px 80px rgba(0,0,0,.6)}
    .toast{position:fixed;right:24px;bottom:24px;max-width:420px;padding:12px 16px;border-radius:14px;background:#14122A;border:1px solid rgba(52,211,153,.5);color:#EDEBF7;font-size:14px;opacity:0;transform:translateY(8px);pointer-events:none;transition:opacity .2s,transform .2s;z-index:60}
    .toast.show{opacity:1;transform:none}.toast.bad{border-color:rgba(248,113,113,.6)}
  </style>
</svelte:head>

<div id="wrap"><div id="app">
  <Sidebar {navHtml} {accHtml} accountActive={S.tab=='account'} />
  <main id="view"></main>
</div></div>
<Toast />

