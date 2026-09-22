export type Channel = "weixin" | "wecom";

/** 账号页与发消息页共享的通道 Tab 选中状态；切页后保持。 */
export const channel = $state<{ value: Channel }>({ value: "weixin" });
