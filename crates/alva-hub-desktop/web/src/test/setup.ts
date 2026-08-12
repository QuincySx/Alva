// INPUT:  @testing-library/jest-dom matchers
// OUTPUT: jest-dom assertions (toBeInTheDocument, toBeDisabled, ...) for vitest
// POS:    Shared setup for every Hub Desktop web test.

import "@testing-library/jest-dom/vitest";

class TestResizeObserver implements ResizeObserver {
  disconnect() {}
  observe() {}
  unobserve() {}
}

globalThis.ResizeObserver = TestResizeObserver;
