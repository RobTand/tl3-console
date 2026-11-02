import "@testing-library/jest-dom/vitest";

// jsdom has no ResizeObserver; Radix menus measure their trigger with one.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
