import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

// No Tauri runtime in jsdom: the SFTP screen (mounted by the shell) listens for file drops.
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent: vi.fn().mockResolvedValue(() => {}) }) }));

afterEach(() => {
  cleanup();
  document.documentElement.className = "";
});
