// INPUT:  Sidebar, primary content, and optional workbench panel content
// OUTPUT: Responsive three-column desktop application frame
// POS:    Canonical Hub shell shared by chat, automation, plugins, and settings

import React from "react";
import { cx } from "./utils";

export interface AppFrameProps extends React.HTMLAttributes<HTMLDivElement> {
  sidebar: React.ReactNode;
  workbench?: React.ReactNode;
  workbenchOpen?: boolean;
}

export function AppFrame({
  sidebar,
  workbench,
  workbenchOpen = false,
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
}

export const Workbench = React.forwardRef<HTMLElement, WorkbenchProps>(
  function Workbench({ title, actions, children, className, ...props }, ref) {
    return (
      <aside
        ref={ref}
        className={cx("ui-workbench", className)}
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
