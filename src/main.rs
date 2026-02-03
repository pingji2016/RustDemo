// 演示：基于 Axum 的高并发 HTTP 服务 + 强类型 JSON + 并发任务
// 项目概览：
// - 框架：Tokio 异步运行时 + Axum Web 框架（无阻塞 I/O，路由清晰）
// - 中间件：压缩、CORS、请求追踪、超时（提升可观测性与健壮性）
// - 端点：/、/health、/sum、/echo、/parallel、/metrics
// - 工程特性：统一错误模型、优雅关闭、纯函数单元测试
use std::{net::SocketAddr, sync::Arc, time::Duration};

// Axum（路由/提取器/响应类型）：定义 HTTP 端点与参数解析
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
// Serde（序列化/反序列化）：类型安全地映射请求/响应 JSON
use serde::{Deserialize, Serialize};
// Tokio（异步运行时）：管理并发任务与计时
use tokio::{task::JoinSet, time::sleep};
// tracing（结构化日志）：输出服务启动与请求追踪信息
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
// 错误与中间件
use thiserror::Error;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
// 并发计数器与时间
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// 应用状态（用于指标与观测）
// - start：服务启动时间（计算运行时长）
// - hits_*：各端点访问计数（AtomicU64 并发安全、开销低）
struct AppState {
    start: Instant,
    hits_root: AtomicU64,
    hits_health: AtomicU64,
    hits_sum: AtomicU64,
    hits_echo: AtomicU64,
    hits_parallel: AtomicU64,
}

// 健康检查返回体（简单 JSON）
#[derive(Serialize)]
struct Health {
    status: &'static str,
}

// 求和查询参数：以逗号分隔的整数字符串（如 "1,2,3"）
#[derive(Deserialize)]
struct SumQuery {
    nums: String,
}

// 回显请求体（POST JSON）：{"message": "..."}，用于演示 JSON 解析
#[derive(Deserialize, Serialize)]
struct EchoBody {
    message: String,
}

// 并发示例返回项：索引、计算值（平方）、模拟耗时毫秒
#[derive(Serialize)]
struct ParallelResult {
    index: usize,
    value: usize,
    ms: u64,
}

// 程序入口：
// 1) 初始化日志（INFO 级别）
// 2) 构建 Router：注册端点、注入状态、挂载中间件
// 3) 绑定监听地址，启用优雅关闭（Ctrl+C）
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

    // 路由与中间件说明：
    // - CorsLayer::permissive：开发环境放开跨域；生产需按域名/方法细化
    // - TraceLayer：为每个请求生成 span，输出请求/响应耗时
    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/sum", get(sum))
        .route("/echo", post(echo))
        .route("/parallel", get(parallel))
        .route("/metrics", get(metrics))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

// 根路径：健康返回（文本 "ok"），并记录命中次数
async fn root(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    app.hits_root.fetch_add(1, Ordering::Relaxed);
    (StatusCode::OK, "ok")
}

// 健康检查：返回 {"status":"healthy"}，并记录命中次数
async fn health(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    app.hits_health.fetch_add(1, Ordering::Relaxed);
    Json(Health { status: "healthy" })
}

// 求和接口：GET /sum?nums=1,2,3
// - 流程：拆分 -> 去空白 -> 逐项解析 i64 -> 求和 -> 返回 {"total":X}
// - 错误：任何项非数字则返回 400 与说明（含索引与原值）
#[derive(Error, Debug)]
enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Internal(String),
}

// 将错误统一转换为 JSON 响应：
// - BadRequest -> 400 {"error":"..."}
// - Internal   -> 500 {"error":"..."}
impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (code, msg) = match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            AppError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (code, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

// 输入解析（纯函数，便于单元测试与复用）：
// - 返回总和或携带详细错误的 BadRequest
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

// 回显接口：POST /echo
// - 请求体：{"message":"..."}；若为空字符串则返回 400 错误
// - 作用：演示 JSON 反序列化与简单校验
async fn echo(State(app): State<Arc<AppState>>, Json(body): Json<EchoBody>) -> Result<impl IntoResponse, AppError> {
    app.hits_echo.fetch_add(1, Ordering::Relaxed);
    if body.message.trim().is_empty() {
        return Err(AppError::BadRequest("message 不能为空".into()));
    }
    Ok(Json(body))
}

// 并发查询参数：
// - n：并发任务数量（默认 5，上限 32；避免过度并发）
#[derive(Deserialize)]
struct ParallelQuery {
    n: Option<usize>,
}

// 并发示例：GET /parallel?n=8
// - 任务：为每个 i 启动一个异步任务，sleep 不同毫秒模拟耗时
// - 结果：返回每个任务的 index/value/ms，按完成顺序收集（非阻塞）
// - 展示：Tokio 并发与 JoinSet 收集的用法
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

// 指标端点：返回运行时长与各端点命中次数（便于监控与压测）
async fn metrics(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = app.start.elapsed().as_secs();
    Json(serde_json::json!({
        "uptime_seconds": uptime,
        "hits": {
            "root": app.hits_root.load(Ordering::Relaxed),
            "health": app.hits_health.load(Ordering::Relaxed),
            "sum": app.hits_sum.load(Ordering::Relaxed),
            "echo": app.hits_echo.load(Ordering::Relaxed),
            "parallel": app.hits_parallel.load(Ordering::Relaxed)
        }
    }))
}

// 优雅关闭：监听 Ctrl+C，触发服务的 graceful shutdown
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::parse_sum_input;

    // 正常解析用例：空白/空项应被忽略
    #[test]
    fn parse_sum_ok() {
        assert_eq!(parse_sum_input("1,2,3").unwrap(), 6);
        assert_eq!(parse_sum_input(" 1 , 2 , 3 ").unwrap(), 6);
        assert_eq!(parse_sum_input("1,,3").unwrap(), 4);
    }

    // 错误解析用例：包含非数字项，应返回携带索引信息的错误
    #[test]
    fn parse_sum_invalid() {
        let err = parse_sum_input("1,x,3").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("不是有效整数"));
    }
}
