import { useEffect, useRef } from "react";

/**
 * The Maple mark walking its own wordmark: M -> A -> P -> L -> E.
 *
 * Every letter of the mark is a single closed contour with no counter, so all five
 * can be resampled to the same point count and interpolated directly. That is why
 * this needs no morph library: `d` is just a lerp between two equal-length rings.
 *
 * Geometry is derived once, lazily, from the wordmark's own path data — so if the
 * logo asset changes, the animation follows it instead of drifting out of sync.
 */

// The five glyphs of the wordmark, viewBox "0 0 124 24".
const GLYPHS = [
  "M0 20.6204V3.03281C0 0.326049 2.89961-0.862295 5.11604 0.72215L13.5079 7.78051L21.8998 0.72215C24.1176-0.862295 27.0158 0.326049 27.0158 3.03281V20.6204C27.0158 22.4858 25.4925 23.9986 23.6141 23.9986H3.10046C1.08488 23.9986 0 22.8159 0 20.6204Z",
  "M29.5038 19.9181L39.9905 1.68295C41.2805-0.557469 43.4432-0.564493 44.7374 1.68295L55.2198 19.9181C56.5946 22.3074 55.7474 24 52.9737 24H31.75C28.9777 24 28.1304 22.3102 29.5038 19.9181Z",
  "M68.8833 19.7032V20.6204C68.8833 22.4858 67.36 23.9986 65.4816 23.9986H60.9709C59.0911 23.9986 57.5692 22.4858 57.5692 20.6204V3.78007C57.5692 1.91329 59.0926 0.400493 60.9723 0.400493H68.6697C77.6132 0.400493 80.7816 4.32788 80.7816 10.0659C80.7816 15.8039 77.6642 19.6414 68.8847 19.7032H68.8833Z",
  "M82.4006 20.6204V3.78007C82.4006 1.91329 83.924 0.400493 85.8038 0.400493H90.3144C92.1942 0.400493 93.7162 1.91329 93.7162 3.77867V8.26653H98.2353C100.115 8.26653 101.637 9.77933 101.637 11.6447V20.619C101.637 22.4844 100.114 23.9972 98.2353 23.9972H85.8038C83.924 23.9972 82.4021 22.4844 82.4021 20.619L82.4006 20.6204Z",
  "M120.598 8.26653H115.728C117.406 8.74271 118.632 10.2766 118.632 12.0942C118.632 14.013 117.269 15.6128 115.453 15.9921H120.607C122.81 15.9921 124 17.1706 124 19.3619V20.6274C124 22.8173 122.81 23.9986 120.604 23.9986H106.887C104.679 23.9986 103.491 22.8173 103.491 20.6274V3.77165C103.491 1.5804 104.679 0.400493 106.887 0.400493H120.604C122.81 0.400493 124 1.5804 124 3.77165V4.88834C124 6.75372 122.477 8.26512 120.6 8.26512L120.598 8.26653Z"
] as const;

const N = 64; // points per letter; sub-pixel accurate at every size we render
const BOX = 32;

type Ring = Float64Array; // [x0,y0,x1,y1,...]

function parse(d: string): number[][] {
  const tok = d.match(/[MLHVCZ]|-?\d*\.?\d+(?:e-?\d+)?/gi) ?? [];
  const pts: number[][] = [];
  let x = 0,
    y = 0,
    cmd = "";
  for (let i = 0; i < tok.length; ) {
    if (/[MLHVCZ]/i.test(tok[i])) {
      cmd = tok[i++];
      if (cmd === "Z") continue;
    }
    const num = () => parseFloat(tok[i++]);
    if (cmd === "M") {
      x = num();
      y = num();
      pts.push([x, y]);
      cmd = "L";
    } else if (cmd === "L") {
      x = num();
      y = num();
      pts.push([x, y]);
    } else if (cmd === "H") {
      x = num();
      pts.push([x, y]);
    } else if (cmd === "V") {
      y = num();
      pts.push([x, y]);
    } else if (cmd === "C") {
      const x1 = num(),
        y1 = num(),
        x2 = num(),
        y2 = num(),
        x3 = num(),
        y3 = num();
      for (let s = 1; s <= 16; s++) {
        const t = s / 16,
          u = 1 - t;
        pts.push([
          u * u * u * x + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t * x3,
          u * u * u * y + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * y3
        ]);
      }
      x = x3;
      y = y3;
    } else i++;
  }
  return pts;
}

