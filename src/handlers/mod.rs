use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;
use tokio::{task::JoinSet, time::sleep};

use crate::error::AppError;
use crate::models::api::{EchoBody, Health, ParallelQuery, ParallelResult, SumQuery};
use crate::models::app_state::{AppState, MetricsResponse};

pub async fn root(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    app.hits_root.fetch_add(1, Ordering::Relaxed);
    "ok"
}

pub async fn health(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    app.hits_health.fetch_add(1, Ordering::Relaxed);
    Json(Health { status: "healthy" })
}

pub async fn sum(
    State(app): State<Arc<AppState>>,
    Query(q): Query<SumQuery>,
) -> Result<impl IntoResponse, AppError> {
    app.hits_sum.fetch_add(1, Ordering::Relaxed);
    let total = parse_sum_input(&q.nums)?;
    Ok(Json(serde_json::json!({ "total": total })))
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

pub async fn echo(
    State(app): State<Arc<AppState>>,
    Json(body): Json<EchoBody>,
) -> Result<impl IntoResponse, AppError> {
    app.hits_echo.fetch_add(1, Ordering::Relaxed);
    if body.message.trim().is_empty() {
        return Err(AppError::BadRequest("message 不能为空".into()));
    }
    Ok(Json(body))
}

pub async fn parallel(
    State(app): State<Arc<AppState>>,
    Query(q): Query<ParallelQuery>,
) -> Result<impl IntoResponse, AppError> {
    app.hits_parallel.fetch_add(1, Ordering::Relaxed);
    let n = q.n.unwrap_or(5).min(32);
    let mut tasks = JoinSet::new();
    
    // 优化：预分配容量避免频繁扩容
    let mut results = Vec::with_capacity(n);

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

    while let Some(res) = tasks.join_next().await {
        match res {
            Ok(val) => results.push(val),
            Err(e) => return Err(AppError::Internal(format!("Task failed: {}", e))),
        }
    }
    
    // 保持结果顺序一致（按 index 排序）可能更符合直觉，但这里演示并发收集的乱序特性，故不排序
    // results.sort_by_key(|r| r.index);
    
    Ok(Json(results))
}

pub async fn metrics(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    let response = MetricsResponse::from(app.as_ref());
    Json(response)
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
