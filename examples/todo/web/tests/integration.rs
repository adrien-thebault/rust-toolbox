//! The end-to-end test: a real gRPC backend on a real socket, an in-process
//! gateway, and a browser-shaped request going all the way through.
//!
//! This is the regression test the macros do not otherwise have.
#![allow(missing_docs, clippy::missing_panics_doc)]

mod roundtrip;
