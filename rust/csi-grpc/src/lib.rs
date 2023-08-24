//! Include gRPC definition files that have been generated by `build.rs`

pub static FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

// Trivial warnings that come from prost-generated code
#[allow(clippy::derive_partial_eq_without_eq)]
pub mod v1 {
    tonic::include_proto!("csi.v1");
}

pub mod listop {
    pub mod v1 {
        tonic::include_proto!("listop.v1");
    }
}
