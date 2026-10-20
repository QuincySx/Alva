// INPUT: Explicit author SDK, tool arguments and scoped event metadata.
// OUTPUT: An echo tool and one model-request log through AEP v0.1.
// POS: Self-contained plugin bundle entry; no ambient configuration or credential discovery.
import { definePlugin, run, actions, textResult } from './alva-sdk.mjs';

await run(definePlugin({
  name: 'javascript-echo',
  version: '1',
  capabilities: ['host:log', 'host:state.get_metadata'],
  handlers: {
    on_llm_call_start: async (_, host) => {
      const metadata = await host.getMetadata();
      await host.log('JavaScript plugin observed a model request', 'info', { session: metadata.session_id });
      return actions.continue();
    },
  },
  tools: {
    echo: {
      description: 'Echo the explicitly supplied text',
      inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'], additionalProperties: false },
      execute: args => textResult(args.text),
    },
  },
}));
