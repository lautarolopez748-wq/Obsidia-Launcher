// ============================================================================
// Puente Etapa 2 (launcher): el almacén del editor apoyado en el SISTEMA REAL
// del lanzador (`localStorage['obsidia.skins']`: máx. 4 skins base64
// "Skin 1..4"), en vez de su propio IndexedDB. Misma API que `library.ts`
// (que queda deshabilitada sin borrar su código): el componente del editor
// no nota la diferencia y su galería refleja el almacén del lanzador.
//
// La metainformación por entrada (id estable + modelo + fecha) vive en
// `obsidia.skins.meta`, alineada por índice con `obsidia.skins`. Las
// miniaturas 3D se cachean SOLO en memoria por sesión: no se persisten.
// ============================================================================

export type SkinModel = 'classic' | 'slim';

export interface SkinRecord {
  id: string;
  name: string;
  model: SkinModel;
  pngBlob: Blob;
  createdAt: number;
  schemaVersion: 1;
  // Miniatura 3D pre-renderizada del personaje (PNG transparente).
  thumbBlob?: Blob;
}

// Códigos de fallo (mismo catálogo que `library.ts`, más 'limit').
export type LibraryErrorCode =
  | 'unavailable'
  | 'open'
  | 'transaction'
  | 'collision'
  | 'quota'
  | 'invalid-blob'
  | 'corrupt'
  | 'limit';

export class LibraryError extends Error {
  readonly code: LibraryErrorCode;

  constructor(code: LibraryErrorCode, message: string, cause?: unknown) {
    super(message);
    this.name = 'LibraryError';
    this.code = code;
    if (cause !== undefined) (this as { cause?: unknown }).cause = cause;
  }
}

const SKINS_KEY = 'obsidia.skins';
const META_KEY = 'obsidia.skins.meta';
const MAX_SKINS = 4;

// Miniaturas 3D: caché volátil por sesión (no se persiste nada extra).
const thumbCache = new Map<string, Blob>();
// PNG ya decodificados por id (evita repetir fetch data URL por refresco).
const blobCache = new Map<string, Blob>();

interface MetaEntry {
  id: string;
  model: SkinModel;
  createdAt: number;
}

