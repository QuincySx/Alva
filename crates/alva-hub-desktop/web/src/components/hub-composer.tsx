// INPUT:  Prompt/attachment draft, submit/attach action, model picker, and session controls
// OUTPUT: Chat-first composer with IME-safe submission, live Stop and explicit attachment availability
// POS:    Canonical Hub conversation input shared by every engine runtime

import type { FormEvent, KeyboardEvent, ReactNode, Ref } from "react";
import { ArrowUp, Plus, Square } from "lucide-react";
import { useI18n } from "../i18n";
import { IconButton, TextArea } from "../ui";

export interface HubComposerProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onStop?: () => void;
  placeholder: string;
  engineId: string;
  busy?: boolean;
  canStop?: boolean;
  disabled?: boolean;
  controls?: ReactNode;
  modelPicker?: ReactNode;
  status?: ReactNode;
  onAttach?: () => void;
  attachments?: ReactNode;
  hasAttachments?: boolean;
  /** R34: lets the page return keyboard focus to the input after a
   *  workspace/session is chosen instead of leaving it on a nav control. */
  inputRef?: Ref<HTMLTextAreaElement>;
}

export function HubComposer({
  value,
  onChange,
  onSubmit,
  onStop,
  placeholder,
  engineId,
  busy = false,
  canStop = false,
  disabled = false,
  controls,
  modelPicker,
  status,
  onAttach,
  attachments,
  hasAttachments = false,
  inputRef,
}: HubComposerProps) {
  const { t } = useI18n();
  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.nativeEvent.isComposing || event.keyCode === 229 || busy || disabled) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
  }

  return (
    <div className="hub-composer-wrap">
      <form className="hub-composer" onSubmit={onSubmit}>
        {attachments}
        <TextArea
          aria-label={t("composer.message")}
          className="hub-composer__input"
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          ref={inputRef}
          value={value}
        />
        <footer className="hub-composer__footer">
          <div className="hub-composer__controls">
            <IconButton
              icon={<Plus aria-hidden="true" size={18} />}
              label={t("composer.attach")}
              disabled={!onAttach || busy || disabled}
              onClick={onAttach}
              variant="ghost"
            />
            {controls}
          </div>
          <div className="hub-composer__actions">
            {modelPicker}
            <IconButton
              className="hub-composer__send"
              disabled={busy ? !canStop : disabled || (!value.trim() && !hasAttachments)}
              icon={
                busy ? (
                  <Square aria-hidden="true" fill="currentColor" size={13} />
                ) : (
                  <ArrowUp aria-hidden="true" size={18} />
                )
              }
              label={busy ? (canStop ? t("composer.stop") : t("composer.starting")) : t("composer.runWith", { engine: engineId })}
              onClick={busy && canStop ? onStop : undefined}
              type={busy ? "button" : "submit"}
              variant="primary"
            />
          </div>
        </footer>
      </form>
      {status ? <div className="hub-composer-status">{status}</div> : null}
    </div>
  );
}
