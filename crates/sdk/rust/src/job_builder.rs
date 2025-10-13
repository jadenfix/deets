use aether_types::H256;
use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::types::{JobRequest, JobSubmission};

pub struct JobBuilder {
    endpoint: String,
    job_id: Option<String>,
    model_hash: Option<H256>,
    input_hash: Option<H256>,
    max_fee: u128,
    expires_at: Option<u64>,
    metadata: Option<Value>,
}

impl JobBuilder {
    pub(crate) fn new(endpoint: &str) -> Self {
        JobBuilder {
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            job_id: None,
            model_hash: None,
            input_hash: None,
            max_fee: 1_000_000,
            expires_at: None,
            metadata: None,
        }
    }

    pub fn job_id(mut self, job_id: impl Into<String>) -> Self {
        let job_id = job_id.into();
        if job_id.trim().is_empty() {
            return self;
        }
        self.job_id = Some(job_id);
        self
    }

    pub fn model_hash(mut self, hash: H256) -> Self {
        self.model_hash = Some(hash);
        self
    }

    pub fn input_hash(mut self, hash: H256) -> Self {
        self.input_hash = Some(hash);
        self
    }

    pub fn max_fee(mut self, fee: u128) -> Self {
        if fee > 0 {
            self.max_fee = fee;
        }
        self
    }

    pub fn expires_at(mut self, ts: u64) -> Self {
        if ts > 0 {
            self.expires_at = Some(ts);
        }
        self
    }

    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn build(&self) -> Result<JobRequest> {
        let job_id = self
            .job_id
            .clone()
            .ok_or_else(|| anyhow!("job_id not set"))?;
        let model_hash = self
            .model_hash
            .ok_or_else(|| anyhow!("model_hash not set"))?;
        let input_hash = self
            .input_hash
            .ok_or_else(|| anyhow!("input_hash not set"))?;
        let expires_at = self
            .expires_at
            .ok_or_else(|| anyhow!("expires_at not set"))?;

        Ok(JobRequest {
            job_id,
            model_hash,
            input_hash,
            max_fee: self.max_fee,
            expires_at,
            metadata: self.metadata.clone(),
        })
    }

    pub fn to_submission(&self) -> Result<JobSubmission> {
        let job = self.build()?;
        Ok(JobSubmission {
            url: format!("{}/v1/jobs", self.endpoint),
            method: "POST".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: job,
        })
    }
}
