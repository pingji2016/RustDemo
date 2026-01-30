// 演示：基于 Axum 的高并发 HTTP 服务 + 强类型 JSON + 并发任务
// 端点包括：根路径、健康检查、求和（查询参数）、回显（POST JSON）、并发计算示例
use std::{net::SocketAddr, time::Duration};

// Axum：路由、提取器、响应类型
use axum::{
    extract::Query,
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

    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/sum", get(sum))
        .route("/echo", post(echo))
        .route("/parallel", get(parallel));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// 根路径：返回 200 OK 与文本 "ok"
async fn root() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

// 健康检查：返回 JSON {"status":"healthy"}
async fn health() -> impl IntoResponse {
    Json(Health { status: "healthy" })
}

// 求和接口：?nums=1,2,3
// 1) 将逗号分隔的字符串拆分
// 2) 逐项解析为 i64（解析失败按 0 处理）
// 3) 计算总和并以 JSON 返回
async fn sum(Query(q): Query<SumQuery>) -> impl IntoResponse {
    let nums: Vec<i64> = q
        .nums
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().parse::<i64>().unwrap_or(0))
        .collect();
    let total: i64 = nums.iter().sum();
    Json(serde_json::json!({ "total": total }))
}

// 回显接口：POST JSON，原样返回
async fn echo(Json(body): Json<EchoBody>) -> impl IntoResponse {
    Json(body)
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
async fn parallel(Query(q): Query<ParallelQuery>) -> impl IntoResponse {
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
