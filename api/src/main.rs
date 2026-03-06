use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};

use anyhow::Result;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Query, State};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use bob_core::config::BobConfig;
use bob_core::db_manager::{
    ConversationRecord, DbManager, MessageRecord, OllamaEndpointRecord, OllamaEndpointUpsert,
    WorkspaceProfileRecord, WorkspaceProfileUpdate,
};
use bob_core::db_migrations::{MigrationSummary, apply_migrations};
use bob_core::db_policy::load_db_policy_rules;
use bob_core::fs_cache::{FileEntry, FsIndex};
use bob_core::fs_watch::{WatchSummary, watch_and_persist};
use bob_core::ollama::{
    OllamaGenerateResponse, generate as ollama_generate,
    generate_stream as ollama_generate_stream, list_models as ollama_list_models,
};
use bob_core::permissions::{PermissionDecision, PermissionEngine, PermissionRequest};
use bob_core::service_bootstrap::{
    RetryConfig, require_non_empty_connection, wait_for_http_health, wait_for_postgres,
};
use clap::Parser;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tokio_postgres::NoTls;

#[derive(Parser, Debug)]
#[command(name = "bob-api")]
#[command(about = "BoB HTTP API server for remote access")]
struct ApiCli {
    #[arg(long, env = "BOB_API_BIND", default_value = "127.0.0.1:8787")]
    bind: String,
    #[arg(long, env = "BOB_API_TOKEN")]
    token: Option<String>,
    #[arg(long, default_value_t = false)]
    allow_no_auth: bool,
    #[arg(long, env = "BOB_API_ALLOW_INSECURE_CORS", default_value_t = false)]
    allow_insecure_cors: bool,
    #[arg(long, env = "BOB_API_CORS_ORIGIN", default_value = "*")]
    cors_origin: String,
}

#[derive(Clone)]
struct AppState {
    cfg: Arc<BobConfig>,
    permission: Arc<RwLock<PermissionEngine>>,
    api_token: Option<String>,
    db_manager: Arc<DbManager>,
    default_ollama_url: Arc<String>,
}

#[derive(Debug, Serialize)]
struct BuildCacheResponse {
    entries: usize,
    output: String,
}

#[derive(Debug, Serialize)]
struct LookupCacheResponse {
    entry: Option<FileEntry>,
    avg_lookup_ns: u64,
    iterations: u32,
}

#[derive(Debug, Serialize)]
struct SyncPathResponse {
    cache: String,
    path: String,
    entries: usize,
}

