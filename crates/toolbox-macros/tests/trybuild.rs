//! For a derive macro the error messages *are* the product, so every misuse
//! has a case here with a committed `.stderr`.
//!
//! Regenerate after an intentional message change with:
//! `TRYBUILD=overwrite cargo test -p toolbox-macros`.

#[test]
fn every_misuse_produces_a_useful_error() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}

#[test]
fn a_correct_entity_compiles() {
    trybuild::TestCases::new().pass("tests/pass/*.rs");
}
