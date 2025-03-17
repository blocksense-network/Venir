use air::messages::{AirMessage, AirMessageLabel, ArcDynMessage, MessageLevel};
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

#[derive(Serialize)]
enum SmtOutput {
    Error(ErrorBlock),
    Warning(WarningBlock),
    Note(String),
    AirMessage(CrashBlock),
}

impl air::messages::Diagnostics for Reporter {
    fn report_as(&self, msg: &ArcDynMessage, level: MessageLevel) {
        if let Some(air_msg) = msg.downcast_ref::<AirMessage>() {
            eprintln!(
                "{}",
                serde_json::to_string(&SmtOutput::AirMessage(CrashBlock {
                    crash_message: air_msg.note.clone(),
                    crash_span: extract_span_from_string(&air_msg.note)
                }))
                .unwrap()
            );
            return;
        } else if let Some(msgx) = msg.downcast_ref::<MessageX>() {
            use MessageLevel::*;
            match level {
                Note => eprintln!(
                    "{}",
                    serde_json::to_string(&SmtOutput::Note(msgx.note.clone())).unwrap()
                ),
                Warning => eprintln!(
                    "{}",
                    serde_json::to_string(&SmtOutput::Warning(WarningBlock {
                        warning_message: msgx.note.clone()
                    }))
                    .unwrap()
                ),
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
                    eprintln!(
                        "{}",
                        serde_json::to_string(&SmtOutput::Error(error_block)).unwrap()
                    )
                }
            }
        }
    }

    fn report(&self, msg: &ArcDynMessage) {
        if let Some(air_msg) = msg.downcast_ref::<AirMessage>() {
            self.report_as(msg, air_msg.level);
        } else if let Some(air_label_msg) = msg.downcast_ref::<AirMessageLabel>() {
            eprintln!(
                "{}",
                serde_json::to_string(&SmtOutput::AirMessage(CrashBlock {
                    crash_message: air_label_msg.note.clone(),
                    crash_span: extract_span_from_string(&air_label_msg.note)
                })).unwrap()
            );
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
