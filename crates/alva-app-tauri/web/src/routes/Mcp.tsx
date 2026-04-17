import { Plug } from "lucide-react";
import { useEffect, useState } from "react";
import { listMcpServers, type McpServerInfo } from "../agent-bridge";

export default function Mcp() {
  const [servers, setServers] = useState<McpServerInfo[]>([]);
  const [loading, setLoading] = useState(true);

  const reload = async () => {
    setLoading(true);
    try {
      setServers(await listMcpServers());
    } catch (e) {
      console.error("list MCP servers failed", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    reload();
  }, []);

  return (
    <div className="flex h-full flex-col bg-neutral-950 text-neutral-100">
      <header className="flex items-center gap-3 border-b border-neutral-900 px-6 py-4">
        <Plug size={18} className="text-blue-400" />
        <div className="text-lg font-semibold">MCP</div>
        <span className="flex-1" />
        <button
          type="button"
          onClick={reload}
          className="rounded bg-neutral-900 border border-neutral-800 hover:border-neutral-700 px-3 py-1.5 text-xs"
        >
          刷新
        </button>
      </header>

      <div className="border-b border-neutral-900 px-6 py-2 text-xs text-neutral-500">
        从 <code className="text-neutral-400">~/.alva/mcp.json</code> 读取。编辑
        JSON 文件后点刷新。
      </div>

      <main className="flex-1 overflow-auto px-6 py-4">
        {loading && <div className="text-neutral-500 text-sm">加载中…</div>}
        {!loading && servers.length === 0 && (
          <div className="flex flex-col items-start max-w-xl">
            <div className="text-sm text-neutral-400 mb-2">
              还没有 MCP 服务器。
            </div>
            <div className="text-xs text-neutral-500 mb-3">
              在{" "}
              <code className="text-neutral-400">~/.alva/mcp.json</code> 里写:
            </div>
            <pre className="rounded bg-neutral-900 border border-neutral-800 px-3 py-2 text-[11px] font-mono text-neutral-300 w-full overflow-auto">
              {`{
  "servers": [
    {
      "name": "github",
      "kind": "stdio",
      "command": "npx -y @modelcontextprotocol/server-github",
      "enabled": true
    },
    {
      "name": "search",
      "kind": "http",
      "url": "http://localhost:3100/mcp"
    }
  ]
}`}
            </pre>
          </div>
        )}
        <div className="space-y-2">
          {servers.map((s) => (
            <div
              key={s.id}
              className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4"
            >
              <div className="flex items-center gap-3">
                <Plug size={14} className="text-neutral-400" />
                <div className="text-sm font-medium">{s.name}</div>
                <span className="rounded bg-neutral-800 text-[10px] px-2 py-0.5 uppercase font-mono text-neutral-400">
                  {s.kind}
                </span>
                <span className="flex-1" />
                <span
                  className={`rounded-full px-2 py-0.5 text-[10px] border ${
                    s.enabled
                      ? "bg-green-950/50 border-green-900/60 text-green-400"
                      : "bg-neutral-900 border-neutral-800 text-neutral-500"
                  }`}
                >
                  {s.enabled ? "已启用" : "已禁用"}
                </span>
              </div>
              <div className="mt-2 text-[11px] font-mono text-neutral-500 truncate">
                {s.command_or_url}
              </div>
            </div>
          ))}
        </div>
      </main>
    </div>
  );
}
