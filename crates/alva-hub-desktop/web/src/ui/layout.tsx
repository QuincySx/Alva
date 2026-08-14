// INPUT:  Sidebar, primary content, and optional tabbed workbench panel content
// OUTPUT: Responsive split application frame with a resizable auxiliary workspace
// POS:    Canonical Hub shell shared by chat, automation, plugins, and settings

import React from "react";
import { cx } from "./utils";
import { useI18n } from "../i18n";

export interface AppFrameProps extends React.HTMLAttributes<HTMLDivElement> {
  sidebar: React.ReactNode;
  workbench?: React.ReactNode;
  workbenchOpen?: boolean;
  onWorkbenchDismiss?: () => void;
}

export function AppFrame({
  sidebar,
  workbench,
  workbenchOpen = false,
  onWorkbenchDismiss,
  children,
  className,
  style,
  ...props
}: AppFrameProps) {
  const { t } = useI18n();
  const frameRef = React.useRef<HTMLDivElement>(null);
  const resizingRef = React.useRef(false);
  const [workbenchWidth, setWorkbenchWidth] = React.useState(440);

  function clampWorkbenchWidth(width: number) {
    const frameWidth = frameRef.current?.clientWidth ?? window.innerWidth;
    return Math.max(340, Math.min(width, 760, Math.max(340, frameWidth - 640)));
  }

  function resizeWorkbench(clientX: number) {
    const frame = frameRef.current;
    if (!frame) return;
    setWorkbenchWidth(clampWorkbenchWidth(frame.getBoundingClientRect().right - clientX));
  }

  return (
    <div
      ref={frameRef}
      className={cx(
        "ui-app-frame",
        workbenchOpen && "ui-app-frame--workbench",
        className,
      )}
      style={{
        ...style,
        "--workbench-current-width": `${workbenchWidth}px`,
      } as React.CSSProperties}
      {...props}
    >
      {sidebar}
      <main className="ui-main-pane">{children}</main>
      {workbench ? (
        <button
          aria-label={t("workbench.dismissOverlay")}
          className="ui-workbench-scrim"
          data-state={workbenchOpen ? "open" : "closed"}
          onClick={onWorkbenchDismiss}
          type="button"
        />
      ) : null}
      {workbench ? (
        <div
          aria-hidden={!workbenchOpen}
          className="ui-workbench-region"
          data-state={workbenchOpen ? "open" : "closed"}
          ref={(element) => {
            if (element) (element as HTMLDivElement & { inert: boolean }).inert = !workbenchOpen;
          }}
        >
          <button
            aria-label={t("workbench.resize")}
            aria-orientation="vertical"
            aria-valuemax={760}
            aria-valuemin={340}
            aria-valuenow={Math.round(workbenchWidth)}
            className="ui-workbench-resize"
            onKeyDown={(event) => {
              if (event.key === "ArrowLeft") setWorkbenchWidth((width) => clampWorkbenchWidth(width + 24));
              if (event.key === "ArrowRight") setWorkbenchWidth((width) => clampWorkbenchWidth(width - 24));
            }}
            onPointerDown={(event) => {
              resizingRef.current = true;
              event.currentTarget.setPointerCapture(event.pointerId);
            }}
            onPointerMove={(event) => {
              if (resizingRef.current) resizeWorkbench(event.clientX);
            }}
            onPointerUp={(event) => {
              resizingRef.current = false;
              event.currentTarget.releasePointerCapture(event.pointerId);
            }}
            onPointerCancel={() => {
              resizingRef.current = false;
            }}
            role="separator"
            type="button"
          />
          {workbench}
        </div>
      ) : null}
    </div>
  );
}

export const Sidebar = React.forwardRef<
  HTMLElement,
  React.HTMLAttributes<HTMLElement>
>(function Sidebar({ className, ...props }, ref) {
  return (
    <aside ref={ref} className={cx("ui-sidebar", className)} {...props} />
  );
});

export interface WorkbenchProps
  extends Omit<React.HTMLAttributes<HTMLElement>, "title"> {
  title?: React.ReactNode;
  actions?: React.ReactNode;
  mode?: "docked" | "overlay";
}

export const Workbench = React.forwardRef<HTMLElement, WorkbenchProps>(
  function Workbench({ title, actions, children, className, mode = "docked", ...props }, ref) {
    return (
      <aside
        ref={ref}
        className={cx("ui-workbench", className)}
        data-mode={mode}
        {...props}
      >
        {title || actions ? (
          <header className="ui-workbench__header">
            <strong>{title}</strong>
            <div className="ui-workbench__actions">{actions}</div>
          </header>
        ) : null}
        <div className="ui-workbench__body">{children}</div>
      </aside>
    );
  },
);

export function PaneHeader({
  title,
  eyebrow,
  actions,
  className,
}: {
  title?: React.ReactNode;
  eyebrow?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}) {
  return (
    <header className={cx("ui-pane-header", className)}>
      <div className="ui-pane-header__copy">
        {eyebrow ? <span>{eyebrow}</span> : null}
        {title ? <strong>{title}</strong> : null}
      </div>
      <div className="ui-pane-header__actions">{actions}</div>
    </header>
  );
}

export const ContentFrame = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(function ContentFrame({ className, ...props }, ref) {
  return (
    <div
      ref={ref}
      className={cx("ui-content-frame", className)}
      {...props}
    />
  );
});

export const ScrollRegion = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(function ScrollRegion({ className, ...props }, ref) {
  return (
    <div
      ref={ref}
      className={cx("ui-scroll-region", className)}
      {...props}
    />
  );
});

export const DockRegion = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(function DockRegion({ className, ...props }, ref) {
  return (
    <div
      ref={ref}
      className={cx("ui-dock-region", className)}
      {...props}
    />
  );
});

export const WorkbenchPage = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(function WorkbenchPage({ className, ...props }, ref) {
  return (
    <div
      ref={ref}
      className={cx("ui-workbench-page", className)}
      {...props}
    />
  );
});
