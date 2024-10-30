use air::messages::{AirMessage, AirMessageLabel, ArcDynMessage, MessageLevel};
use vir::messages::MessageX;

pub struct Reporter {}

impl air::messages::Diagnostics for Reporter {
    fn report_as(&self, msg: &ArcDynMessage, level: MessageLevel) {
        let mut msg_note = String::new();
        if let Some(air_msg) = msg.downcast_ref::<AirMessage>() {
            msg_note = air_msg.note.clone();
        } else if let Some(msgx) = msg.downcast_ref::<MessageX>() {
            msg_note = msgx.note.clone();
        }
        use MessageLevel::*;
        match level {
            Note => println!("Note: {}", msg_note),
            Warning => println!("Warning: {}", msg_note),
            Error => eprintln!("Error: {}", msg_note),
        }
    }

    fn report(&self, msg: &ArcDynMessage) {
        if let Some(air_msg) = msg.downcast_ref::<AirMessage>() {
            self.report_as(msg, air_msg.level);
        } else if let Some(air_label_msg) = msg.downcast_ref::<AirMessageLabel>() {
            println!("AirMessageLabel {}", air_label_msg.note);
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
