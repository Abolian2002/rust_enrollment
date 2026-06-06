use admissions_agent::{AdmissionsAgent, chunk_reply_text};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use db::Database;
use domain::{ChatRequest, fail, ok, ok_with_meta};
use serde_json::json;
use std::collections::HashMap;
use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::{
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    db: Database,
    agent: AdmissionsAgent,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_env();
    init_tracing();
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:55432/hnu_enrollment".to_owned()
    });
    let db = Database::connect_lazy(&database_url)?;
    let state = Arc::new(AppState {
        agent: AdmissionsAgent::new(db.clone()),
        db,
    });
    let app = build_router(state);
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(4000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, "rust enrollment api listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn load_env() {
    let _ = dotenvy::from_filename(".env");
    let _ = dotenvy::from_filename("../../.env");
}

fn init_tracing() {
    let filter = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "api=info,admissions_agent=info,tower_http=info".to_owned());
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(filter))
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/chat", post(chat))
        .route("/api/v1/chat/stream", post(chat_stream))
        .route("/api/v1/chat/history/{conversation_id}", get(chat_history))
        .route("/api/v1/majors", get(list_majors))
        .route("/api/v1/majors/{slug}", get(get_major))
        .route("/api/v1/admission/scores", get(admission_scores))
        .route(
            "/api/v1/admission/plans/by-major",
            get(admission_plans_by_major),
        )
        .route("/api/v1/knowledge/faq", get(knowledge_faq))
        .route("/api/v1/knowledge/policies", get(knowledge_policies))
        .route("/api/v1/tts/token", post(tts_token))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(90),
        ))
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let db_status = match state.db.health_check().await {
        Ok(()) => "ok",
        Err(_) => "unavailable",
    };
    Json(ok(json!({
        "service": "rust-enrollment-api",
        "status": "ok",
        "database": db_status
    })))
}

async fn chat(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> impl IntoResponse {
    match state.agent.chat(payload).await {
        Ok(reply) => {
            let meta = reply
                .diagnostics
                .as_ref()
                .map(|diagnostics| json!({ "diagnostics": diagnostics }))
                .unwrap_or_else(|| json!({}));
            (StatusCode::OK, Json(ok_with_meta(reply, meta))).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "chat request failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(fail(
                    "CHAT_ERROR",
                    "当前咨询人数较多，暂时无法完成本次查询，请稍后再试。",
                )),
            )
                .into_response()
        }
    }
}

async fn chat_history(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_conversation_history(&conversation_id).await {
        Ok(Some(history)) => (StatusCode::OK, Json(ok(history))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(fail("NOT_FOUND", "Conversation not found")),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to load conversation history");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(fail("HISTORY_ERROR", "无法读取对话历史。")),
            )
                .into_response()
        }
    }
}

async fn chat_stream(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(payload): Json<ChatRequest>,
) -> impl IntoResponse {
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);
    let agent = state.agent.clone();

    tokio::spawn(async move {
        let send = |tx: mpsc::Sender<Result<Event, Infallible>>, event: Event| async move {
            tx.send(Ok(event)).await.is_ok()
        };

        if !send(tx.clone(), status_event("resolving")).await {
            return;
        }
        if !send(tx.clone(), status_event("retrieving")).await {
            return;
        }

        match agent.chat(payload).await {
            Ok(reply) => {
                if !send(tx.clone(), status_event("generating")).await {
                    return;
                }

                for chunk in chunk_reply_text(&reply.reply) {
                    let event = Event::default().event("chunk").data(
                        json!({
                            "conversationId": reply.conversation_id,
                            "delta": chunk
                        })
                        .to_string(),
                    );
                    if !send(tx.clone(), event).await {
                        return;
                    }
                }

                let meta = reply
                    .diagnostics
                    .as_ref()
                    .map(|diagnostics| json!({ "diagnostics": diagnostics }))
                    .unwrap_or_else(|| json!({}));
                let event = Event::default().event("message").data(
                    serde_json::to_string(&ok_with_meta(reply, meta))
                        .unwrap_or_else(|_| "{}".to_owned()),
                );
                if !send(tx.clone(), event).await {
                    return;
                }
            }
            Err(error) => {
                tracing::error!(error = %error, "stream chat request failed");
                let event = Event::default().event("message").data(
                    serde_json::to_string(&fail(
                        "CHAT_ERROR",
                        "当前咨询人数较多，暂时无法完成本次查询，请稍后再试。",
                    ))
                    .unwrap_or_else(|_| "{}".to_owned()),
                );
                if !send(tx.clone(), event).await {
                    return;
                }
            }
        }

        let _ = send(
            tx,
            Event::default()
                .event("done")
                .data(json!({ "done": true }).to_string()),
        )
        .await;
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(axum::response::sse::KeepAlive::default())
}

fn status_event(status: &'static str) -> Event {
    Event::default()
        .event("status")
        .data(json!({ "status": status }).to_string())
}

async fn list_majors(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let query = params.get("q").map(|value| value.trim().to_owned());
    match state.db.list_major_catalog().await {
        Ok(majors) => {
            let filtered = majors
                .into_iter()
                .filter(|major| {
                    query.as_ref().is_none_or(|query| {
                        major.name.contains(query) || major.slug.contains(query)
                    })
                })
                .map(|major| {
                    json!({
                        "id": major.slug,
                        "slug": major.slug,
                        "code": major.code.unwrap_or_default(),
                        "name": major.name,
                        "degreeLevel": null,
                        "durationYears": null,
                        "tuitionFee": null,
                        "isNormalMajor": major.is_normal_major,
                        "hasMaster": false,
                        "hasDoctor": false,
                        "university": { "code": "HRBNU", "name": "哈尔滨师范大学" },
                        "latestScore": null,
                        "tags": []
                    })
                })
                .collect::<Vec<_>>();
            (StatusCode::OK, Json(ok(json!(filtered)))).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "failed to list majors");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(fail("MAJORS_ERROR", "无法读取专业目录。")),
            )
                .into_response()
        }
    }
}

