use serde::Deserialize;

pub mod controller;
pub mod identity;
pub mod node;

#[derive(Deserialize)]
enum LbSelector {
    #[serde(rename = "lb.stackable.tech/lb-class")]
    LbClass(String),
    #[serde(rename = "lb.stackable.tech/lb-name")]
    Lb(String),
}

#[derive(Deserialize)]
struct LbVolumeContext {
    #[serde(flatten)]
    lb_selector: LbSelector,
}
