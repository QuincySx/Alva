// INPUT:  Hub session summaries, workspace state, and navigation callbacks
// OUTPUT: Cindy-style project navigation and durable session list
// POS:    Left product rail composed only from ALVA UI primitives

import { Blocks, FolderOpen, MessageSquarePlus, Search, Settings, Workflow } from "lucide-react";
import type { HubSessionSummary } from "../bridge";
import type { HubPage } from "./product-pages";
import {
  Button,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuRoot,
  ContextMenuSeparator,
  ContextMenuTrigger,
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
  activePage: HubPage;
  onNavigate: (page: HubPage) => void;
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
  activePage,
  onNavigate,
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
          active={activePage === "conversation" && activeSessionId === null}
          disabled={busy}
          icon={<MessageSquarePlus aria-hidden="true" size={17} />}
          label="New session"
          onClick={onNewSession}
        />
        <NavItem
          active={activePage === "automations"}
          icon={<Workflow aria-hidden="true" size={17} />}
          label="Automations"
          onClick={() => onNavigate("automations")}
        />
        <NavItem
          active={activePage === "extensions"}
          icon={<Blocks aria-hidden="true" size={17} />}
          label="Plugins"
          onClick={() => onNavigate("extensions")}
        />
        <NavItem
          active={activePage === "search"}
          icon={<Search aria-hidden="true" size={17} />}
          label="Search"
          onClick={() => onNavigate("search")}
        />
        <NavItem
          active={activePage === "settings"}
          icon={<Settings aria-hidden="true" size={17} />}
          label="Settings"
          onClick={() => onNavigate("settings")}
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
              <ContextMenuRoot key={session.product_session_id}>
                <ContextMenuTrigger asChild>
                  <NavItem
                    active={activePage === "conversation" && activeSessionId === session.product_session_id}
                    description={session.workspace.split("/").at(-1)}
                    label={session.last_prompt ?? "Untitled session"}
                    meta={`${session.engine_id} · ${session.status} · ${formatUpdatedAt(session.updated_at)}`}
                    onClick={() => onOpenSession(session)}
                    title={session.workspace}
                  />
                </ContextMenuTrigger>
                <ContextMenuContent aria-label="Session actions">
                  <ContextMenuItem onSelect={() => onOpenSession(session)}>Open session</ContextMenuItem>
                  <ContextMenuItem onSelect={() => void navigator.clipboard?.writeText(session.product_session_id)}>Copy session ID</ContextMenuItem>
                  <ContextMenuSeparator />
                  <ContextMenuItem onSelect={() => onSessionQueryChange(session.workspace.split("/").at(-1) ?? "")}>Find related sessions</ContextMenuItem>
                </ContextMenuContent>
              </ContextMenuRoot>
            ))
          )}
        </NavList>
      </NavGroup>
      <ContextMenuRoot>
        <ContextMenuTrigger asChild>
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
        </ContextMenuTrigger>
        <ContextMenuContent aria-label="Project actions">
          <ContextMenuItem onSelect={onChooseWorkspace}>Choose another project</ContextMenuItem>
          {workspace ? <ContextMenuItem onSelect={() => void navigator.clipboard?.writeText(workspace)}>Copy project path</ContextMenuItem> : null}
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={() => onNavigate("search")}>Search this Hub</ContextMenuItem>
        </ContextMenuContent>
      </ContextMenuRoot>
    </Sidebar>
  );
}
