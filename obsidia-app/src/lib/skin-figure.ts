// Figura 3D del jugador para «Mi cuenta».
// Reutiliza skinview3d (ya usado por el editor) en UN visor oculto y sin
// controles: dibuja la skin completa, con las DOS capas (interior + exterior),
// respetando el modelo Classic/Slim y las transparencias. Devuelve un PNG
// transparente cacheado como object URL, así cada tarjeta solo inserta un <img>.
import * as skinview3d from "skinview3d";
import { skinFull, paintSkinCanvases } from "$lib/utils.js";

// 132×264 = misma proporción (1:2) y mismo encuadre que la miniatura del
// editor; se muestra a 132×264 en la tarjeta superior y a 88×176 en la inferior.
const FIG_W = 132;
const FIG_H = 264;
const PIXEL_RATIO = 2;

const CACHE_MAX = 8;

let viewer: skinview3d.SkinViewer | null = null;
const urls = new Map<string, string>();

function ensureViewer(): skinview3d.SkinViewer {
  if (viewer) return viewer;

  const canvas = document.createElement("canvas");
  viewer = new skinview3d.SkinViewer({
    canvas,
    width: FIG_W,
    height: FIG_H,
    pixelRatio: PIXEL_RATIO,
    // La lectura del canvas (drawImage) necesita el buffer preservado.
    preserveDrawingBuffer: true,
    // Sin bucle de rendering: se llama render() manualmente por skin.
    renderPaused: true,
    enableControls: false,
    fov: 50,
    zoom: 0.9,
  });
  viewer.animation = null;
  viewer.autoRotate = false;
  // Pose 3/4: deja ver el volumen de la capa exterior.
  viewer.playerObject.rotation.y = -0.45;
  return viewer;
}

function cachePut(key: string, url: string): string {
  urls.set(key, url);
  while (urls.size > CACHE_MAX) {
    const oldest = urls.keys().next().value as string | undefined;
    if (oldest === undefined) break;
    const stale = urls.get(oldest);
    urls.delete(oldest);
    if (stale) URL.revokeObjectURL(stale);
  }
  return url;
}

/** Límites del contenido visible (alfa > 8) de un canvas. */
function contentBox(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number
): { cx: number; bottom: number; h: number } | null {
  const d = ctx.getImageData(0, 0, w, h).data;
  let minY = h;
  let maxY = -1;
  let minX = w;
  let maxX = -1;
  for (let y = 0; y < h; y++) {
    const row = y * w;
    for (let x = 0; x < w; x++) {
      if (d[(row + x) * 4 + 3] > 8) {
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
    }
  }
  if (maxY < minY) return null;
  return { cx: (minX + maxX + 1) / 2, bottom: maxY + 1, h: maxY + 1 - minY };
}

/** Decodifica el data URL a ImageData 64×64, convirtiendo el formato legacy 64×32. */
function toImageData(url: string): Promise<ImageData> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      const w = img.naturalWidth;
      const h = img.naturalHeight;
      if (w !== 64 || (h !== 64 && h !== 32)) {
        reject(new Error(`Skin con dimensiones no válidas (${w}×${h}).`));
        return;
      }
      const canvas = document.createElement("canvas");
      canvas.width = 64;
      canvas.height = 64;
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) {
        reject(new Error("Sin contexto 2D."));
        return;
      }
      ctx.clearRect(0, 0, 64, 64);
      ctx.drawImage(img, 0, 0);
      if (h === 32) {
        // Brazo y pierna izquierdos: espejo horizontal desde el lado derecho.
        ctx.save();
        ctx.translate(32 + 15, 48 + 15);
        ctx.scale(-1, 1);
        ctx.drawImage(img, 40, 16, 16, 16, -8, -8, 16, 16);
        ctx.restore();
        ctx.save();
        ctx.translate(16 + 15, 48 + 15);
        ctx.scale(-1, 1);
        ctx.drawImage(img, 0, 16, 16, 16, -8, -8, 16, 16);
        ctx.restore();
      }
      resolve(ctx.getImageData(0, 0, 64, 64));
    };
    img.onerror = () => reject(new Error("No se pudo decodificar la skin."));
    img.src = url;
  });
}

