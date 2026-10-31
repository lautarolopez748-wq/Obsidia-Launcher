<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { check } from "@tauri-apps/plugin-updater";
  import { onMount } from "svelte";

  type Profile = { name: string; version: string; username: string; ram: string; java_path: string | null };
  type JavaInfo = { path: string; version: string };
  type ModResult = { slug: string; title: string; description: string; downloads: number };
  type ServerEntry = { name: string; address: string; version?: string | null };
  type AppSettings = { ram: string; java_path: string | null; light_mode: boolean; console: boolean; username: string };

  let view = $state("home");

  // home/profile launch
  let username = $state("Player");
  let version = $state("");
  let ram = $state("2G");
  let javaPath = $state<string>("");
  let profileName = $state("default");
  let profiles = $state<Profile[]>([]);
  let installed = $state<string[]>([]);
  let remote = $state<string[]>([]);
  let javas = $state<JavaInfo[]>([]);
  let status = $state("");
  let loading = $state(false);

  const allVersions = $derived([...new Set([...remote, ...installed])]);

  // mods
  let modQuery = $state("");
  let modResults = $state<ModResult[]>([]);
  let installedMods = $state<string[]>([]);
  let shaderpacks = $state<string[]>([]);
  let resourcepacks = $state<string[]>([]);
  let modStatus = $state("");
  let modLoader = $state("fabric");
  let modSec = $state("mods");
  let rpOrder = $state<string[]>([]);
  let rpActive = $state<Record<string, boolean>>({});
  let rpDeleteConfirm = $state<string | null>(null);

  // servers
  let servers = $state<ServerEntry[]>([]);
  let serverName = $state("");
  let serverAddr = $state("");
  let serverStatus = $state("");

  // settings
  let settings = $state<AppSettings>({ ram: "2G", java_path: null, light_mode: false, console: false, username: "Player" });

  // account
  let accountName = $state("Player");
  let accountStatus = $state("");
  let msProfile = $state<{ name: string; uuid: string; access_token: string } | null>(null);
  let deviceCode = $state<{ device_code: string; user_code: string; verification_uri: string } | null>(null);
  let msLoading = $state(false);

  $effect(() => {
    if (typeof document !== "undefined") {
      document.body.classList.toggle("lite", settings.light_mode);
    }
  });

  onMount(async () => {
    try {
      installed = await invoke<string[]>("get_installed_versions");
      // Sin internet la lista remota falla: no debe impedir cargar el resto.
      remote = await invoke<string[]>("list_versions").catch(() => [] as string[]);
      profiles = await invoke<Profile[]>("get_profiles");
      javas = await invoke<JavaInfo[]>("detect_javas");
      installedMods = await invoke<string[]>("list_installed_mods");
      shaderpacks = await invoke<string[]>("list_shaderpacks");
      resourcepacks = await invoke<string[]>("list_resourcepacks");
      rpOrder = [...resourcepacks];
      rpActive = Object.fromEntries(resourcepacks.map(n => [n, false]));
      servers = await invoke<ServerEntry[]>("get_servers");
      settings = await invoke<AppSettings>("get_settings");
      msProfile = await invoke("ms_get_profile");
      if (profiles.length) loadProfile(profiles[0]);
      else version = installed[0] ?? remote[0] ?? "";
      // check updates (no auto-install)
      try {
        const update = await check();
        if (update) {
          status = "Actualización disponible: " + update.version;
        }
      } catch (e) {
        // ignore update errors
      }
      username = settings.username;
      accountName = settings.username;
    } catch (e) {
      status = String(e);
    }
  });

  function loadProfile(p: Profile) {
    profileName = p.name;
    version = p.version;
    username = p.username;
    ram = p.ram;
    javaPath = p.java_path ?? "";
  }
  function newProfile() {
    profileName = "Nuevo perfil";
    version = installed[0] ?? remote[0] ?? "";
    username = "Player";
    ram = "2G";
    javaPath = "";
  }
  async function saveProfile() {
    const p: Profile = { name: profileName, version, username, ram, java_path: javaPath || null };
    try {
      await invoke("save_profile", { profile: p });
      profiles = await invoke<Profile[]>("get_profiles");
      status = "Perfil guardado";
    } catch (e) { status = String(e); }
  }
  async function deleteProfile() {
    try {
      await invoke("delete_profile", { name: profileName });
      profiles = await invoke<Profile[]>("get_profiles");
      if (profiles.length) loadProfile(profiles[0]);
      else newProfile();
      status = "Perfil eliminado";
    } catch (e) { status = String(e); }
  }
  async function play() {
    if (!version) return;
    loading = true;
    status = "";
    const labels: Record<string, string> = {
      version: "Preparando versión",
      libraries: "Descargando librerías",
      assets: "Descargando recursos",
      starting: "Abriendo Minecraft",
    };
    const unlisten = await listen<{ stage: string; done: number; total: number }>("launch-progress", (e) => {
      const { stage, done, total } = e.payload;
      status = `${labels[stage] ?? stage}…` + (total > 1 ? ` ${Math.round((done / total) * 100)}%` : "");
    });
    try {
      status = await invoke<string>("launch_version", { version, username, ram, javaPath: javaPath || null });
    } catch (e) { status = String(e); }
    finally { unlisten(); loading = false; }
  }

  async function doSearchMods() {
    modStatus = "";
    try {
      modResults = await invoke<ModResult[]>("search_mods", { query: modQuery });
    } catch (e) { modStatus = String(e); }
  }
  async function installMod(m: ModResult) {
    if (!version) { modStatus = "Selecciona una versión primero"; return; }
    loading = true;
    modStatus = "";
    try {
      modStatus = await invoke<string>("install_mod", { slug: m.slug, mcVersion: version, loader: modLoader });
      installedMods = await invoke<string[]>("list_installed_mods");
    } catch (e) { modStatus = String(e); }
    finally { loading = false; }
  }

  async function addServer() {
    serverStatus = "";
    try {
      await invoke("save_server", { name: serverName, address: serverAddr, version: version || null });
      servers = await invoke<ServerEntry[]>("get_servers");
      serverName = "";
      serverAddr = "";
    } catch (e) { serverStatus = String(e); }
  }
  async function removeServer(addr: string) {
    try {
      await invoke("delete_server", { address: addr });
      servers = await invoke<ServerEntry[]>("get_servers");
    } catch (e) { serverStatus = String(e); }
  }
  async function ping(addr: string) {
    serverStatus = "";
    try { serverStatus = await invoke<string>("ping_server", { address: addr }); }
    catch (e) { serverStatus = String(e); }
  }

  async function saveSettings() {
    try {
      await invoke("save_settings", { settings });
      status = "Ajustes guardados";
    } catch (e) { status = String(e); }
  }

  function validateName(name: string): string {
    if (name.length < 3 || name.length > 16) return "Longitud 3-16";
    if (!/^[a-zA-Z0-9_]+$/.test(name)) return "Solo letras, números y _";
    return "Nombre válido";
  }
  async function saveAccount() {
    accountStatus = validateName(accountName);
    if (accountStatus !== "Nombre válido") return;
    settings.username = accountName;
    settings = { ...settings };
    try {
      await invoke("save_settings", { settings });
      username = accountName;
      accountStatus = "Guardado";
    } catch (e) { accountStatus = String(e); }
  }

  async function msLogin() {
    msLoading = true;
    accountStatus = "";
    try {
      deviceCode = await invoke("ms_device_code");
      // poll
      accountStatus = `Visita ${deviceCode.verification_uri} e ingresa ${deviceCode.user_code}`;
      const profile = await invoke("ms_login", { deviceCode: deviceCode.device_code });
      msProfile = profile as any;
      accountName = (profile as any).name;
      await saveAccount();
      deviceCode = null;
      accountStatus = "Login Microsoft OK";
    } catch (e) {
      accountStatus = String(e);
    } finally {
      msLoading = false;
    }
  }
  async function msLogout() {
    try {
      await invoke("ms_logout");
      msProfile = null;
      accountStatus = "Sesión cerrada";
    } catch (e) { accountStatus = String(e); }
  }
