// INPUT: Available model state and explicit prompt/settings actions
// OUTPUT: ConversationStart with non-executing quick-start prompts
// POS: Useful empty conversation after workspace selection, not a fake message.
import { Button, Notice } from "../ui";
import { useI18n } from "../i18n";

export function ConversationStart({ modelAvailable, onPrompt, onSettings }: {
  modelAvailable: boolean;
  onPrompt: (prompt: string) => void;
  onSettings: () => void;
}) {
  const { t } = useI18n();
  return <section className="conversation-start" aria-labelledby="conversation-start-title">
    <h2 id="conversation-start-title">{t("home.title")}</h2><p>{t("home.description")}</p>
    {!modelAvailable ? <Notice tone="info" heading={t("home.chooseModel")} actions={
      <Button variant="secondary" onClick={onSettings}>{t("home.openSettings")}</Button>
    } /> : null}
    <div className="conversation-start__prompts">
      {(["home.inspect", "home.explain", "home.plan"] as const).map((key) =>
        <Button key={key} variant="secondary" onClick={() => onPrompt(t(key))}>{t(key)}</Button>)}
    </div><small>{t("home.noAutoRun")}</small>
  </section>;
}
