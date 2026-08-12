// INPUT:  Navigation labels, active state, metadata, and actions
// OUTPUT: Consistent sidebar navigation groups and selectable rows
// POS:    Canonical navigation controls for projects, sessions, and pages

import React from "react";
import { cx } from "./utils";

export function NavGroup({
  label,
  count,
  actions,
  children,
  className,
}: {
  label: React.ReactNode;
  count?: React.ReactNode;
  actions?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={cx("ui-nav-group", className)}>
      <header className="ui-nav-group__header">
        <span>{label}</span>
        <div className="ui-nav-group__meta">
          {count !== undefined ? <small>{count}</small> : null}
          {actions}
        </div>
      </header>
      <div className="ui-nav-group__body">{children}</div>
    </section>
  );
}

export interface NavItemProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
  icon?: React.ReactNode;
  label: React.ReactNode;
  description?: React.ReactNode;
  meta?: React.ReactNode;
  trailing?: React.ReactNode;
}

export const NavItem = React.forwardRef<HTMLButtonElement, NavItemProps>(
  function NavItem(
    {
      active,
      icon,
      label,
      description,
      meta,
      trailing,
      className,
      type = "button",
      ...props
    },
    ref,
  ) {
    return (
      <button
        ref={ref}
        aria-current={active ? "page" : undefined}
        className={cx("ui-nav-item", active && "ui-nav-item--active", className)}
        type={type}
        {...props}
      >
        {icon ? <span className="ui-nav-item__icon">{icon}</span> : null}
        <span className="ui-nav-item__copy">
          <strong>{label}</strong>
          {description ? <span>{description}</span> : null}
          {meta ? <small>{meta}</small> : null}
        </span>
        {trailing ? <span className="ui-nav-item__trailing">{trailing}</span> : null}
      </button>
    );
  },
);

export function NavList({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return <div className={cx("ui-nav-list", className)}>{children}</div>;
}
