//! Reading one credential through `op`: started the ordinary way first, and under
//! [`OpResponsibility::InheritedUnlessRefused`] read again through an `op` that answers for itself
//! when 1Password refuses the first (decision 027, addendum of 2026-11-03).
use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use domain::credential_failure::{CredentialCause, CredentialFailure};
use domain::known_secrets::KnownSecrets;
use domain::op_spawn::{OpSpawn, OpSpawnsUsed};
use domain::screen_at_deadline::ScreenAtDeadline;
use secrecy::Secret;

use crate::op_process::read_reference;
use crate::op_responsibility::OpResponsibility;
use crate::screen_look::look_at_screen_and_app;

/// How long one `op read` may take before it is ended.
///
/// Longer than [`OP_PROMPT_LIMIT`], so it never cuts short a prompt `op` is still showing: past it,
/// `op` is not waiting on anyone, it has gone silent.
const OP_DEADLINE: Duration = Duration::from_secs(90);

/// How long `op` waits on an approval prompt nobody answers before giving up with "authorization
/// timeout": 60 seconds, measured three times with `op` 2.40.0 on 2026-10-17.
const OP_PROMPT_LIMIT: Duration = Duration::from_secs(60);

const _: () = assert!(
    OP_DEADLINE.as_secs() > OP_PROMPT_LIMIT.as_secs(),
    "the deadline would end prompts op is still showing"
);

/// How much longer a read may run when 1Password's Touch ID prompt is on screen at the deadline.
/// Longer than [`OP_PROMPT_LIMIT`], so `op` gives up on the prompt by itself first.
const TOUCH_ID_GRACE: Duration = Duration::from_secs(65);

const _: () = assert!(
    TOUCH_ID_GRACE.as_secs() > OP_PROMPT_LIMIT.as_secs(),
    "the extra wait would end a Touch ID prompt op is still showing"
);

/// The credentials one call to [`crate::ConfigFile::resolve`] has resolved so far, which a later
/// failure in the same call must not repeat, how that call may start `op`, and which ways
/// answered.
#[derive(Debug)]
pub(crate) struct ResolvedSoFar {
    responsibility: OpResponsibility,
    values: Vec<String>,
    pub(crate) spawns: OpSpawnsUsed,
}

impl ResolvedSoFar {
    pub(crate) fn new(responsibility: OpResponsibility) -> Self {
        Self {
            responsibility,
            values: Vec::new(),
            spawns: OpSpawnsUsed::default(),
        }
    }

    /// Resolves `value`, remembering it, or fails redacted of everything remembered before it.
    pub(crate) async fn read(&mut self, value: &str) -> Result<Secret<String>, CredentialFailure> {
        self.read_with(
            value,
            |spawn| read_reference(value, spawn),
            look_at_screen_and_app,
        )
        .await
    }

    /// Resolves `value` as [`Self::read`] does, asking `read` for an `op://` reference's value
    /// through an `op` started each way, and `look` what is on screen at a deadline.
    ///
    /// The reference is first read through an `op` started the ordinary way. Under
    /// [`OpResponsibility::InheritedUnlessRefused`], a refusal with "No accounts configured" reads
    /// it once more through an `op` that answers for itself. Anything else the first read gives
    /// stands, so a dialog waiting on hub is never joined by one waiting on `op`.
    async fn read_with<Read, Answer, Look, Seen>(
        &mut self,
        value: &str,
        mut read: Read,
        mut look: Look,
    ) -> Result<Secret<String>, CredentialFailure>
    where
        Read: FnMut(OpSpawn) -> Answer,
        Answer: Future<Output = Result<String>>,
        Look: FnMut() -> Seen,
        Seen: Future<Output = ScreenAtDeadline>,
    {
        if !is_reference(value) {
            self.values.push(value.to_string());
            return Ok(Secret::new(value.to_string()));
        }
        let ordinary = self
            .answered_within(
                value,
                OP_DEADLINE,
                OpSpawn::Inherited,
                read(OpSpawn::Inherited),
                &mut look,
            )
            .await;
        match ordinary {
            Err(refusal)
                if refusal.cause() == CredentialCause::NoAccounts
                    && self.responsibility == OpResponsibility::InheritedUnlessRefused =>
            {
                self.read_disclaimed_after(&refusal, value, read(OpSpawn::Disclaimed), look)
                    .await
            }
            answered => answered,
        }
    }

