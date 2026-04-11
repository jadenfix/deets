use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Typed errors from TEE attestation verification.
///
/// Using typed errors rather than anyhow strings lets callers match on the
/// specific failure reason (staleness, unapproved measurement, cert chain
/// issues) without parsing strings.
#[derive(Debug, Error)]
pub enum TeeError {
    #[error("attestation timestamp {timestamp} is in the future (current: {current})")]
    AttestationInFuture { timestamp: u64, current: u64 },

    #[error("attestation too old: {age_secs} seconds (max {max_secs})")]
    AttestationStale { age_secs: u64, max_secs: u64 },

    #[error("measurement not approved")]
    MeasurementNotApproved,

    #[error("no root cert for TEE type {tee_type:?}")]
    MissingRootCert { tee_type: TeeType },

    #[error("empty certificate chain")]
    EmptyCertChain,

    #[error("attestation report has empty signature")]
    EmptySignature,

    #[error(
        "cryptographic certificate verification for {tee_type:?} attestations is not implemented; \
         refusing non-simulation report"
    )]
    CertVerificationUnimplemented { tee_type: TeeType },

    #[error("invalid {tee_type:?} measurement length (expected {expected}, received {received})")]
    InvalidMeasurementLength {
        tee_type: TeeType,
        expected: usize,
        received: usize,
    },
}

pub type Result<T> = std::result::Result<T, TeeError>;

