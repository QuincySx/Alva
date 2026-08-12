// INPUT:  Conversation title, workspace, status, and panel visibility callbacks
// OUTPUT: Compact Cindy-style chat header with details and workbench actions
// POS:    Central-column chrome between App state and the conversation timeline

import { PanelRight } from "lucide-react";
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
}: HubTopbarProps) {
  return (
    <header className="topbar">
      <div className="topbar__title">
        <strong>{title}</strong>
        <small>
          {workspace ? workspace.split("/").at(-1) : "Choose a project to begin"}
        </small>
      </div>
      <div className="topbar__actions">
        <IconButton
          icon={<PanelRight aria-hidden="true" size={16} />}
          label={workbenchOpen ? "Close workbench" : "Open workbench"}
          onClick={onToggleWorkbench}
          variant="ghost"
        />
        <Button
          disabled={!detailsAvailable}
          onClick={onToggleDetails}
          size="small"
          variant="ghost"
        >
          {detailsOpen ? "Hide details" : "Details"}
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
