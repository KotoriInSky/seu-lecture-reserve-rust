use seu_lecture_reserve::batch::*;
use seu_lecture_reserve::domain::*;
use std::collections::VecDeque;

#[derive(Default)]
struct ScriptedBackend {
    bookings: Vec<Lecture>,
    outcomes: VecDeque<SubmitOutcome>,
    submitted: Vec<String>,
    auth_count: usize,
    query_count: usize,
    fail_query_at: Option<usize>,
    commit_uncertain: bool,
    auth_query_at: Vec<usize>,
    omit_success_booking: bool,
}
impl Backend for ScriptedBackend {
    fn bookings(&mut self) -> anyhow::Result<Vec<Lecture>> {
        self.query_count += 1;
        if self.auth_query_at.contains(&self.query_count) {
            return Err(anyhow::anyhow!("session expired"));
        }
        anyhow::ensure!(
            self.fail_query_at != Some(self.query_count),
            "booking query unavailable"
        );
        Ok(self.bookings.clone())
    }
    fn submit(&mut self, lecture: &Lecture) -> SubmitOutcome {
        self.submitted.push(lecture.id.clone());
        let outcome = self.outcomes.pop_front().unwrap_or(SubmitOutcome::Success);
        if (matches!(outcome, SubmitOutcome::Success) && !self.omit_success_booking)
            || (matches!(outcome, SubmitOutcome::Uncertain(_)) && self.commit_uncertain)
        {
            self.bookings.push(lecture.clone());
        }
        outcome
    }
    fn reauthenticate(&mut self) -> anyhow::Result<()> {
        self.auth_count += 1;
        Ok(())
    }
    fn is_auth_error(&self, error: &anyhow::Error) -> bool {
        error.to_string() == "session expired"
    }
}

#[test]
fn initial_booking_auth_expiry_recovers_and_skips_existing_id() {
    let mut b = ScriptedBackend {
        auth_query_at: vec![1],
        bookings: vec![sample("a", 10)],
        ..Default::default()
    };
    let reports = run_batch(
        &mut b,
        &[sample("a", 10), sample("b", 12)],
        BatchOptions::default(),
    )
    .unwrap();
    assert_eq!(reports[0].status, ItemStatus::Skipped);
    assert_eq!(reports[1].status, ItemStatus::Success);
    assert_eq!(b.submitted, vec!["b"]);
    assert_eq!(b.auth_count, 1);
}
#[test]
fn verification_auth_expiry_recovers_without_resubmission() {
    let mut b = ScriptedBackend {
        auth_query_at: vec![2],
        ..Default::default()
    };
    let reports = run_batch(
        &mut b,
        &[sample("a", 10), sample("b", 12)],
        BatchOptions::default(),
    )
    .unwrap();
    assert!(reports.iter().all(|r| r.status == ItemStatus::Success));
    assert_eq!(b.submitted, vec!["a", "b"]);
    assert_eq!(b.auth_count, 1);
}
#[test]
fn initial_read_and_submit_share_single_reauthentication_budget() {
    let mut b = ScriptedBackend {
        auth_query_at: vec![1],
        outcomes: VecDeque::from([SubmitOutcome::AuthExpired]),
        ..Default::default()
    };
    let reports = run_batch(&mut b, &[sample("a", 10)], BatchOptions::default()).unwrap();
    assert_eq!(reports[0].status, ItemStatus::Failed);
    assert_eq!(b.auth_count, 1);
    assert_eq!(b.submitted.len(), 1);
}
#[test]
fn repeated_booking_auth_loss_is_bounded_and_fails_closed() {
    let mut b = ScriptedBackend {
        auth_query_at: vec![1, 3],
        ..Default::default()
    };
    let reports = run_batch(
        &mut b,
        &[sample("a", 10), sample("b", 12)],
        BatchOptions::default(),
    )
    .unwrap();
    assert_eq!(reports[0].status, ItemStatus::Uncertain);
    assert_eq!(reports[1].status, ItemStatus::Skipped);
    assert_eq!(b.auth_count, 1);
    assert_eq!(b.submitted.len(), 1);
}
#[test]
fn server_success_without_booking_remains_uncertain() {
    let mut b = ScriptedBackend {
        omit_success_booking: true,
        ..Default::default()
    };
    let reports = run_batch(
        &mut b,
        &[sample("a", 10), sample("b", 12)],
        BatchOptions::default(),
    )
    .unwrap();
    assert_eq!(reports[0].status, ItemStatus::Uncertain);
    assert_eq!(reports[1].status, ItemStatus::Skipped);
    assert_eq!(b.submitted.len(), 1);
}

