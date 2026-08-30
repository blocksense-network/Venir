use air::messages::{AirMessage, AirMessageLabel, ArcDynMessage, MessageLevel};
use air::model::Counterexample;
use regex::Regex;
use serde::Serialize;
use vir::messages::MessageX;

fn extract_span_from_string(input: &str) -> String {
    let pattern = r"^\(\d+, \d+, \d+\)";
    let re = Regex::new(pattern).unwrap();

    if let Some(matched) = re.find(input) {
        matched.as_str().to_string()
    } else {
        String::new()
    }
}

pub struct Reporter {}

#[derive(Serialize)]
struct ErrorBlock {
    error_message: String,
    error_span: String,
    secondary_message: String,
}

#[derive(Serialize)]
struct WarningBlock {
    warning_message: String,
}

#[derive(Serialize)]
struct CrashBlock {
    crash_message: String,
    crash_span: String,
}

/// The values the solver committed to for the query that is about to be
/// reported as an error.
///
/// Emitted **immediately before** the `Error` it belongs to: `air` reports the
/// counterexample from inside `smt_get_model`, while the error itself is returned
/// up to `rust_verify` and reported afterwards. A consumer therefore attaches a
/// `Counterexample` line to the next `Error` line, and may cross-check
/// `assert_id` when the query carried one.
#[derive(Serialize)]
struct CounterexampleBlock {
    model: Counterexample,
}

#[derive(Serialize)]
enum SmtOutput {
    Error(ErrorBlock),
    Warning(WarningBlock),
    Note(String),
    AirMessage(CrashBlock),
    Counterexample(CounterexampleBlock),
}

fn emit(out: &SmtOutput) {
    eprintln!("{}", serde_json::to_string(out).unwrap());
}

/// The line this reporter writes for a crash-level AIR message.
fn render_crash(note: &str) -> SmtOutput {
    SmtOutput::AirMessage(CrashBlock {
        crash_message: note.to_string(),
        crash_span: extract_span_from_string(note),
    })
}

/// The line this reporter writes for a VIR message at `level`.
///
/// Kept pure and separate from the `Diagnostics` impl so that the mapping from
/// message to wire shape can be tested without a solver, a verifier or a rustc.
fn render_vir(msgx: &MessageX, level: MessageLevel) -> SmtOutput {
    use MessageLevel::*;
    match level {
        Note => {
            // A counterexample arrives as an ordinary note carrying a known
            // prefix, because `ArcDynMessage` is `Arc<dyn Any>` and every
            // reporter in the chain downcasts it to the one type it knows -- a
            // second message type would panic Verus's own reporter on its way to
            // rustc rather than reach this one. The prefix is the whole
            // discriminator: a note that merely *looks* like a counterexample
            // stays a note.
            match air::model::counterexample_from_note(&msgx.note) {
                Some(model) => SmtOutput::Counterexample(CounterexampleBlock { model }),
                None => SmtOutput::Note(msgx.note.clone()),
            }
        }
        Warning => SmtOutput::Warning(WarningBlock { warning_message: msgx.note.clone() }),
        Error => {
            let mut error_block: ErrorBlock = ErrorBlock {
                error_message: msgx.note.clone(),
                error_span: String::new(),
                secondary_message: String::new(),
            };
            if let Some(span) = msgx.spans.last() {
                error_block.error_span = extract_span_from_string(&span.as_string);
            }
            if let Some(label) = msgx.labels.last() {
                // If a label exists maybe we should report two errors
                // instead of one. Currently we are overwriting the span.
                error_block.secondary_message = label.note.clone();
                error_block.error_span = extract_span_from_string(&label.span.as_string);
            }
            SmtOutput::Error(error_block)
        }
    }
}

impl air::messages::Diagnostics for Reporter {
    fn report_as(&self, msg: &ArcDynMessage, level: MessageLevel) {
        if let Some(air_msg) = msg.downcast_ref::<AirMessage>() {
            emit(&render_crash(&air_msg.note));
        } else if let Some(msgx) = msg.downcast_ref::<MessageX>() {
            emit(&render_vir(msgx, level));
        }
    }

    fn report(&self, msg: &ArcDynMessage) {
        if let Some(air_msg) = msg.downcast_ref::<AirMessage>() {
            self.report_as(msg, air_msg.level);
        } else if let Some(air_label_msg) = msg.downcast_ref::<AirMessageLabel>() {
            emit(&render_crash(&air_label_msg.note));
        } else if let Some(msgx) = msg.downcast_ref::<MessageX>() {
            self.report_as(msg, msgx.level);
        }
    }

    fn report_now(&self, msg: &ArcDynMessage) {
        self.report(msg);
    }

    fn report_as_now(&self, msg: &ArcDynMessage, msg_as: MessageLevel) {
        self.report_as(msg, msg_as)
    }
}

impl rust_verify::verifier::Diagnostics for Reporter {
    fn use_progress_bars(&self) -> bool {
        false
    }

    fn add_progress_bar(&self, _ctx: vir::def::CommandContext) {}

    fn complete_progress_bar(&self, _ctx: vir::def::CommandContext) {}
}

