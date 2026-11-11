// Superficie de la unión de las cajas exteriores de skinview3d.
//
// Las cajas `outerLayer` de skinview3d 3.4.2 son versiones infladas (+0.25
// por lado) de la capa interior, centradas en el mismo eje. Como las partes
// vecinas quedan a 8 / 12 / 4 unidades en la capa interior (se tocan justo),
// sus cajas exteriores acaban solapándose 0.5 unidades:
//   - piernas contra piernas (franja vertical en la entrepierna),
//   - mangas contra torso (franja lateral de alto completo),
//   - piernas contra torso (banda horizontal de 0.5 en la cintura),
//   - sombrero contra torso/mangas (esquinas del sombrero).
// Donde se solapan hay caras internas y, en las parejas con el frente
// coplanar (piernas entre sí, torso↔mangas), z-fighting.
//
// Este módulo construye, para cada parte cuya capa exterior sea visible, la
// geometría de la superficie de la UNIÓN de las cajas exteriores: cada cara
// de cada caja se recorta contra el volumen de las demás cajas exteriores
// visibles. Ninguna superficie queda envuelta dentro de otra caja.
//
// Hipótesis (documentada): se asume pose de reposo, sin rotaciones en los
// grupos de partes (el visor no anima: `animation = null`). Las AABB se
// calculan a partir de la geometría original y `matrixWorld` (se llama a
// `skin.updateWorldMatrix(true, true)` antes); las inclusiones entre cajas
// se evalúan en el espacio mundo del skin, que difiere del espacio propio
// del skin solo por la transformación rígida compartida de los padres
// (p. ej. el offset y=8 de PlayerObject), que no altera ninguna relación de
// inclusión entre cajas. Rotaciones en las partes (brazos elevados, cabeza
// inclinada...) invalidan el supuesto de cajas alineadas.
//
// Convención de coordenadas devueltas: las posiciones viven en el espacio
// LOCAL del mesh con la escala horneada, es decir, el mesh puede quedar con
// `scale = (1, 1, 1)`: mundo = matrixWorldConEscala1 * local. Al adoptar
// estas geometrías, quien llame debe resetear `mesh.scale.set(1, 1, 1)`.

import { BufferAttribute, BufferGeometry, Matrix4, Vector3 } from 'three';

export type BodyPartId = 'head' | 'body' | 'leftArm' | 'rightArm' | 'leftLeg' | 'rightLeg';
export type OuterVisible = Record<BodyPartId, boolean>;
export interface FaceRect {
  left: number;
  top: number;
  width: number;
  height: number;
}
export type OuterGeometryMap = Map<BodyPartId, BufferGeometry>;

/** Parte mínima de un mesh de three para trabajar sin importar skinview3d. */
export interface OuterMeshLike {
  geometry: {
    attributes: {
      position: { array: ArrayLike<number>; count: number; itemSize: number };
      uv: { array: ArrayLike<number>; count: number; itemSize: number };
      normal: { array: ArrayLike<number>; count: number; itemSize: number };
    };
  };
  matrixWorld: { elements: ArrayLike<number> };
  scale: { x: number; y: number; z: number };
}

/** Parte mínima del SkinObject de skinview3d (viewer.playerObject.skin). */
export interface OuterSkinLike {
  updateWorldMatrix?: (updateParents: boolean, updateChildren: boolean) => void;
  head: { outerLayer: OuterMeshLike };
  body: { outerLayer: OuterMeshLike };
  leftArm: { outerLayer: OuterMeshLike };
  rightArm: { outerLayer: OuterMeshLike };
  leftLeg: { outerLayer: OuterMeshLike };
  rightLeg: { outerLayer: OuterMeshLike };
}

const PARTS: BodyPartId[] = ['head', 'body', 'leftArm', 'rightArm', 'leftLeg', 'rightLeg'];
const EPS = 1e-9;
const COPLANAR_EPS = 1e-6;
const TEXTURE_SIZE = 64;

// Prioridad ante caras coplanares con la misma normal: el torso gana sobre
// brazos y piernas; la cabeza va segunda (no hay pares coplanares reales
// con ella, pero la regla queda definida). Las piernas entre sí se reparten
// por el plano medio x=0 en vez de por rango (ver cellOccluded).
const RANK: Record<BodyPartId, number> = {
  body: 0,
  head: 1,
  leftArm: 2,
  rightArm: 2,
  leftLeg: 3,
  rightLeg: 3,
};

type Vec3 = [number, number, number];
type AABB = { min: Vec3; max: Vec3 };

