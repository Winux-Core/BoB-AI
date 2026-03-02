export type ConversationRecord = {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
};

export type MessageRecord = {
  id: string;
  conversation_id: string;
  role: "system" | "user" | "assistant" | "tool" | string;
  content: string;
  created_at: string;
};

export type LocalProfile = {
  settings: {
    api_base_url: string;
    api_token: string;
    default_model: string;
    system_prompt: string;
  };
  context_injection: string;
  personalization: Record<string, unknown>;
};

export type CachedConversation = {
  conversation_id: string;
  title?: string | null;
  updated_at_unix_ms: number;
  messages: MessageRecord[];
};

export type CachedChatSummary = {
  conversation_id: string;
  title?: string | null;
  updated_at_unix_ms: number;
  message_count: number;
};

export type ConversationReplyResponse = {
  conversation_id: string;
  user_message: MessageRecord;
  assistant_message: MessageRecord;
  model_response: {
    model: string;
    response: string;
    done: boolean;
    prompt_eval_count?: number | null;
    eval_count?: number | null;
    total_duration?: number | null;
  };
};

export const isTauri = (): boolean => {
  if (typeof window === "undefined") {
    return false;
  }
  return "__TAURI_INTERNALS__" in window || "__TAURI_IPC__" in window;
};

export const invokeDesktop = async <T>(
  command: string,
  payload?: Record<string, unknown>
): Promise<T> => {
  if (!isTauri()) {
    throw new Error("This action is available only in the Tauri desktop app.");
  }
  const mod = await import("@tauri-apps/api/core");
  return mod.invoke<T>(command, payload);
};
