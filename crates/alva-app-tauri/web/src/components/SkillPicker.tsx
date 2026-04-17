import { Check, Search, Sparkles, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { listAllSkills, type SkillInfo } from "../agent-bridge";

interface SkillPickerProps {
  /** Currently selected skill names (controlled). */
  selected: string[];
  onChange: (next: string[]) => void;
}

/**
 * Button + popover skill selector. Fetches the flat skill list on mount,
 * shows a searchable checkbox list. Selection is lifted to the parent so it
 * can be attached to each send_message call.
 */
export function SkillPicker({ selected, onChange }: SkillPickerProps) {
  const [open, setOpen] = useState(false);
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  // Close when clicking outside the popover.
  useEffect(() => {
    if (!open) return;
    const onDocDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", onDocDown);
    return () => window.removeEventListener("mousedown", onDocDown);
  }, [open]);

  // Lazy-load on first open.
  useEffect(() => {
    if (!open || loaded) return;
    (async () => {
      try {
        const list = await listAllSkills();
        setSkills(list);
      } catch (e) {
        console.error("listAllSkills failed", e);
      } finally {
        setLoaded(true);
      }
    })();
  }, [open, loaded]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return skills;
    return skills.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q),
    );
  }, [skills, query]);

  const toggle = (name: string) => {
    if (selected.includes(name)) {
      onChange(selected.filter((n) => n !== name));
    } else {
      onChange([...selected, name]);
    }
  };

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 rounded-md bg-neutral-800/50 hover:bg-neutral-800 px-2 py-1 text-xs"
      >
        <Sparkles size={12} />
        技能
        {selected.length > 0 && (
          <span className="ml-1 rounded bg-blue-700 px-1.5 text-[10px]">
            {selected.length}
          </span>
        )}
      </button>

      {open && (
        <div className="absolute bottom-full left-0 mb-2 w-80 rounded-lg border border-neutral-800 bg-neutral-950 shadow-2xl overflow-hidden">
          <div className="flex items-center gap-2 border-b border-neutral-800 px-3 py-2">
            <Search size={14} className="text-neutral-500" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="搜索技能"
              className="flex-1 bg-transparent outline-none text-xs"
              autoFocus
            />
            {query && (
              <button
                type="button"
                onClick={() => setQuery("")}
                className="text-neutral-500 hover:text-white"
              >
                <X size={12} />
              </button>
            )}
          </div>

          <div className="max-h-80 overflow-auto py-1">
            {!loaded && (
              <div className="px-3 py-4 text-xs text-neutral-500">
                加载中…
              </div>
            )}
            {loaded && filtered.length === 0 && (
              <div className="px-3 py-4 text-xs text-neutral-500">
                {skills.length === 0
                  ? "没发现任何技能。在 ~/.config/alva/skills 或 ~/.claude/skills 下放点 skill 包。"
                  : "无匹配"}
              </div>
            )}
            {filtered.map((s) => {
              const isSelected = selected.includes(s.name);
              return (
                <button
                  key={`${s.source_dir}::${s.name}`}
                  type="button"
                  onClick={() => toggle(s.name)}
                  className="w-full text-left px-3 py-2 hover:bg-neutral-900 flex items-start gap-2"
                >
                  <span
                    className={`mt-0.5 shrink-0 w-3.5 h-3.5 rounded-[3px] border flex items-center justify-center ${
                      isSelected
                        ? "bg-blue-600 border-blue-600"
                        : "border-neutral-600"
                    }`}
                  >
                    {isSelected && (
                      <Check size={9} strokeWidth={3.5} className="text-white" />
                    )}
                  </span>
                  <span className="flex-1 min-w-0">
                    <span className="block text-xs font-medium truncate">
                      {s.name}
                    </span>
                    <span className="block text-[10px] text-neutral-500 truncate">
                      {s.description || s.kind}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
