// INPUT:  ALVA UI primitives rendered in jsdom
// OUTPUT: Regression coverage for loading, labels, selection, and switching
// POS:    Design-system behavior tests independent of Hub business state

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  AppFrame,
  Button,
  Checkbox,
  CardGrid,
  DialogContent,
  DialogRoot,
  Disclosure,
  NavItem,
  PopoverContent,
  PopoverRoot,
  ListRow,
  PageHeader,
  SettingsGroup,
  SettingsRow,
  SelectField,
  Switch,
  UIProvider,
  Workbench,
} from ".";

describe("ALVA UI primitives", () => {
  it("locks a loading action without losing its accessible name", () => {
    render(<Button density="compact" loading>Save model</Button>);

    expect(screen.getByRole("button", { name: "Save model" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save model" })).toHaveAttribute(
      "data-density",
      "compact",
    );
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

  it("composes active navigation and a responsive workbench", () => {
    render(
      <AppFrame
        sidebar={<NavItem active label="Current session" />}
        workbench={<Workbench mode="docked" title="Inspector">Events</Workbench>}
        workbenchOpen
      >
        Chat
      </AppFrame>,
    );

    expect(screen.getByRole("button", { name: "Current session" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByText("Inspector")).toBeVisible();
    expect(screen.getByText("Inspector").closest("aside")).toHaveAttribute(
      "data-mode",
      "docked",
    );
  });

  it("dismisses the workbench through the frame-owned scrim", () => {
    const onDismiss = vi.fn();
    render(
      <AppFrame
        onWorkbenchDismiss={onDismiss}
        sidebar={<aside />}
        workbench={<Workbench title="Tools" />}
        workbenchOpen
      >
        Main
      </AppFrame>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Dismiss workbench overlay" }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("opens disclosure content through the shared trigger", () => {
    render(<Disclosure title="Tool activity">cargo test</Disclosure>);

    fireEvent.click(screen.getByRole("button", { name: "Tool activity" }));
    expect(screen.getByText("cargo test")).toBeVisible();
  });

  it("exposes checkbox state through the shared field", () => {
    const onCheckedChange = vi.fn();
    render(
      <Checkbox
        label="Authorize automatic use"
        onCheckedChange={onCheckedChange}
      />,
    );

    fireEvent.click(screen.getByRole("checkbox", { name: "Authorize automatic use" }));
    expect(onCheckedChange).toHaveBeenCalledWith(true);
  });

  it("applies finite overlay size variants", () => {
    render(
      <UIProvider>
        <PopoverRoot open>
          <PopoverContent size="wide">Models</PopoverContent>
        </PopoverRoot>
        <DialogRoot open>
          <DialogContent size="compact" title="Confirm">
            Continue?
          </DialogContent>
        </DialogRoot>
      </UIProvider>,
    );

    expect(screen.getByText("Models")).toHaveAttribute("data-size", "wide");
    expect(screen.getByRole("dialog")).toHaveAttribute(
      "data-size",
      "compact",
    );
  });

  it("marks unavailable status rows as disabled controls", async () => {
    const { StatusListItem } = await import(".");
    render(
      <StatusListItem
        description="Sign in first"
        heading="Claude"
        unavailable
      />,
    );

    expect(screen.getByRole("button", { name: /Claude/ })).toBeDisabled();
  });

  it("composes page and settings surfaces without primitive overrides", () => {
    render(
      <>
        <PageHeader actions={<Button>Add</Button>} title="Automations" />
        <CardGrid>
          <ListRow heading="Nightly tests" meta="02:00" />
        </CardGrid>
        <SettingsGroup heading="Appearance">
          <SettingsRow
            control={<Switch label="Use compact density" />}
            heading="Density"
          />
        </SettingsGroup>
      </>,
    );

    expect(screen.getByRole("heading", { name: "Automations" })).toBeVisible();
    expect(screen.getByText("Nightly tests")).toBeVisible();
    expect(screen.getByRole("switch", { name: "Use compact density" })).toBeVisible();
  });
});
