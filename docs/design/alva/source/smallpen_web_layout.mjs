// INPUT: SmallPen Web alpha.18 projection and its bundled native layout engine
// OUTPUT: Native geometry for editor-menu catalogs, secondary roots and subpage-return consumers in Web
// POS: Opt-in preview repair; no installed files or canonical design data are changed.
import { createHash } from 'node:crypto';
import { registerHooks } from 'node:module';
import { Worker } from 'node:worker_threads';

const expected = '0d48b72e54724fd460e4475f34c3ee4121ba03e16d3eb213a88f469442f8c1c1';
const marker = '        const locatedNodes = adaptLocatedInstances(projected.nodes);';
const componentMarker = '          adaptLocatedInstances(projectComponentVariant(product, variant, options)),';
const serverExpected = 'b27ae2c51c9c02a257713b7d179888c60b527cd24ddcf7a7632fe1c4efda9278';
const sampleMarker = '          cached = {\n            body: JSON.stringify({\n              combinationId,';
const sheetExpected = '215f03bcca21f272bc4970bd55ee6ca28dc3e90368f0f2747dcf35d070d6056f';
const sizeMarker = '    if (!node) return;\n    const x = id === sample.rootId ? 0 :';
const menuComponents = new Set(['cmp_alva_composermenu', 'cmp_alva_menuitem']);
const cache = new Map();
const digest = value => createHash('sha256').update(value).digest('hex');
let measurementWorker;
let measurementTail = Promise.resolve();
let measurementWaiting = 0;

function measure(product, projected, options, moduleUrl) {
  if (measurementWaiting >= 16) {
    return Promise.reject(new Error('SmallPen preview measurement queue is full; retry after the current layout settles.'));
  }
  measurementWaiting++;
  // Admit one native request before cloning, not a worker queue of Packages.
  const result = measurementTail.then(() => dispatchMeasurement(product, projected, options, moduleUrl));
  measurementTail = result.catch(() => {});
  return result.finally(() => measurementWaiting--);
}

function renderInputs(snapshot) {
  if (!snapshot) return snapshot;
  const assets = snapshot.manifest.entries.assets;
  return { revision: snapshot.revision, blobs: snapshot.blobs,
    manifest: { packageId: snapshot.manifest.packageId, entries: { assets } },
    entries: Object.fromEntries(assets.map(entry => [entry, snapshot.entries[entry]])),
    runtime: { file: snapshot.runtime.file, fonts: snapshot.runtime.fonts, media: snapshot.runtime.media } };
}

function dispatchMeasurement(product, projected, options, moduleUrl) {
  let state = measurementWorker;
  if (!state) {
    const worker = new Worker(new URL('./smallpen_layout_worker.mjs', import.meta.url), { execArgv: [] });
    state = { worker, pending: new Map(), serial: 0, idle: null };
    measurementWorker = state;
    const failed = error => {
      if (measurementWorker === state) measurementWorker = undefined;
      clearTimeout(state.idle);
      for (const request of state.pending.values()) request.reject(error);
      state.pending.clear();
    };
    worker.on('error', failed);
    worker.on('exit', code => failed(new Error(`SmallPen measurement thread exited (${code})`)));
    worker.on('message', ({ id, snapshot, error }) => {
      const request = state.pending.get(id);
      if (!request) return;
      state.pending.delete(id);
      if (error) request.reject(Object.assign(new Error(error.message), { code: error.code }));
      else request.resolve(snapshot);
      if (!state.pending.size) {
        // WASM/font heaps belong to this thread, not the long-lived Web server.
        state.idle = setTimeout(() => {
          if (measurementWorker === state) measurementWorker = undefined;
          void worker.terminate();
        }, 1000);
      }
    });
  }
  clearTimeout(state.idle);
  return new Promise((resolve, reject) => {
    const id = ++state.serial;
    state.pending.set(id, { resolve, reject });
    try {
      state.worker.postMessage({ id, product: renderInputs(product), projected,
        options: { ...options, foundation: renderInputs(options.foundation),
          libraries: options.libraries?.map(renderInputs) }, moduleUrl });
    } catch (error) {
      state.pending.delete(id);
      if (!state.pending.size) {
        if (measurementWorker === state) measurementWorker = undefined;
        void state.worker.terminate();
      }
      reject(error);
    }
  });
}

