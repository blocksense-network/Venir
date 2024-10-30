use std::sync::Arc;

use air::messages::Diagnostics;
use rust_verify::{
    buckets::BucketId,
    config::ShowTriggers,
    verifier::{report_chosen_triggers, Verifier, VerifyBucketOut},
};
use vir::{
    ast::{Krate, VirErr},
    context::FuncCallGraphLogFiles,
    messages::{note, Span, ToAny},
};

use crate::stub_structs::Reporter;

// This function is equivalent to Verus' verify_inner_crate.
// We can't use it directly from Verus because the function
// uses the Rust compiler for reporting errors
/// Simplify the VIR crate even more and verify it.
pub fn verify_crate(verifier: &mut Verifier, air_no_span: Option<Span>) -> Result<(), VirErr> {
    let krate = verifier
        .vir_crate
        .clone()
        .expect("vir_crate should be initialized");
    let air_no_span = air_no_span
        .clone()
        .expect("air_no_span should be initialized");

    #[cfg(debug_assertions)] // Maybe always check and not only when in debug mode
    vir::check_ast_flavor::check_krate(&krate);
    let call_graph_log: Option<FuncCallGraphLogFiles> = None; // As a reminder that we can log if needed
    let mut global_ctx = vir::context::GlobalCtx::new(
        &krate,
        Arc::new(verifier.crate_name.clone().expect("crate_name")),
        air_no_span.clone(),
        verifier.args.rlimit,
        Arc::new(std::sync::Mutex::new(None)),
        Arc::new(std::sync::Mutex::new(call_graph_log)),
        verifier.args.solver,
        false,
    )?;
    vir::recursive_types::check_traits(&krate, &global_ctx)?;
    let krate = vir::ast_simplify::simplify_krate(&mut global_ctx, &krate)?;

    #[cfg(debug_assertions)]
    vir::check_ast_flavor::check_krate_simplified(&krate);


    let user_filter = verifier.user_filter.as_ref().unwrap();
    let modules_to_verify: Vec<vir::ast::Module> = {
        let current_crate_module_ids = verifier
            .current_crate_modules
            .as_ref()
            .expect("current_crate_module_ids should be initialized");
        user_filter.filter_modules(current_crate_module_ids)?
    };
    // Buckets are the "groups" which we want to formally verify
    let buckets = rust_verify::buckets::get_buckets(&krate, &modules_to_verify);
    let buckets = user_filter.filter_buckets(buckets);
    let bucket_ids: Vec<BucketId> = buckets.iter().map(|p| p.0.clone()).collect();
    verifier.buckets = buckets.into_iter().collect();


    // Single thread verification. Multi-thread verification of buckets is possible
    global_ctx.set_interpreter_log_file(Arc::new(std::sync::Mutex::new(
        if verifier.args.log_all || verifier.args.log_args.log_interpreter {
            Some(verifier.create_log_file(None, rust_verify::config::INTERPRETER_FILE_SUFFIX)?)
        } else {
            None
        },
    )));

    for bucket_id in &bucket_ids {
        global_ctx = verify_bucket_outer(verifier, &krate, bucket_id, global_ctx)?;
    }

    let stub_reporter = Reporter::new();
    let chosen_triggers = global_ctx.get_chosen_triggers();
    let mut low_confidence_triggers = None;
    for chosen in chosen_triggers {
        match (
            verifier.args.show_triggers,
            modules_to_verify
                .iter()
                .find(|m| &m.x.path == &chosen.module)
                .is_some(),
        ) {
            (ShowTriggers::Selective, true) if chosen.low_confidence => {
                report_chosen_triggers(&stub_reporter, &chosen);
                low_confidence_triggers = Some(chosen.span);
            }
            (ShowTriggers::Module, true) => {
                report_chosen_triggers(&stub_reporter, &chosen);
            }
            (ShowTriggers::Verbose, _) => {
                report_chosen_triggers(&stub_reporter, &chosen);
            }
            _ => {}
        }
    }
    if let Some(span) = low_confidence_triggers {
        let msg = "Verus printed one or more automatically chosen quantifier triggers\n\
                because it had low confidence in the chosen triggers.\n\
                To suppress these messages, do one of the following:\n  \
                (1) manually annotate a single desired trigger using #[trigger]\n      \
                (example: forall|i: int, j: int| f(i) && #[trigger] g(i) && #[trigger] h(j)),\n  \
                (2) manually annotate multiple desired triggers using #![trigger ...]\n      \
                (example: forall|i: int| #![trigger f(i)] #![trigger g(i)] f(i) && g(i)),\n  \
                (3) accept the automatically chosen trigger using #![auto]\n      \
                (example: forall|i: int, j: int| #![auto] f(i) && g(i) && h(j))\n  \
                (4) use the --triggers-silent command-line option to suppress all printing of triggers.\n\
                (Note: triggers are used by the underlying SMT theorem prover to instantiate quantifiers;\n\
                the theorem prover instantiates a quantifier whenever some expression matches the\n\
                pattern specified by one of the quantifier's triggers.)\
                ";
        stub_reporter.report(&note(&span, msg).to_any());
    }
    Ok(())
}