</script>

<div id="wrap">
  <div id="app">
    <aside>
      <div class="logo">
        <svg width="34" height="34" viewBox="0 0 34 34" aria-hidden="true"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#C4B5FD"/><stop offset=".5" stop-color="#8B5CF6"/><stop offset="1" stop-color="#C026D3"/></linearGradient></defs><polygon points="17,2 30,12 24,32 10,32 4,12" fill="url(#g)"/><polygon points="17,2 30,12 17,18" fill="rgba(255,255,255,.35)"/></svg>
        Obsidia
      </div>
      <nav>
        {#each [["home","Inicio"],["profiles","Perfiles"],["mods","Mods"],["servers","Servidores"],["settings","Ajustes"],["account","Mi cuenta"]] as [t,label]}
          <button class:on={view===t} onclick={() => view=t}>
            {label}
          </button>
        {/each}
      </nav>
      <button class="acc gl" onclick={() => view="account"} aria-label="Abrir mi cuenta">
        <div class="av"></div>
        <div>
          <div style="font-weight:600">{username}</div>
          <div class="mu">{version || "Sin versión"}</div>
        </div>
      </button>
    </aside>
    <main>
      {#if view === "home"}
    <section class="hero">
      <div class="shard" aria-hidden="true"></div>
      <div class="mu" style="color:#C4B5FD;font-weight:600;letter-spacing:2px;font-size:12px">PERFIL ACTIVO</div>
      <h1 style="font-size:42px;line-height:1.1">{profileName || "Sin perfil"}</h1>
      <div style="color:#C9C7DD">{version || "Sin version"} · {username} · {ram} GB de RAM{javaPath ? " · Java seleccionado" : ""}</div>
      <div style="display:flex;gap:12px;flex-wrap:wrap;margin-top:6px">
        <button class="play" onclick={play} disabled={loading || !version}>{loading ? "Preparando..." : "JUGAR"}</button>
        <button class="sec" onclick={() => view="settings"}>Configurar</button>
      </div>
      {#if status}<p class="status">{status}</p>{/if}
    </section>
    <div class="g3">
      <div class="st gl"><span class="mu">Version</span><b>{version || "-"}</b></div>
      <div class="st gl"><span class="mu">Java</span><b>{javaPath ? "Seleccionado" : "Automatico"}</b></div>
      <div class="st gl"><span class="mu">Mods</span><b>{installedMods.length}</b></div>
    </div>
    <section class="card">
      <label>Jugador <input bind:value={username} /></label>
      <label>Version
        <select bind:value={version}>
          {#each allVersions as v}<option value={v}>{v}</option>{/each}
        </select>
      </label>
      <label>RAM <input bind:value={ram} /></label>
      <label>Java
        <select bind:value={javaPath}>
          <option value="">Automatico</option>
          {#each javas as j}<option value={j.path}>{j.version || j.path}</option>{/each}
        </select>
      </label>
    </section>
  {/if}

  {#if view === "profiles"}
    <div style="display:flex;justify-content:space-between;align-items:center">
      <h1>Perfiles</h1>
      <button class="ib" style="margin:0;padding:0 22px;height:44px" onclick={newProfile}>+ Nuevo perfil</button>
    </div>
    <div class="g3">
      {#each profiles as p}
        <button class="pc gl" class:on={p.name===profileName} onclick={() => loadProfile(p)}>
          <div class="th" style="background:linear-gradient(135deg,#8B5CF6,#C026D3)"></div>
          <div>
            <div style="font-weight:600">{p.name}</div>
            <div class="mu">{p.version} · {p.username}</div>
          </div>
        </button>
      {/each}
    </div>
    <section class="card">
      <label>Nombre <input bind:value={profileName} /></label>
      <label>Jugador <input bind:value={username} /></label>
      <label>Version
        <select bind:value={version}>
          {#each allVersions as v}<option value={v}>{v}</option>{/each}
        </select>
      </label>
      <label>RAM <input bind:value={ram} /></label>
      <label>Java
        <select bind:value={javaPath}>
          <option value="">Automatico</option>
          {#each javas as j}<option value={j.path}>{j.version || j.path}</option>{/each}
        </select>
      </label>
      <div class="row">
        <button type="button" onclick={saveProfile}>Guardar</button>
        <button type="button" class="sec" onclick={deleteProfile}>Eliminar</button>
      </div>
      {#if status}<p class="status">{status}</p>{/if}
    </section>
  {/if}

  {#if view === "mods"}
    <div style="display:flex;justify-content:space-between;align-items:center">
      <h1>Mods</h1>
      <div class="gl" style="padding:9px 16px;color:#C9C7DD;font-size:14px">Perfil: {profileName}</div>
    </div>
    <div class="seg">
      <button class="segbtn" class:on={modSec==="mods"} onclick={() => modSec="mods"}>Mods</button>
      <button class="segbtn" class:on={modSec==="shaders"} onclick={() => modSec="shaders"}>Shaders</button>
      <button class="segbtn" class:on={modSec==="packs"} onclick={() => modSec="packs"}>Resource packs</button>
    </div>

    {#if modSec === "mods"}
      <input class="srch" bind:value={modQuery} placeholder="Buscar en Modrinth…" />
      <div class="chips">
        <button class="chip" class:on={modLoader==="fabric"} onclick={() => modLoader="fabric"}>Fabric</button>
        <button class="chip" class:on={modLoader==="forge"} onclick={() => modLoader="forge"}>Forge</button>
        <button class="chip" onclick={doSearchMods}>Buscar</button>
      </div>
      <h2>Instalados</h2>
      <div class="g3">
        {#each installedMods as m}
          <div class="mc gl">
            <div style="display:flex;align-items:center;gap:12px"><div class="mi" style="background:linear-gradient(135deg,#8B5CF6,#C026D3)"></div><b>{m}</b></div>
            <div class="mu">Instalado</div>
            <div style="display:flex;gap:10px;margin-top:auto">
              <button class="ib in" style="flex:1;margin:0" onclick={async () => { await invoke("delete_file_in", { folder: "mods", name: m }); installedMods = await invoke<string[]>("list_installed_mods"); }}>Eliminar</button>
            </div>
          </div>
        {/each}
      </div>
      <h2>Resultados</h2>
      <div class="g3">
        {#each modResults as m}
          <div class="mc gl">
            <div style="display:flex;align-items:center;gap:12px"><div class="mi" style="background:linear-gradient(135deg,#22D3EE,#8B5CF6)"></div><b>{m.title}</b></div>
            <div class="mu" style="line-height:1.5">{m.description}</div>
            <div style="display:flex;gap:10px;margin-top:auto">
              <button class="ib" style="flex:1;margin:0" onclick={() => installMod(m)} disabled={loading}>Instalar</button>
            </div>
          </div>
        {/each}
      </div>
      {#if modStatus}<p class="status">{modStatus}</p>{/if}
    {/if}

    {#if modSec === "shaders"}
      <div class="gl" style="padding:14px 18px;border-color:rgba(251,191,36,.45)">
        <b>Los shaders necesitan Iris Shaders</b>
        <div class="mu">Instalalo en este perfil para poder activarlos.</div>
      </div>
      <div class="g3">
        {#each shaderpacks as s}
          <div class="mc gl">
            <div style="height:88px;border-radius:12px;background:linear-gradient(135deg,#FBBF24,#F472B6)"></div>
            <div style="display:flex;justify-content:space-between;align-items:center;gap:8px"><b>{s}</b><span class="pf" style="color:#FBBF24">Medio</span></div>
            <div class="mu" style="line-height:1.5">Shader pack instalado.</div>
            <div style="display:flex;gap:10px;margin-top:auto">
              <button class="ib in" style="flex:1;margin:0" onclick={async () => { await invoke("delete_file_in", { folder: "shaderpacks", name: s }); shaderpacks = await invoke<string[]>("list_shaderpacks"); }}>Eliminar</button>
            </div>
          </div>
        {/each}
      </div>
      {#if shaderpacks.length === 0}<p class="mu">No hay shaders instalados. Copia archivos .zip en la carpeta shaderpacks.</p>{/if}
    {/if}

    {#if modSec === "packs"}
      <div class="gl" style="padding:18px 20px;display:flex;flex-direction:column;gap:12px">
        <div style="display:flex;justify-content:space-between;align-items:center;gap:16px">
          <div><b>Orden de prioridad</b><div class="mu">Arriba = mayor prioridad. Lo de arriba se ve sobre lo de abajo.</div></div>
          <button class="ib" style="margin:0;padding:0 20px" onclick={() => { rpOrder = [...rpOrder.filter(n => true)]; }}>Ordenar automatico</button>
        </div>
        {#each rpOrder as n, i}
          <div style="display:flex;align-items:center;gap:12px;padding:8px 12px;border-radius:12px;background:rgba(255,255,255,.04);opacity:{rpActive[n]?1:.6}">
            <span class="mu" style="width:20px">{i+1}</span>
            <b style="flex:1">{n}</b>
            <span class="pf" style="color:#C9C7DD">Pack</span>
            <button class="sw" role="switch" aria-checked={rpActive[n]} onclick={() => rpActive[n]=!rpActive[n]}></button>
            <button class="mb" onclick={() => { if (i>0) { const a=[...rpOrder]; [a[i-1],a[i]]=[a[i],a[i-1]]; rpOrder=a; } }} disabled={i===0}>↑</button>
            <button class="mb" onclick={() => { if (i<rpOrder.length-1) { const a=[...rpOrder]; [a[i],a[i+1]]=[a[i+1],a[i]]; rpOrder=a; } }} disabled={i===rpOrder.length-1}>↓</button>
            <button class="mb" style="width:auto;padding:0 12px;font-size:13px;font-weight:600;color:#F87171" onclick={async () => { await invoke("delete_file_in", { folder: "resourcepacks", name: n }); resourcepacks = await invoke<string[]>("list_resourcepacks"); rpOrder = resourcepacks; }}>Eliminar</button>
          </div>
        {/each}
        {#if rpOrder.length === 0}<p class="mu">No hay resource packs instalados.</p>{/if}
      </div>
      <h2>Disponibles</h2>
      <div class="g3">
        {#each resourcepacks as r}
          <div class="mc gl">
            <div style="height:88px;border-radius:12px;background:repeating-linear-gradient(90deg,rgba(0,0,0,.14) 0 8px,transparent 8px 16px),linear-gradient(135deg,#34D399,#22D3EE)"></div>
            <div style="display:flex;justify-content:space-between;align-items:center;gap:8px"><b>{r}</b><span class="pf" style="color:#C9C7DD">16x</span></div>
            <div class="mu" style="line-height:1.5">Resource pack instalado.</div>
          </div>
        {/each}
      </div>
    {/if}
    {#if modStatus}<p class="status">{modStatus}</p>{/if}
  {/if}

  {#if view === "servers"}
    <div style="display:flex;justify-content:space-between;align-items:center">
      <h1>Servidores</h1>
      <div style="display:flex;gap:10px">
        <button class="sec" style="height:44px" onclick={() => { servers = servers; }}>Actualizar</button>
        <button class="ib" style="margin:0;padding:0 22px;height:44px" onclick={() => { serverName=''; serverAddr=''; }}>+ Anadir servidor</button>
      </div>
    </div>
    <div class="gl" style="padding:18px 20px;display:flex;gap:12px;align-items:center;flex-wrap:wrap">
      <input class="srch" style="flex:1;min-width:180px" bind:value={serverName} placeholder="Nombre del servidor" />
      <input class="srch" style="flex:1;min-width:220px" bind:value={serverAddr} placeholder="Direccion (ej. mi.servidor.net)" />
      <button class="ib" style="margin:0;padding:0 24px;height:48px" onclick={addServer}>Guardar</button>
    </div>
    <div style="display:grid;grid-template-columns:repeat(2,1fr);gap:16px">
      {#each servers as s, i}
        <div class="gl" style="padding:18px;display:flex;flex-direction:column;gap:14px">
          <div style="display:flex;align-items:center;gap:14px">
            <div class="th" style="width:52px;height:52px;background:linear-gradient(135deg,#8B5CF6,#C026D3)"></div>
            <div style="flex:1;min-width:0"><b>{s.name}</b><div class="mu">{s.address}</div></div>
            <span class="pf" style="color:#34D399">Guardado</span>
          </div>
          <div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap">
            <span class="pf" style="color:#C9C7DD">{s.version || "sin version"}</span>
            {#if s.version && version && s.version !== version}
              <span class="pf" style="color:#FCD34D">Version distinta a tu perfil</span>
            {/if}
          </div>
          <div style="display:flex;gap:10px">
            <button class="ib" style="flex:1;margin:0" onclick={() => ping(s.address)}>Ping</button>
            <button class="mb" style="width:auto;height:38px;padding:0 14px;font-size:14px" onclick={() => removeServer(s.address)}>Quitar</button>
          </div>
        </div>
      {/each}
    </div>
    {#if serverStatus}<p class="status">{serverStatus}</p>{/if}
  {/if}

  {#if view === "settings"}
    <h1>Ajustes</h1>
    <div class="row gl">
      <div><div style="font-weight:600">Memoria RAM</div><div class="mu">Cuanta memoria usa Minecraft</div></div>
      <div style="display:flex;align-items:center;gap:14px">
        <input type="range" min="2" max="16" bind:value={settings.ram} oninput={(e) => settings.ram = (e.target as HTMLInputElement).value} />
        <b style="width:60px">{settings.ram} GB</b>
      </div>
    </div>
    <div class="row gl">
      <div><div style="font-weight:600">Version de Java</div><div class="mu">Obsidia la descarga sola si falta</div></div>
      <select bind:value={settings.java_path}>
        <option value={null}>Automatico</option>
        {#each javas as j}<option value={j.path}>{j.version || j.path}</option>{/each}
      </select>
    </div>
    <div class="row gl"><div><div style="font-weight:600">Modo ligero</div><div class="mu">Sin animaciones, ideal para PCs modestas</div></div><button class="sw" role="switch" aria-checked={settings.light_mode} onclick={() => settings.light_mode=!settings.light_mode}></button></div>
    <div class="row gl"><div><div style="font-weight:600">Mostrar consola</div><div class="mu">Ver los logs del juego en vivo</div></div><button class="sw" role="switch" aria-checked={settings.console} onclick={() => settings.console=!settings.console}></button></div>
    <button type="button" onclick={saveSettings}>Guardar</button>
    {#if status}<p class="status">{status}</p>{/if}
  {/if}

  {#if view === "account"}
    <h1>Mi cuenta</h1>
    <div style="display:grid;grid-template-columns:1fr 1.3fr;gap:16px;align-items:start">
      <div class="gl" style="padding:26px;display:flex;flex-direction:column;align-items:center;gap:18px">
        <div class="av" style="width:132px;height:132px;border-radius:24px"></div>
        <div style="width:100%">
          <label>Nombre de jugador
            <input bind:value={accountName} maxlength="16" />
          </label>
          <p class="status">{validateName(accountName)}</p>
        </div>
      </div>
      <div style="display:flex;flex-direction:column;gap:16px">
        <div class="gl" style="padding:20px 22px;display:flex;flex-direction:column;gap:10px">
          <b>Tipo de cuenta</b>
          <div style="color:#6EE7B7">{msProfile ? 'Cuenta Microsoft vinculada' : 'Estas jugando en modo offline'}</div>
          <div class="mu">{msProfile ? 'Podés entrar a servidores premium y usar tu skin oficial.' : 'En modo offline no podés entrar a servidores que verifican la cuenta.'}</div>
          {#if msProfile}
            <p>Conectado: {msProfile.name} ({msProfile.uuid})</p>
            <button class="sec" style="height:44px" onclick={msLogout}>Cerrar sesión</button>
          {:else}
            <button class="ib" style="margin:0;height:44px" onclick={msLogin} disabled={msLoading}>{msLoading ? "Conectando…" : "Iniciar sesión con Microsoft"}</button>
            {#if deviceCode}
              <p class="status">Visita {deviceCode.verification_uri} e ingresa {deviceCode.user_code}</p>
            {/if}
          {/if}
        </div>
        <div class="gl" style="padding:20px 22px;display:flex;flex-direction:column;gap:14px">
          <b>Skin</b>
          <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:12px">
            {#each [0,1,2,3] as i}
              <button class="pc gl" style="flex-direction:column;padding:12px" onclick={() => {}}>
                <div class="th" style="background:linear-gradient(135deg,#8B5CF6,#C026D3)"></div>
                <span class="mu">Skin {i+1}</span>
              </button>
            {/each}
          </div>
          <button class="sec" style="height:44px;opacity:.6;cursor:default" disabled>Subir skin (pronto)</button>
        </div>
        <button type="button" onclick={saveAccount}>Guardar offline</button>
      </div>
    </div>
    {#if accountStatus}<p class="status">{accountStatus}</p>{/if}
  {/if}
</main>
  </div>
</div>

<style>
  :root{--bg:#0A0A12;--tx:#EDEBF7;--mu:#A9A7C0;--gl:rgba(255,255,255,.05);--bd:rgba(255,255,255,.09);--v:#8B5CF6;--c:#22D3EE;color-scheme:dark}
  :global(body){margin:0;height:100%;color:var(--tx);font:15px Inter,system-ui,sans-serif;background:radial-gradient(900px 500px at 75% -10%,rgba(139,92,246,.28),transparent 60%),radial-gradient(700px 400px at 100% 100%,rgba(192,38,211,.16),transparent 60%),var(--bg)}
  #wrap{height:100vh;overflow:auto}
  #app{display:flex;min-width:900px;min-height:100%}
  aside{width:232px;flex:none;padding:28px 18px;display:flex;flex-direction:column;gap:28px;border-right:1px solid var(--bd);background:rgba(255,255,255,.02)}
  .logo{display:flex;align-items:center;gap:12px;padding:0 8px;font:700 22px Sora,sans-serif}
  nav{display:flex;flex-direction:column;gap:6px}
  nav button{display:flex;align-items:center;gap:12px;height:44px;padding:0 14px;border:0;border-radius:12px;background:transparent;color:var(--mu);transition:background .2s,color .2s;text-align:left;width:100%}
  nav button:hover{background:rgba(255,255,255,.05);color:var(--tx)}
  nav button.on{color:#fff;background:rgba(139,92,246,.18);box-shadow:inset 0 0 0 1px rgba(139,92,246,.5)}
  .acc{margin-top:auto;display:flex;align-items:center;gap:12px;padding:12px;border-radius:16px;background:var(--gl);border:1px solid var(--bd);width:100%}
  .av{width:40px;height:40px;border-radius:10px;background:linear-gradient(135deg,#8B5CF6,#22D3EE);flex:none}
  .mu{color:var(--mu);font-size:13px}
  main{flex:1;min-width:0;padding:28px 36px;display:flex;flex-direction:column;gap:22px}
  h1{margin:0;font:700 30px Sora,sans-serif}h2{margin:0;font:600 18px Sora,sans-serif}
  .gl{background:var(--gl);border:1px solid var(--bd);border-radius:18px}
  .hero{position:relative;overflow:hidden;min-height:260px;padding:40px;border-radius:24px;display:flex;flex-direction:column;justify-content:flex-end;gap:16px;border:1px solid rgba(255,255,255,.1);background:linear-gradient(135deg,rgba(139,92,246,.45),rgba(192,38,211,.22) 55%,rgba(10,10,18,.2)),#12101F}
  .shard{position:absolute;right:70px;top:24px;width:200px;height:240px;clip-path:polygon(50% 0,100% 30%,80% 100%,20% 100%,0 30%);background:linear-gradient(160deg,rgba(255,255,255,.28),rgba(139,92,246,.4) 40%,rgba(10,10,18,.7));animation:fl 6s ease-in-out infinite}
  @keyframes fl{50%{transform:translateY(-10px)}}
  .hero>*{position:relative}.hero .shard{position:absolute}
  .play{height:56px;min-width:190px;padding:0 40px;border:0;border-radius:16px;font:700 18px Sora,sans-serif;letter-spacing:1px;color:#06121A;background:linear-gradient(90deg,#67E8F9,#22D3EE);box-shadow:0 8px 32px rgba(34,211,238,.35);transition:transform .15s;align-self:flex-start}
  .play:hover{transform:translateY(-2px)}.play:disabled{opacity:.85;cursor:default;transform:none}
  .sec{height:44px;padding:0 20px;border-radius:12px;background:rgba(255,255,255,.08);border:1px solid rgba(255,255,255,.14);color:var(--tx)}
  .g3{display:grid;grid-template-columns:repeat(3,1fr);gap:16px}
  .st{padding:20px}.st b{display:block;margin-top:6px;font:600 26px Sora,sans-serif}
  .card{background:var(--gl);border:1px solid var(--bd);border-radius:18px;padding:24px;display:flex;flex-direction:column;gap:14px;width:420px}
  .card.wide{width:680px}
  .chips{display:flex;gap:10px;flex-wrap:wrap}
  .chip{height:36px;padding:0 16px;border-radius:18px;color:#C9C7DD;background:var(--gl);border:1px solid var(--bd)}
  .chip.on{color:#fff;background:rgba(139,92,246,.18);border-color:rgba(139,92,246,.55)}
  .srch{height:48px;padding:0 18px;border-radius:14px;background:rgba(255,255,255,.06);border:1px solid rgba(255,255,255,.12);flex:1}
  .mb{width:34px;height:34px;border-radius:9px;border:1px solid var(--bd);background:rgba(255,255,255,.06);font-size:16px}
  .mb:disabled{opacity:.35;cursor:default}
  .row{display:flex;align-items:center;justify-content:space-between;gap:12px}
  label{display:flex;flex-direction:column;gap:6px;font-size:13px;color:var(--mu)}
  input,select{height:40px;padding:0 12px;border-radius:10px;background:#1A1830;border:1px solid var(--bd);color:var(--tx)}
  button{padding:8px 14px;border:0;border-radius:10px;font-weight:600;color:#fff;background:#7C3AED;cursor:pointer;transition:background .2s}
  button:hover{background:#6D28D9}
  button:disabled{opacity:.5;cursor:not-allowed}
  .status{font-size:13px;color:#C026D3;word-break:break-word}
  ul{list-style:none;padding:0;margin:0}
  ul li{padding:10px 0;border-bottom:1px solid rgba(255,255,255,.06);display:flex;align-items:center;justify-content:space-between;gap:10px}
  .results li{align-items:flex-start}
  .ib{height:38px;border:0;border-radius:10px;font-weight:600;color:#fff;background:#7C3AED;padding:0 14px}
  .ib.in{color:#C9C7DD;background:rgba(255,255,255,.08)}
  .sw{width:50px;height:28px;border-radius:14px;border:0;background:rgba(255,255,255,.14);position:relative;transition:background .2s;flex:none}
  .sw::after{content:"";position:absolute;top:3px;left:3px;width:22px;height:22px;border-radius:50%;background:#fff;transition:left .2s}
  .sw[aria-checked=true]{background:#7C3AED}.sw[aria-checked=true]::after{left:25px}
  .seg{display:inline-flex;gap:4px;padding:4px;border-radius:14px;background:var(--gl);border:1px solid var(--bd);align-self:flex-start}
  .segbtn{height:38px;padding:0 24px;border:0;border-radius:10px;background:transparent;color:var(--mu);font-weight:600;transition:background .2s,color .2s}
  .segbtn.on{background:#7C3AED;color:#fff}
</style>