impl Reporter {
    pub fn new() -> Reporter {
        Reporter {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use air::model::{Counterexample, ModelBinding, ModelSnapshot};

    fn json(out: &SmtOutput) -> serde_json::Value {
        serde_json::from_str(&serde_json::to_string(out).unwrap()).unwrap()
    }

    fn note(text: &str) -> MessageX {
        MessageX {
            level: MessageLevel::Note,
            note: text.to_string(),
            spans: Vec::new(),
            labels: Vec::new(),
            help: None,
            fancy_note: None,
        }
    }

    fn a_model() -> Counterexample {
        Counterexample {
            parameters: vec![ModelBinding {
                variable: "n".into(),
                constant: "n".into(),
                value: "42".into(),
                typ: "Int".into(),
            }],
            snapshots: vec![ModelSnapshot {
                snapshot_id: "after_a".into(),
                bindings: vec![ModelBinding {
                    variable: "a".into(),
                    constant: "a@1".into(),
                    value: "43".into(),
                    typ: "Int".into(),
                }],
            }],
            assert_id: Some(vec![7]),
        }
    }

    /// V1: a counterexample note becomes a `Counterexample` line carrying the
    /// values, not a `Note` line carrying JSON as text.
    #[test]
    fn v1_a_counterexample_note_is_emitted_as_a_counterexample() {
        let mut asserts = 0;
        let model = a_model();
        let out = json(&render_vir(&note(&air::model::counterexample_note(&model)), MessageLevel::Note));

        asserts += 1;
        assert!(out.get("Counterexample").is_some(), "expected a Counterexample line, got {out}");
        asserts += 1;
        assert!(out.get("Note").is_none(), "must not also be a Note");

        let m = &out["Counterexample"]["model"];
        asserts += 1;
        assert_eq!(m["parameters"][0]["variable"], "n");
        asserts += 1;
        assert_eq!(m["parameters"][0]["value"], "42");
        asserts += 1;
        assert_eq!(m["snapshots"][0]["snapshot_id"], "after_a");
        asserts += 1;
        assert_eq!(m["snapshots"][0]["bindings"][0]["variable"], "a");
        asserts += 1;
        assert_eq!(m["snapshots"][0]["bindings"][0]["value"], "43");
        asserts += 1;
        assert_eq!(m["snapshots"][0]["bindings"][0]["constant"], "a@1");
        asserts += 1;
        assert_eq!(m["assert_id"][0], 7);
        assert_eq!(asserts, 9, "V1 assertion count");
    }

    /// V1-control: an ordinary note stays a note. Without this arm V1 would pass
    /// over a reporter that called everything a counterexample.
    #[test]
    fn v2_control_an_ordinary_note_stays_a_note() {
        let mut asserts = 0;
        let out = json(&render_vir(&note("automatically chosen trigger"), MessageLevel::Note));
        asserts += 1;
        assert_eq!(out["Note"], "automatically chosen trigger");
        asserts += 1;
        assert!(out.get("Counterexample").is_none());
        assert_eq!(asserts, 2, "V2 assertion count");
    }

    /// A note that is *shaped* like a counterexample but was not announced as one
    /// stays a note. Anything else would let a JSON note from elsewhere in the
    /// verifier be read as solver evidence.
    #[test]
    fn v3_an_unannounced_json_note_is_not_a_counterexample() {
        let mut asserts = 0;
        let bare = serde_json::to_string(&a_model()).unwrap();
        let out = json(&render_vir(&note(&bare), MessageLevel::Note));
        asserts += 1;
        assert!(out.get("Counterexample").is_none(), "bare JSON must not decode as a model");
        asserts += 1;
        assert_eq!(out["Note"], bare);
        assert_eq!(asserts, 2, "V3 assertion count");
    }

    /// The shapes Verno already parses are unchanged: adding a variant must not
    /// move `Error`, `Warning` or `Note`.
    #[test]
    fn v4_the_existing_wire_shapes_are_unchanged() {
        let mut asserts = 0;

        let mut err = note("assertion failed");
        err.level = MessageLevel::Error;
        err.spans = vec![vir::messages::Span {
            raw_span: std::sync::Arc::new(()),
            id: 0,
            data: vec![],
            as_string: "(10, 20, 3) something".to_string(),
        }];
        let out = json(&render_vir(&err, MessageLevel::Error));
        asserts += 1;
        assert_eq!(out["Error"]["error_message"], "assertion failed");
        asserts += 1;
        assert_eq!(out["Error"]["error_span"], "(10, 20, 3)");
        asserts += 1;
        assert_eq!(out["Error"]["secondary_message"], "");

        let out = json(&render_vir(&note("careful"), MessageLevel::Warning));
        asserts += 1;
        assert_eq!(out["Warning"]["warning_message"], "careful");

        let out = json(&render_crash("(1, 2, 0) Verification crashed"));
        asserts += 1;
        assert_eq!(out["AirMessage"]["crash_message"], "(1, 2, 0) Verification crashed");
        asserts += 1;
        assert_eq!(out["AirMessage"]["crash_span"], "(1, 2, 0)");

        assert_eq!(asserts, 6, "V4 assertion count");
    }
}
