// INPUT: HubComposer inside the shared UI and i18n providers
// OUTPUT: Keyboard submission and truthful attachment-control regressions
// POS: Conversation input behavior independent of engine execution.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UIProvider } from "../ui";
import { HubComposer } from "./hub-composer";

describe("HubComposer", () => {
  it("does not submit Enter during IME composition or an active run", () => {
    const submit = vi.fn((event) => event.preventDefault());
    const props = { value: "你好", onChange: vi.fn(), onSubmit: submit, placeholder: "Message", engineId: "alva" };
    const { rerender } = render(<UIProvider><HubComposer {...props} /></UIProvider>);
    const input = screen.getByRole("textbox");
    fireEvent.keyDown(input, { key: "Enter", isComposing: true, keyCode: 229 });
    expect(submit).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(submit).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(submit).toHaveBeenCalledTimes(1);
    rerender(<UIProvider><HubComposer {...props} busy canStop /></UIProvider>);
    fireEvent.keyDown(input, { key: "Enter" });
    expect(submit).toHaveBeenCalledTimes(1);
  });
  it("disables Attach without an actual attachment action", () => {
    render(<UIProvider><HubComposer value="" onChange={() => {}}
      onSubmit={() => {}} placeholder="Message" engineId="alva" /></UIProvider>);
    expect(screen.getByRole("button", { name: "Attach context" })).toBeDisabled();
  });
  it("keeps Stop available when the next-run model route becomes unavailable", () => {
    const stop = vi.fn();
    render(<UIProvider><HubComposer value="" onChange={() => {}} onSubmit={() => {}}
      placeholder="Message" engineId="alva" busy canStop disabled onStop={stop} /></UIProvider>);
    const button = screen.getByRole("button", { name: "Stop current run" });
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(stop).toHaveBeenCalledTimes(1);
  });
});
