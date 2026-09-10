// INPUT:  Hub session summaries and navigation callbacks
// OUTPUT: Cindy-style product navigation and durable session list
// POS:    Left product rail composed only from ALVA UI primitives

import { Blocks, MessageSquarePlus, Search, Settings, Workflow } from "lucide-react";
import type { HubSessionSummary } from "../bridge";
import { useI18n } from "../i18n";
import type { HubPage } from "./product-pages";
import {
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
  onNewSession: () => void;
  onOpenSession: (session: HubSessionSummary) => void;
  onSessionQueryChange: (query: string) => void;
  sessionError: string | null;
  sessionQuery: string;
  sessions: HubSessionSummary[];
  totalSessionCount: number;
  activePage: HubPage;
  onNavigate: (page: HubPage) => void;
}

export function HubSidebar({
  activeSessionId,
  busy,
  formatUpdatedAt,
  onNewSession,
  onOpenSession,
  onSessionQueryChange,
  sessionError,
  sessionQuery,
  sessions,
  totalSessionCount,
  activePage,
  onNavigate,
}: HubSidebarProps) {
  const { localize, t } = useI18n();
  return (
    <Sidebar className="hub-sidebar">
      <header className="hub-sidebar__brand">
        <div className="brand-mark">A</div>
        <div>
          <h1>ALVA Hub</h1>
          <p>{t("brand.tagline")}</p>
        </div>
      </header>
      <nav className="hub-sidebar__primary" aria-label={t("nav.primary")}>
        <NavItem
          active={activePage === "conversation" && activeSessionId === null}
          disabled={busy}
          icon={<MessageSquarePlus aria-hidden="true" size={17} />}
          label={t("nav.newSession")}
          onClick={onNewSession}
        />
        <NavItem
          active={activePage === "automations"}
          icon={<Workflow aria-hidden="true" size={17} />}
          label={t("nav.automations")}
          onClick={() => onNavigate("automations")}
        />
        <NavItem
          active={activePage === "extensions"}
          icon={<Blocks aria-hidden="true" size={17} />}
          label={t("nav.plugins")}
          onClick={() => onNavigate("extensions")}
        />
        <NavItem
          active={activePage === "search"}
          icon={<Search aria-hidden="true" size={17} />}
          label={t("nav.search")}
          onClick={() => onNavigate("search")}
        />
        <NavItem
          active={activePage === "settings"}
          icon={<Settings aria-hidden="true" size={17} />}
          label={t("nav.settings")}
          onClick={() => onNavigate("settings")}
        />
      </nav>
      <div className="hub-sidebar__search">
        <TextField
          aria-label={t("nav.searchSessions")}
          placeholder={t("nav.searchSessionsPlaceholder")}
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
        label={t("nav.sessions")}
      >
        <NavList>
          {sessionError ? (
            <small className="session-error">{sessionError}</small>
          ) : sessions.length === 0 ? (
            <small className="session-empty">
              {sessionQuery.trim()
                ? t("nav.noSessionMatches")
                : t("nav.noSessions")}
            </small>
          ) : (
            sessions.map((session) => (
              <ContextMenuRoot key={session.product_session_id}>
                <ContextMenuTrigger asChild>
                  <NavItem
                    active={activePage === "conversation" && activeSessionId === session.product_session_id}
                    // UI08: independent sessions use a friendly label; the
                    // raw directory stays available via title/context menu.
                    description={session.workspace_kind === "session"
                      ? t("nav.independentSession")
                      : session.workspace.split("/").at(-1)}
                    label={session.last_prompt ?? t("nav.untitledSession")}
                    meta={`${session.engine_id} · ${localize(session.status)} · ${formatUpdatedAt(session.updated_at)}`}
                    onClick={() => onOpenSession(session)}
                    title={session.workspace}
                  />
                </ContextMenuTrigger>
                <ContextMenuContent aria-label={t("nav.sessionActions")}>
                  <ContextMenuItem onSelect={() => onOpenSession(session)}>{t("nav.openSession")}</ContextMenuItem>
                  <ContextMenuItem onSelect={() => void navigator.clipboard?.writeText(session.product_session_id)}>{t("nav.copySessionId")}</ContextMenuItem>
                  <ContextMenuSeparator />
                  <ContextMenuItem onSelect={() => onSessionQueryChange(session.workspace.split("/").at(-1) ?? "")}>{t("nav.findRelated")}</ContextMenuItem>
                </ContextMenuContent>
              </ContextMenuRoot>
            ))
          )}
        </NavList>
      </NavGroup>
    </Sidebar>
  );
}
