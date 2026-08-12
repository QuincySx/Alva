// INPUT:  Hub content, status, and layout composition props
// OUTPUT: Surface, Badge, Tabs, Switch, and empty-state primitives
// POS:    Shared visual grammar above controls and below business components

import React from "react";
import { Tabs, Switch as RadixSwitch } from "radix-ui";
import { cx } from "./utils";

export interface SurfaceProps extends React.HTMLAttributes<HTMLDivElement> {
  tone?: "base" | "raised" | "sunken";
}

export const Surface = React.forwardRef<HTMLDivElement, SurfaceProps>(
  function Surface({ className, tone = "base", ...props }, ref) {
    return (
      <div
        ref={ref}
        className={cx("ui-surface", `ui-surface--${tone}`, className)}
        {...props}
      />
    );
  },
);

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  tone?: "neutral" | "accent" | "success" | "warning" | "danger";
}

export function Badge({ className, tone = "neutral", ...props }: BadgeProps) {
  return (
    <span
      className={cx("ui-badge", `ui-badge--${tone}`, className)}
      {...props}
    />
  );
}

export interface NoticeProps extends React.HTMLAttributes<HTMLDivElement> {
  heading: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  tone?: "neutral" | "info" | "warning" | "danger";
}

export function Notice({
  heading,
  description,
  actions,
  tone = "neutral",
  className,
  children,
  ...props
}: NoticeProps) {
  return (
    <div
      className={cx("ui-notice", `ui-notice--${tone}`, className)}
      {...props}
    >
      <div className="ui-notice__copy">
        <strong>{heading}</strong>
        {description ? <p>{description}</p> : null}
        {children}
      </div>
      {actions ? <div className="ui-notice__actions">{actions}</div> : null}
    </div>
  );
}

export interface PanelSectionProps
  extends React.HTMLAttributes<HTMLElement> {
  heading: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  meta?: React.ReactNode;
}

export function PanelSection({
  heading,
  description,
  actions,
  meta,
  className,
  children,
  ...props
}: PanelSectionProps) {
  return (
    <section className={cx("ui-panel-section", className)} {...props}>
      <header className="ui-panel-section__header">
        <div className="ui-panel-section__copy">
          <strong>{heading}</strong>
          {description ? <small>{description}</small> : null}
        </div>
        {meta || actions ? (
          <div className="ui-panel-section__actions">
            {meta ? <small>{meta}</small> : null}
            {actions}
          </div>
        ) : null}
      </header>
      <div className="ui-panel-section__body">{children}</div>
    </section>
  );
}

export interface StatusListItemProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  heading: React.ReactNode;
  description?: React.ReactNode;
  status?: React.ReactNode;
  leading?: React.ReactNode;
  trailing?: React.ReactNode;
  unavailable?: boolean;
}

export const StatusListItem = React.forwardRef<
  HTMLButtonElement,
  StatusListItemProps
>(function StatusListItem(
  {
    heading,
    description,
    status,
    leading,
    trailing,
    unavailable,
    className,
    disabled,
    ...props
  },
  ref,
) {
  return (
    <button
      ref={ref}
      className={cx(
        "ui-status-item",
        unavailable && "ui-status-item--unavailable",
        className,
      )}
      aria-disabled={unavailable || undefined}
      disabled={disabled || unavailable}
      type="button"
      {...props}
    >
      {leading ? <span className="ui-status-item__leading">{leading}</span> : null}
      <span className="ui-status-item__copy">
        <strong>{heading}</strong>
        {description ? <small>{description}</small> : null}
      </span>
      {status ? <span className="ui-status-item__status">{status}</span> : null}
      {trailing ? <span className="ui-status-item__trailing">{trailing}</span> : null}
    </button>
  );
});

export const TabsRoot = Tabs.Root;

export const TabsList = React.forwardRef<
  React.ElementRef<typeof Tabs.List>,
  React.ComponentPropsWithoutRef<typeof Tabs.List>
>(function TabsList({ className, ...props }, ref) {
  return (
    <Tabs.List ref={ref} className={cx("ui-tabs", className)} {...props} />
  );
});

export const TabsTrigger = React.forwardRef<
  React.ElementRef<typeof Tabs.Trigger>,
  React.ComponentPropsWithoutRef<typeof Tabs.Trigger>