#[derive(Debug, Deserialize)]
struct BuildCacheRequest {
    root: String,
    output: Option<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LookupCacheRequest {
    cache: Option<String>,
    path: String,
    #[serde(default = "default_iterations")]
    iterations: u32,
}

#[derive(Debug, Deserialize)]
struct WatchCacheRequest {
    root: String,
    output: Option<String>,
    #[serde(default)]
    exclude: Vec<String>,
    max_events: Option<u64>,
    #[serde(default = "default_idle_timeout_ms")]
    idle_timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
struct SyncPathRequest {
    cache: Option<String>,
    path: String,
}

#[derive(Debug, Deserialize)]
struct CheckPermissionRequest {
    tool: String,
    path: Option<String>,
    command: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DbMigrateRequest {
    dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SyncPolicyDbRequest {
    scope: Option<String>,
    #[serde(default)]
    persist: bool,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    model: Option<String>,
    prompt: String,
    system: Option<String>,
    ollama_endpoint_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StartConversationRequest {
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConversationMessageRequest {
    model: Option<String>,
    message: String,
    system: Option<String>,
    context_injection: Option<String>,
    personalization: Option<serde_json::Value>,
    history_limit: Option<i64>,
    ollama_endpoint_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConversationListQuery {
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ConversationMessagesQuery {
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceProfileRequest {
    default_model: String,
    system_prompt: String,
    context_injection: String,
    personalization: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OllamaEndpointUpsertRequest {
    id: Option<String>,
    name: String,
    base_url: String,
    #[serde(default = "default_endpoint_kind")]
    kind: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    is_default: bool,
    auth_token: Option<String>,
    #[serde(default)]
    clear_auth_token: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaModelsQuery {
    endpoint_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaModelsResponse {
    endpoint_id: String,
    endpoint_name: String,
    model_names: Vec<String>,
}

#[derive(Debug, Serialize)]
struct OllamaEndpointTestResponse {
    endpoint_id: String,
    endpoint_name: String,
    reachable: bool,
    model_count: usize,
    model_names: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ConversationReplyResponse {
    conversation_id: String,
    ollama_endpoint_id: String,
    user_message: MessageRecord,
    assistant_message: MessageRecord,
    model_response: OllamaGenerateResponse,
}

#[derive(Debug, Deserialize, Serialize)]
struct OrchestratorChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    error: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    fn internal(err: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = ApiCli::parse();
    let cfg = BobConfig::from_env();
    let startup_retry = RetryConfig::startup();
    let bind_addr: SocketAddr = cli
        .bind
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid bind address {}: {}", cli.bind, e))?;
    let token = cli.token.and_then(normalize_token).or_else(|| {
        std::env::var("BOB_API_TOKEN")
            .ok()
            .and_then(normalize_token)
    });

    if token.is_none() && !cli.allow_no_auth {
        anyhow::bail!(
            "no API token configured. Set BOB_API_TOKEN or pass --token. \
             If you intentionally want unauthenticated mode, pass --allow-no-auth."
        );
    }
    if let Some(value) = token.as_deref() {
        validate_api_token(value)?;
    }
    if token.is_none() && !bind_addr.ip().is_loopback() {
        anyhow::bail!(
            "refusing insecure unauthenticated bind on {}. \
             Use a loopback bind (127.0.0.1/::1), configure BOB_API_TOKEN, \
             or explicitly opt into risk with a secure network boundary.",
            bind_addr
        );
    }
    if token.is_some() && cli.cors_origin == "*" && !cli.allow_insecure_cors {
        anyhow::bail!(
            "refusing wildcard CORS while auth is enabled. \
             Set BOB_API_CORS_ORIGIN to an explicit origin or pass --allow-insecure-cors."
        );
    }

    let mut permission_engine =
        PermissionEngine::load_or_default(&cfg.permission_policy_path, &cfg.audit_log_path)?;
    if cfg.policy_sync_from_db {
        let (db_rules, summary) = load_db_policy_rules(&cfg.postgres_url, &cfg.policy_scope)?;
        permission_engine.apply_db_rules(db_rules);
        println!(
            "policy synced from DB scope={} tool_rules={} path_rules={} command_rules={}",
            summary.scope, summary.tool_rules, summary.path_rules, summary.command_rules
        );
    }

    let postgres_url = require_non_empty_connection(
        cfg.postgres_url.clone(),
        "PostgreSQL connection URL",
        "BOB_POSTGRES_URL",
    )?;
    let ollama_url =
        require_non_empty_connection(cfg.ollama_url.clone(), "Ollama base URL", "BOB_OLLAMA_URL")?;
    wait_for_postgres("bob-api", &postgres_url, startup_retry)?;
    wait_for_http_health("ollama", &ollama_url, "/api/tags", None, startup_retry)?;

    let postgres_url_for_migrations = postgres_url.clone();
    let migrations_dir = cfg.migrations_dir.clone();
    let migration_summary = tokio::task::spawn_blocking(move || {
        apply_migrations(&postgres_url_for_migrations, &migrations_dir)
    })
    .await??;
    println!(
        "migrations: applied={} skipped={} total={}",
        migration_summary.applied, migration_summary.skipped, migration_summary.total
    );

    let state = AppState {
        cfg: Arc::new(cfg.clone()),
        permission: Arc::new(RwLock::new(permission_engine)),
        api_token: token.clone(),
        db_manager: Arc::new(DbManager::new(create_pg_pool(&postgres_url)?)),
        default_ollama_url: Arc::new(ollama_url),
    };

    let cors = build_cors(&cli.cors_origin)?;
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/config", get(get_config))
        .route(
            "/workspace/profile",
            get(get_workspace_profile).put(save_workspace_profile),
        )
        .route(
            "/workspace/ollama/endpoints",
            get(list_ollama_endpoints).post(upsert_ollama_endpoint),
        )
        .route(
            "/workspace/ollama/endpoints/{id}",
            delete(delete_ollama_endpoint),
        )
        .route(
            "/workspace/ollama/endpoints/{id}/default",
            post(set_default_ollama_endpoint),
        )
        .route(
            "/workspace/ollama/endpoints/{id}/test",
            post(test_ollama_endpoint),
        )
        .route("/workspace/ollama/models", get(list_ollama_models))
        .route("/cache/build", post(build_cache))
        .route("/cache/lookup", post(lookup_cache))
        .route("/cache/watch", post(watch_cache))
        .route("/cache/sync-path", post(sync_path))
        .route("/policy/check", post(check_permission))
        .route("/policy/sync-db", post(sync_policy_db))
        .route("/db/migrate", post(db_migrate))
        .route("/model/chat", post(chat_ollama))
        .route(
            "/conversations",
            get(list_conversations).post(start_conversation),
        )
        .route(
            "/conversations/{id}/messages",
            get(get_conversation_messages).post(reply_conversation),
        )
        .route(
            "/conversations/{id}/messages/stream",
            post(reply_conversation_stream),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(state)
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-permitted-cross-domain-policies"),
            HeaderValue::from_static("none"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
        ))
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    println!(
        "bob-api listening on http://{} auth={} cors_origin={} mode=unified",
        bind_addr,
        if token.is_some() {
            "enabled"
        } else {
            "disabled"
        },
        cli.cors_origin
    );
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true
    }))
}

async fn get_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<BobConfig>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "config.show", None, None).await?;
    Ok(Json((*state.cfg).clone()))
}

async fn get_workspace_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WorkspaceProfileRecord>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "workspace.profile.read", None, None).await?;

    let manager = state.db_manager.clone();
    let profile = manager.get_workspace_profile().await.map_err(ApiError::internal)?;
    Ok(Json(profile))
}

async fn save_workspace_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<WorkspaceProfileRequest>,
) -> Result<Json<WorkspaceProfileRecord>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "workspace.profile.write", None, None).await?;

    if !req.personalization.is_object() {
        return Err(ApiError::bad_request(
            "personalization must be a JSON object",
        ));
    }

    let update = WorkspaceProfileUpdate {
        default_model: req.default_model,
        system_prompt: req.system_prompt,
        context_injection: req.context_injection,
        personalization: req.personalization,
    };

    let manager = state.db_manager.clone();
    let profile = manager
        .save_workspace_profile(update)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(profile))
}

async fn list_ollama_endpoints(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<OllamaEndpointRecord>>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "workspace.ollama.read", None, None).await?;

