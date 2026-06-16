// liquidGlass.ts — Apple-style "Liquid Glass" refraction for the web.
// Technique (per aave.com/design/building-glass-for-the-web): a generated
// displacement map drives an SVG feDisplacementMap used as a backdrop-filter,
// so the real background behind the dock bends into the glass curves.
// The map's R/G channels say how far each pixel refracts; it's neutral in the
// interior and ramps along the edges. Chromium (WebView2) renders SVG
// backdrop-filters; other engines fall back to plain blur (declared after the
// url() in CSS). Ported verbatim from the Atmospeak design handoff.

const ATMO_NS = "http://www.w3.org/2000/svg";
const XLINK_NS = "http://www.w3.org/1999/xlink";

export interface LensMapOptions {
  depth?: number; // px band from the edge that bends
  splay?: number; // edge falloff curve (higher = tighter to rim)
  curvature?: number; // overall bend amount (0..~1.4)
}

export interface AttachGlassOptions {
  scaleMul?: number;
  depthMul?: number;
  splay?: number;
  curvature?: number;
  chroma?: number;
}

// ── build the displacement map for a rounded-rect lens of w×h, corner r ──
export function generateLensMap(w: number, h: number, radius: number, opts?: LensMapOptions): string {
  const o = opts || {};
  const depth = o.depth || 14; // px band from the edge that bends
  const splay = o.splay || 1.0; // edge falloff curve (higher = tighter to rim)
  const curvature = o.curvature || 1.0; // overall bend amount (0..~1.4)
  w = Math.max(2, Math.round(w));
  h = Math.max(2, Math.round(h));
  const r = Math.max(0.5, Math.min(radius, Math.min(w, h) / 2));

  const cv = document.createElement("canvas");
  cv.width = w;
  cv.height = h;
  const ctx = cv.getContext("2d");
  if (!ctx) return "";
  const img = ctx.createImageData(w, h);
  const D = img.data;

  const halfW = w / 2;
  const halfH = h / 2;
  const innerW = halfW - r;
  const innerH = halfH - r;

  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const px = x + 0.5 - halfW;
      const py = y + 0.5 - halfH;
      const qx = Math.abs(px) - innerW;
      const qy = Math.abs(py) - innerH;
      const axx = Math.max(qx, 0);
      const ayy = Math.max(qy, 0);
      const outLen = Math.hypot(axx, ayy);
      const sdf = outLen + Math.min(Math.max(qx, qy), 0) - r; // <0 inside, 0 on edge

      // outward surface normal of the rounded rect
      let nx = 0;
      let ny = 0;
      if (qx > 0 || qy > 0) {
        const l = outLen || 1;
        nx = (axx / l) * Math.sign(px);
        ny = (ayy / l) * Math.sign(py);
      } else if (qx > qy) {
        nx = Math.sign(px);
        ny = 0;
      } else {
        nx = 0;
        ny = Math.sign(py);
      }

      const distInside = -sdf;
      let mag = 0;
      if (sdf < 0 && distInside < depth) {
        const t = 1 - distInside / depth; // 1 at edge → 0 inward
        mag = Math.pow(t, splay) * curvature;
      }
      if (mag > 1) mag = 1;

      // pull the backdrop outward at the rim → convex-lens magnification
      const dx = nx * mag;
      const dy = ny * mag;
      const i = (y * w + x) * 4;
      D[i] = 128 + dx * 127; // R = horizontal bend
      D[i + 1] = 128 + dy * 127; // G = vertical bend
      D[i + 2] = 128; // B unused
      D[i + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
  return cv.toDataURL();
}

interface GlassRefs {
  svg: SVGSVGElement;
  filter: SVGFilterElement;
  feImage: SVGFEImageElement;
  feR: SVGFEDisplacementMapElement;
  feB: SVGFEDisplacementMapElement;
}

// ── singleton SVG <filter> the CSS backdrop-filter points at ──
let _glass: GlassRefs | null = null;
export function ensureGlassFilter(): GlassRefs | null {
  if (_glass) return _glass;
  if (typeof document === "undefined") return null;

  const svg = document.createElementNS(ATMO_NS, "svg");
  svg.setAttribute("id", "atmo-glass-defs");
  svg.setAttribute("width", "0");
  svg.setAttribute("height", "0");
  svg.style.cssText = "position:absolute;width:0;height:0;overflow:hidden;pointer-events:none";

  const filter = document.createElementNS(ATMO_NS, "filter");
  filter.setAttribute("id", "atmoGlass");
  filter.setAttribute("color-interpolation-filters", "sRGB");
  // region == the element's border box, clipped (objectBoundingBox 0..1)
  filter.setAttribute("x", "0");
  filter.setAttribute("y", "0");
  filter.setAttribute("width", "1");
  filter.setAttribute("height", "1");

  const feImage = document.createElementNS(ATMO_NS, "feImage");
  feImage.setAttribute("result", "map");
  feImage.setAttribute("preserveAspectRatio", "none");
  feImage.setAttribute("x", "0");
  feImage.setAttribute("y", "0");

  // chroma split: bend R and B channels at slightly different strengths for a
  // faint colour fringe along the rim, then keep G from the main pass.
  const feR = document.createElementNS(ATMO_NS, "feDisplacementMap");
  feR.setAttribute("in", "SourceGraphic");
  feR.setAttribute("in2", "map");
  feR.setAttribute("xChannelSelector", "R");
  feR.setAttribute("yChannelSelector", "G");
  feR.setAttribute("result", "rPass");
  const feB = document.createElementNS(ATMO_NS, "feDisplacementMap");
  feB.setAttribute("in", "SourceGraphic");
  feB.setAttribute("in2", "map");
  feB.setAttribute("xChannelSelector", "R");
  feB.setAttribute("yChannelSelector", "G");
  feB.setAttribute("result", "bPass");
  // recombine: R from rPass, G+B from bPass (B carries the blue fringe), keep alpha
  const cmR = document.createElementNS(ATMO_NS, "feColorMatrix");
  cmR.setAttribute("in", "rPass");
  cmR.setAttribute("type", "matrix");
  cmR.setAttribute("values", "1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0");
  cmR.setAttribute("result", "rOnly");
  const cmB = document.createElementNS(ATMO_NS, "feColorMatrix");
  cmB.setAttribute("in", "bPass");
  cmB.setAttribute("type", "matrix");
  cmB.setAttribute("values", "0 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 1 0");
  cmB.setAttribute("result", "gbOnly");
  const blend = document.createElementNS(ATMO_NS, "feBlend");
  blend.setAttribute("in", "rOnly");
  blend.setAttribute("in2", "gbOnly");
  blend.setAttribute("mode", "screen");

  filter.appendChild(feImage);
  filter.appendChild(feR);
  filter.appendChild(feB);
  filter.appendChild(cmR);
  filter.appendChild(cmB);
  filter.appendChild(blend);
  svg.appendChild(filter);
  (document.body || document.documentElement).appendChild(svg);

  _glass = { svg, filter, feImage, feR, feB };
  return _glass;
}

// ── attach live refraction to an element; returns a cleanup fn ──
export function attachLiquidGlass(el: HTMLElement, getOpts?: () => AttachGlassOptions): () => void {
  const g = ensureGlassFilter();
  if (!g) return () => {};
  let lastKey = "";

  const update = () => {
    if (!el.isConnected) return;
    const w = el.clientWidth;
    const h = el.clientHeight;
    if (!w || !h) return;
    const cs = getComputedStyle(el);
    let radius = parseFloat(cs.borderTopLeftRadius) || 0;
    radius = Math.min(radius, Math.min(w, h) / 2);
    const o = (getOpts && getOpts()) || {};
    // bend band + strength scale gently with size
    const depth = Math.max(8, Math.min(Math.min(w, h) * 0.46, 30)) * (o.depthMul || 1);
    const scale = depth * (o.scaleMul || 2.0);
    const chroma = o.chroma != null ? o.chroma : 0.18;

    const key = `${w}x${h}x${Math.round(radius)}x${depth.toFixed(1)}`;
    if (key !== lastKey) {
      lastKey = key;
      const map = generateLensMap(w, h, radius, { depth, splay: o.splay || 1.1, curvature: o.curvature || 1.0 });
      g.feImage.setAttribute("href", map);
      g.feImage.setAttributeNS(XLINK_NS, "xlink:href", map);
      g.feImage.setAttribute("width", String(w));
      g.feImage.setAttribute("height", String(h));
    }
    g.feR.setAttribute("scale", (scale * (1 + chroma)).toFixed(2));
    g.feB.setAttribute("scale", (scale * (1 - chroma)).toFixed(2));
  };

  // run synchronously (preview panes throttle rAF when backgrounded)
  const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(() => update()) : null;
  ro?.observe(el);
  update();

  return () => {
    ro?.disconnect();
  };
}
