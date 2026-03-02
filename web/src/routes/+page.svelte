<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    invokeDesktop,
    isTauri,
    type CachedChatSummary,
    type ConversationRecord,
    type LocalProfile,
    type MessageRecord,
    type ConversationReplyResponse
  } from "$lib/tauri";

  type ConversationSource = "api" | "cache";
  type SourceFilter = "all" | ConversationSource;

  type SidebarItem = {
    id: string;
    title: string;
    updatedMs: number;
    subtitle: string;
    hasApi: boolean;
    hasCache: boolean;
    source: ConversationSource;
  };

  type DebugLogLevel = "info" | "warn" | "error";

  type DebugLogEntry = {
    ts: string;
    level: DebugLogLevel;
    message: string;
    details?: string;
  };

  const defaultProfile: LocalProfile = {
    settings: {
      api_base_url: "http://127.0.0.1:8787",
      api_token: "",
      default_model: "llama3.1",
      system_prompt: "You are BoB, concise and practical."
    },
    context_injection: "",
    personalization: {}
  };

  let profile: LocalProfile = structuredClone(defaultProfile);
  let personalizationText = JSON.stringify(profile.personalization, null, 2);

  let conversations: ConversationRecord[] = [];
  let cachedChats: CachedChatSummary[] = [];

  let selectedConversationId = "";
  let selectedConversationTitle = "";
  let selectedConversationSource: ConversationSource | "none" = "none";
  let messages: MessageRecord[] = [];

  let draftTitle = "";
  let draftMessage = "";
  let status = "Booting BoB desktop...";
  let busy = false;

  let sidebarQuery = "";
  let sourceFilter: SourceFilter = "all";
  let sidebarOpen = true;
  let settingsModalOpen = false;
  let newChatModalOpen = false;
  let logsModalOpen = false;
  let debugLogs: DebugLogEntry[] = [];

  let threadEl: HTMLDivElement | null = null;

  $: hasToken = profile.settings.api_token.trim().length > 0;
  $: sidebarItems = buildSidebarItems(conversations, cachedChats);
  $: filteredSidebarItems = sidebarItems.filter((item) => {
    if (sourceFilter === "api" && !item.hasApi) {
      return false;
    }
    if (sourceFilter === "cache" && !item.hasCache) {
      return false;
    }
    const query = sidebarQuery.trim().toLowerCase();
    if (!query) {
      return true;
    }
    return item.title.toLowerCase().includes(query) || item.id.toLowerCase().includes(query);
  });

  const tokenOrNull = (): string | null => {
    const value = profile.settings.api_token.trim();
    return value.length > 0 ? value : null;
  };

  const logEvent = (level: DebugLogLevel, message: string, details?: unknown) => {
    const detailText =
      details === undefined
        ? undefined
        : typeof details === "string"
          ? details
          : JSON.stringify(details, null, 2);
    const entry: DebugLogEntry = {
      ts: new Date().toISOString(),
      level,
      message,
      details: detailText
    };
    debugLogs = [entry, ...debugLogs].slice(0, 400);
  };

  const clearLogs = () => {
    debugLogs = [];
    logEvent("info", "Debug logs cleared.");
  };

  const copyLogs = async () => {
    const payload = debugLogs
      .map((entry) => {
        const header = `[${entry.ts}] [${entry.level.toUpperCase()}] ${entry.message}`;
        return entry.details ? `${header}\n${entry.details}` : header;
      })
      .join("\n\n");
    if (!payload) {
      status = "No logs to copy.";
      return;
    }
    await navigator.clipboard.writeText(payload);
    status = "Copied debug logs to clipboard.";
  };

  const parsePersonalization = (): Record<string, unknown> => {
    const raw = personalizationText.trim();
    if (!raw) {
      return {};
    }

    const parsed = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      throw new Error("Personalization must be a JSON object.");
    }
    return parsed as Record<string, unknown>;
  };

  const loadProfile = async () => {
    profile = await invokeDesktop<LocalProfile>("load_local_profile_cmd");
    personalizationText = JSON.stringify(profile.personalization, null, 2);
    logEvent("info", "Loaded local profile.", {
      apiBaseUrl: profile.settings.api_base_url,
      defaultModel: profile.settings.default_model,
      hasToken: profile.settings.api_token.trim().length > 0
    });
  };

  const saveProfile = async () => {
    profile.personalization = parsePersonalization();
    profile = await invokeDesktop<LocalProfile>("save_local_profile_cmd", { profile });
    status = "Saved profile to ~/.config/BoB.";
    logEvent("info", "Saved local profile settings.");
  };

  const refreshConversations = async () => {
    logEvent("info", "Fetching API conversations.", {
      apiBaseUrl: profile.settings.api_base_url
    });
    conversations = await invokeDesktop<ConversationRecord[]>("api_list_conversations_cmd", {
      apiBaseUrl: profile.settings.api_base_url,
      apiToken: tokenOrNull(),
      limit: 100
    });
    logEvent("info", "Fetched API conversations.", { count: conversations.length });
  };

  const refreshCachedChats = async () => {
    cachedChats = await invokeDesktop<CachedChatSummary[]>("list_cached_chats_cmd");
    logEvent("info", "Loaded local cached chats.", { count: cachedChats.length });
  };

  const refreshAll = async () => {
    await refreshConversations();
    await refreshCachedChats();
    status = `Loaded ${conversations.length} API chats and ${cachedChats.length} cached chats.`;
  };

  const syncCache = async () => {
    if (!selectedConversationId) {
      return;
    }

    logEvent("info", "Syncing selected conversation to local cache.", {
      conversationId: selectedConversationId
    });
    await invokeDesktop("sync_chat_cache_cmd", {
      apiBaseUrl: profile.settings.api_base_url,
      apiToken: tokenOrNull(),
      conversationId: selectedConversationId,
      title: selectedConversationTitle || null,
      limit: 500
    });
    await refreshCachedChats();
    logEvent("info", "Local cache sync complete.", { conversationId: selectedConversationId });
  };

  const createConversation = async (title?: string): Promise<ConversationRecord> => {
    logEvent("info", "Creating conversation.", {
      title: title?.trim() || null
    });
    const created = await invokeDesktop<ConversationRecord>("api_start_conversation_cmd", {
      apiBaseUrl: profile.settings.api_base_url,
      apiToken: tokenOrNull(),
      title: title?.trim() ? title.trim() : null
    });

    selectedConversationId = created.id;
    selectedConversationTitle = created.title;
    selectedConversationSource = "api";
    messages = [];
    await refreshAll();
    await syncCache();
    logEvent("info", "Conversation created.", {
      conversationId: created.id,
      title: created.title
    });
    return created;
  };

  const startConversation = async () => {
    const created = await createConversation(draftTitle);
    draftTitle = "";
    newChatModalOpen = false;
    status = `Started conversation: ${created.title}`;
  };

  const openConversation = async (conversationId: string, titleHint?: string) => {
    logEvent("info", "Opening API conversation.", { conversationId });
    const loaded = await invokeDesktop<MessageRecord[]>("api_get_messages_cmd", {
      apiBaseUrl: profile.settings.api_base_url,
      apiToken: tokenOrNull(),
      conversationId,
      limit: 500
    });

    selectedConversationId = conversationId;
    selectedConversationTitle = titleHint ?? `Conversation ${conversationId.slice(0, 8)}`;
    selectedConversationSource = "api";
    messages = loaded;
    await syncCache();
    await scrollThreadToBottom();
    status = `Loaded ${loaded.length} message(s) from API.`;
    logEvent("info", "API conversation loaded.", {
      conversationId,
      messageCount: loaded.length
    });
  };

  const openCachedConversation = async (conversationId: string) => {
    logEvent("info", "Opening cached conversation.", { conversationId });
    const cached = await invokeDesktop<
      | {
          conversation_id: string;
          title?: string | null;
          messages: MessageRecord[];
        }
      | null
    >("load_cached_chat_cmd", {
      conversationId
    });

    if (!cached) {
      throw new Error("Cached chat not found.");
    }

    selectedConversationId = cached.conversation_id;
    selectedConversationTitle = cached.title ?? "Cached Conversation";
    selectedConversationSource = "cache";
    messages = cached.messages;
    await scrollThreadToBottom();
    status = `Loaded ${cached.messages.length} cached message(s).`;
    logEvent("info", "Cached conversation loaded.", {
      conversationId: cached.conversation_id,
      messageCount: cached.messages.length
    });
  };

  const openSidebarItem = async (item: SidebarItem) => {
    if (item.hasApi) {
      await openConversation(item.id, item.title);
    } else {
      await openCachedConversation(item.id);
    }

    if (window.matchMedia("(max-width: 1080px)").matches) {
      sidebarOpen = false;
    }
  };

  const ensureConversationForSend = async (): Promise<void> => {
    if (selectedConversationId) {
      return;
    }

    const inferredTitle = draftMessage.trim().slice(0, 44) || "New Conversation";
    await createConversation(inferredTitle);
  };

  const sendMessage = async () => {
    if (!draftMessage.trim()) {
      throw new Error("Message cannot be empty.");
    }

    await ensureConversationForSend();

    const outboundMessage = draftMessage;
    draftMessage = "";
    const outgoingMeta = {
      conversationId: selectedConversationId,
      model: profile.settings.default_model,
      chars: outboundMessage.length,
      hasSystem: Boolean(profile.settings.system_prompt.trim()),
      hasContextInjection: Boolean(profile.context_injection.trim())
    };
    logEvent("info", "Sending message request.", outgoingMeta);

    const response = await invokeDesktop<ConversationReplyResponse>("api_send_message_cmd", {
      apiBaseUrl: profile.settings.api_base_url,
      apiToken: tokenOrNull(),
      conversationId: selectedConversationId,
      model: profile.settings.default_model,
      message: outboundMessage,
      system: profile.settings.system_prompt || null,
      contextInjection: profile.context_injection || null,
      personalization: parsePersonalization(),
      historyLimit: 50
    });

    messages = [...messages, response.user_message, response.assistant_message];
    selectedConversationSource = "api";

    await refreshConversations();
    await syncCache();
    await scrollThreadToBottom();
    status = `Reply received from ${response.model_response.model}.`;
    logEvent("info", "Received assistant response.", {
      conversationId: response.conversation_id,
      model: response.model_response.model,
      responseChars: response.model_response.response.length,
      done: response.model_response.done
    });
  };

  const handleComposerKeydown = async (event: KeyboardEvent) => {
    if (event.key !== "Enter" || event.shiftKey) {
      return;
    }
    event.preventDefault();
    if (busy) {
      return;
    }
    await run(sendMessage);
  };

  const handleGlobalKeydown = (event: KeyboardEvent) => {
    if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === "l") {
      event.preventDefault();
      logsModalOpen = !logsModalOpen;
      logEvent("info", `Debug log modal ${logsModalOpen ? "opened" : "closed"} via shortcut.`);
      return;
    }
    if (event.key === "Escape") {
      settingsModalOpen = false;
      newChatModalOpen = false;
      logsModalOpen = false;
    }
  };

  const saveSettingsFromModal = async () => {
    await saveProfile();
    settingsModalOpen = false;
  };

  const handleBackdropClick = (event: MouseEvent, modal: "settings" | "newChat" | "logs") => {
    if (event.target !== event.currentTarget) {
      return;
    }
    if (modal === "settings") {
      settingsModalOpen = false;
      return;
    }
    if (modal === "newChat") {
      newChatModalOpen = false;
      return;
    }
    logsModalOpen = false;
  };

  const clearComposer = () => {
    draftMessage = "";
  };

  const scrollThreadToBottom = async () => {
    await tick();
    if (!threadEl) {
      return;
    }
    threadEl.scrollTop = threadEl.scrollHeight;
  };

  const prettyDate = (unixMs: number) => {
    if (!unixMs) {
      return "Unknown time";
    }
    return new Date(unixMs).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit"
    });
  };

  const run = async (work: () => Promise<void>) => {
    busy = true;
    try {
      await work();
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      status = message;
      logEvent("error", "Operation failed.", {
        message,
        stack: err instanceof Error ? err.stack : undefined
      });
    } finally {
      busy = false;
    }
  };

  onMount(async () => {
    if (!isTauri()) {
      status = "Run this UI inside Tauri to access local cache/profile and BoB API commands.";
      logEvent("warn", "UI not running in Tauri context.");
      return;
    }

    await run(async () => {
      logEvent("info", "Initializing BoB workspace UI.");
      await loadProfile();
      await refreshAll();
      status = "Ready.";
      logEvent("info", "Workspace UI ready.");
    });
  });

  function buildSidebarItems(
    apiConversations: ConversationRecord[],
    localCache: CachedChatSummary[]
  ): SidebarItem[] {
    const map = new Map<string, SidebarItem>();

    for (const conversation of apiConversations) {
      const updatedMs = toEpochMs(conversation.updated_at);
      map.set(conversation.id, {
        id: conversation.id,
        title: normalizeTitle(conversation.title, conversation.id),
        updatedMs,
        subtitle: `API · ${prettyDate(updatedMs)}`,
        hasApi: true,
        hasCache: false,
        source: "api"
      });
    }

    for (const cached of localCache) {
      const updatedMs = cached.updated_at_unix_ms;
      const existing = map.get(cached.conversation_id);

      if (existing) {
        existing.hasCache = true;
        if (updatedMs > existing.updatedMs) {
          existing.updatedMs = updatedMs;
          existing.subtitle = `API + Cache · ${prettyDate(updatedMs)}`;
        }
        continue;
      }

      map.set(cached.conversation_id, {
        id: cached.conversation_id,
        title: normalizeTitle(cached.title ?? "Cached Conversation", cached.conversation_id),
        updatedMs,
        subtitle: `Cache · ${cached.message_count} messages`,
        hasApi: false,
        hasCache: true,
        source: "cache"
      });
    }

    return [...map.values()].sort((a, b) => b.updatedMs - a.updatedMs);
  }

  function normalizeTitle(raw: string, fallbackId: string): string {
    const trimmed = raw.trim();
    if (trimmed) {
      return trimmed;
    }
    return `Conversation ${fallbackId.slice(0, 8)}`;
  }

  function toEpochMs(raw: string): number {
    const parsed = Date.parse(raw);
    return Number.isNaN(parsed) ? 0 : parsed;
  }
