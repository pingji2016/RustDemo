use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Health {
    pub status: &'static str,
}

#[derive(Deserialize)]
pub struct SumQuery {
    pub nums: String,
}

#[derive(Deserialize, Serialize)]
pub struct EchoBody {
    pub message: String,
}

#[derive(Deserialize)]
pub struct ParallelQuery {
    pub n: Option<usize>,
}

#[derive(Serialize)]
pub struct ParallelResult {
    pub index: usize,
    pub value: usize,
    pub ms: u64,
}
