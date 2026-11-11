// Capa de almacenamiento local (IndexedDB) para la biblioteca de skins de
// Obsidia Custom. Es independiente de Svelte y de Three.js: toda la
// operación vive detrás de la interfaz `SkinStorage`, de modo que más
// adelante pueda implementarse otro adaptador (por ejemplo, sistema de
// archivos de Tauri/Rust) sin tocar los componentes.

export interface SkinRecord {
  id: string;
  name: string;
  model: 'classic' | 'slim';
  pngBlob: Blob;
  createdAt: number;
  schemaVersion: 1;
  // Miniatura 3D pre-renderizada del personaje (PNG transparente).
  // Campo opcional y aditivo: las entradas anteriores siguen siendo válidas
  // sin necesidad de migrar la base de datos (versión 1 no cambia).
  thumbBlob?: Blob;
}

// Códigos de fallo de la biblioteca. Permiten distinguir sin parsear
// mensajes: falta IndexedDB, error de apertura, error de transacción,
// colisión de id, cuota excedida, Blob inválido o registro dañado.
export type LibraryErrorCode =
  | 'unavailable'
  | 'open'
  | 'transaction'
  | 'collision'
  | 'quota'
  | 'invalid-blob'
  | 'corrupt';

export class LibraryError extends Error {
  readonly code: LibraryErrorCode;

  constructor(code: LibraryErrorCode, message: string, cause?: unknown) {
    super(message);
    this.name = 'LibraryError';
    this.code = code;
    if (cause !== undefined) (this as { cause?: unknown }).cause = cause;
  }
}

// Interfaz de almacenamiento. Un futuro adaptador Tauri implementa estos
// cuatro métodos y reemplaza al adaptador por defecto.
export interface SkinStorage {
  // Siempre crea una entrada nueva: si el id colisiona, falla y nunca
  // sobrescribe un registro anterior.
  save(record: SkinRecord): Promise<void>;
  list(): Promise<SkinRecord[]>;
  getById(id: string): Promise<SkinRecord | null>;
  // Devuelve true sólo si existía la entrada indicada y fue eliminada.
  delete(id: string): Promise<boolean>;
}

// Etapa 1 del launcher: base aislada del editor (no se mezcla con el
// resto de la app, que guarda en localStorage).
const DB_NAME = 'obsidia-skin-editor';
const DB_VERSION = 1;
const STORE_NAME = 'skins';

let openPromise: Promise<IDBDatabase> | null = null;

function openDatabase(): Promise<IDBDatabase> {
  if (openPromise) return openPromise;

  if (typeof window === 'undefined' || typeof indexedDB === 'undefined' || indexedDB === null) {
    return Promise.reject(
      new LibraryError(
        'unavailable',
        'IndexedDB no está disponible en este entorno (¿modo incógnito o navegador antiguo?).'
      )
    );
  }

  const p = new Promise<IDBDatabase>((resolve, reject) => {
    let request: IDBOpenDBRequest;
    try {
      request = indexedDB.open(DB_NAME, DB_VERSION);
    } catch (err) {
      reject(new LibraryError('open', 'No se pudo abrir IndexedDB.', err));
      return;
    }

    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: 'id' });
      }
    };

    request.onerror = () => {
      reject(
        new LibraryError('open', `No se pudo abrir la base de datos "${DB_NAME}".`, request.error)
      );
    };

    request.onsuccess = () => {
      const db = request.result;
      // Si otra pestaña pide cambiar de versión, cerrar para no bloquearla.
      // El error "IndexedDB no está disponible" desciende como propuesta.
      db.onversionchange = () => {
        db.close();
        if (openPromise === p) openPromise = null;
      };
      resolve(db);
    };
  });

  openPromise = p;
  // Fallo de apertura: no cachear el rechazo para permitir reintento. El
  // error sigue propagándose a quien esperó la promesa original.
  p.catch(() => {
    if (openPromise === p) openPromise = null;
  });

  return openPromise;
}

