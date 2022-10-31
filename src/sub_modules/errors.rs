use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigParseError {
    #[error("unexpected field: `{0}`")]
    UnexpectedField(String),
}
