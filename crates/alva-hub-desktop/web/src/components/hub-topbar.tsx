// INPUT:  Conversation title, workspace, status, and panel visibility callbacks
// OUTPUT: Compact Cindy-style chat header with details and workbench actions
// POS:    Central-column chrome between App state and the conversation timeline

import { Menu, PanelRight } from "lucide-react";
import { useI18n } from "../i18n";
import { Button, IconButton } from "../ui";

export interface HubTopbarProps {
  detailsAvailable: boolean;
  detailsOpen: boolean;
  onToggleDetails: () => void;
  onToggleWorkbench: () => void;
  status: string;
  statusLabel: string;
  title: string;
  workbenchOpen: boolean;
  workspace: string;
  /** UI08: friendly identity label (never a raw session UUID). */
  workspaceLabel?: string;
  /** UI10: narrow-viewport sidebar sheet toggle (rendered <760px only). */
  onToggleSidebar?: () => void;
}

export function HubTopbar({
  detailsAvailable,
  detailsOpen,
  onToggleDetails,
  onToggleWorkbench,
  status,
  statusLabel,
  title,
  workbenchOpen,
  workspace,
  workspaceLabel,
  onToggleSidebar,
}: HubTopbarProps) {
  const { t } = useI18n();
  return (
    <header className="topbar">
      <div className="topbar__title">
        <strong>{title}</strong>
        <small title={workspace || undefined}>
          {workspace
            ? workspaceLabel ?? workspace.split("/").at(-1)
            : t("workspace.chooseToBegin")}
        </small>
      </div>
      <div className="topbar__actions">
        {onToggleSidebar ? (
          <IconButton
            className="hub-topbar__menu"
            icon={<Menu aria-hidden="true" size={16} />}
            label={t("nav.primary")}
            onClick={onToggleSidebar}
            variant="ghost"
          />
        ) : null}
        {!workbenchOpen ? (
          <IconButton
            icon={<PanelRight aria-hidden="true" size={16} />}
            label={t("topbar.openWorkbench")}
            onClick={onToggleWorkbench}
            variant="ghost"
          />
        ) : null}
        <Button
          disabled={!detailsAvailable}
          onClick={onToggleDetails}
          size="small"
          variant="ghost"
        >
          {detailsOpen ? t("topbar.hideDetails") : t("topbar.details")}
        </Button>
        <div
          aria-live="polite"
          className={`status-pill status-${status}`}
        >
          <span />
          {statusLabel}
        </div>
      </div>
    </header>
  );
}
