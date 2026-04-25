pub enum Action<'a> {
    Generate(&'a str),
    Refactor(&'a str),
}

fn action_predicate<'a>(comment: &'a str, action: &str) -> Option<&'a str> {
    comment
        .split_once(action)
        .map(|(_discard, predicate)| predicate)
}

impl<'a> Action<'a> {
    pub(crate) fn new(comment: &'a str) -> Option<Action<'a>> {
        let upto_newline = match comment.rsplit_once("\n") {
            Some((upto_newline, _discard)) => upto_newline,
            None => comment,
        };
        use Action::*;
        let maybe_generate = action_predicate(upto_newline, "generate: ").map(Generate);
        let maybe_refactor = action_predicate(upto_newline, "refactor: ").map(Refactor);
        maybe_generate.or(maybe_refactor)
    }
}
