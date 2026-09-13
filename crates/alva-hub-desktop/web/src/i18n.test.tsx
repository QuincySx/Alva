// INPUT:  I18nProvider and a deterministic browser storage seam
// OUTPUT: Regression coverage for Chinese-first defaults and persisted live language switching
// POS:    Product-copy contract test above the typed dictionaries

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { I18nProvider, localizeProductText, useI18n } from "./i18n";
// R37 guard: read the component source through Vite's ?raw import.
import productPagesSource from "./components/product-pages.tsx?raw";

function Probe() {
  const { locale, setPreference, t } = useI18n();
  return (
    <div>
      <span>{locale}</span>
      <strong>{t("nav.newSession")}</strong>
      <button onClick={() => setPreference("en-US")}>English</button>
    </div>
  );
}

describe("I18nProvider", () => {
  beforeEach(() => localStorage.clear());

  it("starts in Simplified Chinese and persists an explicit English switch", async () => {
    const view = render(<I18nProvider><Probe /></I18nProvider>);
    expect(screen.getByText("zh-CN")).toBeInTheDocument();
    expect(screen.getByText("新建会话")).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("zh-CN");

    fireEvent.click(screen.getByRole("button", { name: "English" }));
    expect(screen.getByText("en-US")).toBeInTheDocument();
    expect(screen.getByText("New session")).toBeInTheDocument();
    await waitFor(() => expect(localStorage.getItem("alva-hub.language")).toBe("en-US"));

    view.unmount();
    render(<I18nProvider><Probe /></I18nProvider>);
    expect(screen.getByText("en-US")).toBeInTheDocument();
  });

  it("projects backend enum and account copy without changing technical IDs", () => {
    expect(localizeProductText("zh-CN", "running")).toBe("运行中");
    expect(localizeProductText("zh-CN", "Workspace provider account")).toBe("工作区供应商账号");
    expect(localizeProductText("zh-CN", "claude-sonnet-4-5")).toBe("claude-sonnet-4-5");
    expect(localizeProductText("en-US", "running")).toBe("running");
  });

  it("keeps reworked domain components on typed keys, not inline locale branches (R37)", () => {
    expect(productPagesSource).not.toMatch(/locale === ["']zh-CN["']/);
  });
});
