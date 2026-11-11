<script lang="ts">
  // Editor de skins de Obsidia — Svelte 5 (runes). Componente autónomo:
  // no toca Tauri ni Rust; el contenedor decide qué hacer con los eventos.
  import SkinViewer3D from './SkinViewer3D.svelte';
  // Puente Etapa 2: almacén real del lanzador (la biblioteca IndexedDB de
  // `library.ts` queda deshabilitada sin borrar su código).
  import {
    LibraryError,
    deleteSkin,
    loadSkinById,
    loadSkins,
    saveSkin,
    updateThumb,
    type SkinRecord
  } from './launcher-store';
  import { disposeThumbRenderer, renderThumb } from './thumbs';
  import { onMount } from 'svelte';

  type Skin = { id: string; shirt: string; pants: string; hair: string; skin?: string };
  type Tool = 'pencil' | 'eraser' | 'fill' | 'picker';
  type BodyPartId = 'head' | 'body' | 'leftArm' | 'rightArm' | 'leftLeg' | 'rightLeg';

  const defaultSkins: Skin[] = [
    { id: 's1', shirt: '#3f7fbf', pants: '#3a3f8c', hair: '#3b2a20' },
    { id: 's2', shirt: '#c4553f', pants: '#2f2f36', hair: '#6b3b1f' },
    { id: 's3', shirt: '#2f2f36', pants: '#1a1a1e', hair: '#101014' },
    { id: 's4', shirt: '#5aa469', pants: '#4a3b2a', hair: '#d9a441' },
    { id: 's5', shirt: '#8b6cf0', pants: '#3a3f8c', hair: '#e8e9ec' }
  ];

  let {
    skins = defaultSkins,
    title = 'Editor de skins',
    subtitle = 'Editando: Explorador azul',
    onselect,
    onnew,
    onimport,
    onexport,
    onsave,
    oncolor,
    initialSkinId = null
  }: {
    skins?: Skin[];
    title?: string;
    subtitle?: string;
    onselect?: (id: string) => void;
    onnew?: () => void;
    onimport?: () => void;
    onexport?: () => void;
    onsave?: (state: { skinId: string; model: 'classic' | 'slim'; color: string }) => void;
    oncolor?: (hex: string) => void;
    // Etapa 2 (launcher): id del slot activo de `obsidia.skins` a cargar al
    // abrir (su textura y su modelo). Null = arrancar con la skin de prueba.
    initialSkinId?: string | null;
  } = $props();

  // ---- estado ----
  let panelOpen = $state(true);
  let selectedId = $state(skins[0]?.id ?? '');
  let tool = $state<Tool>('pencil');
  let model = $state<'classic' | 'slim'>('classic');
  let baseLayer = $state(true);
  let outerLayer = $state(false);

  // Capa activa para edición (independiente de la visibilidad).
  let activeLayer = $state<'inner' | 'outer'>('inner');

  // Visibilidad anatómica: estado de edición local, no se guarda en la skin.
  let bodyParts = $state<Record<BodyPartId, boolean>>({
    head: true,
    body: true,
    leftArm: true,
    rightArm: true,
    leftLeg: true,
    rightLeg: true,
  });

  // Entrada de la biblioteca cargada actualmente (resaltado de la galería).
  let selectedLibraryId = $state<string | null>(null);
  // Token anti-carreras para cargas desde la galería.
  let loadToken = 0;

  // Archivo seleccionado para importación.
  let importFileInput: HTMLInputElement | null = null;

  // Disponibilidad del historial emitida por el visor 3D.
  let canUndo = $state(false);
  let canRedo = $state(false);
  let historyApi: {
    undo: () => void;
    redo: () => void;
    recenter: () => void;
    importSkin: (imageData: ImageData) => void;
    exportSkin: () => Promise<ImageData>;
  } | null = null;

  function syncHistory(state: { canUndo: boolean; canRedo: boolean }) {
    canUndo = state.canUndo;
    canRedo = state.canRedo;
  }

  // Color en HSV: h 0–360, s 0–1, v 0–1
  let h = $state(210);
  let s = $state(0.42);
  let v = $state(0.75);
  let hexDraft = $state<string | null>(null);

  const selected = $derived(skins.find((k) => k.id === selectedId) ?? skins[0]);

  function hsvToHex(h: number, s: number, v: number): string {
    const f = (n: number) => {
      const k = (n + h / 60) % 6;
      return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
    };
    const to = (x: number) => Math.round(x * 255).toString(16).padStart(2, '0');
    return `#${to(f(5))}${to(f(3))}${to(f(1))}`.toUpperCase();
  }

  function hexToHsv(hex: string): [number, number, number] | null {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
    if (!m) return null;
    const n = parseInt(m[1], 16);
    const r = ((n >> 16) & 255) / 255;
    const g = ((n >> 8) & 255) / 255;
    const b = (n & 255) / 255;
    const max = Math.max(r, g, b);
    const d = max - Math.min(r, g, b);
    let hh = 0;
    if (d !== 0) {
      if (max === r) hh = ((g - b) / d) % 6;
      else if (max === g) hh = (b - r) / d + 2;
      else hh = (r - g) / d + 4;
      hh *= 60;
      if (hh < 0) hh += 360;
    }
    return [hh, max === 0 ? 0 : d / max, max];
  }

  const hex = $derived(hsvToHex(h, s, v));

  // posición del marcador sobre la rueda (porcentajes)
  const marker = $derived.by(() => {
    const rad = (h * Math.PI) / 180;
    return {
      x: 50 + s * 50 * Math.sin(rad),
      y: 50 - s * 50 * Math.cos(rad)
    };
  });

  // ---- rueda de color ----
  let dragging = false;

  function pickFromPointer(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    const rect = el.getBoundingClientRect();
    const dx = e.clientX - (rect.left + rect.width / 2);
    const dy = e.clientY - (rect.top + rect.height / 2);
    const radius = rect.width / 2;
    s = Math.min(Math.hypot(dx, dy) / radius, 1);
    h = ((Math.atan2(dx, -dy) * 180) / Math.PI + 360) % 360;
    hexDraft = null;
    oncolor?.(hex);
  }

  function wheelDown(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    pickFromPointer(e);
  }
  function wheelMove(e: PointerEvent) {
    if (dragging) pickFromPointer(e);
  }
  function wheelUp() {
    dragging = false;
  }

  function onBrightness() {
    hexDraft = null;
    oncolor?.(hex);
  }

  function commitHex() {
    if (hexDraft === null) return;
    const parsed = hexToHsv(hexDraft);
    if (parsed) {
      [h, s, v] = parsed;
      oncolor?.(hex);
    }
    hexDraft = null;
  }

  // Aplica un color leído con el cuentagotas 3D al selector existente.
  // El estado sigue siendo único: HSV manda y el hex se deriva de él.
  function applyPickedColor(picked: string) {
    const parsed = hexToHsv(picked);
    if (!parsed) return;
    [h, s, v] = parsed;
    hexDraft = null;
    oncolor?.(hex);
  }

  function choose(id: string) {
    selectedId = id;
    onselect?.(id);
  }

  // ---- guardado en la biblioteca local ----

  // Convierte el ImageData 64×64 en un PNG Blob independiente: canvas
  // temporal + putImageData + toBlob('image/png'). Sin escalado ni
  // suavizado; conserva todos los canales RGBA.
  async function exportPngBlob(imageData: ImageData): Promise<Blob> {
    const exportCanvas = document.createElement('canvas');
    exportCanvas.width = imageData.width;
    exportCanvas.height = imageData.height;
    const ctx = exportCanvas.getContext('2d');
    if (!ctx) {
      throw new Error('No se pudo crear el canvas de exportación.');
    }
    ctx.putImageData(imageData, 0, 0);
    const blob = await new Promise<Blob | null>((resolve) => {
      exportCanvas.toBlob(resolve, 'image/png');
    });
    if (!blob || blob.size === 0) {
      throw new Error('No se pudo generar el PNG de la skin.');
    }
    return blob;
  }

  function describeSaveError(err: unknown): string {
    if (err instanceof LibraryError) {
      switch (err.code) {
        case 'limit':
          return 'Máximo 4 skins: borrá una para guardar otra.';
        case 'quota':
          return 'Se agotó el espacio disponible para la biblioteca local.';
        case 'unavailable':
          return 'IndexedDB no está disponible en este navegador (¿modo incógnito?).';
        case 'collision':
          return 'Conflicto con un identificador ya guardado. Intentá de nuevo.';
        case 'invalid-blob':
          return 'No se pudo generar el PNG de la skin.';
        case 'corrupt':
          return 'La biblioteca local está dañada.';
        default:
          return 'No se pudo guardar la skin en la biblioteca local.';
      }
    }
    if (err instanceof Error) {
      return `No se pudo guardar la skin: ${err.message}`;
    }
    return 'No se pudo guardar la skin por un error inesperado.';
  }

  async function handleSaveClick(): Promise<void> {
    const raw = prompt('Nombre de la skin:');
    // Cancelar el diálogo no debe bloquear el botón ni mostrar error.
    if (raw === null) return;
    const name = raw.trim();
    if (!name) {
      alert('El nombre de la skin no puede quedar vacío.');
      return;
    }
    const api = historyApi;
    if (!api) {
      alert('El visor aún no está listo. Esperá un momento e intentá de nuevo.');
      return;
    }
    try {
      // Copia independiente del buffer activo.
      const imageData = await api.exportSkin();
      const pngBlob = await exportPngBlob(imageData);
      // Miniatura 3D de la misma textura; si falla, se guarda sin
      // miniatura y el backfill la genera en la próxima carga.
      let thumbBlob: Blob | undefined;
      try {
        thumbBlob = await renderThumb(imageData, model);
      } catch (thumbErr) {
        // La entrada se guarda igual, con placeholder en la galería.
        console.warn('No se pudo generar la miniatura de la skin:', thumbErr);
      }
      // Resuelve cuando la transacción IndexedDB ya confirmó el guardado.
      const record = await saveSkin(name, model, pngBlob, thumbBlob);
      alert(`Skin "${name}" guardada en la biblioteca (id ${record.id.slice(0, 8)}…).`);
      // El id es el del registro recién guardado (no el de la paleta):
      // el lanzador lo resuelve al slot para aplicar la skin al juego.
      onsave?.({ skinId: record.id, model, color: hex });
      // Refresco automático de la galería con la entrada nueva.
      await refreshLibrary();
    } catch (err) {
      alert(describeSaveError(err));
    }
  }

  // ---- importación de PNG ----

  async function handleImportFile(file: File): Promise<void> {
    // Validar tipo MIME
    if (!file.type.startsWith('image/')) {
      alert('El archivo debe ser una imagen PNG.');
      return;
    }

    // Validar magic bytes PNG
    const arrayBuffer = await file.arrayBuffer();
    const header = new Uint8Array(arrayBuffer.slice(0, 8));
    const pngSignature = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    for (let i = 0; i < 8; i++) {
      if (header[i] !== pngSignature[i]) {
        alert('El archivo no es un PNG válido.');
        return;
      }
    }

    // Decodificar imagen
    const img = new Image();
    img.src = URL.createObjectURL(file);
    img.onload = () => {
      URL.revokeObjectURL(img.src);

      if ((img.width === 64 && img.height === 64) || (img.width === 64 && img.height === 32)) {
        const tempCanvas = document.createElement('canvas');
        tempCanvas.width = 64;
        tempCanvas.height = 64;
        const tempCtx = tempCanvas.getContext('2d', { willReadFrequently: true })!;

        if (img.width === 64 && img.height === 32) {
          // --- Legacy 64×32 → 64×64 ---
          tempCtx.fillStyle = 'transparent';
          tempCtx.fillRect(0, 0, 64, 64);
          tempCtx.drawImage(img, 0, 0);

          // Brazo derecho (40-55, 16-31) → brazo izquierdo (32-47, 48-63) con espejo horizontal
          tempCtx.save();
          tempCtx.translate(32 + 15, 48 + 15);
          tempCtx.scale(-1, 1);
          tempCtx.drawImage(img, 40, 16, 16, 16, -8, -8, 16, 16);
          tempCtx.restore();

          // Pierna derecha (0-15, 16-31) → pierna izquierda (16-31, 48-63) con espejo horizontal
          tempCtx.save();
          tempCtx.translate(16 + 15, 48 + 15);
          tempCtx.scale(-1, 1);
          tempCtx.drawImage(img, 0, 16, 16, 16, -8, -8, 16, 16);
          tempCtx.restore();
        } else {
          // 64×64: copiar directamente
          tempCtx.drawImage(img, 0, 0);
        }

        const imageData = tempCtx.getImageData(0, 0, 64, 64);
        if (!historyApi) {
          alert('El visor aún no está listo. Esperá un momento e intentá de nuevo.');
          return;
        }
        // Legacy 64×32: el formato no tiene los bloques izquierdos exteriores
        // completos; fuerza Classic (sincroniza el control y la prop).
        if (img.height === 32) {
          model = 'classic';
        }
        try {
          historyApi.importSkin(imageData);
        } catch (err) {
          alert(`No se pudo importar la imagen: ${(err as Error).message}`);
        }
      } else {
        alert('Dimensiones de imagen no válidas. Se requieren 64×64 o 64×32.');
      }
    };
    img.onerror = () => {
      URL.revokeObjectURL(img.src);
      alert('No se pudo decodificar la imagen.');
    };
  }

  const tools: { id: Tool; label: string; path: string }[] = [
    { id: 'pencil', label: 'Lápiz', path: 'M4 20l4-1 11-11-3-3L5 16z' },
    { id: 'eraser', label: 'Borrador', path: 'M5 15l8-8 6 6-5 5H8z' },
    { id: 'fill', label: 'Relleno', path: 'M5 12l7-7 7 7-7 7zM20 16c1 2 1 3 0 4' },
    { id: 'picker', label: 'Gotero', path: 'M14 5l5 5-9 9H5v-5zM12 7l5 5' }
  ];

  // ---- biblioteca local (galería de skins guardadas) ----

  type GalleryEntry = {
    id: string;
    name: string;
    model: 'classic' | 'slim';
    thumbUrl: string | null;
  };

  let gallery = $state<GalleryEntry[]>([]);
  let libraryError = $state('');
  // URLs de blob activas: se revocan en cada refresco para no filtrar.
  let thumbUrls: string[] = [];

  function releaseThumbUrls(): void {
    for (const url of thumbUrls) URL.revokeObjectURL(url);
    thumbUrls = [];
  }

  function describeLibraryError(err: unknown): string {
    if (err instanceof LibraryError) {
      switch (err.code) {
        case 'unavailable':
          return 'IndexedDB no está disponible acá (¿modo incógnito?): la biblioteca guardada no se puede leer.';
        case 'quota':
          return 'Se agotó el espacio disponible para la biblioteca local.';
        case 'invalid-blob':
          return 'Una miniatura de la biblioteca no es válida.';
        case 'corrupt':
          return 'La biblioteca local está dañada.';
        default:
          return 'La biblioteca local no está disponible ahora mismo.';
      }
    }
    if (err instanceof Error) {
      return `La biblioteca local falló: ${err.message}`;
    }
    return 'La biblioteca local falló por un error inesperado.';
  }

  // Decodifica un Blob PNG 64×64 a ImageData (copia nueva, sin compartir
  // memoria con nada del editor).
  async function blobToImageData(blob: Blob): Promise<ImageData> {
    const bitmap = await createImageBitmap(blob);
    if (bitmap.width !== 64 || bitmap.height !== 64) {
      throw new Error(`Registro con PNG no 64×64 (${bitmap.width}×${bitmap.height}).`);
    }
    const canvas = document.createElement('canvas');
    canvas.width = 64;
    canvas.height = 64;
    const ctx = canvas.getContext('2d')!;
    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
    return ctx.getImageData(0, 0, 64, 64);
  }

  // Entradas viejas (o fallidas) sin thumbBlob: genera y persiste la
  // miniatura. Nunca bloquea la galería: un fallo por registro se informa
  // en consola y ese tile muestra su placeholder.
  async function backfillThumbs(records: SkinRecord[]): Promise<void> {
    for (const rec of records) {
      if (rec.thumbBlob !== undefined) continue;
      try {
        const imageData = await blobToImageData(rec.pngBlob);
        const thumbBlob = await renderThumb(imageData, rec.model);
        await updateThumb(rec.id, thumbBlob);
      } catch (err) {
        // Sin miniatura para esta entrada; el resto de la biblioteca sigue.
        console.warn(`Miniatura no generada para ${rec.id}:`, err);
      }
    }
  }

  async function refreshLibrary(): Promise<void> {
    try {
      let records = (await loadSkins()).sort((a, b) => b.createdAt - a.createdAt);
      libraryError = '';
      await backfillThumbs(records);
      const refreshed = (await loadSkins()).sort((a, b) => b.createdAt - a.createdAt);
      releaseThumbUrls();
      gallery = refreshed.map((r) => ({
        id: r.id,
        name: r.name,
        model: r.model,
        thumbUrl: r.thumbBlob ? URL.createObjectURL(r.thumbBlob) : null,
      }));
      thumbUrls = gallery.map((e) => e.thumbUrl).filter((u): u is string => u !== null);
      // Si la entrada resaltada desapareció, quitar el resaltado.
      if (selectedLibraryId !== null && !gallery.some((e) => e.id === selectedLibraryId)) {
        selectedLibraryId = null;
      }
    } catch (err) {
      // La biblioteca queda vacía y con mensaje; el visor sigue usable.
      releaseThumbUrls();
      gallery = [];
      libraryError = describeLibraryError(err);
    }
  }

  function describeLoadError(err: unknown): string {
    if (err instanceof LibraryError) return describeLibraryError(err);
    if (err instanceof Error) return `No se pudo cargar la skin: ${err.message}`;
    return 'No se pudo cargar la skin por un error inesperado.';
  }

  // Carga una skin guardada en el visor. No modifica el registro de la
  // biblioteca: la edición trabaja sobre una copia en memoria.
  async function loadLibrarySkin(entry: GalleryEntry): Promise<void> {
    const token = ++loadToken;
    try {
      // Registro fresco desde IndexedDB; nunca se escribe nada acá.
      const record = await loadSkinById(entry.id);
      if (token !== loadToken) return; // hay una carga más reciente
      if (!record) {
        alert('Esa skin ya no existe en la biblioteca.');
        await refreshLibrary();
        return;
      }
      // Decodificar y validar 64×64: rechaza PNG corrupto o incompatible.
      const imageData = await blobToImageData(record.pngBlob);
      if (token !== loadToken) return;
      const api = historyApi;
      if (!api) {
        alert('El visor aún no está listo. Esperá un momento e intentá de nuevo.');
        return;
      }
      // Aplica la textura (copia propia) y limpia Undo/Redo.
      api.importSkin(imageData);
      // Restaura el modelo guardado: se propaga al visor por el $effect.
      model = record.model;
      // La selección se actualiza recién después de una carga correcta.
      selectedId = record.id;
      selectedLibraryId = record.id;
      onselect?.(record.id);
    } catch (err) {
      if (token !== loadToken) return; // el fallo es de una carga ya superada
      alert(describeLoadError(err));
    }
  }

  // Elimina una entrada de la biblioteca. La textura cargada en el visor
  // queda intacta: el buffer en memoria es independiente del registro.
  async function handleDeleteEntry(entry: GalleryEntry): Promise<void> {
    const confirmed = confirm(
      `¿Eliminar la skin "${entry.name}" de la biblioteca? Esta acción no se puede deshacer.`
    );
    if (!confirmed) return;

    // Invalida cualquier carga en vuelo: evita que una selección pendiente
    // "resucite" la tarjeta que se está por eliminar.
    loadToken++;

    try {
      // Resuelve sólo cuando IndexedDB completa la transacción.
      const existed = await deleteSkin(entry.id);
      // Una carga que arrancó durante la eliminación ya no puede seleccionar
      // una entrada que dejó de existir: se invalida también al terminar.
      loadToken++;
      if (!existed) {
        alert('Esa skin ya no existía en la biblioteca.');
      }
      // Si era la skin abierta, sólo se limpia la selección de la galería;
      // la textura y el modelo actuales del visor no se tocan.
      if (selectedLibraryId === entry.id) selectedLibraryId = null;
      if (selectedId === entry.id) selectedId = '';
      await refreshLibrary();
    } catch (err) {
      alert(describeLibraryError(err));
      // Refrescar igual para mantener la galería coherente con la BD.
      await refreshLibrary();
    }
  }

  onMount(() => {
    // Tras poblar la galería, cargar la skin del slot activo si el lanzador
    // la indicó: el buffer y el modelo quedan con esa textura y modelo.
    void refreshLibrary().then(() => {
      if (initialSkinId) {
        const e = gallery.find((g) => g.id === initialSkinId);
        if (e) void loadLibrarySkin(e);
      }
    });
    return () => {
      releaseThumbUrls();
      disposeThumbRenderer();
    };
  });
