// INPUT:  Sidebar, primary content, and optional workbench panel content
// OUTPUT: Responsive three-column desktop application frame
// POS:    Canonical Hub shell shared by chat, automation, plugins, and settings

import React from "react";
import { cx } from "./utils";

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
  ...props
}: AppFrameProps) {
  return (
    <div
      className={cx(
        "ui-app-frame",
        workbenchOpen && "ui-app-frame--workbench",
        className,
      )}
      {...props}
    >
      {sidebar}
      <main className="ui-main-pane">{children}</main>
      {workbenchOpen && workbench ? (
        <button
          aria-label="Dismiss workbench overlay"
          className="ui-workbench-scrim"
          onClick={onWorkbenchDismiss}
          type="button"
        />
      ) : null}
      {workbenchOpen && workbench ? workbench : null}
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