    let manager = state.db_manager.clone();
    let endpoints = manager
        .list_ollama_endpoints()
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(endpoints))
}

async fn upsert_ollama_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<OllamaEndpointUpsertRequest>,
) -> Result<Json<OllamaEndpointRecord>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "workspace.ollama.write", None, None).await?;

    let input = OllamaEndpointUpsert {
        id: req.id,
        name: req.name,
        base_url: req.base_url,
        kind: req.kind,
        enabled: req.enabled,
        is_default: req.is_default,
        auth_token: req.auth_token,
        clear_auth_token: req.clear_auth_token,
    };

    let manager = state.db_manager.clone();
    let endpoint = manager
        .upsert_ollama_endpoint(input)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(endpoint))
}

async fn delete_ollama_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "workspace.ollama.delete", None, None).await?;

    let manager = state.db_manager.clone();
    manager
        .delete_ollama_endpoint(&id)
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_default_ollama_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<OllamaEndpointRecord>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "workspace.ollama.default", None, None).await?;

    let manager = state.db_manager.clone();
    let endpoint = manager
        .set_default_ollama_endpoint(&id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(endpoint))
}

async fn test_ollama_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<OllamaEndpointTestResponse>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "workspace.ollama.test", None, None).await?;

    let manager = state.db_manager.clone();
    let endpoint = manager
        .get_ollama_endpoint(&id)
        .await
        .map_err(ApiError::internal)?;

    let endpoint_id = endpoint.id.clone();
    let endpoint_name = endpoint.name.clone();
    let base_url = endpoint.base_url;
    let auth_token = endpoint.auth_token;
    let models = run_blocking(move || ollama_list_models(&base_url, auth_token.as_deref())).await?;
    let model_names = models
        .iter()
        .map(|item| item.name.clone())
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>();

    Ok(Json(OllamaEndpointTestResponse {
        endpoint_id,
        endpoint_name,
        reachable: true,
        model_count: model_names.len(),
        model_names,
    }))
}

