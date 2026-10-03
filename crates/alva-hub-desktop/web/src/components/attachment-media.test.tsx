// INPUT: Controlled read-only media bridge and real shared Dialog/preview components
// OUTPUT: Lazy media evidence, paging, truthful receipts, isolation and URL/focus lifecycle regressions
// POS: Account-free message-media UX contract, not decoder/provider/native WebView acceptance.
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getHubInputAttachment, getHubMediaArtifact, getHubMediaReceipts, listHubAttachmentMedia,
  type MediaArtifactContent, type MediaArtifactRef, type MediaArtifactPage } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { MessageAttachments } from "./input-attachments";

vi.mock("../bridge", async (original) => ({ ...await original<typeof import("../bridge")>(),
  listHubAttachmentMedia: vi.fn(), getHubMediaArtifact: vi.fn(), getHubMediaReceipts: vi.fn(), getHubInputAttachment: vi.fn() }));
const source = { attachment_id: "source", name: "source.mp4", total_bytes: 100, content_type: "video/mp4" };
const jpeg = btoa("\xff\xd8\xffsynthetic-image");
function artifact(id: string, data = jpeg): MediaArtifactRef {
  return { artifact_id: id, source_attachment_id: "source", name: "derived.jpg", total_bytes: atob(data).length,
    path: `media/${id}/derived.jpg`, content_type: "image/jpeg", processing: { output_sha256: id, method: "fixture-only", actual_timestamp_seconds: 0.6, selection_reason: "<script>inert</script>" } };
}
function content(a: MediaArtifactRef, data_base64 = jpeg): MediaArtifactContent { return { ...a, data_base64 }; }
function page(artifacts: MediaArtifactRef[], next_cursor = 1, has_more = false): MediaArtifactPage { return { artifacts, next_cursor, has_more }; }
const listing = vi.mocked(listHubAttachmentMedia);
const reading = vi.mocked(getHubMediaArtifact);
const receipts = vi.mocked(getHubMediaReceipts);
const create = vi.fn(() => "blob:derived-preview");
const revoke = vi.fn();
function view(sessionId = "session", chinese = false) {
  const child = <UIProvider><MessageAttachments sessionId={sessionId} files={[source]} /></UIProvider>;
  return chinese ? <I18nProvider>{child}</I18nProvider> : child;
}
async function open() {
  fireEvent.click(screen.getByRole("button", { name: "Inspect processing and delivery for source.mp4" }));
  await screen.findByText(/Local preview only/);
}
beforeEach(() => {
  vi.clearAllMocks();
  listing.mockReset().mockResolvedValue(page([artifact("a")]));
  reading.mockReset().mockResolvedValue(content(artifact("a")));
  receipts.mockReset().mockResolvedValue({ receipts: [], next_cursor: 0, through: 10, has_more: false });
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: create });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revoke });
});
describe("message media evidence", () => {
  it("loads metadata only on inspect, selected bytes only on click, and releases URL/restores focus", async () => {
    const { container } = render(view());
    expect(listing).not.toHaveBeenCalled(); expect(reading).not.toHaveBeenCalled();
    await open();
    expect(reading).not.toHaveBeenCalled(); expect(getHubInputAttachment).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toHaveTextContent("actual_timestamp_seconds");
    expect(container.querySelector("script")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /derived.jpg.*a$/ }));
    expect(await screen.findByRole("img", { name: "derived.jpg" })).toHaveAttribute("src", "blob:derived-preview");
    expect(reading).toHaveBeenCalledWith("session", "source", "a");
    await screen.findByText(/No recorded selection/);
    expect(screen.getByRole("dialog")).toHaveTextContent("a response does not prove visual understanding");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(revoke).toHaveBeenCalledWith("blob:derived-preview"));
    await waitFor(() => expect(screen.getByRole("button", { name: /Inspect processing/ })).toHaveFocus());
  });
  it("replaces bounded pages rather than accumulating media and refreshes at the start", async () => {
    listing.mockResolvedValueOnce(page([artifact("first")], 4, true)).mockResolvedValueOnce(page([artifact("last")], 5)).mockResolvedValueOnce(page([], 0));
    render(view()); await open();
    fireEvent.click(screen.getByRole("button", { name: "Next derivatives" }));
    await screen.findByRole("button", { name: /derived.jpg.*last$/ });
    expect(screen.queryByRole("button", { name: /derived.jpg.*first$/ })).toBeNull();
    expect(listing).toHaveBeenLastCalledWith("session", "source", 4);
    fireEvent.click(screen.getByRole("button", { name: "Refresh derivatives" }));
    await screen.findByText(/No saved derivatives/);
    expect(screen.getByRole("dialog")).toHaveTextContent("failed or cancelled processing attempt");
    expect(listing).toHaveBeenLastCalledWith("session", "source", 0);
    expect(reading).not.toHaveBeenCalled();
  });
  it("retries failures without showing backend details and rejects mismatched preview bytes", async () => {
    listing.mockRejectedValueOnce(new Error("private-host-path")).mockResolvedValueOnce(page([artifact("a")]));
    render(view());
    fireEvent.click(screen.getByRole("button", { name: /Inspect processing/ }));
    await screen.findByRole("alert");
    expect(screen.queryByText("private-host-path")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry inspection" }));
    await screen.findByRole("button", { name: /derived.jpg.*a$/ });
    reading.mockResolvedValueOnce(content(artifact("a"), btoa("wrong-size")));
    fireEvent.click(screen.getByRole("button", { name: /derived.jpg.*a$/ }));
    await screen.findByText(/Could not verify or load/); expect(create).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Retry inspection" }));
    await screen.findByRole("img"); expect(reading).toHaveBeenCalledTimes(2);
  });
  it("does not accept an old derivative or session response after navigation", async () => {
    listing.mockResolvedValueOnce(page([artifact("a"), artifact("b")]));
    let old!: (v: MediaArtifactContent) => void;
    reading.mockImplementationOnce(() => new Promise((resolve) => { old = resolve; })).mockResolvedValueOnce(content(artifact("b")));
    const { rerender } = render(view()); await open();
    fireEvent.click(screen.getByRole("button", { name: /derived.jpg.*a$/ }));
    fireEvent.click(screen.getByRole("button", { name: /derived.jpg.*b$/ }));
    await screen.findByRole("img");
    await act(async () => old(content(artifact("a"))));
    expect(within(screen.getByRole("region", { name: "Selected derivative preview" })).queryByText("media/a/derived.jpg")).toBeNull();
    rerender(view("other-session"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(revoke).toHaveBeenCalled();
  });
  it("pages receipt snapshots, shows unknown acceptance, and refreshes without reloading/sending media", async () => {
    receipts.mockResolvedValueOnce({ receipts: [{ delivery_id: "r1", status: "response_received", created_at: 1, updated_at: 2, selection: { actual_model_id: "actual-model" } }], next_cursor: 5, through: 20, has_more: true })
      .mockResolvedValueOnce({ receipts: [{ delivery_id: "r2", status: "provider_error_acceptance_unknown", created_at: 3, updated_at: 4, selection: {} }], next_cursor: 8, through: 20, has_more: false })
      .mockResolvedValueOnce({ receipts: [{ delivery_id: "r3", status: "future_state", created_at: 5, updated_at: 6, selection: {} }], next_cursor: 21, through: 21, has_more: false });
    render(view()); await open(); fireEvent.click(screen.getByRole("button", { name: /derived.jpg.*a$/ }));
    await screen.findByText(/Response received; understanding unconfirmed/);
    fireEvent.click(screen.getByRole("button", { name: "Next receipts" }));
    await screen.findByText(/Delivery acceptance unknown/);
    expect(receipts).toHaveBeenLastCalledWith("session", "source", "a", 5, 20);
    expect(screen.queryByText(/actual-model/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Refresh receipts" }));
    await screen.findByText(/Unrecognized receipt state/);
    expect(receipts).toHaveBeenLastCalledWith("session", "source", "a", 0, undefined);
    expect(reading).toHaveBeenCalledTimes(1); expect(getHubInputAttachment).not.toHaveBeenCalled();
  });
  it("supports local video controls without autoplay and truthful Chinese evidence labels", async () => {
    localStorage.setItem("alva-hub.language", "zh-CN");
    const bytes = btoa("\0\0\0\x18ftypisomsynthetic-video");
    const video = { ...artifact("video", bytes), name: "derived.mp4", content_type: "video/mp4" as const };
    listing.mockResolvedValueOnce(page([video])); reading.mockResolvedValueOnce(content(video, bytes));
    render(view("session", true));
    fireEvent.click(screen.getByRole("button", { name: "查看 source.mp4 的处理与投递" }));
    fireEvent.click(await screen.findByRole("button", { name: /derived.mp4.*video$/ }));
    const element = await screen.findByLabelText("derived.mp4");
    expect(element.tagName).toBe("VIDEO"); expect(element).toHaveAttribute("controls"); expect(element).not.toHaveAttribute("autoplay");
    expect(screen.getByRole("dialog")).toHaveTextContent("收到响应也不证明视觉理解");
    localStorage.removeItem("alva-hub.language");
  });
});
