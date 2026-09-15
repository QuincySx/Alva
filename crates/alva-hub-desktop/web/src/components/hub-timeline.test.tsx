// R76：工具自述的效果说明是运行期事实，任何 locale 都必须原样出现在审批卡上；
// 静态 UI 文案（标题/标签/按钮）本地化，不吞掉未收录英文说明；空说明不编造效果。
import { render, cleanup } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { UIProvider } from "../ui";
import { I18nProvider } from "../i18n";
import { HubTimeline, type HubTimelineEvent } from "./hub-timeline";

afterEach(cleanup);

const EFFECT_SUMMARY = "publishes the draft post to the production blog";

function permissionEvent(description: string | null): HubTimelineEvent {
  return {
    status: "permission_required",
    engine_id: "alva",
    product_session_id: "session-r76",
    message: null,
    permission_request: {
      request_id: "approval-r76",
      tool_name: "custom_publish",
      engine_session_id: "native-r76",
      description,
      resource: "prod-blog",
    },
  };
}

function renderCard(language: string, event: HubTimelineEvent) {
  localStorage.setItem("alva-hub.language", language);
  return render(
    <UIProvider>
      <I18nProvider>
        <HubTimeline
          activeSessionId="session-r76"
          answeredPermissions={{}}
          attemptIsLive
          events={[event]}
          onAnswerPermission={() => {}}
          onStop={() => {}}
          onSwitchBack={() => {}}
        />
      </I18nProvider>
    </UIProvider>,
  );
}

for (const language of ["zh-CN", "en-US"]) {
  it(`R76 ${language} permission card preserves the tool-authored effect summary`, () => {
    const { container } = renderCard(language, permissionEvent(EFFECT_SUMMARY));
    const card = container.querySelector(".permission-card");
    expect(card).not.toBeNull();
    // 工具名、资源、效果说明与审批控件同屏
    expect(card?.textContent).toContain("custom_publish");
    expect(card?.textContent).toContain("prod-blog");
    expect(card?.textContent).toContain(EFFECT_SUMMARY);
    expect(card?.querySelectorAll("button")).toHaveLength(2);
    // 说明不能被替换成通用诊断兜底文案
    expect(card?.textContent).not.toContain("暂无中文诊断摘要");
    expect(card?.textContent).not.toContain("No localized diagnostic is available");
  });
}

it("R76 empty description renders no fabricated effect text", () => {
  const { container } = renderCard("zh-CN", permissionEvent(null));
  const card = container.querySelector(".permission-card");
  expect(card).not.toBeNull();
  expect(card?.textContent).toContain("custom_publish");
  expect(card?.textContent).not.toContain("publishes");
  expect(card?.querySelectorAll("button")).toHaveLength(2);
});
