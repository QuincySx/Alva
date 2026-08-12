// INPUT:  Collapsible content, title, metadata, and controlled open state
// OUTPUT: Shared disclosure section with keyboard and screen-reader behavior
// POS:    Canonical expandable container for activity, settings, and details

import React from "react";
import { ChevronRight } from "lucide-react";
import { Collapsible } from "radix-ui";
import { cx } from "./utils";

export interface DisclosureProps
  extends Omit<
    React.ComponentPropsWithoutRef<typeof Collapsible.Root>,
    "title"
  > {
  title: React.ReactNode;
  description?: React.ReactNode;
  meta?: React.ReactNode;
  actions?: React.ReactNode;
  contentClassName?: string;
}

export function Disclosure({
  title,
  description,
  meta,
  actions,
  children,
  className,
  contentClassName,
  ...props
}: DisclosureProps) {
  return (
    <Collapsible.Root className={cx("ui-disclosure", className)} {...props}>
      <div className="ui-disclosure__row">
        <Collapsible.Trigger className="ui-disclosure__trigger">
          <ChevronRight aria-hidden="true" size={15} />
          <span className="ui-disclosure__copy">
            <strong>{title}</strong>
            {description ? <small>{description}</small> : null}
          </span>
          {meta ? <span className="ui-disclosure__meta">{meta}</span> : null}
        </Collapsible.Trigger>
        {actions ? <div className="ui-disclosure__actions">{actions}</div> : null}
      </div>
      <Collapsible.Content
        className={cx("ui-disclosure__content", contentClassName)}
      >
        {children}
      </Collapsible.Content>
    </Collapsible.Root>
  );
}
