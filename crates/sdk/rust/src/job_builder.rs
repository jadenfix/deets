use aether_types::H256;
use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::types::{JobRequest, JobSubmission};

/// Builder for constructing AI job submissions.
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

    /// Set the unique job identifier. Returns an error if the ID is empty.
    pub fn job_id(mut self, job_id: impl Into<String>) -> Result<Self> {
        let job_id = job_id.into();
        if job_id.trim().is_empty() {
            return Err(anyhow!("job_id must not be empty"));
        }
        self.job_id = Some(job_id);
        Ok(self)
    }

    /// Set the hash of the model to execute.
    pub fn model_hash(mut self, hash: H256) -> Self {
        self.model_hash = Some(hash);
        self
    }

    /// Set the hash of the input data.
    pub fn input_hash(mut self, hash: H256) -> Self {
        self.input_hash = Some(hash);
        self
    }

    /// Set the maximum fee willing to pay for execution.
    pub fn max_fee(mut self, fee: u128) -> Self {
        if fee > 0 {
            self.max_fee = fee;
        }
        self
    }

    /// Set the expiration timestamp (unix seconds).
    pub fn expires_at(mut self, ts: u64) -> Self {
        if ts > 0 {
            self.expires_at = Some(ts);
        }
        self
    }

    /// Attach arbitrary JSON metadata to the job.
    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Validate and build the job request.
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

    /// Build the job and wrap it in a ready-to-send submission payload.
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
