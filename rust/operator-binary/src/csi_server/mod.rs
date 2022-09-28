use serde::Deserialize;

pub mod controller;
pub mod identity;
pub mod node;

#[derive(Deserialize)]
enum ListenerSelector {
    #[serde(rename = "listeners.stackable.tech/listener-class")]
    ListenerClass(String),
    #[serde(rename = "listeners.stackable.tech/listener-name")]
    Listener(String),
}

#[derive(Deserialize)]
struct ListenerVolumeContext {
    #[serde(flatten)]
    listener_selector: ListenerSelector,
}

fn tonic_unimplemented<T>() -> Result<T, tonic::Status> {
    Err(tonic::Status::unimplemented(
        "this endpoint is not implemented",
    ))
}
