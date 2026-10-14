import React, { useEffect, useRef } from 'react';

/**
 * Diwali "cracker pop" fireworks — dependency-free canvas 2D.
 *
 *  - `burstCrackers()`   full-viewport, click-through overlay (e.g. after an order is placed).
 *  - `HeroCrackers`      a canvas confined to its (relatively positioned) parent that pops a
 *                        burst or two every ~6–12 s while it is on screen.
 *
 * Both respect `prefers-reduced-motion: reduce` (they draw nothing), stop while the tab is
 * hidden, cap particle counts and only run requestAnimationFrame while something is in flight.
 */

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/** gold, crimson, saffron, green, magenta, white */
const PALETTE = ['#F5B301', '#E11D48', '#FF7A00', '#16A34A', '#D61F9E', '#FFFFFF'] as const;
const WHITE = PALETTE.length - 1;
const GLITTER_COLOR = '#FFE9A8';
/** Alpha is quantised into this many levels so sparks can be batched into a few strokes. */
const ALPHA_LEVELS = 4;

const rand = (min: number, max: number) => min + Math.random() * (max - min);
const pick = <T,>(items: readonly T[]): T => items[Math.floor(Math.random() * items.length)];

function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
    : false;
}

/** Rough "is this a small / low-end device" scale for particle budgets. */
function deviceScale(cssWidth: number): number {
  const cores = typeof navigator !== 'undefined' ? navigator.hardwareConcurrency || 4 : 4;
  let s = 1;
  if (cssWidth < 640) s *= 0.65;
  if (cores <= 4) s *= 0.75;
  return s;
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

interface Spark {
  x: number;
  y: number;
  px: number;
  py: number;
  vx: number;
  vy: number;
  life: number;
  maxLife: number;
  color: number;
  glitter: boolean;
  drag: number;
  gravity: number;
}

interface Rocket {
  x: number;
  y: number;
  px: number;
  py: number;
  vx: number;
  vy: number;
  g: number;
  age: number;
  burst: BurstSpec;
}

interface Flash {
  x: number;
  y: number;
  r: number;
  life: number;
  maxLife: number;
}

type BurstKind = 'peony' | 'ring' | 'willow';

interface BurstSpec {
  x: number;
  y: number;
  /** 1 = normal size; affects spark count and speed. */
  scale: number;
  kind: BurstKind;
  colors: number[];
}

interface Launch {
  /** Engine clock (ms) at which the rocket leaves. */
  at: number;
  burst: BurstSpec;
  /** How far (css px) below the burst point the rocket starts; Infinity = from the bottom edge. */
  rise: number;
  /** Seconds to reach the burst point. */
  riseTime: number;
}

interface EngineOptions {
  maxSparks: number;
  maxDpr: number;
  /** Multiplies every alpha — lets the hero version blend in more softly. */
  opacity: number;
  /** Base spark lifetime in seconds. */
  sparkLife: number;
  onIdle?: () => void;
}

class FireworksEngine {
  private readonly ctx: CanvasRenderingContext2D | null;
  private w = 0;
  private h = 0;
  private dpr = 1;
  private sparks: Spark[] = [];
  private pool: Spark[] = [];
  private rockets: Rocket[] = [];
  private flashes: Flash[] = [];
  private queue: Launch[] = [];
  private buckets: Spark[][] = Array.from({ length: PALETTE.length * ALPHA_LEVELS }, () => []);
  private raf = 0;
  private last = 0;
  private clock = 0;
  private paused = false;
  private dirty = false;
  private destroyed = false;

  constructor(private readonly canvas: HTMLCanvasElement, private readonly opts: EngineOptions) {
    this.ctx = canvas.getContext('2d');
  }

  get width() {
    return this.w;
  }

  get height() {
    return this.h;
  }

  get now() {
    return this.clock;
  }

  get budgetLeft() {
    return this.opts.maxSparks - this.sparks.length;
  }

  resize(cssW: number, cssH: number) {
    const w = Math.max(1, Math.round(cssW));
    const h = Math.max(1, Math.round(cssH));
    const dpr = Math.min(this.opts.maxDpr, typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1);
    if (w === this.w && h === this.h && dpr === this.dpr) return;
    this.w = w;
    this.h = h;
    this.dpr = dpr;
    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
    // Resizing wipes the bitmap and resets the transform.
    this.ctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  /** Queue a rocket. `delayMs` is relative to now. */
  launch(delayMs: number, burst: BurstSpec, rise = Infinity, riseTime = rand(0.5, 0.75)) {
    if (this.destroyed) return;
    this.queue.push({ at: this.clock + Math.max(0, delayMs), burst, rise, riseTime });
  }

  isIdle() {
    return this.sparks.length === 0 && this.rockets.length === 0 && this.flashes.length === 0 && this.queue.length === 0;
  }

  start() {
    if (this.destroyed || this.paused || this.raf || !this.ctx) return;
    this.last = performance.now();
    this.raf = requestAnimationFrame(this.frame);
  }

  pause() {
    this.paused = true;
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
  }

  resume() {
    if (!this.paused) return;
    this.paused = false;
    if (!this.isIdle()) this.start();
  }

  /** Drop everything in flight and wipe the canvas (keeps the engine usable). */
  reset() {
    this.pool.push(...this.sparks);
    this.sparks = [];
    this.rockets = [];
    this.flashes = [];
    this.queue = [];
    this.dirty = true;
    this.clear();
  }

  destroy() {
    this.destroyed = true;
    this.pause();
    this.sparks = [];
    this.pool = [];
    this.rockets = [];
    this.flashes = [];
    this.queue = [];
  }

  private frame = (ts: number) => {
    this.raf = 0;
    if (this.destroyed || this.paused) return;
    const dt = Math.min(Math.max((ts - this.last) / 1000, 0), 1 / 30);
    this.last = ts;
    this.clock += dt * 1000;

    this.spawnDue();
    this.update(dt);
    this.draw(dt);

    if (this.isIdle()) {
      this.clear();
      this.opts.onIdle?.();
      return;
    }
    this.raf = requestAnimationFrame(this.frame);
  };

  private clear() {
    if (!this.ctx || !this.dirty) return;
    this.ctx.clearRect(0, 0, this.w, this.h);
    this.dirty = false;
  }

  private spawnDue() {
    if (this.queue.length === 0) return;
    const keep: Launch[] = [];
    for (const l of this.queue) {
      if (l.at > this.clock) {
        keep.push(l);
        continue;
      }
      const { burst } = l;
      const startY = Number.isFinite(l.rise) ? Math.min(this.h + 8, burst.y + l.rise) : this.h + 8;
      const dist = Math.max(40, startY - burst.y);
      const t = l.riseTime;
      // Constant deceleration so the rocket's apex lands exactly on the burst point.
      const g = (2 * dist) / (t * t);
      const vy = (-2 * dist) / t;
      const startX = burst.x + rand(-0.06, 0.06) * dist;
      this.rockets.push({ x: startX, y: startY, px: startX, py: startY, vx: (burst.x - startX) / t, vy, g, age: 0, burst });
    }
    this.queue = keep;
  }

  private explode(b: BurstSpec) {
    const budget = this.budgetLeft;
    if (budget <= 0) return;
    const sizeK = Math.min(1.35, Math.max(0.7, Math.min(this.w, this.h) / 650));
    const willow = b.kind === 'willow';
    const count = Math.min(budget, Math.round((willow ? 95 : 120) * b.scale * deviceScale(this.w)));
    const maxSpeed = (willow ? 260 : 380) * b.scale * sizeK;
    const baseLife = this.opts.sparkLife * (willow ? 1.6 : 1.05);

    for (let i = 0; i < count; i++) {
      const angle = (i / count) * Math.PI * 2 + rand(-0.08, 0.08);
      const speed =
        b.kind === 'ring' ? maxSpeed * rand(0.92, 1) : maxSpeed * (0.22 + 0.78 * Math.sqrt(Math.random()));
      const s = this.pool.pop() ?? ({} as Spark);
      s.x = s.px = b.x;
      s.y = s.py = b.y;
      s.vx = Math.cos(angle) * speed;
      s.vy = Math.sin(angle) * speed;
      s.maxLife = s.life = baseLife * rand(0.75, 1.15);
      s.color = willow ? 0 : b.colors[i % b.colors.length];
      s.glitter = Math.random() < (willow ? 0.38 : 0.18);
      s.drag = willow ? 0.942 : 0.965;
      s.gravity = willow ? 180 : 115;
      this.sparks.push(s);
    }
    this.flashes.push({ x: b.x, y: b.y, r: maxSpeed * 0.45, life: 0.22, maxLife: 0.22 });
  }

  private update(dt: number) {
    // Rockets
    for (let i = this.rockets.length - 1; i >= 0; i--) {
      const r = this.rockets[i];
      r.px = r.x;
      r.py = r.y;
      r.age += dt;
      r.vy += r.g * dt;
      r.x += r.vx * dt;
      r.y += r.vy * dt;
      if (r.vy >= 0 || r.y <= r.burst.y) {
        this.explode(r.burst);
        this.rockets[i] = this.rockets[this.rockets.length - 1];
        this.rockets.pop();
      }
    }

    // Sparks
    const f60 = dt * 60;
    for (let i = this.sparks.length - 1; i >= 0; i--) {
      const s = this.sparks[i];
      s.life -= dt;
      if (s.life <= 0 || s.y > this.h + 20) {
        this.pool.push(s);
        this.sparks[i] = this.sparks[this.sparks.length - 1];
        this.sparks.pop();
        continue;
      }
      const drag = Math.pow(s.drag, f60);
      s.px = s.x;
      s.py = s.y;
      s.vx *= drag;
      s.vy = s.vy * drag + s.gravity * dt;
      s.x += s.vx * dt;
      s.y += s.vy * dt;
    }

    for (let i = this.flashes.length - 1; i >= 0; i--) {
      this.flashes[i].life -= dt;
      if (this.flashes[i].life <= 0) this.flashes.splice(i, 1);
    }
  }

  private draw(dt: number) {
    const ctx = this.ctx;
    if (!ctx) return;
    const { w, h } = this;
    const opacity = this.opts.opacity;

    // Fade the previous frame towards transparent -> trails (frame-rate independent).
    ctx.globalCompositeOperation = 'destination-out';
    ctx.globalAlpha = 1;
    ctx.fillStyle = `rgba(0,0,0,${1 - Math.pow(1 - 0.28, dt * 60)})`;
    ctx.fillRect(0, 0, w, h);
    ctx.globalCompositeOperation = 'source-over';
    this.dirty = true;

    // Burst flashes: one soft radial glow per explosion.
    for (const f of this.flashes) {
      const k = f.life / f.maxLife;
      const g = ctx.createRadialGradient(f.x, f.y, 0, f.x, f.y, f.r);
      g.addColorStop(0, 'rgba(255,236,170,0.9)');
      g.addColorStop(1, 'rgba(255,190,80,0)');
      ctx.globalAlpha = k * 0.55 * opacity;
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(f.x, f.y, f.r, 0, Math.PI * 2);
      ctx.fill();
    }

    // Rockets: bright gold head with a short streak.
    ctx.lineCap = 'round';
    if (this.rockets.length) {
      ctx.strokeStyle = '#FFD066';
      ctx.lineWidth = 2.8;
      ctx.beginPath();
      for (const r of this.rockets) {
        if (r.age < 0.04) continue; // let mid-air launches appear without a pop-in dot
        ctx.moveTo(r.px, r.py + 8);
        ctx.lineTo(r.x, r.y);
      }
      ctx.globalAlpha = opacity;
      ctx.stroke();
    }

    // Sparks, batched by colour x quantised alpha.
    for (const b of this.buckets) b.length = 0;
    const glitter: Spark[] = [];
    for (const s of this.sparks) {
      const k = s.life / s.maxLife;
      if (s.glitter) {
        // Crackle: glitter twinkles on and off as it falls.
        if (Math.random() < 0.55) glitter.push(s);
        continue;
      }
      const level = Math.min(ALPHA_LEVELS - 1, Math.floor(k * ALPHA_LEVELS));
      this.buckets[s.color * ALPHA_LEVELS + level].push(s);
    }

    ctx.lineWidth = 2.4;
    for (let c = 0; c < PALETTE.length; c++) {
      ctx.strokeStyle = PALETTE[c];
      for (let a = 0; a < ALPHA_LEVELS; a++) {
        const bucket = this.buckets[c * ALPHA_LEVELS + a];
        if (bucket.length === 0) continue;
        ctx.beginPath();
        for (const s of bucket) {
          ctx.moveTo(s.px, s.py);
          ctx.lineTo(s.x + 0.01, s.y + 0.01);
        }
        ctx.globalAlpha = ((a + 1) / ALPHA_LEVELS) * opacity;
        ctx.stroke();
      }
    }

    if (glitter.length) {
      ctx.fillStyle = GLITTER_COLOR;
      ctx.globalAlpha = 0.95 * opacity;
      for (const s of glitter) {
        const size = 1.4 + Math.random() * 2.2;
        ctx.fillRect(s.x - size / 2, s.y - size / 2, size, size);
      }
    }
    ctx.globalAlpha = 1;
  }
}

function randomBurst(x: number, y: number, scale: number, allowWillow: boolean): BurstSpec {
  const roll = Math.random();
  const kind: BurstKind = allowWillow && roll < 0.18 ? 'willow' : roll < 0.45 ? 'ring' : 'peony';
  // 1–2 main colours, sometimes sprinkled with white.
  const base = pick([0, 1, 2, 3, 4]);
  const colors = [base];
  if (Math.random() < 0.55) colors.push(pick([0, 1, 2, 3, 4].filter((c) => c !== base)));
  if (Math.random() < 0.35) colors.push(WHITE);
  return { x, y, scale, kind, colors };
}

// ---------------------------------------------------------------------------
// Full-screen overlay: burstCrackers()
// ---------------------------------------------------------------------------

interface Overlay {
  engine: FireworksEngine;
  canvas: HTMLCanvasElement;
  safetyTimer: number;
  teardown: () => void;
}

let overlay: Overlay | null = null;

function createOverlay(): Overlay {
  const canvas = document.createElement('canvas');
  canvas.setAttribute('aria-hidden', 'true');
  canvas.setAttribute('data-cracker-pop', '');
  Object.assign(canvas.style, {
    position: 'fixed',
    inset: '0',
    width: '100%',
    height: '100%',
    pointerEvents: 'none',
    zIndex: '2147483000',
  } satisfies Partial<CSSStyleDeclaration>);

  let torn = false;
  const teardown = () => {
    if (torn) return;
    torn = true;
    engine.destroy();
    window.removeEventListener('resize', onResize);
    document.removeEventListener('visibilitychange', onVisibility);
    window.clearTimeout(o.safetyTimer);
    canvas.remove();
    if (overlay === o) overlay = null;
  };

  const engine = new FireworksEngine(canvas, {
    maxSparks: Math.round(1400 * deviceScale(window.innerWidth)),
    maxDpr: 2,
    opacity: 1,
    sparkLife: 1.25,
    onIdle: () => teardown(),
  });

  const onResize = () => engine.resize(window.innerWidth, window.innerHeight);
  const onVisibility = () => {
    if (document.visibilityState === 'hidden') teardown();
  };

  const o: Overlay = { engine, canvas, safetyTimer: 0, teardown };
  onResize();
  window.addEventListener('resize', onResize);
  document.addEventListener('visibilitychange', onVisibility);
  document.body.appendChild(canvas);
  return o;
}

function planShow(engine: FireworksEngine, intensity: 'light' | 'celebration', durationMs: number) {
  const { width: w, height: h } = engine;
  if (intensity === 'light') {
    const n = 2 + Math.floor(Math.random() * 3);
    const window_ = Math.max(0, durationMs - 1250);
    for (let i = 0; i < n; i++) {
      const x = w * rand(0.15, 0.85);
      const y = h * rand(0.12, 0.42);
      const delay = n === 1 ? 0 : (window_ * i) / (n - 1);
      engine.launch(delay, randomBurst(x, y, rand(0.9, 1.2), true), Infinity, rand(0.42, 0.55));
    }
    return;
  }

  // Celebration: an opening volley, then bursts spread across the screen.
  const smallScreen = w < 640;
  const n = Math.max(10, Math.round((durationMs / 1000) * (smallScreen ? 3.6 : 5.2)));
  const launchWindow = Math.max(400, durationMs - 1700);
  const slots = Array.from({ length: n }, (_, i) => (i + 0.5) / n); // stratified x positions
  for (let i = slots.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [slots[i], slots[j]] = [slots[j], slots[i]];
  }
  for (let i = 0; i < n; i++) {
    const volley = i < 4;
    const delay = volley ? i * 85 : 220 + ((launchWindow - 220) * (i - 4)) / Math.max(1, n - 5) + rand(-100, 100);
    const x = w * (0.06 + 0.88 * slots[i]);
    const y = h * rand(0.1, 0.45);
    engine.launch(delay, randomBurst(x, y, rand(0.95, 1.45), true), Infinity, rand(0.5, 0.75));
  }
}

/**
 * Fire a short fireworks show over the whole viewport. Click-through, self-removing,
 * safe to call repeatedly (overlapping calls share one overlay). No-op when the user
 * prefers reduced motion, during SSR, or while the tab is hidden.
 */
export function burstCrackers(opts?: { intensity?: 'light' | 'celebration'; durationMs?: number }): void {
  if (typeof window === 'undefined' || typeof document === 'undefined' || !document.body) return;
  if (prefersReducedMotion() || document.visibilityState === 'hidden') return;

  const intensity = opts?.intensity ?? 'light';
  const durationMs = Math.min(10000, Math.max(800, opts?.durationMs ?? (intensity === 'celebration' ? 3600 : 1500)));

  try {
    if (!overlay) overlay = createOverlay();
    const o = overlay;
    if (!o) return;
    planShow(o.engine, intensity, durationMs);
    // Hard stop in case rAF is throttled and the engine never reaches idle.
    window.clearTimeout(o.safetyTimer);
    o.safetyTimer = window.setTimeout(o.teardown, durationMs + 4000);
    o.engine.start();
  } catch {
    overlay?.teardown();
  }
}

// ---------------------------------------------------------------------------
// Hero: occasional pops inside a container
// ---------------------------------------------------------------------------

export interface HeroCrackersOptions {
  /** Delay before the first pop after mount (ms). Default ~300–600 ms. */
  firstDelayMs?: number;
  /** Random gap between pops (ms). Default 1600–3200 ms for continuous festive excitement. */
  minDelayMs?: number;
  maxDelayMs?: number;
}

/**
 * Drives occasional firework pops on the given canvas, which should be absolutely
 * positioned (and `pointer-events: none`) inside its container. Pauses when the canvas
 * is scrolled out of view or the tab is hidden; does nothing under reduced motion.
 */
export function useHeroCrackers(
  canvasRef: React.RefObject<HTMLCanvasElement>,
  options: HeroCrackersOptions = {}
): void {
  const { firstDelayMs, minDelayMs = 1600, maxDelayMs = 3200 } = options;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || typeof window === 'undefined' || prefersReducedMotion()) return;

    const engine = new FireworksEngine(canvas, {
      maxSparks: Math.round(950 * deviceScale(window.innerWidth)),
      maxDpr: 1.5,
      opacity: 0.95,
      sparkLife: 1.25,
    });

    let inView = typeof IntersectionObserver === 'undefined';
    let docVisible = document.visibilityState !== 'hidden';
    let timer: number | undefined;
    let popped = false;

    const measure = () => {
      const rect = canvas.getBoundingClientRect();
      engine.resize(rect.width, rect.height);
    };

    const pop = () => {
      timer = undefined;
      if (!(inView && docVisible) || prefersReducedMotion()) return;
      measure();
      const { width: w, height: h } = engine;
      // Salvo of 2 to 3 vibrant cracker blasts
      const n = Math.random() < 0.45 ? 3 : 2;
      for (let i = 0; i < n; i++) {
        const x = w * rand(0.08, 0.92);
        const y = h * rand(0.08, 0.46);
        engine.launch(
          i * rand(120, 280),
          randomBurst(x, y, rand(0.95, 1.45), true),
          rand(180, 380),
          rand(0.35, 0.48)
        );
      }
      engine.start();
      popped = true;
      timer = window.setTimeout(pop, rand(minDelayMs, maxDelayMs));
    };

    const sync = () => {
      if (inView && docVisible) {
        engine.resume();
        if (timer === undefined) {
          const delay = popped ? rand(800, 1800) : firstDelayMs ?? rand(300, 600);
          timer = window.setTimeout(pop, delay);
        }
      } else {
        if (timer !== undefined) window.clearTimeout(timer);
        timer = undefined;
        engine.pause();
        engine.reset(); // don't leave a frozen half-burst on screen
      }
    };

    measure();

    let ro: ResizeObserver | undefined;
    if (typeof ResizeObserver !== 'undefined') {
      ro = new ResizeObserver(measure);
      ro.observe(canvas);
    } else {
      window.addEventListener('resize', measure);
    }

    let io: IntersectionObserver | undefined;
    if (typeof IntersectionObserver !== 'undefined') {
      io = new IntersectionObserver(
        (entries) => {
          inView = entries.some((e) => e.isIntersecting);
          sync();
        },
        { threshold: 0.15 }
      );
      io.observe(canvas);
    }

    const onVisibility = () => {
      docVisible = document.visibilityState !== 'hidden';
      sync();
    };
    document.addEventListener('visibilitychange', onVisibility);

    sync();

    return () => {
      if (timer !== undefined) window.clearTimeout(timer);
      io?.disconnect();
      ro?.disconnect();
      window.removeEventListener('resize', measure);
      document.removeEventListener('visibilitychange', onVisibility);
      engine.destroy();
    };
  }, [canvasRef, firstDelayMs, minDelayMs, maxDelayMs]);
}

/**
 * Drop-in canvas for a `relative` container. Sits behind content (give siblings
 * `relative` + a higher z-index) and never intercepts clicks.
 */
export function HeroCrackers({ className = '', ...options }: HeroCrackersOptions & { className?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useHeroCrackers(ref, options);
  return React.createElement('canvas', {
    ref,
    'aria-hidden': true,
    className: `pointer-events-none select-none ${className}`.trim(),
  });
}