interface FaceInfo {
  axis: number; // eje de la normal (plano de la cara)
  sign: number; // +1 si la normal apunta al máximo del eje, -1 si al mínimo
  normalLocal: Vec3; // normal original (espacio local del mesh)
  a: number; // primer eje del plano
  b: number; // segundo eje del plano
  aMin: number;
  aMax: number;
  bMin: number;
  bMax: number;
  c: number; // coordenada mundo sobre el plano
  uv00: [number, number]; // (amin, bmin)
  uv10: [number, number]; // (amax, bmin)
  uv01: [number, number]; // (amin, bmax)
  uv11: [number, number]; // (amax, bmax)
  rect: FaceRect;
}

function isLeg(p: BodyPartId): boolean {
  return p === 'leftLeg' || p === 'rightLeg';
}

function isLegPair(a: BodyPartId, b: BodyPartId): boolean {
  return isLeg(a) && isLeg(b);
}

/** AABB mundo de un mesh a partir de sus vértices locales y matrixWorld. */
function meshWorldBox(mesh: OuterMeshLike, m: Matrix4): AABB {
  const pos = mesh.geometry.attributes.position;
  const min: Vec3 = [Infinity, Infinity, Infinity];
  const max: Vec3 = [-Infinity, -Infinity, -Infinity];
  const v = new Vector3();
  for (let i = 0; i < pos.count; i++) {
    v.set(pos.array[i * 3], pos.array[i * 3 + 1], pos.array[i * 3 + 2]).applyMatrix4(m);
    for (let k = 0; k < 3; k++) {
      const c = v.getComponent(k);
      if (c < min[k]) min[k] = c;
      if (c > max[k]) max[k] = c;
    }
  }
  return { min, max };
}

function worldToLocalScaled(inv: Matrix4, scale: { x: number; y: number; z: number }, w: Vec3): Vec3 {
  const v = new Vector3(w[0], w[1], w[2]).applyMatrix4(inv);
  return [v.x * scale.x, v.y * scale.y, v.z * scale.z];
}

/** Agrupa los vértices de la caja original por normal y describe cada cara. */
function extractFaces(mesh: OuterMeshLike, m: Matrix4): FaceInfo[] {
  const pos = mesh.geometry.attributes.position;
  const uvA = mesh.geometry.attributes.uv;
  const nrA = mesh.geometry.attributes.normal;

  const byNormal = new Map<string, number[]>();
  for (let i = 0; i < pos.count; i++) {
    const key = `${Math.round(nrA.array[i * 3])},${Math.round(nrA.array[i * 3 + 1])},${Math.round(nrA.array[i * 3 + 2])}`;
    const list = byNormal.get(key);
    if (list) list.push(i);
    else byNormal.set(key, [i]);
  }

  const faces: FaceInfo[] = [];
  for (const [key, idx] of byNormal) {
    if (idx.length < 3) continue;
    const nLocal = key.split(',').map(Number) as Vec3;
    let axis = 0;
    for (let k = 1; k < 3; k++) if (Math.abs(nLocal[k]) > Math.abs(nLocal[axis])) axis = k;
    const sign = nLocal[axis] >= 0 ? 1 : -1;
    const axes: number[] = [];
    for (let k = 0; k < 3; k++) if (k !== axis) axes.push(k);
    const a = axes[0];
    const b = axes[1];

    const corners: { a: number; b: number; uv: [number, number] }[] = [];
    const cVals: number[] = [];
    for (const i of idx) {
      const v = new Vector3(pos.array[i * 3], pos.array[i * 3 + 1], pos.array[i * 3 + 2]).applyMatrix4(m);
      corners.push({ a: v.getComponent(a), b: v.getComponent(b), uv: [uvA.array[i * 2], uvA.array[i * 2 + 1]] });
      cVals.push(v.getComponent(axis));
    }
    const c = cVals[0];
    const aMin = Math.min(...corners.map((c2) => c2.a));
    const aMax = Math.max(...corners.map((c2) => c2.a));
    const bMin = Math.min(...corners.map((c2) => c2.b));
    const bMax = Math.max(...corners.map((c2) => c2.b));

    const findCorner = (a: number, b: number): [number, number] | null => {
      for (const corner of corners) {
        if (Math.abs(corner.a - a) < 1e-6 && Math.abs(corner.b - b) < 1e-6) return corner.uv;
      }
      return null;
    };
    const uv00 = findCorner(aMin, bMin);
    const uv10 = findCorner(aMax, bMin);
    const uv01 = findCorner(aMin, bMax);
    const uv11 = findCorner(aMax, bMax);
    if (!uv00 || !uv10 || !uv01 || !uv11) continue; // no es una caja: se omite

    const minU = Math.min(uv00[0], uv10[0], uv01[0], uv11[0]);
    const maxU = Math.max(uv00[0], uv10[0], uv01[0], uv11[0]);
    const minV = Math.min(uv00[1], uv10[1], uv01[1], uv11[1]);
    const maxV = Math.max(uv00[1], uv10[1], uv01[1], uv11[1]);
    const left = Math.round(minU * TEXTURE_SIZE);
    const rightEx = Math.round(maxU * TEXTURE_SIZE);
    const top = Math.round((1 - maxV) * TEXTURE_SIZE);
    const bottomEx = Math.round((1 - minV) * TEXTURE_SIZE);

    faces.push({
      axis,
      sign,
      normalLocal: nLocal,
      a,
      b,
      aMin,
      aMax,
      bMin,
      bMax,
      c,
      uv00,
      uv10,
      uv01,
      uv11,
      rect: { left, top, width: rightEx - left, height: bottomEx - top },
    });
  }
  return faces;
}

