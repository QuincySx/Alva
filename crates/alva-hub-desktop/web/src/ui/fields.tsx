// INPUT:  Native field props and Radix Select state
// OUTPUT: Consistent ALVA text, textarea, and select controls
// POS:    Canonical form controls for composer, settings, and dialogs

import React from "react";
import { Check, ChevronDown } from "lucide-react";
import { Select } from "radix-ui";
import { cx } from "./utils";

export interface FieldFrameProps {
  label?: React.ReactNode;
  description?: React.ReactNode;
  error?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}

export function FieldFrame({
  label,
  description,
  error,
  children,
  className,
}: FieldFrameProps) {
  return (
    <label className={cx("ui-field", className)}>
      {label ? <span className="ui-field__label">{label}</span> : null}
      {description ? (
        <span className="ui-field__description">{description}</span>
      ) : null}
      {children}
      {error ? <span className="ui-field__error">{error}</span> : null}
    </label>
  );
}

export const TextField = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(function TextField({ className, ...props }, ref) {
  return <input ref={ref} className={cx("ui-input", className)} {...props} />;
});

export const TextArea = React.forwardRef<
  HTMLTextAreaElement,
  React.TextareaHTMLAttributes<HTMLTextAreaElement>
>(function TextArea({ className, ...props }, ref) {
  return <textarea ref={ref} className={cx("ui-textarea", className)} {...props} />;
});

export interface SelectOption {
  value: string;
  label: React.ReactNode;
  disabled?: boolean;
}

export interface SelectFieldProps {
  value?: string;
  defaultValue?: string;
  onValueChange?: (value: string) => void;
  options: SelectOption[];
  placeholder?: string;
  ariaLabel: string;
  disabled?: boolean;
  className?: string;
}

export function SelectField({
  value,
  defaultValue,
  onValueChange,
  options,
  placeholder,
  ariaLabel,
  disabled,
  className,
}: SelectFieldProps) {
  return (
    <Select.Root
      value={value}
      defaultValue={defaultValue}
      onValueChange={onValueChange}
      disabled={disabled}
    >
      <Select.Trigger
        aria-label={ariaLabel}
        className={cx("ui-select-trigger", className)}
      >
        <Select.Value placeholder={placeholder} />
        <Select.Icon asChild>
          <ChevronDown aria-hidden="true" size={14} />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Content
          className="ui-select-content"
          collisionPadding={10}
          position="popper"
          sideOffset={6}
        >
          <Select.Viewport className="ui-select-viewport">
            {options.map((option) => (
              <Select.Item
                className="ui-select-item"
                disabled={option.disabled}
                key={option.value}
                value={option.value}
              >
                <Select.ItemText>{option.label}</Select.ItemText>
                <Select.ItemIndicator className="ui-select-indicator">
                  <Check aria-hidden="true" size={14} />
                </Select.ItemIndicator>
              </Select.Item>
            ))}
          </Select.Viewport>
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  );
}
