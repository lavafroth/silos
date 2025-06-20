use std::sync::Mutex;

pub struct StateWrapper {
    pub inner: Mutex<State>,
}

pub struct State {
    pub embed: crate::embed::Embed,
    pub v1: crate::v1::api::State,
    pub v2: crate::v2::api::State,
}

impl State {
    pub fn build(self) -> StateWrapper {
        StateWrapper {
            inner: Mutex::new(self),
        }
    }
}