fn lecture(id: &str, start: &str, end: &str) -> Lecture {
    Lecture {
        id: id.into(),
        title: id.into(),
        category: "science".into(),
        campus: "main".into(),
        location: "room".into(),
        start: start.parse().unwrap(),
        end: end.parse().unwrap(),
        online: false,
        reservable: true,
    }
}
fn sample(id: &str, hour: u8) -> Lecture {
    lecture(
        id,
        &format!("2026-10-15T{hour:02}:00:00+08:00"),
        &format!("2026-10-15T{:02}:00:00+08:00", hour + 1),
    )
}
#[test]
fn boundaries_buffer_and_invalid_times() {
    let a = sample("a", 10);
    let b = sample("b", 11);
    assert!(!conflicts(&a, &b, 0).unwrap());
    assert!(conflicts(&a, &b, 1).unwrap());
    assert!(conflicts(&a, &b, -1).is_err());
    let mut invalid = a.clone();
    invalid.end = invalid.start;
    assert!(conflicts(&invalid, &b, 0).is_err());
}
#[test]
fn named_title_bypasses_categories_and_campus_but_not_availability_or_conflicts() {
    let a = sample("a", 10);
    let p = Preferences {
        titles: vec!["a".into()],
        categories: vec!["other".into()],
        campuses: vec!["other".into()],
        ..Preferences::default()
    };
    assert_eq!(
        filter_lectures(std::slice::from_ref(&a), &p, &[], 0)
            .unwrap()
            .len(),
        1
    );
    assert!(
        filter_lectures(std::slice::from_ref(&a), &p, &[sample("existing", 10)], 0)
            .unwrap()
            .is_empty()
    );
    let mut unavailable = a;
    unavailable.reservable = false;
    assert!(
        filter_lectures(&[unavailable], &p, &[], 0)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn online_and_offline_have_identical_conflict_rules() {
    let mut a = sample("a", 10);
    a.online = true;
    let b = sample("b", 10);
    assert_eq!(
        selected_conflicts(&[a.clone(), b.clone()], 0).unwrap(),
        vec![("a".into(), "b".into())]
    );
    assert!(
        filter_lectures(&[a], &Preferences::default(), &[b], 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn both_format_toggles_can_disable_everything() {
    let p = Preferences {
        enable_online: false,
        enable_offline: false,
        ..Default::default()
    };
    assert!(
        filter_lectures(&[sample("a", 10)], &p, &[], 0)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn batch_continues_after_failure_and_skips_duplicate_ids() {
    let mut b = ScriptedBackend {
        outcomes: VecDeque::from([SubmitOutcome::Failed("full".into()), SubmitOutcome::Success]),
        ..Default::default()
    };
    let reports = run_batch(
        &mut b,
        &[sample("a", 10), sample("b", 12), sample("b", 12)],
        BatchOptions::default(),
    )
    .unwrap();
    assert_eq!(
        reports.iter().map(|r| r.status).collect::<Vec<_>>(),
        vec![ItemStatus::Failed, ItemStatus::Success, ItemStatus::Skipped]
    );
    assert_eq!(b.submitted, vec!["a", "b"]);
    assert_eq!(b.query_count, 2, "success must be verified");
}
#[test]
fn conflicts_and_unknown_initial_bookings_prevent_submission() {
    let mut b = ScriptedBackend::default();
    assert!(
        run_batch(
            &mut b,
            &[sample("a", 10), sample("b", 10)],
            BatchOptions::default()
        )
        .is_err()
    );
    assert!(b.submitted.is_empty());
    b.fail_query_at = Some(1);
    assert!(run_batch(&mut b, &[sample("a", 10)], BatchOptions::default()).is_err());
    assert!(b.submitted.is_empty());
}
#[test]
fn timeout_committed_on_server_is_reconciled_without_duplicate() {
    let mut b = ScriptedBackend {
        outcomes: VecDeque::from([SubmitOutcome::Uncertain("timeout".into())]),
        commit_uncertain: true,
        ..Default::default()
    };
    let reports = run_batch(&mut b, &[sample("a", 10)], BatchOptions::default()).unwrap();
    assert_eq!(reports[0].status, ItemStatus::Success);
    assert_eq!(b.submitted.len(), 1);
}
#[test]
fn unknown_reconciliation_prevents_retry_and_further_writes() {
    let mut b = ScriptedBackend {
        outcomes: VecDeque::from([SubmitOutcome::Uncertain("timeout".into())]),
        fail_query_at: Some(2),
        ..Default::default()
    };
    let reports = run_batch(
        &mut b,
        &[sample("a", 10), sample("b", 12)],
        BatchOptions::default(),
    )
    .unwrap();
    assert_eq!(reports[0].status, ItemStatus::Uncertain);
    assert_eq!(reports[1].status, ItemStatus::Skipped);
    assert_eq!(b.submitted.len(), 1);
}
#[test]
fn captcha_and_auth_retries_are_bounded() {
    let mut b = ScriptedBackend {
        outcomes: VecDeque::from([
            SubmitOutcome::CaptchaRejected,
            SubmitOutcome::CaptchaRejected,
            SubmitOutcome::AuthExpired,
            SubmitOutcome::Success,
            SubmitOutcome::AuthExpired,
        ]),
        ..Default::default()
    };
    let options = BatchOptions {
        captcha_retries: 1,
        ..Default::default()
    };
    let reports = run_batch(
        &mut b,
        &[sample("a", 10), sample("b", 12), sample("c", 14)],
        options,
    )
    .unwrap();
    assert_eq!(
        reports.iter().map(|r| r.status).collect::<Vec<_>>(),
        vec![ItemStatus::Failed, ItemStatus::Success, ItemStatus::Failed]
    );
    assert_eq!(b.auth_count, 1);
    assert_eq!(b.submitted.len(), 5);
}
#[test]
fn existing_booking_conflicts_are_skipped_even_online() {
    let mut online = sample("online", 10);
    online.online = true;
    let mut b = ScriptedBackend {
        bookings: vec![online],
        ..Default::default()
    };
    let reports = run_batch(&mut b, &[sample("offline", 10)], BatchOptions::default()).unwrap();
    assert_eq!(reports[0].status, ItemStatus::Skipped);
    assert!(b.submitted.is_empty());
}
