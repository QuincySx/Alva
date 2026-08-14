// INPUT:  new-session workspace actions and loading state
// OUTPUT: project-first choice surface with independent session as the alternative
// POS:    Conversation onboarding gate above workspace IPC and below the App page.

import { FolderKanban, MessageSquare } from "lucide-react";
import { useI18n } from "../i18n";
import { Badge, Button, Surface } from "../ui";

export interface WorkspaceGateProps {
  busy: boolean;
  onChooseProject: () => void;
  onCreateSession: () => void;
}

export function WorkspaceGate({
  busy,
  onChooseProject,
  onCreateSession,
}: WorkspaceGateProps) {
  const { t } = useI18n();
  return (
    <section className="hub-workspace-gate" aria-labelledby="workspace-gate-title">
      <header className="hub-workspace-gate__header">
        <h2 id="workspace-gate-title">{t("workspace.gateTitle")}</h2>
        <p>{t("workspace.gateDescription")}</p>
      </header>
      <div className="hub-workspace-gate__options">
        <Surface className="hub-workspace-gate__card" tone="raised">
          <div className="hub-workspace-gate__icon">
            <FolderKanban aria-hidden="true" size={20} />
          </div>
          <div className="hub-workspace-gate__title">
            <h3>{t("workspace.projectMode")}</h3>
            <Badge tone="accent">{t("workspace.recommended")}</Badge>
          </div>
          <p>{t("workspace.projectModeDescription")}</p>
          <Button
            disabled={busy}
            onClick={onChooseProject}
            size="large"
            variant="primary"
          >
            {t("workspace.selectProject")}
          </Button>
        </Surface>
        <Surface className="hub-workspace-gate__card" tone="base">
          <div className="hub-workspace-gate__icon">
            <MessageSquare aria-hidden="true" size={20} />
          </div>
          <div className="hub-workspace-gate__title">
            <h3>{t("workspace.sessionMode")}</h3>
          </div>
          <p>{t("workspace.sessionModeDescription")}</p>
          <Button
            disabled={busy}
            onClick={onCreateSession}
            size="large"
            variant="secondary"
          >
            {busy ? t("workspace.creatingSession") : t("workspace.createSession")}
          </Button>
        </Surface>
      </div>
    </section>
  );
}
