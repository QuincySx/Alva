// INPUT: Untrusted assistant prose; trusted module renderers
// OUTPUT: renderMarkdown — inert HTML, safe links, block/inline Markdown and display fences
// POS: Text presentation only; no raw HTML, remote images, scripts or embeds.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { openExternal } from "../bridge";
import { useI18n } from "../i18n";
import { CopyTextButton } from "./copy-text-button";
import { MessageModules } from "./message-modules";

function safeLink(value: string): boolean {
  try { const url = new URL(value); return ["https:", "http:", "mailto:"].includes(url.protocol) && !url.username && !url.password; }
  catch { return false; }
}

function inline(raw: string, depth = 0): ReactNode {
  if (depth > 3) return raw;
  const parts: ReactNode[] = [];
  const pattern = /(`[^`]+`)|(\*\*[^*]+\*\*)|(~~[^~]+~~)|(\*[^*\n]+\*)|(\[[^\]\n]+\]\([^\s)]+\))/g;
  let cursor = 0;
  for (const match of raw.matchAll(pattern)) {
    const start = match.index ?? 0;
    if (start > cursor) parts.push(raw.slice(cursor, start));
    const key = `inline-${start}`;
    if (match[1]) parts.push(<code className="chat-inline-code" key={key}>{match[1].slice(1, -1)}</code>);
    else if (match[2]) parts.push(<strong key={key}>{inline(match[2].slice(2, -2), depth + 1)}</strong>);
    else if (match[3]) parts.push(<del key={key}>{inline(match[3].slice(2, -2), depth + 1)}</del>);
    else if (match[4]) parts.push(<em key={key}>{match[4].slice(1, -1)}</em>);
    else {
      const link = /^\[([^\]]+)\]\((.+)\)$/.exec(match[5])!;
      parts.push(safeLink(link[2]) ? <MarkdownLink url={link[2]} label={link[1]} key={key} /> : match[5]);
    }
    cursor = start + match[0].length;
  }
  if (cursor < raw.length) parts.push(raw.slice(cursor));
  return <span>{parts}</span>;
}

function MarkdownLink({ url, label }: { url: string; label: string }) {
  const { t } = useI18n();
  const [failed, setFailed] = useState(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  return <><a href={url} target="_blank" rel="noopener noreferrer" onClick={(event) => {
    event.preventDefault(); setFailed(false);
    void openExternal(url).catch(() => { if (mounted.current) setFailed(true); });
  }}>{label}</a>{failed ? <small role="status">{t("chat.linkFailed")}</small> : null}</>;
}

const tableCells = (line: string) => line.trim().replace(/^\||\|$/g, "").split("|").map((part) => part.trim());
const tableSeparator = (line: string) => /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$/.test(line);
const startsBlock = (line: string) => /^(?:\s*```|#{1,6}\s|\s*>|\s*[-*+]\s|\s*\d+[.)]\s|\s*(?:---+|\*\*\*+)\s*$)/.test(line);

export function renderMarkdown(text: string): ReactNode[] {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    const key = `block-${index}`;
    if (/^\s*```/.test(line)) {
      const language = line.trimStart().slice(3).trim();
      const body: string[] = [];
      index += 1;
      while (index < lines.length && !/^\s*```\s*$/.test(lines[index])) body.push(lines[index++]);
      if (index < lines.length) index += 1;
      blocks.push(<CodeBlock code={body.join("\n")} language={language || null} key={key} />);
      continue;
    }
    if (index + 1 < lines.length && line.includes("|") && tableSeparator(lines[index + 1])) {
      const headers = tableCells(line);
      const rows: string[][] = [];
      index += 2;
      while (index < lines.length && lines[index].includes("|") && lines[index].trim() !== "") rows.push(tableCells(lines[index++]));
      blocks.push(<div className="chat-table-wrap" key={key}><table className="chat-table"><thead><tr>
        {headers.map((header, i) => <th key={i} scope="col">{inline(header)}</th>)}
      </tr></thead><tbody>{rows.map((row, i) => <tr key={i}>{headers.map((_, j) => <td key={j}>{inline(row[j] ?? "")}</td>)}</tr>)}</tbody></table></div>);
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push(<h3 className="chat-heading" key={key}>{inline(heading[2])}</h3>);
      index += 1; continue;
    }
    if (/^\s*>/.test(line)) {
      const quote: string[] = [];
      while (index < lines.length && /^\s*>/.test(lines[index])) quote.push(lines[index++].replace(/^\s*>\s?/, ""));
      // Quote text is inert inline content, not recursive block parsing.
      blocks.push(<blockquote key={key}>{inline(quote.join("\n"))}</blockquote>);
      continue;
    }
    if (/^\s*(?:---+|\*\*\*+)\s*$/.test(line)) { blocks.push(<hr key={key} />); index += 1; continue; }
    const unordered = /^\s*[-*+]\s+/.test(line);
    const ordered = /^\s*\d+[.)]\s+/.test(line);
    if (unordered || ordered) {
      const pattern = unordered ? /^\s*[-*+]\s+/ : /^\s*\d+[.)]\s+/;
      const items: string[] = [];
      const start = ordered ? Number.parseInt(line.trim(), 10) : 1;
      while (index < lines.length && pattern.test(lines[index])) items.push(lines[index++].replace(pattern, ""));
      const content = items.map((item, i) => {
        const task = /^\[([ xX])\]\s+(.*)$/.exec(item);
        return <li key={i}>{task ? <><input type="checkbox" checked={task[1] !== " "} disabled aria-label={task[2]} />{inline(task[2])}</> : inline(item)}</li>;
      });
      blocks.push(unordered ? <ul className="chat-list" key={key}>{content}</ul> : <ol start={start} className="chat-list chat-list--ordered" key={key}>{content}</ol>);
      continue;
    }
    if (!line.trim()) { index += 1; continue; }
    const paragraph: string[] = [lines[index++]];
    while (index < lines.length && lines[index].trim() !== "" && !startsBlock(lines[index])
      && !(index + 1 < lines.length && lines[index].includes("|") && tableSeparator(lines[index + 1]))) paragraph.push(lines[index++]);
    blocks.push(<p className="chat-paragraph" key={key}>{inline(paragraph.join("\n"))}</p>);
  }
  return blocks;
}

function CodeBlock({ code, language }: { code: string; language: string | null }) {
  const { t } = useI18n();
  return <div className="chat-code-block">
    {language === "alva-display" ? <MessageModules source={code} /> : null}
    <div className="chat-code-block__header"><span className="chat-code-block__language">{language ?? "text"}</span>
      <CopyTextButton text={code} label={t("chat.copyCode")} />
    </div><pre className="chat-code-block__body" tabIndex={0}><code>{code}</code></pre>
  </div>;
}
