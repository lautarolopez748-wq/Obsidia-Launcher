<script lang="ts">
  // Visor 3D de skins para el prototipo de Obsidia Custom.
  // Ocupa todo el escenario (.stage) y permite pintar con el lápiz sobre
  // el personaje mediante raycasting contra las mallas de skinview3d.
  // No guarda objetos Three.js en estados reactivos de Svelte.
  import * as skinview3d from 'skinview3d';
  import {
    CanvasTexture,
    MOUSE,
    NearestFilter,
    Raycaster,
    Vector2,
    Vector3,
    type BufferGeometry,
    type Intersection,
    type Mesh
  } from 'three';
  import {
    buildOuterGeometries,
    disposeOuterGeometries,
    outerKey,
    type FaceRect,
    type OuterGeometryMap
  } from './lib/outerGeometry';
  import { onMount } from 'svelte';

  type BodyPartId = 'head' | 'body' | 'leftArm' | 'rightArm' | 'leftLeg' | 'rightLeg';

  let {
    color = '#6F97BF',
    tool = 'pencil',
    onpick,
    onhistory,
    onready,
    baseLayer = true,
    outerLayer = false,
    activeLayer = 'inner',
    model = 'classic',
    bodyParts = { head: true, body: true, leftArm: true, rightArm: true, leftLeg: true, rightLeg: true }
  }: {
    color?: string;
    tool?: string;
    onpick?: (hex: string) => void;
    onhistory?: (state: { canUndo: boolean; canRedo: boolean }) => void;
    onready?: (api: {
      undo: () => void;
      redo: () => void;
      recenter: () => void;
      importSkin: (imageData: ImageData) => void;
      exportSkin: () => Promise<ImageData>;
    }) => void;
    baseLayer?: boolean;
    outerLayer?: boolean;
    activeLayer?: 'inner' | 'outer';
    model?: 'classic' | 'slim';
    bodyParts?: Record<BodyPartId, boolean>;
  } = $props();

  let containerEl: HTMLDivElement | null = null;
  let canvasEl: HTMLCanvasElement | null = null;
  let viewerInstance: skinview3d.SkinViewer | null = null;

  // Visibilidad anatómica independiente de las capas de skin. Solo cambia
  // el flag visible del BodyPart: geometría, textura y mallas se conservan.
  function applyBodyPartVisibility() {
    const skin = viewerInstance?.playerObject.skin;
    if (!skin) return;
    skin.head.visible = bodyParts.head;
    skin.body.visible = bodyParts.body;
    skin.leftArm.visible = bodyParts.leftArm;
    skin.rightArm.visible = bodyParts.rightArm;
    skin.leftLeg.visible = bodyParts.leftLeg;
    skin.rightLeg.visible = bodyParts.rightLeg;
    // Ocultar una parte también la saca del recorte de unión de la capa
    // exterior (las mangas no quedan abiertas al ocultar el torso).
    regenerateOuter();
  }

  // Geometría de la superficie de unión de las cajas exteriores.
  // Caché de una sola entrada por outerKey; las geometrías originales de
  // skinview3d nunca se liberan (solo las creadas por buildOuterGeometries).
  const OUTER_PARTS: BodyPartId[] = ['head', 'body', 'leftArm', 'rightArm', 'leftLeg', 'rightLeg'];
  let currentOuterMap: OuterGeometryMap | null = null;
  let currentOuterKey = '';
  let originalOuterGeometries: Map<BodyPartId, BufferGeometry> | null = null;
  let regenerating = false;

  function computeOuterVisible(): Record<BodyPartId, boolean> {
    return {
      head: outerLayer && bodyParts.head,
      body: outerLayer && bodyParts.body,
      leftArm: outerLayer && bodyParts.leftArm,
      rightArm: outerLayer && bodyParts.rightArm,
      leftLeg: outerLayer && bodyParts.leftLeg,
      rightLeg: outerLayer && bodyParts.rightLeg,
    };
  }

  function regenerateOuter() {
    const viewer = viewerInstance;
    if (!viewer || regenerating) return;
    const skin = viewer.playerObject.skin;
    if (!originalOuterGeometries) {
      originalOuterGeometries = new Map();
      for (const p of OUTER_PARTS) {
        originalOuterGeometries.set(p, skin[p].outerLayer.geometry);
      }
    }
    const visibleNow = computeOuterVisible();
    const key = outerKey(skin.modelType, visibleNow);

    if (currentOuterMap && key === currentOuterKey) {
      // Re-asignar la unión en caché y re-afirmar la escala: una re-asignación
      // de modelType con el mismo valor hace que los listeners de skinview3d
      // reapliquen la escala de los brazos sobre la geometría horneada.
      for (const p of OUTER_PARTS) {
        const mesh = skin[p].outerLayer;
        const geo = currentOuterMap.get(p);
        if (geo) mesh.geometry = geo;
        mesh.scale.set(1, 1, 1);
      }
      skin.updateWorldMatrix(true, true);
      return;
    }

    regenerating = true;
    try {
      // Restaurar originales ANTES de medir: buildOuterGeometries debe leer
      // siempre la geometría de skinview3d con sus escalas correctas, nunca
      // la salida de una regeneración anterior.
      for (const p of OUTER_PARTS) {
        skin[p].outerLayer.geometry = originalOuterGeometries.get(p)!;
      }
      // Re-disparar el setter (mismo valor) para que skinview3d reaplique
      // las escalas/pivotes del modelo actual; el listener propio re-entra
      // aquí y retorna por el guard. Se lee el modo desde el skin (no del
      // prop) para respetar casos como el forzado a Classic de la
      // importación 64×32.
      skin.modelType = skin.modelType;
      if (!(currentOuterMap && key === currentOuterKey)) {
        const next = buildOuterGeometries(skin, visibleNow);
        if (currentOuterMap) disposeOuterGeometries(currentOuterMap);
        currentOuterMap = next;
        currentOuterKey = key;
        for (const p of OUTER_PARTS) {
          const mesh = skin[p].outerLayer;
          const geo = next.get(p);
          if (geo) mesh.geometry = geo;
        }
      }
    // La geometría devuelta ya trae la escala horneada: en brazos cabe
    // resetearla ahora (sus listeners de modelType la reaplican antes).
    for (const p of OUTER_PARTS) {
      skin[p].outerLayer.scale.set(1, 1, 1);
    }
    skin.updateWorldMatrix(true, true);
    } finally {
      regenerating = false;
    }
  }

  // Color de fondo del escenario (.stage) para que el canvas se integre
  // sin el rectángulo negro independiente. skinview3d 3.4.2 crea su
  // WebGLRenderer con alpha: false, así que un canvas transparente real
  // no es posible sin modificar la librería.
  const STAGE_BG = '#121418';

  const SKIN_SIZE = 64;
  // Entradas máximas del historial (estados previos a cada operación).
  const HISTORY_LIMIT = 64;
  // Distancia máxima (en píxeles de textura) para interpolar un trazo.
  // Evita conectar dos islas UV distintas, por ejemplo en una costura.
  const MAX_STROKE_JUMP = 10;

  // Dibuja la textura de prueba 64×64 en el canvas interno (no visible).
  function drawTestSkin(ctx: CanvasRenderingContext2D) {
    ctx.clearRect(0, 0, SKIN_SIZE, SKIN_SIZE);

    // Cabeza (8×8 en 8,8)
    ctx.fillStyle = '#ff5555';
    ctx.fillRect(8, 8, 8, 8);
    // Sombrero / capa externa cabeza
    ctx.fillStyle = '#ff8888';
    ctx.fillRect(32, 0, 32, 16);

    // Cuerpo (8×12 en 16,20)
    ctx.fillStyle = '#55ff55';
    ctx.fillRect(16, 20, 8, 12);
    // Chaqueta / capa externa cuerpo
    ctx.fillStyle = '#88ff88';
    ctx.fillRect(16, 32, 24, 16);

    // Brazos (4×12)
    ctx.fillStyle = '#5555ff';
    ctx.fillRect(40, 16, 4, 12);
    ctx.fillRect(0, 16, 4, 12);
    ctx.fillStyle = '#8888ff';
    ctx.fillRect(40, 32, 16, 16); // Manga derecha exterior
    ctx.fillStyle = '#ff88ff';
    ctx.fillRect(48, 48, 16, 16); // Manga izquierda exterior

    // Piernas (4×12)
    ctx.fillStyle = '#5555ff';
    ctx.fillRect(16, 48, 4, 12);
    ctx.fillRect(0, 48, 4, 12);
    ctx.fillStyle = '#ffff88';
    ctx.fillRect(0, 32, 16, 16); // Pantalón derecho exterior
    ctx.fillStyle = '#88ffff';
    ctx.fillRect(0, 48, 16, 16); // Pantalón izquierdo exterior

    // Ojos
    ctx.fillStyle = '#000000';
    ctx.fillRect(10, 10, 1, 1);
    ctx.fillRect(14, 10, 1, 1);
  }

  onMount(() => {
    const container = containerEl;
    const canvas = canvasEl;
    if (!container || !canvas) return undefined;

    const viewer = new skinview3d.SkinViewer({
      canvas,
      width: Math.max(container.clientWidth, 1),
      height: Math.max(container.clientHeight, 1),
    });
    viewerInstance = viewer;

    // Sincronizar dimensiones internas del canvas con el contenedor para que
    // getBoundingClientRect() y el renderer de Three.js trabajen con el mismo
    // sistema de coordenadas. Sin esto, el canvas DOM puede tener sus
    // dimensiones internas por defecto (300x150) mientras se renderiza
    // estirado al tamaño del contenedor, causando un desfasado entre el
    // cursor y el píxel pintado.
    function syncCanvasSize() {
      if (!containerEl || !canvasEl) return;
      // Usar las dimensiones renderizadas del canvas (CSS pixels), no las del
      // contenedor que incluye el padding del panel. Esto asegura que el
      // canvas interno y el renderizado coincidan exactamente, evitando
      // desfasados entre el cursor y el píxel pintado.
      const width = Math.max(canvasEl.clientWidth, 1);
      const height = Math.max(canvasEl.clientHeight, 1);
      canvasEl.width = width;
      canvasEl.height = height;
      viewer.setSize(width, height);
    }
    syncCanvasSize();

    // Corrige la posición de las piernas para evitar Z-fighting en el centro (X=0).
    // skinview3d las inicializa en x = ±1.9, lo que las hace solaparse.
    const skin = viewer.playerObject.skin;
    skin.rightLeg.position.x = -2.0;
    skin.leftLeg.position.x = 2.0;

    // Aplica el modelo inicial (classic → default, slim → slim).
    viewer.playerObject.skin.modelType = model === 'slim' ? 'slim' : 'default';

    // Unión de cajas exteriores: se regenera en cada cambio de modelo
    // (modelListeners se invoca después de los propios de skinview3d, que
    // reaplican la escala de los brazos; regenerateOuter la hornea en la
    // geometría y deja el mesh con scale 1). La llamada inicial cubre el
    // caso de la asignación de arriba, que ocurre antes del push.
    skin.modelListeners.push(regenerateOuter);
    regenerateOuter();

    viewer.animation = null;
    viewer.autoRotate = false;
    // El zoom ajusta la distancia de cámara automáticamente para que el
    // cuerpo completo quede visible sin estirarse.
    viewer.zoom = 0.8;
    // Rotación solo con clic derecho. El clic izquierdo se usa para pintar.
    viewer.controls.mouseButtons = {
      LEFT: null,
      MIDDLE: MOUSE.DOLLY,
      RIGHT: MOUSE.ROTATE,
    };
    // Pose inicial de la cámara (después de fijar el zoom): referencia para
    // recentrar. Se captura una sola vez; no se modifica al cargar skins.
    const initialCameraPosition = viewer.camera.position.toArray() as [number, number, number];
    const initialControlsTarget = viewer.controls.target.toArray() as [number, number, number];
    const initialZoom = viewer.zoom;
    viewer.globalLight.intensity = 2.8;
    viewer.cameraLight.intensity = 2;
    // Fondo transparente en la escena; el clear color iguala el del stage.
    viewer.background = null;
    viewer.renderer.setClearColor(STAGE_BG, 1);

    // ---- canvas interno 64×64: fuente de verdad de la textura ----
    // Permanece oculto y no se agrega al DOM.
    const skinCanvas = document.createElement('canvas');
    skinCanvas.width = SKIN_SIZE;
    skinCanvas.height = SKIN_SIZE;
    const skinCtx = skinCanvas.getContext('2d', { willReadFrequently: true })!;
    drawTestSkin(skinCtx);

    const skinImage = skinCtx.getImageData(0, 0, SKIN_SIZE, SKIN_SIZE);

    // Se reutiliza una sola textura durante toda la vida del visor.
    const skinTexture = new CanvasTexture(skinCanvas);
    skinTexture.magFilter = NearestFilter;
    skinTexture.minFilter = NearestFilter;
    skinTexture.generateMipmaps = false;
    viewer.playerObject.skin.map = skinTexture;
    viewer.playerObject.skin.visible = true;
    viewer.playerObject.skin.modelType = 'default';

    // ---- raycasting ----
    const raycaster = new Raycaster();
    const pointerNdc = new Vector2();

    const skinParts = [
      viewer.playerObject.skin.head,
      viewer.playerObject.skin.body,
      viewer.playerObject.skin.rightArm,
      viewer.playerObject.skin.leftArm,
      viewer.playerObject.skin.rightLeg,
      viewer.playerObject.skin.leftLeg,
    ];
    // Meshes separados por capa para raycasting independiente.
    const innerMeshes = skinParts.map((part) => part.innerLayer) as Mesh[];
    const outerMeshesArr = skinParts.map((part) => part.outerLayer) as Mesh[];
    // Conjunto de mallas exteriores (mantenido para identificación).
    const outerMeshes = new Set<Mesh>(
      skinParts.map((part) => part.outerLayer as Mesh)
    );
    // Malla → parte anatómica, para excluir las partes ocultas del raycasting.
    const bodyPartIds: BodyPartId[] = ['head', 'body', 'rightArm', 'leftArm', 'rightLeg', 'leftLeg'];
    const meshPart = new Map<Mesh, BodyPartId>();
    skinParts.forEach((part, index) => {
      meshPart.set(part.innerLayer as Mesh, bodyPartIds[index]);
      meshPart.set(part.outerLayer as Mesh, bodyPartIds[index]);
    });

    function getActiveMeshes(): Mesh[] {
      const meshes = activeLayer === 'inner' ? innerMeshes : outerMeshesArr;
      return meshes.filter((mesh) => {
        const partId = meshPart.get(mesh);
        return partId ? bodyParts[partId] : true;
      });
    }

    function layerIsVisible(): boolean {
      return activeLayer === 'inner' ? baseLayer : outerLayer;
    }

    // ---- lápiz / borrador / relleno / cuentagotas ----
    type Bounds = { left: number; top: number; width: number; height: number };
    type PixelHit = { x: number; y: number; faceKey: string; bounds: Bounds; mesh: Mesh };

    let painting = false;
    let strokePrev: PixelHit | null = null;
    let dirty = false;

    // ---- historial de la textura ----
    // Cada entrada es una copia independiente de los 16384 bytes RGBA.
    // Se guarda el estado previo a la operación; la textura actual no se
    // duplica en la pila de deshacer.
    const undoStack: Uint8ClampedArray[] = [];
    const redoStack: Uint8ClampedArray[] = [];
    // Snapshot tentativo del trazo en curso; se confirma al soltar.
    let pendingSnapshot: Uint8ClampedArray | null = null;

    function snapshot(): Uint8ClampedArray {
      return new Uint8ClampedArray(skinImage.data);
    }

    function notifyHistory() {
      onhistory?.({
        canUndo: undoStack.length > 0,
        canRedo: redoStack.length > 0,
      });
    }

    // Registra un estado previo ya confirmado y descarta el futuro.
    function pushHistory(before: Uint8ClampedArray) {
      undoStack.push(before);
      if (undoStack.length > HISTORY_LIMIT) undoStack.shift();
      redoStack.length = 0;
      notifyHistory();
    }

    function beginStrokeHistory() {
      if (pendingSnapshot) return;
      pendingSnapshot = snapshot();
    }

    // Cierra el trazo activo. Solo crea entrada si algún píxel cambió.
    function commitStrokeHistory() {
      const before = pendingSnapshot;
      pendingSnapshot = null;
      if (!before) return;

      const now = skinImage.data;
      let changed = false;
      for (let i = 0; i < now.length; i++) {
        if (now[i] !== before[i]) {
          changed = true;
          break;
        }
      }
      if (changed) pushHistory(before);
    }

    function restoreSnapshot(state: Uint8ClampedArray) {
      skinImage.data.set(state);
      // Reutiliza el canvas y la CanvasTexture existentes.
      skinCtx.putImageData(skinImage, 0, 0);
      skinTexture.needsUpdate = true;
      dirty = false;
    }

    function undo() {
      commitStrokeHistory();
      painting = false;
      strokePrev = null;

      const before = undoStack.pop();
      if (!before) {
        notifyHistory();
        return;
      }
      redoStack.push(snapshot());
      restoreSnapshot(before);
      notifyHistory();
    }

    function redo() {
      commitStrokeHistory();
      painting = false;
      strokePrev = null;

      const next = redoStack.pop();
      if (!next) {
        notifyHistory();
        return;
      }
      undoStack.push(snapshot());
      if (undoStack.length > HISTORY_LIMIT) undoStack.shift();
      restoreSnapshot(next);
      notifyHistory();
    }

    function clampTexel(value: number): number {
      return Math.min(SKIN_SIZE - 1, Math.max(0, value));
    }

    function clampEdge(value: number): number {
      // Borde exclusivo: puede valer SKIN_SIZE (64).
      return Math.min(SKIN_SIZE, Math.max(0, value));
    }

    function toPixel(uv: Vector2): { x: number; y: number } {
      // El UV es continuo: el texel que lo contiene es floor(), no round().
      // flipY = true (CanvasTexture), por eso la v se invierte.
      return {
        x: clampTexel(Math.floor(uv.x * SKIN_SIZE)),
        y: clampTexel(Math.floor((1 - uv.y) * SKIN_SIZE)),
      };
    }

    // Rectángulo del atlas que ocupa la cara impactada. Limita el relleno
    // a esa isla UV y evita que se propague a caras vecinas.
    // Si la geometría es una unión (outerGeometry), `faceRects[faceIndex]`
    // ya trae el rectángulo UV ORIGINAL completo de la cara: el relleno
    // sigue llenando la cara completa, no solo el fragmento visible.
    function faceBounds(
      mesh: Mesh,
      face: { a: number; b: number; c: number } | null | undefined,
      fallbackX: number,
      fallbackY: number,
      faceIndex?: number
    ): Bounds {
      const rects = (mesh.geometry.userData as { faceRects?: FaceRect[] } | undefined)?.faceRects;
      if (rects && faceIndex !== undefined && faceIndex !== null) {
        const rect = rects[faceIndex];
        if (rect) return rect;
      }
      const uv = mesh.geometry.getAttribute('uv');
      if (!face || !uv) {
        return { left: fallbackX, top: fallbackY, width: 1, height: 1 };
      }

      let minU = 1;
      let maxU = 0;
      let minV = 1;
      let maxV = 0;
      for (const index of [face.a, face.b, face.c]) {
        const u = uv.getX(index);
        const v = uv.getY(index);
        minU = Math.min(minU, u);
        maxU = Math.max(maxU, u);
        minV = Math.min(minV, v);
        maxV = Math.max(maxV, v);
      }

      const left = clampEdge(Math.round(minU * SKIN_SIZE));
      const rightExclusive = clampEdge(Math.round(maxU * SKIN_SIZE));
      const top = clampEdge(Math.round((1 - maxV) * SKIN_SIZE));
      const bottomExclusive = clampEdge(Math.round((1 - minV) * SKIN_SIZE));
      if (rightExclusive <= left || bottomExclusive <= top) {
        return { left: fallbackX, top: fallbackY, width: 1, height: 1 };
      }
      return { left, top, width: rightExclusive - left, height: bottomExclusive - top };
    }

    function pixelAlpha(x: number, y: number): number {
      return skinImage.data[(y * SKIN_SIZE + x) * 4 + 3];
    }

    // Las cajas exteriores vecinas se solapan medio voxel y sus caras
    // frontales son coplanares (piernas en la entrepierna, mangas y torso
    // en los laterales): el rayo reporta ambas a la misma distancia y
    // devolver el primero dependería del orden del array (pierna derecha
    // siempre antes que la izquierda; torso antes que los brazos). Para que
    // cada lado pinte la parte visualmente correspondiente, entre hits
    // prácticamente empatados se elige la malla cuyo centro esté más cerca
    // del punto de impacto. Las separaciones reales entre caras no coplanares
    // son >= 0.1 unidades, muy por encima de esta tolerancia.
    const TIE_EPS = 0.01;
    const hitCenterScratch = new Vector3();

    function pickHit(hits: Intersection[]): Intersection | null {
      const valid: Intersection[] = [];
      for (const hit of hits) {
        if (!hit.uv) continue;
        const mesh = hit.object as Mesh;
        // Una parte oculta nunca intercepta el rayo, aunque Three.js la
        // incluya si se raycastan las mallas directamente.
        const partId = meshPart.get(mesh);
        if (partId && !bodyParts[partId]) continue;
        const { x, y } = toPixel(hit.uv);
        // Con la capa exterior activa, permitir impactos incluso en píxeles
        // transparentes (alpha 0) para poder pintar regiones vacías del sombrero,
        // chaqueta, mangas y pantalones exteriores. Con la capa interior activa,
        // solo se raycasting contra mallas interiores, por lo que nunca se llega
        // aquí con una malla exterior.
        if (outerMeshes.has(mesh) && pixelAlpha(x, y) === 0 && activeLayer === 'inner') {
          continue;
        }
        valid.push(hit);
      }
      if (!valid.length) return null;

      // three.js devuelve los hits ordenados por distancia ascendente.
      const minDist = valid[0].distance;
      const tied = valid.filter((hit) => hit.distance <= minDist + TIE_EPS);
      if (tied.length === 1) return tied[0];

      let best = tied[0];
      let bestScore = Infinity;
      for (const hit of tied) {
        const center = (hit.object as Mesh).getWorldPosition(hitCenterScratch);
        const score = hit.point.distanceTo(center);
        if (score < bestScore) {
          bestScore = score;
          best = hit;
        }
      }
      return best;
    }

    function hitTest(event: PointerEvent): PixelHit | null {
      const rect = canvasEl.getBoundingClientRect();
      if (!rect.width || !rect.height) return null;

      pointerNdc.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
      pointerNdc.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(pointerNdc, viewer.camera);

      const activeMeshes = getActiveMeshes();
      const hits = raycaster.intersectObjects(activeMeshes, false);
      const hit = pickHit(hits);
      if (!hit || !hit.uv) return null;
      // Las coordenadas UV vienen directamente de la geometría del mesh,
      // independientemente de dónde esté posicionada la malla en el espacio 3D.
      // Esto elimina cualquier desfasado causado por mover las piernas para
      // corregir el Z-fighting: las UV están atadas a la geometría, no a la
      // posición mundial del objeto.
      const { x, y } = toPixel(hit.uv);

      const mesh = hit.object as Mesh;
      const normal = hit.face?.normal;
      const faceKey = normal
        ? `${mesh.uuid}:${normal.x.toFixed(3)},${normal.y.toFixed(3)},${normal.z.toFixed(3)}`
        : `${mesh.uuid}`;
      return { x, y, faceKey, bounds: faceBounds(mesh, hit.face, x, y, hit.faceIndex), mesh };
    }

    function rgbaAt(x: number, y: number): [number, number, number, number] {
      const i = (y * SKIN_SIZE + x) * 4;
      return [
        skinImage.data[i],
        skinImage.data[i + 1],
        skinImage.data[i + 2],
        skinImage.data[i + 3],
      ];
    }

    function selectedRgba(): [number, number, number, number] {
      const m = /^#?([0-9a-f]{6})$/i.exec(color.trim());
      const n = m ? parseInt(m[1], 16) : 0xffffff;
      return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 255];
    }

    function setRgba(
      x: number,
      y: number,
      [r, g, b, a]: [number, number, number, number]
    ) {
      const i = (y * SKIN_SIZE + x) * 4;
      skinImage.data[i] = r;
      skinImage.data[i + 1] = g;
      skinImage.data[i + 2] = b;
      skinImage.data[i + 3] = a;
      dirty = true;
    }

    function pixelEquals(
      x: number,
      y: number,
      [r, g, b, a]: [number, number, number, number]
    ): boolean {
      const i = (y * SKIN_SIZE + x) * 4;
      return (
        skinImage.data[i] === r &&
        skinImage.data[i + 1] === g &&
        skinImage.data[i + 2] === b &&
        skinImage.data[i + 3] === a
      );
    }

    function applyToolPixel(x: number, y: number) {
      if (tool === 'eraser') {
        setRgba(x, y, [0, 0, 0, 0]);
      } else {
        setRgba(x, y, selectedRgba());
      }
    }

    // Calcula el bounding box UV de TODAS las caras de un mesh (isla UV completa).
    // Esto permite rellenar toda una parte del cuerpo (pierna, brazo, torso, cabeza)
    // con un solo clic, no solo el triángulo impactado por el rayo.
    function meshBoundsUV(mesh: Mesh): Bounds {
      const geometry = mesh.geometry;
      const uvAttr = geometry.getAttribute('uv');
      const indexAttr = geometry.getIndex();
      if (!uvAttr) {
        return { left: 0, top: 0, width: 1, height: 1 };
      }

      let minU = 1;
      let maxU = 0;
      let minV = 1;
      let maxV = 0;

      const process = (a: number, b: number, c: number) => {
        for (const index of [a, b, c]) {
          const u = uvAttr.getX(index);
          const v = uvAttr.getY(index);
          minU = Math.min(minU, u);
          maxU = Math.max(maxU, u);
          minV = Math.min(minV, v);
          maxV = Math.max(maxV, v);
        }
      };

      if (indexAttr) {
        for (let i = 0; i < indexAttr.count; i += 3) {
          process(indexAttr.getX(i), indexAttr.getX(i + 1), indexAttr.getX(i + 2));
        }
      } else {
        for (let i = 0; i < uvAttr.count; i += 3) {
          process(i, i + 1, i + 2);
        }
      }

      const left = clampEdge(Math.round(minU * SKIN_SIZE));
      const rightExclusive = clampEdge(Math.round(maxU * SKIN_SIZE));
      const top = clampEdge(Math.round((1 - maxV) * SKIN_SIZE));
      const bottomExclusive = clampEdge(Math.round((1 - minV) * SKIN_SIZE));
      if (rightExclusive <= left || bottomExclusive <= top) {
        return { left: 0, top: 0, width: 1, height: 1 };
      }
      return { left, top, width: rightExclusive - left, height: bottomExclusive - top };
    }

    // Rellena completamente la isla UV de la parte del cuerpo impactada.
    // Un clic en cualquier punto de la pierna rellena toda la pierna;
    // un clic en cualquier punto del brazo rellena todo el brazo, etc.
    // No se propaga a otras partes del cuerpo (cada mesh es una isla independiente).
    function fillFace(hit: PixelHit): boolean {
      const fill = selectedRgba();
      const bounds = hit.bounds;
      const { left, top, width, height } = bounds;
      const right = left + width;
      const bottom = top + height;

      let changed = false;
      for (let y = top; y < bottom; y++) {
        for (let x = left; x < right; x++) {
          const i = (y * SKIN_SIZE + x) * 4;
          if (
            skinImage.data[i] !== fill[0] ||
            skinImage.data[i + 1] !== fill[1] ||
            skinImage.data[i + 2] !== fill[2] ||
            skinImage.data[i + 3] !== fill[3]
          ) {
            skinImage.data[i] = fill[0];
            skinImage.data[i + 1] = fill[1];
            skinImage.data[i + 2] = fill[2];
            skinImage.data[i + 3] = fill[3];
            dirty = true;
            changed = true;
          }
        }
      }

      return changed;
    }

    function pickHex(hit: PixelHit): string | null {
      const [r, g, b, a] = rgbaAt(hit.x, hit.y);
      if (a === 0) return null;
      const to = (value: number) => value.toString(16).padStart(2, '0');
      return `#${to(r)}${to(g)}${to(b)}`.toUpperCase();
    }

    // Bresenham inclusivo entre dos píxeles de textura.
    function drawLine(x0: number, y0: number, x1: number, y1: number) {
      let x = x0;
      let y = y0;
      const dx = Math.abs(x1 - x0);
      const sx = x0 < x1 ? 1 : -1;
      const dy = -Math.abs(y1 - y0);
      const sy = y0 < y1 ? 1 : -1;
      let err = dx + dy;

      for (;;) {
        applyToolPixel(x, y);
        if (x === x1 && y === y1) break;
        const e2 = 2 * err;
        if (e2 >= dy) {
          err += dy;
          x += sx;
        }
        if (e2 <= dx) {
          err += dx;
          y += sy;
        }
      }
    }

    function flushTexture() {
      if (!dirty) return;
      skinCtx.putImageData(skinImage, 0, 0);
      skinTexture.needsUpdate = true;
      dirty = false;
    }

    // Importa una imagen PNG como nueva textura de skin.
    // Acepta 64×64 o 64×32 (legacy). Convierte legacy a 64×64 reconstruyendo
    // brazos y piernas izquierdas desde las derechas.
    // Llama a flushTexture() y resetea el historial de undo/redo.
    // Devuelve una copia independiente del ImageData 64×64 actual.
    // No comparte memoria con el buffer activo.
    async function exportSkin(): Promise<ImageData> {
      return new ImageData(new Uint8ClampedArray(skinImage.data), SKIN_SIZE, SKIN_SIZE);
    }

    function applyImportedImage(imageData: ImageData): void {
      const { width, height } = imageData;

      if ((width !== 64 || height !== 64) && (width !== 64 || height !== 32)) {
        throw new Error(`Dimensiones no válidas: ${width}×${height}. Se requieren 64×64 o 64×32.`);
      }

      const tempCanvas = document.createElement('canvas');
      tempCanvas.width = 64;
      tempCanvas.height = 64;
      const tempCtx = tempCanvas.getContext('2d', { willReadFrequently: true })!;

      if (width === 64 && height === 32) {
        // --- Legacy 64×32 → 64×64 ---
        // Copiar la mitad superior tal cual
        tempCtx.putImageData(imageData, 0, 0);

        // Región derecha del brazo: X 40-55, Y 16-31 → Dest: X 32-47, Y 48-63
        tempCtx.save();
        tempCtx.translate(32 + 15, 48 + 15); // centro del destino
        tempCtx.scale(-1, 1); // espejo horizontal
        tempCtx.drawImage(
          tempCanvas,
          40, 16, 16, 16,      // origen (sx, sy, sw, sh)
          -8, -8, 16, 16       // destino (dx, dy, dw, dh)
        );
        tempCtx.restore();

        // Región derecha de la pierna: X 0-15, Y 16-31 → Dest: X 16-31, Y 48-63
        tempCtx.save();
        tempCtx.translate(16 + 15, 48 + 15);
        tempCtx.scale(-1, 1);
        tempCtx.drawImage(
          tempCanvas,
          0, 16, 16, 16,
          -8, -8, 16, 16
        );
        tempCtx.restore();
      } else {
        // 64×64: copiar directamente
        tempCtx.putImageData(imageData, 0, 0);
      }

      const imported = tempCtx.getImageData(0, 0, 64, 64);

      // Copiar al buffer de la textura
      skinImage.data.set(imported.data);
      skinCtx.putImageData(skinImage, 0, 0);
      skinTexture.needsUpdate = true;
      dirty = false;

      // Resetear historial: la importación es una nueva base de edición
      undoStack.length = 0;
      redoStack.length = 0;
      notifyHistory();

      // Si es legacy 64×32, forzar modelo Classic
      if (height === 32) {
        viewer.playerObject.skin.modelType = 'default';
      }
    }

    function paintStroke(hit: PixelHit) {
      if (!strokePrev) {
        applyToolPixel(hit.x, hit.y);
      } else if (strokePrev.faceKey !== hit.faceKey) {
        // Otra cara o malla: no unir a través de la costura UV.
        applyToolPixel(hit.x, hit.y);
      } else if (
        Math.hypot(hit.x - strokePrev.x, hit.y - strokePrev.y) >
        MAX_STROKE_JUMP
      ) {
        // Salto demasiado grande dentro de la textura: evitar líneas falsas.
        applyToolPixel(hit.x, hit.y);
      } else {
        drawLine(strokePrev.x, strokePrev.y, hit.x, hit.y);
      }
      strokePrev = hit;
    }

    function releasePointer(pointerId: number) {
      if (canvas.hasPointerCapture(pointerId)) {
        canvas.releasePointerCapture(pointerId);
      }
    }

    // Centro del modelo en el mundo. La rotación orbita siempre este punto:
    // el paneo (Ctrl+clic) desplaza objetivo y cámara en conjunto, y sin
    // re-anclar, una rotación posterior pivotaría donde haya quedado el
    // paneo (p. ej. sobre los pies). Solo se ajusta si se desplazó.
    function reAnchorPivot() {
      const viewer = viewerInstance;
      if (!viewer) return;
      const target = viewer.controls.target;
      const dx = initialControlsTarget[0] - target.x;
      const dy = initialControlsTarget[1] - target.y;
      const dz = initialControlsTarget[2] - target.z;
      if (dx * dx + dy * dy + dz * dz <= 1) return;
      viewer.camera.position.x += dx;
      viewer.camera.position.y += dy;
      viewer.camera.position.z += dz;
      target.set(initialControlsTarget[0], initialControlsTarget[1], initialControlsTarget[2]);
      viewer.controls.update();
    }

    function onPointerDown(event: PointerEvent) {
      // La rotación (clic derecho) orbita el centro del modelo aunque el
      // paneo haya desplazado el pivote (p. ej. hasta los pies).
      if (event.button === 2) {
        reAnchorPivot();
      }
      if (event.button !== 0) return;
      if (event.pointerType !== 'mouse') return;
      // Ctrl + clic izquierdo: desplazar la vista, nunca pintar.
      if (event.ctrlKey) {
        startPan(event);
        return;
      }
      // No pintar si el clic derecho está activo (rotación).
      if ((event.buttons & 2) !== 0) return;
      if (tool !== 'pencil' && tool !== 'eraser' && tool !== 'fill' && tool !== 'picker') {
        return;
      }
      // Si la capa activa está ocultada, no aplicar pintura.
      if (!layerIsVisible()) return;

      const hit = hitTest(event);
      if (!hit) return;

      event.preventDefault();

      if (tool === 'fill') {
        // Un solo clic: reemplaza todos los píxeles de la cara UV impactada
        // con el color seleccionado, independientemente de su color o
        // transparencia anterior.
        // El snapshot se toma antes de modificar; solo se registra si
        // el relleno cambió al menos un píxel.
        const before = snapshot();
        if (fillFace(hit)) {
          flushTexture();
          pushHistory(before);
        }
        return;
      }

      if (tool === 'picker') {
        // Solo lectura: no modifica el buffer ni la textura.
        const hex = pickHex(hit);
        if (hex) onpick?.(hex);
        return;
      }

      // Lápiz o borrador: trazo continuo compartido.
      // Un arrastre completo genera una sola entrada de historial.
      beginStrokeHistory();
      painting = true;
      strokePrev = null;
      canvas.setPointerCapture(event.pointerId);
      paintStroke(hit);
      flushTexture();
    }

    function onPointerMove(event: PointerEvent) {
      if (!painting) return;
      if ((event.buttons & 1) === 0) {
        commitStrokeHistory();
        painting = false;
        strokePrev = null;
        releasePointer(event.pointerId);
        return;
      }
      // Durante una rotación con clic derecho no se pinta.
      if ((event.buttons & 2) !== 0) return;

      const hit = hitTest(event);
      if (!hit) {
        // Fuera del personaje: cortar el trazo para no unir extremos.
        strokePrev = null;
        return;
      }
      paintStroke(hit);
      flushTexture();
    }

    function onPointerEnd(event: PointerEvent) {
      commitStrokeHistory();
      painting = false;
      strokePrev = null;
      releasePointer(event.pointerId);
    }

    // Cancela el trazo si OrbitControls inicia una rotación o un zoom.
    const onControlsStart = () => {
      commitStrokeHistory();
      painting = false;
      strokePrev = null;
    };

    // ---- pan: Ctrl + clic izquierdo + arrastre ----
    // Desplaza cámara y objetivo en el plano de pantalla, como la herramienta
    // mano de Photoshop. No toca geometría, textura, UV ni modelo.
    let panning = false;
    let panPointerId: number | null = null;
    let panLastX = 0;
    let panLastY = 0;
    let ctrlHeld = false;
    const panRight = new Vector3();
    const panUp = new Vector3();
    const panShift = new Vector3();

    function updatePanCursor() {
      // Estilo inline: gana a las reglas de cursor de pintura/arrastre y se
      // limpia solo al terminar el gesto.
      canvas.style.cursor = panning ? 'grabbing' : ctrlHeld ? 'move' : '';
    }

    // Desplazamiento proporcional al arrastre y a la distancia al objetivo.
    // Réplica del cálculo de pan de OrbitControls, aplicado a cámara y target
    // para conservar la rotación, el zoom y la distancia actuales.
    function panBy(dx: number, dy: number) {
      const rect = canvas.getBoundingClientRect();
      if (!rect.height) return;

      const camera = viewer.camera;
      const controls = viewer.controls;
      const targetDistance =
        camera.position.distanceTo(controls.target) *
        Math.tan((camera.fov * Math.PI) / 360);
      const scale = (2 * targetDistance) / rect.height;

      camera.updateMatrix();
      panRight.setFromMatrixColumn(camera.matrix, 0);
      panUp.setFromMatrixColumn(camera.matrix, 1);
      panShift.copy(panRight).multiplyScalar(-dx * scale);
      panShift.addScaledVector(panUp, dy * scale);
      camera.position.add(panShift);
      controls.target.add(panShift);
      controls.update();
    }

    function startPan(event: PointerEvent) {
      // Un gesto de pan nunca debe dejar un trazo a medias.
      commitStrokeHistory();
      painting = false;
      strokePrev = null;

      panning = true;
      panPointerId = event.pointerId;
      panLastX = event.clientX;
      panLastY = event.clientY;
      event.preventDefault();
      try {
        canvas.setPointerCapture(event.pointerId);
      } catch {
        // Si el navegador no permite capturar este evento, los listeners de
        // window mantienen el gesto igualmente.
      }
      window.addEventListener('pointermove', onPanMove);
      window.addEventListener('pointerup', onPanEnd);
      window.addEventListener('pointercancel', onPanEnd);
      updatePanCursor();
    }

    function onPanMove(event: PointerEvent) {
      if (!panning || event.pointerId !== panPointerId) return;
      if ((event.buttons & 1) === 0) {
        endPan();
        return;
      }

      const dx = event.clientX - panLastX;
      const dy = event.clientY - panLastY;
      if (dx === 0 && dy === 0) return;
      panLastX = event.clientX;
      panLastY = event.clientY;
      event.preventDefault();
      panBy(dx, dy);
    }

    function onPanEnd(event: PointerEvent) {
      if (!panning || event.pointerId !== panPointerId) return;
      endPan();
    }

    function endPan() {
      if (!panning) return;
      panning = false;
      const pointerId = panPointerId;
      panPointerId = null;
      window.removeEventListener('pointermove', onPanMove);
      window.removeEventListener('pointerup', onPanEnd);
      window.removeEventListener('pointercancel', onPanEnd);
      if (pointerId !== null) releasePointer(pointerId);
      updatePanCursor();
    }

    // Restaura la pose inicial de la cámara y el objetivo. Deshace el Pan
    // acumulado y recupera el encuadre original sin tocar textura, modelo,
    // historial, color ni capas.
    function recenterCamera() {
      endPan();
      viewer.camera.position.set(...initialCameraPosition);
      viewer.controls.target.set(...initialControlsTarget);
      viewer.zoom = initialZoom;
      viewer.controls.update();
    }

    // Ctrl mantenido (solo feedback de cursor) y fin de pan al soltarlo.
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== 'Control') return;
      ctrlHeld = true;
      updatePanCursor();
    }

    function onKeyUp(event: KeyboardEvent) {
      if (event.key !== 'Control') return;
      ctrlHeld = false;
      // Soltar Ctrl finaliza el pan aunque el botón izquierdo siga presionado.
      endPan();
      updatePanCursor();
    }

    function onWindowBlur() {
      ctrlHeld = false;
      endPan();
      // No dejar un trazo abierto si la ventana pierde el foco.
      commitStrokeHistory();
      painting = false;
      strokePrev = null;
      updatePanCursor();
    }

    canvas.addEventListener('pointerdown', onPointerDown);
    canvas.addEventListener('pointermove', onPointerMove);
    canvas.addEventListener('pointerup', onPointerEnd);
    canvas.addEventListener('pointercancel', onPointerEnd);
    viewer.controls.addEventListener('start', onControlsStart);
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onWindowBlur);

    // Expone el historial al contenedor y publica la disponibilidad inicial.
    viewer.playerObject.skin.setInnerLayerVisible(baseLayer);
    viewer.playerObject.skin.setOuterLayerVisible(outerLayer);
    applyBodyPartVisibility();
    onready?.({
      undo,
      redo,
      recenter: recenterCamera,
      importSkin: (imageData) => applyImportedImage(imageData),
      exportSkin,
    });
    notifyHistory();

    // Adapta el renderer al tamaño real del contenedor, incluyendo
    // cambios de ventana y del layout.
    const resizeObserver = new ResizeObserver(() => {
      syncCanvasSize();
    });
    resizeObserver.observe(container);

    return () => {
      endPan();
      if (currentOuterMap) disposeOuterGeometries(currentOuterMap);
      currentOuterMap = null;
      currentOuterKey = '';
      resizeObserver.disconnect();
      canvas.removeEventListener('pointerdown', onPointerDown);
      canvas.removeEventListener('pointermove', onPointerMove);
      canvas.removeEventListener('pointerup', onPointerEnd);
      canvas.removeEventListener('pointercancel', onPointerEnd);
      viewer.controls.removeEventListener('start', onControlsStart);
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onWindowBlur);

      viewer.playerObject.skin.map = null;
      skinTexture.dispose();
      viewer.dispose();
    };
  });

  $effect(() => {
    if (viewerInstance) {
      viewerInstance.playerObject.skin.setInnerLayerVisible(baseLayer);
      viewerInstance.playerObject.skin.setOuterLayerVisible(outerLayer);
      regenerateOuter();
    }
  });

  $effect(() => {
    applyBodyPartVisibility();
  });

  $effect(() => {
    if (viewerInstance) {
      viewerInstance.playerObject.skin.modelType = model === 'slim' ? 'slim' : 'default';
    }
  });
</script>

<div bind:this={containerEl} class="viewer-host">
  <canvas
    bind:this={canvasEl}
    class="viewer-canvas"
    class:paint-tool={tool === 'pencil' || tool === 'eraser' || tool === 'fill' || tool === 'picker'}
  ></canvas>
</div>

<style>
  .viewer-host {
    position: absolute;
    inset: 0;
    z-index: 0;
  }
  .viewer-canvas {
    display: block;
    width: 100%;
    height: 100%;
    cursor: grab;
    touch-action: none;
  }
  .viewer-canvas:active {
    cursor: grabbing;
  }
  .viewer-canvas.paint-tool,
  .viewer-canvas.paint-tool:active {
    cursor: crosshair;
  }
</style>