/** UV afín dentro de la cara: interpolación bilineal entre las 4 esquinas. */
function bilinearUv(face: FaceInfo, a: number, b: number): [number, number] {
  const fu = face.aMax - face.aMin > EPS ? (a - face.aMin) / (face.aMax - face.aMin) : 0;
  const fv = face.bMax - face.bMin > EPS ? (b - face.bMin) / (face.bMax - face.bMin) : 0;
  const u =
    face.uv00[0] * (1 - fu) * (1 - fv) +
    face.uv10[0] * fu * (1 - fv) +
    face.uv01[0] * (1 - fu) * fv +
    face.uv11[0] * fu * fv;
  const v =
    face.uv00[1] * (1 - fu) * (1 - fv) +
    face.uv10[1] * fu * (1 - fv) +
    face.uv01[1] * (1 - fu) * fv +
    face.uv11[1] * fu * fv;
  return [u, v];
}

/**
 * Construye las geometrías de la unión de las cajas exteriores visibles.
 * El mapa devuelto solo contiene entradas para las partes visibles; una
 * parte no visible no recorta a las demás.
 */
export function buildOuterGeometries(skin: OuterSkinLike, visible: OuterVisible): OuterGeometryMap {
  const result = new Map<BodyPartId, BufferGeometry>();
  skin.updateWorldMatrix?.(true, true);

  const meshes = new Map<BodyPartId, OuterMeshLike>();
  const boxes = new Map<BodyPartId, AABB>();
  const matrices = new Map<BodyPartId, Matrix4>();
  const inverses = new Map<BodyPartId, Matrix4>();

  for (const part of PARTS) {
    const mesh = skin[part].outerLayer;
    meshes.set(part, mesh);
    const m = new Matrix4().fromArray(mesh.matrixWorld.elements);
    matrices.set(part, m);
    inverses.set(part, new Matrix4().copy(m).invert());
    boxes.set(part, meshWorldBox(mesh, m));
  }

  const visibleParts = PARTS.filter((p) => visible[p]);

  for (const part of PARTS) {
    if (!visible[part]) continue;
    const mesh = meshes.get(part)!;
    const box = boxes.get(part)!;
    const inv = inverses.get(part)!;
    const occluders = visibleParts
      .filter((q) => q !== part)
      .map((q) => ({ id: q, box: boxes.get(q)! }));
    const myCenterX = (box.min[0] + box.max[0]) / 2;

    const faces = extractFaces(mesh, matrices.get(part)!);
    const positions: number[] = [];
    const normals: number[] = [];
    const uvs: number[] = [];
    const indices: number[] = [];
    const faceRects: FaceRect[] = [];

    const pointOnPlane = (face: FaceInfo, a: number, b: number): Vec3 => {
      const p: Vec3 = [0, 0, 0];
      p[face.axis] = face.c;
      p[face.a] = a;
      p[face.b] = b;
      return p;
    };

    const cellOccluded = (face: FaceInfo, ca: number, cb: number): boolean => {
      for (const { id, box: ob } of occluders) {
        // 1) volumen estricto: el centro cae dentro de la otra caja
        const inNormal = face.c > ob.min[face.axis] && face.c < ob.max[face.axis];
        if (
          inNormal &&
          ca > ob.min[face.a] &&
          ca < ob.max[face.a] &&
          cb > ob.min[face.b] &&
          cb < ob.max[face.b]
        ) {
          return true;
        }
        // 2) caras coplanares con la misma normal: decide la prioridad
        const planeCoord = face.sign > 0 ? ob.max[face.axis] : ob.min[face.axis];
        if (Math.abs(face.c - planeCoord) <= COPLANAR_EPS) {
          const inFootprint =
            ca >= ob.min[face.a] - EPS &&
            ca <= ob.max[face.a] + EPS &&
            cb >= ob.min[face.b] - EPS &&
            cb <= ob.max[face.b] + EPS;
          if (!inFootprint) continue;
          if (isLegPair(part, id)) {
            // cada pierna conserva su mitad hasta el plano medio x=0
            const center: Vec3 = [0, 0, 0];
            center[face.axis] = face.c;
            center[face.a] = ca;
            center[face.b] = cb;
            const mySide = myCenterX < 0; // pierna derecha (centro x<0)
            if ((mySide && center[0] > 0) || (!mySide && center[0] < 0)) return true;
          } else if (RANK[id] < RANK[part]) {
            return true; // el otro gana el empate
          }
          // si yo gano, conservo la región (el otro la pierde en su pasada)
        }
      }
      return false;
    };

    for (const face of faces) {
      // Grilla de cortes compartida: los extremos de todas las AABB en los
      // dos ejes del plano; así los fragmentos vecinos comparten vértices y
      // no aparecen T-junctions entre fragmentos de la misma cara.
      const cutA = new Set<number>([face.aMin, face.aMax]);
      const cutB = new Set<number>([face.bMin, face.bMax]);
      for (const { box: ob } of occluders) {
        cutA.add(ob.min[face.a]);
        cutA.add(ob.max[face.a]);
        cutB.add(ob.min[face.b]);
        cutB.add(ob.max[face.b]);
      }
      if (isLeg(part) && (face.a === 0 || face.b === 0)) {
        // corte en el plano medio para el reparto pierna/pierna
        if (face.a === 0) cutA.add(0);
        else cutB.add(0);
      }
      const dedupeSort = (vals: Set<number>, lo: number, hi: number): number[] => {
        const arr = [...vals]
          .filter((v) => v >= lo - EPS && v <= hi + EPS)
          .sort((x, y) => x - y);
        const out: number[] = [];
        for (const v of arr) {
          if (!out.length || Math.abs(v - out[out.length - 1]) > EPS) out.push(v);
        }
        return out;
      };
      const A = dedupeSort(cutA, face.aMin, face.aMax);
      const B = dedupeSort(cutB, face.bMin, face.bMax);

      for (let i = 0; i + 1 < A.length; i++) {
        for (let j = 0; j + 1 < B.length; j++) {
          const a0 = A[i];
          const a1 = A[i + 1];
          const b0 = B[j];
          const b1 = B[j + 1];
          if (a1 - a0 < EPS || b1 - b0 < EPS) continue;
          const ca = (a0 + a1) / 2;
          const cb = (b0 + b1) / 2;
          if (cellOccluded(face, ca, cb)) continue;

          const corners: [number, number][] = [
            [a0, b0],
            [a1, b0],
            [a1, b1],
            [a0, b1],
          ];
          const worldPts = corners.map(([x, y]) => pointOnPlane(face, x, y));
          const base = positions.length / 3;
          for (let k = 0; k < 4; k++) {
            const [cx, cy] = corners[k];
            const local = worldToLocalScaled(inv, mesh.scale, worldPts[k]);
            positions.push(local[0], local[1], local[2]);
            normals.push(face.normalLocal[0], face.normalLocal[1], face.normalLocal[2]);
            const uv = bilinearUv(face, cx, cy);
            uvs.push(uv[0], uv[1]);
          }
          // Winding: la normal del triángulo debe coincidir con la cara.
          const e1 = new Vector3(
            worldPts[1][0] - worldPts[0][0],
            worldPts[1][1] - worldPts[0][1],
            worldPts[1][2] - worldPts[0][2]
          );
          const e2 = new Vector3(
            worldPts[3][0] - worldPts[0][0],
            worldPts[3][1] - worldPts[0][1],
            worldPts[3][2] - worldPts[0][2]
          );
          const nrm = e1.cross(e2);
          const ref: Vec3 = [0, 0, 0];
          ref[face.axis] = face.sign;
          const dot = nrm.x * ref[0] + nrm.y * ref[1] + nrm.z * ref[2];
          if (dot >= 0) {
            indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
          } else {
            indices.push(base, base + 2, base + 1, base, base + 3, base + 2);
          }
          faceRects.push(face.rect, face.rect);
        }
      }
    }

    const geo = new BufferGeometry();
    geo.setAttribute('position', new BufferAttribute(new Float32Array(positions), 3));
    geo.setAttribute('normal', new BufferAttribute(new Float32Array(normals), 3));
    geo.setAttribute('uv', new BufferAttribute(new Float32Array(uvs), 2));
    geo.setIndex(indices);
    geo.userData.faceRects = faceRects;
    result.set(part, geo);
  }
  return result;
}

/** Libera las geometrías devueltas por buildOuterGeometries y vacía el mapa. */
export function disposeOuterGeometries(map: OuterGeometryMap): void {
  map.forEach((geo) => geo.dispose());
  map.clear();
}

/** Clave de caché determinista para una (modelo, visibilidad) dada. */
export function outerKey(modelType: string, visible: OuterVisible): string {
  const parts = PARTS.filter((p) => visible[p]).join('.');
  return `outer:${modelType}:${parts}`;
}
