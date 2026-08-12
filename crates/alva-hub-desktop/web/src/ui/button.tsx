// INPUT:  Native button props, semantic variants, icons, and loading state
// OUTPUT: ALVA Button and tooltip-backed IconButton primitives
// POS:    Canonical action controls for every Hub surface

import React from "react";
import { LoaderCircle } from "lucide-react";
import { Tooltip } from "radix-ui";
import { cx } from "./utils";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "small" | "medium" | "large" | "icon";
export type ControlDensity = "default" | "compact";

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  leadingIcon?: React.ReactNode;
  trailingIcon?: React.ReactNode;
  density?: ControlDensity;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    {
      className,
      variant = "secondary",
      size = "medium",
      loading = false,
      leadingIcon,
      trailingIcon,
      density = "default",
      disabled,
      children,
      type = "button",
      ...props
    },
    ref,
  ) {
    return (
      <button
        ref={ref}
        className={cx(
          "ui-button",
          `ui-button--${variant}`,
          `ui-button--${size}`,
          className,
        )}
        data-density={density}
        disabled={disabled || loading}
        type={type}
        {...props}
      >
        {loading ? (
          <LoaderCircle className="ui-button__spinner" aria-hidden="true" />
        ) : (
          leadingIcon
        )}
        {children ? <span className="ui-button__label">{children}</span> : null}
        {trailingIcon}
      </button>
    );
  },
);

export interface IconButtonProps extends Omit<ButtonProps, "children" | "size"> {
  label: string;
  icon: React.ReactNode;
  tooltipSide?: React.ComponentProps<typeof Tooltip.Content>["side"];
}

export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  function IconButton(
    { label, icon, tooltipSide = "top", ...buttonProps },
    ref,
  ) {
    return (
      <Tooltip.Root>
        <Tooltip.Trigger asChild>
          <Button
            ref={ref}
            aria-label={label}
            size="icon"
            {...buttonProps}
          >
            {icon}
          </Button>
        </Tooltip.Trigger>
        <Tooltip.Portal>
          <Tooltip.Content
            className="ui-tooltip"
            collisionPadding={8}
            side={tooltipSide}
            sideOffset={6}
          >
            {label}
          </Tooltip.Content>
        </Tooltip.Portal>
      </Tooltip.Root>
    );
  },
);
