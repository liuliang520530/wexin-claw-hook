import { invoke } from "@tauri-apps/api/core";

export type LoginStatus =
  | { state: "wait" }
  | { state: "scanned" }
  | { state: "confirmed" }
  | { state: "expired" }
  | { state: "error"; message: string };

export interface LoginView {
  svg: string;
  status: LoginStatus;
}

export interface StatusInfo {
  logged_in: boolean;
  bot_id: string | null;
  user_id: string | null;
  server_running: boolean;
  port: number;
  version: string;
  token_expired: boolean;
}

export interface Recipient {
  id: string;
  name: string;
}

export interface Config {
  port: number;
  api_key: string;
  default_recipient: string | null;
  recipients: Recipient[];
}

export interface LogEntry {
  ts: string;
  ok: boolean;
  code: string | null;
  to: string;
  text: string;
}

export const api = {
  getStatus: () => invoke<StatusInfo>("get_status"),
  loginStart: () => invoke<LoginView>("login_start"),
  loginStatus: () => invoke<LoginView | null>("login_status"),
  logout: () => invoke<void>("logout"),
  getConfig: () => invoke<Config>("get_config"),
  saveRecipients: (recipients: Recipient[], defaultRecipient: string | null) =>
    invoke<Config>("save_recipients", { recipients, defaultRecipient }),
  setPort: (port: number) => invoke<StatusInfo>("set_port", { port }),
  resetApiKey: () => invoke<string>("reset_api_key"),
  serverStart: () => invoke<StatusInfo>("server_start"),
  serverStop: () => invoke<StatusInfo>("server_stop"),
  listLogs: () => invoke<LogEntry[]>("list_logs"),
};
