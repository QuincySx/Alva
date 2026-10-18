// INPUT: HubTimeline permission DTO with the Rust custom_publish test's effect summary
// OUTPUT: Chinese approval-card regression and English control
// POS: Round 6 independent R57 end-user effect-summary evidence
import { render, cleanup } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';
import { UIProvider } from '../ui';
import { I18nProvider } from '../i18n';
import { HubTimeline, type HubTimelineEvent } from './hub-timeline';
afterEach(cleanup);
const summary = 'publishes the draft post to the production blog';
const event: HubTimelineEvent = { status:'permission_required',engine_id:'alva',product_session_id:'session-r57',permission_request:{ request_id:'approval-r57',tool_name:'custom_publish',engine_session_id:'native-r57',description:summary } };
for (const language of ['zh-CN','en-US']) {
 it(`R57 ${language} permission card preserves the custom tool effect summary`, () => {
  localStorage.setItem('alva-hub.language',language);
  const {container}=render(<UIProvider><I18nProvider><HubTimeline activeSessionId="session-r57" answeredPermissions={{}} attemptIsLive events={[event]} onAnswerPermission={()=>{}} onStop={()=>{}} onSwitchBack={()=>{}}/></I18nProvider></UIProvider>);
  const card=container.querySelector('.permission-card');
  expect(card).not.toBeNull();
  expect(card?.querySelectorAll('button')).toHaveLength(2);
  expect(card?.textContent).toContain(summary);
 });
}
