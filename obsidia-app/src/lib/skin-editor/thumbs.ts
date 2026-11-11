// Generador de miniaturas 3D para la biblioteca de skins.
// Usa UN solo SkinViewer oculto y offscreen que se reutiliza para todas las
// skins (evita decenas de contextos WebGL). No comparte ni modifica el
// buffer activo del editor: recibe el ImageData ya copiado y dibuja sobre
// un canvas temporal propio. disposeThumbRenderer() libera el visor.
import * as skinview3d from 'skinview3d';

// Resolución de render del mini-visor. pixelRatio 2 → 208×416 reales.
const THUMB_W = 104;
const THUMB_H = 208;
const THUMB_PIXEL_RATIO = 2;

let viewer: skinview3d.SkinViewer | null = null;

function ensureViewer(): skinview3d.SkinViewer {
  if (viewer) return viewer;

  const canvas = document.createElement('canvas');
  viewer = new skinview3d.SkinViewer({
    canvas,
    width: THUMB_W,
    height: THUMB_H,
    pixelRatio: THUMB_PIXEL_RATIO,
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
  // Pose 3/4: leve giro hacia la izquierda.
  viewer.playerObject.rotation.y = -0.45;
  return viewer;
}

export function disposeThumbRenderer(): void {
  if (!viewer) return;
  viewer.dispose();
  viewer = null;
}

/**
 * Renderiza una miniatura 3D del personaje para el ImageData dado.
 * `imageData` debe ser 64×64 y ya debe venir convertido (legacy incluido).
 * Devuelve un Blob PNG transparente; no toca el buffer enviado.
 */
export async function renderThumb(
  imageData: ImageData,
  model: 'classic' | 'slim'
): Promise<Blob> {
  if (imageData.width !== 64 || imageData.height !== 64) {
    throw new Error(`La miniatura requiere una textura 64×64 (llegó ${imageData.width}×${imageData.height}).`);
  }

  const skin = ensureViewer();

  const skinCanvas = document.createElement('canvas');
  skinCanvas.width = 64;
  skinCanvas.height = 64;
  skinCanvas.getContext('2d')!.putImageData(imageData, 0, 0);

  // Con fuente canvas la carga es síncrona; model 'classic' → 'default'.
  skin.loadSkin(skinCanvas, {
    model: model === 'slim' ? 'slim' : 'default',
    makeVisible: true,
  });
  skin.render();

  const out = document.createElement('canvas');
  out.width = THUMB_W;
  out.height = THUMB_H;
  const ctx = out.getContext('2d');
  if (!ctx) throw new Error('No se pudo preparar el canvas de la miniatura.');
  ctx.clearRect(0, 0, THUMB_W, THUMB_H);
  ctx.drawImage(skin.canvas, 0, 0, THUMB_W, THUMB_H);

  const blob = await new Promise<Blob | null>((resolve) => {
    out.toBlob(resolve, 'image/png');
  });
  if (!blob || blob.size === 0) {
    throw new Error('No se pudo generar el PNG de la miniatura.');
  }
  return blob;
}
