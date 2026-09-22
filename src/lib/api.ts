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
  wecom_app_count: number;
  wecom_invalid_count: number;
  default_wecom_app: string | null;
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
  channel: string;
  from: string;
  to: string;
  text: string;
}

export interface WecomAppView {
  name: string;
  corpid: string;
  agentid: number;
  invalid: string | null;
  is_default: boolean;
}

export interface WecomAddResult {
  apps: WecomAppView[];
  notice: string | null;
}

export interface WecomSendOk {
  app: string;
  to: string;
  msgid: string;
  invalid_users: string[];
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
  wecomListApps: () => invoke<WecomAppView[]>("wecom_list_apps"),
  wecomAddApp: (p: { name?: string; corpid: string; agentid: number; secret: string }) =>
    invoke<WecomAddResult>("wecom_add_app", p),
  wecomUpdateApp: (p: { name: string; newName?: string; corpid?: string; agentid?: number; secret?: string }) =>
    invoke<WecomAddResult>("wecom_update_app", p),
  wecomRemoveApp: (name: string) => invoke<WecomAppView[]>("wecom_remove_app", { name }),
  wecomSetDefault: (name: string) => invoke<WecomAppView[]>("wecom_set_default", { name }),
  wecomSendTest: (name: string, to: string, text: string) =>
    invoke<WecomSendOk>("wecom_send_test", { name, to, text }),
};
