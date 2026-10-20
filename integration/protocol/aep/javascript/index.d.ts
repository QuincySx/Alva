// INPUT: Author-provided tool handlers, lifecycle hooks and Node stdio.
// OUTPUT: TypeScript declarations for the zero-dependency AEP JavaScript SDK.
// POS: Plugin-side type assistance; grants, runtime launch and storage remain host decisions.
import type { Readable, Writable } from 'node:stream';
export const protocolVersion: '0.1.0';
export const frameLimit: number;
export interface Message { id: string; role: string; content: { type: string; [key: string]: unknown }[]; timestamp: number }
export interface ToolResult { content: { type: string; [key: string]: unknown }[]; is_error: boolean }
export type Action = { action: string; reason?: string; modified_arguments?: unknown; result?: unknown; messages?: unknown[]; response?: unknown };
export type Event = 'before_tool_call' | 'after_tool_call' | 'on_llm_call_start' | 'on_llm_call_end' | 'on_user_message' | 'on_agent_start' | 'on_agent_end';
export interface Host {
  readonly signal: AbortSignal;
  log(message: string, level?: string, fields?: Record<string, unknown>): Promise<unknown>;
  notify(message: string, level?: string): Promise<unknown>;
  emitMetric(name: string, value: number, labels?: Record<string, string>): Promise<unknown>;
  getMessages(limit?: number, offset?: number): Promise<Message[]>;
  getMetadata(): Promise<Record<string, unknown>>;
  countTokens(): Promise<number>;
  readReference(uri: string, offset?: number, limit?: number): Promise<unknown>;
  searchReference(uri: string, query: string, limit?: number): Promise<unknown>;
}
export interface Plugin {
  name: string; version: string; description?: string; capabilities?: string[];
  handlers?: Partial<Record<Event, (params: Record<string, any>, host: Host) => Action | void | Promise<Action | void>>>;
  tools?: Record<string, { description: string; inputSchema: Record<string, unknown>; execute(args: Record<string, any>, host: Host): unknown | Promise<unknown> }>;
  onInit?(host: Host): void | Promise<void>;
  onShutdown?(host: Host): void | Promise<void>;
}
export function definePlugin(plugin: Plugin): Readonly<Plugin>;
export function run(plugin: Plugin, options?: { input?: Readable; output?: Writable; signal?: AbortSignal; callbackTimeout?: number }): Promise<void>;
export function message(role: string, text: string): Message;
export function textResult(text: string, isError?: boolean): ToolResult;
export const actions: {
  continue(): Action; block(reason: string): Action; modifyArgs(args: unknown): Action;
  replaceResult(result: unknown): Action; modifyResult(result: unknown): Action;
  modifyMessages(messages: unknown[]): Action; modifyResponse(response: unknown): Action;
};
