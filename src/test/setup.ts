import "@testing-library/jest-dom/vitest";

Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  configurable: true,
  value: () => ({
    setTransform: () => undefined,
    clearRect: () => undefined,
    beginPath: () => undefined,
    moveTo: () => undefined,
    arcTo: () => undefined,
    closePath: () => undefined,
    fill: () => undefined,
    arc: () => undefined,
    lineTo: () => undefined,
    createLinearGradient: () => ({ addColorStop: () => undefined }),
    set fillStyle(_value: string) {},
    set globalAlpha(_value: number) {},
  }),
});
