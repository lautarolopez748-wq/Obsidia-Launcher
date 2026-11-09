<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
import Sidebar from "$lib/components/Sidebar.svelte";
import Toast from "$lib/components/Toast.svelte";

import { ic, TABS, G, TRASH, LOADERS, RAMS, STG, LBL, esc, norm, validName, normRam, skinFace, skinFull, paintSkinCanvases, accCard, trb, resCard, irisBanner, safeImageUrl } from "$lib/utils.js";

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
    pstate:[], filemap:{ mods:{}, shaders:{}, packs:{} },
    files:{ mods:[], shaders:[], packs:[] },
    content:{ mods:[], shaders:[], packs:[] },
    contentIcons:{ mods:{}, shaders:{}, packs:{} },
    migratingContent:false, migratedInstance:'', migratingProfile:'', inv:{}, invLoading:{}, searchCache:{}, searchPending:{},
    results:{ mods:null, shaders:null, packs:null }, searching:false, searchErr:{ mods:'', shaders:'', packs:'' }, searchSeq:0, busy:{},
    sv:[], af:false, rf:false, es:null, ns:'', na:'',
    np:false, npEdit:-1, npName:'', npVersion:'', npLoader:'Vanilla', npLoaderVersion:'', npRam:4, npJava:'', npErr:'', npBusy:false, npFullscreen:false, npBlock:'',
    javas:[],
    plan:null, planSlug:'', planKind:'', planId:null, planInstance:'', pendingRender:false,
    progress:{}, prePlans:[], updateCheckTs:{}, prePlanPending:[]
  };

  // ───────────── Utilidades ─────────────
  const Q = s => document.querySelector(s);
  // Instancia del perfil activo: cada perfil nuevo tiene su carpeta con mods, packs y mundos.
  // Los perfiles viejos (sin id) usan el token explícito "legacy" (.minecraft).
  const inst = () => (P[S.p] && P[S.p].id) || 'legacy';
  const newId = () => 'p' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
  // Clave de progreso que separa el mismo slug entre distintas instancias.
  /** @param {string} slug @param {string} instance @param {any[]} [deps] */
  function progressClear(slug, instance, deps){
    delete S.progress[`${slug}|${instance}`];
    if (deps) for (const d of deps) if (d && d.slug) delete S.progress[`${d.slug}|${instance}`];
    if (S.tab === 'mods') refreshGrid();
  }
  // Categorías tal como las nombra Modrinth (facet `categories`).
  const CAT = { 'Optimización':'optimization', 'Gráficos':'graphics', 'Utilidades':'utility' };
  // La clave vieja de localStorage se conserva para no perder los mapas ya guardados.
  // Los perfiles sin id usan la clave histórica 'global'.
  /** @param {any} x */
  const mapInstance = x => (x && x !== 'legacy') ? x : 'global';
  /** @param {string} kind @param {string} [instance] */
  const mapKey = (kind, instance) => 'obsidia.map.' + mapInstance(instance || (P[S.p] && P[S.p].id)) + '.' + kind;
  /** @param {string} kind @param {string} [instance] */
  function fileMap(kind, instance){
    try { const m = JSON.parse(localStorage.getItem(mapKey(kind, instance)) || '{}'); return m && typeof m === 'object' ? m : {}; }
    catch(e){ return {}; }
  }
  /** @param {string} kind @param {any} map @param {string} [instance] */
  function saveFileMap(kind, map, instance){ try { localStorage.setItem(mapKey(kind, instance), JSON.stringify(map)); } catch(e){} }
  const fileName = v => { let x = String(v ?? "").trim(); x = x.replace(/^(?:Mod|Shader(?:pack)?|Resource ?pack|Shaderpack|Resourcepack)\s+instalado:\s*/i, ""); const parts = x.split(/[\\\/]/); return (parts.pop() || x).trim(); };
  /** @param {string} kind @param {string} slug @param {string} file @param {string} [icon] @param {string} [instance] */
  function rememberFile(kind, slug, file, icon, instance){
    if (!slug || !file) return;
    const map = fileMap(kind, instance);
    const f = fileName(file);
    const ic = icon || (map[slug] && map[slug].icon) || (map[f] && map[f].icon) || '';
    map[slug] = { file: f, icon: ic };
    if (kind !== 'mods') map[f] = { file: f, icon: ic };
    saveFileMap(kind, map, instance);
  }
  /** @param {string} kind @param {string} file @param {string} [instance] */
  function forgetFile(kind, file, instance){
    const map = fileMap(kind, instance); let changed = false;
    const target = fileName(file);
    for (const k of Object.keys(map)) if (map[k] && fileName(map[k].file || map[k]) === target){ delete map[k]; changed = true; }
    if (changed) saveFileMap(kind, map, instance);
  }
  function loadFileMap(){
    S.filemap = { mods:{}, shaders:{}, packs:{} };
    for (const kind of ['mods','shaders','packs']) S.filemap[kind] = fileMap(kind);
  }
  // Un archivo instalado puede llamarse distinto al slug: primero el mapa guardado en la
  // instalación y después una coincidencia por palabras del slug o del título.
  const installedFile = (kind, slug) => {
    const map = (S.filemap && S.filemap[kind]) ? S.filemap[kind] : {};
    const entry = map && map[slug];
    const file = entry && (entry.file || entry);
    if (file && (S.files[kind] || []).includes(file)) return file;
    return null;
  };

  function findFile(kind, key, title){
    const files = S.files[kind] || [];
    const map = fileMap(kind);
    if (map[key] && files.includes((map[key].file || map[key]))) return (map[key].file || map[key]);
    function installedMatches(file, k){
      const ff = norm(file), kk = norm(k);
      if (!ff || !kk || !ff.startsWith(kk)) return false;
      const rest = ff.slice(kk.length);
      if (!rest) return true;
      return /^(v?\d|mc\d|r\d|fabric|forge|neoforge|quilt|jar|zip)/.test(rest);
    }
    let hit = files.find(f => installedMatches(f, key));
    if (hit) return hit;
    if (title){
      hit = files.find(f => norm(f).startsWith(norm(title)));
      if (hit) return hit;
    }
    return null;
  }
  function matchesInstalled(file, key, title){
    const ff = norm(file); if (!ff) return false;
    for (const raw of [key, title]){
      if (!raw) continue; const kk = norm(raw); if (!kk || !ff.startsWith(kk)) continue;
      const rest = ff.slice(kk.length); if (!rest) return true;
      if (/^(?:v?\d|mc\d|r\d)/.test(rest)) return true;
      if (/^(?:fabric|forge|neoforge|quilt|jar|zip)/.test(rest)) return true;
    }
    return false;
  }
  function iconFor(kind, file){
    const target = norm(fileName(file));
    const cmap = (S.contentIcons && S.contentIcons[kind]) ? S.contentIcons[kind] : {};
    if (cmap[file]) return cmap[file];
    const map = fileMap(kind);
    for (const slug of Object.keys(map)){
      const v = map[slug];
      if (!v || !v.icon) continue;
      if (norm(fileName(v.file || v)) === target) return v.icon;
    }
    if (kind !== 'mods'){
      for (const slug of Object.keys(map)){
        const v = map[slug];
        if (!v || !v.icon) continue;
        if (matchesInstalled(file, slug, '')) return v.icon;
      }
      const hit = (S.results && S.results[kind] ? S.results[kind] : []).find(r => matchesInstalled(file, r.slug, r.title));
      if (hit && hit.icon_url) return hit.icon_url;
    }
    return '';
  }
  const refreshGrid = () => { const g = Q('#grid'); if (g) g.innerHTML = grid(); };

  function toast(msg, bad){
    const t = Q('#toast'); if (!t) return;
    t.textContent = msg; t.className = 'toast show' + (bad ? ' bad' : '');
    clearTimeout(toast.t); toast.t = setTimeout(() => { t.className = 'toast'; }, 5000);
  }

  // Solo enlaces https de dominios de Microsoft para el login por código.
  /** @param {any} raw */
  function isMicrosoftUrl(raw){
    try {
      const u = new URL(String(raw || ''));
      if (u.protocol !== 'https:') return false;
      const h = u.hostname.toLowerCase();
      return ['microsoft.com','live.com','microsoftonline.com','msftauth.net','msauth.net','office.com']
        .some(d => h === d || h.endsWith('.' + d));
    } catch(e){ return false; }
  }

  const sw = (k,l,d) => `<div class="row gl"><div><div style="font-weight:600">${l}</div><div class="mu">${d}</div></div><button class="sw" role="switch" aria-label="${l}" aria-checked="${S.t[k]}" data-act="tg" data-k="${k}"></button></div>`;
  const seg = () => `<div class="seg">${[['mods','Mods'],['shaders','Shaders'],['packs','Resource packs']].map(s=>`<button class="${S.sec==s[0]?'on':''}" data-act="seg" data-s="${s[0]}">${s[1]}</button>`).join('')}</div>`;

  // ───────────── Skins ─────────────
  // Cara por defecto (Steve).
  // accCard moved to utils.js

  // ───────────── Perfiles ─────────────
  function cards(){
    return P.map((p,i)=>{
      const block = esc(encodeURIComponent(p.block || 'Crafting_Table'));
      return `<div class="pcard ${i==S.p?'on':''}" role="button" tabindex="0" data-act="sel" data-i="${i}"><div class="slot"><img src="/images/blocks/${block}.png" alt=""></div><div class="pinfo"><b>${esc(p.n)}</b><span class="${i==S.p?'on':''}">${i==S.p?'Activo · ':''}${esc(p.s)}</span></div>${S.migratingProfile===p.n?'<span class="mu" style="color:#FBBF24">Migrando…</span>':`<button class="edit" data-act="ped" data-i="${i}">Editar</button>${p.id?'':`<button class="mig" data-act="migrate" data-i="${i}" title="Migrar a instancia independiente">Migrar</button>`}`}</div>`;
    }).join('');
  }
  const newCard = `<button class="newcard" data-act="new"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>Nueva instancia</button>`;

  function npModal(){
    const title = S.npEdit>=0 ? 'Editar instancia' : 'Nueva instancia';
    const rams = RAMS.filter(r => r <= (S.totalRam||16));
    if (!rams.includes(S.npRam)) rams.push(S.npRam), rams.sort((a,b)=>a-b);
    const versions = VERSIONS.includes(S.npVersion) ? VERSIONS : [S.npVersion, ...VERSIONS];
    const editing = S.npEdit>=0;
    const lockIco = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="4" y="10" width="16" height="10" rx="2"/><path d="M8 10V7a4 4 0 018 0v3"/></svg>';
    const chevIco = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>';
    const infoIco = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/></svg>';
    const blockOpts = `<option value="">Predeterminado</option><option value="Acacia_Log" ${S.npBlock=="Acacia_Log"?'selected':''}>Acacia Log</option><option value="Amethyst_Block" ${S.npBlock=="Amethyst_Block"?'selected':''}>Amethyst Block</option><option value="Ancient_Debris" ${S.npBlock=="Ancient_Debris"?'selected':''}>Ancient Debris</option><option value="Beacon" ${S.npBlock=="Beacon"?'selected':''}>Beacon</option><option value="Bedrock" ${S.npBlock=="Bedrock"?'selected':''}>Bedrock</option><option value="Black_Glazed_Terracotta" ${S.npBlock=="Black_Glazed_Terracotta"?'selected':''}>Black Glazed Terracotta</option><option value="Blue_Glazed_Terracotta" ${S.npBlock=="Blue_Glazed_Terracotta"?'selected':''}>Blue Glazed Terracotta</option><option value="Carved_Pumpkin" ${S.npBlock=="Carved_Pumpkin"?'selected':''}>Carved Pumpkin</option><option value="Cherry_Leaves" ${S.npBlock=="Cherry_Leaves"?'selected':''}>Cherry Leaves</option><option value="Cherry_Log" ${S.npBlock=="Cherry_Log"?'selected':''}>Cherry Log</option><option value="Chiseled_Polished_Blackstone" ${S.npBlock=="Chiseled_Polished_Blackstone"?'selected':''}>Chiseled Polished Blackstone</option><option value="Command_Block" ${S.npBlock=="Command_Block"?'selected':''}>Command Block</option><option value="Cracked_Deepslate_Bricks" ${S.npBlock=="Cracked_Deepslate_Bricks"?'selected':''}>Cracked Deepslate Bricks</option><option value="Crafting_Table" ${S.npBlock=="Crafting_Table"?'selected':''}>Crafting Table</option><option value="Deepslate_Diamond_Ore" ${S.npBlock=="Deepslate_Diamond_Ore"?'selected':''}>Deepslate Diamond Ore</option><option value="Deepslate_Emerald_Ore" ${S.npBlock=="Deepslate_Emerald_Ore"?'selected':''}>Deepslate Emerald Ore</option><option value="Deepslate_Gold_Ore" ${S.npBlock=="Deepslate_Gold_Ore"?'selected':''}>Deepslate Gold Ore</option><option value="Deepslate_Iron_Ore" ${S.npBlock=="Deepslate_Iron_Ore"?'selected':''}>Deepslate Iron Ore</option><option value="Diamond_Block" ${S.npBlock=="Diamond_Block"?'selected':''}>Diamond Block</option><option value="Diamond_Ore" ${S.npBlock=="Diamond_Ore"?'selected':''}>Diamond Ore</option><option value="Iron_Block" ${S.npBlock=="Iron_Block"?'selected':''}>Iron Block</option><option value="Iron_Ore" ${S.npBlock=="Iron_Ore"?'selected':''}>Iron Ore</option><option value="Oak_Log" ${S.npBlock=="Oak_Log"?'selected':''}>Oak Log</option><option value="Pale_Oak_Log" ${S.npBlock=="Pale_Oak_Log"?'selected':''}>Pale Oak Log</option><option value="White_Glazed_Terracotta" ${S.npBlock=="White_Glazed_Terracotta"?'selected':''}>White Glazed Terracotta</option>`;
    return `<div class="md-back" data-act="npclose"><div class="np-modal">
      <div class="np-head">
        <div>
          <h2>${title}</h2>
          <div class="np-sub">${editing?`${esc(S.npName)} · ${esc(S.npLoader)} ${esc(S.npVersion)}`:'Elegí versión, loader y recursos'}</div>
        </div>
        <button class="np-x" data-act="npclose" aria-label="Cerrar">✕</button>
      </div>
      <div class="np-body">
        <div class="np-field"><label for="np-name">Nombre de la instancia</label><input id="np-name" class="np-in" maxlength="24" autocomplete="off" value="${esc(S.npName)}"></div>
        <div class="np-grid2">
          <div class="np-field"><label for="np-version">Versión de Minecraft</label><div class="np-selwrap"><select id="np-version" class="np-in" ${editing?'disabled':''}>${versions.map(v=>`<option ${v==S.npVersion?'selected':''}>${esc(v)}</option>`).join('')}</select><span class="np-ico">${editing?lockIco:chevIco}</span></div></div>
          <div class="np-field"><label for="np-loader">Loader</label><div class="np-selwrap"><select id="np-loader" class="np-in" ${editing?'disabled':''}>${LOADERS.map(l=>`<option ${l==S.npLoader?'selected':''}>${l}</option>`).join('')}</select><span class="np-ico">${editing?lockIco:chevIco}</span></div></div>
        </div>
        <div class="np-note">${infoIco}<span>${editing?'La versión y el loader quedan fijos al crear la instancia. Para cambiarlos, creá una nueva.':'Fabric y Forge se instalan solos la primera vez que jugás.'}</span></div>
        <div class="np-grid2">
          <div class="np-field"><label for="np-ram">RAM asignada</label><div class="np-selwrap"><select id="np-ram" class="np-in">${rams.map(r=>`<option value="${r}" ${r==S.npRam?'selected':''}>${r} GB</option>`).join('')}</select><span class="np-ico">${chevIco}</span></div></div>
          <div class="np-field"><label for="np-java">Java</label><div class="np-selwrap"><select id="np-java" class="np-in"><option value="">Automático</option>${S.javas.map(j=>`<option value="${esc(j.path)}" ${j.path==S.npJava?'selected':''}>${esc(j.version||j.path)}</option>`).join('')}</select><span class="np-ico">${chevIco}</span></div></div>
        </div>
        <label class="np-toggle">
          <span><span class="np-t">Pantalla completa</span><span class="np-d">Abre el juego sin bordes al iniciar.</span></span>
          <input type="checkbox" id="np-fullscreen" ${S.npFullscreen?'checked':''}>
          <span class="np-switch"></span>
        </label>
        <div class="np-field"><label for="np-block">Bloque representativo</label><div class="np-selwrap"><select id="np-block" class="np-in">${blockOpts}</select><span class="np-ico">${chevIco}</span></div></div>
      </div>
      <div class="np-foot">
        ${S.npErr?`<div class="np-err">${esc(S.npErr)}</div>`:''}
        <div class="np-foot-row">
          ${editing?`<button class="np-danger" data-act="npdel" title="Eliminar instancia" aria-label="Eliminar instancia">${TRASH}</button>`:'<span></span>'}
          <div class="np-actions"><button class="sec" data-act="npclose">Cancelar</button><button class="ib" data-act="npcreate" ${S.npBusy?'disabled':''}>${editing?'Guardar':'Crear instancia'}</button></div>
        </div>
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
    const btn = g===null ? 'Jugar' : g<100 ? 'Preparando… '+g+' %' : 'En juego';
    const java = p ? (p.java ? ((S.javas.find(j => j.path===p.java)||{}).version || 'Java personalizado') : 'Java automático') : '';
    const meta = p
      ? `<b>${esc(p.loader||'Vanilla')}</b><span class="sep">·</span>${esc(p.version)}<span class="sep">·</span>${p.ram} GB de RAM<span class="sep">·</span>${esc(java)}`
      : 'Creá una instancia para empezar a jugar.';
    const latest = VERSIONS[0] || (p && p.version) || '—';
    const modsN = (S.files.mods||[]).length;
    const instN = P.length;
    return `<div class="home">
    <header class="topbar">
      <div><div class="hello">Biblioteca</div><h1>Inicio</h1></div>
      <div class="top-right"><span class="pill"><i></i>Todo listo</span></div>
    </header>

    <section class="hero">
      <div class="hero-in">
        <span class="eyebrow">${p?'Instancia activa':'Sin instancia activa'}</span>
        <h2>${p?esc(p.n):'Sin instancia'}</h2>
        <div class="meta">${meta}</div>
        <div class="actions">
          <button class="btn-primary" data-act="play" ${g!==null?'disabled':''}>${g===null?'<span class="play-ico"><svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7Z"/></svg></span>':''}${p?btn:'Crear instancia'}</button>
          ${p?`<button class="btn-ghost" data-act="ped" data-i="${S.p}">Configurar</button>`:''}
        </div>
      </div>
      <div class="hint"><i></i>${g===null?'Listo para jugar':esc(LBL[S.stage]||'Preparando…')}</div>
    </section>

    ${g!==null?`<div class="gl launch"><div style="display:flex;justify-content:space-between;font-size:13px;color:#C9C7DD"><span>${LBL[S.stage]||''}</span><span>${g} %</span></div><div class="bar" style="margin-top:10px"><i style="width:${g}%"></i></div></div>`:''}
    ${S.launchErr?`<div class="gl launch" style="border-color:rgba(248,113,113,.5)"><b style="color:#F87171">No se pudo abrir el juego</b><div class="mu" style="margin-top:6px;white-space:pre-wrap;word-break:break-word">${esc(S.launchErr)}</div></div>`:''}
    ${S.launchMsg?`<div class="gl launch" style="border-color:rgba(52,211,153,.4)"><div class="mu" style="color:#6EE7B7;word-break:break-word">${esc(S.launchMsg)}</div></div>`:''}

    <div class="grid">
      <section>
        <div class="head"><h3>Tus instancias</h3><span>${instN} ${instN===1?'instancia':'instancias'}</span></div>
        <div class="cards">${cards()}${newCard}</div>
      </section>
      <aside class="panel">
        <div class="head"><h3>Novedades</h3><button class="link" data-act="go" data-t="mods">Ver mods</button></div>
        <ul class="rows">
          <li><div class="nthumb cy"><svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 19h16"/></svg></div><div class="t"><b>Minecraft ${esc(latest)} disponible</b><span>Se descarga al iniciar el juego.</span></div></li>
          <li><div class="nthumb vi"><svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 3 20.5 7.5v9L12 21l-8.5-4.5v-9Z"/><path d="m3.5 7.5 8.5 4.5 8.5-4.5"/></svg></div><div class="t"><b>${modsN} ${modsN===1?'mod instalado':'mods instalados'}</b><span>${modsN?'Gestioná las actualizaciones desde Mods.':'Todavía no instalaste mods en esta instancia.'}</span></div></li>
          <li><div class="nthumb li"><svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.6 2.4 3.9 5.3 3.9 8.5s-1.3 6.1-3.9 8.5c-2.6-2.4-3.9-5.3-3.9-8.5s1.3-6.1 3.9-8.5Z"/></svg></div><div class="t"><b>Mundos compartidos</b><span>Todas las instancias comparten la carpeta de mundos.</span></div></li>
        </ul>
      </aside>
    </div>

    <div class="strip">
      <div class="stat"><div class="ico"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 3 20.5 7.5v9L12 21l-8.5-4.5v-9Z"/><path d="m3.5 7.5 8.5 4.5 8.5-4.5"/></svg></div><div><b>${modsN} mods</b><small>instalados en esta instancia</small></div></div>
      <div class="stat"><div class="ico cy"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.6 2.4 3.9 5.3 3.9 8.5s-1.3 6.1-3.9 8.5c-2.6-2.4-3.9-5.3-3.9-8.5s1.3-6.1 3.9-8.5Z"/></svg></div><div><b>Mundos</b><small>carpeta compartida</small></div></div>
      <div class="stat"><div class="ico pe"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/></svg></div><div><b>— h</b><small>total de hs jugadas</small></div></div>
      <div class="right">${instN} ${instN===1?'instancia':'instancias'} en tu biblioteca</div>
    </div>
  </div>`;
  }
  function profiles(){ return `<div class="home"><h1>Instancias</h1><div class="mu" style="margin:6px 0 18px">Elegí con cuál jugar o creá una nueva.</div><div class="cards">${cards()}${newCard}</div></div>`; }
  function settings(){
    return `<div class="home">
      <h1>Configuración</h1>
      <div class="mu" style="margin:6px 0 18px">Ajustes generales de Obsidia.</div>
      <div class="gl mc">
        <div class="row">
          <div>
            <div style="font-weight:600">RAM asignada por defecto</div>
            <div class="mu">Límite para instancias nuevas (máx ${S.totalRam} GB).</div>
          </div>
          <div style="display:flex;align-items:center;gap:12px">
            <b id="ramv">${S.defRam} GB</b>
            <input type="range" id="ram" min="2" max="${S.totalRam}" step="1" value="${S.defRam}">
          </div>
        </div>
        <hr style="border:0;border-top:1px solid rgba(255,255,255,.05);margin:0">
        ${sw('lite', 'Modo ligero', 'Desactiva animaciones y transiciones.')}
      </div>
    </div>`;
  }

  // ───────────── Mods / shaders / resource packs ─────────────
  function installedList(kind){
    const files = S.files[kind];
    if (!files.length) return '';
    const names = { mods:'Mods instalados', shaders:'Shaders instalados', packs:'Resource packs instalados' };
    const grad = kind==='mods' ? 'linear-gradient(135deg,#8B5CF6,#22D3EE)' : kind==='shaders' ? 'linear-gradient(135deg,#FBBF24,#F472B6)' : 'repeating-linear-gradient(90deg,rgba(0,0,0,.14) 0 8px,transparent 8px 16px),linear-gradient(135deg,#34D399,#22D3EE)';
    return `<div class="gl" style="grid-column:1/-1;padding:18px 20px;display:flex;flex-direction:column;gap:8px"><b>${names[kind]}</b>${files.map(f=>{
      const icon = safeImageUrl(iconFor(kind, f));
      const bg = icon ? `background-image:url("${icon}"),${grad};background-size:cover;background-position:center` : `background:${grad}`;
      return `<div style="display:flex;align-items:center;gap:12px;padding:6px 12px;border-radius:12px;background:rgba(255,255,255,.04)"><div class="mi" style='${bg}'></div><span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(f)}</span>${trb(kind,f,34)}</div>`;
    }).join('')}</div>`;
  }
  function grid(){
    const kind = S.sec, q = S.q.trim(), res = S.results[kind];
    let h = '';
    if (kind==='shaders' && !findFile('mods','iris')) h += irisBanner(S.busy['iris']);
    if (!res){
      const err = S.searchErr[kind];
      h += err
        ? `<div class="gl" style="padding:16px 18px;display:flex;align-items:center;gap:12px;flex-wrap:wrap"><span class="mu">No se pudo buscar: ${esc(err)}</span><button class="sec" style="height:38px;padding:0 16px" data-act="retrysearch">Reintentar</button></div>`
        : `<div class="mu">${S.searching?'Buscando en Modrinth…':'Cargando…'}</div>`;
    } else {
      h += res.length ? res.map(r => resCard(kind, r, S.busy[r.slug], installedFile(kind, r.slug) || findFile(kind, r.slug, r.title), S.progress[`${r.slug}|${inst()}`])).join('') : '<div class="mu">No hay resultados en Modrinth.</div>';
    }
    return h;
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
    return `<div style="display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap"><h1>Mods</h1><div class="gl" style="padding:9px 16px;color:#C9C7DD;font-size:14px;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:100%">Instancia: ${p?esc(p.n)+' · '+esc(p.loader||'Vanilla'):'—'}</div></div>${seg()}${S.sec==='packs'?packsPanel():''}${installedList(S.sec)}<input id="q" class="srch" aria-label="${ph}" placeholder="${ph}" value="${esc(S.q)}">${S.sec==='mods'&&!S.q.trim()?`<div class="chips">${['Todos','Optimización','Gráficos','Utilidades'].map(c=>`<button class="chip ${c==S.f?'on':''}" data-act="f" data-c="${c}">${c}</button>`).join('')}</div>`:''}<div class="g3" id="grid">${grid()}</div>`;
  }

  async function migrateContentMaps(){
     const instance = inst();
     if (S.migratingContent || S.migratedInstance === instance) return;
     S.migratingContent = true;
     let ok = true;
     try {
       for (const kind of ['mods','shaders','packs']){
         const map = fileMap(kind);
         const seen = new Set();
         const entries = [];
         for (const [slug, v] of Object.entries(map)){
           const file = (v && typeof v === 'object') ? (v.file || '') : (typeof v === 'string' ? v : '');
           const f = fileName(file);
           if (!f) continue;
           if (seen.has(f)) continue;
           seen.add(f);
           entries.push({ slug, version_id:'unknown', filename:f, sha1:null, loader:null, mc_version:null, icon_url: (v && v.icon) ? v.icon : null });
         }
         if (entries.length){
           try { await invoke('migrate_content_map', { instance, kind, entries }); }
           catch(e){ console.error('migrate_content_map', kind, e); ok = false; }
         }
       }
     } finally {
       if (ok) S.migratedInstance = instance;
       S.migratingContent = false;
     }
   }
async function loadInventory(instance){
    if (S.inv[instance]) return S.inv[instance];
    if (S.invLoading[instance]) return await S.invLoading[instance];
    const promise = (async () => {
      const [mc, sc, pc, ps] = await Promise.all([
        invoke('list_content', { instance, kind:'mods' }).catch(()=>null),
        invoke('list_content', { instance, kind:'shaders' }).catch(()=>null),
        invoke('list_content', { instance, kind:'packs' }).catch(()=>null),
        invoke('get_resourcepack_state', { instance }).catch(()=>[])
      ]);
      let m = (mc && Array.isArray(mc) && mc.length) ? mc.map(x=>x.file) : null;
      let sh = (sc && Array.isArray(sc) && sc.length) ? sc.map(x=>x.file) : null;
      let r = (pc && Array.isArray(pc) && pc.length) ? pc.map(x=>x.file) : null;
      if (m === null) m = await invoke('list_installed_mods', { instance }).catch(()=>[]);
      if (sh === null) sh = await invoke('list_shaderpacks', { instance }).catch(()=>[]);
      if (r === null) r = await invoke('list_resourcepacks', { instance }).catch(()=>[]);
      const inv = { files:{ mods:m||[], shaders:sh||[], packs:r||[] }, content:{ mods:mc||[], shaders:sc||[], packs:pc||[] }, icons:{ mods:{}, shaders:{}, packs:{} }, pstate: ps||[] };
      for (const kind of ['mods','shaders','packs']) { const arr = inv.content[kind] || []; const map = {}; for (const e of arr) if (e && e.file) map[e.file] = e.icon || ''; inv.icons[kind] = map; }
      S.inv[instance] = inv;
      delete S.invLoading[instance];
      return inv;
    })();
    S.invLoading[instance] = promise;
    return promise;
  }
  async function syncInventory(instance){
    const inv = await loadInventory(instance);
    S.files = inv.files; S.content = inv.content; S.contentIcons = inv.icons; S.pstate = inv.pstate;
    loadFileMap();
  }
  /** @param {string} kind @param {string} [instance] */
  async function refreshKind(kind, instance){
    const target = instance || inst();
    const r = await invoke('list_content', { instance: target, kind }).catch(()=>null);
    if (Array.isArray(r)) {
      S.files[kind] = r.map(x=>x.file);
      S.content[kind] = r;
      S.contentIcons[kind] = {};
      const m = {}; for (const e of r) if (e && e.file) m[e.file] = e.icon || '';
      S.contentIcons[kind] = m;
    }
    if (kind === 'packs') {
      S.pstate = await invoke('get_resourcepack_state', { instance: target }).catch(()=>[]);
    }
    if (target === inst()) {
      loadFileMap();
      if (S.tab === 'mods') { refreshGrid(); renderOrDefer(); }
    }
  }

  /** @param {string} kind @param {string} [instance] */
  async function checkUpdatesIfDue(kind, instance){
    const target = instance || inst();
    const key = `${target}|${kind}`;
    const now = Date.now();
    const last = S.updateCheckTs[key] || 0;
    if (now - last < 600000) return;
    try {
      const ups = await invoke('check_content_updates', { instance: target, kind });
      S.updateCheckTs[key] = now;
      if (Array.isArray(ups)){
        const m = new Map(ups.map(u => [u.file, !!u.hay_actualizacion]));
        const arr = S.content[kind] || [];
        for (const e of arr) if (m.has(e.file)) e.hay_actualizacion = m.get(e.file);
      }
    } catch(e){}
    if (target === inst() && S.tab === 'mods') { refreshGrid(); renderOrDefer(); }
  }
  async function refreshFiles(){
    const instance = inst();
    await syncInventory(instance);
    (async () => {
      for (const k of ['mods','shaders','packs']) {
        try {
          const ups = await invoke('check_content_updates', { instance, kind:k });
          if (Array.isArray(ups)) {
            const m = new Map(ups.map(u => [u.file, !!u.hay_actualizacion]));
            const arr = S.content[k] || [];
            for (const e of arr) if (m.has(e.file)) e.hay_actualizacion = m.get(e.file);
          }
        } catch(e){}
      }
      if (S.tab === 'mods') { refreshGrid(); renderOrDefer(); }
    })();
  }
  // Si hay un modal abierto, el render se difiere hasta que se cierre (que ya re-renderiza).
  function renderOrDefer(){
    if (S.np || S.msc){ S.pendingRender = true; return; }
    render();
  }
async function savePacks(){
    const active = S.pstate.filter(x=>x.enabled).map(x=>x.name);
    try { await invoke('save_resourcepack_state', { active, instance:inst() }); }
    catch(e){ toast('No se pudo guardar el orden de packs: '+e, true); }
    await refreshKind('packs');
  }

  async function doSearch(){
    const kind = S.sec, q = S.q.trim(), my = ++S.searchSeq;
    const cat = (kind==='mods' && !q && S.f !== 'Todos') ? (CAT[S.f] || null) : null;
    const text = q;
    S.searchErr[kind] = '';
    const p = P[S.p];
    const ver = (p && p.version) || null;
    const lod = (kind==='mods' && p && p.loader && p.loader !== 'Vanilla') ? p.loader : null;
    const key = [kind, text, cat||'', ver||'', lod||'', inst()].join('|');
    const cached = S.searchCache[key];
    if (cached) {
      S.results[kind] = cached.res;
      S.searching = false;
      refreshGrid();
    } else {
      S.searching = true;
      S.results[kind] = null;
      refreshGrid();
    }
    // Evitar solicitudes duplicadas simultáneas.
    if (S.searchPending[key]) {
      try { await S.searchPending[key]; } catch(e){}
      return;
    }
    // Caché fresca: no consultar de nuevo.
    if (cached && (Date.now() - cached.ts) < 300000) return;
    const cmd = { mods:'search_mods', shaders:'search_shaders', packs:'search_resourcepacks' }[kind];
    let promise = (async () => {
      let res;
      try {
        res = await invoke(cmd, { query:text, category: cat, version: ver, loader: lod });
      } catch(e){
        if (my !== S.searchSeq) return;
        // No cachear el error: así "Reintentar" vuelve a consultar de verdad.
        S.results[kind] = null;
        S.searching = false;
        S.searchErr[kind] = String(e);
        refreshGrid();
        return;
      }
      if (my !== S.searchSeq) return;
      const arr = Array.isArray(res) ? res : [];
      S.searchCache[key] = { res: arr, ts: Date.now() };
      S.results[kind] = arr;
      S.searching = false;
      refreshGrid();
    })();
    S.searchPending[key] = promise;
    try { await promise; } finally { delete S.searchPending[key]; }
  }

  // Identificador de operación para correlacionar eventos de performance.
  let perfOpId = null;
  function perfLog(stage, extra='') {
    if (!import.meta.env.DEV) return;
    const t = Date.now();
    const msg = `[PERF][${perfOpId||'-'}] ${stage} t=${t} ${extra}`;
    console.log(msg);
    try { invoke('log_performance_event', { event: msg }).catch(()=>{}); } catch(e){}
  }

  async function doInstall(kind, slug){
    const instance = inst();
    const p = P[S.p];
    // Mods: plan previo sin descargas + confirmación explícita.
    if (kind==='mods'){
      if (S.busy[slug] || S.plan) return;
      if (!p){ toast('Creá una instancia primero.', true); return; }
      if ((p.loader||'Vanilla')==='Vanilla'){ toast('Esta instancia es Vanilla: creá una instancia Fabric o Forge para usar mods.', true); return; }
      perfOpId = slug+'-'+Date.now();
      perfLog('clic_instalar');
      S.busy[slug] = 1; refreshGrid();
      let result = null;
      const preKey = `${slug}|${instance}`;
      const pre = S.prePlans.find(x => x && x.key === preKey);
      if (pre && Date.now() - pre.ts < 240000) {
        result = { plan: pre.plan, plan_id: pre.planId };
      }
      if (!result) {
        try {
          perfLog('inicio_resolucion');
          const result2 = await invoke('plan_mod_dependencies', { slug, mcVersion:p.version, loader:p.loader.toLowerCase(), instance });
          perfLog('fin_resolucion');
          result = result2;
        } catch(e) {
          S.planId = null;
          toast('No se pudo planear la instalación: '+e, true);
          delete S.busy[slug];
          progressClear(slug, instance);
          render();
          return;
        }
      }
      progressClear(slug, instance);
      if (!S.busy[slug] || !result) { delete S.busy[slug]; render(); return; }
      const plan = result.plan;
      const planId = result.plan_id;
      const pending = (plan.dependencies||[]).filter(d => !d.already_installed);
      if (pending.length === 0) {
        try {
          perfLog('inicio_invoke_install_mod');
          const res = await invoke('install_mod', { slug, mcVersion:p.version, loader:p.loader.toLowerCase(), instance, confirmedPlan:plan, planId, iconUrl: pre && pre.iconUrl });
          perfLog('respuesta_backend');
          const result2 = (S.results && S.results[kind]) ? S.results[kind].find(r => r.slug === slug) : null;
          if (res) rememberFile(kind, slug, res, result2 && result2.icon_url, instance);
          // Asegurar que S.filemap[kind][slug] quede actualizado inmediatamente en memoria
          if (!S.filemap[kind]) S.filemap[kind] = {};
          S.filemap[kind][slug] = { file: res, icon: result2 && result2.icon_url };
          toast('Instalado correctamente.');
          perfLog('boton_instalado');
        } catch(e){ toast('No se pudo instalar: '+e, true); }
        delete S.busy[slug];
        await refreshKind(kind, instance);
        progressClear(slug, instance, plan.dependencies);
        perfLog('fin_refreshkind');
        return;
      }
      S.plan = plan;
      S.planId = planId;
      S.planSlug = slug;
      S.planKind = kind;
      S.planInstance = instance;
      delete S.busy[slug];
      refreshGrid(); render();
      return;
    }
    if (S.busy[slug]) return;
    if (!p){ toast('Creá una instancia primero.', true); return; }
    perfOpId = slug+'-'+Date.now();
    perfLog('clic_instalar');
    S.busy[slug] = 1; refreshGrid();
    try {
      let res = null;
      if (kind==='shaders') res = await invoke('install_shaderpack', { slug, mcVersion:p.version, instance });
      else res = await invoke('install_resourcepack', { slug, mcVersion:p.version, instance });
      perfLog('respuesta_backend');
      const result = (S.results && S.results[kind]) ? S.results[kind].find(r => r.slug === slug) : null;
      if (res) rememberFile(kind, slug, res, result && result.icon_url, instance);
      toast('Instalado correctamente.');
      perfLog('boton_instalado');
    } catch(e){ toast('No se pudo instalar: '+e, true); }
    delete S.busy[slug];
    await refreshKind(kind, instance);
    progressClear(slug, instance);
    perfLog('fin_refreshkind');
  }

  // Instala el plan confirmado tal cual (el backend lo recalcula y verifica).
  async function doInstallConfirmed(){
    if (!S.plan) return;
    const instance = S.planInstance || inst();
    const p = P[S.p];
    if (!p){ toast('Creá una instancia primero.', true); return; }
    const slug = S.planSlug, kind = S.planKind;
    if (S.busy[slug]) return;
    perfOpId = slug+'-'+Date.now();
    perfLog('clic_instalar');
    S.busy[slug] = 1;
    const plan = S.plan;
    const planId = S.planId;
    S.plan = null; S.planSlug=''; S.planKind=''; S.planId=null; S.planInstance='';
    render();
    try {
      perfLog('inicio_invoke_install_mod');
      const res = await invoke('install_mod', { slug, mcVersion:p.version, loader:p.loader.toLowerCase(), instance, confirmedPlan:plan, planId, iconUrl: S.prePlans.find(x => x && x.key === `${slug}|${instance}`)?.iconUrl });
      perfLog('respuesta_backend');
      const result = (S.results && S.results[kind]) ? S.results[kind].find(r => r.slug === slug) : null;
      if (res) rememberFile(kind, slug, res, result && result.icon_url, instance);
      // Asegurar que S.filemap[kind][slug] quede actualizado inmediatamente en memoria
      if (!S.filemap[kind]) S.filemap[kind] = {};
      S.filemap[kind][slug] = { file: res, icon: result && result.icon_url };
      toast('Instalado correctamente.');
      perfLog('boton_instalado');
    } catch(e){ toast('No se pudo instalar: '+e, true); }
    delete S.busy[slug];
    await refreshKind(kind, instance);
    progressClear(slug, instance, plan.dependencies);
    perfLog('fin_refreshkind');
  }

  function planModal(){
    const pl = S.plan;
    if (!pl) return '';
    const pending = (pl.dependencies||[]).filter(d => !d.already_installed);
    const deps = pending.map(d=>`<div style="display:flex;align-items:center;gap:12px;padding:6px 12px;border-radius:12px;background:rgba(255,255,255,.04)"><span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(d.slug)}</span><span class="mu" style="font-size:12px">${esc(d.filename)}</span><span class="pf" style="color:#6EE7B7">A instalar</span></div>`).join('');
    return `<div class="md-back" data-act="plancancel"><div class="md gl" style="width:min(520px,94vw);padding:26px;display:flex;flex-direction:column;gap:14px">
      <h2 style="margin:0">Confirmar instalación</h2>
      <div class="mu">Se instalarán primero las dependencias obligatorias pendientes y después el mod principal.</div>
      <div style="display:flex;align-items:center;gap:12px;padding:8px 12px;border-radius:12px;background:rgba(255,255,255,.04)"><b style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(pl.slug)}</b><span class="mu" style="font-size:12px">${esc(pl.filename)}</span><span class="pf" style="color:#C4B5FD">Mod principal</span></div>
      ${pending.length?deps:'<div class="mu">Todas las dependencias obligatorias ya están instaladas.</div>'}
      <div style="display:flex;gap:10px;justify-content:flex-end"><button class="sec" style="height:42px" data-act="plancancel">Cancelar</button><button class="ib" style="margin:0;height:42px;padding:0 22px" data-act="planinstall">Instalar</button></div>
    </div></div>`;
  }

  // ───────────── Servidores ─────────────
  function servers(){
    const pc = x => x<60?'#34D399':x<120?'#FBBF24':'#F87171';
    return `<div style="display:flex;justify-content:space-between;align-items:center;gap:16px;flex-wrap:wrap"><h1>Servidores</h1><div style="display:flex;gap:10px;flex-wrap:wrap"><button class="sec" style="height:44px" data-act="sref">${S.rf?'Actualizando…':'Actualizar'}</button><button class="ib" style="margin:0;padding:0 22px;height:44px" data-act="sadd">+ Añadir servidor</button></div></div>
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
    const list = await invoke('get_servers').catch(e => { toast(String(e), true); return []; });
    S.sv = (list||[]).map(s => ({ n:s.name, a:s.address, v:s.version, on:null, pg:0 }));
  }

  // ───────────── Cuenta ─────────────
