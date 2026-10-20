// INPUT: Checked-in JS author SDK and explicit native AEP capability grants.
// OUTPUT: Actual hooks, reverse calls, scoped references and remote tool results.
// POS: End-to-end loader fixture, never discovered from global Node configuration.
import { definePlugin, run, actions, message, textResult } from './alva-sdk.mjs';

const plugin = definePlugin({
  name: 'fixture', version: '1',
  capabilities: ['host:log', 'host:state.get_metadata', 'host:state.get_messages'],
  handlers: {
    on_llm_call_start: async (params, host) => {
      if ((await host.getMetadata()).session_id !== 'native') throw new Error('wrong authority');
      const history = await host.getMessages();
      if (!history.some(x => x.content.some(y => y.type === 'text' && y.text))) throw new Error('wire text missing');
      return actions.modifyMessages([message('system', 'sdk-system'), ...params.messages]);
    },
    before_tool_call: async (params, host) => {
      if (params.toolCall.name === 'target') {
        await host.log('real JavaScript pre-intent callback');
        return actions.modifyArgs({ path: 'sdk-effective' });
      }
      return actions.continue();
    },
    after_tool_call: params => {
      if (params.toolCall.name === 'target') return actions.modifyResult(textResult('SDK transformed result', true));
      return actions.continue();
    },
    on_llm_call_end: params => {
      if (params.response.content.some(x => x.type === 'text' && x.text === 'done')) return actions.modifyResponse(message('assistant', 'sdk-done'));
      return actions.continue();
    },
  },
  tools: {
    echo: { description: 'Explicit fixture echo', inputSchema: { type: 'object' }, execute: async (args, host) => {
      if (args.hang) { await host.log('fixture tool entered'); return new Promise(() => {}); }
      if (args.fail) throw new Error('private fixture error');
      return textResult(`JS SDK echo: ${args.text ?? ''}`);
    } },
  },
});
await run(plugin).catch(() => { process.stderr.write('Plugin runtime failed\n'); process.exitCode = 1; });
