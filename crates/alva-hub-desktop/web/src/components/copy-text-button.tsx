// INPUT: Explicit user copy action and known message/code text
// OUTPUT: CopyTextButton with success/failure feedback and timer cleanup
// POS: Shared message/code/transcript action; never reads clipboard contents.
import { useEffect, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";
import { Button } from "../ui";
import { useI18n } from "../i18n";

export function CopyTextButton({ text, label }: { text: string | (() => string); label: string }) {
  const { t } = useI18n();
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const generation = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => {
    generation.current += 1;
    setState("idle");
    return () => { generation.current += 1; clearTimeout(timer.current); };
  }, [text]);
  async function copy() {
    const current = generation.current;
    try {
      await navigator.clipboard.writeText(typeof text === "function" ? text() : text);
      if (generation.current !== current) return;
      setState("copied");
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setState("idle"), 2000);
    } catch { if (generation.current === current) setState("failed"); }
  }
  return <span className="chat-copy-action"><Button aria-label={label} onClick={() => void copy()} size="small" variant="ghost">
    {state === "copied" ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}
    {state === "copied" ? t("chat.copied") : label}
  </Button>{state === "failed" ? <small role="status">{t("chat.copyFailed")}</small> : null}</span>;
}