async fn list_ollama_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OllamaModelsQuery>,
) -> Result<Json<OllamaModelsResponse>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "workspace.ollama.models", None, None).await?;

    let manager = state.db_manager.clone();
    let fallback_ollama_url = (*state.default_ollama_url).clone();
    let endpoint_id = query.endpoint_id;
    let endpoint = manager
        .resolve_ollama_endpoint(endpoint_id.as_deref(), &fallback_ollama_url)
        .await
        .map_err(ApiError::internal)?;

    let resolved_endpoint_id = endpoint.id.clone();
    let resolved_endpoint_name = endpoint.name.clone();
    let base_url = endpoint.base_url;
    let auth_token = endpoint.auth_token;

    let models = run_blocking(move || ollama_list_models(&base_url, auth_token.as_deref())).await?;
    let model_names = models
        .iter()
        .map(|item| {
            if item.name.trim().is_empty() {
                item.model.clone()
            } else {
                item.name.clone()
            }
        })
        .collect::<Vec<_>>();

    Ok(Json(OllamaModelsResponse {
        endpoint_id: resolved_endpoint_id,
        endpoint_name: resolved_endpoint_name,
        model_names,
    }))
}

async fn build_cache(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<BuildCacheRequest>,
) -> Result<Json<BuildCacheResponse>, ApiError> {
    require_api_auth(&state, &headers)?;
    let normalized_root = ensure_allowed_root(&state.cfg, &req.root)?;
    authorize_tool(&state, "cache.build", Some(normalized_root), None).await?;

    let output = req
        .output
        .unwrap_or_else(|| state.cfg.fs_cache_path.display().to_string());
    let root = req.root;
    let exclude = req.exclude;

    let result = run_blocking(move || {
        let root_path = PathBuf::from(root);
        let output_path = PathBuf::from(&output);
        let index = FsIndex::build(&root_path, &exclude)?;
        let entries = index.total_entries();
        index.save(&output_path)?;
        Ok(BuildCacheResponse { entries, output })
    })
    .await?;

    Ok(Json(result))
}

async fn lookup_cache(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LookupCacheRequest>,
) -> Result<Json<LookupCacheResponse>, ApiError> {
    require_api_auth(&state, &headers)?;
    if req.iterations == 0 {
        return Err(ApiError::bad_request("iterations must be > 0"));
    }
    let normalized_path = ensure_allowed_root(&state.cfg, &req.path)?;
    authorize_tool(&state, "cache.lookup", Some(normalized_path), None).await?;

    let cache = req
        .cache
        .unwrap_or_else(|| state.cfg.fs_cache_path.display().to_string());
    let path = req.path;
    let iterations = req.iterations;

    let result = run_blocking(move || {
        let index = FsIndex::load(PathBuf::from(&cache).as_path())?;
        let lookup_path = std::fs::canonicalize(&path).unwrap_or_else(|_| PathBuf::from(path));
        let lookup_path_str = lookup_path.to_string_lossy().to_string();
        let entry = index.lookup(&lookup_path_str).cloned();

        let started = Instant::now();
        for _ in 0..iterations {
            let _ = index.lookup(&lookup_path_str);
        }
        let elapsed_ns = started.elapsed().as_nanos();
        let avg_lookup_ns = (elapsed_ns / u128::from(iterations)) as u64;

        Ok(LookupCacheResponse {
            entry,
            avg_lookup_ns,
            iterations,
        })
    })
    .await?;

    Ok(Json(result))
}

async fn watch_cache(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<WatchCacheRequest>,
) -> Result<Json<WatchSummary>, ApiError> {
    require_api_auth(&state, &headers)?;
    let normalized_root = ensure_allowed_root(&state.cfg, &req.root)?;
    authorize_tool(&state, "cache.watch", Some(normalized_root), None).await?;

    let root = req.root;
    let output = req
        .output
        .unwrap_or_else(|| state.cfg.fs_cache_path.display().to_string());
    let exclude = req.exclude;
    let max_events = req.max_events;
    let idle_timeout_ms = req.idle_timeout_ms;

    let summary = run_blocking(move || {
        watch_and_persist(
            PathBuf::from(root).as_path(),
            PathBuf::from(output).as_path(),
            &exclude,
            max_events,
            idle_timeout_ms,
        )
    })
    .await?;

    Ok(Json(summary))
}

async fn sync_path(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SyncPathRequest>,
) -> Result<Json<SyncPathResponse>, ApiError> {
    require_api_auth(&state, &headers)?;
    let normalized_path = ensure_allowed_root(&state.cfg, &req.path)?;
    authorize_tool(&state, "cache.sync_path", Some(normalized_path), None).await?;

    let cache = req
        .cache
        .unwrap_or_else(|| state.cfg.fs_cache_path.display().to_string());
    let path = req.path;

    let response = run_blocking(move || {
        let cache_path = PathBuf::from(&cache);
        let mut index = FsIndex::load(&cache_path)?;
        index.apply_path_change(PathBuf::from(&path).as_path())?;
        index.save(&cache_path)?;
        Ok(SyncPathResponse {
            cache,
            path,
            entries: index.total_entries(),
        })
    })
    .await?;

    Ok(Json(response))
}

