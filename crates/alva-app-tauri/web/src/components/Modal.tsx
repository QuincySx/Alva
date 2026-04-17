import { useEffect, type ReactNode } from "react";

interface ModalProps {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  /** Optional max width class — defaults to a large settings-style modal. */
  widthClass?: string;
}

/**
 * Lightweight modal: fixed overlay + centered panel + ESC close. No portal —
 * we render in place since the whole app is a single webview.
 */
export function Modal({
  open,
  onClose,
  children,
  widthClass = "w-[880px] max-w-[calc(100vw-64px)]",
}: ModalProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={onClose}
      />
      <div
        className={`relative ${widthClass} h-[580px] max-h-[calc(100vh-64px)] rounded-lg border border-neutral-800 bg-neutral-950 shadow-2xl overflow-hidden flex flex-col`}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