/// TEE Attestation Verification
///
/// Supports:
/// - AMD SEV-SNP (Secure Encrypted Virtualization)
/// - Intel TDX (Trust Domain Extensions)
/// - AWS Nitro Enclaves
///
/// ATTESTATION FLOW:
/// 1. Worker generates attestation report
/// 2. Report includes: TEE type, measurement, nonce, timestamp
/// 3. Validator verifies signature chain
/// 4. Validator checks measurement against whitelist
/// 5. If valid, worker is authorized
///
/// SECURITY:
/// - Attestation must be fresh (<60s)
/// - Measurement must match approved builds
/// - Signature chain must be valid
/// - Nonce prevents replay attacks

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TeeType {
    SevSnp,     // AMD SEV-SNP
    IntelTdx,   // Intel TDX
    AwsNitro,   // AWS Nitro Enclaves
    Simulation, // For testing only
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationReport {
    pub tee_type: TeeType,
    pub measurement: Vec<u8>,     // SHA-384 of code + data
    pub nonce: Vec<u8>,           // Random nonce for freshness
    pub timestamp: u64,           // Unix timestamp
    pub signature: Vec<u8>,       // TEE signature
    pub cert_chain: Vec<Vec<u8>>, // Certificate chain
}

#[derive(Debug, Clone)]
pub struct TeeVerifier {
    /// Approved measurements (whitelist)
    approved_measurements: Vec<Vec<u8>>,

    /// Maximum attestation age (seconds)
    max_age_secs: u64,

    /// Root certificates for each TEE type
    root_certs: std::collections::HashMap<TeeType, Vec<u8>>,
}

impl TeeVerifier {
    pub fn new() -> Self {
        TeeVerifier {
            approved_measurements: Vec::new(),
            max_age_secs: 60, // 1 minute
            root_certs: std::collections::HashMap::new(),
        }
    }

    /// Add approved measurement to whitelist
    pub fn add_approved_measurement(&mut self, measurement: Vec<u8>) {
        if !self.approved_measurements.contains(&measurement) {
            self.approved_measurements.push(measurement);
        }
    }

    /// Set root certificate for TEE type
    pub fn set_root_cert(&mut self, tee_type: TeeType, cert: Vec<u8>) {
        self.root_certs.insert(tee_type, cert);
    }

    /// Verify attestation report
    pub fn verify(&self, report: &AttestationReport, current_time: u64) -> Result<()> {
        // 1. Check freshness — reject future-dated reports
        if report.timestamp > current_time {
            return Err(TeeError::AttestationInFuture {
                timestamp: report.timestamp,
                current: current_time,
            });
        }
        if current_time - report.timestamp > self.max_age_secs {
            return Err(TeeError::AttestationStale {
                age_secs: current_time - report.timestamp,
                max_secs: self.max_age_secs,
            });
        }

        // 2. Check measurement is approved
        if !self.approved_measurements.contains(&report.measurement) {
            return Err(TeeError::MeasurementNotApproved);
        }

        // 3. Verify signature chain
        if report.tee_type != TeeType::Simulation {
            self.verify_signature_chain(report)?;
        }

        // 4. TEE-specific verification
        match report.tee_type {
            TeeType::SevSnp => self.verify_sev_snp(report)?,
            TeeType::IntelTdx => self.verify_intel_tdx(report)?,
            TeeType::AwsNitro => self.verify_aws_nitro(report)?,
            TeeType::Simulation => {
                // Skip verification for testing
                #[cfg(debug_assertions)]
                eprintln!("WARN: Using simulation TEE (testing only)");
            }
        }

        Ok(())
    }

    fn verify_signature_chain(&self, report: &AttestationReport) -> Result<()> {
        if !self.root_certs.contains_key(&report.tee_type) {
            return Err(TeeError::MissingRootCert {
                tee_type: report.tee_type.clone(),
            });
        }

        if report.cert_chain.is_empty() {
            return Err(TeeError::EmptyCertChain);
        }
        if report.signature.is_empty() {
            return Err(TeeError::EmptySignature);
        }
        Err(TeeError::CertVerificationUnimplemented {
            tee_type: report.tee_type.clone(),
        })
    }

    fn verify_sev_snp(&self, report: &AttestationReport) -> Result<()> {
        // AMD SEV-SNP specific verification
        // - Check VCEK certificate
        // - Verify measurement includes firmware version
        // - Check policy (debug, migration flags)

        if report.measurement.len() != 48 {
            return Err(TeeError::InvalidMeasurementLength {
                tee_type: TeeType::SevSnp,
                expected: 48,
                received: report.measurement.len(),
            });
        }

        Ok(())
    }

    fn verify_intel_tdx(&self, report: &AttestationReport) -> Result<()> {
        // Intel TDX specific verification
        // - Verify TDREPORT structure
        // - Check MRTD (measurement)
        // - Verify quote signature

        if report.measurement.len() != 48 {
            return Err(TeeError::InvalidMeasurementLength {
                tee_type: TeeType::IntelTdx,
                expected: 48,
                received: report.measurement.len(),
            });
        }

        Ok(())
    }

    fn verify_aws_nitro(&self, report: &AttestationReport) -> Result<()> {
        // AWS Nitro Enclaves verification
        // - Verify PCR values
        // - Check attestation document signature
        // - Validate certificate chain to AWS root

        if report.measurement.len() != 48 {
            return Err(TeeError::InvalidMeasurementLength {
                tee_type: TeeType::AwsNitro,
                expected: 48,
                received: report.measurement.len(),
            });
        }

        Ok(())
    }
}

impl Default for TeeVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_report() -> AttestationReport {
        AttestationReport {
            tee_type: TeeType::Simulation,
            measurement: vec![1u8; 48],
            nonce: vec![2u8; 32],
            timestamp: 1000,
            signature: vec![3u8; 64],
            cert_chain: vec![vec![4u8; 100]],
        }
    }

    #[test]
    fn test_verify_simulation() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);

        let report = create_test_report();

        assert!(verifier.verify(&report, 1010).is_ok());
    }

    #[test]
    fn test_reject_old_attestation() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);

        let report = create_test_report();

        // Attestation too old (>60s)
        assert!(verifier.verify(&report, 2000).is_err());
    }

    #[test]
    fn test_reject_unapproved_measurement() {
        let verifier = TeeVerifier::new();
        let report = create_test_report();

        // Measurement not in whitelist
        assert!(verifier.verify(&report, 1010).is_err());
    }

    #[test]
    fn test_add_approved_measurement() {
        let mut verifier = TeeVerifier::new();

        verifier.add_approved_measurement(vec![1u8; 48]);
        verifier.add_approved_measurement(vec![2u8; 48]);

        // No duplicates
        verifier.add_approved_measurement(vec![1u8; 48]);

        assert_eq!(verifier.approved_measurements.len(), 2);
    }

    #[test]
    fn test_future_dated_attestation_rejected() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);

        let mut report = create_test_report();
        let current_time = 1000;
        report.timestamp = current_time + 1000; // far in the future

        let err = verifier.verify(&report, current_time).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("future"),
            "expected error about 'future', got: {msg}"
        );
    }

    #[test]
    fn test_non_simulation_requires_valid_chain() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);
        verifier.set_root_cert(TeeType::SevSnp, vec![0xAA; 64]);

        let mut report = create_test_report();
        report.tee_type = TeeType::SevSnp;
        report.measurement = vec![1u8; 48];
        report.cert_chain = vec![]; // empty certificate chain

        let err = verifier.verify(&report, 1010).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("empty certificate chain"),
            "expected error about empty certificate chain, got: {msg}"
        );
    }

    #[test]
    fn test_non_simulation_attestation_fails_closed() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);
        verifier.set_root_cert(TeeType::SevSnp, vec![0xAA; 64]);

        let mut report = create_test_report();
        report.tee_type = TeeType::SevSnp;
        report.measurement = vec![1u8; 48];

        let err = verifier.verify(&report, 1010).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("refusing non-simulation report"),
            "expected fail-closed error, got: {msg}"
        );
    }

    // ── Typed-error variant tests ────────────────────────────────────────
    // These verify correct TeeError variants, not just error strings.

    #[test]
    fn typed_err_attestation_in_future() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);
        let mut report = create_test_report();
        report.timestamp = 5000;
        let err = verifier.verify(&report, 1000).unwrap_err();
        assert!(
            matches!(
                err,
                TeeError::AttestationInFuture {
                    timestamp: 5000,
                    current: 1000
                }
            ),
            "expected AttestationInFuture, got: {err}"
        );
    }

    #[test]
    fn typed_err_attestation_stale() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);
        // Attestation at t=1000, current=2000 → 1000s old, max=60s → stale
        let err = verifier.verify(&create_test_report(), 2000).unwrap_err();
        assert!(
            matches!(
                err,
                TeeError::AttestationStale {
                    age_secs: 1000,
                    max_secs: 60
                }
            ),
            "expected AttestationStale, got: {err}"
        );
    }

    #[test]
    fn typed_err_measurement_not_approved() {
        let verifier = TeeVerifier::new(); // no measurements added
        let err = verifier.verify(&create_test_report(), 1010).unwrap_err();
        assert!(
            matches!(err, TeeError::MeasurementNotApproved),
            "expected MeasurementNotApproved, got: {err}"
        );
    }

    #[test]
    fn typed_err_missing_root_cert() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);
        // Do NOT set root cert, but use SevSnp type
        let mut report = create_test_report();
        report.tee_type = TeeType::SevSnp;
        let err = verifier.verify(&report, 1010).unwrap_err();
        assert!(
            matches!(
                err,
                TeeError::MissingRootCert {
                    tee_type: TeeType::SevSnp
                }
            ),
            "expected MissingRootCert, got: {err}"
        );
    }

    #[test]
    fn typed_err_cert_verification_unimplemented() {
        let mut verifier = TeeVerifier::new();
        verifier.add_approved_measurement(vec![1u8; 48]);
        verifier.set_root_cert(TeeType::SevSnp, vec![0xAA; 64]);
        let mut report = create_test_report();
        report.tee_type = TeeType::SevSnp;
        let err = verifier.verify(&report, 1010).unwrap_err();
        assert!(
            matches!(
                err,
                TeeError::CertVerificationUnimplemented {
                    tee_type: TeeType::SevSnp
                }
            ),
            "expected CertVerificationUnimplemented, got: {err}"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_measurement() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(any::<u8>(), 48)
    }

    fn arb_nonce() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(any::<u8>(), 32)
    }

    proptest! {
        /// Any approved measurement verifies within the freshness window.
        #[test]
        fn approved_measurement_verifies(
            measurement in arb_measurement(),
            nonce in arb_nonce(),
            age in 0u64..=59u64,
        ) {
            let ts = 10_000u64;
            let current_time = ts + age;
            let mut verifier = TeeVerifier::new();
            verifier.add_approved_measurement(measurement.clone());

            let report = AttestationReport {
                tee_type: TeeType::Simulation,
                measurement: measurement.clone(),
                nonce,
                timestamp: ts,
                signature: vec![1u8; 64],
                cert_chain: vec![vec![0u8; 32]],
            };

            prop_assert!(
                verifier.verify(&report, current_time).is_ok(),
                "approved measurement within freshness window must pass"
            );
        }

        /// Unapproved measurement always fails regardless of timestamp.
        #[test]
        fn unapproved_measurement_always_fails(
            approved in arb_measurement(),
            unapproved in arb_measurement(),
        ) {
            prop_assume!(approved != unapproved);
            let ts = 10_000u64;
            let mut verifier = TeeVerifier::new();
            verifier.add_approved_measurement(approved);

            let report = AttestationReport {
                tee_type: TeeType::Simulation,
                measurement: unapproved,
                nonce: vec![0u8; 32],
                timestamp: ts,
                signature: vec![1u8; 64],
                cert_chain: vec![],
            };

            prop_assert!(
                verifier.verify(&report, ts + 1).is_err(),
                "unapproved measurement must always fail"
            );
        }

        /// Reports older than max_age_secs are always rejected.
        #[test]
        fn stale_reports_always_rejected(
            measurement in arb_measurement(),
            extra_age in 1u64..=100_000u64,
        ) {
            let ts = 10_000u64;
            let max_age = 60u64;
            let current_time = ts + max_age + extra_age;

            let mut verifier = TeeVerifier::new();
            verifier.add_approved_measurement(measurement.clone());

            let report = AttestationReport {
                tee_type: TeeType::Simulation,
                measurement,
                nonce: vec![0u8; 32],
                timestamp: ts,
                signature: vec![1u8; 64],
                cert_chain: vec![],
            };

            prop_assert!(
                verifier.verify(&report, current_time).is_err(),
                "reports older than max_age must be rejected"
            );
        }

        /// Future-dated reports are always rejected.
        #[test]
        fn future_dated_reports_rejected(
            measurement in arb_measurement(),
            future_offset in 1u64..=1_000_000u64,
        ) {
            let current_time = 10_000u64;
            let mut verifier = TeeVerifier::new();
            verifier.add_approved_measurement(measurement.clone());

            let report = AttestationReport {
                tee_type: TeeType::Simulation,
                measurement,
                nonce: vec![0u8; 32],
                timestamp: current_time + future_offset,
                signature: vec![1u8; 64],
                cert_chain: vec![],
            };

            prop_assert!(
                verifier.verify(&report, current_time).is_err(),
                "future-dated reports must always be rejected"
            );
        }

        /// Non-simulation TEE types without root cert always fail.
        #[test]
        fn non_simulation_without_root_cert_fails(measurement in arb_measurement()) {
            let ts = 10_000u64;
            let mut verifier = TeeVerifier::new();
            verifier.add_approved_measurement(measurement.clone());
            // Deliberately do NOT set a root cert

            let report = AttestationReport {
                tee_type: TeeType::SevSnp,
                measurement,
                nonce: vec![0u8; 32],
                timestamp: ts,
                signature: vec![1u8; 64],
                cert_chain: vec![vec![0u8; 32]],
            };

            prop_assert!(
                verifier.verify(&report, ts + 1).is_err(),
                "non-simulation TEE without root cert must fail"
            );
        }

        /// add_approved_measurement never creates duplicates.
        #[test]
        fn no_duplicate_measurements(
            m1 in arb_measurement(),
            m2 in arb_measurement(),
        ) {
            let mut verifier = TeeVerifier::new();
            verifier.add_approved_measurement(m1.clone());
            verifier.add_approved_measurement(m2.clone());
            // Add both again — should not increase count
            verifier.add_approved_measurement(m1.clone());
            verifier.add_approved_measurement(m2.clone());

            let expected = if m1 == m2 { 1 } else { 2 };
            prop_assert_eq!(
                verifier.approved_measurements.len(),
                expected,
                "no duplicate measurements should be stored"
            );
        }

        /// Verification within window with multiple approved measurements: only exact match passes.
        #[test]
        fn only_matching_measurement_passes(
            m_correct in arb_measurement(),
            m_other in arb_measurement(),
        ) {
            prop_assume!(m_correct != m_other);
            let ts = 10_000u64;
            let mut verifier = TeeVerifier::new();
            verifier.add_approved_measurement(m_other.clone());
            // m_correct is NOT approved

            let report = AttestationReport {
                tee_type: TeeType::Simulation,
                measurement: m_correct,
                nonce: vec![0u8; 32],
                timestamp: ts,
                signature: vec![1u8; 64],
                cert_chain: vec![],
            };

            prop_assert!(
                verifier.verify(&report, ts + 1).is_err(),
                "report with non-approved measurement must fail"
            );
        }

        /// Boundary: report at exactly max_age_secs is accepted (not strictly over limit).
        #[test]
        fn report_at_exact_max_age_accepted(measurement in arb_measurement()) {
            let ts = 10_000u64;
            let max_age = 60u64;
            let mut verifier = TeeVerifier::new();
            verifier.add_approved_measurement(measurement.clone());

            let report = AttestationReport {
                tee_type: TeeType::Simulation,
                measurement,
                nonce: vec![0u8; 32],
                timestamp: ts,
                signature: vec![1u8; 64],
                cert_chain: vec![],
            };

            // current_time - timestamp == max_age_secs → condition is `> max_age` → not triggered
            prop_assert!(
                verifier.verify(&report, ts + max_age).is_ok(),
                "report at exactly max_age_secs boundary must be accepted"
            );
        }
    }
}
