use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// This diagram type (or construct) has no Rust renderer yet; use the Ruby path.
    Unported(String),
    /// Source is invalid for its diagram type.
    Parse { class: String, message: String, line: usize, col: usize },
}

impl Error {
    pub fn to_json(&self) -> String {
        let v = match self {
            Error::Unported(t) => serde_json::json!({"class":"Unported","message":t,"line":0,"col":0}),
            Error::Parse { class, message, line, col } => {
                serde_json::json!({"class":class,"message":message,"line":line,"col":col})
            }
        };
        v.to_string()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unported(t) => write!(f, "unported: {t}"),
            Error::Parse { class, message, .. } => write!(f, "{class}: {message}"),
        }
    }
}
impl std::error::Error for Error {}
