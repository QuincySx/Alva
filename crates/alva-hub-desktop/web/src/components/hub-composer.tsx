// INPUT:  Prompt state, submit action, model picker, and session controls
// OUTPUT: Chat-first composer card with stable keyboard and running behavior
// POS:    Canonical Hub conversation input shared by every engine runtime

import type { FormEvent, KeyboardEvent, ReactNode } from "react";
import { ArrowUp, Plus, Square } from "lucide-react";
import { IconButton, TextArea } from "../ui";

export interface HubComposerProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onStop?: () => void;
  placeholder: string;
  engineId: string;
  busy?: boolean;
  disabled?: boolean;
  controls?: ReactNode;
  modelPicker?: ReactNode;
  status?: ReactNode;
  onAttach?: () => void;
}

export function HubComposer({
  value,
  onChange,
  onSubmit,
  onStop,
  placeholder,
  engineId,
  busy = false,
  disabled = false,
  controls,
  modelPicker,
  status,
  onAttach,
}: HubComposerProps) {
  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
  }

  return (
    <div className="hub-composer-wrap">
      <form className="hub-composer" onSubmit={onSubmit}>
        <TextArea
          aria-label="Message"
          className="hub-composer__input"
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          value={value}
        />
        <footer className="hub-composer__footer">
          <div className="hub-composer__controls">
            <IconButton
              icon={<Plus aria-hidden="true" size={18} />}
              label="Attach context"
              onClick={onAttach}
              variant="ghost"
            />
            {controls}
          </div>
          <div className="hub-composer__actions">
            {modelPicker}
            <IconButton
              className="hub-composer__send"
              disabled={disabled || (!busy && !value.trim())}
              icon={
                busy ? (
                  <Square aria-hidden="true" fill="currentColor" size={13} />
                ) : (
                  <ArrowUp aria-hidden="true" size={18} />
                )
              }
              label={busy ? "Stop current run" : `Run with ${engineId}`}
              onClick={busy ? onStop : undefined}
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
