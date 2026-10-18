// INPUT: Current InspectorTool/GitTool/UsageTool with deterministic persisted DTOs
// OUTPUT: Independent R42/R54/R55 regression assertions
// POS: Round 6 archived review probe, copied temporarily beside component tests
import { render, screen, fireEvent } from '@testing-library/react';
import { expect, it } from 'vitest';
import { I18nProvider } from '../i18n';
import { UIProvider } from '../ui';
import { demoDetail } from '../demo-data';
import { InspectorTool, GitTool, UsageTool } from './workbench-tools';
import type { HubTimelineEvent } from './hub-timeline';
function wrap(child: React.ReactNode) { localStorage.setItem('alva-hub.locale','en'); return render(<UIProvider><I18nProvider>{child}</I18nProvider></UIProvider>); }
it('R42 locate expands the complete disclosure ancestry to expose the tool record', () => {
 const detail = demoDetail(); const effect = detail.turns[0].attempts[0].side_effects[0];
 const event = {product_session_id:detail.session.id, engine_id:'alva',status:'activity_completed', item_id:effect.tool_call_id, activity:{tool_name:effect.tool_name,succeeded:true,duration_ms:210}} as HubTimelineEvent;
 const {container} = wrap(<InspectorTool detail={detail} events={[event]} loadInspection={async()=>{throw new Error('fixture unavailable')}}/>);
 const button = Array.from(container.querySelectorAll('button')).find(b => b.textContent?.includes('定位记录') || b.textContent?.includes('Locate record'))!;
 expect(button).toBeTruthy(); expect(button.disabled).toBe(false); fireEvent.click(button);
 expect(container.querySelector(`[data-inspector-tool="${effect.tool_call_id}"]`)).not.toBeNull();
});
it('R55 attempts without usage still make the cost aggregate partial', () => {
 const detail = demoDetail(); const attempt=detail.turns[0].attempts[0]; detail.turns=[{...detail.turns[0],attempts:[attempt,{...attempt,attempt_id:'unknown',usage:null}]}];
 const {container}=wrap(<UsageTool detail={detail}/>);
 expect(container.textContent).toMatch(/部分统计|partial/i);
});
it('R54 read-only tool records do not claim changes anywhere in the Git surface', () => {
 const detail=demoDetail(); const attempt=detail.turns[0].attempts[0]; detail.turns=[{...detail.turns[0],attempts:[{...attempt,side_effects:[{tool_call_id:'read',tool_name:'file_read',succeeded:true,duration_ms:null,path:'README.md'}]}]}];
 const {container}=wrap(<GitTool detail={detail}/>);
 expect(container.textContent).not.toMatch(/变更|change/i);
});