async fn check_permission(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CheckPermissionRequest>,
) -> Result<Json<PermissionDecision>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "permission.check", None, None).await?;

    let decision = {
        let engine = state.permission.read().await;
        engine
            .authorize_and_audit(&PermissionRequest {
                tool: req.tool,
                path: req.path,
                command: req.command,
            })
            .map_err(ApiError::internal)?
    };
    Ok(Json(decision))
}

async fn sync_policy_db(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SyncPolicyDbRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "policy.sync_db", None, None).await?;

    let scope = req.scope.unwrap_or_else(|| state.cfg.policy_scope.clone());
    let postgres_url = state.cfg.postgres_url.clone();
    let (db_rules, summary) =
        run_blocking(move || load_db_policy_rules(&postgres_url, &scope)).await?;

    {
        let mut engine = state.permission.write().await;
        engine.apply_db_rules(db_rules);
        if req.persist {
            engine.persist_policy().map_err(ApiError::internal)?;
        }
    }

    Ok(Json(json!({
        "scope": summary.scope,
        "tool_rules": summary.tool_rules,
        "path_rules": summary.path_rules,
        "command_rules": summary.command_rules,
        "persisted": req.persist
    })))
}

async fn db_migrate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DbMigrateRequest>,
) -> Result<Json<MigrationSummary>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "db.migrate", None, None).await?;

    let postgres_url = state.cfg.postgres_url.clone();
    let dir = req
        .dir
        .map(PathBuf::from)
        .unwrap_or_else(|| state.cfg.migrations_dir.clone());
    let summary = run_blocking(move || apply_migrations(&postgres_url, &dir)).await?;
    Ok(Json(summary))
}

async fn chat_ollama(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<OllamaGenerateResponse>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "model.chat", None, None).await?;

    if req.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt cannot be empty"));
    }

    let should_load_profile = req
        .model
        .as_ref()
        .map(|model| model.trim().is_empty())
        .unwrap_or(true)
        || req.system.is_none();

    let profile = if should_load_profile {
        let manager = state.db_manager.clone();
        Some(manager.get_workspace_profile().await.map_err(ApiError::internal)?)
    } else {
        None
    };

    let model = req
        .model
        .and_then(normalize_non_empty)
        .or_else(|| profile.as_ref().map(|value| value.default_model.clone()))
        .ok_or_else(|| ApiError::bad_request("model is required"))?;
    let system = req
        .system
        .or_else(|| profile.as_ref().map(|value| value.system_prompt.clone()));

    let messages = vec![OrchestratorChatMessage {
        role: "user".to_string(),
        content: req.prompt,
    }];
    let prompt = build_prompt(system.clone(), None, None, &messages);

    let manager = state.db_manager.clone();
    let fallback_ollama_url = (*state.default_ollama_url).clone();
    let endpoint_id = req.ollama_endpoint_id;
    let endpoint = manager
        .resolve_ollama_endpoint(endpoint_id.as_deref(), &fallback_ollama_url)
        .await
        .map_err(ApiError::internal)?;

    let base_url = endpoint.base_url;
    let auth_token = endpoint.auth_token;
    let response = run_blocking(move || {
        ollama_generate(&base_url, &model, &prompt, system, auth_token.as_deref())
    })
    .await?;
    Ok(Json(response))
}

async fn start_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StartConversationRequest>,
) -> Result<Json<ConversationRecord>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "conversation.start", None, None).await?;

    let title = req.title;
    let manager = state.db_manager.clone();
    let conversation: ConversationRecord =
        manager
            .create_conversation(title.as_deref())
            .await
            .map_err(ApiError::internal)?;
    Ok(Json(conversation))
}

async fn list_conversations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ConversationListQuery>,
) -> Result<Json<Vec<ConversationRecord>>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "conversation.list", None, None).await?;

    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let manager = state.db_manager.clone();
    let conversations = manager
        .list_conversations(limit)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(conversations))
}

async fn get_conversation_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<ConversationMessagesQuery>,
) -> Result<Json<Vec<MessageRecord>>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "conversation.read", None, None).await?;

    let limit = query.limit.unwrap_or(200).clamp(1, 500);
    let manager = state.db_manager.clone();
    let messages = manager
        .list_messages(&id, limit)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(messages))
}