    /// Reads `reference` through an `op` that answers for itself, after `refusal` from the
    /// ordinary one.
    async fn read_disclaimed_after<Look, Seen>(
        &mut self,
        refusal: &CredentialFailure,
        reference: &str,
        read: impl Future<Output = Result<String>>,
        look: Look,
    ) -> Result<Secret<String>, CredentialFailure>
    where
        Look: FnMut() -> Seen,
        Seen: Future<Output = ScreenAtDeadline>,
    {
        assert_eq!(
            refusal.cause(),
            CredentialCause::NoAccounts,
            "a disclaimed read followed something other than a refusal"
        );
        assert_eq!(
            refusal.spawn(),
            OpSpawn::Inherited,
            "a disclaimed read followed a disclaimed one"
        );
        self.answered_within(reference, OP_DEADLINE, OpSpawn::Disclaimed, read, look)
            .await
    }

    /// What `read`, from an `op` started as `spawn`, gave for `reference`, or when it had not
    /// finished by `deadline`, a [`CredentialFailure::no_answer`] naming what `look` then found.
    /// When `look` finds 1Password's Touch ID prompt, `read` gets [`TOUCH_ID_GRACE`] longer
    /// instead, since `op` gives up on that prompt by itself. Dropping `read` ends the `op` it
    /// started.
    async fn answered_within<Look, Seen>(
        &mut self,
        reference: &str,
        deadline: Duration,
        spawn: OpSpawn,
        read: impl Future<Output = Result<String>>,
        mut look: Look,
    ) -> Result<Secret<String>, CredentialFailure>
    where
        Look: FnMut() -> Seen,
        Seen: Future<Output = ScreenAtDeadline>,
    {
        assert!(
            spawn == OpSpawn::Inherited
                || self.responsibility == OpResponsibility::InheritedUnlessRefused,
            "an op was disclaimed under {:?}, which never disclaims",
            self.responsibility
        );
        let mut read = std::pin::pin!(read);
        let mut waited = deadline;
        let mut answered = tokio::time::timeout(deadline, &mut read).await;
        if answered.is_err() {
            let seen = look().await;
            if seen != ScreenAtDeadline::TouchIdPrompt {
                return Err(self.no_answer(reference, waited, &seen, spawn));
            }
            // op gives up on a Touch ID prompt by itself within a minute, so let it.
            waited += TOUCH_ID_GRACE;
            answered = tokio::time::timeout(TOUCH_ID_GRACE, &mut read).await;
        }
        match answered {
            Ok(Ok(resolved)) => {
                self.values.push(resolved.clone());
                self.spawns.add(spawn);
                Ok(Secret::new(resolved))
            }
            Ok(Err(error)) => Err(self.failed(&error, spawn)),
            Err(_elapsed) => Err(self.no_answer(reference, waited, &look().await, spawn)),
        }
    }

    fn no_answer(
        &self,
        reference: &str,
        waited: Duration,
        seen: &ScreenAtDeadline,
        spawn: OpSpawn,
    ) -> CredentialFailure {
        CredentialFailure::no_answer(reference, waited, seen, &self.known_secrets(), spawn)
    }

    fn failed(&self, error: &anyhow::Error, spawn: OpSpawn) -> CredentialFailure {
        CredentialFailure::new(error, &self.known_secrets(), spawn)
    }

    fn known_secrets(&self) -> KnownSecrets {
        KnownSecrets::new(self.values.clone())
    }
}

