import { ChevronDown, Settings } from "lucide-react";
import { useActiveProviderConfig, useAppStore } from "../store/appStore";

/**
 * Top-left model indicator + opener. Shows the active provider config's name
 * and the model id; click to open the settings modal on the Models tab.
 */
export function ModelPicker() {
  const active = useActiveProviderConfig();
  const openSettings = useAppStore((s) => s.openSettings);

  return (
    <button
      type="button"
      onClick={openSettings}
      className="flex items-center gap-2 rounded-md bg-neutral-900 border border-neutral-800 hover:border-neutral-700 px-3 py-1.5 text-sm"
      title="点击打开模型设置"
    >
      {active ? (
        <>
          <span className="font-medium">{active.name}</span>
          <span className="text-neutral-500 text-xs font-mono">
            {active.model}
          </span>
        </>
      ) : (
        <>
          <Settings size={14} className="text-neutral-400" />
          <span className="text-neutral-400">选择模型</span>
        </>
      )}
      <ChevronDown size={14} className="text-neutral-500" />
    </button>
  );
}
