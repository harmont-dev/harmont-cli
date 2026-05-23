#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use hm_pipeline_ir::Pipeline;
use schemars::schema_for;

#[test]
fn pipeline_schema_is_stable() {
    let schema = schema_for!(Pipeline);
    insta::assert_json_snapshot!("pipeline", schema);
}