function account(){
    const ok = validName(S.nick);
    const p = P[S.p] || {};
    const modCount = S.content.mods?.length || 0;
    const ramGb = p.ram || S.defRam || 4;
    const skinUrl = S.sk >= 0 && S.sk < S.skins.length ? S.skins[S.sk] : null;
    const modelLabel = S.skmodel === 'slim' ? 'Fino' : 'Clásico';
    const currentSkinLabel = S.sk >= 0 ? `Skin ${S.sk + 1}` : 'Sin skin';

    const skinBig = skinFull(skinUrl, S.skmodel, 8);
    const previewSkinImg = skinFull(skinUrl, S.skmodel, 5);

    return `<div class="account-page">
      <div class="page-head">
        <div>
          <h1>Mi cuenta</h1>
          <p class="page-sub">Tu identidad en Obsidia y en el juego.</p>
        </div>
        <div class="save-state"><svg viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>Guardado automático</div>
      </div>

      <div class="top">
        <section class="panel profile" aria-labelledby="player-name">
          <div class="figure-wrap">
            <div class="figure-shadow"></div>
            ${skinBig}
          </div>
          <div class="identity">
            <label class="field-label" for="nick">Nombre de jugador</label>
            <div class="name-row">
              <input id="nick" class="js-nick" maxlength="16" autocomplete="off" spellcheck="false" value="${esc(S.nick)}" ${S.ms ? 'disabled' : ''} aria-describedby="nick-help">
              <button class="pencil" type="button" data-act="nickfocus" aria-label="Editar nombre" ${S.ms ? 'disabled' : ''}><svg viewBox="0 0 24 24"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 013 3L7 19l-4 1 1-4Z"/></svg></button>
            </div>
            <div class="valid" id="nick-help"><svg viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg><span class="js-valid">${ok ? 'Nombre válido' : 'Usá de 3 a 16 letras, números o guion bajo.'}</span></div>
            <div class="badges">
              <span class="badge ${S.ms ? '' : 'offline'}"><i></i>${S.ms ? 'Cuenta Microsoft' : 'Modo offline'}</span>
              <span class="badge"><span class="js-current-skin">${currentSkinLabel}</span></span>
              <span class="badge"><span class="js-model">Modelo ${modelLabel}</span></span>
            </div>
            <p class="fine">Este nombre aparece en el juego y en los servidores. Podés cambiarlo cuando quieras.</p>
            <div class="profile-stats">
              <div><b>${P[S.p] ? `${esc(p.loader || 'Vanilla')} ${esc(p.version || '—')}` : 'Sin instancia'}</b><span>instancia activa</span></div>
              <div><b>${modCount}</b><span>mods instalados</span></div>
              <div><b>${ramGb} GB</b><span>RAM asignada</span></div>
            </div>
          </div>
        </section>

        <section class="panel account" aria-labelledby="account-title">
          <div class="panel-head"><h2 id="account-title">Cuenta</h2><span class="badge ${S.ms ? '' : 'offline'}"><i></i>${S.ms ? 'Microsoft' : 'Offline'}</span></div>
          ${S.ms
            ? `<h3>Cuenta Microsoft vinculada</h3><p>Tu identidad de Microsoft está conectada a Obsidia.</p>`
            : `<h3>Estás jugando sin cuenta</h3><p>Tus datos se guardan en esta PC. Vinculá Microsoft para jugar en servidores premium y usar tu skin oficial.</p>`}
          <ul class="facts">
            <li class="${S.ms ? 'yes' : 'no'}"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M8 12h8"/></svg>Servidores premium</li>
            <li class="${S.ms ? 'yes' : 'no'}"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M8 12h8"/></svg>Skin oficial de Microsoft</li>
            <li class="yes"><svg viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>Mundos y mods en esta PC</li>
          </ul>
          ${S.ms
            ? `<button class="btn btn-sec" type="button" data-act="mso">Cerrar sesión</button>`
            : `<button class="btn btn-primary" type="button" data-act="ms" ${S.ml ? 'disabled' : ''}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 5.5 10.5 4v7H3zM12 3.7 21 2v9H12zM3 12.5h7.5v7L3 18zM12 12.5h9v9l-9-1.5z"/></svg>${S.ml ? 'Esperando a Microsoft...' : 'Vincular cuenta de Microsoft'}</button>`}
          <p class="micro">${S.ms ? 'Tu skin oficial de Microsoft se usa automáticamente en el juego.' : 'No se comparte nada hasta que inicies sesión.'}</p>
        </section>
      </div>

      <section class="panel skins" aria-labelledby="skin-title">
        <div class="panel-head">
          <div><h2 id="skin-title">Skin</h2><p>Elegí la que se aplica en tu cliente.</p></div>
          <div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap">
            <div class="seg" role="group" aria-label="Modelo de skin">
              <button class="chip ${S.skmodel === 'classic' ? 'on' : ''}" data-act="skinmodel" data-m="classic" type="button" aria-pressed="${S.skmodel === 'classic'}">Clásico</button>
              <button class="chip ${S.skmodel === 'slim' ? 'on' : ''}" data-act="skinmodel" data-m="slim" type="button" aria-pressed="${S.skmodel === 'slim'}">Fino</button>
            </div>
            ${S.sk >= 0 ? `<button class="skin-del-bar" type="button" aria-label="Borrar skin activa" data-act="skindel"><svg viewBox="0 0 24 24"><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14M10 11v6M14 11v6"/></svg></button>` : ''}
          </div>
        </div>
        <div class="skin-grid">
          ${S.skins.map((u,i) => {
            const isSelected = i === S.sk;
            return `<button class="skin-tile ${isSelected ? 'selected' : ''}" type="button" data-skin="${i}" aria-pressed="${isSelected}" data-act="skin" data-i="${i}">
              <span class="in-use">${isSelected ? 'En uso' : ''}</span><span class="face">${skinFace(i,68,S.skins)}</span><span class="label">Skin ${i+1}</span></button>`;
          }).join('')}
          <button class="skin-tile upload" type="button" data-act="skinup" ${S.skins.length >= 4 ? 'disabled aria-disabled="true" style="opacity:.45;cursor:default"' : ''}><svg viewBox="0 0 24 24"><path d="M12 16V4"/><path d="m7 9 5-5 5 5"/><path d="M4 20h16"/></svg><b>Subir PNG</b><span>64×64 · máximo 4 skins</span></button>
        </div>
        <div class="skin-foot">
          <div class="skin-note"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/></svg><span>En modo offline la skin se aplica con un resource pack automático: solo la ves vos. Con cuenta Microsoft se usa tu skin oficial.</span></div>
          <span class="png-spec">PNG · 64×64</span>
        </div>
        <input type="file" id="skinfile" accept="image/png" style="display:none" aria-hidden="true">
      </section>

      <section class="preview" aria-labelledby="preview-title">
        <div class="preview-bg"></div>
        <div class="preview-copy">
          <h2 id="preview-title">Cómo se ve en el juego</h2>
          <p>Tu nombre aparece sobre el personaje. ${S.ms ? 'Con cuenta Microsoft se usa tu skin oficial.' : 'En modo offline, los demás jugadores ven la skin por defecto.'}</p>
          <div class="preview-chips">
            <span class="js-name">${esc(S.nick || 'Jugador')}</span>
            <span class="js-current-skin">${currentSkinLabel}</span>
            <span class="js-model">Modelo ${modelLabel}</span>
          </div>
        </div>
        <div class="preview-figure"><span class="nametag js-name">${esc(S.nick || 'Jugador')}</span><div class="figure-shadow"></div>${previewSkinImg}</div>
      </section>
    </div>`;
  }

  // ───────────── Lanzar el juego ─────────────
  async function findLoaderVersion(p){
    const inst = await invoke('get_installed_versions').catch(()=>[]);
    const installed = Array.isArray(inst) ? inst : [];
    const mc = p.version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const rx = p.loader==='Fabric' ? new RegExp('^fabric-loader-.+-'+mc+'$') : new RegExp('^'+mc+'-forge-');
    // Si la instancia fija una versión del loader, esa manda; la más nueva es solo respaldo.
    if (p.loaderVersion){
      const exact = p.loader==='Fabric'
        ? `fabric-loader-${p.loaderVersion}-${p.version}`
        : `${p.version}-forge-${p.loaderVersion}`;
      if (installed.includes(exact)) return exact;
    }
    // Orden numérico ("0.16.10" > "0.9.3"); el orden alfabético elegiría el loader equivocado.
    return installed.filter(v => rx.test(v)).sort((a,b) => a.localeCompare(b, undefined, { numeric:true })).pop() || null;
  }
  async function launchVersionId(p){
    if (!p.loader || p.loader==='Vanilla') return p.version;
    let id = await findLoaderVersion(p);
    if (id) return id;
    S.stage = 'loader'; S.pg = 3; render();
    /** @type {{ mcVersion: string, loaderVersion?: string }} */
    const installArgs = { mcVersion:p.version };
    if (p.loaderVersion) installArgs.loaderVersion = p.loaderVersion;
    await invoke(p.loader==='Fabric' ? 'install_fabric' : 'install_forge', installArgs);
    id = await findLoaderVersion(p);
    if (!id) throw new Error('No encontré el loader '+p.loader+' después de instalarlo.');
    return id;
  }
  // Aplica la skin elegida al resource pack del juego (solo modo offline).
  async function applySkinToGame(silent, instance){
    if (S.ms){ if (!silent) toast('Con cuenta Microsoft se usa tu skin oficial: no se aplica el pack offline.', true); return false; }
    const url = S.skins[S.sk];
    if (!url) return false;
    const instId = instance || inst();
    try {
      await invoke('apply_offline_skin', { skinPngBase64:String(url), model:S.skmodel||'classic', mcVersion:(P[S.p]&&P[S.p].version)||null, instance:instId });
      if (!silent) toast('Skin aplicada al juego.');
      return true;
    } catch(e){ if (!silent) toast('No se pudo aplicar la skin: '+e, true); return false; }
  }
  async function play(server){
    if (S.pg !== null) return;
    const p = P[S.p];
    if (!p){ openNew(); return; }
    if (!validName(S.nick)){ toast('Tu nombre de jugador no es válido. Cambialo en Mi cuenta.', true); S.tab = 'account'; render(); return; }
    const instanceId = inst();
    S.pg = 0; S.stage = 'version'; S.launchErr = ''; S.launchMsg = ''; render();
    let un = null;
    /** @type {null | (() => void)} */
    let unClosed = null;
    try {
      un = await listen('launch-progress', ev => {
        const { stage, done, total, instance } = ev.payload || {};
        if (instance && instance !== instanceId) return;
        const r = STG[stage] || [0,99];
        S.stage = stage;
        S.pg = Math.min(99, Math.round(r[0] + (r[1]-r[0]) * (total > 0 ? done/total : 0)));
        if (S.tab==='home') render();
      });
    } catch(e){ /* sin eventos: igual se lanza */ }
    // Registrar antes de lanzar: si el juego cierra rápido, el evento no se pierde.
    try {
      unClosed = await listen('minecraft-closed', (ev) => {
        const payload = ev.payload || {};
        if (payload.instance && payload.instance !== instanceId) return;
        S.pg = null; S.launchMsg = ''; render();
        if (unClosed) { try { unClosed(); } catch(e){} unClosed = null; }
      });
    } catch(e){ unClosed = null; }
    try {
      // Solo Vanilla con ID usa launch_instance.
      const useNew = instanceId !== 'legacy' && (!p.loader || p.loader === 'Vanilla');
     let loaderId = null;
    if (!useNew) {
      loaderId = await launchVersionId(p);
    }
      // Volver a aplicar la skin elegida: si cambió el pack activo, igual queda puesta.
      if (S.ms) invoke('remove_offline_skin', { instance:instanceId }).catch(()=>{});
      else if (S.skins[S.sk]) await applySkinToGame(true);
       const msg = useNew
         ? await invoke('launch_instance', { id:instanceId, server:server||null, username:S.ms ? null : S.nick })
         : await invoke('launch_version', { version:loaderId, username:S.nick, ram:p.ram+'G', javaPath:p.java||null, server:server||null, fullscreen:!!p.fullscreen, instance:instanceId });
      S.pg = 100; S.launchMsg = msg; render();
    } catch(e){
      S.pg = null; S.launchErr = String(e); render();
      if (unClosed) { try { unClosed(); } catch(_){} unClosed = null; }
    } finally { if (un) un(); }
  }

  // ───────────── Persistencia ─────────────
  async function persistSettings(){
    try { await invoke('save_settings', { settings:{ ram:S.defRam+'G', java_path:null, light_mode:S.t.lite, console:false, username:S.nick } }); } catch(e){ console.error(e); }
  }
  function saveSkins() {
    try {
      localStorage.setItem('obsidia.skins', JSON.stringify(S.skins));
      localStorage.setItem('obsidia.sk', String(S.sk));
      localStorage.setItem('obsidia.skmodel', S.skmodel);
      return true;
    } catch(e){
      toast('No se pudieron guardar las skins: almacenamiento lleno.', true);
      return false;
    }
  }
  async function loadProfiles(select){
    // Si profiles.json está dañado se avisa (con la ruta del respaldo) en vez de mostrar una lista vacía sin explicación.
    const list = await invoke('get_profiles').catch(e => { toast(String(e), true); return []; });
    P = (list||[]).map(p => {
      const loader = p.loader || 'Vanilla';
      return { n:p.name, version:p.version, loader, loaderVersion:p.loader_version||'', ram:normRam(p.ram), java:p.java_path||'', username:p.username, fullscreen:!!p.fullscreen, id:p.id||'', block:p.block||'', s:loader+' · '+p.version };
    });
    let i = P.findIndex(x => x.n === select);
    if (i < 0) { try { i = P.findIndex(x => x.n === localStorage.getItem('obsidia.sel')); } catch(e){ i = -1; } }
    S.p = i >= 0 ? i : 0;
    render();
  }
  function selectProfile(i){
    S.p = i;
    try { localStorage.setItem('obsidia.sel', P[i] ? P[i].n : ''); } catch(e){}
  }
  function openNew(){
    S.np = true; S.npEdit = -1; S.npName = ''; S.npVersion = VERSIONS[0] || '1.21.4'; S.npLoader = 'Vanilla';
    S.npLoaderVersion = ''; S.npRam = Math.min(S.defRam, S.totalRam); S.npJava = ''; S.npErr = ''; S.npBusy = false; S.npFullscreen = false; S.npBlock = ''; render();
  }
  function openEdit(i){
    const p = P[i]; if (!p) return;
    // Un perfil legacy no tiene id: editarlo crearía otra instancia y dejaría un duplicado.
    if (!p.id){ toast('Este perfil es legacy. Usá «Migrar» para convertirlo en una instancia editable.', true); return; }
    S.np = true; S.npEdit = i; S.npName = p.n; S.npVersion = p.version; S.npLoader = p.loader || 'Vanilla';
    S.npLoaderVersion = p.loaderVersion || '';
    S.npRam = p.ram; S.npJava = p.java || ''; S.npErr = ''; S.npBusy = false; S.npFullscreen = !!p.fullscreen; S.npBlock = p.block || ''; render();
  }
  async function saveProfileFromModal(){
    // Doble clic: la segunda llamada no debe crear otra instancia.
    if (S.npBusy) return;
    S.npBusy = true;
    render();
    try {
      const val = id => { const e = Q(id); return e ? e.value : ''; };
      const name = val('#np-name').trim() || ('Instancia '+(P.length+1));
      S.npName = name; S.npVersion = val('#np-version') || S.npVersion || (VERSIONS && VERSIONS[0]) || '1.21.4'; S.npLoader = val('#np-loader') || 'Vanilla';
      S.npRam = +val('#np-ram') || 4; S.npJava = val('#np-java'); S.npFullscreen = !!(Q('#np-fullscreen') && Q('#np-fullscreen').checked); S.npBlock = val('#np-block');
      // Resolver la versión del loader antes de guardar: única fuente de verdad.
      let loaderVersion = S.npLoaderVersion || '';
      if (S.npLoader && S.npLoader !== 'Vanilla'){
        try { loaderVersion = await invoke('resolve_loader_version_cmd', { loader:S.npLoader, mcVersion: (S.npVersion || '') }) || ''; }
        catch(e){ S.npErr = `No se pudo resolver la versión del loader ${S.npLoader}: ${e}`; return; }
        if (!loaderVersion){ S.npErr = `El loader ${S.npLoader} no tiene versión para ${S.npVersion}`; return; }
      }
      const old = S.npEdit >= 0 ? P[S.npEdit] : null;
      // Defensa extra: un perfil legacy nunca debe pasar por create_instance.
      if (old && !old.id){ S.npErr = 'Este perfil es legacy. Usá «Migrar» antes de editarlo.'; return; }
      if (P.some((x,i) => x.n === name && i !== S.npEdit)){ S.npErr = 'Ya existe una instancia con ese nombre.'; return; }
      if (old && old.id) {
        await invoke('update_instance', { id: old.id, profile:{ name, version:S.npVersion, username:S.nick, ram:S.npRam+'G', java_path:S.npJava||null, loader:S.npLoader, loader_version: loaderVersion || null, fullscreen:S.npFullscreen, block:S.npBlock||null, id: old.id } });
      } else {
        await invoke('create_instance', { profile:{ name, version:S.npVersion, username:S.nick, ram:S.npRam+'G', java_path:S.npJava||null, loader:S.npLoader, loader_version: loaderVersion || null, fullscreen:S.npFullscreen, block:S.npBlock||null, id: newId() } });
      }
      await loadProfiles(name);
      selectProfile(S.p);
      S.np = false; S.npEdit = -1; S.npErr = '';
    } catch(e){ S.npErr = String(e); }
    finally { S.npBusy = false; render(); }
  }

  // ───────────── Eventos ─────────────
  // Cerrar el modal de Microsoft también cancela la espera en Rust: así no queda una sesión
  // guardada en segundo plano mientras la interfaz dice "offline".
  let msCancelled = false;
  function closeMs(){ msCancelled = true; invoke('ms_cancel').catch(()=>{}); S.msc = null; render(); }
  async function onClick(e){
    const b = e.target.closest('[data-act]');
    if (!b) return;
    const a = b.dataset.act, d = b.dataset;
    if (a=='go'){ S.tab = d.t; render(); if (d.t==='servers') pingAll(); if (d.t==='mods'){ if (S.content.mods && S.content.mods.length) { doSearch(); checkUpdatesIfDue('mods'); } else { await syncInventory(inst()); doSearch(); checkUpdatesIfDue('mods'); } } }
    else if (a=='sel'){ selectProfile(+d.i); render(); syncInventory(inst()).then(()=>{ if (S.tab==='home'||S.tab==='profiles') render(); }).catch(()=>{}); }
    else if (a=='play'){ play(); }
    else if (a=='new'){ openNew(); }
    else if (a=='ped'){ openEdit(+d.i); }
    else if (a=='migrate'){
        const p = P[+d.i];
        if (!p || p.id){ toast('Este perfil ya es una instancia propia.', true); return; }
        const confirmMsg = `¿Migrar el perfil '${p.n}' a una instancia independiente?\n\nSe copiarán mods, shaders, resource packs, configuraciones y options.txt desde .minecraft.\n\nLa instalación original de .minecraft no se moverá ni se borrará.\nLos mundos compartidos se conservan para todas las instancias.\n\nEsta acción es por este perfil solamente; no se migra nada automáticamente.`;
        if (!confirm(confirmMsg)) return;
        S.migratingProfile = p.n;
        render();
        try {
            await invoke('migrate_legacy_profile', { profileName: p.n });
            await loadProfiles(p.n);
            selectProfile(S.p);
            toast('Perfil migrado a instancia independiente.');
        } catch(e){
            toast('No se pudo migrar: '+String(e), true);
        } finally {
            S.migratingProfile = '';
            render();
        }
    }
    else if (a=='npclose'){ if (e.target !== b) return; S.np = false; S.npEdit = -1; render(); }
    else if (a=='npcreate'){ saveProfileFromModal(); }
    else if (a=='npdel'){
      const p = P[S.npEdit];
      if (!p) return;
      S.npErr = '';
      S.npBusy = true; render();
      try {
        if (p.id) await invoke('delete_instance', { id:p.id });
        else await invoke('delete_profile', { name:p.name || p.n });
        await loadProfiles(); S.p = 0; S.np = false; S.npEdit = -1;
      }
      catch(err){ S.npErr = String(err); }
      finally { S.npBusy = false; render(); }
    }
    else if (a=='f'){ S.f = d.c; render(); doSearch(); }
    else if (a=='retrysearch'){ doSearch(); }
    else if (a=='tg'){ S.t[d.k] = !S.t[d.k]; persistSettings(); render(); }
    else if (a=='seg'){ S.sec = d.s; S.q = ''; render(); doSearch(); }
    else if (a=='ptg'){ const x = S.pstate[+d.i]; if (x){ x.enabled = !x.enabled; S.pstate = [...S.pstate.filter(p=>p.enabled), ...S.pstate.filter(p=>!p.enabled)]; savePacks(); render(); } }
    else if (a=='pup'){ const i = +d.i; if (i>0 && S.pstate[i].enabled && S.pstate[i-1].enabled){ [S.pstate[i-1], S.pstate[i]] = [S.pstate[i], S.pstate[i-1]]; savePacks(); render(); } }
    else if (a=='pdown'){ const i = +d.i; if (i<S.pstate.length-1 && S.pstate[i].enabled && S.pstate[i+1].enabled){ [S.pstate[i+1], S.pstate[i]] = [S.pstate[i], S.pstate[i+1]]; savePacks(); render(); } }
    else if (a=='pauto'){ S.pstate = [...S.pstate].sort((x,y) => (Number(y.enabled)-Number(x.enabled)) || x.name.localeCompare(y.name)); savePacks(); render(); }
    else if (a=='inst'){ doInstall(d.kind, d.slug); }
    else if (a=='plancancel'){ if (e.target !== b) return; const cslug = S.planSlug; const cinst = S.planInstance || inst(); S.plan = null; S.planSlug=''; S.planKind=''; S.planId=null; S.planInstance=''; progressClear(cslug, cinst); render(); }
    else if (a=='planinstall'){ doInstallConfirmed(); }
    else if (a=='fdel'){
      const folder = { mods:'mods', shaders:'shaderpacks', packs:'resourcepacks' }[d.kind];
      try { await invoke('delete_file_in', { folder, name:d.file, instance:inst() }); forgetFile(d.kind, d.file); toast('Eliminado.'); }
      catch(err){ toast('No se pudo eliminar: '+err, true); }
      await refreshKind(d.kind);
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
      if (/\s/.test(ad)){ toast('La dirección no puede tener espacios.', true); return; }
      // Guardar primero el nuevo: si falla, el servidor anterior sigue intacto.
      try {
        await invoke('save_server', { name:nm, address:ad, version:(P[S.p] && P[S.p].version) || null });
      } catch(err){ toast('No se pudo guardar: '+err, true); return; }
      if (S.es && S.es !== ad){
        // El nuevo ya está guardado; si falla el borrado, se avisa y quedan ambos.
        try { await invoke('delete_server', { address:S.es }); }
        catch(err){ toast('Se guardó el servidor nuevo, pero no se pudo borrar el anterior: '+err, true); }
      }
      await loadServers(); S.af = false; S.es = null; S.ns = ''; S.na = ''; render(); pingAll();
    }
    else if (a=='sdel'){
      const s = S.sv[+d.i];
      if (!s) return;
      try { await invoke('delete_server', { address:s.a }); await loadServers(); render(); }
      catch(err){ toast('No se pudo eliminar: '+err, true); }
    }
    else if (a=='sref'){ pingAll(); }
    else if (a=='join'){
      const s = S.sv[+d.i];
      if (s){ S.tab = 'home'; render(); play(s.a); }
    }
    else if (a=='skin'){ S.sk = +d.i; saveSkins(); render(); applySkinToGame(); }
    else if (a=='skinmodel'){ S.skmodel = d.m==='slim'?'slim':'classic'; saveSkins(); render(); applySkinToGame(); }
    else if (a=='skindel'){
      try {
        await invoke('remove_offline_skin', { instance:inst() });
        if (S.sk >= 0 && S.sk < S.skins.length){ S.skins.splice(S.sk,1); S.sk = S.skins.length ? 0 : -1; saveSkins(); render(); }
        if (S.sk >= 0) await applySkinToGame();
        else toast('Skin quitada del juego.');
      }
      catch(err){ toast('No se pudo quitar la skin: '+err, true); }
    }
    else if (a=='skinup'){
      if (S.skins.length >= 4){ toast('Máximo 4 skins', true); return; }
      const fi = Q('#skinfile'); if (fi) fi.click();
    }
    else if (a=='nickfocus'){ const n = Q('#nick'); if (n){ n.focus(); n.select(); } }
    else if (a=='ms'){
      S.ml = true; msCancelled = false; render();
      try {
        const dc = await invoke('ms_device_code');
        S.msc = dc; render();
        const profile = await invoke('ms_login', { deviceCode:dc.device_code });
        if (!msCancelled && S.msc){ S.ms = true; S.nick = (profile && profile.name) ? profile.name : S.nick; persistSettings(); toast('Sesión de Microsoft iniciada.'); }
      } catch(err){ if (!msCancelled) toast('No se pudo iniciar sesión: '+err, true); }
      S.ml = false; S.msc = null; msCancelled = false; render();
    }
    else if (a=='msclose'){ if (e.target !== b) return; closeMs(); }
    else if (a=='msopen'){
      if (!isMicrosoftUrl(d.url)){ toast('Enlace de inicio de sesión no válido.', true); return; }
      invoke('plugin:opener|open_url', { url:d.url }).catch(() => window.open(d.url, '_blank'));
    }
    else if (a=='mscopy'){ try { await navigator.clipboard.writeText(d.code); toast('Código copiado.'); } catch(err){} }
    else if (a=='mso'){ invoke('ms_logout').catch(()=>{}); S.ms = false; render(); }
  }

  function onChange(e){
    if (e.target && e.target.id=='skinfile'){
      const f = e.target.files && e.target.files[0];
      e.target.value = '';
      if (!f) return;
      if (S.skins.length >= 4){ toast('Máximo 4 skins', true); return; }
      if (f.size > 200 * 1024){ toast('La skin no puede superar los 200 KB.', true); return; }
      if (f.type && f.type !== 'image/png'){ toast('El archivo debe ser un PNG.', true); return; }
      const rd = new FileReader();
      rd.onload = () => {
        const url = String(rd.result || '');
        let sig = '';
        try { sig = atob((url.split(',')[1] || '').slice(0, 12)); } catch(err){}
        if (sig.slice(0, 8) !== '\x89PNG\r\n\x1a\n'){ toast('El archivo no es un PNG válido.', true); return; }
        const img = new Image();
        img.onload = () => {
          const w = img.naturalWidth, h = img.naturalHeight;
          if (!((w === 64 && h === 64) || (w === 64 && h === 32))){ toast('La skin debe medir 64×64 o 64×32 píxeles.', true); return; }
          const prev = S.sk;
          S.skins.push(url); S.sk = S.skins.length - 1;
          if (!saveSkins()){ S.skins.pop(); S.sk = prev; render(); return; }
          render(); applySkinToGame();
        };
        img.onerror = () => toast('No se pudo leer la imagen.', true);
        img.src = url;
      };
      rd.onerror = () => toast('No se pudo leer el archivo.', true);
      rd.readAsDataURL(f);
    }
    else if (e.target && e.target.id=='np-version') S.npVersion = e.target.value;
    else if (e.target && e.target.id=='np-loader') S.npLoader = e.target.value;
    else if (e.target && e.target.id=='np-ram') S.npRam = +e.target.value || 4;
    else if (e.target && e.target.id=='np-java') S.npJava = e.target.value;
    else if (e.target && e.target.id=='np-block') S.npBlock = e.target.value;
    else if (e.target && e.target.id=='np-fullscreen') S.npFullscreen = !!e.target.checked;
  }
  function onInput(e){
    const id = e.target.id;
    if (id=='q'){ S.q = e.target.value; clearTimeout(S.dt); S.dt = setTimeout(doSearch, 350); }
    else if (id=='ram'){ S.defRam = +e.target.value; const rv = Q('#ramv'); if (rv) rv.textContent = S.defRam+' GB'; clearTimeout(S.pt); S.pt = setTimeout(persistSettings, 400); }
    else if (id=='nick'){
      S.nick = e.target.value; const acc = Q('#acc'); if (acc) acc.innerHTML = accCard(S.sk, S.nick, S.ms, S.skins);
      const ok = validName(S.nick);
      document.querySelectorAll('.js-name').forEach(n => { n.textContent = S.nick || 'Jugador'; });
      document.querySelectorAll('.js-valid').forEach(n => { n.textContent = ok ? 'Nombre válido' : 'Usá de 3 a 16 letras, números o guion bajo.'; n.style.color = ok ? '#6EE7B7' : '#FCD34D'; });
      if (ok){ clearTimeout(S.pt); S.pt = setTimeout(persistSettings, 400); }
    }
    else if (id=='np-name'){ S.npName = e.target.value; }
    else if (id=='sn'){ S.ns = e.target.value; }
    else if (id=='sa'){ S.na = e.target.value; }
  }
  function onKey(e){
    if ((e.key==='Enter' || e.key===' ') && e.target.matches && e.target.matches('[role=button][data-act]')){ e.preventDefault(); e.target.click(); }
    if (e.key==='Escape' && S.plan){ S.plan = null; S.planSlug=''; S.planKind=''; S.planId=null; S.planInstance=''; render(); }
    if (e.key==='Escape' && (S.np || S.msc)){ if (S.msc){ msCancelled = true; invoke('ms_cancel').catch(()=>{}); } S.np = false; S.msc = null; S.npEdit = -1; render(); }
  }

  // ───────────── Render ─────────────
  function render(){
    const view = Q('#view');
    if (!view) return;
    const ae = document.activeElement;
    const focusId = ae && ae.id ? ae.id : '';
    const selStart = ae && typeof ae.selectionStart === 'number' ? ae.selectionStart : null;
    const selEnd = ae && typeof ae.selectionEnd === 'number' ? ae.selectionEnd : null;
    navHtml = TABS.map(([k,label])=>`<button class="${S.tab===k?'on':''}" data-act="go" data-t="${k}" aria-label="${label}">${ic(k)}<span>${esc(label)}</span></button>`).join('');
    accHtml = accCard(S.sk, S.nick, S.ms, S.skins);
    let html = { home, profiles, mods, servers, settings, account }[S.tab]();
    if (S.np) html += npModal();
    if (S.msc) html += msModal();
    if (S.plan) html += planModal();
    view.innerHTML = html;
    paintSkinCanvases(view);
    S.pendingRender = false;
    if (focusId){
      const el = Q('#' + focusId);
      if (el && typeof el.focus === 'function'){
        el.focus();
        if (selStart !== null && typeof el.setSelectionRange === 'function'){
          try { el.setSelectionRange(selStart, selEnd); } catch(e){}
        }
      }
    }
  }

  // ───────────── Arranque ─────────────
  async function init(){
    render();
    try {
      let select = null;
      try { select = localStorage.getItem('obsidia.sel') || null; } catch(e){}
      await loadProfiles(select);
      render(); // UI con perfiles en cuanto llegan

      // Datos secundarios sin bloquear el primer render: se cargan en paralelo
      // y actualizan la UI cuando terminan.
      (async () => {
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
          render();
        } catch(e){ console.error(e); }
      })();

      // Tareas terciarias no bloqueantes.
      setTimeout(async () => {
        try {
          const preserved = await invoke('get_migration_preserved_saves').catch(()=>[]);
          if (preserved && preserved.length) toast('Se encontraron mundos antiguos conservados en su ubicacion original.');
        } catch(e){}
        try { await loadServers(); } catch(e){}
        try { await syncInventory(inst()); } catch(e){}
        render();
      }, 0);
    } catch(e){ console.error(e); }
    render();
  }

  // Progreso de instalación (install-progress) por slug.
  let progressUn = null;

  // Pre-planificación por mouseover/focusin sobre botones "Instalar" de mods.
  let prePlanTimer = null;

  async function prePlanMod(slug, instance){
    const key = `${slug}|${instance}`;
    if (S.prePlans.find(x => x && x.key === key)) return;
    const p = P[S.p];
    try {
      const result = await invoke('plan_mod_dependencies', { slug, mcVersion:(p&&p.version)||'', loader:(p&&p.loader||'Vanilla').toLowerCase(), instance });
      if (result && result.plan) {
        const iconUrl = (S.results && S.results.mods) ? S.results.mods.find(r => r.slug === slug)?.icon_url : null;
        S.prePlans.push({ key, plan: result.plan, planId: result.plan_id, ts: Date.now(), iconUrl });
        if (S.tab === 'mods') refreshGrid();
      }
    } catch(e){ /* pre-plan opcional: no bloquear */ }
    finally { if (!S.busy[slug]) delete S.progress[key]; if (S.tab === 'mods') refreshGrid(); }
  }

  function onPrePlanMouseOver(e){
    const b = e.target.closest('[data-act="inst"][data-kind="mods"]');
    if (!b) return;
    const slug = b.dataset.slug;
    if (!slug) return;
    const instance = inst();
    prePlanTimer = setTimeout(() => { prePlanMod(slug, instance); prePlanTimer = null; }, 250);
  }

  function onPrePlanMouseOut(){
    if (prePlanTimer) { clearTimeout(prePlanTimer); prePlanTimer = null; }
  }

  // Al pulsar el mouse se cancela el temporizador de pre-planificación para
  // que refreshGrid() no reconstruya la grilla entre el mousedown y el click,
  // lo que perdería el evento de clic del botón presionado.
  function onPrePlanMouseDown(){
    if (prePlanTimer) { clearTimeout(prePlanTimer); prePlanTimer = null; }
  }

  onMount(() => {
    // En producción se bloquean menú contextual y recarga; en dev quedan disponibles
    // para depurar. El menú sí se permite dentro de campos editables.
    const prod = import.meta.env.PROD;
    const blockCtx = e => {
      const t = e.target;
      const editable = t && t.closest && t.closest('input,textarea,select,[contenteditable=""],[contenteditable="true"]');
      if (editable) return;
      e.preventDefault();
    };
    const blockReload = e => { if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && (e.key === 'r' || e.key === 'R' || e.key === 'f' || e.key === 'F'))) e.preventDefault(); };
    if (prod) {
      document.addEventListener('contextmenu', blockCtx);
      document.addEventListener('keydown', blockReload, true);
    }
    document.addEventListener('click', onClick);
    document.addEventListener('change', onChange);
    document.addEventListener('input', onInput);
    document.addEventListener('keydown', onKey);
    // Progreso de instalación en tiempo real.
    listen('install-progress', ev => {
      const { stage, slug, instance, done, total } = ev.payload || {};
      if (!slug) return;
      S.progress[`${slug}|${instance}`] = { stage, done, total };
      if (S.tab === 'mods') refreshGrid();
    }).then(u => { progressUn = u; }).catch(() => { progressUn = null; });
    // Pre-planificación al pasar el mouse sobre botones "Instalar" de mods.
    document.addEventListener('mouseover', onPrePlanMouseOver);
    document.addEventListener('mouseout', onPrePlanMouseOut);
    document.addEventListener('mousedown', onPrePlanMouseDown);
    document.addEventListener('focusin', onPrePlanMouseOver);
    init();
    return () => {
      if (prod) {
        document.removeEventListener('contextmenu', blockCtx);
        document.removeEventListener('keydown', blockReload, true);
      }
      document.removeEventListener('click', onClick);
      document.removeEventListener('change', onChange);
      document.removeEventListener('input', onInput);
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('mouseover', onPrePlanMouseOver);
      document.removeEventListener('mouseout', onPrePlanMouseOut);
      document.removeEventListener('mousedown', onPrePlanMouseDown);
      document.removeEventListener('focusin', onPrePlanMouseOver);
      if (prePlanTimer) clearTimeout(prePlanTimer);
      if (progressUn) { try { progressUn(); } catch(e){} progressUn = null; }
    };
  });
</script>

<svelte:head>
  <style>
    :root{--bg:#0B0A10;--tx:#F2F0F7;--mu:#A09CB2;--mut:#A09CB2;--ink:#F2F0F7;--line:rgba(255,255,255,.075);--line-soft:rgba(255,255,255,.045);--panel:#121019;--panel-2:#16131F;--gl:rgba(255,255,255,.05);--bd:rgba(255,255,255,.09);--v:#8B5CF6;--c:#7DD3FC;color-scheme:dark;box-sizing:border-box;padding-top:env(safe-area-inset-top,0px);padding-bottom:env(safe-area-inset-bottom,0px)}
    html{height:100%;scroll-padding-top:env(safe-area-inset-top,0px)}
    *{box-sizing:border-box}
    body{margin:0;height:100%;color:var(--tx);font:14.5px/1.6 Inter,"Segoe UI",system-ui,sans-serif;background:radial-gradient(900px 620px at 8% -10%,rgba(139,92,246,.10),transparent 60%),radial-gradient(820px 560px at 100% 0%,rgba(125,211,252,.07),transparent 60%),var(--bg)}
    button,input,select{font:inherit;color:inherit}button{cursor:pointer}
    #wrap{height:100%;overflow-y:auto;overflow-x:hidden}
    #app{display:flex;min-width:0;width:100%;min-height:100%}
    #app > aside{width:236px;flex:none;padding:28px 18px 22px;display:flex;flex-direction:column;gap:28px;border-right:1px solid rgba(255,255,255,.045);background:transparent;position:sticky;top:0;height:100vh;align-self:flex-start}
    .logo{display:flex;align-items:center;gap:11px;padding:0 10px;font:600 19px/1 Sora,Inter,system-ui,sans-serif;letter-spacing:.2px}
    nav{display:flex;flex-direction:column;gap:2px}
    nav button{display:flex;align-items:center;gap:12px;height:42px;padding:0 12px;border:0;border-radius:11px;background:transparent;color:#A9A5B8;font-size:13.5px;transition:background .15s,color .15s;text-align:left}
    nav button svg{flex:none;opacity:.75}
    nav button:hover{background:rgba(255,255,255,.04);color:var(--tx)}
    nav button.on{color:#fff;background:linear-gradient(90deg,rgba(167,139,250,.16),rgba(167,139,250,.05))}
    nav button.on svg{opacity:1;color:#C4B5FD}
    .acc{margin-top:auto;display:flex;align-items:center;gap:11px;padding:11px;border-radius:14px;text-align:left;width:100%;border:1px solid rgba(255,255,255,.055);background:rgba(255,255,255,.022);transition:border-color .2s,background .2s}
    .acc:hover{background:rgba(255,255,255,.05);border-color:rgba(255,255,255,.10)}.acc.on{border-color:rgba(167,139,250,.45);background:rgba(167,139,250,.08)}
    .acc .who{min-width:0;flex:1}
    .acc .who b{display:block;font-size:12.5px;line-height:1.2;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
    .acc .who span{display:block;font-size:11px;color:var(--mu);margin-top:3px}
    .acc .status{width:7px;height:7px;border-radius:50%;background:#86EFAC;margin-left:auto;flex:none;box-shadow:0 0 0 3px rgba(134,239,172,.12)}
    .mu{color:var(--mu);font-size:13px}
    main{flex:1;min-width:0;padding:32px 40px 22px;display:flex;flex-direction:column;gap:22px}
    h1{margin:0;font:600 26px/1.2 Sora,Inter,system-ui,sans-serif;letter-spacing:-.2px}h2{margin:0;font:600 18px Sora,sans-serif}
    .gl{background:var(--gl);border:1px solid var(--bd);border-radius:18px}

    /* ── Inicio ── */
    .home{display:flex;flex-direction:column;width:100%;gap:0;flex:1;min-height:0}
    .topbar{display:flex;align-items:flex-start;justify-content:space-between;gap:20px;margin-bottom:24px;flex-wrap:wrap}
    .hello{font-size:10.5px;font-weight:600;letter-spacing:.14em;text-transform:uppercase;color:#8B869E;margin-bottom:5px}
    .top-right{display:flex;align-items:center;gap:8px;padding-top:4px}
    .pill{display:inline-flex;align-items:center;gap:7px;height:32px;padding:0 12px;border-radius:999px;border:1px solid rgba(134,239,172,.18);background:rgba(134,239,172,.06);color:#BBF7D0;font-size:11.5px}
    .pill i{width:6px;height:6px;border-radius:50%;background:#86EFAC}
    .hero{position:relative;overflow:hidden;height:352px;border-radius:26px;border:1px solid rgba(255,255,255,.075);background:#141020 url('/images/hero.png') center 42%/cover no-repeat;box-shadow:0 24px 70px rgba(0,0,0,.42)}
    .hero::before{content:"";position:absolute;inset:0;z-index:1;pointer-events:none;background:linear-gradient(90deg,rgba(11,9,16,.90) 0%,rgba(11,9,16,.68) 32%,rgba(11,9,16,.24) 58%,rgba(11,9,16,.04) 100%),linear-gradient(0deg,rgba(9,8,13,.42),transparent 48%)}
    .hero-in{position:relative;z-index:2;height:100%;display:flex;flex-direction:column;justify-content:center;gap:13px;padding:0 44px;max-width:78%}
    .eyebrow{font-size:10.5px;font-weight:600;letter-spacing:.16em;color:#DCD2F5;text-transform:uppercase;text-shadow:0 1px 6px rgba(0,0,0,.5)}
    .hero h2{margin:0;font:600 42px/1.05 Sora,Inter,system-ui,sans-serif;letter-spacing:-.6px;text-shadow:0 2px 14px rgba(0,0,0,.45);overflow-wrap:anywhere}
    .meta{font-size:13px;color:#DAD4E8;text-shadow:0 1px 6px rgba(0,0,0,.5)}
    .meta b{color:#F5F2FB;font-weight:500}
    .meta .sep{margin:0 8px;color:#B4AEC4}
    .actions{display:flex;align-items:center;gap:10px;margin-top:8px;flex-wrap:wrap}
    .btn-primary{display:inline-flex;align-items:center;gap:10px;height:54px;padding:0 30px 0 11px;border:0;border-radius:999px;cursor:pointer;font:600 15px/1 Inter,system-ui,sans-serif;letter-spacing:.1px;color:#14101F;background:linear-gradient(135deg,#F0ABFC 0%,#C4B5FD 46%,#7DD3FC 100%);box-shadow:0 10px 32px rgba(167,139,250,.32),inset 0 1px 0 rgba(255,255,255,.45);transition:transform .14s,box-shadow .16s,filter .16s}
    .btn-primary:hover{filter:brightness(1.05);box-shadow:0 14px 38px rgba(167,139,250,.42);transform:translateY(-2px)}
    .btn-primary:active{transform:translateY(0)}
    .btn-primary:disabled{cursor:default;opacity:.85;transform:none;filter:none}
    .btn-primary .play-ico{width:32px;height:32px;flex:none;display:grid;place-items:center;border-radius:50%;background:rgba(20,16,31,.14);color:#171326}
    .btn-ghost{height:50px;padding:0 22px;cursor:pointer;border:1px solid rgba(255,255,255,.13);border-radius:14px;background:rgba(255,255,255,.045);color:#DDD9EA;font:500 14px/1 Inter,system-ui,sans-serif;transition:.15s}
    .btn-ghost:hover{background:rgba(255,255,255,.08);border-color:rgba(255,255,255,.2)}
    .hint{position:absolute;right:26px;bottom:22px;z-index:2;display:inline-flex;align-items:center;gap:8px;padding:8px 13px;border-radius:999px;border:1px solid rgba(255,255,255,.10);background:rgba(12,10,18,.42);backdrop-filter:blur(8px);font-size:11.5px;color:#C9C4DA;max-width:calc(100% - 52px);overflow:hidden;white-space:nowrap;text-overflow:ellipsis}
    .hint i{width:6px;height:6px;border-radius:50%;background:#7DD3FC;flex:none}
    .home .launch{margin-top:18px;padding:14px 18px}
    .grid{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:22px;margin:24px 0 22px 0}
    .head{display:flex;align-items:baseline;justify-content:space-between;gap:12px;margin-bottom:12px;flex-wrap:wrap}
    .head h3{margin:0;font:600 15.5px/1.2 Sora,Inter,system-ui,sans-serif;letter-spacing:-.1px}
    .head span{font-size:11.5px;color:var(--mu)}
    .link{border:0;background:transparent;color:#9C93B8;font-size:12px;padding:0;cursor:pointer}
    .link:hover{color:#C4B5FD}
    .cards{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:12px}
    .pcard{position:relative;display:flex;align-items:center;gap:13px;padding:13px;min-height:92px;border:1px solid rgba(255,255,255,.07);border-radius:16px;background:linear-gradient(180deg,rgba(255,255,255,.035),rgba(255,255,255,.012));cursor:pointer;transition:.16s}
    .pcard:hover{background:rgba(255,255,255,.05);border-color:rgba(255,255,255,.12);transform:translateY(-1px)}
    .pcard.on{border-color:rgba(167,139,250,.45);background:linear-gradient(180deg,rgba(167,139,250,.14),rgba(167,139,250,.04));box-shadow:0 10px 30px rgba(139,92,246,.10)}
    .pcard.on::before{content:"";position:absolute;left:-1px;top:16px;bottom:16px;width:3px;border-radius:0 3px 3px 0;background:linear-gradient(180deg,#7DD3FC,#A78BFA)}
    .slot{width:64px;height:64px;flex:none;display:grid;place-items:center}
    .slot img{width:60px;height:60px;object-fit:contain;display:block;filter:drop-shadow(0 6px 8px rgba(0,0,0,.40))}
    .pinfo{min-width:0;flex:1}
    .pinfo b{display:block;font-size:14px;font-weight:600;line-height:1.25;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
    .pinfo span{display:block;margin-top:4px;font-size:11.5px;color:var(--mu);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
    .pinfo span.on{color:#C4B5FD}
    .edit{flex:none;height:28px;padding:0 11px;border-radius:8px;border:1px solid rgba(255,255,255,.10);background:rgba(255,255,255,.04);color:#C9C7DD;font:500 11.5px Inter,system-ui,sans-serif;cursor:pointer;transition:.15s}
    .edit:hover{background:rgba(255,255,255,.09);border-color:rgba(255,255,255,.18)}
    .newcard{display:flex;align-items:center;justify-content:center;gap:8px;min-height:92px;border-radius:16px;border:1px dashed rgba(255,255,255,.13);background:rgba(255,255,255,.012);color:#9C97AC;font:500 12.5px Inter,system-ui,sans-serif;cursor:pointer;transition:.16s}
    .newcard:hover{color:#D5D0E3;border-color:rgba(167,139,250,.4);background:rgba(167,139,250,.05)}
    .home .grid > .panel{border:1px solid rgba(255,255,255,.07);border-radius:16px;background:linear-gradient(180deg,rgba(255,255,255,.03),rgba(255,255,255,.01));padding:14px;height:100%;width:600px;justify-self:end}
    .panel .head{margin:0 2px 6px}
    .rows{list-style:none;margin:0;padding:0}
    .rows li{display:flex;align-items:flex-start;gap:11px;padding:11px 2px;border-top:1px solid rgba(255,255,255,.05)}
    .rows li:first-child{border-top:0}
    .nthumb{width:36px;height:36px;flex:none;border-radius:10px;display:grid;place-items:center;border:1px solid rgba(255,255,255,.07)}
    .nthumb.cy{background:linear-gradient(140deg,#164B57,#0E2026);color:#9BE8F6}
    .nthumb.vi{background:linear-gradient(140deg,#3B2A63,#1C1530);color:#C4B5FD}
    .nthumb.li{background:linear-gradient(140deg,#3C4A1D,#1B2110);color:#D9F99D}
    .rows .t{min-width:0;flex:1}
    .rows .t b{display:block;font-size:12.5px;font-weight:500;line-height:1.4}
    .rows .t span{display:block;margin-top:2px;font-size:11px;color:var(--mu)}
    .strip{display:flex;align-items:center;gap:0;flex-wrap:wrap;margin-top:auto;border:1px solid rgba(255,255,255,.07);border-radius:16px;background:linear-gradient(180deg,rgba(255,255,255,.028),rgba(255,255,255,.012));overflow:hidden}
    .stat{display:flex;align-items:center;gap:11px;padding:14px 20px;border-left:1px solid rgba(255,255,255,.05)}
    .stat:first-child{border-left:0}
    .stat .ico{width:34px;height:34px;flex:none;display:grid;place-items:center;border-radius:10px;background:rgba(167,139,250,.11);color:#C4B5FD;border:1px solid rgba(167,139,250,.16)}
    .stat .ico.cy{background:rgba(125,211,252,.10);color:#BAE6FD;border-color:rgba(125,211,252,.16)}
    .stat .ico.pe{background:rgba(253,186,116,.09);color:#FED7AA;border-color:rgba(253,186,116,.16)}
    .stat b{display:block;font-size:14px;font-weight:600;line-height:1.2}
    .stat small{display:block;font-size:11px;color:var(--mu);margin-top:1px}
    .strip .right{margin-left:auto;padding:0 20px;font-size:11.5px;color:#837E93}
    @media (max-width:1380px){.grid{grid-template-columns:1fr}}
    @media (max-width:1100px){.cards{grid-template-columns:repeat(2,minmax(0,1fr))}}
    @media (max-width:940px){main{padding:26px 22px 30px}.hero-in{padding:0 30px}.strip .right{margin-left:0;padding:12px 20px}}

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
    .row{display:flex;align-items:center;justify-content:space-between;gap:20px;padding:20px 22px;flex-wrap:wrap}
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
  
    /* ── Modal de perfil ── */
    .np-modal{width:min(600px,94vw);max-height:88vh;display:flex;flex-direction:column;overflow:hidden;border:1px solid rgba(255,255,255,.14);border-radius:22px;background:linear-gradient(180deg,#17152F 0%,#131126 100%);box-shadow:0 28px 90px rgba(0,0,0,.62)}
    .np-head{display:flex;align-items:flex-start;justify-content:space-between;gap:16px;padding:26px 32px 0;flex-wrap:wrap}
    .np-head h2{margin:0;font:700 22px/1.25 Sora,Inter,system-ui,sans-serif;letter-spacing:.2px}
    .np-sub{margin-top:6px;font-size:12.5px;color:var(--mu)}
    .np-x{width:36px;height:36px;flex:none;display:grid;place-items:center;border:1px solid var(--bd);border-radius:11px;background:rgba(255,255,255,.05);color:var(--mu);font-size:14px;line-height:1;transition:background .15s,color .15s,border-color .15s}
    .np-x:hover{background:rgba(255,255,255,.10);color:var(--tx);border-color:rgba(255,255,255,.18)}
    .np-body{padding:24px 32px 8px;display:flex;flex-direction:column;gap:22px;overflow:auto}
    .np-field{display:flex;flex-direction:column;gap:8px;min-width:0}
    .np-field>label{font-size:11.5px;font-weight:600;letter-spacing:.09em;text-transform:uppercase;color:#B9B7CE}
    .np-in{width:100%;height:48px;padding:0 16px;border:1px solid rgba(255,255,255,.12);border-radius:12px;background:rgba(255,255,255,.045);color:var(--tx);font:inherit;font-size:14.5px;outline:none;transition:border-color .15s,box-shadow .15s,background .15s}
    .np-in:focus{border-color:rgba(34,211,238,.55);box-shadow:0 0 0 3px rgba(34,211,238,.12)}
    .np-selwrap{position:relative;min-width:0}
    .np-selwrap select{appearance:none;-webkit-appearance:none;padding-right:44px;cursor:pointer}
    .np-selwrap select:disabled{cursor:not-allowed;color:#B4B2C8;background:rgba(255,255,255,.025);border-style:dashed;border-color:rgba(255,255,255,.10)}
    .np-selwrap .np-ico{position:absolute;right:15px;top:50%;transform:translateY(-50%);display:flex;color:#8E8CA6;pointer-events:none}
    .np-grid2{display:grid;grid-template-columns:1fr 1fr;gap:20px}
    .np-note{display:flex;align-items:flex-start;gap:10px;padding:12px 14px;border:1px solid rgba(139,92,246,.22);border-radius:12px;background:rgba(139,92,246,.075);font-size:12.5px;line-height:1.55;color:#BEBBD6}
    .np-note svg{flex:none;margin-top:2px;color:#A78BFA}
    .np-toggle{position:relative;display:flex;align-items:center;justify-content:space-between;gap:18px;padding:14px 16px;border:1px solid rgba(255,255,255,.10);border-radius:12px;background:rgba(255,255,255,.028);cursor:pointer}
    .np-t{display:block;font-size:14px;font-weight:600}
    .np-d{display:block;margin-top:2px;font-size:12px;color:var(--mu)}
    .np-toggle input{position:absolute;opacity:0;width:0;height:0}
    .np-switch{position:relative;width:46px;height:26px;flex:none;border-radius:999px;background:rgba(255,255,255,.10);border:1px solid rgba(255,255,255,.12);transition:background .15s,border-color .15s}
    .np-switch::after{content:"";position:absolute;top:3px;left:3px;width:18px;height:18px;border-radius:50%;background:#C9C7DD;transition:transform .18s,background .18s}
    .np-toggle input:checked+.np-switch{background:linear-gradient(90deg,rgba(34,211,238,.85),rgba(139,92,246,.9));border-color:transparent}
    .np-toggle input:checked+.np-switch::after{transform:translateX(20px);background:#fff}
    .np-toggle input:focus-visible+.np-switch{outline:2px solid var(--c);outline-offset:2px}
    .np-foot{margin-top:18px;padding:18px 32px;border-top:1px solid rgba(255,255,255,.09);background:rgba(0,0,0,.18);display:flex;flex-direction:column;gap:12px}
    .np-err{font-size:12.5px;color:#F87171}
    .np-foot-row{display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap}
    .np-actions{display:flex;gap:10px;margin-left:auto}
    .np-danger{width:42px;height:42px;display:grid;place-items:center;border:1px solid rgba(248,113,113,.28);border-radius:12px;background:rgba(248,113,113,.07);color:#F87171;cursor:pointer;transition:background .15s,border-color .15s}
    .np-danger:hover{background:rgba(248,113,113,.14);border-color:rgba(248,113,113,.45)}
    .np-foot .sec{height:42px;border-radius:12px}
    .np-foot .ib{height:42px;margin:0;padding:0 24px;border-radius:12px}
    @media (max-width:560px){.np-grid2{grid-template-columns:1fr}.np-head{padding:22px 22px 0}.np-body{padding:20px 22px 6px}.np-foot{padding:16px 22px}}

    /* ── Mi cuenta (scoped) ── */
    .account-page {
      width: 100%;
      max-width: 1300px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    }
    .account-page > *{flex-shrink:0}
    .account-page .top {
      display: grid !important;
      grid-template-columns: minmax(0, 1.62fr) minmax(340px, .92fr) !important;
      gap: 16px;
      width: 100%;
      align-items: stretch;
    }
    @media (max-width: 1280px) {
      .account-page .top {
        grid-template-columns: 1fr !important;
      }
    }
    .account-page .page-head{display:flex;align-items:flex-end;justify-content:space-between;gap:24px;margin-bottom:20px;flex-wrap:wrap}
    .account-page .page-sub{margin-top:5px;font-size:13px;color:var(--mut)}
    .account-page .save-state{display:inline-flex;align-items:center;gap:7px;font-size:12px;color:#8E899F}
    .account-page .save-state svg{width:14px;height:14px;stroke:#6EE7B7;fill:none;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}

    .account-page .panel{background:var(--panel,#121019);border:1px solid var(--line,rgba(255,255,255,.075));border-radius:18px;width:auto;height:auto;justify-self:stretch}

    /* ── Player card ── */
    .account-page .profile{position:relative;overflow:hidden;padding:26px 30px;display:flex;gap:30px;align-items:center}
    .account-page .profile::before{
      content:"";position:absolute;inset:-40% 35% 10% -25%;pointer-events:none;
      background:radial-gradient(closest-side,rgba(139,92,246,.16),transparent 72%);
    }
    .account-page .figure-wrap{position:relative;width:142px;height:284px;flex:none;display:flex;align-items:flex-end;justify-content:center;z-index:1}
    .account-page .figure{width:132px;height:264px;filter:drop-shadow(0 22px 24px rgba(0,0,0,.55))}
    .account-page .figure-shadow{
      position:absolute;left:50%;bottom:-6px;width:112px;height:18px;transform:translateX(-50%);
      border-radius:50%;background:radial-gradient(ellipse,rgba(0,0,0,.75),transparent 70%);
    }
    .account-page .identity{position:relative;z-index:1;flex:1;min-width:0;display:flex;flex-direction:column;gap:4px}
    .account-page .field-label{font-size:12px;color:var(--mut);margin-bottom:2px}
    .account-page .name-row{display:flex;align-items:center;gap:10px;border-bottom:1px solid rgba(255,255,255,.12);padding-bottom:5px;transition:border-color .15s}
    .account-page .name-row:focus-within{border-color:rgba(167,139,250,.75)}
    .account-page .name-row input{
      flex:1;min-width:0;background:none;border:0;outline:none;padding:0;
      font:600 33px/1.15 Sora,Inter,system-ui,sans-serif;letter-spacing:-.6px;color:var(--ink);
    }
    .account-page .pencil{
      width:32px;height:32px;flex:none;display:grid;place-items:center;border-radius:9px;
      border:1px solid rgba(255,255,255,.09);background:rgba(255,255,255,.035);color:#B9B4C8;
      transition:.15s;
    }
    .account-page .pencil:hover{color:#fff;background:rgba(255,255,255,.08);border-color:rgba(255,255,255,.18)}
    .account-page .pencil svg{width:15px;height:15px;fill:none;stroke:currentColor;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round}
    .account-page .valid{display:flex;align-items:center;gap:6px;margin-top:7px;font-size:12.5px;color:var(--mint,#6EE7B7);min-height:18px}
    .account-page .valid svg{width:13px;height:13px;fill:none;stroke:currentColor;stroke-width:2.2;stroke-linecap:round;stroke-linejoin:round}
    .account-page .badges{display:flex;flex-wrap:wrap;gap:8px;margin-top:13px}
    .account-page .badge{display:inline-flex;align-items:center;gap:7px;height:27px;padding:0 11px;border-radius:999px;font-size:11.5px;border:1px solid var(--line,rgba(255,255,255,.075));background:rgba(255,255,255,.04);color:#C9C7DD}
    .account-page .badge i{width:6px;height:6px;border-radius:50%;background:#86EFAC;flex:none}
    .account-page .badge.offline{border-color:rgba(252,211,77,.24);background:rgba(252,211,77,.07);color:#FDE68A}
    .account-page .badge.offline i{background:#FCD34D}
    .account-page .fine{margin-top:13px;font-size:12px;line-height:1.5;color:#827D93;max-width:44ch}
    .account-page .profile-stats{display:flex;gap:30px;margin-top:18px;padding-top:15px;border-top:1px solid var(--line-soft,rgba(255,255,255,.045));position:relative;z-index:1}
    .account-page .profile-stats b{display:block;font-size:13px;font-weight:600;line-height:1.3}
    .account-page .profile-stats span{display:block;margin-top:2px;font-size:11px;color:var(--dim,#7E798E)}

    /* ── Account card ── */
    .account-page .account{padding:24px 24px 22px;display:flex;flex-direction:column;gap:12px}
    .account-page .panel-head{display:flex;align-items:center;justify-content:space-between;gap:14px}
    .account-page .panel-head h2{font:600 16px/1.25 Sora,Inter,system-ui,sans-serif;letter-spacing:-.1px}
    .account-page .account h3{font:600 19px/1.3 Sora,Inter,system-ui,sans-serif;letter-spacing:-.25px;margin-top:2px}
    .account-page .account p{font-size:12.5px;line-height:1.6;color:var(--mut)}
    .account-page .facts{display:flex;flex-direction:column;gap:9px;margin:4px 0 2px}
    .account-page .facts li{display:flex;align-items:center;gap:9px;font-size:12.5px;color:#C6C2D4}
    .account-page .facts svg{width:16px;height:16px;flex:none;fill:none;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}
    .account-page .facts .yes svg{stroke:#6EE7B7}
    .account-page .facts .no svg{stroke:#6E6980}
    .account-page .facts .no{color:#8C879C}
    .account-page .btn{margin-top:auto;display:inline-flex;align-items:center;justify-content:center;gap:9px;height:44px;padding:0 18px;border:0;border-radius:12px;font:600 13px/1 Inter,system-ui,sans-serif;transition:.15s}
    .account-page .btn svg{width:16px;height:16px;fill:currentColor}
    .account-page .btn-primary{background:var(--violet-strong,#7C3AED);color:#fff;box-shadow:none;transform:none;filter:none;transition:background .15s,color .15s}
    .account-page .btn-primary:hover{background:var(--violet,#8B5CF6)}
    .account-page .btn-sec{background:rgba(255,255,255,.08);border:1px solid rgba(255,255,255,.14);color:#fff}
    .account-page .btn-sec:hover{background:rgba(255,255,255,.14)}
    .account-page .micro{margin-top:2px;font-size:11px;color:#777286;text-align:center}

    /* ── Skins ── */
    .account-page .skins{margin-top:16px;padding:22px 24px 18px}
    .account-page .skins .panel-head{align-items:flex-start}
    .account-page .skins .panel-head p{margin-top:4px;font-size:12px;color:var(--mut)}
    .account-page .seg{display:inline-flex;gap:3px;padding:3px;border-radius:12px;background:rgba(255,255,255,.045);border:1px solid var(--line);flex:none}
    .account-page .seg button{height:30px;padding:0 15px;border:0;border-radius:9px;background:transparent;color:var(--mut);font-size:12px;font-weight:600;transition:.15s}
    .account-page .seg button:hover{color:var(--ink)}
    .account-page .seg button.on{background:#2A2440;color:#fff;box-shadow:inset 0 0 0 1px rgba(167,139,250,.25)}
    .account-page .skin-grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:12px;margin-top:16px}
    .account-page .skin-tile{
      position:relative;height:138px;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:7px;
      border:1px solid var(--line);border-radius:14px;background:var(--panel-2,#16131F);transition:.16s;
    }
    .account-page .skin-tile:hover{border-color:rgba(255,255,255,.17);background:#191622}
    .account-page .skin-tile.selected{border-color:rgba(167,139,250,.65);background:linear-gradient(180deg,rgba(139,92,246,.13),rgba(139,92,246,.035))}
    .account-page .skin-tile .face{display:block;width:68px;height:68px;overflow:hidden;border-radius:6px;background:#1a1a2e}
    .account-page .skin-tile .face svg{width:100%;height:100%}
    .account-page .skin-tile .label{font-size:12px;color:#C9C5D6}
    .account-page .skin-del-bar{
      width:36px;height:36px;border-radius:10px;border:1px solid var(--line);background:rgba(255,255,255,.06);color:#C9C7DD;cursor:pointer;transition:.15s;flex:none
    }
    .account-page .skin-del-bar:hover{background:#F87171;color:#fff;border-color:transparent}
    .account-page .skin-del-bar svg{width:16px;height:16px;flex:none;fill:none;stroke:currentColor;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round}
    .account-page .in-use{
      position:absolute;top:8px;right:8px;display:none;align-items:center;gap:4px;height:20px;padding:0 7px;
      border-radius:999px;background:rgba(139,92,246,.24);border:1px solid rgba(167,139,250,.38);
      font-size:9.5px;font-weight:600;color:#DDD6FE;letter-spacing:.02em;
    }
    .account-page .skin-tile.selected .in-use{display:inline-flex}
    .account-page .upload{
      border-style:dashed;border-color:rgba(255,255,255,.15);background:rgba(255,255,255,.014);color:#928DA2;gap:8px;
    }
    .account-page .upload:hover{border-color:rgba(167,139,250,.5);background:rgba(139,92,246,.05);color:#D5D0E3}
    .account-page .upload svg{width:20px;height:20px;fill:none;stroke:currentColor;stroke-width:1.8;stroke-linecap:round}
    .account-page .upload b{font-size:12.5px;font-weight:500;color:#CFCBDA}
    .account-page .upload span{font-size:10.5px;color:#7C778C}
    .account-page .skin-foot{display:flex;align-items:flex-start;justify-content:space-between;gap:24px;margin-top:14px;padding-top:14px;border-top:1px solid var(--line-soft)}
    .account-page .skin-note{display:flex;gap:9px;max-width:74ch;font-size:11.5px;line-height:1.55;color:#858092}
    .account-page .skin-note svg{width:15px;height:15px;flex:none;margin-top:1px;fill:none;stroke:#8B84A0;stroke-width:1.7;stroke-linecap:round;stroke-linejoin:round}
    .account-page .png-spec{flex:none;font-size:11px;color:#777286;padding-top:1px}

    /* ── In-game preview ── */
    .account-page .preview{position:relative;margin-top:16px;height:206px;border-radius:18px;overflow:hidden;border:1px solid rgba(255,255,255,.08);background:#0E0B14}
    .account-page .preview>img{position:absolute;inset:0;width:100%;height:100%;object-fit:cover;object-position:center 58%}
    .account-page .preview-bg{position:absolute;inset:0;background:url('/images/hero.png') center 58%/cover no-repeat}
    .account-page .preview::after{
      content:"";position:absolute;inset:0;
      background:linear-gradient(90deg,rgba(9,8,13,.90) 0%,rgba(9,8,13,.70) 28%,rgba(9,8,13,.18) 58%,rgba(9,8,13,0) 100%),
                 linear-gradient(0deg,rgba(9,8,13,.42),transparent 52%);
    }
    .account-page .preview-copy{position:absolute;z-index:2;left:30px;top:50%;transform:translateY(-50%);max-width:430px}
    .account-page .preview-copy h2{font:600 20px/1.25 Sora,Inter,system-ui,sans-serif;letter-spacing:-.2px}
    .account-page .preview-copy p{margin-top:7px;font-size:12.5px;line-height:1.55;color:#B9B4C7;max-width:40ch}
    .account-page .preview-chips{display:flex;gap:7px;margin-top:13px;flex-wrap:wrap}
    .account-page .preview-chips span{display:inline-flex;align-items:center;height:25px;padding:0 10px;border-radius:999px;border:1px solid rgba(255,255,255,.12);background:rgba(12,10,18,.42);font-size:11px;color:#D7D3E2;backdrop-filter:blur(6px)}
    .account-page .preview-figure{position:absolute;z-index:2;right:12%;bottom:0;width:96px;height:192px;display:flex;flex-direction:column;align-items:center;justify-content:flex-end}
    .account-page .nametag{
      position:absolute;top:-4px;left:50%;transform:translateX(-50%);padding:2px 8px;border-radius:3px;
      background:rgba(10,8,16,.55);font:600 12px/1.5 Inter,system-ui,sans-serif;color:#fff;white-space:nowrap;
      text-shadow:0 1px 2px #000;box-shadow:0 1px 0 rgba(0,0,0,.35);
    }
    .account-page .preview-figure .pfigure{width:88px;height:176px;filter:drop-shadow(0 14px 18px rgba(0,0,0,.65))}
    .account-page .preview-figure .figure-shadow{bottom:2px;width:78px;height:12px}

    /* ── Responsive ── */
    @media (max-width:1280px){
      .account-page .account .btn{margin-top:10px}
    }
    @media (max-width:980px){
      .account-page aside{width:76px;padding:24px 12px 18px;align-items:center}
      .account-page .logo{justify-content:center;padding:0}
      .account-page .logo span{display:none}
      .account-page nav a{justify-content:center;width:46px;padding:0}
      .account-page nav a span{display:none}
      .account-page .sidebar-account{justify-content:center;padding:8px}
      .account-page .sidebar-account .who,.account-page .sidebar-account .dot-status{display:none}
      .account-page main{padding:26px 22px 30px}
    }
    @media (max-width:760px){
      .account-page .profile{flex-direction:column;align-items:flex-start;padding:24px}
      .account-page .figure-wrap{align-self:center}
      .account-page .profile-stats{width:100%;gap:20px;flex-wrap:wrap}
      .account-page .skin-grid{grid-template-columns:repeat(2,minmax(0,1fr))}
      .account-page .skin-foot{flex-direction:column;gap:8px}
      .account-page .preview{height:auto;min-height:230px}
      .account-page .preview-copy{position:relative;left:auto;top:auto;transform:none;padding:22px}
      .account-page .preview-figure{right:6%;height:160px;opacity:.55}
      .account-page .png-spec{padding-top:0}
    }
    @media (prefers-reduced-motion:reduce){.account-page *{transition:none!important}}

</style>
</svelte:head>

<div id="wrap"><div id="app">
  <Sidebar {navHtml} {accHtml} accountActive={S.tab=='account'} />
  <main id="view"></main>
</div></div>
<Toast />


