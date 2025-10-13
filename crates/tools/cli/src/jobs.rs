use std::fs;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::Value;

use crate::config::{expand_path, ResolvedConfig};
use crate::io::parse_h256;

#[derive(Subcommand, Debug)]
pub enum JobCommands {
    /// Build and print a job submission envelope
    Post(PostJobCommand),
    /// Walk through the hello-world AI job tutorial
    Tutorial,
}

impl JobCommands {
    pub async fn execute(&self, config: &ResolvedConfig) -> Result<()> {
        match self {
            JobCommands::Post(cmd) => cmd.execute(config).await,
            JobCommands::Tutorial => {
                print_tutorial();
                Ok(())
            }
        }
    }
}

#[derive(Args, Debug)]
pub struct PostJobCommand {
    /// Stable job identifier
    #[arg(long = "job-id")]
    pub job_id: String,

    /// Model hash (H256, 0x-prefixed)
    #[arg(long = "model")]
    pub model_hash: String,

    /// Input hash (H256, 0x-prefixed)
    #[arg(long = "input")]
    pub input_hash: String,

    /// Maximum fee willing to pay (AIC)
    #[arg(long = "max-fee")]
    pub max_fee: u128,

    /// Expiry timestamp (seconds since epoch)
    #[arg(long = "expires-at")]
    pub expires_at: u64,

    /// Inline metadata JSON (string)
    #[arg(long = "metadata")]
    pub metadata: Option<String>,

    /// Metadata file path (JSON)
    #[arg(long = "metadata-file")]
    pub metadata_file: Option<String>,
}

impl PostJobCommand {
    pub async fn execute(&self, config: &ResolvedConfig) -> Result<()> {
        let client = config.client();
        let model_hash = parse_h256(&self.model_hash)?;
        let input_hash = parse_h256(&self.input_hash)?;
        let metadata = self.load_metadata()?;

        let mut builder = client
            .job()
            .job_id(self.job_id.clone())
            .model_hash(model_hash)
            .input_hash(input_hash)
            .max_fee(self.max_fee)
            .expires_at(self.expires_at);

        if let Some(value) = metadata {
            builder = builder.metadata(value);
        }

        let submission = builder.to_submission()?;
        let prepared = client.prepare_job_submission(submission.body.clone());
        let prepared_matches = submission == prepared;
        let aether_sdk::types::JobSubmission {
            url,
            method,
            headers,
            body,
        } = submission;
        let output = JobSubmissionSummary {
            url,
            method,
            headers,
            payload: body,
            prepared_matches,
            endpoint: config.endpoint.clone(),
        };

        println!("{}", serde_json::to_string_pretty(&output)?);
        Ok(())
    }

    fn load_metadata(&self) -> Result<Option<Value>> {
        let inline = match &self.metadata {
            Some(raw) => Some(parse_metadata(raw)?),
            None => None,
        };

        if let Some(file) = &self.metadata_file {
            let path = expand_path(file)?;
            let contents = fs::read_to_string(&path)
                .with_context(|| format!("failed to read metadata file {}", path.display()))?;
            let value = parse_metadata(&contents)?;
            return Ok(Some(value));
        }

        Ok(inline)
    }
}

#[derive(Serialize)]
struct JobSubmissionSummary {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    payload: aether_sdk::types::JobRequest,
    prepared_matches: bool,
    endpoint: String,
}

fn parse_metadata(data: &str) -> Result<Value> {
    serde_json::from_str(data).map_err(|err| anyhow!("invalid metadata JSON: {err}"))
}

fn print_tutorial() {
    println!(
        r#"Aether AI Job Tutorial
-------------------------
1) Generate a signing key:
   aetherctl keys generate --out ~/.aether/keys/tutorial.json

2) Craft the job submission:
   aetherctl job post \
     --job-id hello-aic-job \
     --model 0x{model_hash} \
     --input 0x{input_hash} \
     --max-fee 500000000 \
     --expires-at {expiry} \
     --metadata '{{"prompt":"Generate a haiku about verifiable compute."}}'

3) Submit the payload to the coordinator endpoint:
   curl -X POST $AETHER_ENDPOINT/v1/jobs \
     -H 'content-type: application/json' \
     -d @job.json

4) Track status via explorer or:
   curl $AETHER_ENDPOINT/v1/jobs/hello-aic-job
"#,
        model_hash = "12".repeat(32),
        input_hash = "ab".repeat(32),
        expiry = 1_700_000_000
    );
}
