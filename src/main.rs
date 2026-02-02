// 演示：基于 Axum 的高并发 HTTP 服务 + 强类型 JSON + 并发任务
// 端点包括：根路径、健康检查、求和（查询参数）、回显（POST JSON）、并发计算示例
use std::{net::SocketAddr, sync::Arc, time::Duration};

// Axum：路由、提取器、响应类型
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
// Serde：序列化/反序列化（类型安全地映射请求/响应）
use serde::{Deserialize, Serialize};
// Tokio：异步运行时与并发任务管理
use tokio::{task::JoinSet, time::sleep};
// tracing：结构化日志
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use thiserror::Error;
use tower::timeout::TimeoutLayer;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Clone)]
struct AppState {
    start: Instant,
    hits_root: AtomicU64,
    hits_health: AtomicU64,
    hits_sum: AtomicU64,
    hits_echo: AtomicU64,
    hits_parallel: AtomicU64,
}

// 健康检查返回体
#[derive(Serialize)]
struct Health {
    status: &'static str,
}

// 查询参数：以逗号分隔的整数字符串
#[derive(Deserialize)]
struct SumQuery {
    nums: String,
}

// 回显请求体（POST JSON）
#[derive(Deserialize, Serialize)]
struct EchoBody {
    message: String,
}

// 并发示例返回项：索引、计算值、耗时毫秒
#[derive(Serialize)]
struct ParallelResult {
    index: usize,
    value: usize,
    ms: u64,
}

// 程序入口：初始化日志、注册路由、启动 TCP 监听并提供服务
#[tokio::main]
async fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_env_filter("info")
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    let state = Arc::new(AppState {
        start: Instant::now(),
        hits_root: AtomicU64::new(0),
        hits_health: AtomicU64::new(0),
        hits_sum: AtomicU64::new(0),
        hits_echo: AtomicU64::new(0),
        hits_parallel: AtomicU64::new(0),
    });

    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/sum", get(sum))
        .route("/echo", post(echo))
        .route("/parallel", get(parallel))
        .route("/metrics", get(metrics))
        .with_state(state)
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(Duration::from_secs(10)));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

// 根路径：返回 200 OK 与文本 "ok"
async fn root(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    app.hits_root.fetch_add(1, Ordering::Relaxed);
    (StatusCode::OK, "ok")
}

// 健康检查：返回 JSON {"status":"healthy"}
async fn health(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    app.hits_health.fetch_add(1, Ordering::Relaxed);
    Json(Health { status: "healthy" })
}

// 求和接口：?nums=1,2,3
// 1) 将逗号分隔的字符串拆分
// 2) 逐项解析为 i64（解析失败按 0 处理）
// 3) 计算总和并以 JSON 返回
#[derive(Error, Debug)]
enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (code, msg) = match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            AppError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (code, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

fn parse_sum_input(s: &str) -> Result<i64, AppError> {
    let mut total: i64 = 0;
    for (idx, token) in s.split(',').filter(|t| !t.is_empty()).enumerate() {
        let t = token.trim();
        match t.parse::<i64>() {
            Ok(v) => total += v,
            Err(_) => {
                return Err(AppError::BadRequest(format!(
                    "nums 第 {idx} 项不是有效整数: {t}"
                )))
            }
        }
    }
    Ok(total)
}

async fn sum(State(app): State<Arc<AppState>>, Query(q): Query<SumQuery>) -> Result<impl IntoResponse, AppError> {
    app.hits_sum.fetch_add(1, Ordering::Relaxed);
    let total = parse_sum_input(&q.nums)?;
    Ok(Json(serde_json::json!({ "total": total })))
}

// 回显接口：POST JSON，原样返回
async fn echo(State(app): State<Arc<AppState>>, Json(body): Json<EchoBody>) -> Result<impl IntoResponse, AppError> {
    app.hits_echo.fetch_add(1, Ordering::Relaxed);
    if body.message.trim().is_empty() {
        return Err(AppError::BadRequest("message 不能为空".into()));
    }
    Ok(Json(body))
}

// 并发查询参数：n 表示并发任务数量（默认 5，上限 32）
#[derive(Deserialize)]
struct ParallelQuery {
    n: Option<usize>,
}

// 并发示例：
// 1) 启动 n 个异步任务，分别 sleep 不同毫秒以模拟耗时操作
// 2) 每个任务返回其索引与平方值
// 3) 使用 JoinSet 按完成顺序收集，体现并发非阻塞
async fn parallel(State(app): State<Arc<AppState>>, Query(q): Query<ParallelQuery>) -> impl IntoResponse {
    app.hits_parallel.fetch_add(1, Ordering::Relaxed);
    let n = q.n.unwrap_or(5).min(32);
    let mut tasks = JoinSet::new();
    for i in 0..n {
        tasks.spawn(async move {
            let ms = 50 + (i as u64) * 30;
            sleep(Duration::from_millis(ms)).await;
            ParallelResult {
                index: i,
                value: i * i,
                ms,
            }
        });
    }
    let mut results = Vec::with_capacity(n);
    while let Some(res) = tasks.join_next().await {
        results.push(res.unwrap());
    }
    Json(results)
}

async fn metrics(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = app.start.elapsed().as_secs();
    Json(serde_json::json!({
        "uptime_seconds": uptime,
        "hits": {
            "root": app.hits_root.load(Ordering::Relaxed),
            "health": app.hits_health.load(Onordering::Relaxed)
        }
    }))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::parse_sum_input;

    #[test]
    fn parse_sum_ok() {
        assert_eq!(parse_sum_input("1,2,3").unwrap(), 6);
        assert_eq!(parse_sum_input(" 1 , 2 , 3 ").unwrap(), 6);
        assert_eq!(parse_sum_input("1,,3").unwrap(), 4);
    }

    #[test]
    fn parse_sum_invalid() {
        let err = parse_sum_input("1,x,3").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("不是有效整数"));
    }
}