</script>

<svelte:window on:keydown={handleGlobalKeydown} />

<main class="app-shell">
  <aside class="history-rail" class:open={sidebarOpen}>
    <header class="history-top">
      <div class="brand-block">
        <p class="eyebrow">BoB</p>
        <h1>Conversations</h1>
      </div>
      <button class="ghost" on:click={() => (sidebarOpen = !sidebarOpen)}>
        {sidebarOpen ? "Hide" : "Show"}
      </button>
    </header>

    <div class="rail-actions">
      <button class="primary" disabled={busy} on:click={() => (newChatModalOpen = true)}>New Chat</button>
      <input class="search" placeholder="Search chats" bind:value={sidebarQuery} />
      <div class="filters">
        <button class:active={sourceFilter === "all"} on:click={() => (sourceFilter = "all")}>All</button>
        <button class:active={sourceFilter === "api"} on:click={() => (sourceFilter = "api")}>API</button>
        <button class:active={sourceFilter === "cache"} on:click={() => (sourceFilter = "cache")}>Cache</button>
      </div>
      <div class="meta-row">
        <span>{conversations.length} API</span>
        <span>{cachedChats.length} Cache</span>
      </div>
    </div>

    <div class="history-list">
      {#if filteredSidebarItems.length === 0}
        <p class="empty">No chats for this filter.</p>
      {:else}
        {#each filteredSidebarItems as item}
          <button
            class="history-item"
            class:selected={item.id === selectedConversationId}
            on:click={() => run(() => openSidebarItem(item))}
          >
            <div class="history-title-row">
              <strong>{item.title}</strong>
              {#if item.hasApi && item.hasCache}
                <small>SYNC</small>
              {:else if item.hasApi}
                <small>API</small>
              {:else}
                <small>CACHE</small>
              {/if}
            </div>
            <span>{item.subtitle}</span>
          </button>
        {/each}
      {/if}
    </div>
  </aside>

  <section class="chat-workspace">
    <header class="workspace-top">
      <div class="workspace-title">
        <h2>{selectedConversationTitle || "New Conversation"}</h2>
        <p>
          {#if selectedConversationSource === "none"}
            No chat selected
          {:else if selectedConversationSource === "api"}
            Live API conversation
          {:else}
            Local cache preview
          {/if}
        </p>
      </div>

      <div class="workspace-controls">
        <span class="model-chip">{profile.settings.default_model}</span>
        <button class="ghost" disabled={busy} on:click={() => run(refreshAll)}>Refresh</button>
        <button class="ghost" disabled={busy || !selectedConversationId} on:click={() => run(syncCache)}>
          Sync Cache
        </button>
        <button class="ghost" on:click={() => (logsModalOpen = true)}>Logs</button>
        <button class="ghost" on:click={() => (settingsModalOpen = true)}>Settings</button>
      </div>
    </header>

    <div class="thread" bind:this={threadEl}>
      {#if messages.length === 0}
        <div class="welcome-card">
          <h3>Start chatting</h3>
          <p>Type a message below. If no chat is selected, BoB creates one automatically.</p>
          <p>Press <kbd>Enter</kbd> to send and <kbd>Shift + Enter</kbd> for new lines.</p>
        </div>
      {:else}
        {#each messages as message}
          <article class="bubble" class:user={message.role === "user"} class:assistant={message.role === "assistant"}>
            <header>{message.role.toUpperCase()}</header>
            <p>{message.content}</p>
          </article>
        {/each}
      {/if}

      {#if busy}
        <div class="typing">BoB is thinking...</div>
      {/if}
    </div>

    <footer class="composer">
      <textarea
        rows="4"
        bind:value={draftMessage}
        placeholder="Message BoB..."
        on:keydown={handleComposerKeydown}
      ></textarea>
      <div class="composer-actions">
        <span class="status-dot" class:ok={hasToken}>{hasToken ? "API Token Set" : "No API Token"}</span>
        <div class="actions-right">
          <button class="ghost" disabled={busy} on:click={clearComposer}>Clear</button>
          <button class="primary" disabled={busy} on:click={() => run(sendMessage)}>Send</button>
        </div>
      </div>
      <p class="status-line">{status}</p>
      <p class="status-line shortcut-hint">Open logs: Ctrl+Shift+L</p>
    </footer>
  </section>
</main>

{#if settingsModalOpen}
  <div
    class="modal-backdrop"
    role="button"
    tabindex="0"
    aria-label="Close settings modal"
    on:click={(event) => handleBackdropClick(event, "settings")}
    on:keydown={(event) => event.key === "Escape" && (settingsModalOpen = false)}
  >
    <div class="modal-card" role="dialog" aria-modal="true" aria-label="Workspace Settings" tabindex="0">
      <header class="modal-head">
        <h3>Workspace Settings</h3>
        <button class="ghost" on:click={() => (settingsModalOpen = false)}>Close</button>
      </header>

      <div class="modal-body settings-fields">
        <label>
          API URL
          <input bind:value={profile.settings.api_base_url} />
        </label>
        <label>
          API Token
          <input type="password" bind:value={profile.settings.api_token} />
        </label>
        <label>
          Default Model
          <input bind:value={profile.settings.default_model} />
        </label>
        <label>
          System Prompt
          <textarea rows="3" bind:value={profile.settings.system_prompt}></textarea>
        </label>
        <label>
          Context Injection
          <textarea rows="4" bind:value={profile.context_injection}></textarea>
        </label>
        <label>
          Personalization JSON
          <textarea rows="7" bind:value={personalizationText}></textarea>
        </label>
      </div>

      <footer class="modal-actions">
        <button class="ghost" disabled={busy} on:click={() => run(refreshCachedChats)}>Refresh Cache List</button>
        <button class="primary" disabled={busy} on:click={() => run(saveSettingsFromModal)}>Save Settings</button>
      </footer>
    </div>
  </div>
{/if}

{#if logsModalOpen}
  <div
    class="modal-backdrop"
    role="button"
    tabindex="0"
    aria-label="Close debug logs modal"
    on:click={(event) => handleBackdropClick(event, "logs")}
    on:keydown={(event) => event.key === "Escape" && (logsModalOpen = false)}
  >
    <div class="modal-card modal-logs" role="dialog" aria-modal="true" aria-label="Debug Logs" tabindex="0">
      <header class="modal-head">
        <h3>Debug Logs</h3>
        <button class="ghost" on:click={() => (logsModalOpen = false)}>Close</button>
      </header>
      <div class="modal-body logs-body">
        {#if debugLogs.length === 0}
          <p class="empty">No debug logs yet.</p>
        {:else}
          <div class="logs-list">
            {#each debugLogs as entry}
              <article class="log-entry" class:error={entry.level === "error"} class:warn={entry.level === "warn"}>
                <header>
                  <span>[{entry.level.toUpperCase()}]</span>
                  <time datetime={entry.ts}>{entry.ts}</time>
                </header>
                <p>{entry.message}</p>
                {#if entry.details}
                  <pre>{entry.details}</pre>
                {/if}
              </article>
            {/each}
          </div>
        {/if}
      </div>
      <footer class="modal-actions">
        <button class="ghost" disabled={busy || debugLogs.length === 0} on:click={() => run(copyLogs)}>
          Copy Logs
        </button>
        <button class="ghost" disabled={busy || debugLogs.length === 0} on:click={clearLogs}>Clear Logs</button>
      </footer>
    </div>
  </div>
{/if}

{#if newChatModalOpen}
  <div
    class="modal-backdrop"
    role="button"
    tabindex="0"
    aria-label="Close new conversation modal"
    on:click={(event) => handleBackdropClick(event, "newChat")}
    on:keydown={(event) => event.key === "Escape" && (newChatModalOpen = false)}
  >
    <div
      class="modal-card modal-sm"
      role="dialog"
      aria-modal="true"
      aria-label="Create Conversation"
      tabindex="0"
    >
      <header class="modal-head">
        <h3>New Conversation</h3>
        <button class="ghost" on:click={() => (newChatModalOpen = false)}>Close</button>
      </header>
      <div class="modal-body">
        <label>
          Title
          <input bind:value={draftTitle} placeholder="Conversation title (optional)" />
        </label>
      </div>
      <footer class="modal-actions">
        <button class="ghost" on:click={() => (newChatModalOpen = false)}>Cancel</button>
        <button class="primary" disabled={busy} on:click={() => run(startConversation)}>Create</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .app-shell {
    width: min(1500px, calc(100% - 1.4rem));
    margin: 0.7rem auto 1.2rem;
    display: grid;
    grid-template-columns: 320px minmax(0, 1fr);
    gap: 0.8rem;
    min-height: calc(100vh - 1.4rem);
  }

  .history-rail,
  .chat-workspace {
    border: 1px solid rgba(157, 177, 194, 0.18);
    border-radius: 16px;
    background: linear-gradient(160deg, rgba(12, 20, 31, 0.84), rgba(11, 17, 27, 0.91));
    box-shadow: 0 20px 35px rgba(5, 8, 15, 0.45);
    backdrop-filter: blur(8px);
  }

  .history-rail {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    overflow: hidden;
  }

  .history-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.85rem 0.9rem 0.65rem;
    border-bottom: 1px solid rgba(144, 164, 180, 0.14);
  }

  .eyebrow {
    margin: 0;
    font-size: 0.72rem;
    letter-spacing: 0.12em;
    color: #8ab8d8;
  }

  h1,
  h2,
  h3,
  p {
    margin: 0;
  }

  h1 {
    font-size: 1.1rem;
    margin-top: 0.08rem;
  }

  .rail-actions {
    padding: 0.7rem;
    display: grid;
    gap: 0.55rem;
    border-bottom: 1px solid rgba(144, 164, 180, 0.14);
  }

  .search {
    width: 100%;
  }

  .filters {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.35rem;
  }

  .filters button.active {
    border-color: rgba(124, 180, 255, 0.6);
    background: linear-gradient(170deg, rgba(50, 98, 153, 0.27), rgba(49, 76, 116, 0.22));
  }

  .meta-row {
    display: flex;
    justify-content: space-between;
    font-size: 0.72rem;
    color: #a6b7cb;
  }

  .history-list {
    overflow: auto;
    padding: 0.6rem;
    display: grid;
    gap: 0.42rem;
    align-content: start;
  }

  .history-item {
    display: grid;
    gap: 0.2rem;
    text-align: left;
  }

  .history-title-row {
    display: flex;
    justify-content: space-between;
    gap: 0.6rem;
    align-items: center;
  }

  .history-item small {
    font-size: 0.65rem;
    color: #83d6ba;
    letter-spacing: 0.09em;
  }

  .history-item span {
    color: #9fb1c6;
    font-size: 0.74rem;
  }

  .history-item.selected {
    border-color: rgba(124, 180, 255, 0.68);
    background: linear-gradient(170deg, rgba(66, 95, 131, 0.31), rgba(49, 66, 94, 0.26));
  }

  .chat-workspace {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    overflow: hidden;
  }

  .workspace-top {
    display: flex;
    justify-content: space-between;
    gap: 0.8rem;
    align-items: flex-start;
    padding: 0.85rem 1rem;
    border-bottom: 1px solid rgba(144, 164, 180, 0.14);
  }

  .workspace-title h2 {
    font-size: 1.1rem;
  }

  .workspace-title p {
    margin-top: 0.2rem;
    font-size: 0.78rem;
    color: #9cb0c5;
  }

  .workspace-controls {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    flex-wrap: wrap;
  }

  .model-chip {
    font-size: 0.72rem;
    border: 1px solid rgba(125, 189, 165, 0.4);
    color: #9ee0c4;
    background: rgba(25, 69, 56, 0.3);
    padding: 0.26rem 0.54rem;
    border-radius: 999px;
  }

  .thread {
    padding: 1rem;
    overflow: auto;
    display: grid;
    gap: 0.6rem;
    align-content: start;
    background:
      radial-gradient(circle at 30% 0%, rgba(41, 74, 112, 0.14), transparent 35%),
      radial-gradient(circle at 80% 90%, rgba(27, 61, 53, 0.15), transparent 42%);
  }

  .welcome-card {
    width: min(680px, 100%);
    border: 1px solid rgba(155, 172, 190, 0.23);
    background: rgba(19, 28, 40, 0.68);
    border-radius: 16px;
    padding: 1rem;
    display: grid;
    gap: 0.35rem;
    color: #c7d8eb;
  }

  .welcome-card h3 {
    font-size: 1rem;
  }

  kbd {
    border: 1px solid rgba(143, 160, 182, 0.36);
    border-bottom-width: 2px;
    border-radius: 6px;
    padding: 0.06rem 0.35rem;
    font-size: 0.72rem;
    background: rgba(20, 26, 35, 0.8);
  }

  .bubble {
    width: min(780px, 100%);
    border: 1px solid rgba(156, 177, 200, 0.21);
    border-radius: 14px;
    padding: 0.72rem 0.8rem;
    background: rgba(19, 29, 41, 0.76);
    justify-self: start;
  }

  .bubble.user {
    justify-self: end;
    border-color: rgba(138, 179, 229, 0.38);
    background: linear-gradient(160deg, rgba(38, 73, 110, 0.56), rgba(28, 52, 82, 0.52));
  }

  .bubble.assistant {
    border-color: rgba(103, 181, 150, 0.34);
    background: linear-gradient(160deg, rgba(24, 56, 62, 0.56), rgba(21, 46, 54, 0.55));
  }

  .bubble header {
    font-size: 0.69rem;
    letter-spacing: 0.08em;
    color: #8fd7c0;
    margin-bottom: 0.35rem;
  }

  .bubble.user header {
    color: #a9cbf1;
  }

  .bubble p {
    font-size: 0.92rem;
    line-height: 1.42;
    white-space: pre-wrap;
    color: #e8f0fb;
  }

  .typing {
    font-size: 0.78rem;
    color: #9eb7d2;
    padding-left: 0.2rem;
    animation: pulse 1200ms ease-in-out infinite;
  }

  .composer {
    border-top: 1px solid rgba(144, 164, 180, 0.14);
    padding: 0.75rem 0.8rem;
    display: grid;
    gap: 0.5rem;
  }

  .composer textarea {
    width: 100%;
    resize: vertical;
  }

  .composer-actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.8rem;
  }

  .status-dot {
    font-size: 0.72rem;
    padding: 0.2rem 0.5rem;
    border-radius: 999px;
    border: 1px solid rgba(196, 115, 115, 0.4);
    color: #f1a8a8;
    background: rgba(106, 35, 35, 0.2);
  }

  .status-dot.ok {
    border-color: rgba(115, 197, 157, 0.44);
    color: #95e3c0;
    background: rgba(30, 86, 70, 0.25);
  }

  .actions-right {
    display: flex;
    gap: 0.45rem;
  }

  .status-line {
    font-size: 0.78rem;
    color: #a9c0d8;
  }

  .shortcut-hint {
    font-size: 0.72rem;
    opacity: 0.86;
  }

  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(6, 12, 20, 0.72);
    backdrop-filter: blur(6px);
    display: grid;
    place-items: center;
    z-index: 90;
    padding: 1rem;
  }

  .modal-card {
    width: min(760px, calc(100vw - 1.6rem));
    max-height: calc(100vh - 2rem);
    border: 1px solid rgba(157, 177, 194, 0.24);
    border-radius: 16px;
    background: linear-gradient(160deg, rgba(11, 18, 29, 0.95), rgba(10, 16, 26, 0.98));
    box-shadow: 0 26px 46px rgba(3, 8, 15, 0.6);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    overflow: hidden;
  }

  .modal-sm {
    width: min(480px, calc(100vw - 1.6rem));
  }

  .modal-logs {
    width: min(980px, calc(100vw - 1.6rem));
  }

  .modal-head {
    padding: 0.9rem 1rem;
    border-bottom: 1px solid rgba(142, 164, 187, 0.16);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.8rem;
  }

  .modal-body {
    padding: 0.9rem 1rem;
    overflow: auto;
  }

  .settings-fields {
    display: grid;
    gap: 0.56rem;
    align-content: start;
  }

  .settings-fields label,
  .modal-body label {
    display: grid;
    gap: 0.24rem;
    font-size: 0.78rem;
    color: #a2b7cc;
  }

  .modal-actions {
    padding: 0.85rem 1rem;
    border-top: 1px solid rgba(142, 164, 187, 0.16);
    display: flex;
    justify-content: flex-end;
    gap: 0.45rem;
  }

  .logs-body {
    padding-top: 0.7rem;
    padding-bottom: 0.7rem;
  }

  .logs-list {
    display: grid;
    gap: 0.55rem;
  }

  .log-entry {
    border: 1px solid rgba(147, 166, 188, 0.24);
    border-radius: 12px;
    padding: 0.6rem 0.7rem;
    background: rgba(14, 23, 34, 0.74);
    display: grid;
    gap: 0.3rem;
  }

  .log-entry.warn {
    border-color: rgba(201, 167, 102, 0.42);
    background: rgba(58, 48, 25, 0.36);
  }

  .log-entry.error {
    border-color: rgba(202, 112, 112, 0.46);
    background: rgba(66, 28, 34, 0.45);
  }

  .log-entry header {
    display: flex;
    justify-content: space-between;
    gap: 0.6rem;
    font-size: 0.7rem;
    color: #b6c9dd;
  }

  .log-entry p {
    margin: 0;
    font-size: 0.85rem;
    color: #e4eefb;
  }

  .log-entry pre {
    margin: 0;
    font-size: 0.74rem;
    line-height: 1.34;
    max-height: 220px;
    overflow: auto;
    background: rgba(8, 13, 22, 0.84);
    border: 1px solid rgba(147, 166, 188, 0.24);
    border-radius: 10px;
    padding: 0.5rem;
    color: #b6cbdf;
    white-space: pre-wrap;
    word-break: break-word;
  }

  input,
  textarea,
  button {
    font: inherit;
  }

  input,
  textarea {
    color: #e9f2fd;
    border: 1px solid rgba(146, 166, 188, 0.32);
    border-radius: 10px;
    padding: 0.52rem 0.58rem;
    background: rgba(10, 17, 28, 0.72);
  }

  textarea {
    line-height: 1.4;
  }

  button {
    border: 1px solid rgba(153, 171, 191, 0.24);
    border-radius: 10px;
    padding: 0.5rem 0.64rem;
    color: #e6f2ff;
    background: rgba(20, 31, 45, 0.76);
    cursor: pointer;
  }

  button:hover {
    border-color: rgba(154, 191, 235, 0.44);
  }

  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .primary {
    border-color: rgba(119, 183, 255, 0.62);
    background: linear-gradient(170deg, rgba(56, 112, 179, 0.62), rgba(41, 82, 133, 0.58));
  }

  .ghost {
    background: rgba(20, 28, 39, 0.6);
  }

  .empty {
    color: #9cb1c9;
    font-size: 0.8rem;
    padding: 0.5rem;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.5;
    }
    50% {
      opacity: 1;
    }
  }

  @media (max-width: 1080px) {
    .app-shell {
      grid-template-columns: minmax(0, 1fr);
      width: min(100%, calc(100% - 0.8rem));
      margin: 0.4rem auto 0.8rem;
      gap: 0.65rem;
      min-height: calc(100vh - 0.8rem);
    }

    .history-rail {
      max-height: 44vh;
      display: none;
    }

    .history-rail.open {
      display: grid;
    }

    .workspace-top {
      flex-direction: column;
    }

    .thread {
      min-height: 42vh;
    }
  }
</style>
