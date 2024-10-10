/// Combines the messages of an error and its sources into a [`String`] of the form `"error: source 1: source 2: root error"`
pub fn error_full_message(err: &dyn std::error::Error) -> String {
    use std::fmt::Write;
    // Build the full hierarchy of error messages by walking up the stack until an error
    // without `source` set is encountered and concatenating all encountered error strings.
    let mut full_msg = format!("{}", err);
    let mut curr_err = err.source();
    while let Some(curr_source) = curr_err {
        write!(full_msg, ": {curr_source}").expect("string formatting should be infallible");
        curr_err = curr_source.source();
    }
    full_msg
}

#[cfg(test)]
pub(crate) mod tests {
    use super::error_full_message;

    #[test]
    fn error_messages() {
        assert_eq!(
            error_full_message(anyhow::anyhow!("standalone error").as_ref()),
            "standalone error"
        );
        assert_eq!(
            error_full_message(
                anyhow::anyhow!("root error")
                    .context("middleware")
                    .context("leaf")
                    .as_ref()
            ),
            "leaf: middleware: root error"
        );
    }
}
