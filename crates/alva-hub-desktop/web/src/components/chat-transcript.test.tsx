// INPUT:  ChatTranscript rendered inside a scroll region with deterministic events
// OUTPUT: Reading-behavior regressions: raw-character Markdown, HTML inertness,
//         code copy, scroll-up unpins follow, continued streaming and jump-to-bottom
// POS:    R41 — direct ChatTranscript behavior coverage (not HubTimeline proxies)

import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TimelineEvent } from "../bridge";
import { I18nProvider } from "../i18n";
import { UIProvider } from "../ui";
import { ChatTranscript } from "./chat-transcript";
import type { HubTimelineEvent } from "./hub-timeline";

function message(overrides: Partial<HubTimelineEvent> = {}): HubTimelineEvent {
  const base: TimelineEvent = {
    status: "assistant_message",
    engine_id: "alva",
    product_session_id: "ps-1",
    message: "hello",
    created_at: 100,
    item_id: "assistant-1",
  };
  return { ...base, ...overrides } as HubTimelineEvent;
}

function renderTranscript(events: HubTimelineEvent[], streaming = false) {
  return render(
    <UIProvider>
      <I18nProvider>
        <div data-chat-scroll-region="">
          <ChatTranscript events={events} streaming={streaming} />
        </div>
      </I18nProvider>
    </UIProvider>,
  );
}

describe("ChatTranscript Markdown rendering", () => {
  it("keeps messages and execution in one ordered bounded list", () => {
    const events = [
      message({ role: "user", status: "user_message", message: "first", item_id: "u" }),
      message({ status: "activity_completed", message: "tool result", item_id: "t" }),
      message({ message: "last", item_id: "a" }),
    ];
    const { container } = render(<UIProvider><I18nProvider>
      <div data-chat-scroll-region="">
        <ChatTranscript events={events} streaming={false} renderExecution={(entry) => <span>{entry.message}</span>} />
      </div>
    </I18nProvider></UIProvider>);
    const rows = container.querySelectorAll("[data-bounded-list] > li");
    expect(rows).toHaveLength(3);
    expect(rows[0].textContent).toContain("first");
    expect(rows[1].textContent).toContain("tool result");
    expect(rows[2].textContent).toContain("last");
  });
  it("keeps raw < > & quote characters and never parses injected HTML", () => {
    const { container } = renderTranscript([
      message({
        message:
          'a < b && c > d "quoted"\n\n**bold < x** and `code < y` with & amp\n\n<script>alert(1)</script><img src=x onerror="boom" />',
      }),
    ]);

    const text = container.textContent ?? "";
    // R41: the raw characters survive verbatim — no "&lt;"/"&amp;" entities.
    expect(text).toContain('a < b && c > d "quoted"');
    expect(text).toContain("<script>alert(1)</script>");
    expect(text).toContain('<img src=x onerror="boom" />');
    expect(text).not.toContain("&lt;");
    expect(text).not.toContain("&amp;");
    // Injected markup stays text: no live script/img elements in the DOM.
    expect(container.querySelectorAll("script, img")).toHaveLength(0);
    // Inline formatting still works over raw characters.
    expect(container.querySelector(".chat-inline-code")?.textContent).toBe("code < y");
    expect(container.querySelector("strong")?.textContent).toBe("bold < x");
  });

  it("renders fenced code blocks whose body keeps raw characters and copies verbatim", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    const { container } = renderTranscript([
      message({ message: "```html\n<img src=x> && <b>\n```" }),
    ]);

    const body = container.querySelector(".chat-code-block__body");
    expect(body?.textContent).toBe("<img src=x> && <b>");

    // Locale defaults to zh-CN — query structurally, not by localized label.
    const button = container.querySelector<HTMLButtonElement>(
      ".chat-code-block__header button",
    );
    expect(button).toBeTruthy();
    fireEvent.click(button!);
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("<img src=x> && <b>"),
    );
    // The button confirms the copy completed.
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
  });
});

describe("ChatTranscript follow / jump-to-bottom", () => {
  it("stops following when the reader scrolls up and resumes on jump-to-latest", () => {
    const { container, rerender } = renderTranscript([
      message({ message: "first" }),
    ], true);

    const region = document.querySelector("[data-chat-scroll-region]") as HTMLElement;
    // jsdom has no layout: scrollHeight is 0, so the pinned check reads 0 < 48.
    expect(region.scrollTop).toBe(0);

    // The reader scrolls up (negative offsets read as "away from bottom").
    region.scrollTop = -500;
    fireEvent.scroll(region);
    // Unpinned → the jump-to-latest affordance appears.
    const jump = () => container.querySelector<HTMLButtonElement>(".chat-jump");
    expect(jump()).toBeTruthy();

    // Continued streaming while unpinned must NOT yank the reading position.
    rerender(
      <UIProvider>
        <I18nProvider>
          <div data-chat-scroll-region="">
            <ChatTranscript
              events={[
                message({ message: "first" }),
                message({ item_id: "assistant-2", message: "second", created_at: 200 }),
              ]}
              streaming
            />
          </div>
        </I18nProvider>
      </UIProvider>,
    );
    expect(region.scrollTop).toBe(-500);

    // Jump to latest re-pins and scrolls to the bottom (scrollHeight = 0).
    fireEvent.click(jump()!);
    expect(region.scrollTop).toBe(0);
    expect(jump()).toBeNull();
  });
});