</script>

<!-- Miniaturas reales: imagen 3D renderizada al guardar -->
<!-- Reemplaza al snippet figure3d (silueta SVG genérica) -->

<section class="editor">
  <header class="top">
    <div>
      <h1>{title}</h1>
      <p>{subtitle}</p>
    </div>
    <div class="actions">
      <button class="btn" onclick={() => importFileInput?.click()}>Importar PNG</button>
      <input
        type="file"
        accept="image/png"
        bind:this={importFileInput}
        style="display: none"
        onchange={(e) => {
          const input = e.target as HTMLInputElement;
          const file = input.files?.[0];
          if (file) {
            handleImportFile(file);
          }
          input.value = '';
        }}
      />
      <button class="btn" onclick={() => onexport?.()}>Exportar</button>
      <button
        class="btn primary"
        onclick={() => handleSaveClick()}
      >
        Guardar y usar
      </button>
    </div>
  </header>

  <div class="body">
    <!-- Panel de skins: solo cuadrados, desplegable -->
    <aside class="library" class:closed={!panelOpen}>
      <button
        class="toggle"
        aria-label={panelOpen ? 'Plegar panel de skins' : 'Desplegar panel de skins'}
        aria-expanded={panelOpen}
        onclick={() => (panelOpen = !panelOpen)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>
      </button>

      <div class="tiles" inert={!panelOpen}>
        {#if libraryError}
          <p class="lib-note error">{libraryError}</p>
        {/if}
        {#if !libraryError && gallery.length === 0}
          <p class="lib-note">Aún no hay skins guardadas. Importá un PNG y usá «Guardar y usar».</p>
        {/if}
        {#each gallery as e, i (e.id)}
          <div class="tile thumb" class:active={selectedLibraryId === e.id}>
            <button
              type="button"
              class="pick"
              title={`${e.name} · ${e.model === 'slim' ? 'Slim' : 'Classic'}`}
              aria-label={`Cargar skin: ${e.name} (${e.model === 'slim' ? 'Slim' : 'Classic'})`}
              aria-pressed={selectedLibraryId === e.id}
              onclick={() => loadLibrarySkin(e)}
            >
              <span class="fig">
                {#if e.thumbUrl}
                  <img src={e.thumbUrl} alt="" draggable="false" />
                {:else}
                  <span class="nothumb">{e.name.charAt(0).toUpperCase() || '?'}</span>
                {/if}
                <b class="badge">{e.model === 'slim' ? 'S' : 'C'}</b>
              </span>
              <span class="caption">{e.name}</span>
            </button>
            <button
              type="button"
              class="del"
              aria-label={`Eliminar skin: ${e.name}`}
              title={`Eliminar «${e.name}»`}
              onclick={() => handleDeleteEntry(e)}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M4 7h16M9 7V4h6v3M7 7l1 12h8l1-12" />
              </svg>
            </button>
          </div>
        {/each}
      </div>
    </aside>

    <!-- Visor con paneles flotantes -->
    <div class="stage-wrap">
      <div class="stage">
        <SkinViewer3D
          color={hex}
          tool={tool}
          onpick={applyPickedColor}
          onhistory={syncHistory}
          onready={(api) => (historyApi = api)}
          baseLayer={baseLayer}
          outerLayer={outerLayer}
          activeLayer={activeLayer}
          model={model}
          bodyParts={bodyParts}
        />

        <!-- Herramientas -->
        <div class="glass toolbar" role="toolbar" aria-label="Herramientas">
          {#each tools as t (t.id)}
            <button
              class="tool"
              class:active={tool === t.id}
              aria-label={t.label}
              aria-pressed={tool === t.id}
              onclick={() => (tool = t.id)}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d={t.path} /></svg>
            </button>
          {/each}

          <button
            class="tool"
            aria-label="Deshacer"
            disabled={!canUndo}
            onclick={() => historyApi?.undo()}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M9 14L4 9l5-5M4 9h9a6 6 0 0 1 0 12h-2" />
            </svg>
          </button>
          <button
            class="tool"
            aria-label="Rehacer"
            disabled={!canRedo}
            onclick={() => historyApi?.redo()}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M15 14l5-5-5-5M20 9h-9a6 6 0 0 0 0 12h2" />
            </svg>
          </button>
        </div>

        <!-- Paleta, modelo y capas -->
        <div class="side">
          <div class="glass options">
          <div class="group">
            <h2>Paleta</h2>
            <div class="picker">
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="wheel"
                aria-label="Rueda de color"
                onpointerdown={wheelDown}
                onpointermove={wheelMove}
                onpointerup={wheelUp}
                onpointercancel={wheelUp}
              >
                <div class="wheel-dim" style:opacity={1 - v}></div>
                <div
                  class="marker"
                  style:left="{marker.x}%"
                  style:top="{marker.y}%"
                  style:background={hex}
                ></div>
              </div>

              <label class="bright">
                <span class="label">Brillo</span>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.01"
                  bind:value={v}
                  oninput={onBrightness}
                />
              </label>

              <div class="hexrow">
                <span class="chip" style:background={hex}></span>
                <input
                  class="hex"
                  type="text"
                  spellcheck="false"
                  maxlength="7"
                  aria-label="Código hex"
                  value={hexDraft ?? hex}
                  oninput={(e) => (hexDraft = e.currentTarget.value)}
                  onblur={commitHex}
                  onkeydown={(e) => e.key === 'Enter' && commitHex()}
                />
              </div>
            </div>
          </div>

          <div class="group">
            <h2>Modelo</h2>
            <div class="seg">
              <button class:active={model === 'classic'} onclick={() => (model = 'classic')}>
                Classic
              </button>
              <button class:active={model === 'slim'} onclick={() => (model = 'slim')}>
                Slim
              </button>
            </div>
          </div>

          <div class="group">
            <h2>Capas</h2>
            <label class="layer"><span>Capa base</span><input type="checkbox" bind:checked={baseLayer} /></label>
            <label class="layer"><span>Capa exterior</span><input type="checkbox" bind:checked={outerLayer} /></label>
          </div>

          <div class="group">
            <h2>Capa de edición</h2>
            <div class="seg">
              <button class:active={activeLayer === 'inner'} onclick={() => (activeLayer = 'inner')}>
                Interior
              </button>
              <button class:active={activeLayer === 'outer'} onclick={() => (activeLayer = 'outer')}>
                Exterior
              </button>
            </div>
          </div>
          </div>

          <div class="group parts-group">
            <h2>Partes del cuerpo</h2>
            <div class="body-map" role="group" aria-label="Selector de partes del cuerpo">
              <button
                type="button"
                class="part head"
                class:hidden={!bodyParts.head}
                aria-label="Cabeza visible"
                aria-pressed={bodyParts.head}
                title="Cabeza"
                onclick={() => (bodyParts.head = !bodyParts.head)}
              ></button>
              <!-- Desde el frente, el brazo del lado izquierdo de la pantalla
                   es el derecho anatómico del personaje (skinview3d). -->
              <button
                type="button"
                class="part left-arm"
                class:hidden={!bodyParts.rightArm}
                aria-label="Brazo izquierdo visible"
                aria-pressed={bodyParts.rightArm}
                title="Brazo izquierdo"
                onclick={() => (bodyParts.rightArm = !bodyParts.rightArm)}
              ></button>
              <button
                type="button"
                class="part body"
                class:hidden={!bodyParts.body}
                aria-label="Torso visible"
                aria-pressed={bodyParts.body}
                title="Torso"
                onclick={() => (bodyParts.body = !bodyParts.body)}
              ></button>
              <button
                type="button"
                class="part right-arm"
                class:hidden={!bodyParts.leftArm}
                aria-label="Brazo derecho visible"
                aria-pressed={bodyParts.leftArm}
                title="Brazo derecho"
                onclick={() => (bodyParts.leftArm = !bodyParts.leftArm)}
              ></button>
              <!-- Ídem para piernas: izquierda visual = derecha anatómica. -->
              <button
                type="button"
                class="part left-leg"
                class:hidden={!bodyParts.rightLeg}
                aria-label="Pierna izquierda visible"
                aria-pressed={bodyParts.rightLeg}
                title="Pierna izquierda"
                onclick={() => (bodyParts.rightLeg = !bodyParts.rightLeg)}
              ></button>
              <button
                type="button"
                class="part right-leg"
                class:hidden={!bodyParts.leftLeg}
                aria-label="Pierna derecha visible"
                aria-pressed={bodyParts.leftLeg}
                title="Pierna derecha"
                onclick={() => (bodyParts.leftLeg = !bodyParts.leftLeg)}
              ></button>
            </div>
          </div>
        </div>

        <!-- Recentrar cámara -->
        <button
          type="button"
          class="glass recenter"
          aria-label="Recentrar vista"
          title="Recentrar"
          onclick={() => historyApi?.recenter()}
        >
          Recentrar
        </button>

        <!-- Aviso inferior -->
        <div class="glass hint">Arrastrá con clic derecho para rotar</div>
      </div>
    </div>
  </div>
</section>

<style>
  .editor {
    --bg: #0e0f12;
    --stage: #121418;
    --panel: #15171b;
    --line: #24272d;
    --line-2: #2c2f36;
    --text: #e8e9ec;
    --muted: #9a9ea8;
    --accent: #8b6cf0;
    --glass-bg: rgba(24, 26, 32, 0.2);
    --glass-line: rgba(255, 255, 255, 0.12);
    --glass-blur: 10.4px;

    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background: var(--bg);
    color: var(--text);
    font-family: Inter, system-ui, sans-serif;
  }

  /* ---------- encabezado ---------- */
  .top {
    flex: none;
    height: 76px;
    box-sizing: border-box;
    padding: 0 28px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--line);
  }
  h1 {
    margin: 0;
    font-family: Sora, Inter, sans-serif;
    font-size: 22px;
    font-weight: 600;
  }
  .top p {
    margin: 2px 0 0;
    font-size: 13px;
    color: var(--muted);
  }
  .actions {
    display: flex;
    gap: 10px;
  }
  .btn {
    height: 40px;
    padding: 0 16px;
    border-radius: 8px;
    border: 1px solid var(--line-2);
    background: var(--panel);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    cursor: pointer;
  }
  .btn.primary {
    padding: 0 18px;
    border: 0;
    background: var(--accent);
    color: var(--bg);
    font-weight: 600;
  }

  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  /* ---------- panel de skins ---------- */
  .library {
    flex: none;
    width: 194px;
    box-sizing: border-box;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow: hidden;
    border-right: 1px solid var(--line);
    transition: width 0.3s ease;
  }
  .library.closed {
    width: 68px;
  }
  .toggle {
    align-self: flex-end;
    flex: none;
    width: 36px;
    height: 36px;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 8px;
    border: 1px solid var(--line-2);
    background: var(--panel);
    cursor: pointer;
  }
  .toggle svg {
    width: 18px;
    height: 18px;
    fill: none;
    stroke: var(--text);
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 0.3s ease;
  }
  .library.closed .toggle svg {
    transform: rotate(180deg);
  }
  .tiles {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    width: 162px;
    display: grid;
    grid-template-columns: repeat(2, 76px);
    justify-content: start;
    gap: 10px;
    transition: opacity 0.2s ease;
  }
  .library.closed .tiles {
    opacity: 0;
    pointer-events: none;
  }
  .tile {
    width: 76px;
    height: 76px;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--panel);
    cursor: pointer;
  }
  .tile.thumb {
    position: relative;
    margin: 0;
    padding: 0;
    box-sizing: border-box;
    overflow: hidden;
    font: inherit;
    cursor: default;
  }
  .tile.thumb.active {
    border-color: var(--accent);
  }
  /* Botón de carga: ocupa toda la tarjeta. */
  .thumb .pick {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    padding: 4px;
    display: grid;
    grid-template-rows: 1fr 12px;
    row-gap: 2px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  /* Acción de eliminar: independiente del clic de selección. */
  .thumb .del {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .thumb .del:hover,
  .thumb .del:focus-visible {
    color: #ff9a9a;
    background: rgba(0, 0, 0, 0.35);
  }
  .thumb .del svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .thumb .fig {
    position: relative;
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
  }
  .thumb img {
    max-width: 100%;
    max-height: 100%;
    width: auto;
    height: auto;
    object-fit: contain;
    display: block;
    image-rendering: auto;
  }
  .thumb .nothumb {
    font-size: 18px;
    font-weight: 600;
    color: var(--muted);
  }
  .thumb .badge {
    position: absolute;
    top: 1px;
    right: 3px;
    font-size: 8px;
    font-weight: 700;
    color: var(--muted);
  }
  .thumb .caption {
    font-size: 9px;
    line-height: 12px;
    color: var(--muted);
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lib-note {
    grid-column: 1 / -1;
    margin: 0;
    font-size: 10px;
    line-height: 1.45;
    color: var(--muted);
  }
  .lib-note.error {
    color: #ff9a9a;
  }

  /* ---------- visor ---------- */
  .stage-wrap {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    box-sizing: border-box;
    padding: 24px;
  }
  .stage {
    position: relative;
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--stage);
  }
  /* ---------- paneles flotantes (cristal) ---------- */
  .glass {
    position: absolute;
    box-sizing: border-box;
    border: 1px solid var(--glass-line);
    border-radius: 12px;
    background: var(--glass-bg);
    backdrop-filter: blur(var(--glass-blur));
    -webkit-backdrop-filter: blur(var(--glass-blur));
  }
  .toolbar {
    left: 16px;
    top: 16px;
    max-height: calc(100% - 32px);
    overflow-y: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .tool {
    flex: none;
    width: 44px;
    height: 44px;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    background: rgba(255, 255, 255, 0.05);
    cursor: pointer;
  }
  .tool.active {
    border-color: var(--accent);
    background: rgba(255, 255, 255, 0.12);
  }
  .tool:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .tool svg {
    width: 20px;
    height: 20px;
    fill: none;
    stroke: var(--text);
    stroke-width: 1.8;
    stroke-linejoin: round;
  }

  /* Columna derecha: tarjeta de opciones y selector fuera de ella, debajo. */
  .side {
    position: absolute;
    right: 16px;
    top: 16px;
    width: 236px;
    /* En stages angostos se encoge sin tapar la barra de herramientas. */
    max-width: calc(100% - 94px);
    max-height: calc(100% - 32px);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .options {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h2,
  .label {
    margin: 0 0 2px;
    font-size: 12px;
    font-weight: 0;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .picker {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
  }
  .wheel {
    position: relative;
    width: 160px;
    max-width: 100%;
    aspect-ratio: 1 / 1;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.14);
    background:
      radial-gradient(circle, #fff 0%, rgba(255, 255, 255, 0) 70%),
      conic-gradient(#f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00);
    cursor: crosshair;
    touch-action: none;
  }
  .wheel-dim {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: #000;
    pointer-events: none;
  }
  .marker {
    position: absolute;
    width: 18px;
    height: 18px;
    box-sizing: border-box;
    border-radius: 50%;
    border: 2px solid #fff;
    box-shadow: 0 0 0 1px var(--bg);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }
  .bright {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bright input {
    width: 100%;
    margin: 0;
    accent-color: var(--accent);
  }
  .hexrow {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .chip {
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.14);
  }
  .hex {
    flex: 1;
    min-width: 0;
    height: 32px;
    box-sizing: border-box;
    padding: 0 10px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.28);
    color: var(--text);
    font: inherit;
    font-size: 13px;
  }

  .seg {
    display: flex;
    gap: 8px;
  }
  .seg button {
    flex: 1;
    height: 36px;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    background: rgba(255, 255, 255, 0.05);
    color: var(--muted);
    font: inherit;
    font-size: 14px;
    cursor: pointer;
  }
  .seg button.active {
    border-color: var(--accent);
    background: rgba(255, 255, 255, 0.12);
    color: var(--text);
  }
  .layer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 10px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    font-size: 14px;
  }
  .layer input {
    accent-color: var(--accent);
  }

  /* Esquema de partes del cuerpo: segmentos interactivos con tooltips.
     Sin tarjeta propia: vive fuera de .options, debajo de ella.
     Proporciones Minecraft (ancho 4-8-4) en contenedor 4:3. */
  .body-map {
    position: relative;
    width: min(176px, 100%);
    aspect-ratio: 4 / 3;
    margin: 0 auto;
    box-sizing: border-box;
  }
  .body-map .part {
    position: absolute;
    box-sizing: border-box;
    padding: 0;
    border: 1px solid var(--accent);
    border-radius: 3px;
    background: rgba(139, 108, 240, 0.3);
    cursor: pointer;
  }
  /* Silueta Minecraft compacta y centrada: brazo 4u, torso/cabeza 8u,
     pierna 4u. Ocupa el 22-78% del ancho: más flaca hacia el centro. */
  .body-map .part.head {
    left: 39%;
    top: 0%;
    width: 22%;
    height: 26%;
  }
  .body-map .part.left-arm {
    left: 26%;
    top: 28%;
    width: 11%;
    height: 38%;
  }
  .body-map .part.body {
    left: 39%;
    top: 28%;
    width: 22%;
    height: 38%;
  }
  .body-map .part.right-arm {
    left: 63%;
    top: 28%;
    width: 11%;
    height: 38%;
  }
  .body-map .part.left-leg {
    left: 39%;
    top: 68%;
    width: 11%;
    height: 32%;
  }
  .body-map .part.right-leg {
    left: 50%;
    top: 68%;
    width: 11%;
    height: 32%;
  }
  .body-map .part.hidden {
    border-style: dashed;
    border-color: rgba(255, 255, 255, 0.3);
    background: repeating-linear-gradient(
      45deg,
      rgba(255, 255, 255, 0.03) 0 5px,
      rgba(139, 108, 240, 0.08) 5px 6px
    );
    opacity: 0.65;
  }
  .body-map .part:hover,
  .body-map .part:focus-visible {
    border-color: var(--accent);
    background: rgba(139, 108, 240, 0.42);
  }
  .body-map .part.hidden:hover,
  .body-map .part.hidden:focus-visible {
    background: rgba(139, 108, 240, 0.16);
  }

  .hint {
    left: 50%;
    bottom: 16px;
    transform: translateX(-50%);
    max-width: calc(100% - 32px);
    padding: 10px 16px;
    box-sizing: border-box;
    font-size: 13px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    /* Es solo informativo: no debe bloquear pintar ni recentrar. */
    pointer-events: none;
  }
  .recenter {
    left: 16px;
    bottom: 16px;
    z-index: 1;
    padding: 10px 16px;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    cursor: pointer;
  }
  .recenter:hover {
    background: rgba(255, 255, 255, 0.08);
  }

  @media (prefers-reduced-motion: reduce) {
    .library,
    .tiles,
    .toggle svg {
      transition: none;
    }
  }
</style>
