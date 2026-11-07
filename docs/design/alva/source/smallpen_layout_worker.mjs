// INPUT: One native layout module URL and read-only design projections from the preview process.
// OUTPUT: Native layout snapshots or explicit measurement errors over the parent message port.
// POS: Disposable preview measurement thread; no server, UI, package writes or production dependency.
import { parentPort } from 'node:worker_threads';

parentPort.on('message', async ({ id, product, projected, options, moduleUrl }) => {
  try {
    const { inspectProjection } = await import(new URL('../../local-package/src/layout-inspection.mjs', moduleUrl));
    const { snapshot } = await inspectProjection(product, projected, options);
    parentPort.postMessage({ id, snapshot });
  } catch (error) {
    parentPort.postMessage({ id, error: { message: error.message, code: error.code } });
  }
});