fn verify_bucket_outer(
    verifier: &mut Verifier,
    krate: &Krate,
    bucket_id: &BucketId,
    mut global_ctx: vir::context::GlobalCtx,
) -> Result<vir::context::GlobalCtx, VirErr> {
    verifier
        .bucket_stats
        .insert(bucket_id.clone(), Default::default());

    let bucket_name = bucket_id.friendly_name();
    let user_filter = verifier.user_filter.as_ref().unwrap();
    if verifier.args.trace || !user_filter.is_everything() {
        let functions_msg = if user_filter.is_function_filter() {
            " (selected functions)"
        } else {
            ""
        };
        println!("Verifying {bucket_name}{functions_msg}");
    }
    let (pruned_krate, mono_abstract_datatypes, spec_fn_types, uses_array, fndef_types) =
        vir::prune::prune_krate_for_module_or_krate(
            &krate,
            &Arc::new(verifier.crate_name.clone().expect("crate_name")),
            None,
            Some(bucket_id.module().clone()),
            bucket_id.function(),
            true,
        );
    let mono_abstract_datatypes = mono_abstract_datatypes.unwrap();
    let module = pruned_krate
        .modules
        .iter()
        .find(|m| &m.x.path == bucket_id.module())
        .expect("module in krate")
        .clone();
    let mut ctx = vir::context::Ctx::new(
        &pruned_krate,
        global_ctx,
        module,
        mono_abstract_datatypes,
        spec_fn_types,
        uses_array,
        fndef_types,
        verifier.args.debugger,
    )?;
   
    if verifier.args.log_all || verifier.args.log_args.log_vir_poly {
        let mut file = verifier
            .create_log_file(Some(&bucket_id), rust_verify::config::VIR_POLY_FILE_SUFFIX)?;
        vir::printer::write_krate(
            &mut file,
            &pruned_krate,
            &verifier.args.log_args.vir_log_option,
        );
    }

    let stub_reporter = Reporter::new();

    let krate_sst = vir::ast_to_sst_crate::ast_to_sst_krate(
        &mut ctx,
        &stub_reporter,
        &verifier.get_bucket(bucket_id).funs,
        &pruned_krate,
    )?;
    let krate_sst = vir::poly::poly_krate_for_module(&mut ctx, &krate_sst);

    let VerifyBucketOut {
        time_smt_init,
        time_smt_run,
        rlimit_count,
    } = verifier.verify_bucket(&stub_reporter, &krate_sst, None, bucket_id, &mut ctx)?;

    global_ctx = ctx.free();

    let stats_bucket = verifier
        .bucket_stats
        .get_mut(bucket_id)
        .expect("bucket should exist");
    stats_bucket.time_smt_init = time_smt_init;
    stats_bucket.time_smt_run = time_smt_run;
    stats_bucket.rlimit_count = rlimit_count;

    Ok(global_ctx)
}
