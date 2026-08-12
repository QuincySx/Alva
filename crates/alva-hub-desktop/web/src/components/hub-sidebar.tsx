// INPUT:  Hub session summaries, workspace state, and navigation callbacks
// OUTPUT: Cindy-style project navigation and durable session list
// POS:    Left product rail composed only from ALVA UI primitives

import { Blocks, FolderOpen, MessageSquarePlus, Workflow } from "lucide-react";
import type { HubSessionSummary } from "../bridge";
import {
  Button,
  NavGroup,
  NavItem,
  NavList,
  Sidebar,
  TextField,
} from "../ui";

export interface HubSidebarProps {
  activeSessionId: string | null;
  busy: boolean;
  formatUpdatedAt: (updatedAt: number) => string;
  onChooseWorkspace: () => void;
  onNewSession: () => void;
  onOpenSession: (session: HubSessionSummary) => void;
  onSessionQueryChange: (query: string) => void;
  sessionError: string | null;
  sessionQuery: string;
  sessions: HubSessionSummary[];
  totalSessionCount: number;
  workspace: string;
}

export function HubSidebar({
  activeSessionId,
  busy,
  formatUpdatedAt,
  onChooseWorkspace,
  onNewSession,
  onOpenSession,
  onSessionQueryChange,
  sessionError,
  sessionQuery,
  sessions,
  totalSessionCount,
  workspace,
}: HubSidebarProps) {
  return (
    <Sidebar className="hub-sidebar">
      <header className="hub-sidebar__brand">
        <div className="brand-mark">A</div>
        <div>
          <h1>ALVA Hub</h1>
          <p>One project. Every engine.</p>
        </div>
      </header>
      <nav className="hub-sidebar__primary" aria-label="Primary">
        <NavItem
          disabled={busy}
          icon={<MessageSquarePlus aria-hidden="true" size={17} />}
          label="New session"
          onClick={onNewSession}
        />
        <NavItem
          icon={<Workflow aria-hidden="true" size={17} />}
          label="Automations"
        />
        <NavItem
          icon={<Blocks aria-hidden="true" size={17} />}
          label="Plugins"
        />
      </nav>
      <div className="hub-sidebar__search">
        <TextField
          aria-label="Search sessions"
          placeholder="Search sessions…"
          value={sessionQuery}
          onChange={(event) => onSessionQueryChange(event.target.value)}
        />
      </div>
      <NavGroup
        className="hub-sidebar__sessions"
        count={
          sessionQuery.trim()
            ? `${sessions.length}/${totalSessionCount}`
            : totalSessionCount
        }
        label="Sessions"
      >
        <NavList>
          {sessionError ? (
            <small className="session-error">{sessionError}</small>
          ) : sessions.length === 0 ? (
            <small className="session-empty">
              {sessionQuery.trim()
                ? "No sessions match that search."
                : "Hub runs will appear here."}
            </small>
          ) : (
            sessions.map((session) => (
              <NavItem
                active={activeSessionId === session.product_session_id}
                description={session.workspace.split("/").at(-1)}
                key={session.product_session_id}
                label={session.last_prompt ?? "Untitled session"}
                meta={`${session.engine_id} · ${session.status} · ${formatUpdatedAt(session.updated_at)}`}
                onClick={() => onOpenSession(session)}
                title={session.workspace}
              />
            ))
          )}
        </NavList>
      </NavGroup>
      <footer className="hub-sidebar__workspace">
        <div>
          <FolderOpen aria-hidden="true" size={16} />
          <span>
            <strong>{workspace ? workspace.split("/").at(-1) : "No workspace"}</strong>
            <small>{workspace || "Choose a local project"}</small>
          </span>
        </div>
        <Button onClick={onChooseWorkspace} size="small" variant="ghost">
          Choose
        </Button>
      </footer>
    </Sidebar>
  );
}