function resampleCentred(pts: number[][], n: number): Ring {
  const len: number[] = [];
  let total = 0;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i],
      b = pts[(i + 1) % pts.length];
    const d = Math.hypot(b[0] - a[0], b[1] - a[1]);
    len.push(d);
    total += d;
  }
  const out = new Float64Array(n * 2);
  const step = total / n;
  let seg = 0,
    acc = 0;
  for (let k = 0; k < n; k++) {
    const target = k * step;
    while (seg < len.length - 1 && acc + len[seg] < target) acc += len[seg++];
    const a = pts[seg],
      b = pts[(seg + 1) % pts.length];
    const t = len[seg] ? (target - acc) / len[seg] : 0;
    out[k * 2] = a[0] + (b[0] - a[0]) * t;
    out[k * 2 + 1] = a[1] + (b[1] - a[1]) * t;
  }
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  for (let k = 0; k < n; k++) {
    minX = Math.min(minX, out[k * 2]);
    maxX = Math.max(maxX, out[k * 2]);
    minY = Math.min(minY, out[k * 2 + 1]);
    maxY = Math.max(maxY, out[k * 2 + 1]);
  }
  const dx = (BOX - (maxX - minX)) / 2 - minX,
    dy = (BOX - (maxY - minY)) / 2 - minY;
  for (let k = 0; k < n; k++) {
    out[k * 2] += dx;
    out[k * 2 + 1] += dy;
  }
  return out;
}

function rotate(r: Ring, shift: number): Ring {
  const n = r.length / 2,
    out = new Float64Array(r.length);
  for (let k = 0; k < n; k++) {
    const s = (k + shift) % n;
    out[k * 2] = r[s * 2];
    out[k * 2 + 1] = r[s * 2 + 1];
  }
  return out;
}

/** Rotate each ring so its points travel the short way to the next letter. */
function align(rings: Ring[]): Ring[] {
  const n = rings[0].length / 2;
  const cost = (r: Ring, ref: Ring, shift: number) => {
    let c = 0;
    for (let k = 0; k < n; k += 2) {
      const s = (k + shift) % n;
      c += (r[s * 2] - ref[k * 2]) ** 2 + (r[s * 2 + 1] - ref[k * 2 + 1]) ** 2;
    }
    return c;
  };
  const best = (r: Ring, ref: Ring) => {
    let bi = 0,
      bc = Infinity;
    for (let s = 0; s < n; s++) {
      const c = cost(r, ref, s);
      if (c < bc) {
        bc = c;
        bi = s;
      }
    }
    return bi;
  };
  // A one-way chain leaves the wrap-around pair unoptimised, and that is exactly
  // where a morph crumples. Two sweeps settle every pair including the last.
  for (let pass = 0; pass < 2; pass++)
    for (let i = 1; i < rings.length; i++)
      rings[i] = rotate(rings[i], best(rings[i], rings[i - 1]));
  rings[0] = rotate(rings[0], best(rings[0], rings[rings.length - 1]));
  return rings;
}

let cached: Ring[] | null = null;
function letters(): Ring[] {
  if (!cached) cached = align(GLYPHS.map((g) => resampleCentred(parse(g), N)));
  return cached;
}

function pathAt(a: Ring, b: Ring, t: number): string {
  let d = "";
  for (let k = 0; k < N; k++) {
    const x = a[k * 2] + (b[k * 2] - a[k * 2]) * t;
    const y = a[k * 2 + 1] + (b[k * 2 + 1] - a[k * 2 + 1]) * t;
    d += (k ? "L" : "M") + Math.round(x * 100) / 100 + " " + Math.round(y * 100) / 100;
  }
  return d + "Z";
}

export function MapleLoadingMark({
  size = 48,
  morphMs = 400,
  holdMs = 140,
  className,
  label = "Loading"
}: {
  size?: number;
  morphMs?: number;
  holdMs?: number;
  className?: string;
  label?: string;
}) {
  const ref = useRef<SVGPathElement>(null);

  useEffect(() => {
    const rings = letters();
    const node = ref.current;
    if (!node) return;

    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      node.setAttribute("d", pathAt(rings[0], rings[0], 0));
      return;
    }

    let i = 0,
      phase: "hold" | "morph" = "hold",
      t0 = performance.now(),
      raf = 0;
    const tick = (now: number) => {
      const elapsed = now - t0;
      // Advance state before reading it: deriving the pair first makes the frame
      // that completes a morph render the letter it just left, which flashes.
      if (phase === "hold") {
        if (elapsed >= holdMs) {
          phase = "morph";
          t0 = now;
        }
      } else if (elapsed >= morphMs) {
        i = (i + 1) % rings.length;
        phase = "hold";
        t0 = now;
      }

      const t = phase === "morph" ? Math.min(1, (now - t0) / morphMs) : 0;
      const eased = -(Math.cos(Math.PI * t) - 1) / 2;
      node.setAttribute("d", pathAt(rings[i], rings[(i + 1) % rings.length], eased));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [morphMs, holdMs]);

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${BOX} ${BOX}`}
      className={className}
      role="img"
      aria-label={label}
    >
      <path ref={ref} fill="currentColor" />
    </svg>
  );
}
