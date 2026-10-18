// INPUT: Current ChatTranscript and live timeline reducer with deterministic events
// OUTPUT: Round-5 regression evidence for text preservation and event updates
// POS: Temporary review probe; archived outside production test directories
import { render } from "@testing-library/react";
import { expect, it } from "vitest";
import { I18nProvider } from "./i18n";
import { ChatTranscript } from "./components/chat-transcript";
import { mergeLiveTimeline } from "./App";
import type { HubTimelineEvent } from "./components/hub-timeline";
const entry: HubTimelineEvent = {status:"assistant_message", engine_id:"alva", product_session_id:"review", item_id:"item", message:null};
it("preserves literal prose and inline code without displaying HTML entities", () => {
  const text = 'a < b && c > d "quoted"';
  const {container} = render(<I18nProvider><ChatTranscript events={[{...entry,message:text + "\n\n`" + text + "`"}]} streaming={false}/></I18nProvider>);
  expect(container.querySelector(".chat-paragraph")?.textContent).toBe(text);
  expect(container.querySelector(".chat-inline-code")?.textContent).toBe(text);
});
it("updates execution progress to the latest failure reason", () => {
  const previous: HubTimelineEvent = {...entry,status:"activity_started", message:"bash running"};
  const failed: HubTimelineEvent = {...entry,status:"activity_completed", message:"bash failed"};
  expect(mergeLiveTimeline([previous],failed)[0].message).toBe("bash failed");
});
