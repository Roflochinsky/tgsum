// Generative night painting behind the app, ported from the author's site
// (roflochinsky.github.io, src/scripts/paint.ts): TypeScript types stripped,
// logic and comments unchanged; the `app` preset is added for tgsum.
// Генеративная живопись в духе Ван Гога. Готовых картинок нет: небо, вихри,
// звёзды, холмы и кипарис складываются из коротких мазков прямо в браузере.
//
// Как устроено. У холста есть векторное поле: волнистый «ветер», вихри,
// кольца вокруг звёзд, контуры холмов, пламя кипариса. Мазок кладётся вдоль
// этого поля, цвет берётся из палитры по месту на холсте. Каждый мазок
// рисуется трижды: тёмная подложка (щели между мазками), краска и светлый
// гребень посередине — так плоская заливка читается как пастозная живопись.
//
// Один и тот же seed на одном размере даёт ту же картину. Холст пишется
// порциями по кадрам, чтобы не держать основной поток, и только когда
// подъезжает к экрану. Живой слой (ветер и мерцание) работает, только пока
// холст виден, и выключается при prefers-reduced-motion.
const TAU = Math.PI * 2;
const NIGHT = {
  bg: ['#0c1a4d', '#1d3f80'],
  sky: ['#0b1a50', '#112566', '#17317c', '#1e3f92', '#274fa4', '#3261b2', '#4176be', '#588cc8', '#78a6d4'],
  swirl: ['#9fc3e4', '#c4dcec', '#e6eed6', '#7eb0dc', '#b3d6d6'],
  glow: ['#fff5c4', '#fde68a', '#f8d34f', '#f1bb34', '#e9a126'],
  ground: ['#0a1430', '#101f44', '#172c56', '#203c62', '#2a4d68', '#1a3450'],
  tree: ['#06100c', '#0c1a13', '#13261a', '#1d3423', '#2a3f23', '#35301a'],
};
const DARK_TREE = ['#0d1a12', '#15281b', '#1f3824', '#2c4a2c', '#3a3a1e'];
const PRESETS = {
  // большой ночной пейзаж: герой и финальный блок
  night: {
    pal: NIGHT,
    stars: [6, 9],
    starSize: 0.016,
    orb: 'moon',
    tree: true,
    horizon: 0.8,
    vortices: [3, 4],
    clear: [0.2, 0.16, 0.8, 0.8],
  },
  // tgsum: the same night behind the app; its text column is wide, so the
  // stars keep to the sides, where the facts about the app sit under them
  app: {
    pal: NIGHT,
    stars: [9, 12],
    starSize: 0.016,
    orb: 'moon',
    tree: true,
    horizon: 0.8,
    vortices: [3, 4],
    clear: [0.22, 0.1, 0.78, 0.92],
  },
  // ночное небо без земли: под портретом и в шапках страниц
  sky: {
    pal: NIGHT,
    stars: [4, 6],
    starSize: 0.022,
    orb: null,
    tree: false,
    horizon: 0,
    vortices: [2, 3],
  },
  wheat: {
    pal: {
      bg: ['#2d5fa6', '#d8a23a'],
      sky: ['#1f4a88', '#2a5ba2', '#376bb2', '#4a7fbf', '#6395cb', '#84acd6'],
      swirl: ['#b9d3ea', '#dfe9ee', '#f2efd8', '#9ec0e0'],
      glow: ['#fff3b8', '#fde07a', '#f6c642', '#eda92a', '#d98a1c'],
      ground: ['#8e5a14', '#b37a1e', '#cf9326', '#e2ac35', '#efc452', '#f5d777', '#9aa03a'],
      tree: DARK_TREE,
    },
    stars: [0, 0],
    starSize: 0.02,
    orb: 'sun',
    tree: false,
    horizon: 0.52,
    vortices: [2, 3],
  },
  iris: {
    pal: {
      bg: ['#3d6d58', '#3e3c8f'],
      sky: ['#2b5646', '#376852', '#467b5c', '#588d68', '#74a377', '#98bb88'],
      swirl: ['#c7d9a6', '#e3e7c0', '#a9c98f'],
      glow: ['#fbf6e4', '#f3ecd0', '#e8d98a', '#e0c262'],
      ground: ['#221d66', '#2d2882', '#3b369c', '#4f45b0', '#6858c2', '#8876d0', '#2b4e8c'],
      tree: DARK_TREE,
    },
    stars: [3, 5],
    starSize: 0.018,
    orb: null,
    tree: false,
    horizon: 0.44,
    vortices: [2, 3],
  },
  sunflower: {
    pal: {
      bg: ['#2b8a86', '#d99a26'],
      sky: ['#1d6a6e', '#257b7b', '#308c87', '#429d93', '#5daea0', '#82c1af'],
      swirl: ['#b8e0cf', '#dff0e1', '#9fd2c2'],
      glow: ['#fff1a6', '#fcd95c', '#f6bd2e', '#e99a1d', '#c9721a'],
      ground: ['#7a4a12', '#a4631a', '#c98222', '#e3a02c', '#f0bd3c', '#f6d35e', '#b0561b'],
      tree: DARK_TREE,
    },
    stars: [0, 0],
    starSize: 0.02,
    orb: 'sun',
    tree: false,
    horizon: 0.6,
    vortices: [2, 3],
  },
  almond: {
    pal: {
      bg: ['#4f9fb0', '#9fcfd0'],
      sky: ['#357f95', '#4392a6', '#56a4b5', '#6fb6c3', '#8cc6cf', '#abd6d8'],
      swirl: ['#d7ece8', '#eef6f1', '#c3e1df'],
      glow: ['#fffaf4', '#f9ecec', '#f3d9df', '#f7e7c9', '#eec3ca'],
      ground: ['#3a2a1f', '#4a3526', '#5b4330'],
      tree: DARK_TREE,
    },
    stars: [6, 9],
    starSize: 0.022,
    orb: null,
    tree: false,
    horizon: 0,
    vortices: [3, 4],
  },
};
// ---------- случайность и шум ----------
function hashStr(s) {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
function mulberry32(seed) {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
function valueNoise(rand) {
  const perm = new Uint8Array(512);
  const val = new Float32Array(256);
  for (let i = 0; i < 256; i++) {
    perm[i] = i;
    val[i] = rand();
  }
  for (let i = 255; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    const t = perm[i];
    perm[i] = perm[j];
    perm[j] = t;
  }
  for (let i = 0; i < 256; i++) perm[i + 256] = perm[i];
  const at = (x, y) => val[perm[perm[x & 255] + (y & 255)]];
  const s = (t) => t * t * (3 - 2 * t);
  return (x, y) => {
    const xi = Math.floor(x),
      yi = Math.floor(y);
    const u = s(x - xi),
      v = s(y - yi);
    const a = at(xi, yi),
      b = at(xi + 1, yi),
      c = at(xi, yi + 1),
      d = at(xi + 1, yi + 1);
    return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
  };
}
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const lerp = (a, b, t) => a + (b - a) * t;
function pick(arr, t, rand) {
  const i = Math.round(clamp(t, 0, 1) * (arr.length - 1) + (rand() - 0.5) * 1.3);
  return arr[clamp(i, 0, arr.length - 1)];
}
function mixAngle(a, b, w) {
  return Math.atan2(lerp(Math.sin(a), Math.sin(b), w), lerp(Math.cos(a), Math.cos(b), w));
}
// Тень и блик для каждого цвета считаются один раз.
const shadeCache = new Map();
function shades(hex) {
  let s = shadeCache.get(hex);
  if (!s) {
    const n = parseInt(hex.slice(1), 16);
    const r = n >> 16,
      g = (n >> 8) & 255,
      b = n & 255;
    const mix = (t, to) =>
      `rgb(${Math.round(lerp(r, to[0], t))},${Math.round(lerp(g, to[1], t))},${Math.round(lerp(b, to[2], t))})`;
    s = [mix(0.5, [2, 6, 20]), mix(0.45, [255, 255, 250])];
    shadeCache.set(hex, s);
  }
  return s;
}
function buildModel(p, W, H, rand) {
  const S = Math.min(W, H);
  const ph = [rand() * TAU, rand() * TAU, rand() * TAU, rand() * TAU];
  const noise = valueNoise(rand);
  const skyH = p.horizon ? H * p.horizon : H;
  const wide = W / H > 1.6 ? 1.2 : 1;
  const vort = [];
  const nv = p.vortices[0] + Math.floor(rand() * (p.vortices[1] - p.vortices[0] + 1));
  for (let i = 0; i < nv; i++) {
    const main = i === 0;
    vort.push({
      x: main ? W * lerp(0.32, 0.6, rand()) : W * rand(),
      y: main ? skyH * lerp(0.3, 0.46, rand()) : skyH * lerp(0.12, 0.85, rand()),
      r: S * (main ? lerp(0.22, 0.3, rand()) * wide : lerp(0.1, 0.18, rand())),
      k: (i % 2 ? -1 : 1) * lerp(0.75, 1.1, rand()),
    });
  }
  // Кипарис и луна по разные стороны, чтобы не спорили за один угол.
  const treeLeft = rand() < 0.5;
  const tree = p.tree
    ? {
        x: W * (treeLeft ? lerp(0.06, 0.12, rand()) : lerp(0.88, 0.94, rand())),
        top: H * lerp(0.14, 0.26, rand()),
        w: Math.max(S * 0.06, 16),
        ph: rand() * TAU,
      }
    : null;
  const orbs = [];
  if (p.orb === 'moon') {
    const core = S * 0.042;
    orbs.push({
      x: W * (treeLeft ? lerp(0.84, 0.9, rand()) : lerp(0.1, 0.16, rand())),
      y: skyH * lerp(0.14, 0.22, rand()),
      core,
      halo: core * 3.4,
      rings: 4,
      kind: 'moon',
    });
  } else if (p.orb === 'sun') {
    const core = S * 0.085;
    orbs.push({
      x: W * lerp(0.62, 0.82, rand()),
      y: skyH * lerp(0.3, 0.44, rand()),
      core,
      halo: core * 2.8,
      rings: 4,
      kind: 'sun',
    });
  }
  const ns = p.stars[0] + Math.floor(rand() * (p.stars[1] - p.stars[0] + 1));
  for (let i = 0, tries = 0; i < ns && tries < 400; tries++) {
    const core = S * p.starSize * lerp(0.7, 1.3, rand());
    const halo = core * lerp(3.2, 4.4, rand());
    const x = W * lerp(0.04, 0.96, rand());
    const y = skyH * lerp(0.06, 0.82, rand());
    const c = p.clear;
    if (c && x > W * c[0] && x < W * c[2] && y > H * c[1] && y < H * c[3]) continue;
    if (tree && Math.abs(x - tree.x) < tree.w + halo && y > tree.top - halo) continue;
    if (orbs.some((o) => Math.hypot(o.x - x, o.y - y) < (o.halo + halo) * 1.05)) continue;
    orbs.push({ x, y, core, halo, rings: 3, kind: 'star' });
    i++;
  }
  // Шаг сетки мазков в CSS-пикселях: крупнее на больших холстах, но с потолком,
  // чтобы 4K-экран не получил сотню тысяч мазков. Нижняя граница держит
  // телефон: на экране 390×844 выходит ~12 тысяч мазков, а не ~18.
  const sp = clamp(Math.sqrt(W * H) / 120, 5.4, 12);
  return { W, H, S, sp, p, vort, orbs, tree, ph, noise };
}
function horizonY(m, x) {
  if (!m.p.horizon) return Infinity;
  const u = x / m.W;
  return (
    m.H *
    (m.p.horizon +
      0.035 * Math.sin(u * Math.PI * 2.1 + m.ph[0]) +
      0.018 * Math.sin(u * Math.PI * 5.3 + m.ph[1]))
  );
}
function horizonSlope(m, x) {
  const u = x / m.W;
  return (
    (m.H / m.W) *
    (0.035 * Math.PI * 2.1 * Math.cos(u * Math.PI * 2.1 + m.ph[0]) +
      0.018 * Math.PI * 5.3 * Math.cos(u * Math.PI * 5.3 + m.ph[1]))
  );
}
/** Угол мазка внутри кипариса или null, если точка снаружи. */
function treeAngle(m, x, y) {
  const tr = m.tree;
  if (!tr || y < tr.top) return null;
  const t = (y - tr.top) / (m.H - tr.top);
  const cx = tr.x + Math.sin(t * 7 + tr.ph) * tr.w * 0.22 * (1 - t * 0.4);
  const hw =
    tr.w *
    Math.pow(Math.sin(Math.min(t, 1) * Math.PI * 0.62), 0.85) *
    (1 + 0.14 * Math.sin(y / (tr.w * 0.9) + tr.ph));
  const dx = x - cx;
  if (Math.abs(dx) > hw) return null;
  return -Math.PI / 2 + 0.6 * Math.sin(t * 13 + (dx / tr.w) * 2.4 + tr.ph);
}
function angleAt(m, x, y) {
  const { W, H, S, ph } = m;
  const ta = treeAngle(m, x, y);
  if (ta !== null) return ta;
  const hz = horizonY(m, x);
  if (y > hz) {
    const depth = Math.min(1, (y - hz) / Math.max(1, H - hz));
    return (
      Math.atan(horizonSlope(m, x) * (1 - depth * 0.6)) + (m.noise(x / (S * 0.1), y / (S * 0.1)) - 0.5) * 0.9
    );
  }
  let vx = 1;
  let vy = 0.32 * Math.sin((x / W) * TAU * 1.1 + ph[2] + (y / H) * 2.6);
  for (const v of m.vort) {
    const dx = x - v.x,
      dy = y - v.y,
      d2 = dx * dx + dy * dy;
    const f = Math.exp(-d2 / (v.r * v.r)) * v.k * 2.6;
    const d = Math.sqrt(d2) + 1e-3;
    vx += (-dy / d) * f;
    vy += (dx / d) * f;
  }
  let a = Math.atan2(vy, vx) + (m.noise(x / (S * 0.22), y / (S * 0.22)) - 0.5) * 0.9;
  for (const o of m.orbs) {
    const dx = x - o.x,
      dy = y - o.y,
      d = Math.hypot(dx, dy);
    if (d > o.halo * 1.25) continue;
    const r = Math.atan2(dy, dx);
    if (d < o.core) return r; // ядро: лучи наружу
    a = mixAngle(a, r + Math.PI / 2, Math.min(1, (o.halo * 1.25 - d) / (o.halo * 0.45)));
  }
  return a;
}
const Kind = { Sky: 0, Ground: 1, Halo: 2, Core: 3, Tree: 4 };
function paintAt(m, x, y, rand) {
  const P = m.p.pal;
  if (treeAngle(m, x, y) !== null) return [pick(P.tree, rand() * 0.85, rand), Kind.Tree];
  const hz = horizonY(m, x);
  if (y > hz) {
    const depth = Math.min(1, (y - hz) / Math.max(1, m.H - hz));
    const n = m.noise(x / (m.S * 0.08), y / (m.S * 0.08));
    return [pick(P.ground, depth * 0.35 + n * 0.65, rand), Kind.Ground];
  }
  for (const o of m.orbs) {
    const d = Math.hypot(x - o.x, y - o.y);
    if (d < o.core) {
      // луна — серп: внутренний круг темнее и теплее
      if (o.kind === 'moon' && Math.hypot(x - o.x - o.core * 0.38, y - o.y + o.core * 0.22) < o.core * 0.8) {
        return [pick(P.glow, 0.85, rand), Kind.Core];
      }
      return [pick(P.glow, rand() * 0.55, rand), Kind.Core];
    }
    if (d < o.halo) {
      const u = (d - o.core) / (o.halo - o.core);
      if (rand() < 1.05 - u * 0.7) {
        const odd = Math.floor(u * o.rings * 2) % 2 === 1;
        if (o.kind === 'sun') return [pick(P.glow, odd ? rand() * 0.3 : 0.4 + u * 0.6, rand), Kind.Halo];
        return [odd ? pick(P.swirl, rand(), rand) : pick(P.glow, 0.1 + u * 0.7, rand), Kind.Halo];
      }
    }
  }
  let best = 0;
  let odd = false;
  for (const v of m.vort) {
    const d = Math.hypot(x - v.x, y - v.y);
    const f = Math.exp(-(d * d) / (v.r * v.r));
    if (f > best) {
      best = f;
      odd = Math.floor(d / (v.r * 0.2)) % 2 === 1;
    }
  }
  if (best > 0.18 && rand() < best * 1.15) {
    return [odd ? pick(P.swirl, rand(), rand) : pick(P.sky, 0.55 + rand() * 0.4, rand), Kind.Sky];
  }
  const t = Math.min(1, y / (Number.isFinite(hz) ? hz : m.H));
  const n = m.noise(x / (m.S * 0.25), y / (m.S * 0.25));
  return [pick(P.sky, 0.08 + t * 0.5 + (n - 0.5) * 0.7, rand), Kind.Sky];
}
/** Мазок — короткая ломаная вдоль поля, центрированная на своей точке. */
function trace(m, x, y, len) {
  const n = 3,
    seg = len / n;
  const a = angleAt(m, x, y);
  let dx = Math.cos(a),
    dy = Math.sin(a);
  let px = x - (dx * len) / 2,
    py = y - (dy * len) / 2;
  const pts = new Float32Array(2 * (n + 1));
  pts[0] = px;
  pts[1] = py;
  for (let i = 1; i <= n; i++) {
    const b = angleAt(m, px, py);
    let nx = Math.cos(b),
      ny = Math.sin(b);
    // у мазка нет направления: не даём ему развернуться на месте
    if (nx * dx + ny * dy < 0) {
      nx = -nx;
      ny = -ny;
    }
    px += nx * seg;
    py += ny * seg;
    dx = nx;
    dy = ny;
    pts[2 * i] = px;
    pts[2 * i + 1] = py;
  }
  return pts;
}
function shuffle(a, rand) {
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    const t = a[i];
    a[i] = a[j];
    a[j] = t;
  }
}
/** Мазки по рядам сетки. Генератор отдаёт управление после каждого ряда,
 *  чтобы подготовку, как и рисование, можно было растянуть на несколько кадров:
 *  на холст героя это десятки тысяч вызовов поля, одним куском — заметный
 *  подвис прокрутки, особенно на телефоне. */
function* makeStrokes(m, rand, out) {
  const base = [];
  const top = [];
  const { sp, W, H } = m;
  const step = sp * 0.92;
  for (let gy = -sp; gy < H + sp; gy += step) {
    for (let gx = -sp; gx < W + sp; gx += step) {
      const x = gx + (rand() - 0.5) * sp;
      const y = gy + (rand() - 0.5) * sp;
      const [c, k] = paintAt(m, x, y, rand);
      let len = sp * (2.3 + rand() * 1.3);
      let w = sp * (0.72 + rand() * 0.34);
      if (k === Kind.Core) {
        len *= 0.6;
        w *= 0.9;
      } else if (k === Kind.Halo) len *= 0.8;
      else if (k === Kind.Tree) {
        len *= 1.25;
        w *= 0.85;
      }
      (k === Kind.Core || k === Kind.Tree ? top : base).push({ pts: trace(m, x, y, len), w, c });
    }
    yield;
  }
  // Случайный порядок — иначе мазки ложатся рядами и видна сетка.
  shuffle(base, rand);
  shuffle(top, rand);
  out.push(...base, ...top);
}
function drawStroke(ctx, s) {
  const p = s.pts;
  ctx.beginPath();
  ctx.moveTo(p[0], p[1]);
  for (let i = 2; i < p.length; i += 2) ctx.lineTo(p[i], p[i + 1]);
  const [dark, light] = shades(s.c);
  ctx.globalAlpha = 0.55;
  ctx.strokeStyle = dark;
  ctx.lineWidth = s.w * 1.28;
  ctx.stroke();
  ctx.globalAlpha = 0.97;
  ctx.strokeStyle = s.c;
  ctx.lineWidth = s.w;
  ctx.stroke();
  ctx.globalAlpha = 0.32;
  ctx.strokeStyle = light;
  ctx.lineWidth = s.w * 0.3;
  ctx.stroke();
}
// ---------- мерцание звёзд ----------
// Только CSS-пятна света поверх готового холста: анимация прозрачности идёт
// на композиторе и не трогает основной поток. Никакой покадровой отрисовки.
function addTwinkles(cv, m) {
  const host = cv.parentElement;
  host.querySelector(':scope > .paint-stars')?.remove();
  const stars = document.createElement('div');
  stars.className = 'paint-stars';
  stars.setAttribute('aria-hidden', 'true');
  for (const o of m.orbs) {
    if (o.kind === 'sun') continue;
    const el = document.createElement('span');
    el.className = 'twinkle';
    const size = o.halo * 1.5;
    el.style.cssText =
      `left:${(o.x / m.W) * 100}%;top:${(o.y / m.H) * 100}%;width:${size}px;height:${size}px;` +
      `--dur:${(2.6 + Math.random() * 2.6).toFixed(2)}s;--delay:${(-Math.random() * 5).toFixed(2)}s`;
    stars.append(el);
  }
  cv.after(stars);
}
const jobs = [];
let pumping = false;
function pump() {
  const deadline = performance.now() + 8;
  for (let k = jobs.length - 1; k >= 0; k--) if (!jobs[k].alive()) jobs.splice(k, 1);
  // сначала кладём мазки на открытые холсты, потом в остаток кадра готовим следующие
  for (let k = 0; k < jobs.length; k++) {
    const j = jobs[k];
    if (j.ready && j.started() && j.draw(deadline)) {
      jobs.splice(k, 1);
      k--;
    }
  }
  for (const j of jobs) {
    if (performance.now() >= deadline) break;
    if (!j.ready) j.prepare(deadline);
  }
  // Готовые, но ещё закрытые холсты очередь не крутят: их запустит kick().
  if (jobs.some((j) => !j.ready || j.started())) requestAnimationFrame(pump);
  else pumping = false;
}
function kick() {
  if (!pumping && jobs.length) {
    pumping = true;
    requestAnimationFrame(pump);
  }
}
const states = new WeakMap();
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
/** Раздел открыт? Холсты вне разделов (шапки блога, 404) открыты всегда. */
function isOpen(cv) {
  const sec = cv.closest('[data-section]');
  return !sec || sec.classList.contains('is-played');
}
function render(cv, st) {
  const box = cv.getBoundingClientRect();
  const W = Math.round(box.width),
    H = Math.round(box.height);
  if (W < 2 || H < 2) return;
  st.W = W;
  st.H = H;
  const token = ++st.token;
  cv.classList.remove('is-done');
  // 1.5 вместо полной плотности экрана: мазки мягкие, разницы не видно,
  // а холст на весь экран 4K весил бы вчетверо больше.
  const dpr = Math.min(devicePixelRatio || 1, 1.5);
  cv.width = Math.round(W * dpr);
  cv.height = Math.round(H * dpr);
  const ctx = cv.getContext('2d');
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  const name = cv.dataset.preset && PRESETS[cv.dataset.preset] ? cv.dataset.preset : 'night';
  const preset = PRESETS[name];
  const rand = mulberry32(hashStr(`${cv.dataset.seed ?? ''}|${name}`));
  const m = buildModel(preset, W, H, rand);
  // Где встали звёзды и луна — для цифр поверх холста на первом экране.
  cv.dispatchEvent(
    new CustomEvent('paint:model', {
      bubbles: true,
      detail: { W, H, orbs: m.orbs.map(({ x, y, halo, kind }) => ({ x, y, halo, kind })) },
    }),
  );
  const strokes = [];
  const gen = makeStrokes(m, rand, strokes);
  const duration = reduced.matches ? 0 : Number(cv.dataset.duration) || 2000;
  let i = 0,
    first = true,
    perFrame = 0;
  jobs.push({
    alive: () => token === st.token,
    started: () => isOpen(cv),
    ready: false,
    prepare(deadline) {
      while (performance.now() < deadline) {
        if (gen.next().done) {
          this.ready = true;
          break;
        }
      }
    },
    draw(deadline) {
      if (first) {
        first = false;
        const g = ctx.createLinearGradient(0, 0, 0, H);
        g.addColorStop(0, preset.pal.bg[0]);
        g.addColorStop(1, preset.pal.bg[1]);
        ctx.fillStyle = g;
        ctx.fillRect(0, 0, W, H);
        ctx.lineCap = 'round';
        ctx.lineJoin = 'round';
        perFrame = duration ? Math.ceil(strokes.length / (duration / 16.7)) : strokes.length;
      }
      const end = Math.min(strokes.length, i + perFrame);
      while (i < end && performance.now() < deadline) drawStroke(ctx, strokes[i++]);
      if (i < strokes.length) return false;
      ctx.globalAlpha = 1;
      cv.classList.add('is-done');
      if (cv.hasAttribute('data-twinkle') && !reduced.matches) addTwinkles(cv, m);
      return true;
    },
  });
  kick();
}
function schedule(cv, st, delay) {
  clearTimeout(st.timer);
  st.timer = window.setTimeout(() => {
    if (!st.near) return;
    const box = cv.getBoundingClientRect();
    // Перерисовываем только при заметной смене размера: полотно пишется
    // заново целиком, и дрожание на пару пикселей того не стоит.
    if (Math.abs(box.width - st.W) < 2 && Math.abs(box.height - st.H) < 2) return;
    render(cv, st);
  }, delay);
}
let mounted = false;
export function mountPaintings() {
  if (mounted) return;
  mounted = true;
  const canvases = document.querySelectorAll('canvas.paint[data-preset]');
  // Открылся раздел — его холсты начинают писаться.
  document.addEventListener('section:play', kick);
  // Готовить мазки начинаем заранее, за экран до появления холста.
  const near = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        const cv = e.target;
        const st = states.get(cv);
        st.near = e.isIntersecting;
        if (st.near) schedule(cv, st, 0);
      }
    },
    { rootMargin: '100% 0px' },
  );
  const resize = new ResizeObserver((entries) => {
    for (const e of entries) {
      const cv = e.target;
      const st = states.get(cv);
      if (st.token) schedule(cv, st, 300);
    }
  });
  canvases.forEach((cv) => {
    states.set(cv, { near: false, W: 0, H: 0, token: 0, timer: 0 });
    near.observe(cv);
    resize.observe(cv);
  });
}
