/**
 * Reusable right-side slide-over panel with overlay and close button.
 * Used by the "Manage installs" panel in InstanceDetail (CP5).
 */

import { useEffect } from "react";
import { X } from "lucide-react";

interface SlideOverProps {
  open: boolean;
  onClose: () => void;
  title: string;
  /** Panel width class. Defaults to "w-full max-w-xl". */
  widthClass?: string;
  children: React.ReactNode;
}

export function SlideOver({
  open,
  onClose,
  title,
  widthClass = "w-full max-w-xl",
  children,
}: SlideOverProps) {
  // Close on Escape key.
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/50"
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Panel */}
      <div
        className={
          "relative flex h-full flex-col border-l border-border bg-background shadow-xl " +
          widthClass
        }
      >
        {/* Header */}
        <div className="flex shrink-0 items-center justify-between border-b border-border px-6 py-4">
          <h2 className="text-base font-semibold">{title}</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-muted transition-colors hover:bg-surface hover:text-foreground"
            aria-label="Close panel"
          >
            <X className="size-5" />
          </button>
        </div>

        {/* Scrollable body */}
        <div className="min-h-0 flex-1 overflow-y-auto p-6">{children}</div>
      </div>
    </div>
  );
}