async fn reply_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ConversationMessageRequest>,
) -> Result<Json<ConversationReplyResponse>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "conversation.reply", None, None).await?;

    if req.message.trim().is_empty() {
        return Err(ApiError::bad_request("message cannot be empty"));
    }

    let should_load_profile = req
        .model
        .as_ref()
        .map(|model| model.trim().is_empty())
        .unwrap_or(true)
        || req.system.is_none()
        || req.context_injection.is_none()
        || req.personalization.is_none();

    let profile = if should_load_profile {
        let manager = state.db_manager.clone();
        Some(manager.get_workspace_profile().await.map_err(ApiError::internal)?)
    } else {
        None
    };

    let model = req
        .model
        .and_then(normalize_non_empty)
        .or_else(|| profile.as_ref().map(|value| value.default_model.clone()))
        .ok_or_else(|| ApiError::bad_request("model is required"))?;
    let system = req
        .system
        .or_else(|| profile.as_ref().map(|value| value.system_prompt.clone()));
    let context_injection = req.context_injection.or_else(|| {
        profile
            .as_ref()
            .map(|value| value.context_injection.clone())
    });
    let personalization = req
        .personalization
        .or_else(|| profile.as_ref().map(|value| value.personalization.clone()));

    let user_message_content = req.message.clone();
    let conversation_id = id.clone();
    let manager = state.db_manager.clone();
    let user_message =
        manager
            .add_message(&conversation_id, "user", &user_message_content)
            .await
            .map_err(ApiError::internal)?;

    let history_limit = req.history_limit.unwrap_or(40).clamp(1, 500);
    let conversation_id = id.clone();
    let manager = state.db_manager.clone();
    let history =
        manager
            .list_messages(&conversation_id, history_limit)
            .await
            .map_err(ApiError::internal)?;

    let prompt_messages: Vec<OrchestratorChatMessage> = history
        .iter()
        .map(|message| OrchestratorChatMessage {
            role: message.role.clone(),
            content: message.content.clone(),
        })
        .collect();
    let prompt = build_prompt(
        system.clone(),
        context_injection,
        personalization,
        &prompt_messages,
    );

    let manager = state.db_manager.clone();
    let fallback_ollama_url = (*state.default_ollama_url).clone();
    let endpoint_id = req.ollama_endpoint_id;
    let endpoint = manager
        .resolve_ollama_endpoint(endpoint_id.as_deref(), &fallback_ollama_url)
        .await
        .map_err(ApiError::internal)?;

    let resolved_endpoint_id = endpoint.id.clone();
    let base_url = endpoint.base_url;
    let auth_token = endpoint.auth_token;
    let model_response = run_blocking(move || {
        ollama_generate(&base_url, &model, &prompt, system, auth_token.as_deref())
    })
    .await?;

    let assistant_content = model_response.response.clone();
    let conversation_id = id.clone();
    let manager = state.db_manager.clone();
    let assistant_message = manager
        .add_message(&conversation_id, "assistant", &assistant_content)
        .await
        .map_err(ApiError::internal)?;

    Ok(Json(ConversationReplyResponse {
        conversation_id: id,
        ollama_endpoint_id: resolved_endpoint_id,
        user_message,
        assistant_message,
        model_response,
    }))
}

