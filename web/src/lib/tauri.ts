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

export type ApiConnectionStatus = {
  reachable: boolean;
  url: string;
  error?: string | null;
};

export type StreamMetaEvent = {
  conversation_id: string;
  ollama_endpoint_id: string;
  user_message: MessageRecord;
  model: string;
};

export type StreamTokenEvent = {
  token: string;
  done: boolean;
};

export type StreamStatsEvent = {
  prompt_eval_count?: number | null;
  eval_count?: number | null;
  total_duration?: number | null;
};

export type StreamDoneEvent = {
  assistant_message: MessageRecord;
};

export type StreamCallbacks = {
  onMeta: (meta: StreamMetaEvent) => void;
  onToken: (token: StreamTokenEvent) => void;
  onStats?: (stats: StreamStatsEvent) => void;
  onDone: (done: StreamDoneEvent) => void;
  onError: (error: string) => void;
};

export function streamConversationReply(
  apiBaseUrl: string,
  apiToken: string | null,
  conversationId: string,
  body: {
    model?: string;
    message: string;
    system?: string | null;
    context_injection?: string | null;
    personalization?: Record<string, unknown> | null;
    history_limit?: number;
    ollama_endpoint_id?: string | null;
  },
  callbacks: StreamCallbacks
): { abort: () => void } {
  const url = `${apiBaseUrl.replace(/\/+$/, "")}/conversations/${conversationId}/messages/stream`;

  const controller = new AbortController();

  (async () => {
    try {
      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (apiToken) {
        headers["Authorization"] = `Bearer ${apiToken}`;
      }

      const resp = await fetch(url, {
        method: "POST",
        headers,
        body: JSON.stringify(body),
        signal: controller.signal,
      });

      if (!resp.ok) {
        const text = await resp.text();
        callbacks.onError(`HTTP ${resp.status}: ${text}`);
        return;
      }

      const reader = resp.body?.getReader();
      if (!reader) {
        callbacks.onError("No response body");
        return;
      }

      const decoder = new TextDecoder();
      let buffer = "";
      let currentEvent = "";
      let sawDone = false;
      let emittedError = false;

      const emitErrorOnce = (message: string) => {
        if (emittedError) {
          return;
        }
        emittedError = true;
        callbacks.onError(message);
      };

      const processBufferedLines = (chunk: string) => {
        buffer += chunk;
        const lines = buffer.split("\n");
        buffer = lines.pop() ?? "";

        for (const line of lines) {
          const normalized = line.trimEnd();
          if (normalized.startsWith("event:")) {
            currentEvent = normalized.slice(6).trim();
          } else if (normalized.startsWith("data:")) {
            const eventName = currentEvent;
            currentEvent = "";
            if (!eventName) {
              continue;
            }
            const raw = normalized.slice(5).trim();
            if (!raw) continue;
            try {
              const data = JSON.parse(raw);
              switch (eventName) {
                case "meta":
                  callbacks.onMeta(data as StreamMetaEvent);
                  break;
                case "token":
                  callbacks.onToken(data as StreamTokenEvent);
                  break;
                case "stats":
                  callbacks.onStats?.(data as StreamStatsEvent);
                  break;
                case "done":
                  sawDone = true;
                  callbacks.onDone(data as StreamDoneEvent);
                  break;
                case "error":
                  emitErrorOnce(data.error ?? "Unknown stream error");
                  break;
              }
            } catch {
              // skip unparseable lines
            }
          }
        }
      };

      while (true) {
        const { done, value } = await reader.read();
        if (value) {
          processBufferedLines(decoder.decode(value, { stream: true }));
        }
        if (done) {
          break;
        }
      }

      // Flush decoder tail and parse any trailing lines that arrived with stream close.
      processBufferedLines(decoder.decode());
      if (buffer.trim()) {
        // Handle single-line trailing frame without terminal newline.
        processBufferedLines("\n");
      }

      if (!sawDone && !controller.signal.aborted && !emittedError) {
        emitErrorOnce("Stream ended before done event.");
      }
    } catch (err) {
      if ((err as Error).name !== "AbortError") {
        callbacks.onError(err instanceof Error ? err.message : String(err));
      }
    }
  })();

  return { abort: () => controller.abort() };
}

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