// Ejecuta un pedido IndexedDB y resuelve sólo cuando la transacción llega
// al evento 'complete': ahí es donde IndexedDB garantiza durabilidad, no en
// el 'onsuccess' del request individual. Única vía de rechazo, para no
// generar promesas con rechazo no observado.
function perform<T>(
  db: IDBDatabase,
  mode: IDBTransactionMode,
  action: (store: IDBObjectStore) => IDBRequest
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let request: IDBRequest;
    let tx: IDBTransaction;
    try {
      tx = db.transaction(STORE_NAME, mode);
      request = action(tx.objectStore(STORE_NAME));
    } catch (err) {
      // Incluir DataCloneError (Blob no cloneable) y errores de sincronía.
      reject(new LibraryError('transaction', 'No se pudo iniciar la transacción IndexedDB.', err));
      return;
    }

    let settled = false;
    const settleError = (error: unknown) => {
      if (settled) return;
      settled = true;
      reject(classifyError(error));
    };

    request.onsuccess = () => {
      if (settled) return;
      const value = request.result as T;
      tx.oncomplete = () => {
        if (settled) return;
        settled = true;
        resolve(value);
      };
      tx.onerror = () => settleError(tx.error);
      tx.onabort = () => settleError(tx.error);
    };
    request.onerror = () => settleError(request.error);
    tx.onabort = () => settleError(tx.error);
  });
}