async fn reply_conversation_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ConversationMessageRequest>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    require_api_auth(&state, &headers)?;
    authorize_tool(&state, "conversation.reply", None, None).await?;

    if req.message.trim().is_empty() {
        return Err(ApiError::bad_request("message cannot be empty"));
    }

    // Resolve profile defaults
    let should_load_profile = req
        .model
        .as_ref()
        .map(|m| m.trim().is_empty())
        .unwrap_or(true)
        || req.system.is_none()
        || req.context_injection.is_none()
        || req.personalization.is_none();

    let profile = if should_load_profile {
        let manager = state.db_manager.clone();
        Some(manager.get_workspace_profile().await.map_err(ApiError::internal)?)
    } else {
        None
    };

    let model = req
        .model
        .and_then(normalize_non_empty)
        .or_else(|| profile.as_ref().map(|v| v.default_model.clone()))
        .ok_or_else(|| ApiError::bad_request("model is required"))?;
    let system = req
        .system
        .or_else(|| profile.as_ref().map(|v| v.system_prompt.clone()));
    let context_injection = req
        .context_injection
        .or_else(|| profile.as_ref().map(|v| v.context_injection.clone()));
    let personalization = req
        .personalization
        .or_else(|| profile.as_ref().map(|v| v.personalization.clone()));

    // Store user message
    let user_message_content = req.message.clone();
    let conversation_id = id.clone();
    let manager = state.db_manager.clone();
    let user_message: MessageRecord =
        manager
            .add_message(&conversation_id, "user", &user_message_content)
            .await
            .map_err(ApiError::internal)?;

    // Build prompt from history
    let history_limit = req.history_limit.unwrap_or(40).clamp(1, 500);
    let conversation_id = id.clone();
    let manager = state.db_manager.clone();
    let history =
        manager
            .list_messages(&conversation_id, history_limit)
            .await
            .map_err(ApiError::internal)?;

    let prompt_messages: Vec<OrchestratorChatMessage> = history
        .iter()
        .map(|m| OrchestratorChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();
    let prompt = build_prompt(system.clone(), context_injection, personalization, &prompt_messages);

    // Resolve endpoint
    let manager = state.db_manager.clone();
    let fallback_ollama_url = (*state.default_ollama_url).clone();
    let endpoint_id = req.ollama_endpoint_id;
    let endpoint = manager
        .resolve_ollama_endpoint(endpoint_id.as_deref(), &fallback_ollama_url)
        .await
        .map_err(ApiError::internal)?;

    let resolved_endpoint_id = endpoint.id.clone();
    let base_url = endpoint.base_url;
    let auth_token = endpoint.auth_token;

    // Send initial metadata event with user_message info, then stream tokens
    let ollama_stream = ollama_generate_stream(
        &base_url,
        &model,
        &prompt,
        system,
        auth_token.as_deref(),
    )
    .await
    .map_err(|e| ApiError::internal(e))?;

    let conv_id = id.clone();
    let db = state.db_manager.clone();
    let user_message_json =
        serde_json::to_value(&user_message).map_err(|e: serde_json::Error| ApiError::internal(e))?;
    let sse_stream = async_stream::stream! {
        // Emit metadata event first
        let meta = serde_json::json!({
            "conversation_id": conv_id,
            "ollama_endpoint_id": resolved_endpoint_id,
            "user_message": user_message_json,
            "model": model,
        });
        yield Ok::<Event, std::convert::Infallible>(Event::default().event("meta").data(meta.to_string()));

        let mut full_response = String::new();
        tokio::pin!(ollama_stream);

        while let Some(chunk_result) = ollama_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    full_response.push_str(&chunk.response);
                    let data = serde_json::json!({
                        "token": chunk.response,
                        "done": chunk.done,
                    });
                    yield Ok::<Event, std::convert::Infallible>(Event::default().event("token").data(data.to_string()));

                    if chunk.done {
                        let stats = serde_json::json!({
                            "prompt_eval_count": chunk.prompt_eval_count,
                            "eval_count": chunk.eval_count,
                            "total_duration": chunk.total_duration,
                        });
                        yield Ok::<Event, std::convert::Infallible>(Event::default().event("stats").data(stats.to_string()));
                    }
                }
                Err(e) => {
                    let err = serde_json::json!({ "error": e.to_string() });
                    yield Ok::<Event, std::convert::Infallible>(Event::default().event("error").data(err.to_string()));
                    return;
                }
            }
        }

        // Store the complete assistant message in DB
        let content = full_response;
        let cid = conv_id.clone();
        let stored = db
            .add_message(&cid, "assistant", &content)
            .await;

        match stored {
            Ok(msg) => {
                let done_data = serde_json::json!({
                    "assistant_message": msg,
                });
                yield Ok::<Event, std::convert::Infallible>(Event::default().event("done").data(done_data.to_string()));
            }
            Err(e) => {
                let err = serde_json::json!({ "error": format!("failed to store message: {}", e) });
                yield Ok::<Event, std::convert::Infallible>(Event::default().event("error").data(err.to_string()));
            }
        }
    };

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::default()))
}

fn build_prompt(
    system: Option<String>,
    context_injection: Option<String>,
    personalization: Option<serde_json::Value>,
    messages: &[OrchestratorChatMessage],
) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(value) = system.filter(|v| !v.trim().is_empty()) {
        lines.push("System Instructions:".to_string());
        lines.push(value);
    }
    if let Some(value) = context_injection.filter(|v| !v.trim().is_empty()) {
        lines.push("Context Injection:".to_string());
        lines.push(value);
    }
    if let Some(value) = personalization {
        lines.push("Personalization Settings (JSON):".to_string());
        lines.push(value.to_string());
    }
    for message in messages {
        if message.content.trim().is_empty() {
            continue;
        }
        let role = message.role.to_uppercase();
        lines.push(format!("{}: {}", role, message.content));
    }
    lines.push("ASSISTANT:".to_string());

    lines.join("\n\n")
}

