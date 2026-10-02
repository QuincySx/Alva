// INPUT: WorkspaceFiles and controlled workspace bridge promises
// OUTPUT: Navigation, inert preview, failure/retry and session-race regressions
// POS: Read-only file workbench UI acceptance.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { listHubWorkspaceFiles, readHubWorkspaceFile, type WorkspaceDirectory } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { WorkspaceFiles } from "./workspace-files";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), listHubWorkspaceFiles: vi.fn(), readHubWorkspaceFile: vi.fn() }));
const list = vi.mocked(listHubWorkspaceFiles);
const read = vi.mocked(readHubWorkspaceFile);
const view = (id = "a") => <I18nProvider><UIProvider><WorkspaceFiles key={id} sessionId={id} /></UIProvider></I18nProvider>;
beforeEach(() => { list.mockReset(); read.mockReset(); localStorage.setItem("alva-hub.language", "en-US"); });
afterEach(cleanup);

it("navigates real directory DTOs and displays inert, bounded text", async () => {
  list.mockImplementation(async (_, path) => ({ path, truncated: false, entries: path ? [{ name: "file", path: "src/file", kind: "file" }] : [{ name: "src", path: "src", kind: "directory" }] }));
  read.mockResolvedValue({ path: "src/file", kind: "text", text: "<script>inert</script>", total_bytes: 999999, truncated: true });
  const { container } = render(view());
  fireEvent.click(await screen.findByText("src"));
  fireEvent.click(await screen.findByText("file"));
  await screen.findByText("<script>inert</script>");
  expect(read).toHaveBeenCalledWith("a", "src/file");
  expect(container.querySelector("script")).toBeNull();
  expect(screen.getByText(/Preview limited/)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  await screen.findByText("src");
  expect(screen.queryByText("<script>inert</script>")).toBeNull();
});

it("discards a late old-session listing", async () => {
  let finish!: (value: WorkspaceDirectory) => void;
  list.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  list.mockResolvedValueOnce({ path: "", entries: [{ name: "new", path: "new", kind: "file" }], truncated: false });
  const rendered = render(view());
  rendered.rerender(view("b"));
  await screen.findByText("new");
  await act(async () => finish({ path: "", entries: [{ name: "old", path: "old", kind: "file" }], truncated: false }));
  expect(screen.queryByText("old")).toBeNull();
});

it("retries failure and labels binary files without fabricating a preview", async () => {
  list.mockRejectedValueOnce(new Error("private path details"));
  list.mockResolvedValue({ path: "", entries: [{ name: "binary", path: "binary", kind: "file" }], truncated: true });
  read.mockResolvedValue({ path: "binary", kind: "binary", text: null, total_bytes: 8, truncated: false });
  render(view());
  await screen.findByText(/Unavailable or protected/);
  expect(screen.queryByText("private path details")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  fireEvent.click(await screen.findByText("binary"));
  await screen.findByText(/Binary or non-UTF-8/);
  expect(screen.getByText(/2,000 entries/)).toBeVisible();
});
