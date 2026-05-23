#![allow(clippy::cargo_common_metadata, clippy::multiple_crate_versions)]

#[test]
fn command_step_schema_is_stable() {
    let schema = schemars::schema_for!(hm_pipeline_ir::CommandStep);
    insta::assert_json_snapshot!(schema);
}