async fn get_major(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match state.db.list_major_catalog().await {
        Ok(majors) => {
            let Some(major) = majors.into_iter().find(|major| major.slug == slug) else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(fail("NOT_FOUND", format!("Major {slug} was not found"))),
                )
                    .into_response();
            };
            (
                StatusCode::OK,
                Json(ok(json!({
                    "id": major.slug,
                    "slug": major.slug,
                    "code": major.code.unwrap_or_default(),
                    "name": major.name,
                    "degreeLevel": null,
                    "durationYears": null,
                    "tuitionFee": null,
                    "isNormalMajor": major.is_normal_major,
                    "hasMaster": false,
                    "hasDoctor": false,
                    "introduction": null,
                    "employmentSummary": null,
                    "postgraduateSummary": null,
                    "university": { "code": "HRBNU", "name": "哈尔滨师范大学" },
                    "scoreTrend": [],
                    "planTrend": []
                }))),
            )
                .into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "failed to load major");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(fail("MAJOR_ERROR", "无法读取专业详情。")),
            )
                .into_response()
        }
    }
}

async fn admission_scores(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let province = params.get("province").cloned().unwrap_or_default();
    let major_slug = params
        .get("majorSlug")
        .or_else(|| params.get("majorId"))
        .cloned()
        .unwrap_or_default();
    if province.is_empty() || major_slug.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(fail("BAD_REQUEST", "province and majorSlug are required")),
        )
            .into_response();
    }
    let year = params
        .get("year")
        .and_then(|value| value.parse::<i32>().ok());
    let subject_type = params.get("subjectType").map(String::as_str);
    match state
        .db
        .query_admission_scores(&province, &major_slug, subject_type, year)
        .await
    {
        Ok(records) => (StatusCode::OK, Json(ok(records))).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to query admission scores");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(fail("SCORES_ERROR", "无法读取录取分数。")),
            )
                .into_response()
        }
    }
}

async fn admission_plans_by_major() -> impl IntoResponse {
    (StatusCode::OK, Json(ok(json!([])))).into_response()
}

async fn knowledge_faq(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let query = params.get("q").cloned().unwrap_or_default();
    match state.db.search_faq(&query, 50).await {
        Ok(faq) => (StatusCode::OK, Json(ok(faq))).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to search faq");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(fail("FAQ_ERROR", "无法读取 FAQ。")),
            )
                .into_response()
        }
    }
}

async fn knowledge_policies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let query = params.get("q").cloned().unwrap_or_default();
    let filters = db::KnowledgeSearchFilters {
        category: params.get("category").cloned(),
        year: params
            .get("year")
            .and_then(|value| value.parse::<i32>().ok()),
        document_kind: None,
    };
    match state.db.search_policies(&query, &filters, 50).await {
        Ok(policies) => (StatusCode::OK, Json(ok(policies))).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to search policies");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(fail("POLICY_ERROR", "无法读取政策资料。")),
            )
                .into_response()
        }
    }
}

async fn tts_token() -> impl IntoResponse {
    let api_key = match std::env::var("DASHSCOPE_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(fail(
                    "TTS_CONFIG_ERROR",
                    "DASHSCOPE_API_KEY is not configured",
                )),
            )
                .into_response();
        }
    };

    let response = match reqwest::Client::new()
        .post("https://dashscope.aliyuncs.com/api/v1/tokens")
        .bearer_auth(api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(error = %error, "failed to request DashScope TTS token");
            return (
                StatusCode::BAD_GATEWAY,
                Json(fail(
                    "TTS_TOKEN_ERROR",
                    "Failed to fetch temporary token from DashScope",
                )),
            )
                .into_response();
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!(%status, body = %body, "DashScope TTS token API returned an error");
        return (
            StatusCode::BAD_GATEWAY,
            Json(fail(
                "TTS_TOKEN_ERROR",
                "Failed to fetch temporary token from DashScope",
            )),
        )
            .into_response();
    }

    let payload = match response.json::<serde_json::Value>().await {
        Ok(payload) => payload,
        Err(error) => {
            tracing::error!(error = %error, "failed to parse DashScope TTS token response");
            return (
                StatusCode::BAD_GATEWAY,
                Json(fail(
                    "TTS_TOKEN_ERROR",
                    "Failed to parse temporary token from DashScope",
                )),
            )
                .into_response();
        }
    };

    let token = payload
        .get("token")
        .and_then(|value| value.as_str())
        .or_else(|| {
            payload
                .get("data")
                .and_then(|data| data.get("token"))
                .and_then(|value| value.as_str())
        });

    match token {
        Some(token) if !token.trim().is_empty() => {
            (StatusCode::OK, Json(ok(json!({ "token": token })))).into_response()
        }
        _ => {
            tracing::error!(response = %payload, "DashScope returned empty TTS token");
            (
                StatusCode::BAD_GATEWAY,
                Json(fail("TTS_TOKEN_ERROR", "DashScope returned empty token")),
            )
                .into_response()
        }
    }
}
