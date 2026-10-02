// INPUT: Declarative display envelopes and trusted custom registrations
// OUTPUT: Real renderer, transport, version/limit and inert-data regressions
// POS: Hub presentation extension acceptance, not an agent Plugin installation test.
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createMessageModuleRegistry, MessageModuleProvider, MessageModules } from "./message-modules";
import { ChatTranscript } from "./chat-transcript";
import { UIProvider } from "../ui";

const source = (type: string, data: unknown, version = 1) => JSON.stringify({ schema: "alva.display", version: 1, modules: [{ type, version, data }] });
describe("Message modules", () => {
  it("renders table cells as inert text and handles the existing ToolResult wrapper", () => {
    const body = source("alva.table", { title: "Results", columns: ["Name"], rows: [["<img src=x onerror=boom>"]] });
    const { container } = render(<MessageModules source={JSON.stringify({ content: [{ type: "text", text: body }], is_error: false })} />);
    expect(screen.getByRole("table")).toHaveTextContent("<img src=x onerror=boom>");
    expect(container.querySelector("img")).toBeNull();
  });
  it("renders negative-value charts with an accessible data table", () => {
    render(<MessageModules source={source("alva.chart", { title: "Change", style: "bar", points: [{ label: "A", value: -2 }, { label: "B", value: 5 }] })} />);
    expect(screen.getByRole("img", { name: "Change" })).toBeTruthy();
    expect(screen.getByRole("table")).toHaveTextContent("-2");
  });
  it("renders diagram relationships and preserves unsupported data", () => {
    const { rerender } = render(<MessageModules source={source("alva.diagram", { title: "Flow", nodes: [{ id: "a", label: "Input" }, { id: "b", label: "Output" }], edges: [{ from: "a", to: "b", label: "maps" }] })} />);
    expect(screen.getByRole("img", { name: "Flow" })).toBeTruthy();
    expect(screen.getByText("Input → Output: maps")).toBeTruthy();
    rerender(<MessageModules source={source("vendor.future", { value: "preserved" })} />);
    expect(screen.getByRole("status")).toHaveTextContent("preserved");
  });
  it("rejects dangling edges, oversized tables, versions and partial JSON", () => {
    const { container, rerender } = render(<MessageModules source={source("alva.diagram", { title: "Bad", nodes: [{ id: "a", label: "A" }], edges: [{ from: "a", to: "missing" }] })} />);
    expect(container.querySelector("svg")).toBeNull();
    rerender(<MessageModules source={source("alva.table", { title: "Big", columns: ["x"], rows: Array.from({ length: 101 }, () => [1]) })} />);
    expect(screen.getByRole("status")).toBeTruthy();
    rerender(<MessageModules source={source("alva.chart", {}, 2)} />);
    expect(screen.getByRole("status")).toBeTruthy();
    rerender(<MessageModules source={'{"schema":"alva.display"'} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<MessageModules source={"x".repeat(16_385)} />);
    expect(container).toBeEmptyDOMElement();
  });
  it("uses real custom registration without editing transcript dispatch", () => {
    const registry = createMessageModuleRegistry([
      { type: "example.notice", version: 1, validate: (data) => typeof data === "string", render: (data) => <aside>{String(data)}</aside> },
      { type: "example.metric", version: 1, validate: (data) => typeof data === "number", render: (data) => <output>{String(data)}</output> },
    ]);
    const { rerender } = render(<MessageModules registry={registry} source={source("example.notice", "custom notice")} />);
    expect(screen.getByText("custom notice")).toBeTruthy();
    rerender(<MessageModules registry={registry} source={source("example.metric", 42)} />);
    expect(screen.getByRole("status")).toHaveTextContent("42");
    expect(() => createMessageModuleRegistry([registry.get("example.notice@1")!, registry.get("example.notice@1")!])).toThrow("Duplicate");
  });
  it("contains renderer failure and retains original module data", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const registry = createMessageModuleRegistry([{ type: "example.crash", version: 1, validate: () => true, render: () => { throw new Error("renderer crashed"); } }]);
      render(<MessageModules registry={registry} source={source("example.crash", { retained: "original" })} />);
      expect(screen.getByRole("status")).toHaveTextContent("original");
    } finally { error.mockRestore(); }
  });
  it("provides an installed renderer to the actual assistant transcript", () => {
    const registry = createMessageModuleRegistry([{ type: "example.notice", version: 1, validate: (data) => typeof data === "string", render: (data) => <aside>{String(data)}</aside> }]);
    render(<UIProvider><MessageModuleProvider registry={registry}><div data-chat-scroll-region=""><ChatTranscript streaming={false}
      events={[{ status: "assistant_message", engine_id: "alva", product_session_id: "s", message: `\`\`\`alva-display\n${source("example.notice", "installed module")}\n\`\`\`` }]} />
    </div></MessageModuleProvider></UIProvider>);
    expect(screen.getByText("installed module")).toBeTruthy();
  });
});
