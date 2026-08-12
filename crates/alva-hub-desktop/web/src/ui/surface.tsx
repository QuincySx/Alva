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
