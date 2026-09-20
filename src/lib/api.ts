import { invoke } from "@tauri-apps/api/core";

export type LoginStatus =
  | { state: "wait" }
  | { state: "scanned" }
  | { state: "confirmed"; user_id: string; refreshed: boolean }
  | { state: "expired" }
  | { state: "error"; message: string };

export interface LoginView {
  svg: string;
  status: LoginStatus;
}

export interface StatusInfo {
  logged_in: boolean;
  account_count: number;
  expired_count: number;
  default_user_id: string | null;
  server_running: boolean;
  port: number;
  version: string;
}

export interface AccountView {
  name: string;
  user_id: string;
  bot_id: string;
  token_expired: boolean;
  is_default: boolean;
}

export interface Config {
  port: number;
  api_key: string;
  default_account: string | null;
}

export interface LogEntry {
  ts: string;
  ok: boolean;
  code: string | null;
  from: string;
  to: string;
  text: string;
}

export const api = {
  getStatus: () => invoke<StatusInfo>("get_status"),
  loginStart: () => invoke<LoginView>("login_start"),
  loginStatus: () => invoke<LoginView | null>("login_status"),
  listAccounts: () => invoke<AccountView[]>("list_accounts"),
  renameAccount: (userId: string, name: string) =>
    invoke<AccountView[]>("rename_account", { userId, name }),
  removeAccount: (userId: string) => invoke<AccountView[]>("remove_account", { userId }),
  setDefaultAccount: (userId: string) => invoke<AccountView[]>("set_default_account", { userId }),
  getConfig: () => invoke<Config>("get_config"),
  setPort: (port: number) => invoke<StatusInfo>("set_port", { port }),
  resetApiKey: () => invoke<string>("reset_api_key"),
  serverStart: () => invoke<StatusInfo>("server_start"),
  serverStop: () => invoke<StatusInfo>("server_stop"),
  listLogs: () => invoke<LogEntry[]>("list_logs"),
  clearLogs: () => invoke<void>("clear_logs"),
  sendTest: (userId: string, text: string) => invoke<string>("send_test", { userId, text }),
};