function classifyError(raw: unknown): LibraryError {
  if (raw instanceof LibraryError) return raw;
  const name =
    raw instanceof DOMException
      ? raw.name
      : raw && typeof raw === 'object'
        ? typeof (raw as { name?: unknown }).name === 'string'
          ? (raw as { name: string }).name
          : ''
        : '';
  switch (name) {
    case 'QuotaExceededError':
      return new LibraryError('quota', 'Se agotó el espacio disponible para la biblioteca local.', raw);
    case 'ConstraintError':
      return new LibraryError(
        'collision',
        'Colisión de identificador: la entrada ya existe y no fue sobrescrita.',
        raw
      );
    case 'DataCloneError':
    case 'InvalidStateError':
      return new LibraryError('invalid-blob', 'El Blob no pudo clonarse dentro de la transacción.', raw);
    default:
      return new LibraryError('transaction', 'Error de transacción IndexedDB.', raw);
  }
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function generateId(): string {
  const c = typeof crypto !== 'undefined' ? crypto : undefined;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  if (!c || typeof c.getRandomValues !== 'function') {
    throw new LibraryError('unavailable', 'crypto no está disponible para generar identificadores.');
  }
  // Fallback UUID v4 con crypto.getRandomValues.
  const bytes = c.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

// Valida la forma de un registro leído de la base de datos. Los registros
// corruptos se informan, no se silencian.
function assertRecordShape(value: unknown): SkinRecord {
  if (typeof value !== 'object' || value === null) {
    throw new LibraryError('corrupt', 'Registro de biblioteca dañado (no es un objeto).');
  }
  const r = value as Partial<SkinRecord>;
  const id = typeof r.id === 'string' ? r.id : '';
  const thumbOk = r.thumbBlob === undefined || r.thumbBlob instanceof Blob;
  if (
    !id ||
    typeof r.name !== 'string' ||
    (r.model !== 'classic' && r.model !== 'slim') ||
    !(r.pngBlob instanceof Blob) ||
    typeof r.createdAt !== 'number' ||
    r.schemaVersion !== 1 ||
    !thumbOk
  ) {
    throw new LibraryError('corrupt', `Registro de biblioteca dañado (id: ${id || '?'}).`);
  }
  return r as SkinRecord;
}

function createIndexedDbStorage(): SkinStorage {
  return {
    async save(record: SkinRecord): Promise<void> {
      // add(), nunca put(): la colisión de id debe producir error, no
      // sobrescribir una entrada anterior.
      const db = await openDatabase();
      await perform<unknown>(db, 'readwrite', (store) => store.add(assertRecordShape(record)));
    },

    async list(): Promise<SkinRecord[]> {
      const db = await openDatabase();
      const records = await perform<SkinRecord[]>(
        db,
        'readonly',
        (store) => store.getAll() as IDBRequest<SkinRecord[]>
      );
      // Blob se recupera como copia (structured clone): sin referencias
      // mutables al buffer activo del editor.
      return records.map(assertRecordShape);
    },

    async getById(id: string): Promise<SkinRecord | null> {
      const db = await openDatabase();
      const found = await perform<SkinRecord | undefined>(db, 'readonly', (store) => store.get(id));
      return found ? assertRecordShape(found) : null;
    },

    async delete(id: string): Promise<boolean> {
      const db = await openDatabase();
      const existing = await perform<SkinRecord | undefined>(db, 'readonly', (store) => store.get(id));
      if (!existing) return false;
      await perform<unknown>(db, 'readwrite', (store) => store.delete(id));
      return true;
    },
  };
}

const defaultStorage: SkinStorage = createIndexedDbStorage();

export { createIndexedDbStorage };

// ---- API pública de la biblioteca ----

/**
 * Guarda una entrada nueva (id generado con crypto.randomUUID).
 * Nunca sobrescribe: usa add(), por lo que una colisión de id falla.
 * Resuelve después del 'complete' de la transacción: guardado confirmado.
 */
export async function saveSkin(
  name: string,
  model: 'classic' | 'slim',
  pngBlob: Blob,
  thumbBlob?: Blob
): Promise<SkinRecord> {
  if (typeof name !== 'string') {
    throw new TypeError('saveSkin: name debe ser string.');
  }
  if (model !== 'classic' && model !== 'slim') {
    throw new TypeError('saveSkin: model debe ser "classic" o "slim".');
  }
  if (!(pngBlob instanceof Blob) || pngBlob.size === 0) {
    throw new LibraryError('invalid-blob', 'saveSkin: pngBlob debe ser un Blob de PNG no vacío.');
  }
  if (thumbBlob !== undefined && (!(thumbBlob instanceof Blob) || thumbBlob.size === 0)) {
    throw new LibraryError('invalid-blob', 'saveSkin: thumbBlob debe ser un Blob de miniatura no vacío.');
  }

  const record: SkinRecord = {
    id: generateId(),
    name,
    model,
    pngBlob,
    createdAt: Date.now(),
    schemaVersion: 1,
  };
  if (thumbBlob !== undefined) record.thumbBlob = thumbBlob;
  await defaultStorage.save(record);
  return record;
}

/**
 * Adjunta (o reemplaza) la miniatura de una entrada existente sin alterar
 * sus datos originales: se reescribe el mismo registro con su thumbnail.
 * No crea entradas nuevas y no toca ningún otro registro.
 */
export async function updateThumb(id: string, thumbBlob: Blob): Promise<void> {
  if (!(thumbBlob instanceof Blob) || thumbBlob.size === 0) {
    throw new LibraryError('invalid-blob', 'updateThumb: thumbBlob debe ser un Blob de miniatura no vacío.');
  }
  const db = await openDatabase();
  const existing = await perform<SkinRecord | undefined>(db, 'readonly', (store) => store.get(id));
  if (!existing) {
    throw new LibraryError('transaction', `updateThumb: no existe la entrada con id ${id}.`);
  }
  const record = assertRecordShape(existing);
  record.thumbBlob = thumbBlob;
  // put() está permitido acá: modifica exactamente esta entrada.
  await perform<unknown>(db, 'readwrite', (store) => store.put(record));
}

/** Recupera todas las entradas. */
export async function loadSkins(): Promise<SkinRecord[]> {
  const records = await defaultStorage.list();
  return records;
}

/** Recupera una entrada concreta, o null si no existe. */
export async function loadSkinById(id: string): Promise<SkinRecord | null> {
  return defaultStorage.getById(id);
}

/** Elimina exclusivamente la entrada indicada; devuelve true si existía. */
export async function deleteSkin(id: string): Promise<boolean> {
  return defaultStorage.delete(id);
}
