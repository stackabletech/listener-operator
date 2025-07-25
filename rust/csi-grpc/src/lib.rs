//! Include gRPC definition files that have been generated by `build.rs`

pub static FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

// The prost-generated code breaks many clippy lints. Just disable them since there's nothing we can do about it.
#[allow(clippy::all)]
pub mod v1 {
    tonic::include_proto!("csi.v1");
}