// Load-time substitution is scoped to this process and this exact upstream
// module. An upgrade must be reviewed; silently applying an old patch is unsafe.
registerHooks({
  load(url, context, nextLoad) {
    const result = nextLoad(url, context);
    const projection = url.endsWith('/@smallpen/background/src/web-projection.mjs');
    const server = url.endsWith('/@smallpen/background/src/server.mjs');
    const sheet = url.endsWith('/@smallpen/core/src/design-system-page.mjs');
    if (!projection && !server && !sheet) return result;
    const source = typeof result.source === 'string' ? result.source : Buffer.from(result.source).toString();
    if (sheet) {
      if (digest(source) !== sheetExpected || source.split(sizeMarker).length !== 2) {
        throw new Error('SmallPen Web sample sizing changed: review/remove the Alva alpha.18 layout repair.');
      }
      return { ...result, source: source.replace(sizeMarker,
        `    if (!node || (${JSON.stringify([...menuComponents])}.includes(sample.componentSetId) && node.visible === false)) return;\n    const x = id === sample.rootId ? 0 :`) };
    }
    if (server) {
      if (digest(source) !== serverExpected || source.split(sampleMarker).length !== 2) {
        throw new Error('SmallPen Web sample route changed: review/remove the Alva alpha.18 layout repair.');
      }
      return { ...result, source: `import { settleMenuSamples } from ${JSON.stringify(import.meta.url)};\n` +
        source.replace(sampleMarker, '          await settleMenuSamples(current, selected, { foundation }, import.meta.url);\n' + sampleMarker) };
    }
    if (digest(source) !== expected || source.split(marker).length !== 2 || source.split(componentMarker).length !== 2) {
      throw new Error('SmallPen Web projection changed: review/remove the Alva alpha.18 layout repair before using it.');
    }
    return { ...result, source: `import { settleOverlayLayouts, settleMenuVariant } from ${JSON.stringify(import.meta.url)};\n` +
      source.replace(marker, '        await settleOverlayLayouts(product, projected, presentation, options, import.meta.url);\n' + marker)
        .replace(componentMarker, '          adaptLocatedInstances(await settleMenuVariant(product, set, variant, options, import.meta.url)),') };
  },
});

export async function settleMenuVariant(product, set, variant, options, moduleUrl) {
  const { projectComponentVariant } = await import(new URL('../../core/src/design-projection.mjs', moduleUrl));
  const projected = { rootId: variant.rootId, nodes: projectComponentVariant(product, variant, options) };
  if (menuComponents.has(set.id)) {
    await settleLayouts(product, projected, [variant.rootId], `${set.id}/${variant.id}`, options, moduleUrl);
  }
  return projected.nodes;
}

export async function settleMenuSamples(product, refs, options, moduleUrl) {
  // Samples may be shared by core's cache and another theme. Never mutate it.
  refs.componentSamples = (refs.componentSamples ?? []).map(sample =>
    menuComponents.has(sample.componentSetId) && !sample.error
      ? { ...sample, nodes: structuredClone(sample.nodes) } : sample);
  for (const sample of refs.componentSamples) {
    if (!menuComponents.has(sample.componentSetId) || sample.error) continue;
    await settleLayouts(product, sample, [sample.rootId], sample.key, options, moduleUrl);
  }
}

export async function settleOverlayLayouts(product, projected, presentation, options, moduleUrl) {
  const needsNativeLayout = id => {
    const node = projected.nodes[id];
    return [...menuComponents, 'cmp_alva_settingsnav', 'cmp_alva_step'].includes(node.componentRef?.assetId) ||
      node.name === 'WizardBack' || (node.children ?? []).some(needsNativeLayout);
  };
  const roots = (presentation.rootIds ?? [presentation.rootId])
    .filter(id => id !== presentation.rootId || needsNativeLayout(id));
  await settleLayouts(product, projected, roots, presentation.id, options, moduleUrl);
}

async function settleLayouts(product, projected, roots, presentationId, options, moduleUrl) {
  if (!roots.length) return;
  const key = digest(JSON.stringify({ revision: product.revision, foundation: options.foundation?.revision,
    libraries: options.libraries?.map(x => [x.manifest.packageId, x.revision]), presentation: presentationId,
    roots, nodes: projected.nodes }));
  let geometry = cache.get(key);
  if (!geometry) {
    geometry = {};
    for (const rootId of roots) {
      const snapshot = await measure(product, { ...projected, rootId }, options, moduleUrl);
      const parents = new Map();
      for (const [id, node] of Object.entries(projected.nodes)) {
        for (const child of node.children ?? []) parents.set(child, id);
      }
      for (const [id, measured] of Object.entries(snapshot.nodes)) {
        if (!measured.visible) continue;
        const bounds = measured.layoutBox;
        if (!bounds || !Object.values(bounds).every(Number.isFinite)) {
          throw new Error(`Native layout unavailable: ${presentationId}/${id}`);
        }
        const parentId = parents.get(id);
        const parent = parentId ? snapshot.nodes[parentId]?.layoutBox : null;
        if (parentId && !parent) throw new Error(`Native overlay parent unavailable: ${parentId}`);
        geometry[id] = { x: bounds.x - (parent?.x ?? 0), y: bounds.y - (parent?.y ?? 0),
          width: bounds.width, height: bounds.height };
      }
    }
    // Keep a pass of current consumer pages and menu variants cached, while
    // bounding memory across edits/themes. Each key includes all projected
    // inputs; stale geometry cannot be reused after a content/theme/font edit.
    if (cache.size >= 32) cache.delete(cache.keys().next().value);
    cache.set(key, geometry);
  }
  for (const [id, bounds] of Object.entries(geometry)) {
    Object.assign(projected.nodes[id], bounds);
    // The Web adapter gives an unpainted FRAME a white default. Keep the
    // canonical no-paint intent explicit in this projection only.
    const node = projected.nodes[id];
    if (['FRAME', 'INSTANCE'].includes(node.type) && (!node.fills || node.fills.length === 0)) {
      node.fills = [{ type: 'solid', color: '#FFFFFF', opacity: 0 }];
    }
  }
}