>(function TabsTrigger({ className, ...props }, ref) {
  return (
    <Tabs.Trigger
      ref={ref}
      className={cx("ui-tabs__trigger", className)}
      {...props}
    />
  );
});

export const TabsContent = React.forwardRef<
  React.ElementRef<typeof Tabs.Content>,
  React.ComponentPropsWithoutRef<typeof Tabs.Content>
>(function TabsContent({ className, ...props }, ref) {
  return (
    <Tabs.Content
      ref={ref}
      className={cx("ui-tabs__content", className)}
      {...props}
    />
  );
});

export interface SwitchProps
  extends React.ComponentPropsWithoutRef<typeof RadixSwitch.Root> {
  label: string;
  description?: string;
}

export function Switch({ label, description, className, ...props }: SwitchProps) {
  return (
    <label className="ui-switch-field">
      <span className="ui-switch-copy">
        <strong>{label}</strong>
        {description ? <small>{description}</small> : null}
      </span>
      <RadixSwitch.Root
        className={cx("ui-switch", className)}
        aria-label={label}
        {...props}
      >
        <RadixSwitch.Thumb className="ui-switch__thumb" />
      </RadixSwitch.Root>
    </label>
  );
}

export interface EmptyStateProps {
  icon?: React.ReactNode;
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  className?: string;
}

export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: EmptyStateProps) {
  return (
    <div className={cx("ui-empty-state", className)}>
      {icon ? <div className="ui-empty-state__icon">{icon}</div> : null}
      <strong>{title}</strong>
      {description ? <p>{description}</p> : null}
      {action}
    </div>
  );
}

export interface PageHeaderProps {
  title: React.ReactNode;
  description?: React.ReactNode;
  meta?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}

export function PageHeader({
  title,
  description,
  meta,
  actions,
  className,
}: PageHeaderProps) {
  return (
    <header className={cx("ui-page-header", className)}>
      <div className="ui-page-header__copy">
        <div className="ui-page-header__title">
          <h2>{title}</h2>
          {meta ? <span>{meta}</span> : null}
        </div>
        {description ? <p>{description}</p> : null}
      </div>
      {actions ? <div className="ui-page-header__actions">{actions}</div> : null}
    </header>
  );
}

export const CardGrid = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(function CardGrid({ className, ...props }, ref) {
  return <div ref={ref} className={cx("ui-card-grid", className)} {...props} />;
});

export interface ListRowProps extends React.HTMLAttributes<HTMLDivElement> {
  leading?: React.ReactNode;
  heading: React.ReactNode;
  description?: React.ReactNode;
  meta?: React.ReactNode;
  actions?: React.ReactNode;
}

export function ListRow({
  leading,
  heading,
  description,
  meta,
  actions,
  className,
  ...props
}: ListRowProps) {
  return (
    <div className={cx("ui-list-row", className)} {...props}>
      {leading ? <div className="ui-list-row__leading">{leading}</div> : null}
      <div className="ui-list-row__copy">
        <strong>{heading}</strong>
        {description ? <small>{description}</small> : null}
      </div>
      {meta ? <div className="ui-list-row__meta">{meta}</div> : null}
      {actions ? <div className="ui-list-row__actions">{actions}</div> : null}
    </div>
  );
}

export interface SettingsGroupProps extends React.HTMLAttributes<HTMLElement> {
  heading: React.ReactNode;
  description?: React.ReactNode;
}

export function SettingsGroup({
  heading,
  description,
  children,
  className,
  ...props
}: SettingsGroupProps) {
  return (
    <section className={cx("ui-settings-group", className)} {...props}>
      <header>
        <h3>{heading}</h3>
        {description ? <p>{description}</p> : null}
      </header>
      <div className="ui-settings-group__body">{children}</div>
    </section>
  );
}

export interface SettingsRowProps extends React.HTMLAttributes<HTMLDivElement> {
  heading: React.ReactNode;
  description?: React.ReactNode;
  control?: React.ReactNode;
}

export function SettingsRow({
  heading,
  description,
  control,
  className,
  ...props
}: SettingsRowProps) {
  return (
    <div className={cx("ui-settings-row", className)} {...props}>
      <div>
        <strong>{heading}</strong>
        {description ? <small>{description}</small> : null}
      </div>
      {control ? <div className="ui-settings-row__control">{control}</div> : null}
    </div>
  );
}
