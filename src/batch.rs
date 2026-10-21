use crate::domain::{Lecture, conflicts, selected_conflicts};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Implementations must return only a complete, parsed booking snapshot.
/// An authentication page, partial response, or unknown state must be an error.
pub trait Backend {
    fn bookings(&mut self) -> Result<Vec<Lecture>>;
    fn submit(&mut self, lecture: &Lecture) -> SubmitOutcome;
    fn reauthenticate(&mut self) -> Result<()>;
    fn is_auth_error(&self, _error: &anyhow::Error) -> bool {
        false
    }
}
#[derive(Debug, Clone)]
pub enum SubmitOutcome {
    Success,
    Failed(String),
    Uncertain(String),
    CaptchaRejected,
    AuthExpired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Success,
    Failed,
    Skipped,
    Uncertain,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemReport {
    pub id: String,
    pub status: ItemStatus,
    pub reason: String,
}
#[derive(Debug, Clone, Copy)]
pub struct BatchOptions {
    pub buffer_minutes: i64,
    pub captcha_retries: usize,
}
impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            buffer_minutes: 0,
            captcha_retries: 2,
        }
    }
}
fn checked_bookings<B: Backend>(
    backend: &mut B,
    reauthenticated: &mut bool,
) -> Result<Vec<Lecture>> {
    let bookings = match backend.bookings() {
        Err(error) if !*reauthenticated && backend.is_auth_error(&error) => {
            *reauthenticated = true;
            backend
                .reauthenticate()
                .context("reauthentication failed")?;
            backend
                .bookings()
                .context("booking state unavailable after reauthentication")?
        }
        result => result.context("booking state unavailable")?,
    };
    for b in &bookings {
        b.validate().context("invalid booking snapshot")?;
    }
    Ok(bookings)
}
/// Conflicting selections are returned as an error before any submission.
/// Success is accepted only when a subsequent booking snapshot contains the ID.
/// Uncertain results are reconciled, never automatically resubmitted.
pub fn run_batch<B: Backend>(
    backend: &mut B,
    selected: &[Lecture],
    options: BatchOptions,
) -> Result<Vec<ItemReport>> {
    let collisions = selected_conflicts(selected, options.buffer_minutes)?;
    ensure!(
        collisions.is_empty(),
        "selected lectures conflict; resolve selection first: {:?}",
        collisions
    );
    let mut reauthenticated = false;
    let mut bookings = checked_bookings(backend, &mut reauthenticated)?;
    let mut seen = HashSet::new();
    let mut reports = Vec::new();
    let mut state_unknown = false;
    for lecture in selected {
        let mut report = ItemReport {
            id: lecture.id.clone(),
            status: ItemStatus::Skipped,
            reason: String::new(),
        };
        if !seen.insert(lecture.id.clone()) {
            report.reason = "duplicate selected ID".into();
        } else if state_unknown {
            report.reason =
                "booking state unknown after prior submission; refresh before continuing".into();
        } else if !lecture.reservable {
            report.reason = "lecture is not reservable".into();
        } else if bookings.iter().any(|b| b.id == lecture.id) {
            report.reason = "already booked".into();
        } else if bookings
            .iter()
            .map(|b| conflicts(lecture, b, options.buffer_minutes))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .any(|v| v)
        {
            report.reason = "conflicts with existing booking".into();
        } else {
            let mut captcha_attempts = 0;
            loop {
                match backend.submit(lecture) {
                    outcome @ (SubmitOutcome::Success | SubmitOutcome::Uncertain(_)) => {
                        let submission_reason = match outcome {
                            SubmitOutcome::Uncertain(reason) => reason,
                            _ => "server reported success".into(),
                        };
                        match checked_bookings(backend, &mut reauthenticated) {
                            Ok(snapshot) => {
                                bookings = snapshot;
                                if bookings.iter().any(|b| b.id == lecture.id) {
                                    report.status = ItemStatus::Success;
                                    report.reason = "verified in bookings".into();
                                } else {
                                    report.status = ItemStatus::Uncertain;
                                    report.reason = format!(
                                        "{submission_reason}; not present in verification snapshot; no automatic retry"
                                    );
                                    // A delayed server write remains possible despite an absent ID.
                                    state_unknown = true;
                                }
                            }
                            Err(error) => {
                                report.status = ItemStatus::Uncertain;
                                report.reason =
                                    format!("{submission_reason}; verification failed: {error:#}");
                                state_unknown = true;
                            }
                        }
                        break;
                    }
                    SubmitOutcome::Failed(reason) => {
                        report.status = ItemStatus::Failed;
                        report.reason = reason;
                        break;
                    }
                    SubmitOutcome::CaptchaRejected => {
                        if captcha_attempts >= options.captcha_retries {
                            report.status = ItemStatus::Failed;
                            report.reason = "captcha retry limit reached".into();
                            break;
                        }
                        captcha_attempts += 1;
                    }
                    SubmitOutcome::AuthExpired => {
                        if reauthenticated {
                            report.status = ItemStatus::Failed;
                            report.reason =
                                "authentication expired after one reauthentication".into();
                            break;
                        }
                        reauthenticated = true;
                        if let Err(error) = backend.reauthenticate() {
                            report.status = ItemStatus::Failed;
                            report.reason = format!("reauthentication failed: {error:#}");
                            break;
                        }
                        match checked_bookings(backend, &mut reauthenticated) {
                            Ok(snapshot) => {
                                bookings = snapshot;
                                if bookings.iter().any(|b| b.id == lecture.id) {
                                    report.status = ItemStatus::Success;
                                    report.reason = "verified after reauthentication".into();
                                    break;
                                }
                                if bookings
                                    .iter()
                                    .map(|b| conflicts(lecture, b, options.buffer_minutes))
                                    .collect::<Result<Vec<_>>>()?
                                    .into_iter()
                                    .any(|v| v)
                                {
                                    report.reason =
                                        "conflict discovered after reauthentication".into();
                                    break;
                                }
                            }
                            Err(error) => {
                                report.status = ItemStatus::Uncertain;
                                report.reason = format!(
                                    "booking verification after reauthentication failed: {error:#}"
                                );
                                state_unknown = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        reports.push(report);
    }
    Ok(reports)
}
