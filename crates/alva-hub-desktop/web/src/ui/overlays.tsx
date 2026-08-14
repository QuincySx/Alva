// INPUT:  Radix overlay state and ALVA-styled content props
// OUTPUT: Popover, menu, dialog, and tooltip building blocks
// POS:    Centralized focus, collision, dismissal, and portal behavior

import React from "react";
import { Check, ChevronRight, X } from "lucide-react";
import { ContextMenu, Dialog, DropdownMenu, Popover, Tooltip } from "radix-ui";
import { IconButton } from "./button";
import { cx } from "./utils";
import { useI18n } from "../i18n";

export const PopoverRoot = Popover.Root;
export const PopoverTrigger = Popover.Trigger;
export const PopoverClose = Popover.Close;

export const PopoverContent = React.forwardRef<
  React.ElementRef<typeof Popover.Content>,
  React.ComponentPropsWithoutRef<typeof Popover.Content> & {
    size?: "compact" | "default" | "wide" | "full-height";
  }
>(function PopoverContent(
  { className, align = "end", sideOffset = 8, collisionPadding = 12, size = "default", ...props },
  ref,
) {
  return (
    <Popover.Portal>
      <Popover.Content
        ref={ref}
        align={align}
        className={cx("ui-popover", className)}
        data-size={size}
        collisionPadding={collisionPadding}
        sideOffset={sideOffset}
        {...props}
      />
    </Popover.Portal>
  );
});

export const MenuRoot = DropdownMenu.Root;
export const MenuTrigger = DropdownMenu.Trigger;
export const MenuGroup = DropdownMenu.Group;
export const MenuRadioGroup = DropdownMenu.RadioGroup;
export const MenuSub = DropdownMenu.Sub;

export const ContextMenuRoot = ContextMenu.Root;
export const ContextMenuTrigger = ContextMenu.Trigger;

export const ContextMenuContent = React.forwardRef<
  React.ElementRef<typeof ContextMenu.Content>,
  React.ComponentPropsWithoutRef<typeof ContextMenu.Content>
>(function ContextMenuContent({ className, collisionPadding = 10, ...props }, ref) {
  return (
    <ContextMenu.Portal>
      <ContextMenu.Content
        ref={ref}
        className={cx("ui-menu", className)}
        collisionPadding={collisionPadding}
        {...props}
      />
    </ContextMenu.Portal>
  );
});

export const ContextMenuItem = React.forwardRef<
  React.ElementRef<typeof ContextMenu.Item>,
  React.ComponentPropsWithoutRef<typeof ContextMenu.Item> & { destructive?: boolean }
>(function ContextMenuItem({ className, destructive, ...props }, ref) {
  return (
    <ContextMenu.Item
      ref={ref}
      className={cx("ui-menu-item", destructive && "ui-menu-item--danger", className)}
      {...props}
    />
  );
});

export const ContextMenuSeparator = React.forwardRef<
  React.ElementRef<typeof ContextMenu.Separator>,
  React.ComponentPropsWithoutRef<typeof ContextMenu.Separator>
>(function ContextMenuSeparator({ className, ...props }, ref) {
  return <ContextMenu.Separator ref={ref} className={cx("ui-menu-separator", className)} {...props} />;
});

export const MenuContent = React.forwardRef<
  React.ElementRef<typeof DropdownMenu.Content>,
  React.ComponentPropsWithoutRef<typeof DropdownMenu.Content>
>(function MenuContent(
  { className, align = "start", sideOffset = 6, collisionPadding = 10, ...props },
  ref,
) {
  return (
    <DropdownMenu.Portal>
      <DropdownMenu.Content
        ref={ref}
        align={align}
        className={cx("ui-menu", className)}
        collisionPadding={collisionPadding}
        sideOffset={sideOffset}
        {...props}
      />
    </DropdownMenu.Portal>
  );
});

export const MenuItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenu.Item>,
  React.ComponentPropsWithoutRef<typeof DropdownMenu.Item> & {
    inset?: boolean;
    destructive?: boolean;
  }
>(function MenuItem({ className, inset, destructive, ...props }, ref) {
  return (
    <DropdownMenu.Item
      ref={ref}
      className={cx(
        "ui-menu-item",
        inset && "ui-menu-item--inset",
        destructive && "ui-menu-item--danger",
        className,
      )}
      {...props}
    />
  );
});

export const MenuCheckboxItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenu.CheckboxItem>,
  React.ComponentPropsWithoutRef<typeof DropdownMenu.CheckboxItem>
