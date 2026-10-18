// INPUT: Fixed private C capability functions, no ambient state or module loader
// OUTPUT: Synchronous file/JSON/print/fetch bindings with explicit string coercion
// POS: [platform / WASM host] Trusted engine bootstrap, never evaluated in a browser or a model context
(() => {
  const raw = globalThis.__alva;
  const names = ['readFile','writeFile','appendFile','exists','remove','rename','mkdir','readDir','stat','glob','copyFile','print','fetch'];
  const call = Object.fromEntries(names.map((name, i) => [name, raw[i]]));
  Object.assign(globalThis, {
    readFile: path => call.readFile(String(path)),
    writeFile: (path, content) => call.writeFile(String(path), String(content)),
    appendFile: (path, content) => call.appendFile(String(path), String(content)),
    exists: path => call.exists(String(path)),
    remove: path => call.remove(String(path)),
    rename: (from, to) => call.rename(String(from), String(to)),
    mkdir: path => call.mkdir(String(path)),
    readDir: (path = '.') => call.readDir(String(path)),
    stat: path => call.stat(String(path)),
    glob: pattern => call.glob(String(pattern)),
    copyFile: (from, to) => call.copyFile(String(from), String(to)),
    readJson: path => JSON.parse(call.readFile(String(path))),
    writeJson: (path, value, space = 2) => call.writeFile(String(path), JSON.stringify(value, null, space)),
    print: (...values) => call.print(values.map(value => typeof value === 'string' ? value : JSON.stringify(value)).join(' ')),
    fetch: (url, init = {}) => {
      if (init === null || typeof init !== 'object') throw new TypeError('fetch init must be an object');
      const h = init.headers === undefined ? {} : init.headers;
      if (h === null || typeof h !== 'object') throw new TypeError('fetch headers require an object or tuple array');
      const headers = (Array.isArray(h) ? h : Object.entries(h)).map(entry => {
        if (!Array.isArray(entry) || entry.length !== 2) throw new TypeError('fetch headers require [name, value] pairs');
        return {name: String(entry[0]), value: String(entry[1])};
      });
      const r = call.fetch(String(init.method === undefined ? 'GET' : init.method), String(url), headers, String(init.body == null ? '' : init.body));
      return Object.freeze({status: r.status, ok: r.status >= 200 && r.status < 300, headers: Object.freeze(r.headers), body: r.body});
    }
  });
  delete globalThis.__alva;
})();