function generateId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) return crypto.randomUUID();
  return `id-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function globalModel(): SkinModel {
  try {
    return localStorage.getItem('obsidia.skmodel') === 'slim' ? 'slim' : 'classic';
  } catch {
    return 'classic';
  }
}

export function readSkinsDataUrls(): string[] {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(SKINS_KEY);
  } catch (err) {
    throw new LibraryError('unavailable', 'El almacenamiento local no está disponible.', err);
  }
  if (!raw) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    throw new LibraryError('corrupt', 'El almacén de skins está dañado (JSON inválido).', err);
  }
  if (!Array.isArray(parsed) || parsed.some((x) => typeof x !== 'string')) {
    throw new LibraryError('corrupt', 'El almacén de skins está dañado (formato inválido).');
  }
  return parsed as string[];
}

function readMeta(): MetaEntry[] {
  try {
    const raw = localStorage.getItem(META_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const out: MetaEntry[] = [];
    for (const e of parsed) {
      if (e && typeof e.id === 'string') {
        out.push({
          id: e.id,
          model: e.model === 'slim' ? 'slim' : 'classic',
          createdAt: typeof e.createdAt === 'number' ? e.createdAt : 0,
        });
      } else {
        out.push({ id: generateId(), model: globalModel(), createdAt: 0 });
      }
    }
    return out;
  } catch {
    return [];
  }
}

function writeMeta(meta: MetaEntry[]): void {
  try {
    localStorage.setItem(META_KEY, JSON.stringify(meta));
  } catch (err) {
    throw new LibraryError('quota', 'Se agotó el espacio disponible para la biblioteca local.', err);
  }
}

// Devuelve meta alineado por índice con `skins`. Si falta o sobra, se
// autocura (entradas nuevas con el modelo global actual; sobrantes = un
// borrado sin sincronizar por el lanzador).
function alignedMeta(skins: string[]): MetaEntry[] {
  let meta = readMeta();
  if (meta.length !== skins.length) {
    const healed: MetaEntry[] = [];
    for (let i = 0; i < skins.length; i++) {
      if (meta[i]) healed.push(meta[i]);
      else healed.push({ id: generateId(), model: globalModel(), createdAt: 0 });
    }
    meta = healed;
    writeMeta(meta);
  }
  return meta;
}

function writeSkinsAndMeta(skins: string[], meta: MetaEntry[]): void {
  try {
    localStorage.setItem(SKINS_KEY, JSON.stringify(skins));
  } catch (err) {
    throw new LibraryError('quota', 'Se agotó el espacio disponible para la biblioteca local.', err);
  }
  writeMeta(meta);
}

function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const fr = new FileReader();
    fr.onload = () => resolve(String(fr.result || ''));
    fr.onerror = () => reject(new LibraryError('invalid-blob', 'No se pudo leer el PNG generado por el editor.'));
    fr.readAsDataURL(blob);
  });
}

async function dataUrlToBlob(dataUrl: string, id: string): Promise<Blob> {
  const hit = blobCache.get(id);
  if (hit) return hit;
  const res = await fetch(dataUrl);
  if (!res.ok) throw new LibraryError('corrupt', 'Una skin guardada no se pudo leer.');
  const blob = await res.blob();
  blobCache.set(id, blob);
  return blob;
}

// ---- API del editor (misma superficie que `library.ts`) ----------------

export async function saveSkin(
  _name: string,
  model: SkinModel,
  pngBlob: Blob,
  thumbBlob?: Blob
): Promise<SkinRecord> {
  const dataUrl = await blobToDataUrl(pngBlob);
  const skins = readSkinsDataUrls();
  const meta = alignedMeta(skins);
  if (skins.length >= MAX_SKINS) {
    throw new LibraryError('limit', 'Máximo 4 skins: borrá una para guardar otra.');
  }
  const id = generateId();
  const entry: MetaEntry = { id, model: model === 'slim' ? 'slim' : 'classic', createdAt: Date.now() };
  skins.push(dataUrl);
  meta.push(entry);
  writeSkinsAndMeta(skins, meta);
  if (thumbBlob) thumbCache.set(id, thumbBlob);
  blobCache.set(id, pngBlob);
  return {
    id,
    name: `Skin ${skins.length}`,
    model: entry.model,
    pngBlob,
    createdAt: entry.createdAt,
    schemaVersion: 1,
    thumbBlob,
  };
}

export async function loadSkins(): Promise<SkinRecord[]> {
  const skins = readSkinsDataUrls();
  const meta = alignedMeta(skins);
  const records: SkinRecord[] = [];
  for (let i = 0; i < skins.length; i++) {
    const m = meta[i];
    records.push({
      id: m.id,
      name: `Skin ${i + 1}`,
      model: m.model,
      pngBlob: await dataUrlToBlob(skins[i], m.id),
      createdAt: m.createdAt,
      schemaVersion: 1,
      thumbBlob: thumbCache.get(m.id),
    });
  }
  return records;
}

export async function loadSkinById(id: string): Promise<SkinRecord | null> {
  const skins = readSkinsDataUrls();
  const meta = alignedMeta(skins);
  const i = meta.findIndex((m) => m.id === id);
  if (i < 0 || i >= skins.length) return null;
  const m = meta[i];
  return {
    id: m.id,
    name: `Skin ${i + 1}`,
    model: m.model,
    pngBlob: await dataUrlToBlob(skins[i], m.id),
    createdAt: m.createdAt,
    schemaVersion: 1,
    thumbBlob: thumbCache.get(m.id),
  };
}

export async function deleteSkin(id: string): Promise<boolean> {
  const skins = readSkinsDataUrls();
  const meta = alignedMeta(skins);
  const i = meta.findIndex((m) => m.id === id);
  if (i < 0 || i >= skins.length) return false;
  skins.splice(i, 1);
  meta.splice(i, 1);
  writeSkinsAndMeta(skins, meta);
  thumbCache.delete(id);
  blobCache.delete(id);
  return true;
}

export async function updateThumb(id: string, thumbBlob: Blob): Promise<void> {
  if (id) thumbCache.set(id, thumbBlob);
}

// ---- Utilidad del lanzador (Mi cuenta) ---------------------------------

// Índice del slot con ese id estable (o -1).
export function slotIndexOfId(id: string): number {
  try {
    const skins = readSkinsDataUrls();
    const meta = alignedMeta(skins);
    return meta.findIndex((m) => m.id === id);
  } catch {
    return -1;
  }
}

// Id estable del slot en el índice dado (o null).
export function slotIdAt(index: number): string | null {
  try {
    const skins = readSkinsDataUrls();
    if (index < 0 || index >= skins.length) return null;
    const meta = alignedMeta(skins);
    return meta[index] ? meta[index].id : null;
  } catch {
    return null;
  }
}

// Añade una entrada al meta cuando el lanzador agrega una skin sin pasar
// por el editor (subida de PNG en Mi cuenta).
export function metaAppend(model: SkinModel): void {
  try {
    const skins = readSkinsDataUrls();
    const meta = alignedMeta(skins);
    const i = skins.length - 1;
    if (i < 0) return;
    if (meta.length <= i || meta[i] === undefined || meta[i] === null) {
      meta[i] = { id: generateId(), model: model === 'slim' ? 'slim' : 'classic', createdAt: Date.now() };
      writeMeta(meta);
    }
  } catch {
    /* la importación ya se validó; el meta se autocura al leer */
  }
}

// Quita la entrada i del meta cuando el lanzador elimina una skin
// (borrado en Mi cuenta, que reordena los índices).
export function metaRemoveAt(index: number): void {
  try {
    const meta = readMeta();
    if (index >= 0 && index < meta.length) {
      meta.splice(index, 1);
      writeMeta(meta);
    }
  } catch {
    /* la autocuración de lectura corrige cualquier desvío */
  }
}
