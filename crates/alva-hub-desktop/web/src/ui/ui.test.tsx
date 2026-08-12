// INPUT:  ALVA UI primitives rendered in jsdom
// OUTPUT: Regression coverage for loading, labels, selection, and switching
// POS:    Design-system behavior tests independent of Hub business state

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Button, SelectField, Switch, UIProvider } from ".";

describe("ALVA UI primitives", () => {
  it("locks a loading action without losing its accessible name", () => {
    render(<Button loading>Save model</Button>);

    expect(screen.getByRole("button", { name: "Save model" })).toBeDisabled();
  });

  it("exposes a select trigger with the current option", () => {
    render(
      <SelectField
        ariaLabel="Runtime"
        defaultValue="codex"
        options={[
          { value: "alva", label: "ALVA" },
          { value: "codex", label: "Codex" },
        ]}
      />,
    );

    expect(screen.getByRole("combobox", { name: "Runtime" })).toHaveTextContent(
      "Codex",
    );
  });

  it("routes switch state through one semantic control", () => {
    const onCheckedChange = vi.fn();
    render(
      <UIProvider>
        <Switch label="Show Inspector" onCheckedChange={onCheckedChange} />
      </UIProvider>,
    );

    fireEvent.click(screen.getByRole("switch", { name: "Show Inspector" }));
    expect(onCheckedChange).toHaveBeenCalledWith(true);
  });
});