>(function MenuCheckboxItem({ children, className, ...props }, ref) {
  return (
    <DropdownMenu.CheckboxItem
      ref={ref}
      className={cx("ui-menu-item ui-menu-item--inset", className)}
      {...props}
    >
      <DropdownMenu.ItemIndicator className="ui-menu-indicator">
        <Check aria-hidden="true" size={14} />
      </DropdownMenu.ItemIndicator>
      {children}
    </DropdownMenu.CheckboxItem>
  );
});

export const MenuRadioItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenu.RadioItem>,
  React.ComponentPropsWithoutRef<typeof DropdownMenu.RadioItem>
>(function MenuRadioItem({ children, className, ...props }, ref) {
  return (
    <DropdownMenu.RadioItem
      ref={ref}
      className={cx("ui-menu-item ui-menu-item--inset", className)}
      {...props}
    >
      <DropdownMenu.ItemIndicator className="ui-menu-indicator">
        <Check aria-hidden="true" size={14} />
      </DropdownMenu.ItemIndicator>
      {children}
    </DropdownMenu.RadioItem>
  );
});

export const MenuLabel = React.forwardRef<
  React.ElementRef<typeof DropdownMenu.Label>,
  React.ComponentPropsWithoutRef<typeof DropdownMenu.Label>
>(function MenuLabel({ className, ...props }, ref) {
  return (
    <DropdownMenu.Label
      ref={ref}
      className={cx("ui-menu-label", className)}
      {...props}
    />
  );
});

export const MenuSeparator = React.forwardRef<
  React.ElementRef<typeof DropdownMenu.Separator>,
  React.ComponentPropsWithoutRef<typeof DropdownMenu.Separator>
>(function MenuSeparator({ className, ...props }, ref) {
  return (
    <DropdownMenu.Separator
      ref={ref}
      className={cx("ui-menu-separator", className)}
      {...props}
    />
  );
});

export const MenuSubTrigger = React.forwardRef<
  React.ElementRef<typeof DropdownMenu.SubTrigger>,
  React.ComponentPropsWithoutRef<typeof DropdownMenu.SubTrigger>
>(function MenuSubTrigger({ children, className, ...props }, ref) {
  return (
    <DropdownMenu.SubTrigger
      ref={ref}
      className={cx("ui-menu-item", className)}
      {...props}
    >
      {children}
      <ChevronRight className="ui-menu-trailing" aria-hidden="true" size={14} />
    </DropdownMenu.SubTrigger>
  );
});

export const MenuSubContent = React.forwardRef<
  React.ElementRef<typeof DropdownMenu.SubContent>,
  React.ComponentPropsWithoutRef<typeof DropdownMenu.SubContent>
>(function MenuSubContent({ className, sideOffset = 6, ...props }, ref) {
  return (
    <DropdownMenu.Portal>
      <DropdownMenu.SubContent
        ref={ref}
        className={cx("ui-menu", className)}
        sideOffset={sideOffset}
        {...props}
      />
    </DropdownMenu.Portal>
  );
});

export const DialogRoot = Dialog.Root;
export const DialogTrigger = Dialog.Trigger;
export const DialogClose = Dialog.Close;

export interface DialogContentProps
  extends Omit<
    React.ComponentPropsWithoutRef<typeof Dialog.Content>,
    "title"
  > {
  title: React.ReactNode;
  description?: React.ReactNode;
  hideClose?: boolean;
  size?: "compact" | "default" | "wide" | "full-height";
}

export const DialogContent = React.forwardRef<
  React.ElementRef<typeof Dialog.Content>,
  DialogContentProps
>(function DialogContent(
  { title, description, children, className, hideClose, size = "default", ...props },
  ref,
) {
  const { t } = useI18n();
  return (
    <Dialog.Portal>
      <Dialog.Overlay className="ui-dialog-overlay" />
      <Dialog.Content
        ref={ref}
        className={cx("ui-dialog", className)}
        data-size={size}
        aria-describedby={undefined}
        {...props}
      >
        <header className="ui-dialog__header">
          <div>
            <Dialog.Title className="ui-dialog__title">{title}</Dialog.Title>
            {description ? (
              <Dialog.Description className="ui-dialog__description">
                {description}
              </Dialog.Description>
            ) : null}
          </div>
          {!hideClose ? (
            <Dialog.Close asChild>
              <IconButton label={t("common.close")} variant="ghost" icon={<X size={16} />} />
            </Dialog.Close>
          ) : null}
        </header>
        <div className="ui-dialog__body">{children}</div>
      </Dialog.Content>
    </Dialog.Portal>
  );
});

export function UIProvider({ children }: { children: React.ReactNode }) {
  return (
    <Tooltip.Provider delayDuration={350} skipDelayDuration={100}>
      {children}
    </Tooltip.Provider>
  );
}
