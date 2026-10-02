// INPUT: Selected file bytes and controlled session attachment reads
// OUTPUT: File-byte fidelity, limits, local previews/provenance and stale-response regressions
// POS: Deterministic input-context UI contract; no real user files are read.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";
import { getHubInputAttachment } from "../bridge";
import { UIProvider } from "../ui";
import { InputAttachments, MessageAttachments, attachmentBytes, readSelectedFiles } from "./input-attachments";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(), getHubInputAttachment: vi.fn() }));
function file(name: string, text: string): File {
  const bytes = new TextEncoder().encode(text);
  return { name, size: bytes.byteLength, arrayBuffer: async () => bytes.buffer } as File;
}
describe("Input attachments", () => {
  it("retains text and binary bytes without feeding them to a model, and rejects limits/protected names", async () => {
    const existing = [{ name: "old.txt", text: "old" }];
    const selected = await readSelectedFiles([file("资料.txt", "你好\n<code>"), file("binary", "a\0b")], existing);
    expect(selected[0]).toEqual(existing[0]);
    expect(new TextDecoder().decode(attachmentBytes(selected[1]))).toBe("你好\n<code>");
    expect(attachmentBytes(selected[2])).toEqual(new Uint8Array([97, 0, 98]));
    await expect(readSelectedFiles([{ name: "huge.txt", size: 20 * 1024 * 1024 + 1 } as File], existing)).rejects.toThrow();
    await expect(readSelectedFiles([file(".env", "not read")], existing)).rejects.toThrow();
    await expect(readSelectedFiles([file("汉".repeat(70), "not read")], existing)).rejects.toThrow();
    await expect(readSelectedFiles([file("..", "not read")], existing)).rejects.toThrow();
    await expect(readSelectedFiles(Array.from({ length: 9 }, () => file("x.txt", "x")), [])).rejects.toThrow();
  });
  it("previews/removes a draft and does not accept a late file read after disabling", async () => {
    const change = vi.fn();
    const input = createRef<HTMLInputElement>();
    const props = { files: [{ name: "draft.txt", text: "<script>inert</script>" }], onChange: change, inputRef: input, disabled: false };
    const { container, rerender } = render(<UIProvider><InputAttachments {...props} /></UIProvider>);
    fireEvent.click(screen.getByRole("button", { name: /draft.txt ·/ }));
    expect(screen.getByRole("dialog")).toHaveTextContent("<script>inert</script>");
    expect(container.querySelector("script")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.getByRole("button", { name: /draft.txt ·/ })).toHaveFocus());
    fireEvent.click(screen.getByRole("button", { name: "Remove draft.txt" }));
    expect(change).toHaveBeenCalledWith([]);
    change.mockClear();
    let resolve!: (value: ArrayBuffer) => void;
    const pending = { name: "slow.txt", size: 1, arrayBuffer: () => new Promise<ArrayBuffer>((done) => { resolve = done; }) } as File;
    fireEvent.change(input.current!, { target: { files: [pending] } });
    rerender(<UIProvider><InputAttachments {...props} disabled /></UIProvider>);
    await act(async () => resolve(new TextEncoder().encode("x").buffer));
    expect(change).not.toHaveBeenCalled();
  });
  it("retries durable preview and ignores an old session response", async () => {
    const read = vi.mocked(getHubInputAttachment);
    const reference = { attachment_id: "a", name: "a.txt", total_bytes: 3 };
    let old!: (value: typeof reference & { text: string }) => void;
    read.mockImplementationOnce(() => new Promise((resolve) => { old = resolve; }));
    read.mockRejectedValueOnce(new Error("private backend details"));
    read.mockResolvedValueOnce({ ...reference, text: "new" });
    const { rerender } = render(<UIProvider><MessageAttachments sessionId="old" files={[reference]} /></UIProvider>);
    fireEvent.click(screen.getByRole("button", { name: "a.txt · 3 B" }));
    rerender(<UIProvider><MessageAttachments sessionId="new" files={[reference]} /></UIProvider>);
    await screen.findByText(/Attachment could not be loaded/);
    fireEvent.click(screen.getByRole("button", { name: "Retry text loading" }));
    await screen.findByText("new");
    await act(async () => old({ ...reference, text: "old" }));
    expect(screen.queryByText("old")).toBeNull();
    await waitFor(() => expect(read).toHaveBeenLastCalledWith("new", "a"));
  });

  it("previews only local uploaded image bytes and revokes their URL on close", async () => {
    const create = vi.fn(() => "blob:local-image-preview");
    const revoke = vi.fn();
    Object.defineProperty(URL, "createObjectURL", { configurable: true, value: create });
    Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revoke });
    const data_base64 = btoa("\x89PNG\r\n\x1a\nsynthetic-preview");
    render(<UIProvider><InputAttachments files={[{ name: "photo.png", data_base64 }]} onChange={() => {}}
      inputRef={createRef<HTMLInputElement>()} disabled={false} /></UIProvider>);
    fireEvent.click(screen.getByRole("button", { name: /photo.png ·/ }));
    const image = await screen.findByRole("img", { name: "photo.png" });
    expect(image).toHaveAttribute("src", "blob:local-image-preview");
    expect(screen.getByRole("dialog")).toHaveTextContent("no additional compression");
    fireEvent.error(image);
    expect(screen.getByRole("dialog")).toHaveTextContent("cannot preview these media bytes");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(revoke).toHaveBeenCalledWith("blob:local-image-preview"));
  });
});