/// Whether `value` is an `op://` reference for 1Password to resolve, rather than a plain value.
fn is_reference(value: &str) -> bool {
    value.starts_with("op://")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use secrecy::ExposeSecret;

    use super::*;

    #[tokio::test]
    async fn a_failure_never_repeats_a_credential_resolved_before_it() {
        let mut so_far = ResolvedSoFar::new(OpResponsibility::Inherited);
        let _ = so_far.read("tok-4f9a").await.unwrap();

        let failure = so_far.failed(
            &anyhow::anyhow!("op echoed tok-4f9a back"),
            OpSpawn::Inherited,
        );

        assert!(
            !failure.reason().as_str().contains("tok-4f9a"),
            "{}",
            failure.reason()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_unfinished_at_the_deadline_names_who_the_screen_says_was_not_answering() {
        let mut so_far = ResolvedSoFar::new(OpResponsibility::Inherited);
        let _ = so_far.read("tok-4f9a").await.unwrap();

        let failure = so_far
            .answered_within(
                "op://Scripts/GitHub/token",
                OP_DEADLINE,
                OpSpawn::Inherited,
                std::future::pending::<Result<String>>(),
                || async { ScreenAtDeadline::MacosAlert { windows: 1 } },
            )
            .await
            .unwrap_err();

        assert_eq!(failure.cause(), CredentialCause::MacosPromptWaitingForYou);
        assert!(
            failure.reason().as_str().starts_with(
                "1Password did not answer within 90s reading op://Scripts/GitHub/token; "
            ),
            "{}",
            failure.reason()
        );
    }

    #[tokio::test]
    async fn a_read_that_finishes_never_looks_at_the_screen() {
        let mut so_far = ResolvedSoFar::new(OpResponsibility::Inherited);

        let read = so_far
            .answered_within(
                "op://x/y/z",
                OP_DEADLINE,
                OpSpawn::Inherited,
                async { Ok("value".to_string()) },
                || async { panic!("looked at the screen for a read that answered") },
            )
            .await;

        assert!(read.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn a_touch_id_prompt_on_screen_at_the_deadline_lets_the_read_finish() {
        let mut so_far = ResolvedSoFar::new(OpResponsibility::Inherited);
        let late = async {
            tokio::time::sleep(OP_DEADLINE + Duration::from_secs(30)).await;
            Ok("value".to_string())
        };

        let read = so_far
            .answered_within(
                "op://x/y/z",
                OP_DEADLINE,
                OpSpawn::Inherited,
                late,
                || async { ScreenAtDeadline::TouchIdPrompt },
            )
            .await;

        assert!(read.is_ok(), "{read:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_touch_id_prompt_still_up_after_the_extra_wait_ends_the_read() {
        let mut so_far = ResolvedSoFar::new(OpResponsibility::Inherited);

        let failure = so_far
            .answered_within(
                "op://x/y/z",
                OP_DEADLINE,
                OpSpawn::Inherited,
                std::future::pending::<Result<String>>(),
                || async { ScreenAtDeadline::TouchIdPrompt },
            )
            .await
            .unwrap_err();

        assert_eq!(failure.cause(), CredentialCause::TouchIdPromptUnanswered);
        assert_eq!(
            failure.reason().as_str(),
            "1Password did not answer within 155s reading op://x/y/z; 1Password's Touch ID prompt \
             is on screen"
        );
    }

    #[rstest]
    #[case::plain("plain-value")]
    #[case::empty("")]
    #[tokio::test]
    async fn a_plain_value_is_its_own_credential_and_starts_no_op(#[case] value: &str) {
        // A plain value never reaches a spawn, so either responsibility will do.
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);

        let read = so_far
            .read_with(
                value,
                |spawn| async move { panic!("started op {spawn:?} for a plain value") },
                || async { panic!("looked at the screen for a plain value") },
            )
            .await
            .unwrap();

        assert_eq!(read.expose_secret(), value);
        assert_eq!(so_far.spawns.to_string(), "none");
    }

    /// What hub makes of `op read` exiting non-zero with `stderr`.
    fn op_said(stderr: &str) -> anyhow::Error {
        anyhow::anyhow!("failed to resolve credential from 1Password: {stderr}")
    }

    const NO_ACCOUNTS: &str = "[ERROR] 2026/11/01 12:00:00 No accounts configured for use with \
                               1Password CLI. - Turn on the 1Password desktop app integration";

    /// Every spawn a read asked for, in order.
    type Spawned = std::rc::Rc<std::cell::RefCell<Vec<OpSpawn>>>;

    /// A `read` for [`ResolvedSoFar::read_with`] that answers with `inherited` or `disclaimed`
    /// depending on the spawn, and remembers each spawn it was asked for in `spawned`.
    fn op_answering(
        spawned: &Spawned,
        inherited: fn() -> Result<String>,
        disclaimed: fn() -> Result<String>,
    ) -> impl FnMut(OpSpawn) -> std::future::Ready<Result<String>> {
        let spawned = spawned.clone();
        move |spawn| {
            spawned.borrow_mut().push(spawn);
            std::future::ready(match spawn {
                OpSpawn::Inherited => inherited(),
                OpSpawn::Disclaimed => disclaimed(),
            })
        }
    }

    fn never() -> Result<String> {
        panic!("a disclaimed op was started")
    }

    async fn nothing_on_screen() -> ScreenAtDeadline {
        ScreenAtDeadline::AppRunningNoAlert
    }

    #[tokio::test]
    async fn the_daemon_uses_an_ordinary_answer_and_never_disclaims() {
        let spawned = Spawned::default();
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);

        let read = so_far
            .read_with(
                "op://x/y/z",
                op_answering(&spawned, || Ok("value".into()), never),
                nothing_on_screen,
            )
            .await
            .unwrap();

        assert_eq!(read.expose_secret(), "value");
        assert_eq!(*spawned.borrow(), vec![OpSpawn::Inherited]);
        assert_eq!(so_far.spawns.to_string(), "inherited");
    }

    #[tokio::test]
    async fn the_daemon_reads_a_refused_credential_again_through_a_disclaimed_op() {
        let spawned = Spawned::default();
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);

        let read = so_far
            .read_with(
                "op://x/y/z",
                op_answering(
                    &spawned,
                    || Err(op_said(NO_ACCOUNTS)),
                    || Ok("value".into()),
                ),
                nothing_on_screen,
            )
            .await
            .unwrap();

        assert_eq!(read.expose_secret(), "value");
        assert_eq!(
            *spawned.borrow(),
            vec![OpSpawn::Inherited, OpSpawn::Disclaimed]
        );
        assert_eq!(so_far.spawns.to_string(), "disclaimed");
    }

    #[tokio::test]
    async fn a_disclaimed_read_that_fails_after_a_refusal_reports_its_own_cause() {
        let spawned = Spawned::default();
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);

        let failure = so_far
            .read_with(
                "op://x/y/z",
                op_answering(
                    &spawned,
                    || Err(op_said(NO_ACCOUNTS)),
                    || {
                        Err(op_said(
                            "error initializing client: 1Password CLI couldn't connect to the \
                             1Password desktop app.",
                        ))
                    },
                ),
                nothing_on_screen,
            )
            .await
            .unwrap_err();

        assert_eq!(failure.cause(), CredentialCause::AppNotRunning);
        assert_eq!(failure.spawn(), OpSpawn::Disclaimed);
    }

    #[rstest]
    #[case::app_not_running(
        "error initializing client: 1Password CLI couldn't connect to the 1Password desktop app."
    )]
    #[case::touch_id_unanswered("error initializing client: authorization timeout")]
    #[case::unreachable("dial tcp: lookup my.1password.com: no such host")]
    #[case::anything_else("isn't a vault in this account")]
    #[tokio::test]
    async fn the_daemon_keeps_any_other_ordinary_failure(#[case] stderr: &'static str) {
        let mut spawned = Vec::new();
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);
        let read = |spawn| {
            spawned.push(spawn);
            std::future::ready(Err::<String, _>(op_said(stderr)))
        };

        let failure = so_far
            .read_with("op://x/y/z", read, nothing_on_screen)
            .await
            .unwrap_err();

        assert_ne!(failure.cause(), CredentialCause::NoAccounts);
        assert_eq!(failure.spawn(), OpSpawn::Inherited);
        assert_eq!(spawned, vec![OpSpawn::Inherited]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_silent_at_the_deadline_with_a_macos_alert_up_is_not_read_again_disclaimed() {
        let mut spawned = Vec::new();
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);
        let read = |spawn| {
            spawned.push(spawn);
            std::future::pending::<Result<String>>()
        };

        let failure = so_far
            .read_with("op://x/y/z", read, || async {
                ScreenAtDeadline::MacosAlert { windows: 1 }
            })
            .await
            .unwrap_err();

        assert_eq!(failure.cause(), CredentialCause::MacosPromptWaitingForYou);
        assert_eq!(failure.spawn(), OpSpawn::Inherited);
        assert_eq!(spawned, vec![OpSpawn::Inherited]);
    }

    #[rstest]
    #[case::macos_alert(
        ScreenAtDeadline::MacosAlert { windows: 1 },
        CredentialCause::MacosPromptWaitingForYou
    )]
    #[case::nothing_on_screen(
        ScreenAtDeadline::AppRunningNoAlert,
        CredentialCause::AppRunningNotAnswering
    )]
    #[tokio::test(start_paused = true)]
    async fn a_disclaimed_read_silent_at_its_own_deadline_is_named_by_the_screen(
        #[case] screen: ScreenAtDeadline,
        #[case] cause: CredentialCause,
    ) {
        let mut spawned = Vec::new();
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);
        let read = |spawn| {
            spawned.push(spawn);
            async move {
                match spawn {
                    OpSpawn::Inherited => Err(op_said(NO_ACCOUNTS)),
                    OpSpawn::Disclaimed => std::future::pending().await,
                }
            }
        };

        let failure = so_far
            .read_with("op://x/y/z", read, move || {
                let screen = screen.clone();
                async move { screen }
            })
            .await
            .unwrap_err();

        assert_eq!(failure.cause(), cause);
        assert_eq!(failure.spawn(), OpSpawn::Disclaimed);
        assert!(
            failure.reason().as_str().contains("within 90s"),
            "{}",
            failure.reason()
        );
        assert_eq!(spawned, vec![OpSpawn::Inherited, OpSpawn::Disclaimed]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_disclaimed_read_with_the_touch_id_prompt_up_at_its_deadline_gets_the_grace() {
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);
        let read = |spawn| async move {
            match spawn {
                OpSpawn::Inherited => Err(op_said(NO_ACCOUNTS)),
                OpSpawn::Disclaimed => {
                    tokio::time::sleep(OP_DEADLINE + Duration::from_secs(10)).await;
                    Ok("value".to_string())
                }
            }
        };

        let read = so_far
            .read_with("op://x/y/z", read, || async {
                ScreenAtDeadline::TouchIdPrompt
            })
            .await
            .unwrap();

        assert_eq!(read.expose_secret(), "value");
        assert_eq!(so_far.spawns.to_string(), "disclaimed");
    }

    #[tokio::test]
    async fn the_tui_never_disclaims_a_refused_credential() {
        let spawned = Spawned::default();
        let mut so_far = ResolvedSoFar::new(OpResponsibility::Inherited);

        let failure = so_far
            .read_with(
                "op://x/y/z",
                op_answering(&spawned, || Err(op_said(NO_ACCOUNTS)), never),
                nothing_on_screen,
            )
            .await
            .unwrap_err();

        assert_eq!(failure.cause(), CredentialCause::NoAccounts);
        assert_eq!(failure.spawn(), OpSpawn::Inherited);
        assert_eq!(*spawned.borrow(), vec![OpSpawn::Inherited]);
    }

    #[tokio::test]
    async fn a_refusal_of_a_later_credential_never_reads_an_earlier_one_again() {
        let first = Spawned::default();
        let second = Spawned::default();
        let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);

        let _ = so_far
            .read_with(
                "op://x/first/z",
                op_answering(&first, || Ok("one".into()), never),
                nothing_on_screen,
            )
            .await
            .unwrap();
        let _ = so_far
            .read_with(
                "op://x/second/z",
                op_answering(&second, || Err(op_said(NO_ACCOUNTS)), || Ok("two".into())),
                nothing_on_screen,
            )
            .await
            .unwrap();

        assert_eq!(*first.borrow(), vec![OpSpawn::Inherited]);
        assert_eq!(
            *second.borrow(),
            vec![OpSpawn::Inherited, OpSpawn::Disclaimed]
        );
        assert_eq!(so_far.spawns.to_string(), "inherited,disclaimed");
    }

    /// How a credential read earlier in the attempt was resolved.
    #[derive(Debug, Clone, Copy)]
    enum EarlierRead {
        Plain,
        Ordinary,
        DisclaimedAfterRefusal,
    }

    proptest::proptest! {
        #[test]
        fn no_failure_from_either_read_holds_a_credential_resolved_before_it(
            secret in "[A-Za-z0-9]{12,24}",
            earlier in proptest::prop_oneof![
                proptest::strategy::Just(EarlierRead::Plain),
                proptest::strategy::Just(EarlierRead::Ordinary),
                proptest::strategy::Just(EarlierRead::DisclaimedAfterRefusal),
            ],
        ) {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();
            let failure = runtime.block_on(async {
                let mut so_far = ResolvedSoFar::new(OpResponsibility::InheritedUnlessRefused);
                let answer = secret.clone();
                let _ = match earlier {
                    EarlierRead::Plain => {
                        so_far
                            .read_with(&secret, |_| async { unreachable!() }, nothing_on_screen)
                            .await
                    }
                    EarlierRead::Ordinary => {
                        so_far
                            .read_with(
                                "op://x/earlier/z",
                                move |_| std::future::ready(Ok(answer.clone())),
                                nothing_on_screen,
                            )
                            .await
                    }
                    EarlierRead::DisclaimedAfterRefusal => {
                        so_far
                            .read_with(
                                "op://x/earlier/z",
                                move |spawn| {
                                    std::future::ready(match spawn {
                                        OpSpawn::Inherited => Err(op_said(NO_ACCOUNTS)),
                                        OpSpawn::Disclaimed => Ok(answer.clone()),
                                    })
                                },
                                nothing_on_screen,
                            )
                            .await
                    }
                }
                .unwrap();
                let echoed = secret.clone();
                so_far
                    .read_with(
                        "op://x/y/z",
                        move |spawn| {
                            let said = match spawn {
                                OpSpawn::Inherited => format!("{NO_ACCOUNTS} {echoed}"),
                                OpSpawn::Disclaimed => format!("isn't a vault: {echoed}"),
                            };
                            std::future::ready(Err::<String, _>(op_said(&said)))
                        },
                        nothing_on_screen,
                    )
                    .await
                    .unwrap_err()
            });

            proptest::prop_assert_eq!(failure.spawn(), OpSpawn::Disclaimed);
            proptest::prop_assert!(!failure.reason().as_str().contains(&secret));
        }
    }
}