/** Marca una figura: 3D si hay skin válida, o el dibujo plano de siempre. */
export function skinFigureMarkup(url: any, model: string, scale = 8): string {
  const ok = url && /^data:image\/png;base64,[A-Za-z0-9+/=]+$/.test(String(url));
  if (!ok) return skinFull(null, model, scale);
  return `<img class="js-figure" alt="Skin de jugador" draggable="false" data-figure-painted="0" data-figure-scale="${scale}" data-skin="${url}" data-model="${model === "slim" ? "slim" : "classic"}">`;
}

/** Pinta los <img class="js-figure"> ya insertados en el DOM. */
export function paintSkinFigures(root: ParentNode | null): void {
  const box = root || document;
  box.querySelectorAll<HTMLImageElement>("img.js-figure").forEach((img) => {
    if (img.dataset.figurePainted === "1") return;
    img.dataset.figurePainted = "1";
    const url = img.dataset.skin || "";
    const model = img.dataset.model === "slim" ? "slim" : "classic";
    renderSkinFigure(url, model)
      .then((src) => {
        img.src = src;
        img.classList.add("ready");
      })
      .catch(() => {
        // Respaldo: figura plana con el pintor de siempre.
        const scale = Math.max(1, Math.round(+img.dataset.figureScale || 8));
        const tmp = document.createElement("div");
        tmp.innerHTML = skinFull(url, model, scale);
        const cv = tmp.firstElementChild;
        if (!cv) {
          img.remove();
          return;
        }
        img.replaceWith(cv);
        paintSkinCanvases(tmp);
      });
  });
}

/** Renderiza la figura 3D y devuelve un object URL cacheado. */
export async function renderSkinFigure(url: string, model: string): Promise<string> {
  const key = `${model}|${url}`;
  const cached = urls.get(key);
  if (cached) return cached;

  const data = await toImageData(url);
  const skin = ensureViewer();

  const source = document.createElement("canvas");
  source.width = 64;
  source.height = 64;
  const sctx = source.getContext("2d");
  if (!sctx) throw new Error("Sin contexto 2D para la skin.");
  sctx.putImageData(data, 0, 0);
  // skinview-utils borra la capa exterior entera cuando la skin no tiene ni un
  // píxel transparente (heurística para PNG con fondo opaco). Minecraft no hace
  // eso: muestra las dos capas tal cual. Se libera un píxel de una zona sin uso
  // (56..63 × 16..31) para conservar la capa exterior.
  sctx.clearRect(63, 31, 1, 1);

  // Fuente canvas: la carga es síncrona. 'classic' → 'default' en skinview3d.
  skin.loadSkin(source, { model: model === "slim" ? "slim" : "default", makeVisible: true });
  skin.render();

  const out = document.createElement("canvas");
  out.width = FIG_W;
  out.height = FIG_H;
  const ctx = out.getContext("2d");
  if (!ctx) throw new Error("Sin contexto 2D para la figura.");
  ctx.clearRect(0, 0, FIG_W, FIG_H);
  ctx.drawImage(skin.canvas, 0, 0, FIG_W, FIG_H);

  // Recorte: el visor deja aire alrededor. Se ajusta al contenido visible para
  // que los pies queden apoyados en el borde inferior, donde va la sombra.
  const box = contentBox(ctx, FIG_W, FIG_H);
  if (box) {
    const target = FIG_W / FIG_H;
    const bh = Math.min(FIG_H, box.h);
    const bw = Math.min(FIG_W, bh * target);
    const pad = FIG_H - bh >= 6 ? 4 : 0;
    let sx = Math.round(box.cx - bw / 2);
    const sy = Math.round(box.bottom - bh);
    if (sx < 0) sx = 0;
    if (sx + bw > FIG_W) sx = FIG_W - bw;
    const kept = document.createElement("canvas");
    kept.width = FIG_W;
    kept.height = FIG_H;
    const kctx = kept.getContext("2d");
    if (kctx) {
      kctx.drawImage(out, sx, sy, bw, bh, Math.round((FIG_W - bw) / 2), FIG_H - pad - bh, bw, bh);
      ctx.clearRect(0, 0, FIG_W, FIG_H);
      ctx.drawImage(kept, 0, 0);
    }
  }

  const blob = await new Promise<Blob | null>((resolve) => {
    out.toBlob(resolve, "image/png");
  });
  if (!blob || blob.size === 0) throw new Error("No se pudo generar la figura.");
  return cachePut(key, URL.createObjectURL(blob));
}

export function disposeSkinFigureRenderer(): void {
  if (viewer) {
    viewer.dispose();
    viewer = null;
  }
  urls.forEach((u) => URL.revokeObjectURL(u));
  urls.clear();
}
