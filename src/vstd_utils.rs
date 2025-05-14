use rust_verify::import_export::ImportOutput;
use rust_verify::verifier::Verifier;
use vir::messages::MessageLevel;

/// Gets the Verus standard library as a Verus VIR krate
pub fn get_imported_krates(verifier: &Verifier) -> ImportOutput {
    match rust_verify::import_export::import_crates(
        &verifier.args,
        verifier.import_virs_via_cargo.clone().unwrap_or_default(),
    ) {
        Ok(imported) => imported,
        Err(err) => {
            assert!(err.spans.len() == 0);
            assert!(err.level == MessageLevel::Error);
            // compiler.sess.dcx().err(err.note.clone()); //Error emitting
            panic!("{}", err.note);
            // verifier.encountered_vir_error = true;
        }
    }
}