fn build_cors(origin: &str) -> Result<CorsLayer> {
    let allow_headers = [
        header::CONTENT_TYPE,
        header::AUTHORIZATION,
        HeaderName::from_static("x-api-key"),
    ];
    let allow_methods = [
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::DELETE,
        Method::OPTIONS,
    ];
    let layer = if origin == "*" {
        CorsLayer::new().allow_origin(Any)
    } else {
        let header_value: HeaderValue = origin
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid CORS origin {}: {}", origin, e))?;
        CorsLayer::new().allow_origin(header_value)
    };
    Ok(layer.allow_methods(allow_methods).allow_headers(allow_headers))
}

fn require_api_auth(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(expected) = state.api_token.as_ref() else {
        return Ok(());
    };

    let provided = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|token| token.strip_prefix("Bearer "))
                .map(ToOwned::to_owned)
        });

    match provided {
        Some(token) if secure_token_eq(&token, expected) => Ok(()),
        _ => Err(ApiError::unauthorized("missing or invalid API token")),
    }
}

fn secure_token_eq(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let max = left_bytes.len().max(right_bytes.len());
    let mut diff = left_bytes.len() ^ right_bytes.len();
    for i in 0..max {
        let l = *left_bytes.get(i).unwrap_or(&0);
        let r = *right_bytes.get(i).unwrap_or(&0);
        diff |= (l ^ r) as usize;
    }
    diff == 0
}

async fn authorize_tool(
    state: &AppState,
    tool: &str,
    path: Option<String>,
    command: Option<String>,
) -> Result<(), ApiError> {
    let decision = {
        let engine = state.permission.read().await;
        engine
            .authorize_and_audit(&PermissionRequest {
                tool: tool.to_string(),
                path,
                command,
            })
            .map_err(ApiError::internal)?
    };

    if decision.allowed {
        return Ok(());
    }
    Err(ApiError::forbidden(format!(
        "permission denied: {}",
        decision.reason
    )))
}

async fn run_blocking<T, F>(f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::internal)
}

fn create_pg_pool(postgres_url: &str) -> anyhow::Result<Pool> {
    let cfg = postgres_url.parse::<tokio_postgres::Config>()?;
    let mgr_config = ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    };
    let manager = Manager::from_config(cfg, NoTls, mgr_config);
    Ok(Pool::builder(manager).max_size(16).build()?)
}

fn default_iterations() -> u32 {
    10_000
}

fn default_idle_timeout_ms() -> u64 {
    30_000
}

fn default_endpoint_kind() -> String {
    "local".to_string()
}

fn default_true() -> bool {
    true
}

fn ensure_allowed_root(cfg: &BobConfig, raw_path: &str) -> Result<String, ApiError> {
    let candidate = absolute_candidate(raw_path).map_err(ApiError::internal)?;
    let normalized = std::fs::canonicalize(&candidate).unwrap_or(candidate);

    let is_allowed = cfg.allowed_roots.iter().any(|root| {
        let root_abs = if root.is_absolute() {
            root.clone()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(root))
                .unwrap_or_else(|_| root.clone())
        };
        let root_norm = std::fs::canonicalize(&root_abs).unwrap_or(root_abs);
        normalized == root_norm || normalized.starts_with(&root_norm)
    });

    if is_allowed {
        return Ok(normalized.to_string_lossy().to_string());
    }

    Err(ApiError::forbidden(format!(
        "path not within BOB_ALLOWED_ROOTS: {}",
        normalized.display()
    )))
}

fn absolute_candidate(raw_path: &str) -> Result<PathBuf> {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(path))
}

fn normalize_token(token: String) -> Option<String> {
    if token.trim().is_empty() {
        None
    } else {
        Some(token)
    }
}

fn validate_api_token(token: &str) -> Result<()> {
    if token.len() < 16 {
        anyhow::bail!("BOB_API_TOKEN must be at least 16 characters.");
    }
    let valid = token.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~')
    });
    if !valid {
        anyhow::bail!(
            "BOB_API_TOKEN contains unsupported characters. \
             Use only [A-Za-z0-9._~-] so it can be sent safely in HTTP headers."
        );
    }
    Ok(())
}

fn normalize_non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
